//! Map-init spawners: `ConditionalSpawner` and `RandomSpawner` markers that
//! replace themselves with real entities when a map loads. Port of
//! `ConditionalSpawnerSystem`.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use grimoire::Node;

use crate::rng::SimRng;
use crate::spawn::{delete, spawn_proto, ComponentFactories};
use crate::transform::Xform;

#[derive(Component, Debug, Clone, Default)]
pub struct ConditionalSpawner {
    pub prototypes: Vec<String>,
    pub game_rules: Vec<String>,
    pub chance: f32,
}

#[derive(Component, Debug, Clone, Default)]
pub struct RandomSpawner {
    pub prototypes: Vec<String>,
    pub rare_prototypes: Vec<String>,
    pub rare_chance: f32,
    pub offset: f32,
    pub chance: f32,
    pub delete_after: bool,
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("ConditionalSpawner", |e, node, _| {
        e.insert(ConditionalSpawner {
            prototypes: node.get("prototypes").map(Node::as_str_list).unwrap_or_default(),
            game_rules: node.get("gameRules").map(Node::as_str_list).unwrap_or_default(),
            chance: node.get("chance").and_then(Node::as_f32).unwrap_or(1.0),
        });
    });
    reg.register("RandomSpawner", |e, node, _| {
        e.insert(RandomSpawner {
            prototypes: node.get("prototypes").map(Node::as_str_list).unwrap_or_default(),
            rare_prototypes: node.get("rarePrototypes").map(Node::as_str_list).unwrap_or_default(),
            rare_chance: node.get("rareChance").and_then(Node::as_f32).unwrap_or(0.05),
            offset: node.get("offset").and_then(Node::as_f32).unwrap_or(0.2),
            chance: node.get("chance").and_then(Node::as_f32).unwrap_or(1.0),
            delete_after: node.get("deleteSpawnerAfterSpawn").and_then(Node::as_bool).unwrap_or(true),
        });
    });
}

/// Resolve every pending spawner in the world. Call after a map load.
/// Returns how many entities were spawned.
pub fn run_spawners(world: &mut World) -> usize {
    let mut spawned = 0;
    let cond: Vec<(Entity, ConditionalSpawner, Xform)> = {
        let mut q = world.query::<(Entity, &ConditionalSpawner, &Xform)>();
        q.iter(world).map(|(e, c, x)| (e, c.clone(), *x)).collect()
    };
    for (e, c, x) in cond {
        // Spawners tied to game rules only fire when that rule runs.
        if c.game_rules.is_empty() {
            let mut rng = world.get_resource_or_init::<SimRng>().clone();
            if rng.chance(c.chance)
                && let (Some(p), Some(parent)) = (rng.pick(&c.prototypes).cloned(), x.parent)
                && spawn_proto(world, &p, parent, x.pos).is_some()
            {
                spawned += 1;
            }
            *world.resource_mut::<SimRng>() = rng;
        }
        delete(world, e);
    }
    let rand: Vec<(Entity, RandomSpawner, Xform)> = {
        let mut q = world.query::<(Entity, &RandomSpawner, &Xform)>();
        q.iter(world).map(|(e, c, x)| (e, c.clone(), *x)).collect()
    };
    for (e, r, x) in rand {
        let mut rng = world.get_resource_or_init::<SimRng>().clone();
        let pick = if !r.rare_prototypes.is_empty() && rng.chance(r.rare_chance) {
            rng.pick(&r.rare_prototypes).cloned()
        } else if rng.chance(r.chance) {
            rng.pick(&r.prototypes).cloned()
        } else {
            None
        };
        let off = Vec2::new(rng.range_f32(-r.offset, r.offset), rng.range_f32(-r.offset, r.offset));
        *world.resource_mut::<SimRng>() = rng;
        if let (Some(p), Some(parent)) = (pick, x.parent)
            && spawn_proto(world, &p, parent, x.pos + off).is_some()
        {
            spawned += 1;
        }
        if r.delete_after {
            delete(world, e);
        }
    }
    spawned
}
