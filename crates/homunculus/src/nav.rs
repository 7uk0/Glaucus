//! Navigation: A* over grid tiles, line of sight, reachable-tile flood fill,
//! and path-following steering. A simplified port of upstream's
//! `PathfindingSystem` (which uses a polygon navmesh per tile) and
//! `NPCSteeringSystem` (context steering).
//!
//! Walkability: a tile must exist and hold no hard fixture that blocks the
//! mover's mask — except doors, which NPCs bump open (upstream's NavInteract
//! flag), at a small extra cost.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

use athanor::doors::Door;
use athanor::movement::InputMover;
use athanor::physics::{group, Body, Fixtures};
use athanor::transform::{tile_center, tile_of, to_local, world_pose_of, Grid, Xform};
use athanor::SimClock;
use bevy_ecs::prelude::*;
use bevy_math::{IVec2, Vec2};

pub const MAX_NODES: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Walk {
    Open,
    Door,
    Blocked,
}

pub fn tile_walk(world: &World, grid: &Grid, tile: IVec2, mask: u32) -> Walk {
    if !grid.has_tile(tile) {
        return Walk::Blocked;
    }
    let mut door = false;
    for &e in grid.anchored_at(tile) {
        let (Some(b), Some(f)) = (world.get::<Body>(e), world.get::<Fixtures>(e)) else { continue };
        let hard = f.0.iter().any(|fx| fx.hard && (mask & fx.layer) != 0);
        if !hard {
            continue;
        }
        if world.get::<Door>(e).is_some() {
            door = true;
            continue;
        }
        if b.can_collide {
            return Walk::Blocked;
        }
    }
    if door { Walk::Door } else { Walk::Open }
}

#[derive(Copy, Clone, PartialEq)]
struct Node {
    f: f32,
    tile: IVec2,
}
impl Eq for Node {}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.partial_cmp(&self.f).unwrap_or(Ordering::Equal).then_with(|| (o.tile.x, o.tile.y).cmp(&(self.tile.x, self.tile.y)))
    }
}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

fn octile(a: IVec2, b: IVec2) -> f32 {
    let d = (a - b).abs();
    let (mn, mx) = (d.x.min(d.y) as f32, d.x.max(d.y) as f32);
    mx + (std::f32::consts::SQRT_2 - 1.0) * mn
}

/// A* from `from` to any tile whose centre is within `range` of `goal_pos`
/// (grid-local). Returns the tiles to walk through, excluding `from`.
pub fn find_path(world: &World, grid_e: Entity, from: IVec2, goal_pos: Vec2, range: f32, mask: u32) -> Option<Vec<IVec2>> {
    let grid = world.get::<Grid>(grid_e)?;
    let goal = tile_of(goal_pos);
    let in_goal = |t: IVec2| tile_center(t).distance(goal_pos) <= range.max(0.5) || t == goal;
    if in_goal(from) {
        return Some(Vec::new());
    }
    let mut open = BinaryHeap::new();
    let mut g: HashMap<IVec2, f32> = HashMap::new();
    let mut came: HashMap<IVec2, IVec2> = HashMap::new();
    g.insert(from, 0.0);
    open.push(Node {
        f: octile(from, goal),
        tile: from,
    });
    let mut walk_cache: HashMap<IVec2, Walk> = HashMap::new();
    let mut walk = |t: IVec2| *walk_cache.entry(t).or_insert_with(|| tile_walk(world, grid, t, mask));
    let mut expanded = 0;
    while let Some(Node { tile, .. }) = open.pop() {
        if in_goal(tile) {
            let mut path = vec![tile];
            let mut cur = tile;
            while let Some(&p) = came.get(&cur) {
                if p == from {
                    break;
                }
                path.push(p);
                cur = p;
            }
            path.reverse();
            return Some(path);
        }
        expanded += 1;
        if expanded > MAX_NODES {
            return None;
        }
        let gc = g[&tile];
        for d in [
            IVec2::new(1, 0),
            IVec2::new(-1, 0),
            IVec2::new(0, 1),
            IVec2::new(0, -1),
            IVec2::new(1, 1),
            IVec2::new(1, -1),
            IVec2::new(-1, 1),
            IVec2::new(-1, -1),
        ] {
            let n = tile + d;
            let w = walk(n);
            if w == Walk::Blocked {
                continue;
            }
            let diagonal = d.x != 0 && d.y != 0;
            if diagonal && (walk(tile + IVec2::new(d.x, 0)) != Walk::Open || walk(tile + IVec2::new(0, d.y)) != Walk::Open) {
                continue;
            }
            let step = if diagonal { std::f32::consts::SQRT_2 } else { 1.0 } + if w == Walk::Door { 2.0 } else { 0.0 };
            let ng = gc + step;
            if g.get(&n).is_none_or(|&old| ng < old) {
                g.insert(n, ng);
                came.insert(n, tile);
                open.push(Node {
                    f: ng + octile(n, goal),
                    tile: n,
                });
            }
        }
    }
    None
}

/// Every walkable tile reachable within `range` tiles of `from` (BFS).
pub fn reachable_tiles(world: &World, grid_e: Entity, from: IVec2, range: f32, mask: u32) -> Vec<IVec2> {
    let Some(grid) = world.get::<Grid>(grid_e) else { return Vec::new() };
    let mut seen = HashSet::from([from]);
    let mut q = VecDeque::from([from]);
    let mut out = Vec::new();
    while let Some(t) = q.pop_front() {
        out.push(t);
        if out.len() > MAX_NODES {
            break;
        }
        for d in [IVec2::X, -IVec2::X, IVec2::Y, -IVec2::Y] {
            let n = t + d;
            if seen.contains(&n) || (n - from).as_vec2().length() > range {
                continue;
            }
            seen.insert(n);
            if tile_walk(world, grid, n, mask) != Walk::Blocked {
                q.push_back(n);
            }
        }
    }
    out
}

/// Line of sight between two grid-local points: no opaque, hard, colliding
/// anchored fixture on the tiles between (DDA tile walk).
pub fn line_of_sight(world: &World, grid_e: Entity, a: Vec2, b: Vec2) -> bool {
    let Some(grid) = world.get::<Grid>(grid_e) else { return false };
    let dist = a.distance(b);
    let steps = (dist * 4.0).ceil() as i32;
    let start = tile_of(a);
    let end = tile_of(b);
    let mut last = IVec2::new(i32::MIN, i32::MIN);
    for i in 1..steps {
        let p = a.lerp(b, i as f32 / steps as f32);
        let t = tile_of(p);
        if t == last || t == start || t == end {
            continue;
        }
        last = t;
        for &e in grid.anchored_at(t) {
            let (Some(body), Some(f)) = (world.get::<Body>(e), world.get::<Fixtures>(e)) else { continue };
            if body.can_collide && f.0.iter().any(|fx| fx.hard && fx.layer & group::OPAQUE != 0) {
                return false;
            }
        }
    }
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteeringStatus {
    Moving,
    InRange,
    NoPath,
}

/// `NPCSteeringComponent`: where an NPC is walking to.
#[derive(Component, Debug, Clone)]
pub struct NpcSteering {
    pub target: Option<SteerTarget>,
    pub range: f32,
    pub path: Vec<IVec2>,
    pub status: SteeringStatus,
    pub last_progress_time: f64,
    pub last_pos: Vec2,
    pub repath_at: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SteerTarget {
    Coords { parent: Entity, pos: Vec2 },
    Entity(Entity),
}

impl Default for NpcSteering {
    fn default() -> Self {
        NpcSteering {
            target: None,
            range: 1.5,
            path: Vec::new(),
            status: SteeringStatus::Moving,
            last_progress_time: 0.0,
            last_pos: Vec2::ZERO,
            repath_at: 0.0,
        }
    }
}

impl NpcSteering {
    pub fn go(&mut self, target: SteerTarget, range: f32, path: Option<Vec<IVec2>>, now: f64) {
        self.target = Some(target);
        self.range = range;
        self.path = path.unwrap_or_default();
        self.status = SteeringStatus::Moving;
        self.last_progress_time = now;
        self.repath_at = if self.path.is_empty() { now } else { now + 2.0 };
    }

    pub fn stop(&mut self) {
        self.target = None;
        self.path.clear();
    }
}

fn mover_mask(world: &World, e: Entity) -> u32 {
    world
        .get::<Fixtures>(e)
        .map(|f| f.mask())
        .filter(|m| *m != 0)
        .unwrap_or_else(|| group::by_name("MobMask").unwrap())
}

/// Resolve a steering target to the owner's parent frame.
fn target_local(world: &World, owner_parent: Entity, t: SteerTarget) -> Option<Vec2> {
    let world_pos = match t {
        SteerTarget::Coords { parent, pos } => {
            let pp = world_pose_of(world, parent);
            athanor::transform::to_world(&pp, pos)
        }
        SteerTarget::Entity(e) => {
            world.get_entity(e).ok()?;
            world_pose_of(world, e).pos
        }
    };
    let op = world_pose_of(world, owner_parent);
    Some(to_local(&op, world_pos))
}

/// Drive `InputMover`s along steering paths.
pub fn steering_system(world: &mut World) {
    let now = world.resource::<SimClock>().time;
    let npcs: Vec<(Entity, NpcSteering, Xform)> = {
        let mut q = world.query::<(Entity, &NpcSteering, &Xform)>();
        q.iter(world).map(|(e, s, x)| (e, s.clone(), *x)).collect()
    };
    for (e, mut st, x) in npcs {
        let Some(target) = st.target else {
            if let Some(mut m) = world.get_mut::<InputMover>(e)
                && m.wish != Vec2::ZERO
            {
                m.wish = Vec2::ZERO;
            }
            continue;
        };
        let Some(parent) = x.parent else { continue };
        let Some(goal) = target_local(world, parent, target) else {
            st.status = SteeringStatus::NoPath;
            st.stop();
            world.entity_mut(e).insert(st);
            continue;
        };
        let pos = x.pos;
        if pos.distance(goal) <= st.range {
            st.status = SteeringStatus::InRange;
            st.path.clear();
            if let Some(mut m) = world.get_mut::<InputMover>(e) {
                m.wish = Vec2::ZERO;
            }
            world.entity_mut(e).insert(st);
            continue;
        }
        st.status = SteeringStatus::Moving;

        // Stuck? Repath.
        if pos.distance(st.last_pos) > 0.25 {
            st.last_pos = pos;
            st.last_progress_time = now;
        } else if now - st.last_progress_time > 1.5 {
            st.path.clear();
            st.repath_at = now;
            st.last_progress_time = now;
        }

        let on_grid = world.get::<Grid>(parent).is_some();
        if on_grid && st.path.is_empty() && now >= st.repath_at {
            let mask = mover_mask(world, e);
            match find_path(world, parent, tile_of(pos), goal, st.range, mask) {
                Some(p) => st.path = p,
                None => {
                    st.status = SteeringStatus::NoPath;
                    if let Some(mut m) = world.get_mut::<InputMover>(e) {
                        m.wish = Vec2::ZERO;
                    }
                    world.entity_mut(e).insert(st);
                    continue;
                }
            }
            st.repath_at = now + 1.0;
        }

        // Drop waypoints we've reached.
        while let Some(&next) = st.path.first() {
            if pos.distance(tile_center(next)) < 0.3 {
                st.path.remove(0);
            } else {
                break;
            }
        }
        let aim = match st.path.first() {
            Some(&t) => tile_center(t),
            None => goal,
        };
        let wish = (aim - pos).normalize_or_zero();
        if let Some(mut m) = world.get_mut::<InputMover>(e) {
            m.wish = wish;
        }
        world.entity_mut(e).insert(st);
    }
}
