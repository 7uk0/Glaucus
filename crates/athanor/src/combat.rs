//! Melee combat. Port of the core of `SharedMeleeWeaponSystem`: attack rate,
//! range, damage specifier, unarmed attacks with the mob's own weapon.

use bevy_ecs::prelude::*;
use serde::Deserialize;

use crate::content::Content;
use crate::damage::{apply_damage, Damage, DamageSpecifierYaml, MobState};
use crate::events::{MeleeSwing, SoundAt};
use crate::interaction::{in_range, Hands};
use crate::spawn::ComponentFactories;
use crate::{data_component, SimClock};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SoundSpec {
    pub path: Option<String>,
    pub collection: Option<String>,
}

impl SoundSpec {
    pub fn id(&self) -> Option<String> {
        self.path.clone().or_else(|| self.collection.clone())
    }
}

/// `MeleeWeaponComponent`.
#[derive(Component, Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MeleeWeapon {
    pub damage: DamageSpecifierYaml,
    /// Attacks per second.
    pub attack_rate: f32,
    pub range: f32,
    pub angle: f32,
    pub animation: Option<String>,
    pub sound_hit: Option<SoundSpec>,
    pub sound_swing: Option<SoundSpec>,
    #[serde(skip)]
    pub next_attack: f64,
}

impl Default for MeleeWeapon {
    fn default() -> Self {
        MeleeWeapon {
            damage: DamageSpecifierYaml::default(),
            attack_rate: 1.0,
            range: 1.5,
            angle: 60.0,
            animation: None,
            sound_hit: None,
            sound_swing: None,
            next_attack: 0.0,
        }
    }
}

/// `CombatModeComponent`.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct CombatMode {
    pub active: bool,
}

pub fn register(reg: &mut ComponentFactories) {
    data_component!(reg, "MeleeWeapon", MeleeWeapon);
    reg.register("CombatMode", |e, _, _| {
        e.insert(CombatMode::default());
    });
}

/// The weapon an attacker would swing: the held item's, else its own.
pub fn weapon_of(world: &World, attacker: Entity) -> Option<Entity> {
    if let Some(item) = world.get::<Hands>(attacker).and_then(Hands::active_item)
        && world.get::<MeleeWeapon>(item).is_some()
    {
        return Some(item);
    }
    world.get::<MeleeWeapon>(attacker).map(|_| attacker)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackResult {
    Hit,
    Missed,
    OnCooldown,
    NoWeapon,
    OutOfRange,
    Incapacitated,
}

/// Swing at `target` with whatever the attacker wields.
pub fn try_melee(world: &mut World, attacker: Entity, target: Entity) -> AttackResult {
    if world.get::<MobState>(attacker).is_some_and(MobState::is_incapacitated) {
        return AttackResult::Incapacitated;
    }
    let Some(weapon_e) = weapon_of(world, attacker) else {
        return AttackResult::NoWeapon;
    };
    let now = world.resource::<SimClock>().time;
    let w = world.get::<MeleeWeapon>(weapon_e).unwrap().clone();
    if now < w.next_attack {
        return AttackResult::OnCooldown;
    }
    world.get_mut::<MeleeWeapon>(weapon_e).unwrap().next_attack = now + 1.0 / w.attack_rate.max(0.01) as f64;

    if !in_range(world, attacker, target, w.range) {
        world.write_message(MeleeSwing {
            attacker,
            target: None,
            hit: false,
            animation: w.animation.clone(),
        });
        return AttackResult::OutOfRange;
    }
    // `AttackedEvent`: a mining tool gathers the rock outright.
    if crate::mining::on_attacked(world, target, attacker, weapon_e) {
        world.write_message(MeleeSwing {
            attacker,
            target: None,
            hit: true,
            animation: w.animation.clone(),
        });
        return AttackResult::Hit;
    }
    let content = world.resource::<Content>().clone();
    let dmg = Damage::from_yaml(&w.damage, &content.damage);
    apply_damage(world, target, &dmg, Some(attacker));
    world.write_message(MeleeSwing {
        attacker,
        target: Some(target),
        hit: true,
        animation: w.animation.clone(),
    });
    if let Some(s) = w.sound_hit.as_ref().and_then(SoundSpec::id) {
        world.write_message(SoundAt { path: s, at: target });
    }
    AttackResult::Hit
}
