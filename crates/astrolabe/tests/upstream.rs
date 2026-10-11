//! Integration tests over every upstream map and RSI. Skipped (passing) when
//! `upstream/` is absent.

use astrolabe::{read_decal_grid, MapFile, Migrations, TileRegistry};
use grimoire::PrototypeManager;
use vellum::{rsi::Rsi, ResPath, Resources};

fn resources() -> Option<Resources> {
    Some(Resources::from_upstream_dir(&vellum::locate_upstream()?))
}

#[test]
fn every_map_loads_and_decodes() {
    let Some(res) = resources() else { return };
    let pm = PrototypeManager::load_all(&res);
    let tiles = TileRegistry::from_prototypes(&pm);
    assert_eq!(tiles.get(0).unwrap().id, "Space");
    let migrations = Migrations::load(&res);
    assert!(migrations.len() > 100, "{} migrations", migrations.len());

    let maps = res.find_files(&ResPath::new("/Maps"), "yml");
    let t = std::time::Instant::now();
    let (mut ok, mut grids, mut ents, mut tiles_total, mut decals) = (0, 0, 0, 0usize, 0usize);
    let mut failures = Vec::new();
    let mut missing_protos = std::collections::BTreeSet::new();
    for path in &maps {
        let mut map = match MapFile::load(&res, path) {
            Ok(m) => m,
            Err(e) => {
                failures.push(format!("{path}: {e}"));
                continue;
            }
        };
        ok += 1;
        map.apply_migrations(&migrations);
        ents += map.entities.len();
        for g in map.grids() {
            grids += 1;
            match map.grid_tiles(g, &tiles) {
                Ok(gt) => tiles_total += gt.tile_count(),
                Err(e) => failures.push(format!("{path} grid {}: {e}", g.uid)),
            }
            if let Some(dg) = g.component("DecalGrid") {
                decals += read_decal_grid(dg).len();
            }
        }
        for p in map.missing_prototypes(&pm) {
            missing_protos.insert(format!("{p} ({path})"));
        }
    }
    eprintln!(
        "{ok}/{} maps, {grids} grids, {ents} entities, {tiles_total} tiles, {decals} decals in {:?}",
        maps.len(),
        t.elapsed()
    );
    for f in failures.iter().take(20) {
        eprintln!("  FAIL {f}");
    }
    eprintln!("{} distinct missing prototype refs", missing_protos.len());
    for m in missing_protos.iter().take(15) {
        eprintln!("  missing proto {m}");
    }
    assert!(failures.is_empty(), "{} map failures", failures.len());
}

#[test]
fn frontier_shuttle_adder_is_sane() {
    let Some(res) = resources() else { return };
    let pm = PrototypeManager::load_all(&res);
    let tiles = TileRegistry::from_prototypes(&pm);
    let map = MapFile::load(&res, &ResPath::new("/Maps/_NF/Shuttles/adder.yml")).unwrap();
    assert_eq!(map.format, 7);
    let grid = map.grids().next().expect("a grid");
    assert_eq!(grid.name(), Some("Adder"));
    let gt = map.grid_tiles(grid, &tiles).unwrap();
    assert!(gt.tile_count() > 20, "adder has {} tiles", gt.tile_count());
    let airlock = map.entity(20).unwrap();
    assert_eq!(airlock.proto.as_deref(), Some("AirlockCommand"));
    let tr = airlock.transform();
    assert_eq!(tr.pos, (-0.5, -1.5));
    assert_eq!(tr.parent, Some(1));
    let comps = airlock.effective_components(&pm);
    assert!(comps.iter().any(|c| c.get_str("type") == Some("Door")), "airlock inherits Door");
}

#[test]
fn every_rsi_meta_parses() {
    let Some(res) = resources() else { return };
    let mut n = 0;
    let mut bad = Vec::new();
    for meta in res.find_files(&ResPath::new("/Textures"), "json") {
        if meta.file_name() != "meta.json" {
            continue;
        }
        let dir = ResPath::new(meta.as_str().trim_end_matches("/meta.json"));
        if !dir.as_str().ends_with(".rsi") {
            continue;
        }
        n += 1;
        if let Err(e) = Rsi::load(&res, &dir) {
            bad.push(e.to_string());
        }
    }
    eprintln!("{n} RSIs, {} bad", bad.len());
    for b in bad.iter().take(10) {
        eprintln!("  {b}");
    }
    assert!(n > 4000);
    assert!(bad.is_empty());
}
