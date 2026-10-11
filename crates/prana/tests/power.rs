//! Power on real Frontier content. Passes trivially without upstream.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use athanor::doors::Door;
use athanor::map_loader::{load_map_path, LoadOptions};
use athanor::power::{Unpowered, Weightless};
use athanor::shuttles::{Shuttle, Thruster};
use athanor::spawn::MetaData;
use athanor::transform::Xform;
use athanor::{AthanorPlugin, Content};
use bevy_app::App;
use bevy_ecs::prelude::*;
use prana::devices::{FuelGenerator, MaterialStorage};
use prana::net::{ApcPowerReceiver, Battery, PowerSupplier, PowerTopology};
use prana::PranaPlugin;

fn content() -> Option<Content> {
    static C: OnceLock<Option<Content>> = OnceLock::new();
    C.get_or_init(Content::load_upstream).clone()
}

fn app(c: Content) -> App {
    let mut app = App::new();
    app.add_plugins((AthanorPlugin::headless(c), PranaPlugin));
    app.finish();
    app.cleanup();
    app
}

fn run_seconds(app: &mut App, s: f32) {
    for _ in 0..(s * 30.0) as usize {
        app.update();
    }
}

fn on_grid<C: Component>(world: &mut World, grid: Entity) -> Vec<Entity> {
    let mut q = world.query_filtered::<(Entity, &Xform), With<C>>();
    q.iter(world).filter(|(_, x)| x.parent == Some(grid)).map(|(e, _)| e).collect()
}

fn proto(world: &World, e: Entity) -> String {
    world.get::<MetaData>(e).and_then(|m| m.proto.as_deref().map(str::to_string)).unwrap_or_default()
}

fn unpowered_report(world: &mut World, grid: Entity) -> (usize, usize, BTreeMap<String, usize>) {
    let rs = on_grid::<ApcPowerReceiver>(world, grid);
    let mut off = BTreeMap::new();
    for &e in &rs {
        if world.get::<Unpowered>(e).is_some() {
            *off.entry(proto(world, e)).or_insert(0) += 1;
        }
    }
    let n_off = off.values().sum();
    (rs.len(), n_off, off)
}

fn load_kestrel(app: &mut App) -> Entity {
    let world = app.world_mut();
    load_map_path(world, "/Maps/_NF/Shuttles/kestrel.yml", None, LoadOptions::default()).unwrap().grids[0]
}

#[test]
fn the_kestrel_powers_up() {
    let Some(c) = content() else { return };
    let mut app = app(c);
    let grid = load_kestrel(&mut app);
    run_seconds(&mut app, 3.0);
    let world = app.world_mut();
    let topo = world.resource::<PowerTopology>();
    eprintln!("{} networks on the map", topo.nets.len());
    let (n, off, which) = unpowered_report(world, grid);
    eprintln!("Kestrel: {n} receivers, {off} unpowered: {which:?}");
    assert!(n > 10);
    assert_eq!(off, 0, "every machine on a fresh Kestrel has power: {which:?}");
    for t in on_grid::<Thruster>(world, grid) {
        assert!(world.get::<Unpowered>(t).is_none(), "{} unpowered", proto(world, t));
    }
    let generator = on_grid::<FuelGenerator>(world, grid)[0];
    assert!(world.get::<FuelGenerator>(generator).unwrap().on, "shuttle generator starts on map init");
    let s = world.get::<PowerSupplier>(generator).unwrap();
    eprintln!("generator supplying {:.0} W of {:.0} W", s.supply.current_supply, s.supply.max_supply);
    assert!(s.supply.current_supply > 0.0);
    assert!(world.get::<Weightless>(grid).is_none(), "the mini gravity generator gives the Kestrel gravity");
    let sh = world.get::<Shuttle>(grid).unwrap();
    assert!(sh.linear_thrust.iter().sum::<f32>() > 0.0);
}

#[test]
fn losing_the_apc_kills_the_ship() {
    let Some(c) = content() else { return };
    let mut app = app(c);
    let grid = load_kestrel(&mut app);
    run_seconds(&mut app, 2.0);
    let world = app.world_mut();
    let apcs = on_grid::<prana::net::Apc>(world, grid);
    eprintln!("Kestrel has {} APCs", apcs.len());
    assert!(!apcs.is_empty());
    for apc in apcs {
        athanor::spawn::delete(world, apc);
    }
    run_seconds(&mut app, 2.0);
    let world = app.world_mut();
    let (n, off, _) = unpowered_report(world, grid);
    eprintln!("APC gone: {off}/{n} receivers unpowered");
    assert!(off as f32 > n as f32 * 0.8, "most machines hang off the APCs");
    let sh = world.get::<Shuttle>(grid).unwrap();
    assert_eq!(sh.linear_thrust.iter().sum::<f32>(), 0.0, "no power, no thrust");
    assert_eq!(sh.angular_thrust, 0.0, "gyroscope down too");
    // Airlocks freeze.
    let door = on_grid::<Door>(world, grid)
        .into_iter()
        .find(|d| world.get::<Door>(*d).unwrap().airlock && world.get::<Door>(*d).unwrap().is_closed())
        .expect("a closed airlock");
    assert!(!athanor::doors::try_open(world, door, None), "an unpowered airlock won't open");
    // Gravity holds while the generator's charge runs down (~100 s), then fails.
    assert!(world.get::<Weightless>(grid).is_none());
    run_seconds(&mut app, 105.0);
    assert!(app.world().get::<Weightless>(grid).is_some(), "gravity fails once the gravity generator drains");
}

#[test]
fn generators_burn_fuel_and_the_smes_takes_over() {
    let Some(c) = content() else { return };
    let mut app = app(c);
    let grid = load_kestrel(&mut app);
    run_seconds(&mut app, 1.0);
    let world = app.world_mut();
    let generator = on_grid::<FuelGenerator>(world, grid)[0];
    let fuel0 = world.get::<MaterialStorage>(generator).unwrap().storage.clone();
    run_seconds(&mut app, 60.0);
    let world = app.world_mut();
    let fuel1 = world.get::<MaterialStorage>(generator).unwrap().storage.clone();
    eprintln!("SUPERPACMAN fuel after 60 s: {fuel0:?} -> {fuel1:?}");
    assert!(fuel1.values().sum::<i64>() < fuel0.values().sum::<i64>(), "a running generator burns uranium");
    // Empty it: the generator stops and the SMES carries the ship.
    world.get_mut::<MaterialStorage>(generator).unwrap().storage.clear();
    run_seconds(&mut app, 2.0);
    let world = app.world_mut();
    assert!(!world.get::<FuelGenerator>(generator).unwrap().on, "out of fuel, the generator shuts off");
    let smes: Vec<Entity> = on_grid::<Battery>(world, grid).into_iter().filter(|e| proto(world, *e).starts_with("SMES")).collect();
    let s0 = world.get::<Battery>(smes[0]).unwrap().charge;
    run_seconds(&mut app, 10.0);
    let world = app.world_mut();
    let s1 = world.get::<Battery>(smes[0]).unwrap().charge;
    let (n, off, which) = unpowered_report(world, grid);
    eprintln!("on SMES: charge {s0:.0} -> {s1:.0} J; {off}/{n} unpowered {which:?}");
    assert!(s1 < s0, "the SMES discharges");
    assert_eq!(off, 0, "and keeps everything running");
}

#[test]
fn the_outpost_is_powered() {
    let Some(c) = content() else { return };
    let mut app = app(c);
    let world = app.world_mut();
    let grid = load_map_path(world, "/Maps/_NF/Outpost/frontier.yml", None, LoadOptions::default()).unwrap().grids[0];
    run_seconds(&mut app, 3.0);
    let world = app.world_mut();
    let (n, off, which) = unpowered_report(world, grid);
    eprintln!("Frontier Outpost: {n} receivers, {off} unpowered: {which:?}");
    assert!(n > 200);
    assert!((off as f32) < n as f32 * 0.05, "nearly every outpost machine has power");
    assert!(world.get::<Weightless>(grid).is_none(), "the outpost has gravity");
}
