//! Headless simulation tests against real Frontier content. They pass
//! trivially when `upstream/` is missing (run tools/summon-upstream.sh).

use std::sync::OnceLock;

use athanor::damage::{MobState, MobStateKind};
use athanor::doors::{Door, DoorState};
use athanor::map_loader::{load_map_path, LoadOptions};
use athanor::movement::InputMover;
use athanor::spawn::{spawn_proto, MetaData};
use athanor::transform::{tile_center, tile_of, Grid, Xform};
use athanor::{headless_app, Content};
use bevy_ecs::prelude::*;
use bevy_math::{IVec2, Vec2};

fn content() -> Option<Content> {
    static C: OnceLock<Option<Content>> = OnceLock::new();
    C.get_or_init(Content::load_upstream).clone()
}

fn find_by_proto(world: &mut World, proto: &str) -> Vec<Entity> {
    let mut q = world.query::<(Entity, &MetaData)>();
    q.iter(world)
        .filter(|(_, m)| m.proto.as_deref() == Some(proto))
        .map(|(e, _)| e)
        .collect()
}

#[test]
fn loads_adder_shuttle() {
    let Some(c) = content() else { return };
    let mut app = headless_app(c);
    let world = app.world_mut();
    let loaded = load_map_path(world, "/Maps/_NF/Shuttles/adder.yml", None, LoadOptions::default()).unwrap();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert_eq!(loaded.grids.len(), 1);
    let grid = loaded.grids[0];
    assert_eq!(world.get::<MetaData>(grid).unwrap().name, "Adder");
    let g = world.get::<Grid>(grid).unwrap();
    assert!(g.tiles.tile_count() > 20);
    assert!(g.anchors.len() > 20, "walls etc should be anchored, got {}", g.anchors.len());
    let doors = {
        let mut q = world.query::<&Door>();
        q.iter(world).count()
    };
    assert!(doors >= 4, "adder has airlocks, found {doors}");
    app.update();
}

/// Find a floor tile whose east neighbour holds a wall.
fn floor_west_of_wall(world: &mut World, grid: Entity) -> Option<IVec2> {
    let walls: Vec<IVec2> = {
        let g = world.get::<Grid>(grid).unwrap();
        g.anchors
            .iter()
            .filter(|(_, es)| {
                es.iter().any(|e| {
                    world
                        .get::<MetaData>(*e)
                        .and_then(|m| m.proto.as_deref().map(|p| p.starts_with("Wall")))
                        .unwrap_or(false)
                })
            })
            .map(|(t, _)| *t)
            .collect()
    };
    let mob_mask = athanor::physics::group::by_name("MobMask").unwrap();
    walls
        .into_iter()
        .map(|w| w - IVec2::X)
        .find(|t| world.get::<Grid>(grid).unwrap().has_tile(*t) && !athanor::physics::tile_blocked(world, grid, *t, mob_mask))
}

#[test]
fn humans_walk_and_walls_stop_them() {
    let Some(c) = content() else { return };
    let mut app = headless_app(c);
    let grid = {
        let world = app.world_mut();
        load_map_path(world, "/Maps/_NF/Shuttles/adder.yml", None, LoadOptions::default()).unwrap().grids[0]
    };
    let start = floor_west_of_wall(app.world_mut(), grid).expect("a floor tile next to a wall");
    let human = spawn_proto(app.world_mut(), "MobHuman", grid, tile_center(start)).unwrap();
    app.world_mut().get_mut::<InputMover>(human).expect("humans have InputMover").wish = Vec2::X;
    for _ in 0..60 {
        app.update();
    }
    let x = *app.world().get::<Xform>(human).unwrap();
    assert_eq!(x.parent, Some(grid), "still on the shuttle");
    let wall_left_edge = (start.x + 1) as f32;
    assert!(x.pos.x > start.x as f32 + 0.5 + 0.1, "moved east: {:?}", x.pos);
    assert!(x.pos.x < wall_left_edge, "stopped by the wall: {:?} vs wall at {wall_left_edge}", x.pos);
}

#[test]
fn bumping_an_airlock_opens_it() {
    let Some(c) = content() else { return };
    let mut app = headless_app(c);
    let grid = {
        let world = app.world_mut();
        load_map_path(world, "/Maps/_NF/Shuttles/adder.yml", None, LoadOptions::default()).unwrap().grids[0]
    };
    let world = app.world_mut();
    // The command airlock (uid 20) sits at (-0.5, -1.5); approach from the side
    // that has floor.
    let door = find_by_proto(world, "AirlockCommand")[0];
    let dt = tile_of(world.get::<Xform>(door).unwrap().pos);
    let mob_mask = athanor::physics::group::by_name("MobMask").unwrap();
    let (approach, dir) = [IVec2::Y, -IVec2::Y, IVec2::X, -IVec2::X]
        .into_iter()
        .map(|d| (dt + d, -d))
        .find(|(t, d)| {
            let g = world.get::<Grid>(grid).unwrap();
            let beyond = dt + *d;
            g.has_tile(*t)
                && g.has_tile(beyond)
                && !athanor::physics::tile_blocked(world, grid, *t, mob_mask)
                && !athanor::physics::tile_blocked(world, grid, beyond, mob_mask)
        })
        .expect("free tile beside door");

    let human = spawn_proto(world, "MobHuman", grid, tile_center(approach)).unwrap();
    world.get_mut::<InputMover>(human).unwrap().wish = dir.as_vec2();
    let mut opened = false;
    for _ in 0..90 {
        app.update();
        let st = app.world().get::<Door>(door).unwrap().state;
        if matches!(st, DoorState::Opening | DoorState::Open) {
            opened = true;
        }
    }
    assert!(opened, "door should open when bumped");
    let pos = app.world().get::<Xform>(human).unwrap().pos;
    let travelled = (pos - tile_center(approach)).dot(dir.as_vec2());
    assert!(travelled > 1.0, "walked through the doorway, travelled {travelled}");
}

#[test]
fn punching_a_carp_kills_it() {
    let Some(c) = content() else { return };
    let mut app = headless_app(c);
    let grid = {
        let world = app.world_mut();
        load_map_path(world, "/Maps/_NF/Shuttles/adder.yml", None, LoadOptions::default()).unwrap().grids[0]
    };
    let world = app.world_mut();
    let floor = floor_west_of_wall(world, grid).unwrap();
    let human = spawn_proto(world, "MobHuman", grid, tile_center(floor)).unwrap();
    let carp = spawn_proto(world, "MobCarp", grid, tile_center(floor) + Vec2::new(0.0, 0.6)).unwrap();
    assert!(world.get::<MobState>(carp).is_some());
    let mut swings = 0;
    for _ in 0..600 {
        let r = athanor::combat::try_melee(app.world_mut(), human, carp);
        if r == athanor::combat::AttackResult::Hit {
            swings += 1;
        }
        app.update();
        if app.world().get::<MobState>(carp).unwrap().0 == MobStateKind::Dead {
            break;
        }
    }
    assert_eq!(app.world().get::<MobState>(carp).unwrap().0, MobStateKind::Dead, "after {swings} hits");
    assert!(swings >= 3, "carp should take several punches, took {swings}");
}

#[test]
fn every_frontier_shuttle_and_poi_spawns() {
    let Some(c) = content() else { return };
    let res = c.res.clone();
    let mut app = headless_app(c);
    let mut paths = res.find_files(&vellum::ResPath::new("/Maps/_NF/Shuttles"), "yml");
    paths.extend(res.find_files(&vellum::ResPath::new("/Maps/_NF/POI"), "yml"));
    paths.push(vellum::ResPath::new("/Maps/_NF/Outpost/frontier.yml"));
    let t = std::time::Instant::now();
    let mut total = 0;
    let mut warnings = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        let opts = LoadOptions {
            offset: Vec2::new((i as f32) * 300.0, 0.0),
            ..Default::default()
        };
        let loaded = load_map_path(app.world_mut(), p.as_str(), None, opts).unwrap();
        total += loaded.entities.len() + loaded.spawned_by_spawners;
        warnings.extend(loaded.warnings.into_iter().map(|w| format!("{p}: {w}")));
    }
    for _ in 0..10 {
        app.update();
    }
    eprintln!("{} maps, {total} entities spawned in {:?}", paths.len(), t.elapsed());
    for w in warnings.iter().take(10) {
        eprintln!("  {w}");
    }
    assert!(paths.len() > 100);
    assert!(warnings.is_empty());
}
