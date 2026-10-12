//! World generation on real Frontier content. Passes trivially without upstream.

use std::sync::OnceLock;

use athanor::map_loader::{load_map_path, LoadOptions};
use athanor::rng::SimRng;
use athanor::spawn::MetaData;
use athanor::transform::{create_map, Grid, WorldPose, Xform};
use athanor::{AthanorPlugin, Content, Player};
use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use demiurge::poi::{generate_pois, poi_protos, PoiConfig};
use demiurge::world::{OwnedDebris, CHUNK_SIZE};
use demiurge::{Cosmos, DemiurgePlugin};

fn content() -> Option<Content> {
    static C: OnceLock<Option<Content>> = OnceLock::new();
    C.get_or_init(Content::load_upstream).clone()
}

fn app(c: Content) -> App {
    let mut app = App::new();
    app.add_plugins((AthanorPlugin::headless(c), DemiurgePlugin));
    app.finish();
    app.cleanup();
    app
}

/// The outpost at the origin, like `GameTicker` loading the Frontier map.
fn sector(app: &mut App) -> (Entity, Entity) {
    let world = app.world_mut();
    let map = create_map(world);
    let outpost = load_map_path(world, "/Maps/_NF/Outpost/frontier.yml", Some(map), LoadOptions::default())
        .unwrap()
        .grids[0];
    (map, outpost)
}

#[test]
fn points_of_interest_follow_frontier_layout() {
    let Some(c) = content() else { return };
    let mut app = app(c.clone());
    let (map, _) = sector(&mut app);
    let world = app.world_mut();
    let cfg = PoiConfig::default();
    let placed = generate_pois(world, map, SimRng(1234), &cfg);
    let protos = poi_protos(&c);
    let find = |id: &str| protos.iter().find(|p| p.id == id).unwrap();

    // Four depots on a ring, lettered A–D, at their own distance band.
    let depots: Vec<_> = placed.iter().filter(|p| p.group == "CargoDepot").collect();
    assert_eq!(depots.len(), 4, "{placed:#?}");
    for (i, d) in depots.iter().enumerate() {
        let letter = (b'A' + i as u8) as char;
        assert!(d.name.ends_with(&format!(" {letter}")), "{}", d.name);
        assert_eq!(d.cargo_destination.as_deref(), Some(format!("Cargo{letter}").as_str()));
        let p = find(&d.proto);
        let r = d.pos.length();
        assert!(r >= p.min_distance as f32 - 1.5 && r <= p.max_distance as f32 + 1.5, "{} at {r}", d.name);
    }
    // Evenly spaced: neighbouring depots a quarter turn apart.
    let a0 = depots[0].pos.y.atan2(depots[0].pos.x);
    let a1 = depots[1].pos.y.atan2(depots[1].pos.x);
    let quarter = (a1 - a0).rem_euclid(std::f32::consts::TAU);
    assert!((quarter - std::f32::consts::FRAC_PI_2).abs() < 0.01, "{quarter}");

    assert_eq!(placed.iter().filter(|p| p.group == "MarketStation").count(), 1);
    let required: Vec<_> = protos.iter().filter(|p| p.spawn_group == "Required" && p.spawn_presets.is_empty()).collect();
    for r in required {
        assert!(placed.iter().any(|p| p.proto == r.id), "missing required {}", r.id);
    }
    // Every other group places at most one.
    let mut groups: std::collections::HashMap<&str, usize> = Default::default();
    for p in &placed {
        *groups.entry(p.group.as_str()).or_default() += 1;
    }
    for (g, n) in &groups {
        if !matches!(*g, "CargoDepot" | "MarketStation" | "Required" | "Optional") {
            assert_eq!(*n, 1, "group {g}");
        }
    }
    // Each POI is a named grid on the sector map at its offset.
    for p in &placed {
        let x = world.get::<Xform>(p.grid).unwrap();
        assert_eq!(x.parent, Some(map));
        assert!((x.pos - p.pos).length() < 64.0, "{} grid at {:?}, offset {:?}", p.name, x.pos, p.pos);
        assert_eq!(world.get::<MetaData>(p.grid).unwrap().name, p.name);
        assert!(world.get::<Grid>(p.grid).is_some_and(|g| g.tiles.iter().next().is_some()));
    }
    eprintln!("{} POIs: {:?}", placed.len(), placed.iter().map(|p| (&p.name, p.pos.length() as i32)).collect::<Vec<_>>());
}

#[test]
fn asteroid_fields_load_around_the_player_and_unload_behind() {
    let Some(c) = content() else { return };
    let mut app = app(c.clone());
    let (map, _) = sector(&mut app);
    let cosmos = Cosmos::new(&c, map, "NFDefault", 42);
    let ids: Vec<&str> = cosmos.biomes.iter().map(|b| b.id.as_str()).collect();
    for id in ["NFAsteroidsNear", "NFAsteroidsMid", "NFAsteroidsFar"] {
        assert!(ids.contains(&id), "{ids:?}");
    }
    assert!(cosmos.biomes.windows(2).all(|w| w[0].priority >= w[1].priority));
    app.world_mut().insert_resource(cosmos);

    // A player sweeping the near belt (2.6–4.6 km out, past the outpost's
    // sparse zone). Carver noise and random cancellation thin the field, so
    // look at several spots.
    let world = app.world_mut();
    let player = world
        .spawn((Player, Xform { parent: Some(map), pos: Vec2::new(4000.0, 0.0), rot: 0.0, anchored: false }, WorldPose::default()))
        .id();
    let debris = |app: &mut App| {
        let w = app.world_mut();
        let mut q = w.query::<(Entity, &OwnedDebris, &WorldPose)>();
        q.iter(w).map(|(e, o, p)| (e, o.chunk, p.pos)).collect::<Vec<_>>()
    };
    let mut seen = std::collections::HashSet::new();
    let mut near = Vec::new();
    for k in 0..8 {
        let a = k as f32 * std::f32::consts::TAU / 8.0;
        let at = Vec2::new(a.cos(), a.sin()) * 4000.0;
        app.world_mut().get_mut::<Xform>(player).unwrap().pos = at;
        for _ in 0..3 {
            app.update();
        }
        near = debris(&mut app);
        for (e, _, pos) in &near {
            seen.insert(*e);
            assert!(pos.distance(at) < CHUNK_SIZE * 4.0, "debris at {pos:?} with the player at {at:?}");
        }
    }
    assert!(seen.len() >= 5, "only {} debris in the whole near belt", seen.len());
    let biomes: Vec<String> = {
        let cosmos = app.world().resource::<Cosmos>();
        near.iter().filter_map(|(_, c, _)| cosmos.chunks.get(c).and_then(|ch| ch.biome.clone())).collect()
    };
    assert!(biomes.iter().all(|b| b == "NFAsteroidsNear"), "{biomes:?}");

    // Fly on: the old chunks unload and their debris is collected.
    app.world_mut().get_mut::<Xform>(player).unwrap().pos = Vec2::new(7000.0, 2000.0);
    for _ in 0..3 {
        app.update();
    }
    let after = debris(&mut app);
    for (e, _, _) in &near {
        assert!(!after.iter().any(|(a, _, _)| a == e), "old debris {e:?} survived");
    }

    // Near the outpost: the dead zone keeps the first kilometre clear.
    app.world_mut().get_mut::<Xform>(player).unwrap().pos = Vec2::new(300.0, 0.0);
    for _ in 0..3 {
        app.update();
    }
    let home = debris(&mut app);
    for (_, _, pos) in &home {
        assert!(pos.length() > 500.0, "debris inside the outpost dead zone at {pos:?}");
    }
}
