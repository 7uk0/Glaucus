//! Breathing and pressure. Ports of `RespiratorSystem`, the lung half of
//! metabolism it drives (`LungSystem.GasToReagent`, the `Gas` metabolism
//! group's `Oxygenate` / `ModifyLungGas` / `HealthChange` effects, gated on
//! `OrganType`), and `BarotraumaSystem` with `PressureProtection`.
//!
//! What a body can breathe is read from content, not hard-coded: a mob's
//! `Body` prototype names its lungs, the lungs' `Metabolizer` names their
//! organ types (Human, Vox, ...), and each gas's reagent says which organ
//! types it oxygenates or poisons.
//!
//! Simplification: upstream metabolizes the lung solution on the
//! metabolizer's own clock (rate × 100 for lungs, so a breath is gone within
//! a second); here a breath is metabolized the moment it is inhaled, with
//! `MetabolizerSystem`'s quantity and scale (`removed / rate`).

use std::collections::HashMap;
use std::sync::Arc;

use athanor::damage::{change_damage, Damage, DamageSpecifierYaml, Damageable, MobState, MobStateKind};
use athanor::events::{Popup, PopupKind};
use athanor::inventory::Inventory;
use athanor::spawn::{ComponentFactories, EntityData, MetaData};
use athanor::{Content, SimClock};
use bevy_ecs::prelude::*;
use grimoire::{Node, PrototypeManager};
use serde::Deserialize;

use crate::atmos::{containing_tile, GridAtmos};
use crate::gas::{consts::*, gas_index, GasMixture, NUM_GASES};
use crate::Gases;

/// `Atmospherics.BreathMolesToReagentMultiplier`.
pub const BREATH_MOLES_TO_REAGENT: f32 = 1144.0;
/// `LungComponent.Air` volume.
const LUNG_VOLUME: f32 = 6.0;
const GASP_COOLDOWN: f64 = 8.0;

/// An `OrganType` condition.
#[derive(Debug, Clone)]
struct OrganCond {
    organ: String,
    should_have: bool,
}

/// `None` if the effect has a condition we don't evaluate (it never fires).
fn parse_conds(effect: &Node) -> Option<Vec<OrganCond>> {
    let mut out = Vec::new();
    for c in effect.get("conditions").and_then(Node::as_seq).unwrap_or_default() {
        if c.type_tag() != Some("OrganType") {
            return None;
        }
        out.push(OrganCond {
            organ: c.get_str("type")?.to_string(),
            should_have: c.get("shouldHave").and_then(Node::as_bool).unwrap_or(true),
        });
    }
    Some(out)
}

fn conds_pass(conds: &[OrganCond], organ_types: &[String]) -> bool {
    conds.iter().all(|c| organ_types.contains(&c.organ) == c.should_have)
}

#[derive(Debug, Clone)]
enum LungEffect {
    Oxygenate { factor: f32, conds: Vec<OrganCond> },
    ModifyLungGas { ratios: Vec<(usize, f32)>, conds: Vec<OrganCond> },
    HealthChange { damage: DamageSpecifierYaml, scale_by_quantity: bool, conds: Vec<OrganCond> },
}

/// A reagent's `Gas` metabolism entry.
#[derive(Debug, Clone)]
struct GasMetabolism {
    /// `ReagentEffectsEntry.MetabolismRate`.
    rate: f32,
    effects: Vec<LungEffect>,
}

/// A body's lungs: their `metabolizerTypes` and `Gas` group rate modifier.
#[derive(Debug, Clone)]
pub struct LungSpec {
    pub types: Vec<String>,
    pub rate_modifier: f32,
}

/// What each gas does in lungs, and which organ types each body's lungs are.
#[derive(Debug, Default)]
pub struct LungData {
    gas_reagent: [Option<String>; NUM_GASES],
    metabolisms: HashMap<String, GasMetabolism>,
    /// `body` prototype → its lungs. Absent: no lungs.
    bodies: HashMap<String, LungSpec>,
}

#[derive(Resource, Clone, Default)]
pub struct Lungs(pub Arc<LungData>);

impl LungData {
    pub fn from_prototypes(pm: &PrototypeManager) -> LungData {
        let mut d = LungData::default();
        for p in pm.iter_kind("gas") {
            if let (Ok(i), Some(r)) = (p.id.parse::<usize>(), p.get_str("reagent"))
                && i < NUM_GASES
            {
                d.gas_reagent[i] = Some(r.to_string());
            }
        }
        for r in d.gas_reagent.iter().flatten() {
            let Some(p) = pm.get("reagent", r) else { continue };
            let Some(entry) = p.get("metabolisms").and_then(|m| m.get("Gas")) else { continue };
            let effects = entry.get("effects").and_then(Node::as_seq).unwrap_or_default();
            let mut list = Vec::new();
            for e in effects {
                let Some(conds) = parse_conds(e) else { continue };
                match e.type_tag() {
                    Some("Oxygenate") => list.push(LungEffect::Oxygenate {
                        factor: e.get("factor").and_then(Node::as_f32).unwrap_or(1.0),
                        conds,
                    }),
                    Some("ModifyLungGas") => {
                        let ratios = e
                            .get("ratios")
                            .and_then(Node::as_map)
                            .map(|m| m.iter().filter_map(|(k, v)| Some((gas_index(k)?, v.as_f32()?))).collect())
                            .unwrap_or_default();
                        list.push(LungEffect::ModifyLungGas { ratios, conds });
                    }
                    Some("HealthChange") => list.push(LungEffect::HealthChange {
                        damage: e.get("damage").and_then(|n| grimoire::from_node(n).ok()).unwrap_or_default(),
                        scale_by_quantity: e.get("scaleByQuantity").and_then(Node::as_bool).unwrap_or(false),
                        conds,
                    }),
                    _ => {}
                }
            }
            let rate = entry.get("metabolismRate").and_then(Node::as_f32).unwrap_or(0.5);
            d.metabolisms.insert(r.clone(), GasMetabolism { rate, effects: list });
        }
        for b in pm.iter_kind("body") {
            let mut types = Vec::new();
            let mut rate_modifier = 1.0;
            let mut has_lungs = false;
            for (_, slot) in b.get("slots").and_then(Node::as_map).into_iter().flatten() {
                for (_, organ) in slot.get("organs").and_then(Node::as_map).into_iter().flatten() {
                    let Some(op) = organ.as_str().and_then(|o| pm.entity(o)) else { continue };
                    if !op.has_component("Lung") {
                        continue;
                    }
                    has_lungs = true;
                    if let Some(m) = op.component("Metabolizer") {
                        types.extend(m.get("metabolizerTypes").map(Node::as_str_list).unwrap_or_default());
                        for g in m.get("groups").and_then(Node::as_seq).unwrap_or_default() {
                            if g.get_str("id") == Some("Gas") {
                                rate_modifier = g.get("rateModifier").and_then(Node::as_f32).unwrap_or(1.0);
                            }
                        }
                    }
                }
            }
            if has_lungs {
                d.bodies.insert(b.id.to_string(), LungSpec { types, rate_modifier });
            }
        }
        d
    }

    pub fn lungs(&self, body: &str) -> Option<&LungSpec> {
        self.bodies.get(body)
    }

    pub fn lung_types(&self, body: &str) -> Option<&[String]> {
        self.bodies.get(body).map(|l| l.types.as_slice())
    }
}

/// `RespiratorComponent`, with the lungs' air folded in.
#[derive(Component, Debug, Clone)]
pub struct Respirator {
    pub breath_volume: f32,
    pub update_interval: f32,
    pub saturation: f32,
    pub suffocation_threshold: f32,
    pub max_saturation: f32,
    pub min_saturation: f32,
    pub damage: Damage,
    pub damage_recovery: Damage,
    pub next_update: Option<f64>,
    pub inhaling: bool,
    pub suffocation_cycles: u32,
    pub last_gasp: f64,
    /// `LungComponent.Air`.
    pub lung_air: GasMixture,
}

impl Respirator {
    /// Above the suffocation threshold.
    pub fn is_breathing(&self) -> bool {
        self.saturation > self.suffocation_threshold
    }

    fn update_saturation(&mut self, amount: f32) {
        self.saturation = (self.saturation + amount).clamp(self.min_saturation, self.max_saturation);
    }
}

fn damage_field(node: &Node, key: &str, content: &athanor::content::ContentData) -> Damage {
    let spec: DamageSpecifierYaml = node.get(key).and_then(|n| grimoire::from_node(n).ok()).unwrap_or_default();
    Damage::from_yaml(&spec, &content.damage)
}

/// `BarotraumaComponent`.
#[derive(Component, Debug, Clone)]
pub struct Barotrauma {
    pub damage: Damage,
    pub max_damage: f32,
    pub protection_slots: Vec<String>,
    pub taking_damage: bool,
    /// Last felt pressure, for the HUD's pressure alert.
    pub felt_pressure: f32,
}

/// `PressureProtectionComponent`.
#[derive(Component, Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PressureProtection {
    pub high_pressure_multiplier: f32,
    pub high_pressure_modifier: f32,
    pub low_pressure_multiplier: f32,
    pub low_pressure_modifier: f32,
}

impl Default for PressureProtection {
    fn default() -> Self {
        PressureProtection {
            high_pressure_multiplier: 1.0,
            high_pressure_modifier: 0.0,
            low_pressure_multiplier: 1.0,
            low_pressure_modifier: 0.0,
        }
    }
}

/// `PressureImmunityComponent`.
#[derive(Component, Debug, Clone, Default)]
pub struct PressureImmunity;

pub fn register(reg: &mut ComponentFactories) {
    reg.register("Respirator", |e, node, content| {
        e.insert(Respirator {
            breath_volume: node.get("breathVolume").and_then(Node::as_f32).unwrap_or(BREATH_VOLUME),
            update_interval: node.get("updateInterval").and_then(Node::as_seconds).unwrap_or(2.0) as f32,
            saturation: node.get("saturation").and_then(Node::as_f32).unwrap_or(5.0),
            suffocation_threshold: node.get("suffocationThreshold").and_then(Node::as_f32).unwrap_or(0.0),
            max_saturation: node.get("maxSaturation").and_then(Node::as_f32).unwrap_or(5.0),
            min_saturation: node.get("minSaturation").and_then(Node::as_f32).unwrap_or(-2.0),
            damage: damage_field(node, "damage", content),
            damage_recovery: damage_field(node, "damageRecovery", content),
            next_update: None,
            inhaling: true,
            suffocation_cycles: 0,
            last_gasp: f64::MIN,
            lung_air: GasMixture {
                volume: LUNG_VOLUME,
                ..GasMixture::empty()
            },
        });
    });
    reg.register("Barotrauma", |e, node, content| {
        e.insert(Barotrauma {
            damage: damage_field(node, "damage", content),
            max_damage: node.get("maxDamage").and_then(Node::as_f32).unwrap_or(200.0),
            protection_slots: node
                .get("protectionSlots")
                .map(Node::as_str_list)
                .unwrap_or_else(|| vec!["head".into(), "outerClothing".into()]),
            taking_damage: false,
            felt_pressure: ONE_ATMOSPHERE,
        });
    });
    athanor::data_component!(reg, "PressureProtection", PressureProtection);
    reg.register("PressureImmunity", |e, _, _| {
        e.insert(PressureImmunity);
    });
}

fn body_of(world: &World, e: Entity) -> Option<String> {
    world.get::<EntityData>(e)?.get("Body")?.get_str("prototype").map(str::to_string)
}

/// `RespiratorSystem.Update`: one inhale or exhale per interval.
pub fn respirator_system(world: &mut World) {
    let now = world.resource::<SimClock>().time;
    let lungs = world.resource::<Lungs>().0.clone();
    let ents: Vec<Entity> = {
        let mut q = world.query_filtered::<Entity, With<Respirator>>();
        q.iter(world).collect()
    };
    for e in ents {
        let mut r = world.get::<Respirator>(e).unwrap().clone();
        let next = *r.next_update.get_or_insert(now + r.update_interval as f64);
        if now < next {
            world.get_mut::<Respirator>(e).unwrap().next_update = r.next_update;
            continue;
        }
        r.next_update = Some(next + r.update_interval as f64);
        let state = world.get::<MobState>(e).map_or(MobStateKind::Alive, |m| m.0);
        if state == MobStateKind::Dead {
            *world.get_mut::<Respirator>(e).unwrap() = r;
            continue;
        }
        r.update_saturation(-r.update_interval);
        let mut toxins = Damage::default();
        if state == MobStateKind::Alive {
            if r.inhaling {
                inhale(world, e, &mut r, &lungs, &mut toxins);
            } else {
                exhale(world, e, &mut r);
            }
            r.inhaling = !r.inhaling;
        }
        let suffocating = r.saturation < r.suffocation_threshold;
        let mut gasp = false;
        if suffocating {
            if now >= r.last_gasp + GASP_COOLDOWN {
                r.last_gasp = now;
                gasp = true;
            }
            r.suffocation_cycles += 1;
        } else {
            r.suffocation_cycles = 0;
        }
        let (damage, recovery) = (r.damage.clone(), r.damage_recovery.clone());
        *world.get_mut::<Respirator>(e).unwrap() = r;
        if !toxins.0.is_empty() {
            change_damage(world, e, &toxins, None, true);
        }
        if suffocating {
            if gasp {
                let content = world.resource::<Content>().clone();
                let name = world.get::<MetaData>(e).map(|m| m.name.clone()).unwrap_or_default();
                world.write_message(Popup {
                    text: format!("{name} {}", content.localize("chat-emote-msg-gasp")),
                    at: e,
                    kind: PopupKind::Small,
                });
            }
            change_damage(world, e, &damage, None, false);
        } else {
            change_damage(world, e, &recovery, None, false);
        }
    }
}

/// `RespiratorSystem.Inhale` + `TryInhaleGasToBody` + lung metabolism.
fn inhale(world: &mut World, e: Entity, r: &mut Respirator, lungs: &LungData, toxins: &mut Damage) {
    let table = world.resource::<Gases>().0.clone();
    let Some((g, t)) = containing_tile(world, e) else { return };
    let spec = body_of(world, e).and_then(|b| lungs.lungs(&b).cloned());
    let content = world.resource::<Content>().clone();
    let mut ga = world.get_mut::<GridAtmos>(g).unwrap();
    let Some(mix) = ga.tiles.get_mut(&t) else { return };
    if mix.volume <= 0.0 {
        return;
    }
    let breath = mix.remove_ratio(r.breath_volume / mix.volume);
    let Some(spec) = spec else {
        // Nothing could inhale it: give it back.
        mix.merge(&breath, &table);
        return;
    };
    ga.active.insert(t);
    r.lung_air.merge(&breath, &table);

    // `GasToReagent` then metabolize the lung solution.
    let snapshot = r.lung_air.moles;
    for (i, &moles) in snapshot.iter().enumerate() {
        if moles <= 0.0 {
            continue;
        }
        let Some(m) = lungs.gas_reagent[i].as_ref().and_then(|re| lungs.metabolisms.get(re)) else { continue };
        let types = &spec.types;
        // `MetabolizerSystem`: remove up to `rate`, effects scaled by the fraction of it.
        let rate = m.rate * spec.rate_modifier;
        if rate <= 0.0 {
            continue;
        }
        let quantity = (moles * BREATH_MOLES_TO_REAGENT).min(rate);
        let scale = quantity / rate;
        for effect in &m.effects {
            match effect {
                LungEffect::Oxygenate { factor, conds } if conds_pass(conds, types) => {
                    r.update_saturation(quantity * factor);
                }
                LungEffect::ModifyLungGas { ratios, conds } if conds_pass(conds, types) => {
                    for &(gas, ratio) in ratios {
                        let mut q = ratio * quantity / BREATH_MOLES_TO_REAGENT;
                        if q < 0.0 {
                            q = q.max(-r.lung_air.moles[gas]);
                        }
                        r.lung_air.moles[gas] += q;
                    }
                }
                LungEffect::HealthChange { damage, scale_by_quantity, conds } if conds_pass(conds, types) => {
                    let d = Damage::from_yaml(damage, &content.damage).scaled(if *scale_by_quantity { quantity * scale } else { scale });
                    for (k, v) in d.0 {
                        *toxins.0.entry(k).or_insert(0.0) += v;
                    }
                }
                _ => {}
            }
        }
    }
}

/// `RespiratorSystem.Exhale` + `RemoveGasFromBody`: the lungs empty into the
/// tile (or into space).
fn exhale(world: &mut World, e: Entity, r: &mut Respirator) {
    let table = world.resource::<Gases>().0.clone();
    let out = std::mem::replace(
        &mut r.lung_air,
        GasMixture {
            volume: LUNG_VOLUME,
            ..GasMixture::empty()
        },
    );
    let Some((g, t)) = containing_tile(world, e) else { return };
    let mut ga = world.get_mut::<GridAtmos>(g).unwrap();
    if let Some(mix) = ga.tiles.get_mut(&t) {
        mix.merge(&out, &table);
        ga.active.insert(t);
    }
}

/// `BarotraumaSystem.UpdateCachedResistances`: (high mod, high mul, low mod, low mul).
fn protection(world: &World, e: Entity, bt: &Barotrauma) -> (f32, f32, f32, f32) {
    let (mut hmod, mut hmul, mut lmod, mut lmul) = (0.0, 1.0, 0.0, 1.0);
    if !bt.protection_slots.is_empty()
        && let Some(inv) = world.get::<Inventory>(e)
    {
        (hmod, hmul, lmod, lmul) = (f32::MIN, f32::MIN, f32::MAX, f32::MAX);
        for slot in &bt.protection_slots {
            let Some(p) = inv.get(slot).and_then(|i| world.get::<PressureProtection>(i)) else {
                // Missing protection: skin is exposed.
                (hmod, hmul, lmod, lmul) = (0.0, 1.0, 0.0, 1.0);
                break;
            };
            // As protected as the weakest part.
            hmod = f32::max(hmod, p.high_pressure_modifier);
            hmul = f32::max(hmul, p.high_pressure_multiplier);
            lmod = f32::min(lmod, p.low_pressure_modifier);
            lmul = f32::min(lmul, p.low_pressure_multiplier);
        }
    }
    if let Some(p) = world.get::<PressureProtection>(e) {
        hmod += p.high_pressure_modifier;
        hmul *= p.high_pressure_multiplier;
        lmod += p.low_pressure_modifier;
        lmul *= p.low_pressure_multiplier;
    }
    (hmod, hmul, lmod, lmul)
}

/// The pressure an entity actually feels after its suit and helmet.
pub fn felt_pressure(world: &World, e: Entity, bt: &Barotrauma, environment: f32) -> f32 {
    let pressure = environment.max(1.0);
    if pressure > WARNING_LOW_PRESSURE && pressure < WARNING_HIGH_PRESSURE {
        return pressure;
    }
    if world.get::<PressureImmunity>(e).is_some() {
        return ONE_ATMOSPHERE;
    }
    let (hmod, hmul, lmod, lmul) = protection(world, e, bt);
    if pressure <= WARNING_LOW_PRESSURE {
        ((pressure + lmod) * lmul).min(ONE_ATMOSPHERE)
    } else {
        ((pressure + hmod) * hmul).max(ONE_ATMOSPHERE)
    }
}

#[derive(Resource, Default)]
pub struct BarotraumaTimer(pub f32);

/// `BarotraumaSystem.Update`, once a second.
pub fn barotrauma_system(world: &mut World) {
    let dt = world.resource::<SimClock>().dt;
    {
        let mut timer = world.resource_mut::<BarotraumaTimer>();
        timer.0 += dt;
        if timer.0 < 1.0 {
            return;
        }
        timer.0 -= 1.0;
    }
    let ents: Vec<Entity> = {
        let mut q = world.query_filtered::<Entity, (With<Barotrauma>, With<Damageable>)>();
        q.iter(world).collect()
    };
    for e in ents {
        let mut bt = world.get::<Barotrauma>(e).unwrap().clone();
        let current: f32 = {
            let d = world.get::<Damageable>(e).unwrap();
            bt.damage.0.keys().filter_map(|k| d.damage.get(k)).sum()
        };
        if current >= bt.max_damage {
            continue;
        }
        let env = crate::atmos::air_at(world, e).pressure();
        let pressure = felt_pressure(world, e, &bt, env);
        bt.felt_pressure = pressure;
        let dealt = if pressure <= HAZARD_LOW_PRESSURE {
            Some(bt.damage.scaled(LOW_PRESSURE_DAMAGE))
        } else if pressure >= HAZARD_HIGH_PRESSURE {
            let scale = ((pressure / HAZARD_HIGH_PRESSURE - 1.0) * PRESSURE_DAMAGE_COEFFICIENT).min(MAX_HIGH_PRESSURE_DAMAGE);
            Some(bt.damage.scaled(scale))
        } else {
            None
        };
        bt.taking_damage = dealt.is_some();
        *world.get_mut::<Barotrauma>(e).unwrap() = bt;
        if let Some(d) = dealt {
            change_damage(world, e, &d, None, true);
        }
    }
}
