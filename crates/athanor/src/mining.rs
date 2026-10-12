//! Mining. Ports of `Content.Server/Gatherable/GatherableSystem.cs`
//! (+ `.Projectile.cs`) and `Content.Server/Mining/MiningSystem.cs`.
//!
//! * A `Gatherable` entity (rock) is mined in **one hit** by a weapon that
//!   passes its `toolWhitelist` (`AttackedEvent`). A mob that passes the list
//!   can also mine it by interacting (`ActivateInWorldEvent`, complex). The
//!   tool's attack rate is the only limit on mining speed.
//! * Gathering plays `SoundOnGather` and destroys the entity through
//!   `DestructibleSystem.DestroyEntity`. It then spawns its `loot` tables
//!   within `gatherOffset` (0.3 m). A loot key other than "All" needs the
//!   gatherer to have that tag.
//! * An `OreVein` rolls its ore at map init: `Prob(oreChance)`, then a
//!   weighted pick from its `weightedRandomOre`. When it is destroyed by any
//!   means it drops `Next(minOreYield, maxOreYield + 1)` of the ore entity
//!   within 0.2 m, unless `preventSpawning` is set.
//! * `GatheringProjectile` bolts (PKA) gather what they hit, `amount` times.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use grimoire::Node;

use crate::events::SoundAt;
use crate::rng::SimRng;
use crate::spawn::{spawn_proto, ComponentFactories, EntityData};
use crate::transform::Xform;
use crate::whitelist::{self, Whitelist};
use crate::Content;

/// `GatherableComponent`.
#[derive(Component, Debug, Clone)]
pub struct Gatherable {
    pub tool_whitelist: Option<Whitelist>,
    /// (gatherer tag or "All", entity table selector).
    pub loot: Vec<(String, Node)>,
    pub gather_offset: f32,
}

/// `OreVeinComponent`.
#[derive(Component, Debug, Clone)]
pub struct OreVein {
    pub ore_chance: f32,
    pub ore_rarity: Option<String>,
    pub current_ore: Option<String>,
    pub prevent_spawning: bool,
    pub gather_destruction_whitelist: Option<Whitelist>,
    /// The map-init roll has happened.
    pub rolled: bool,
}

/// `GatheringProjectileComponent`.
#[derive(Component, Debug, Clone, Copy)]
pub struct GatheringProjectile {
    pub amount: i32,
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("Gatherable", |e, node, _| {
        e.insert(Gatherable {
            tool_whitelist: node.get("toolWhitelist").map(Whitelist::from_node),
            loot: node
                .get("loot")
                .and_then(Node::as_map)
                .map(|m| m.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
                .unwrap_or_default(),
            gather_offset: node.get("gatherOffset").and_then(Node::as_f32).unwrap_or(0.3),
        });
    });
    reg.register("OreVein", |e, node, _| {
        e.insert(OreVein {
            ore_chance: node.get("oreChance").and_then(Node::as_f32).unwrap_or(0.1),
            ore_rarity: node.get_str("oreRarityPrototypeId").map(str::to_owned),
            current_ore: node.get_str("currentOre").map(str::to_owned),
            prevent_spawning: node.get("preventSpawning").and_then(Node::as_bool).unwrap_or(false),
            gather_destruction_whitelist: node.get("gatherDestructionWhitelist").map(Whitelist::from_node),
            rolled: false,
        });
    });
    reg.register("GatheringProjectile", |e, node, _| {
        e.insert(GatheringProjectile { amount: node.get("amount").and_then(Node::as_i64).unwrap_or(1) as i32 });
    });
}

/// `SharedRandomExtensions.Pick(IWeightedRandomPrototype)`: weights in file order.
fn pick_weighted(weights: &[(String, f32)], rng: &mut SimRng) -> Option<String> {
    let sum: f32 = weights.iter().map(|(_, w)| *w).sum();
    let roll = rng.next_f32() * sum;
    let mut acc = 0.0;
    for (k, w) in weights {
        acc += w;
        if acc >= roll {
            return Some(k.clone());
        }
    }
    None
}

/// `MiningSystem.OnMapInit` for veins spawned since the last tick.
pub fn roll_ore_veins(world: &mut World) {
    let pending: Vec<Entity> = {
        let mut q = world.query::<(Entity, &OreVein)>();
        q.iter(world).filter(|(_, v)| !v.rolled).map(|(e, _)| e).collect()
    };
    if pending.is_empty() {
        return;
    }
    let content = world.resource::<Content>().clone();
    for e in pending {
        let v = world.get::<OreVein>(e).cloned().unwrap();
        let mut ore = v.current_ore.clone();
        if ore.is_none()
            && let Some(rarity) = &v.ore_rarity
            && world.get_resource_or_init::<SimRng>().chance(v.ore_chance)
        {
            let weights: Vec<(String, f32)> = content
                .protos
                .get("weightedRandomOre", rarity)
                .and_then(|p| p.get("weights"))
                .and_then(Node::as_map)
                .map(|m| m.iter().filter_map(|(k, w)| Some((k.to_string(), w.as_f32()?))).collect())
                .unwrap_or_default();
            ore = pick_weighted(&weights, &mut world.resource_mut::<SimRng>());
        }
        if let Some(mut v) = world.get_mut::<OreVein>(e) {
            v.current_ore = ore;
            v.rolled = true;
        }
    }
}

/// `NextVector2(max)`: random direction, magnitude in [0, max).
fn scatter(rng: &mut SimRng, max: f32) -> Vec2 {
    let angle = rng.next_f32() * std::f32::consts::TAU;
    let mag = rng.next_f32() * max;
    Vec2::new(angle.cos(), angle.sin()) * mag
}

/// `MiningSystem.OnDestruction`: called by `damage::destroy_entity`.
pub fn on_destruction(world: &mut World, e: Entity) {
    let Some(v) = world.get::<OreVein>(e).cloned() else { return };
    if v.prevent_spawning {
        return;
    }
    let Some(ore) = v.current_ore else { return };
    let content = world.resource::<Content>().clone();
    let Some(proto) = content.protos.get("ore", &ore) else { return };
    let Some(entity) = proto.get_str("oreEntity").map(str::to_owned) else { return };
    let min = proto.get("minOreYield").and_then(Node::as_i64).unwrap_or(1) as i32;
    let max = proto.get("maxOreYield").and_then(Node::as_i64).unwrap_or(1) as i32;
    let Some(x) = world.get::<Xform>(e).copied() else { return };
    let Some(parent) = x.parent else { return };
    let n = world.get_resource_or_init::<SimRng>().next_range(min, max + 1);
    for _ in 0..n {
        let off = scatter(&mut world.resource_mut::<SimRng>(), 0.2);
        if let Some(s) = spawn_proto(world, &entity, parent, x.pos + off)
            && let Some(mut sx) = world.get_mut::<Xform>(s)
        {
            sx.anchored = false;
        }
    }
}

/// `GatherableSystem.Gather`.
pub fn gather(world: &mut World, target: Entity, gatherer: Option<Entity>) {
    let Some(g) = world.get::<Gatherable>(target).cloned() else { return };
    if let Some(sound) = world.get::<EntityData>(target).and_then(|d| d.get("SoundOnGather")).map(|s| {
        s.get("sound").and_then(|n| n.get_str("path").or_else(|| n.get_str("collection"))).unwrap_or("/Audio/Effects/break_stone.ogg").to_string()
    }) {
        world.write_message(SoundAt { path: sound, at: target });
    }
    let at = world.get::<Xform>(target).copied();
    crate::damage::destroy_entity(world, target);
    let Some(x) = at else { return };
    let Some(parent) = x.parent else { return };
    let content = world.resource::<Content>().clone();
    for (tag, table) in &g.loot {
        if tag != "All" && !gatherer.is_some_and(|u| world.get_entity(u).is_ok() && whitelist::has_tag(world, u, tag)) {
            continue;
        }
        let mut spawns = Vec::new();
        crate::storage::table_spawns(&content, table, &mut world.resource_mut::<SimRng>(), &mut spawns, 0);
        for proto in spawns {
            let off = scatter(&mut world.resource_mut::<SimRng>(), g.gather_offset);
            spawn_proto(world, &proto, parent, x.pos + off);
        }
    }
}

/// `GatherableSystem.OnAttacked`: a melee hit with `used`.
/// Returns true if the target was mined.
pub fn on_attacked(world: &mut World, target: Entity, user: Entity, used: Entity) -> bool {
    let Some(g) = world.get::<Gatherable>(target) else { return false };
    if whitelist::fail_or_null(g.tool_whitelist.as_ref(), world, used) {
        return false;
    }
    gather(world, target, Some(user));
    true
}

/// `GatherableSystem.OnActivate` (complex interaction by the user itself).
pub fn on_activate(world: &mut World, user: Entity, target: Entity) -> bool {
    let Some(g) = world.get::<Gatherable>(target) else { return false };
    if whitelist::fail_or_null(g.tool_whitelist.as_ref(), world, user) {
        return false;
    }
    gather(world, target, Some(user));
    true
}

/// `GatherableSystem.OnProjectileCollide`. Returns true if the bolt is spent.
pub fn on_projectile_hit(world: &mut World, bolt: Entity, target: Entity) -> bool {
    let Some(gp) = world.get::<GatheringProjectile>(bolt).copied() else { return false };
    if gp.amount <= 0 {
        return false;
    }
    let Some(g) = world.get::<Gatherable>(target).cloned() else { return false };
    if whitelist::fail(g.tool_whitelist.as_ref(), world, bolt) {
        return true;
    }
    let destroys_ore = world
        .get::<OreVein>(target)
        .is_some_and(|v| whitelist::pass(v.gather_destruction_whitelist.as_ref(), world, bolt));
    if destroys_ore && let Some(mut v) = world.get_mut::<OreVein>(target) {
        v.prevent_spawning = true;
    }
    gather(world, target, Some(bolt));
    let left = gp.amount - 1;
    if let Some(mut p) = world.get_mut::<GatheringProjectile>(bolt) {
        p.amount = left;
    }
    left <= 0
}

pub fn install(world: &mut World) {
    let mut h = world.get_resource_or_init::<crate::interaction::InteractionHandlers>();
    h.on_interact_hand.push(|world, user, target| on_activate(world, user, target));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_pick_follows_file_order() {
        let w = vec![("A".to_string(), 1.0), ("B".to_string(), 0.0), ("C".to_string(), 3.0)];
        let mut rng = SimRng(5);
        let mut counts = [0; 3];
        for _ in 0..4000 {
            match pick_weighted(&w, &mut rng).as_deref() {
                Some("A") => counts[0] += 1,
                Some("B") => counts[1] += 1,
                _ => counts[2] += 1,
            }
        }
        assert_eq!(counts[1], 0);
        assert!((counts[0] as f32 / 4000.0 - 0.25).abs() < 0.03, "{counts:?}");
    }
}
