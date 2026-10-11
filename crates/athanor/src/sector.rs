//! Single-player sector setup — the stand-in for Frontier's
//! `NFAdventureRule` game rule. Builds a playable slice of the Frontier
//! sector: the outpost, a docked-in-spirit shuttle, a populated crew of NPC
//! contractors, station vermin, and space carp prowling outside.

use bevy_ecs::prelude::*;
use bevy_math::{IVec2, Vec2};

use crate::map_loader::{load_map_path, LoadOptions};
use crate::physics::{group, tile_blocked};
use crate::rng::SimRng;
use crate::spawn::{spawn_proto, MetaData};
use crate::transform::{create_map, tile_center, world_pose_of, Grid, Xform};
use crate::Player;

#[derive(Debug, Clone)]
pub struct SectorConfig {
    pub outpost: String,
    pub shuttle: Option<String>,
    pub player_proto: String,
    pub contractors: usize,
    pub vermin: usize,
    pub carp: usize,
    pub seed: u64,
}

impl Default for SectorConfig {
    fn default() -> Self {
        SectorConfig {
            outpost: "/Maps/_NF/Outpost/frontier.yml".into(),
            shuttle: Some("/Maps/_NF/Shuttles/kestrel.yml".into()),
            player_proto: "MobHuman".into(),
            contractors: 12,
            vermin: 6,
            carp: 4,
            seed: 0xCA9D_0D7E,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Sector {
    pub map: Entity,
    pub outpost: Entity,
    pub shuttle: Option<Entity>,
    pub player: Entity,
    pub npcs: Vec<Entity>,
}

/// Walkable, unblocked tiles of a grid.
pub fn open_tiles(world: &World, grid: Entity) -> Vec<IVec2> {
    let mask = group::by_name("MobMask").unwrap();
    let Some(g) = world.get::<Grid>(grid) else { return Vec::new() };
    let mut v: Vec<IVec2> = g
        .tiles
        .iter()
        .map(|(p, _)| IVec2::new(p.0, p.1))
        .filter(|t| !tile_blocked(world, grid, *t, mask))
        .collect();
    v.sort_by_key(|t| (t.x, t.y));
    v
}

fn find_proto_on_grid(world: &mut World, grid: Entity, pred: impl Fn(&str) -> bool) -> Option<Vec2> {
    let mut q = world.query::<(&MetaData, &Xform)>();
    q.iter(world)
        .filter(|(m, x)| x.parent == Some(grid) && m.proto.as_deref().is_some_and(&pred))
        .map(|(_, x)| x.pos)
        .next()
}

pub fn build_sector(world: &mut World, cfg: &SectorConfig) -> Result<Sector, String> {
    world.insert_resource(SimRng(cfg.seed));
    let map = create_map(world);

    let outpost = load_map_path(world, &cfg.outpost, Some(map), LoadOptions::default())?
        .grids
        .first()
        .copied()
        .ok_or("outpost map has no grid")?;

    // Park the shuttle just off the outpost's east edge.
    let shuttle = match &cfg.shuttle {
        Some(path) => {
            let bounds = world.get::<Grid>(outpost).and_then(|g| g.tiles.bounds()).unwrap_or((0, 0, 0, 0));
            let op = world_pose_of(world, outpost);
            let offset = op.pos + Vec2::new(bounds.2 as f32 + 18.0, (bounds.1 + bounds.3) as f32 * 0.5);
            let loaded = load_map_path(
                world,
                path,
                Some(map),
                LoadOptions {
                    offset,
                    ..Default::default()
                },
            )?;
            loaded.grids.first().copied()
        }
        None => None,
    };

    let tiles = open_tiles(world, outpost);
    if tiles.is_empty() {
        return Err("outpost has no walkable tiles".into());
    }

    // Player: a latejoin/arrivals spawn point if the map has one.
    let spawn_pos = find_proto_on_grid(world, outpost, |p| p.contains("SpawnPointLatejoin"))
        .or_else(|| find_proto_on_grid(world, outpost, |p| p.starts_with("SpawnPoint")))
        .unwrap_or_else(|| tile_center(tiles[tiles.len() / 2]));
    let player = spawn_proto(world, &cfg.player_proto, outpost, spawn_pos).ok_or("bad player prototype")?;
    world.entity_mut(player).insert(Player);
    crate::inventory::equip_starting_gear(world, player, "CaputPlayerGear");
    // The player is not an NPC even if the prototype has a brain.
    world.entity_mut(player).remove::<crate::naming::PendingName>();
    if let Some(mut m) = world.get_mut::<MetaData>(player) {
        m.name = "You".into();
    }

    let mut npcs = Vec::new();
    let mut rng = world.resource::<SimRng>().clone();
    let pick = |rng: &mut SimRng| tile_center(*rng.pick(&tiles).unwrap());
    for _ in 0..cfg.contractors {
        let p = pick(&mut rng);
        if let Some(e) = spawn_proto(world, "CaputMobContractor", outpost, p) {
            npcs.push(e);
        }
    }
    for i in 0..cfg.vermin {
        let p = pick(&mut rng);
        let proto = if i % 3 == 2 { "MobCatSpace" } else { "MobMouse" };
        if let Some(e) = spawn_proto(world, proto, outpost, p).or_else(|| spawn_proto(world, "MobMouse", outpost, p)) {
            npcs.push(e);
        }
    }
    // Carp drift in space around the outpost.
    let bounds = world.get::<Grid>(outpost).and_then(|g| g.tiles.bounds()).unwrap_or((0, 0, 0, 0));
    let op = world_pose_of(world, outpost);
    for i in 0..cfg.carp {
        let side = i % 4;
        let t = rng.next_f32();
        let (x0, y0, x1, y1) = (bounds.0 as f32 - 6.0, bounds.1 as f32 - 6.0, bounds.2 as f32 + 7.0, bounds.3 as f32 + 7.0);
        let local = match side {
            0 => Vec2::new(x0 + (x1 - x0) * t, y1),
            1 => Vec2::new(x1, y0 + (y1 - y0) * t),
            2 => Vec2::new(x0 + (x1 - x0) * t, y0),
            _ => Vec2::new(x0, y0 + (y1 - y0) * t),
        };
        if let Some(e) = spawn_proto(world, "MobCarp", map, op.pos + local) {
            npcs.push(e);
        }
    }
    *world.resource_mut::<SimRng>() = rng;
    crate::naming::resolve_names(world);
    crate::inventory::resolve_loadouts(world);
    Ok(Sector {
        map,
        outpost,
        shuttle,
        player,
        npcs,
    })
}
