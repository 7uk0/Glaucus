//! Mining on real content. Passes trivially without upstream.

use std::sync::OnceLock;

use athanor::combat::{try_melee, AttackResult};
use athanor::interaction::try_pickup;
use athanor::map_loader::{load_map_path, LoadOptions};
use athanor::mining::OreVein;
use athanor::spawn::{spawn_proto, MetaData};
use athanor::transform::{tile_center, Grid, Xform};
use athanor::{headless_app, Content, SimClock};
use bevy_ecs::prelude::*;
use bevy_math::IVec2;

fn content() -> Option<Content> {
    static C: OnceLock<Option<Content>> = OnceLock::new();
    C.get_or_init(Content::load_upstream).clone()
}

fn count(world: &mut World, proto: &str) -> usize {
    let mut q = world.query::<&MetaData>();
    q.iter(world).filter(|m| m.proto.as_deref() == Some(proto)).count()
}

/// A floor tile with a free floor tile to its east (rock goes east, miner west).
fn free_pair(world: &mut World, grid: Entity) -> IVec2 {
    let g = world.get::<Grid>(grid).unwrap();
    let mut tiles: Vec<IVec2> = g.tiles.iter().map(|((x, y), _)| IVec2::new(x, y)).collect();
    tiles.sort_by_key(|t| (t.x, t.y));
    *tiles
        .iter()
        .find(|t| g.anchored_at(**t).is_empty() && g.anchored_at(**t + IVec2::X).is_empty() && g.has_tile(**t + IVec2::X))
        .expect("two free tiles")
}

#[test]
fn a_pickaxe_mines_ore_in_one_hit_and_a_fist_does_not() {
    let Some(c) = content() else { return };
    let mut app = headless_app(c);
    let world = app.world_mut();
    let grid = load_map_path(world, "/Maps/_NF/Shuttles/surveyor.yml", None, LoadOptions { run_spawners: false, ..Default::default() })
        .unwrap()
        .grids[0];
    let t = free_pair(world, grid);
    let miner = spawn_proto(world, "MobHuman", grid, tile_center(t)).unwrap();
    let rock = spawn_proto(world, "AsteroidRockCoal", grid, tile_center(t + IVec2::X)).unwrap();
    assert!(world.get::<Xform>(rock).unwrap().anchored);
    app.update();
    let world = app.world_mut();
    let vein = world.get::<OreVein>(rock).unwrap().clone();
    assert!(vein.rolled);
    assert_eq!(vein.current_ore.as_deref(), Some("NFOreCoal"));

    // Bare hands: only damage, the rock stays.
    assert_eq!(try_melee(world, miner, rock), AttackResult::Hit);
    assert!(world.get_entity(rock).is_ok(), "a fist doesn't mine");

    // A pickaxe: one swing and the vein spills 1-5 coal.
    let pick = spawn_proto(world, "Pickaxe", grid, tile_center(t)).unwrap();
    try_pickup(world, miner, pick).unwrap();
    world.resource_mut::<SimClock>().time += 5.0;
    let before = count(world, "Coal5");
    assert_eq!(try_melee(world, miner, rock), AttackResult::Hit);
    assert!(world.get_entity(rock).is_err(), "pickaxe mined the rock");
    let ore = count(world, "Coal5") - before;
    assert!((1..=5).contains(&ore), "{ore} coal");
    assert!(world.get::<Grid>(grid).unwrap().anchored_at(t + IVec2::X).is_empty());

    // A plain rock with a random vein only sometimes has ore; it still mines.
    let plain = spawn_proto(world, "AsteroidRock", grid, tile_center(t + IVec2::X)).unwrap();
    app.update();
    let world = app.world_mut();
    world.resource_mut::<SimClock>().time += 5.0;
    assert_eq!(try_melee(world, miner, plain), AttackResult::Hit);
    assert!(world.get_entity(plain).is_err());
}
