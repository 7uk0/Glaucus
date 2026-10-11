//! Atmospherics on real Frontier content. Passes trivially without upstream.

use std::sync::OnceLock;

use alembic::breathing::LungData;
use alembic::gas::{consts::ONE_ATMOSPHERE, OXYGEN};
use alembic::{air_at, AlembicPlugin, GridAtmos, Respirator};
use athanor::damage::Damageable;
use athanor::doors::Door;
use athanor::map_loader::{load_map_path, LoadOptions};
use athanor::sector::{build_sector, SectorConfig};
use athanor::spawn::{spawn_proto, MetaData};
use athanor::transform::{create_map, tile_center, Grid, Xform};
use athanor::{AthanorPlugin, Content};
use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_math::{IVec2, Vec2};

fn content() -> Option<Content> {
    static C: OnceLock<Option<Content>> = OnceLock::new();
    C.get_or_init(Content::load_upstream).clone()
}

fn app(c: Content) -> App {
    let mut app = App::new();
    app.add_plugins((AthanorPlugin::headless(c), AlembicPlugin));
    app.finish();
    app.cleanup();
    app
}

fn run_seconds(app: &mut App, s: f32) {
    for _ in 0..(s * 30.0) as usize {
        app.update();
    }
}

fn total_moles(world: &World, grid: Entity) -> f32 {
    world.get::<GridAtmos>(grid).unwrap().tiles.values().map(|m| m.total_moles()).sum()
}

fn damage(world: &World, e: Entity, ty: &str) -> f32 {
    world.get::<Damageable>(e).unwrap().damage.get(ty).copied().unwrap_or(0.0)
}

#[test]
fn bodies_know_their_lungs() {
    let Some(c) = content() else { return };
    let lungs = LungData::from_prototypes(&c.protos);
    assert_eq!(lungs.lung_types("Human"), Some(&["Human".to_string()][..]));
    assert!(lungs.lung_types("Vox").is_some_and(|t| t.contains(&"Vox".to_string())));
}

#[test]
fn outpost_air_is_one_atmosphere_and_holds() {
    let Some(c) = content() else { return };
    let mut app = app(c);
    let sector = build_sector(app.world_mut(), &SectorConfig { contractors: 0, vermin: 0, carp: 0, shuttle: None, ..Default::default() }).unwrap();
    app.update();
    let world = app.world_mut();
    let ga = world.get::<GridAtmos>(sector.outpost).expect("the outpost has a saved atmosphere");
    eprintln!("outpost: {} air tiles", ga.tiles.len());
    assert!(ga.tiles.len() > 500);
    let air = air_at(world, sector.player);
    eprintln!("player breathes {:.1} kPa, {:.0}% O2, {:.1} K", air.pressure(), air.fraction(OXYGEN) * 100.0, air.temperature);
    assert!((air.pressure() - ONE_ATMOSPHERE).abs() < 5.0);
    assert!((air.fraction(OXYGEN) - 0.21).abs() < 0.02);

    let before = total_moles(world, sector.outpost);
    run_seconds(&mut app, 20.0);
    let world = app.world_mut();
    let after = total_moles(world, sector.outpost);
    eprintln!("outpost moles {before:.0} -> {after:.0} after 20 s");
    assert!(after > before * 0.995, "the sealed outpost leaks: {before} -> {after}");

    // Breathing station air: no suffocation.
    let r = world.get::<Respirator>(sector.player).unwrap();
    assert!(r.is_breathing(), "saturation {}", r.saturation);
    assert_eq!(damage(world, sector.player, "Asphyxiation"), 0.0);
    assert_eq!(damage(world, sector.player, "Blunt"), 0.0);
}

/// An airlock (not shutters) with floor on one side and open space on the other.
fn external_airlock(world: &mut World, grid: Entity) -> Option<(Entity, IVec2, IVec2)> {
    let doors: Vec<(Entity, IVec2)> = {
        let g = world.get::<Grid>(grid).unwrap();
        let mut v: Vec<(Entity, IVec2)> =
            g.anchors.iter().flat_map(|(t, es)| es.iter().map(move |e| (*t, *e)))
                .filter(|(_, e)| world.get::<Door>(*e).is_some() && world.get::<MetaData>(*e).and_then(|m| m.proto.as_deref()).is_some_and(|p| p.starts_with("Airlock")))
                .map(|(t, e)| (e, t))
                .collect();
        v.sort_by_key(|(_, t)| (t.x, t.y));
        v
    };
    let g = world.get::<Grid>(grid).unwrap();
    let ga = world.get::<GridAtmos>(grid).unwrap();
    for (door, t) in doors {
        for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            let (inside, outside) = (t - d, t + d);
            if !g.has_tile(outside)
                && g.has_tile(inside)
                && ga.tiles.get(&inside).is_some_and(|m| m.pressure() > 90.0)
                && g.anchors.get(&inside).is_none_or(|es| es.iter().all(|e| world.get::<Door>(*e).is_none()))
            {
                return Some((door, t, inside));
            }
        }
    }
    None
}

#[test]
fn open_airlock_vents_the_ship() {
    let Some(c) = content() else { return };
    let mut app = app(c);
    let world = app.world_mut();
    let grid = load_map_path(world, "/Maps/_NF/Shuttles/kestrel.yml", None, LoadOptions::default()).unwrap().grids[0];
    app.update();
    let world = app.world_mut();
    let (door, door_tile, inside) = external_airlock(world, grid).expect("the Kestrel has an external airlock");
    let before = total_moles(world, grid);
    let p0 = world.get::<GridAtmos>(grid).unwrap().mix_at(inside).pressure();
    run_seconds(&mut app, 3.0);
    let world = app.world_mut();
    let held = total_moles(world, grid);
    assert!(held > before * 0.999, "closed ship leaks: {before} -> {held}");

    // Open the docking airlock and hold it open: its tiny fan keeps the air in.
    athanor::doors::try_open(world, door, None);
    world.get_mut::<Door>(door).unwrap().auto_close = None;
    run_seconds(&mut app, 5.0);
    let world = app.world_mut();
    assert!(!world.get::<Door>(door).unwrap().collidable());
    let fanned = total_moles(world, grid);
    assert!(fanned > before * 0.999, "the tiny fan should hold the air: {before} -> {fanned}");

    // Break the fan: now it vents.
    let fans: Vec<Entity> = world.get::<Grid>(grid).unwrap().anchors.get(&door_tile).into_iter().flatten().copied()
        .filter(|e| world.get::<MetaData>(*e).and_then(|m| m.proto.as_deref()).is_some_and(|p| p.starts_with("AtmosDeviceFan")))
        .collect();
    for f in fans {
        athanor::spawn::delete(world, f);
    }
    run_seconds(&mut app, 10.0);
    let world = app.world_mut();
    let after = total_moles(world, grid);
    let p1 = world.get::<GridAtmos>(grid).unwrap().mix_at(inside).pressure();
    eprintln!("airlock {door_tile} open 10 s without its fan: ship moles {before:.0} -> {after:.0}; inside {p0:.1} -> {p1:.1} kPa");
    // Only the airlock vestibule vents: its inner door stays shut.
    assert!(after < before * 0.98, "venting to space must lose air: {before} -> {after}");
    assert!(p1 < p0 * 0.7, "the tile behind the airlock should depressurize: {p0} -> {p1}");

    // Shut it: the leak stops.
    athanor::doors::try_close(world, door, None);
    run_seconds(&mut app, 2.0);
    let world = app.world_mut();
    let shut = total_moles(world, grid);
    run_seconds(&mut app, 5.0);
    let world = app.world_mut();
    let later = total_moles(world, grid);
    eprintln!("closed again: {shut:.0} -> {later:.0}");
    assert!(later > shut * 0.999, "closed airlock still leaking: {shut} -> {later}");
}

#[test]
fn spacing_a_human_hurts_and_a_suit_saves_them() {
    let Some(c) = content() else { return };
    let mut app = app(c);
    let world = app.world_mut();
    let map = create_map(world);
    let bare = spawn_proto(world, "MobHuman", map, Vec2::new(0.0, 0.0)).unwrap();
    let suited = spawn_proto(world, "MobHuman", map, Vec2::new(10.0, 0.0)).unwrap();
    for (slot, item) in [("outerClothing", "ClothingOuterHardsuitEVA"), ("head", "ClothingHeadHelmetEVA")] {
        let i = spawn_proto(world, item, map, Vec2::new(10.0, 0.0)).unwrap();
        assert!(athanor::inventory::equip(world, suited, slot, i), "equip {item}");
    }
    run_seconds(&mut app, 10.0);
    let world = app.world_mut();
    let (blunt, asphyx) = (damage(world, bare, "Blunt"), damage(world, bare, "Asphyxiation"));
    eprintln!("10 s in vacuum: {blunt:.1} Blunt, {asphyx:.1} Asphyxiation; saturation {:.1}", world.get::<Respirator>(bare).unwrap().saturation);
    assert!(blunt >= 15.0, "barotrauma: 0.5 Blunt × 4 per second, got {blunt}");
    assert!(asphyx >= 2.0, "no air to breathe, got {asphyx}");
    assert_eq!(damage(world, suited, "Blunt"), 0.0, "the EVA suit and helmet stop barotrauma");
    assert!(damage(world, suited, "Asphyxiation") > 0.0, "a suit without internals still can't breathe vacuum");
}

#[test]
fn breathing_turns_oxygen_into_carbon_dioxide() {
    let Some(c) = content() else { return };
    let mut app = app(c);
    let world = app.world_mut();
    let grid = load_map_path(world, "/Maps/_NF/Shuttles/kestrel.yml", None, LoadOptions::default()).unwrap().grids[0];
    app.update();
    let world = app.world_mut();
    let tile = *world.get::<GridAtmos>(grid).unwrap().tiles.iter().find(|(_, m)| m.pressure() > 90.0).unwrap().0;
    let human = spawn_proto(world, "MobHuman", grid, tile_center(tile)).unwrap();
    assert_eq!(world.get::<Xform>(human).unwrap().parent, Some(grid));
    run_seconds(&mut app, 30.0);
    let world = app.world_mut();
    let co2: f32 = world.get::<GridAtmos>(grid).unwrap().tiles.values().map(|m| m.moles[alembic::gas::CARBON_DIOXIDE]).sum();
    eprintln!("30 s of breathing on the Kestrel: {co2:.4} mol CO2");
    assert!(co2 > 0.0);
    let r = world.get::<Respirator>(human).unwrap();
    assert!(r.is_breathing() && r.saturation > 2.0, "saturation {}", r.saturation);
    assert_eq!(damage(world, human, "Asphyxiation"), 0.0);
}

#[test]
fn sector_creatures_survive_their_habitats() {
    let Some(c) = content() else { return };
    // Every spawnable breather has lungs we can resolve.
    let lungs = LungData::from_prototypes(&c.protos);
    let mut lungless = Vec::new();
    for p in c.protos.iter_kind("entity") {
        if p.get("abstract").and_then(grimoire::Node::as_bool) == Some(true) || !p.has_component("Respirator") {
            continue;
        }
        let body = p.component("Body").and_then(|b| b.get_str("prototype"));
        if body.is_none_or(|b| lungs.lung_types(b).is_none()) {
            lungless.push(format!("{} ({body:?})", p.id));
        }
    }
    eprintln!("{} breathers without resolvable lungs: {:?}", lungless.len(), &lungless[..lungless.len().min(15)]);

    let mut app = app(c);
    let world = app.world_mut();
    let map = create_map(world);
    let carp = spawn_proto(world, "MobCarp", map, Vec2::ZERO).unwrap();
    let grid = load_map_path(world, "/Maps/_NF/Shuttles/kestrel.yml", None, LoadOptions::default()).unwrap().grids[0];
    app.update();
    let world = app.world_mut();
    let tile = *world.get::<GridAtmos>(grid).unwrap().tiles.iter().filter(|(_, m)| m.pressure() > 90.0).map(|(t, _)| t).min_by_key(|t| (t.x, t.y)).unwrap();
    let mouse = spawn_proto(world, "MobMouse", grid, tile_center(tile)).unwrap();
    run_seconds(&mut app, 20.0);
    let world = app.world_mut();
    let carp_dmg = world.get::<Damageable>(carp).map_or(0.0, |d| d.total());
    eprintln!("carp in space 20 s: {carp_dmg} damage; mouse aboard: {:?}", world.get::<Damageable>(mouse).map(|d| d.damage.clone()));
    assert_eq!(carp_dmg, 0.0, "space carp live in space");
    assert_eq!(damage(world, mouse, "Asphyxiation"), 0.0, "a mouse breathes station air");
}

#[test]
fn vox_are_poisoned_by_oxygen() {
    let Some(c) = content() else { return };
    if c.protos.entity("MobVox").is_none() {
        return;
    }
    let mut app = app(c);
    let world = app.world_mut();
    let grid = load_map_path(world, "/Maps/_NF/Shuttles/kestrel.yml", None, LoadOptions::default()).unwrap().grids[0];
    app.update();
    let world = app.world_mut();
    let tile = *world.get::<GridAtmos>(grid).unwrap().tiles.iter().filter(|(_, m)| m.pressure() > 90.0).map(|(t, _)| t).min_by_key(|t| (t.x, t.y)).unwrap();
    let vox = spawn_proto(world, "MobVox", grid, tile_center(tile)).unwrap();
    run_seconds(&mut app, 6.0);
    let world = app.world_mut();
    let poison = damage(world, vox, "Poison");
    let r = world.get::<Respirator>(vox).unwrap();
    eprintln!("vox on station air 6 s: {poison:.1} Poison, saturation {:.1}", r.saturation);
    assert!(poison > 0.0, "station air is toxic to Vox");
}
