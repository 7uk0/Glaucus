//! Transforms, maps and grids. Port of Robust's `TransformComponent`,
//! `MapComponent`/`MapGridComponent` and the anchoring rules of
//! `SharedTransformSystem`.
//!
//! Every entity has an [`Xform`] whose position/rotation are *local to its
//! parent*: items and mobs are parented to the grid they stand on, grids to
//! their map, maps to nothing. That is what lets a whole shuttle — and
//! everything on it — move by changing one transform.

use std::collections::HashMap;

use astrolabe::GridTiles;
use bevy_ecs::prelude::*;
use bevy_math::{IVec2, Vec2};

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Xform {
    pub parent: Option<Entity>,
    pub pos: Vec2,
    /// Radians, Robust convention: 0 faces south, CCW positive.
    pub rot: f32,
    pub anchored: bool,
}

impl Xform {
    pub fn at(parent: Entity, pos: Vec2) -> Self {
        Xform {
            parent: Some(parent),
            pos,
            rot: 0.0,
            anchored: false,
        }
    }

    pub fn root() -> Self {
        Xform {
            parent: None,
            pos: Vec2::ZERO,
            rot: 0.0,
            anchored: false,
        }
    }
}

/// World-space pose, recomputed every tick by [`update_world_poses`].
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct WorldPose {
    pub pos: Vec2,
    pub rot: f32,
    /// The map entity this entity is on.
    pub map: Option<Entity>,
    /// The grid this entity is on (itself, for grids).
    pub grid: Option<Entity>,
}

/// A map: an infinite 2D space that grids float in.
#[derive(Component, Debug, Clone, Copy)]
pub struct MapRoot {
    pub id: u32,
}

/// A grid of tiles (a station, a shuttle, an asteroid).
#[derive(Component, Debug, Default)]
pub struct Grid {
    pub tiles: GridTiles,
    /// Anchored entities per tile (walls, doors, machines). Port of
    /// Robust's `MapGridComponent` anchored-entity lookup.
    pub anchors: HashMap<IVec2, Vec<Entity>>,
    /// Bumped whenever tiles or anchors change, so caches (pathfinding,
    /// rendering) can invalidate.
    pub revision: u64,
}

impl Grid {
    pub fn anchored_at(&self, tile: IVec2) -> &[Entity] {
        self.anchors.get(&tile).map_or(&[], Vec::as_slice)
    }

    pub fn anchor(&mut self, tile: IVec2, e: Entity) {
        let v = self.anchors.entry(tile).or_default();
        if !v.contains(&e) {
            v.push(e);
            self.revision += 1;
        }
    }

    pub fn unanchor(&mut self, tile: IVec2, e: Entity) {
        if let Some(v) = self.anchors.get_mut(&tile) {
            v.retain(|x| *x != e);
            if v.is_empty() {
                self.anchors.remove(&tile);
            }
            self.revision += 1;
        }
    }

    pub fn has_tile(&self, tile: IVec2) -> bool {
        !self.tiles.get(tile.x, tile.y).is_empty()
    }
}

/// Every live grid entity. Grid counts are small (tens), so lookups that
/// must consider "any grid under this point" just walk this list.
#[derive(Resource, Debug, Default, Clone)]
pub struct GridIndex {
    pub grids: Vec<Entity>,
}

/// Every map entity, keyed by map id.
#[derive(Resource, Debug, Default, Clone)]
pub struct MapIndex {
    pub maps: HashMap<u32, Entity>,
    pub next_id: u32,
}

/// Create a new empty map and return its entity.
pub fn create_map(world: &mut World) -> Entity {
    let id = {
        let mut idx = world.get_resource_or_insert_with(MapIndex::default);
        idx.next_id += 1;
        idx.next_id
    };
    let e = world
        .spawn((MapRoot { id }, Xform::root(), WorldPose::default(), crate::spawn::MetaData::named("map")))
        .id();
    world.resource_mut::<MapIndex>().maps.insert(id, e);
    e
}

/// Create an empty grid on a map at a world position.
pub fn create_grid(world: &mut World, map: Entity, pos: Vec2, name: &str) -> Entity {
    let e = world
        .spawn((
            Grid::default(),
            Xform::at(map, pos),
            WorldPose::default(),
            crate::spawn::MetaData::named(name),
        ))
        .id();
    world.get_resource_or_insert_with(GridIndex::default).grids.push(e);
    e
}

/// Which tile a grid-local position falls in (tiles span `[x, x+1)`).
pub fn tile_of(local: Vec2) -> IVec2 {
    IVec2::new(local.x.floor() as i32, local.y.floor() as i32)
}

/// Center of a tile in grid-local coordinates.
pub fn tile_center(tile: IVec2) -> Vec2 {
    Vec2::new(tile.x as f32 + 0.5, tile.y as f32 + 0.5)
}

pub fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// World → local for a pose.
pub fn to_local(pose: &WorldPose, world: Vec2) -> Vec2 {
    rotate(world - pose.pos, -pose.rot)
}

/// Local → world for a pose.
pub fn to_world(pose: &WorldPose, local: Vec2) -> Vec2 {
    pose.pos + rotate(local, pose.rot)
}

/// Robust's `Angle.ToWorldVec()`: the unit vector an angle faces.
pub fn angle_to_vec(angle: f32) -> Vec2 {
    rotate(Vec2::new(0.0, -1.0), angle)
}

/// Inverse of [`angle_to_vec`].
pub fn vec_to_angle(v: Vec2) -> f32 {
    // angle_to_vec(a) = (sin a, -cos a)
    v.x.atan2(-v.y)
}

/// Compute world poses for everything. Parents are resolved recursively with
/// memoisation; hierarchy depth in practice is ≤ 4 (map → grid → mob → item).
pub fn update_world_poses(
    mut q: Query<(Entity, &Xform, &mut WorldPose)>,
    maps: Query<(), With<MapRoot>>,
    grids: Query<(), With<Grid>>,
    mut cache: Local<HashMap<Entity, WorldPose>>,
) {
    cache.clear();
    let xforms: HashMap<Entity, Xform> = q.iter().map(|(e, x, _)| (e, *x)).collect();

    fn resolve(
        e: Entity,
        xforms: &HashMap<Entity, Xform>,
        maps: &Query<(), With<MapRoot>>,
        grids: &Query<(), With<Grid>>,
        cache: &mut HashMap<Entity, WorldPose>,
        depth: u32,
    ) -> WorldPose {
        if let Some(p) = cache.get(&e) {
            return *p;
        }
        let Some(x) = xforms.get(&e) else {
            return WorldPose::default();
        };
        let mut pose = match x.parent {
            Some(parent) if depth < 16 => {
                let pp = resolve(parent, xforms, maps, grids, cache, depth + 1);
                WorldPose {
                    pos: to_world(&pp, x.pos),
                    rot: pp.rot + x.rot,
                    map: pp.map,
                    grid: pp.grid,
                }
            }
            _ => WorldPose {
                pos: x.pos,
                rot: x.rot,
                map: None,
                grid: None,
            },
        };
        if maps.contains(e) {
            pose.map = Some(e);
        }
        if grids.contains(e) {
            pose.grid = Some(e);
        }
        cache.insert(e, pose);
        pose
    }

    for (e, _, mut wp) in q.iter_mut() {
        let p = resolve(e, &xforms, &maps, &grids, &mut cache, 0);
        if *wp != p {
            *wp = p;
        }
    }
}

/// Compute one entity's world pose on demand (outside the per-tick pass).
pub fn world_pose_of(world: &World, e: Entity) -> WorldPose {
    let mut chain = Vec::new();
    let mut cur = Some(e);
    while let Some(c) = cur {
        if chain.len() > 16 {
            break;
        }
        let Some(x) = world.get::<Xform>(c) else { break };
        chain.push((c, *x));
        cur = x.parent;
    }
    let mut pose = WorldPose::default();
    for (ent, x) in chain.into_iter().rev() {
        pose = WorldPose {
            pos: to_world(&pose, x.pos),
            rot: pose.rot + x.rot,
            map: pose.map,
            grid: pose.grid,
        };
        if x.parent.is_none() {
            pose.pos = x.pos;
            pose.rot = x.rot;
        }
        if world.get::<MapRoot>(ent).is_some() {
            pose.map = Some(ent);
        }
        if world.get::<Grid>(ent).is_some() {
            pose.grid = Some(ent);
        }
    }
    pose
}

/// Re-parent a free entity onto whichever grid lies under it (or the map, if
/// none), preserving its world pose. Port of Robust's grid traversal.
pub fn reparent_to_grid(world: &mut World, e: Entity) {
    let pose = world_pose_of(world, e);
    let Some(map) = pose.map else { return };
    let current_parent = world.get::<Xform>(e).and_then(|x| x.parent);
    let target = find_grid_at(world, map, pose.pos).unwrap_or(map);
    if Some(target) == current_parent {
        return;
    }
    let tp = world_pose_of(world, target);
    if let Some(mut x) = world.get_mut::<Xform>(e) {
        x.parent = Some(target);
        x.pos = to_local(&tp, pose.pos);
        x.rot = pose.rot - tp.rot;
    }
}

/// The grid on `map` with a non-space tile at world position `pos`.
pub fn find_grid_at(world: &World, map: Entity, pos: Vec2) -> Option<Entity> {
    let mut found = None;
    let grids = world.get_resource::<GridIndex>().map(|g| g.grids.clone()).unwrap_or_default();
    for g in grids {
        let gp = world_pose_of(world, g);
        if gp.map != Some(map) {
            continue;
        }
        let local = to_local(&gp, pos);
        if world.get::<Grid>(g).is_some_and(|grid| grid.has_tile(tile_of(local))) {
            found = Some(g);
            break;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn angle_vec_roundtrip() {
        let south = angle_to_vec(0.0);
        assert!((south - Vec2::new(0.0, -1.0)).length() < 1e-6);
        let east = angle_to_vec(FRAC_PI_2);
        assert!((east - Vec2::new(1.0, 0.0)).length() < 1e-6, "{east}");
        for a in [0.0f32, 0.5, 1.5, 3.0, -2.0] {
            let v = angle_to_vec(a);
            let back = vec_to_angle(v);
            assert!((angle_to_vec(back) - v).length() < 1e-5);
        }
    }

    #[test]
    fn tiles() {
        assert_eq!(tile_of(Vec2::new(-0.5, -1.5)), IVec2::new(-1, -2));
        assert_eq!(tile_center(IVec2::new(-1, -2)), Vec2::new(-0.5, -1.5));
    }
}
