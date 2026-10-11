//! Damage, health and destruction. Ports of `DamageSpecifier`,
//! `DamageableSystem`, `MobThresholdSystem`/`MobStateSystem` and the core of
//! `DestructibleSystem` (Content.Shared/Damage, Content.Shared/Mobs,
//! Content.Server/Destructible).

use std::collections::{BTreeMap, HashMap};

use bevy_ecs::prelude::*;
use grimoire::{Node, PrototypeManager};
use serde::Deserialize;

use crate::content::{Content, ContentData};
use crate::events::{Popup, PopupKind, SoundAt};
use crate::spawn::ComponentFactories;
use crate::transform::Xform;

/// Static damage data derived from `damageType`, `damageGroup`,
/// `damageContainer` and `damageModifierSet` prototypes.
#[derive(Debug, Default, Clone)]
pub struct DamageTables {
    pub types: Vec<String>,
    pub groups: HashMap<String, Vec<String>>,
    pub containers: HashMap<String, Vec<String>>,
    pub modifier_sets: HashMap<String, ModifierSet>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ModifierSet {
    pub coefficients: HashMap<String, f32>,
    pub flat_reductions: HashMap<String, f32>,
}

impl DamageTables {
    pub fn from_prototypes(pm: &PrototypeManager) -> Self {
        let mut t = DamageTables::default();
        for p in pm.iter_kind("damageType") {
            t.types.push(p.id.clone());
        }
        for p in pm.iter_kind("damageGroup") {
            let members = p.get("damageTypes").map(Node::as_str_list).unwrap_or_default();
            t.groups.insert(p.id.clone(), members);
        }
        for p in pm.iter_kind("damageContainer") {
            let mut supported = p.get("supportedTypes").map(Node::as_str_list).unwrap_or_default();
            for g in p.get("supportedGroups").map(Node::as_str_list).unwrap_or_default() {
                if let Some(members) = t.groups.get(&g) {
                    supported.extend(members.iter().cloned());
                }
            }
            supported.sort();
            supported.dedup();
            t.containers.insert(p.id.clone(), supported);
        }
        for p in pm.iter_kind("damageModifierSet") {
            if let Ok(m) = p.read::<ModifierSet>() {
                t.modifier_sets.insert(p.id.clone(), m);
            }
        }
        t
    }
}

/// `DamageSpecifier` as written in YAML: `{ types: {...}, groups: {...} }`.
#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct DamageSpecifierYaml {
    pub types: BTreeMap<String, f32>,
    pub groups: BTreeMap<String, f32>,
}

/// A flattened damage amount per damage type.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Damage(pub BTreeMap<String, f32>);

impl Damage {
    pub fn from_yaml(spec: &DamageSpecifierYaml, tables: &DamageTables) -> Damage {
        let mut d = BTreeMap::new();
        for (k, v) in &spec.types {
            *d.entry(k.clone()).or_insert(0.0) += v;
        }
        for (g, v) in &spec.groups {
            // Even split across the group's types, as upstream does.
            if let Some(members) = tables.groups.get(g) {
                let mut remaining = *v;
                let mut left = members.len() as f32;
                for m in members {
                    let share = remaining / left;
                    *d.entry(m.clone()).or_insert(0.0) += share;
                    remaining -= share;
                    left -= 1.0;
                }
            }
        }
        Damage(d)
    }

    pub fn total(&self) -> f32 {
        self.0.values().sum()
    }

    pub fn scaled(&self, k: f32) -> Damage {
        Damage(self.0.iter().map(|(t, v)| (t.clone(), v * k)).collect())
    }

    /// `DamageSpecifier.ApplyModifierSet`.
    pub fn with_modifiers(&self, set: &ModifierSet) -> Damage {
        let mut out = BTreeMap::new();
        for (k, &v) in &self.0 {
            if v == 0.0 {
                continue;
            }
            if v < 0.0 {
                out.insert(k.clone(), v);
                continue;
            }
            let mut nv = v;
            if let Some(r) = set.flat_reductions.get(k) {
                nv = (nv - r).max(0.0);
            }
            if let Some(c) = set.coefficients.get(k) {
                nv *= c;
            }
            if nv != 0.0 {
                out.insert(k.clone(), nv);
            }
        }
        Damage(out)
    }
}

/// `DamageableComponent`.
#[derive(Component, Debug, Clone, Default)]
pub struct Damageable {
    pub container: Option<String>,
    pub modifier_set: Option<String>,
    pub damage: BTreeMap<String, f32>,
}

impl Damageable {
    pub fn total(&self) -> f32 {
        self.damage.values().sum()
    }

    pub fn group_total(&self, group: &str, tables: &DamageTables) -> f32 {
        tables
            .groups
            .get(group)
            .map(|m| m.iter().filter_map(|t| self.damage.get(t)).sum())
            .unwrap_or(0.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
pub enum MobStateKind {
    Invalid,
    Alive,
    Critical,
    Dead,
}

/// `MobThresholdsComponent`: total damage → state.
#[derive(Component, Debug, Clone, Default)]
pub struct MobThresholds {
    pub thresholds: Vec<(f32, MobStateKind)>,
}

impl MobThresholds {
    pub fn state_for(&self, damage: f32) -> MobStateKind {
        let mut s = MobStateKind::Alive;
        for (t, st) in &self.thresholds {
            if damage >= *t {
                s = *st;
            }
        }
        s
    }

    pub fn threshold_for(&self, state: MobStateKind) -> Option<f32> {
        self.thresholds.iter().find(|(_, s)| *s == state).map(|(t, _)| *t)
    }
}

/// `MobStateComponent`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobState(pub MobStateKind);

impl Default for MobState {
    fn default() -> Self {
        MobState(MobStateKind::Alive)
    }
}

impl MobState {
    pub fn is_alive(&self) -> bool {
        self.0 == MobStateKind::Alive
    }
    pub fn is_incapacitated(&self) -> bool {
        matches!(self.0, MobStateKind::Critical | MobStateKind::Dead)
    }
}

/// Fired when a mob's state changes (`MobStateChangedEvent`).
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct MobStateChanged {
    pub entity: Entity,
    pub old: MobStateKind,
    pub new: MobStateKind,
}

/// Fired after damage is applied (`DamageChangedEvent`).
#[derive(EntityEvent, Debug, Clone)]
pub struct DamageChanged {
    pub entity: Entity,
    pub delta: f32,
    pub origin: Option<Entity>,
}

/// One `Destructible` threshold: a trigger and what happens.
#[derive(Debug, Clone)]
pub struct DestructThreshold {
    pub damage: Option<f32>,
    pub behaviors: Vec<DestructBehavior>,
    pub triggered: bool,
    pub trigger_once: bool,
}

#[derive(Debug, Clone)]
pub enum DestructBehavior {
    Destruction,
    Breakage,
    SpawnEntities(Vec<(String, u32, u32)>),
    PlaySound(String),
    /// Behaviours not ported yet are kept by name for diagnostics.
    Other(String),
}

#[derive(Component, Debug, Clone, Default)]
pub struct Destructible {
    pub thresholds: Vec<DestructThreshold>,
}

fn parse_destructible(node: &Node) -> Destructible {
    let mut out = Destructible::default();
    for th in node.get("thresholds").and_then(Node::as_seq).unwrap_or_default() {
        let trigger = th.get("trigger");
        let damage = trigger
            .filter(|t| t.type_tag() == Some("DamageTrigger"))
            .and_then(|t| t.get("damage"))
            .and_then(Node::as_f32);
        let mut behaviors = Vec::new();
        for b in th.get("behaviors").and_then(Node::as_seq).unwrap_or_default() {
            match b.type_tag() {
                Some("DoActsBehavior") => {
                    for act in b.get("acts").map(Node::as_str_list).unwrap_or_default() {
                        behaviors.push(match act.as_str() {
                            "Destruction" => DestructBehavior::Destruction,
                            "Breakage" => DestructBehavior::Breakage,
                            other => DestructBehavior::Other(format!("act:{other}")),
                        });
                    }
                }
                Some("SpawnEntitiesBehavior") => {
                    let mut spawns = Vec::new();
                    if let Some(m) = b.get("spawn").and_then(Node::as_map) {
                        for (id, mm) in m {
                            let min = mm.get("min").and_then(Node::as_i64).unwrap_or(1).max(0) as u32;
                            let max = mm.get("max").and_then(Node::as_i64).unwrap_or(min as i64).max(min as i64) as u32;
                            spawns.push((id.clone(), min, max));
                        }
                    }
                    behaviors.push(DestructBehavior::SpawnEntities(spawns));
                }
                Some("PlaySoundBehavior") => {
                    let path = b
                        .get("sound")
                        .and_then(|s| s.get_str("path").or_else(|| s.get_str("collection")))
                        .unwrap_or("")
                        .to_string();
                    behaviors.push(DestructBehavior::PlaySound(path));
                }
                Some(other) => behaviors.push(DestructBehavior::Other(other.to_string())),
                None => {}
            }
        }
        out.thresholds.push(DestructThreshold {
            damage,
            behaviors,
            triggered: false,
            trigger_once: th.get("triggersOnce").and_then(Node::as_bool).unwrap_or(true),
        });
    }
    out
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("Damageable", |e, node, _: &ContentData| {
        e.insert(Damageable {
            container: node.get_str("damageContainer").map(str::to_owned),
            modifier_set: node.get_str("damageModifierSet").map(str::to_owned),
            damage: BTreeMap::new(),
        });
    });
    reg.register("MobThresholds", |e, node, _| {
        let mut th = Vec::new();
        if let Some(m) = node.get("thresholds").and_then(Node::as_map) {
            for (k, v) in m {
                let state = match v.as_str() {
                    Some("Alive") => MobStateKind::Alive,
                    Some("Critical") => MobStateKind::Critical,
                    Some("Dead") => MobStateKind::Dead,
                    _ => MobStateKind::Invalid,
                };
                if let Ok(t) = k.parse::<f32>() {
                    th.push((t, state));
                }
            }
        }
        th.sort_by(|a, b| a.0.total_cmp(&b.0));
        e.insert(MobThresholds { thresholds: th });
    });
    reg.register("MobState", |e, _, _| {
        e.insert(MobState::default());
    });
    reg.register("Destructible", |e, node, _| {
        e.insert(parse_destructible(node));
    });
}

/// Apply damage to an entity: modifier sets, container filtering, then
/// threshold checks. Returns the damage actually dealt.
pub fn apply_damage(world: &mut World, target: Entity, damage: &Damage, origin: Option<Entity>) -> f32 {
    change_damage(world, target, damage, origin, false)
}

/// `DamageableSystem.TryChangeDamage` with `ignoreResistances`.
pub fn change_damage(world: &mut World, target: Entity, damage: &Damage, origin: Option<Entity>, ignore_resistances: bool) -> f32 {
    let content = world.resource::<Content>().clone();
    let Some(mut dmg) = world.get_mut::<Damageable>(target) else { return 0.0 };
    let mut d = damage.clone();
    if !ignore_resistances
        && let Some(set) = dmg.modifier_set.as_ref().and_then(|s| content.damage.modifier_sets.get(s))
    {
        d = d.with_modifiers(set);
    }
    let supported = dmg.container.as_ref().and_then(|c| content.damage.containers.get(c));
    let mut dealt = 0.0;
    for (ty, v) in d.0 {
        if let Some(sup) = supported
            && !sup.contains(&ty)
        {
            continue;
        }
        let cur = dmg.damage.entry(ty).or_insert(0.0);
        let new = (*cur + v).max(0.0);
        dealt += new - *cur;
        *cur = new;
    }
    if dealt != 0.0 {
        world.trigger(DamageChanged {
            entity: target,
            delta: dealt,
            origin,
        });
        update_mob_state(world, target);
        check_destructible(world, target);
    }
    dealt
}

/// Recompute mob state from thresholds (`MobThresholdSystem`).
pub fn update_mob_state(world: &mut World, e: Entity) {
    let Some(total) = world.get::<Damageable>(e).map(Damageable::total) else { return };
    let Some(th) = world.get::<MobThresholds>(e) else { return };
    let new = th.state_for(total);
    let Some(mut st) = world.get_mut::<MobState>(e) else { return };
    if st.0 == new {
        return;
    }
    let old = st.0;
    st.0 = new;
    world.trigger(MobStateChanged { entity: e, old, new });
    if let Some(m) = world.get::<crate::movement::InputMover>(e).copied() {
        world.entity_mut(e).insert(crate::movement::InputMover {
            can_move: new == MobStateKind::Alive,
            wish: if new == MobStateKind::Alive { m.wish } else { bevy_math::Vec2::ZERO },
            ..m
        });
    }
    let name = world.get::<crate::spawn::MetaData>(e).map(|m| m.name.clone()).unwrap_or_default();
    let text = match new {
        MobStateKind::Critical => Some(format!("{name} collapses!")),
        MobStateKind::Dead => Some(format!("{name} dies.")),
        _ => None,
    };
    if let Some(text) = text {
        world.write_message(Popup {
            text,
            at: e,
            kind: PopupKind::Large,
        });
    }
}

/// Fire any newly crossed `Destructible` thresholds.
pub fn check_destructible(world: &mut World, e: Entity) {
    let Some(total) = world.get::<Damageable>(e).map(Damageable::total) else { return };
    let Some(mut des) = world.get_mut::<Destructible>(e) else { return };
    let mut fired: Vec<DestructBehavior> = Vec::new();
    for th in des.thresholds.iter_mut() {
        let Some(t) = th.damage else { continue };
        if total >= t && !(th.triggered && th.trigger_once) {
            th.triggered = true;
            fired.extend(th.behaviors.iter().cloned());
        }
    }
    if fired.is_empty() {
        return;
    }
    let x = world.get::<Xform>(e).copied();
    let mut destroy = false;
    for b in fired {
        match b {
            DestructBehavior::Destruction | DestructBehavior::Breakage => destroy = true,
            DestructBehavior::SpawnEntities(list) => {
                if let Some(x) = x
                    && let Some(parent) = x.parent
                {
                    for (proto, min, max) in list {
                        let n = crate::rng::range_u32(world, min, max);
                        for _ in 0..n {
                            if let Some(s) = crate::spawn::spawn_proto(world, &proto, parent, x.pos) {
                                // Spawned debris is loose, not anchored, unless its
                                // prototype says so (girders are anchored).
                                let _ = s;
                            }
                        }
                    }
                }
            }
            DestructBehavior::PlaySound(path) => {
                world.write_message(SoundAt { path, at: e });
            }
            DestructBehavior::Other(_) => {}
        }
    }
    if destroy {
        crate::spawn::delete(world, e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> DamageTables {
        let mut t = DamageTables::default();
        t.groups.insert("Brute".into(), vec!["Blunt".into(), "Slash".into(), "Piercing".into()]);
        t
    }

    #[test]
    fn group_damage_splits_evenly() {
        let spec = DamageSpecifierYaml {
            types: BTreeMap::from([("Heat".into(), 5.0)]),
            groups: BTreeMap::from([("Brute".into(), 9.0)]),
        };
        let d = Damage::from_yaml(&spec, &tables());
        assert_eq!(d.0["Blunt"], 3.0);
        assert_eq!(d.0["Heat"], 5.0);
        assert!((d.total() - 14.0).abs() < 1e-5);
    }

    #[test]
    fn modifiers_match_upstream() {
        let set = ModifierSet {
            coefficients: HashMap::from([("Blunt".into(), 0.5)]),
            flat_reductions: HashMap::from([("Blunt".into(), 2.0)]),
        };
        let d = Damage(BTreeMap::from([("Blunt".into(), 10.0), ("Slash".into(), 4.0)]));
        let m = d.with_modifiers(&set);
        assert_eq!(m.0["Blunt"], 4.0); // (10 - 2) * 0.5
        assert_eq!(m.0["Slash"], 4.0);
    }

    #[test]
    fn thresholds() {
        let th = MobThresholds {
            thresholds: vec![(0.0, MobStateKind::Alive), (100.0, MobStateKind::Critical), (200.0, MobStateKind::Dead)],
        };
        assert_eq!(th.state_for(50.0), MobStateKind::Alive);
        assert_eq!(th.state_for(100.0), MobStateKind::Critical);
        assert_eq!(th.state_for(250.0), MobStateKind::Dead);
    }
}
