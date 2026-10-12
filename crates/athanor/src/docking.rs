//! Docking. Ports of `Content.Server/Shuttles/Systems/DockingSystem.cs`
//! (dock/undock, airlock handling), `DockingSystem.Shuttle.cs` (FTL-dock
//! placement: where a shuttle must sit for its docks to meet a grid's) and
//! `SharedDockingSystem` (range and alignment checks), with Frontier's dock
//! types and receive-only docks.
//!
//! Docks are airlocks with a `Docking` component. Two docks can join when
//! they are within `DOCK_RANGE` (1.2 m) and each faces the other to within
//! 15°. Docking:
//! - welds the grids together;
//! - opens and bolts both airlocks;
//! - stops them changing airtightness, so the open doors still hold air.
//!
//! Upstream's weld is a stiff physics joint (2 Hz, damping 0.7) that pulls
//! the docks' outer edges together, keeping the grids' relative angle at the
//! moment of docking. Here the weld is solved rigidly after shuttle motion:
//! - a grid that isn't a moving shuttle (a station) holds the other in place;
//! - two shuttles share momentum, and the lighter one follows the heavier.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use grimoire::Node;

use crate::doors::{try_close, try_open, Door};
use crate::shuttles::GridBody;
use crate::spawn::ComponentFactories;
use crate::transform::{angle_to_vec, rotate, tile_of, vec_to_angle, world_pose_of, Grid, Xform};

/// `SharedDockingSystem.DockRange`.
pub const DOCK_RANGE: f32 = 1.0 + 0.2;
/// `SharedDockingSystem.AlignmentTolerance` (15°).
pub const ALIGNMENT_TOLERANCE: f32 = 15.0 * std::f32::consts::PI / 180.0;

/// Frontier `DockType` flags.
pub mod dock_type {
    pub const AIRLOCK: u8 = 1;
    pub const GAS: u8 = 2;
    pub const TRANSIT: u8 = 4;
}

/// The weld between two docked grids (`WeldJoint` "docking{dockA}").
#[derive(Debug, Clone, Copy)]
pub struct Weld {
    pub grid_a: Entity,
    pub grid_b: Entity,
    /// Grid-local points that are held together (each dock's outer edge).
    pub anchor_a: Vec2,
    pub anchor_b: Vec2,
    /// rot(B) − rot(A) at the moment of docking.
    pub reference_angle: f32,
}

/// `DockingComponent`.
#[derive(Component, Debug, Clone)]
pub struct Docking {
    pub dock_type: u8,
    pub receive_only: bool,
    pub docked_with: Option<Entity>,
    pub name: Option<String>,
    /// Set on the "A" side of a docked pair.
    pub weld: Option<Weld>,
}

impl Docking {
    pub fn docked(&self) -> bool {
        self.docked_with.is_some()
    }
}

/// `PriorityDockComponent`: FTL docking prefers docks with a matching tag.
#[derive(Component, Debug, Clone)]
pub struct PriorityDock {
    pub tag: Option<String>,
}

fn parse_dock_type(n: Option<&Node>) -> u8 {
    let Some(n) = n else { return dock_type::AIRLOCK | dock_type::TRANSIT };
    let text = n.as_str().map(str::to_owned).unwrap_or_else(|| n.as_str_list().join(","));
    let mut t = 0;
    for part in text.split([',', '|', ' ']).map(str::trim) {
        t |= match part {
            "Airlock" => dock_type::AIRLOCK,
            "Gas" => dock_type::GAS,
            "Transit" => dock_type::TRANSIT,
            _ => 0,
        };
    }
    t
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("Docking", |e, node, _| {
        e.insert(Docking {
            dock_type: parse_dock_type(node.get("dockType")),
            receive_only: node.get("receiveOnly").and_then(Node::as_bool).unwrap_or(false),
            docked_with: None,
            name: node.get_str("name").map(str::to_owned),
            weld: None,
        });
    });
    reg.register("PriorityDock", |e, node, _| {
        e.insert(PriorityDock { tag: node.get_str("tag").map(str::to_owned) });
    });
}

/// `Angle.ShortestDistance(a, 0)` magnitude after `Reduced()`.
fn angle_off(a: f32) -> f32 {
    let r = (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    r.abs()
}

/// `SharedDockingSystem.CanDock(mapPosA, rotA, mapPosB, rotB)`.
pub fn can_dock_poses(pos_a: Vec2, rot_a: f32, pos_b: Vec2, rot_b: f32) -> bool {
    if pos_a.distance(pos_b) > DOCK_RANGE {
        return false;
    }
    // Each dock must face the other.
    let to_b = vec_to_angle(pos_b - pos_a);
    let to_a = vec_to_angle(pos_a - pos_b);
    angle_off(rot_a - to_b) <= ALIGNMENT_TOLERANCE && angle_off(rot_b - to_a) <= ALIGNMENT_TOLERANCE
}

/// `DockingSystem.CanDock(dockA, dockB)`.
pub fn can_dock(world: &World, a: Entity, b: Entity) -> bool {
    let (Some(da), Some(db)) = (world.get::<Docking>(a), world.get::<Docking>(b)) else { return false };
    if da.docked() || db.docked() || da.dock_type & db.dock_type == 0 {
        return false;
    }
    let (Some(xa), Some(xb)) = (world.get::<Xform>(a), world.get::<Xform>(b)) else { return false };
    if !xa.anchored || !xb.anchored {
        return false;
    }
    let (pa, pb) = (world_pose_of(world, a), world_pose_of(world, b));
    if pa.map.is_none() || pa.map != pb.map {
        return false;
    }
    can_dock_poses(pa.pos, pa.rot, pb.pos, pb.rot)
}

/// `DockingSystem.Dock`.
pub fn dock(world: &mut World, a: Entity, b: Entity) {
    // Upstream orders the pair by uid hash.
    let (a, b) = if b.index() < a.index() { (b, a) } else { (a, b) };
    let (Some(xa), Some(xb)) = (world.get::<Xform>(a).copied(), world.get::<Xform>(b).copied()) else { return };
    let (Some(grid_a), Some(grid_b)) = (xa.parent, xb.parent) else { return };
    let reference_angle = world_pose_of(world, grid_b).rot - world_pose_of(world, grid_a).rot;
    let weld = Weld {
        grid_a,
        grid_b,
        anchor_a: xa.pos + angle_to_vec(xa.rot) / 2.0,
        anchor_b: xb.pos + angle_to_vec(xb.rot) / 2.0,
        reference_angle,
    };
    if let Some(mut d) = world.get_mut::<Docking>(a) {
        d.docked_with = Some(b);
        d.weld = Some(weld);
    }
    if let Some(mut d) = world.get_mut::<Docking>(b) {
        d.docked_with = Some(a);
        d.weld = None;
    }
    for door in [a, b] {
        if world.get::<Door>(door).is_none() {
            continue;
        }
        if try_open(world, door, None)
            && let Some(mut d) = world.get_mut::<Door>(door)
        {
            d.bolted = true;
        }
        if let Some(mut d) = world.get_mut::<Door>(door) {
            d.change_airtight = false;
        }
    }
}

/// `DockingSystem.Undock`.
pub fn undock(world: &mut World, dock: Entity) {
    let Some(other) = world.get::<Docking>(dock).and_then(|d| d.docked_with) else { return };
    for door in [dock, other] {
        if world.get_entity(door).is_err() {
            continue;
        }
        if let Some(mut d) = world.get_mut::<Door>(door) {
            d.bolted = false;
        }
        if world.get::<Door>(door).is_some()
            && try_close(world, door, None)
            && let Some(mut d) = world.get_mut::<Door>(door)
        {
            d.change_airtight = true;
        }
        if let Some(mut d) = world.get_mut::<Docking>(door) {
            d.docked_with = None;
            d.weld = None;
        }
    }
}

/// Every dock on a grid (`GetDocks`).
pub fn docks_on(world: &mut World, grid: Entity) -> Vec<Entity> {
    let mut q = world.query::<(Entity, &Docking, &Xform)>();
    let mut v: Vec<Entity> = q.iter(world).filter(|(_, _, x)| x.parent == Some(grid)).map(|(e, _, _)| e).collect();
    v.sort();
    v
}

/// Grids welded to `grid` by a dock.
pub fn docked_grids(world: &World, grid: Entity) -> Vec<Entity> {
    let mut out = Vec::new();
    let Some(mut q) = world.try_query::<(&Docking, &Xform)>() else { return out };
    for (d, x) in q.iter(world) {
        let Some(other) = d.docked_with else { continue };
        if x.parent != Some(grid) {
            continue;
        }
        if let Some(og) = world.get::<Xform>(other).and_then(|x| x.parent)
            && !out.contains(&og)
        {
            out.push(og);
        }
    }
    out
}

/// Undock every dock on a grid (`UndockDocks`).
pub fn undock_all(world: &mut World, grid: Entity) -> usize {
    let mut n = 0;
    for d in docks_on(world, grid) {
        if world.get::<Docking>(d).is_some_and(Docking::docked) {
            undock(world, d);
            n += 1;
        }
    }
    n
}

/// A pilot's dock request: join the best pair of in-range docks between this
/// shuttle and any other grid (the console's dock list, picked for you).
pub fn try_dock_nearest(world: &mut World, shuttle: Entity) -> bool {
    let ours = docks_on(world, shuttle);
    let all: Vec<Entity> = {
        let mut q = world.query::<(Entity, &Docking)>();
        q.iter(world).map(|(e, _)| e).collect()
    };
    let mut best: Option<(f32, Entity, Entity)> = None;
    for &a in &ours {
        if world.get::<Docking>(a).is_some_and(|d| d.receive_only) {
            continue;
        }
        for &b in &all {
            if world.get::<Xform>(b).and_then(|x| x.parent) == Some(shuttle) || !can_dock(world, a, b) {
                continue;
            }
            let d = world_pose_of(world, a).pos.distance(world_pose_of(world, b).pos);
            if best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, a, b));
            }
        }
    }
    match best {
        Some((_, a, b)) => {
            dock(world, a, b);
            true
        }
        None => false,
    }
}

/// Hold welded grids together. Runs after shuttle motion.
pub fn solve_welds(world: &mut World) {
    let welds: Vec<Weld> = {
        let mut q = world.query::<&Docking>();
        q.iter(world).filter_map(|d| d.weld).collect()
    };
    for w in welds {
        let movable = crate::shuttles::is_movable;
        let (ma, mb) = (movable(world, w.grid_a), movable(world, w.grid_b));
        // Which grid follows the other?
        let (anchor, follower, a_is_anchor) = match (ma, mb) {
            (false, false) => continue,
            (false, true) => (w.grid_a, w.grid_b, true),
            (true, false) => (w.grid_b, w.grid_a, false),
            (true, true) => {
                let mass = |g| world.get::<GridBody>(g).map_or(0.0, |b| b.mass);
                if mass(w.grid_a) >= mass(w.grid_b) { (w.grid_a, w.grid_b, true) } else { (w.grid_b, w.grid_a, false) }
            }
        };
        let (Some(xa), Some(xf)) = (world.get::<Xform>(anchor).copied(), world.get::<Xform>(follower).copied()) else { continue };
        if xa.parent != xf.parent {
            continue;
        }
        let (anchor_local, follower_local, rel) =
            if a_is_anchor { (w.anchor_a, w.anchor_b, w.reference_angle) } else { (w.anchor_b, w.anchor_a, -w.reference_angle) };
        let rot = xa.rot + rel;
        let meet = xa.pos + rotate(anchor_local, xa.rot);
        let pos = meet - rotate(follower_local, rot);
        if let Some(mut x) = world.get_mut::<Xform>(follower) {
            x.pos = pos;
            x.rot = rot;
        }
        // Velocities: a station holds the shuttle still; two shuttles share momentum.
        let anchor_body = world.get::<GridBody>(anchor).cloned();
        let follower_body = world.get::<GridBody>(follower).cloned();
        match (ma && mb, anchor_body, follower_body) {
            (true, Some(mut ab), Some(mut fb)) => {
                let m = (ab.mass + fb.mass).max(1e-3);
                let v = (ab.velocity * ab.mass + fb.velocity * fb.mass) / m;
                let i = (ab.inertia + fb.inertia).max(1e-3);
                let w_ = (ab.angular_velocity * ab.inertia + fb.angular_velocity * fb.inertia) / i;
                ab.velocity = v;
                fb.velocity = v;
                ab.angular_velocity = w_;
                fb.angular_velocity = w_;
                world.entity_mut(anchor).insert(ab);
                world.entity_mut(follower).insert(fb);
            }
            (_, _, Some(mut fb)) => {
                fb.velocity = Vec2::ZERO;
                fb.angular_velocity = 0.0;
                world.entity_mut(follower).insert(fb);
            }
            _ => {}
        }
    }
}

/// `DockingConfig`: where a shuttle goes to dock with a grid.
#[derive(Debug, Clone)]
pub struct DockingConfig {
    /// (shuttle dock, grid dock) pairs that meet.
    pub docks: Vec<(Entity, Entity)>,
    /// Shuttle origin in the target grid's local frame.
    pub grid_position: Vec2,
    /// Shuttle rotation relative to the target grid.
    pub angle: f32,
    /// Docked AABB in grid-local space, rounded to 2 digits.
    pub area: (i64, i64, i64, i64),
}

/// Tiles of a grid as unit boxes: (min corner) list.
fn tiles_of(world: &World, grid: Entity) -> Vec<Vec2> {
    world.get::<Grid>(grid).map(|g| g.tiles.iter().map(|((x, y), _)| Vec2::new(x as f32, y as f32)).collect()).unwrap_or_default()
}

/// Shuttle-local → target-grid-local transform for a pair of docks
/// (`CanDock`'s `matty`): returns (origin, angle).
fn dock_transform(shuttle_dock: &Xform, grid_dock: &Xform) -> (Vec2, f32) {
    let station_dock_pos = shuttle_dock.pos + rotate(Vec2::new(0.0, -1.0), shuttle_dock.rot);
    let shuttle_dock_angle = shuttle_dock.rot;
    let grid_dock_angle = grid_dock.rot + std::f32::consts::PI;
    let offset = grid_dock_angle - shuttle_dock_angle;
    let origin = grid_dock.pos + rotate(-station_dock_pos, offset);
    (origin, offset)
}

fn reduced(a: f32) -> f32 {
    a.rem_euclid(std::f32::consts::TAU)
}

/// The docked shuttle's AABB in target-grid space.
fn docked_aabb(tiles: &[Vec2], origin: Vec2, angle: f32) -> (Vec2, Vec2) {
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for t in tiles {
        for c in [*t, *t + Vec2::X, *t + Vec2::ONE, *t + Vec2::Y] {
            let p = origin + rotate(c, angle);
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    (lo, hi)
}

/// `ValidSpawn`: no shuttle tile may overlap a tile of the target grid.
fn valid_spawn(target: &Grid, tiles: &[Vec2], origin: Vec2, angle: f32) -> bool {
    for t in tiles {
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for c in [*t, *t + Vec2::X, *t + Vec2::ONE, *t + Vec2::Y] {
            let p = origin + rotate(c, angle);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        lo += Vec2::splat(0.01);
        hi -= Vec2::splat(0.01);
        for x in lo.x.floor() as i32..=(hi.x.ceil() as i32 - 1) {
            for y in lo.y.floor() as i32..=(hi.y.ceil() as i32 - 1) {
                if target.has_tile(bevy_math::IVec2::new(x, y)) {
                    return false;
                }
            }
        }
    }
    true
}

/// World AABB of a grid.
fn world_aabb(world: &World, g: Entity) -> Option<(Vec2, Vec2)> {
    let grid = world.get::<Grid>(g)?;
    let (x0, y0, x1, y1) = grid.tiles.bounds()?;
    let p = world_pose_of(world, g);
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for c in [Vec2::new(x0 as f32, y0 as f32), Vec2::new(x1 as f32 + 1.0, y0 as f32), Vec2::new(x1 as f32 + 1.0, y1 as f32 + 1.0), Vec2::new(x0 as f32, y1 as f32 + 1.0)] {
        let w = p.pos + rotate(c, p.rot);
        lo = lo.min(w);
        hi = hi.max(w);
    }
    Some((lo, hi))
}

/// `GetDockingConfigs`.
pub fn docking_configs(world: &mut World, shuttle: Entity, target: Entity, want: u8) -> Vec<DockingConfig> {
    let shuttle_docks = docks_on(world, shuttle);
    let grid_docks = docks_on(world, target);
    let mut out = Vec::new();
    if grid_docks.is_empty() {
        return out;
    }
    let tiles = tiles_of(world, shuttle);
    let tp = world_pose_of(world, target);
    let target_angle_world = reduced(tp.rot);
    let others: Vec<(Vec2, Vec2)> = world
        .get_resource::<crate::transform::GridIndex>()
        .map(|i| i.grids.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|g| *g != target && *g != shuttle && world_pose_of(world, *g).map == tp.map)
        .filter_map(|g| world_aabb(world, g))
        .collect();
    let Some(tgrid) = world.get::<Grid>(target) else { return out };
    let eligible = |world: &World, sd: Entity, gd: Entity| -> Option<(Vec2, f32)> {
        let (a, b) = (world.get::<Docking>(sd)?, world.get::<Docking>(gd)?);
        if want != 0 && a.dock_type & want == 0 {
            return None;
        }
        if a.dock_type & b.dock_type == 0 || a.docked() || b.docked() {
            return None;
        }
        let (xa, xb) = (world.get::<Xform>(sd)?, world.get::<Xform>(gd)?);
        if !xa.anchored || !xb.anchored {
            return None;
        }
        let (origin, angle) = dock_transform(xa, xb);
        valid_spawn(tgrid, &tiles, origin, angle).then_some((origin, angle))
    };
    let round = |(lo, hi): (Vec2, Vec2)| {
        let r = |v: f32| (v * 100.0).round() as i64;
        (r(lo.x), r(lo.y), r(hi.x), r(hi.y))
    };
    for &sd in &shuttle_docks {
        for &gd in &grid_docks {
            let Some((origin, angle)) = eligible(world, sd, gd) else { continue };
            // The docked shuttle must not hit any other grid.
            let world_angle = reduced(target_angle_world + angle);
            let spawn = tp.pos + rotate(origin, tp.rot);
            let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
            for t in &tiles {
                for c in [*t, *t + Vec2::X, *t + Vec2::ONE, *t + Vec2::Y] {
                    let p = spawn + rotate(c, world_angle);
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
            if others.iter().any(|(a, b)| a.x < hi.x && b.x > lo.x && a.y < hi.y && b.y > lo.y) {
                continue;
            }
            let area = round(docked_aabb(&tiles, origin, angle));
            let mut docks = vec![(sd, gd)];
            for &osd in &shuttle_docks {
                if osd == sd {
                    continue;
                }
                for &ogd in &grid_docks {
                    if ogd == gd {
                        continue;
                    }
                    let Some((o2, a2)) = eligible(world, osd, ogd) else { continue };
                    if (a2 - angle).abs() > 1e-4 || round(docked_aabb(&tiles, o2, a2)) != area {
                        continue;
                    }
                    docks.push((osd, ogd));
                }
            }
            out.push(DockingConfig { docks, grid_position: origin, angle, area });
        }
    }
    out
}

/// `GetDockingConfig(shuttle, target, priorityTag, dockType)`.
pub fn docking_config(world: &mut World, shuttle: Entity, target: Entity, priority_tag: Option<&str>, want: u8) -> Option<DockingConfig> {
    let mut configs = docking_configs(world, shuttle, target, want);
    if configs.is_empty() {
        return None;
    }
    let target_angle = reduced(world_pose_of(world, target).rot);
    let priority = |world: &World, c: &DockingConfig| {
        c.docks.iter().any(|(_, gd)| {
            world.get::<PriorityDock>(*gd).and_then(|p| p.tag.as_deref()).is_some_and(|t| Some(t) == priority_tag)
        })
    };
    let keyed: Vec<(bool, usize, f32)> =
        configs.iter().map(|c| (priority(world, c), c.docks.len(), angle_off(reduced(c.angle) - target_angle))).collect();
    let mut idx: Vec<usize> = (0..configs.len()).collect();
    // Stable: priority first, then most docks, then closest to the grid's angle.
    idx.sort_by(|&i, &j| {
        let (a, b) = (keyed[i], keyed[j]);
        b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.total_cmp(&b.2))
    });
    Some(configs.swap_remove(idx[0]))
}

/// `ShuttleSystem.FTLDock`: put the shuttle in place and join its docks.
pub fn ftl_dock(world: &mut World, shuttle: Entity, target: Entity, config: &DockingConfig) {
    let tp = world_pose_of(world, target);
    let Some(map) = tp.map else { return };
    if let Some(mut x) = world.get_mut::<Xform>(shuttle) {
        x.parent = Some(map);
        x.pos = tp.pos + rotate(config.grid_position, tp.rot);
        x.rot = config.angle + tp.rot;
    }
    if let Some(mut b) = world.get_mut::<GridBody>(shuttle) {
        b.velocity = Vec2::ZERO;
        b.angular_velocity = 0.0;
    }
    for &(a, b) in &config.docks {
        dock(world, a, b);
    }
}

/// `ShuttleSystem.TryFTLDock` (without the proximity fallback).
pub fn try_ftl_dock(world: &mut World, shuttle: Entity, target: Entity, priority_tag: Option<&str>) -> bool {
    match docking_config(world, shuttle, target, priority_tag, 0) {
        Some(c) => {
            ftl_dock(world, shuttle, target, &c);
            true
        }
        None => false,
    }
}

/// A dock's tile on its grid (for UIs and tests).
pub fn dock_tile(world: &World, dock: Entity) -> Option<bevy_math::IVec2> {
    world.get::<Xform>(dock).map(|x| tile_of(x.pos))
}
