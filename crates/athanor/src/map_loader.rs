//! Instantiate an [`astrolabe::MapFile`] into the world. Port of Robust's
//! `MapLoaderSystem`/`EntityDeserializer` load path:
//!
//! 1. reserve an ECS entity for every map uid (so parent references resolve),
//! 2. layer each entity's file components over its prototype's,
//! 3. build grids (tile chunks) and transforms, run component factories,
//! 4. anchor snap-grid entities, then run map-init spawners.

use std::collections::HashMap;
use std::sync::Arc;

use astrolabe::MapFile;
use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use grimoire::Node;
use vellum::ResPath;

use crate::content::Content;
use crate::spawn::{spawn_with, ProtoCache};
use crate::transform::{create_map, Grid, GridIndex, MapRoot, WorldPose, Xform};

#[derive(Debug, Default, Clone)]
pub struct LoadedMap {
    pub map: Option<Entity>,
    pub grids: Vec<Entity>,
    pub entities: HashMap<u32, Entity>,
    pub spawned_by_spawners: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct LoadOptions {
    /// Where the file's root grids land, in world coordinates of the map.
    pub offset: Vec2,
    pub rotation: f32,
    /// Run `ConditionalSpawner`/`RandomSpawner` markers after loading.
    pub run_spawners: bool,
}

impl Default for LoadOptions {
    fn default() -> Self {
        LoadOptions {
            offset: Vec2::ZERO,
            rotation: 0.0,
            run_spawners: true,
        }
    }
}

/// Load a map file from resources onto `map` (or a fresh map if `None`).
pub fn load_map_path(world: &mut World, path: &str, map: Option<Entity>, opts: LoadOptions) -> Result<LoadedMap, String> {
    let content = world.resource::<Content>().clone();
    let mut file = MapFile::load(&content.res, &ResPath::new(path)).map_err(|e| e.to_string())?;
    file.apply_migrations(&content.migrations);
    Ok(load_map(world, &file, map, opts))
}

pub fn load_map(world: &mut World, file: &MapFile, map: Option<Entity>, opts: LoadOptions) -> LoadedMap {
    let content = world.resource::<Content>().clone();
    let mut out = LoadedMap::default();
    let target_map = map.unwrap_or_else(|| create_map(world));
    out.map = Some(target_map);

    // The file's own map entities (whole-map saves) collapse onto target_map.
    let file_maps: Vec<u32> = file.maps().map(|e| e.uid).collect();

    for &m in &file_maps {
        out.entities.insert(m, target_map);
    }

    // Spawn in parent-before-child order: sort by hierarchy depth, then uid.
    let parent_of: HashMap<u32, Option<u32>> = file.entities.iter().map(|e| (e.uid, e.transform().parent)).collect();
    let mut depth: HashMap<u32, u32> = HashMap::new();
    fn depth_of(uid: u32, parent_of: &HashMap<u32, Option<u32>>, memo: &mut HashMap<u32, u32>, guard: u32) -> u32 {
        if let Some(d) = memo.get(&uid) {
            return *d;
        }
        let d = match parent_of.get(&uid).copied().flatten() {
            Some(p) if guard < 64 && parent_of.contains_key(&p) => depth_of(p, parent_of, memo, guard + 1) + 1,
            _ => 0,
        };
        memo.insert(uid, d);
        d
    }
    let mut order: Vec<&astrolabe::MapEntity> = file.entities.iter().filter(|e| !file_maps.contains(&e.uid)).collect();
    for e in &order {
        depth_of(e.uid, &parent_of, &mut depth, 0);
    }
    order.sort_by_key(|e| (depth[&e.uid], e.uid));

    let mut cache = world.remove_resource::<ProtoCache>().unwrap_or_default();
    for me in order {
        let comps: Arc<Vec<Node>> = if me.components.iter().all(|c| {
            // Only Transform/MetaData deltas? Share the prototype's list.
            matches!(c.get_str("type"), Some("Transform") | Some("MetaData"))
        }) && me.missing_components.is_empty()
            && me.proto.is_some()
        {
            match cache.components(&content, me.proto.as_deref().unwrap()) {
                Some(a) => a,
                None => {
                    out.warnings.push(format!("unknown prototype {:?}", me.proto));
                    continue;
                }
            }
        } else {
            Arc::new(me.effective_components(&content.protos))
        };

        let t = me.transform();
        let parent = t.parent.and_then(|p| out.entities.get(&p).copied());
        let is_grid = me.component("MapGrid").is_some();
        let (parent, pos, rot) = match parent {
            Some(p) => (Some(p), Vec2::new(t.pos.0, t.pos.1), t.rot as f32),
            // Roots (grids saved without a map) go onto the target map at the offset.
            None => (
                Some(target_map),
                opts.offset + crate::transform::rotate(Vec2::new(t.pos.0, t.pos.1), opts.rotation),
                t.rot as f32 + opts.rotation,
            ),
        };
        let parent = if parent.is_some_and(|p| file_maps.iter().any(|m| out.entities.get(m) == Some(&p))) {
            Some(target_map)
        } else {
            parent
        };
        let mut xform = Xform {
            parent,
            pos,
            rot,
            anchored: false,
        };
        if let Some(a) = t.anchored {
            xform.anchored = a;
        }

        let id = spawn_with(world, me.proto.as_deref(), comps, xform);
        out.entities.insert(me.uid, id);

        if is_grid {
            let mut grid = Grid::default();
            match file.grid_tiles(me, &content.tiles) {
                Ok(t) => grid.tiles = t,
                Err(e) => out.warnings.push(format!("grid {}: {e}", me.uid)),
            }
            let name = me.name().unwrap_or("grid").to_string();
            world.entity_mut(id).insert(grid);
            if let Some(mut m) = world.get_mut::<crate::spawn::MetaData>(id) {
                if m.name.is_empty() {
                    m.name = name;
                }
            }
            world.get_resource_or_init::<GridIndex>().grids.push(id);
            out.grids.push(id);
        }
        if me.component("Map").is_some() {
            world.entity_mut(id).insert(MapRoot { id: 0 });
        }
    }
    world.insert_resource(cache);

    // Anchored entities that were spawned before their grid existed (parents
    // are sorted first, so this is a safety net) get anchored now.
    let to_anchor: Vec<Entity> = out
        .entities
        .values()
        .copied()
        .filter(|&e| world.get::<Xform>(e).is_some_and(|x| x.anchored))
        .collect();
    for e in to_anchor {
        crate::spawn::anchor(world, e);
    }

    if opts.run_spawners {
        out.spawned_by_spawners = crate::spawners::run_spawners(world) + crate::storage::run_table_spawners(world);
    }
    crate::storage::resolve_fills(world);
    let _ = world.get_resource_or_init::<GridIndex>();
    let _ = WorldPose::default();
    out
}
