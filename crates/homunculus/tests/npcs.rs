//! NPC behaviour on real Frontier content. Passes trivially without upstream.

use std::sync::OnceLock;

use athanor::damage::Damageable;
use athanor::map_loader::{load_map_path, LoadOptions};
use athanor::physics::{group, tile_blocked};
use athanor::spawn::spawn_proto;
use athanor::transform::{tile_center, Grid, Xform};
use athanor::{AthanorPlugin, Content};
use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_math::IVec2;
use homunculus::{Domain, HomunculusPlugin, Htn};

fn content() -> Option<Content> {
    static C: OnceLock<Option<Content>> = OnceLock::new();
    C.get_or_init(Content::load_upstream).clone()
}

fn app(c: Content) -> App {
    let mut app = App::new();
    app.add_plugins((AthanorPlugin::headless(c), HomunculusPlugin));
    app.finish();
    app.cleanup();
    app
}

/// Two walkable tiles on the same row, `gap` apart, with open floor between.
fn open_row(world: &World, grid: Entity, gap: i32) -> Option<(IVec2, IVec2)> {
    let mask = group::by_name("MobMask").unwrap();
    let g = world.get::<Grid>(grid)?;
    let mut tiles: Vec<IVec2> = g.tiles.iter().map(|(p, _)| IVec2::new(p.0, p.1)).collect();
    tiles.sort_by_key(|t| (t.y, t.x));
    tiles.into_iter().find_map(|a| {
        let ok = (0..=gap).all(|i| {
            let t = a + IVec2::new(i, 0);
            g.has_tile(t) && !tile_blocked(world, grid, t, mask)
        });
        ok.then_some((a, a + IVec2::new(gap, 0)))
    })
}

#[test]
fn every_htn_compound_parses() {
    let Some(c) = content() else { return };
    let d = Domain::from_prototypes(&c.protos);
    eprintln!(
        "{} compounds, {} queries, {} curves; unported: {:?}",
        d.compounds.len(),
        d.queries.len(),
        d.curves.len(),
        d.unported
    );
    assert!(d.compounds.contains_key("SimpleHostileCompound"));
    assert!(d.compounds.contains_key("IdleCompound"));
    assert!(d.queries.contains_key("NearbyMeleeTargets"));
}

#[test]
fn carp_hunts_and_bites_a_human() {
    let Some(c) = content() else { return };
    let mut app = app(c);
    let grid = load_map_path(app.world_mut(), "/Maps/_NF/Outpost/frontier.yml", None, LoadOptions {
        run_spawners: false,
        ..Default::default()
    })
    .unwrap()
    .grids[0];
    let (a, b) = open_row(app.world(), grid, 5).expect("an open corridor");
    let human = spawn_proto(app.world_mut(), "MobHuman", grid, tile_center(a)).unwrap();
    let carp = spawn_proto(app.world_mut(), "MobCarp", grid, tile_center(b)).unwrap();
    assert!(app.world().get::<Htn>(carp).is_some(), "carp has an HTN brain");
    let start = app.world().get::<Xform>(carp).unwrap().pos;
    let mut closest = f32::MAX;
    for _ in 0..(30 * 20) {
        app.update();
        let d = athanor::interaction::distance(app.world(), carp, human);
        closest = closest.min(d);
        if app.world().get::<Damageable>(human).unwrap().total() > 15.0 {
            break;
        }
    }
    let hurt = app.world().get::<Damageable>(human).unwrap().total();
    let htn = app.world().get::<Htn>(carp).unwrap();
    eprintln!(
        "carp moved {:?} -> {:?}, closest {closest:.2}, human damage {hurt}, plan {:?}",
        start,
        app.world().get::<Xform>(carp).unwrap().pos,
        htn.plan.as_ref().map(|p| (&p.btr, p.index))
    );
    assert!(closest < 1.6, "carp closed in on the human");
    assert!(hurt > 0.0, "carp bit the human");
}

#[test]
fn idle_mobs_wander() {
    let Some(c) = content() else { return };
    let mut app = app(c);
    let grid = load_map_path(app.world_mut(), "/Maps/_NF/Outpost/frontier.yml", None, LoadOptions {
        run_spawners: false,
        ..Default::default()
    })
    .unwrap()
    .grids[0];
    let (a, _) = open_row(app.world(), grid, 4).unwrap();
    let mouse = spawn_proto(app.world_mut(), "MobMouse", grid, tile_center(a)).unwrap();
    assert!(app.world().get::<Htn>(mouse).is_some());
    let start = app.world().get::<Xform>(mouse).unwrap().pos;
    let mut max_d: f32 = 0.0;
    for _ in 0..(30 * 30) {
        app.update();
        let p = app.world().get::<Xform>(mouse).unwrap().pos;
        max_d = max_d.max(p.distance(start));
    }
    eprintln!("mouse wandered up to {max_d:.2} tiles");
    assert!(max_d > 1.0, "the mouse should wander off its spawn tile");
}
