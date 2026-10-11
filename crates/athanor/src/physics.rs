//! Physics: bodies, fixtures, collision groups and a kinematic integrator.
//!
//! Upstream runs a full Box2D-derived solver. For a single-player sim the
//! behaviour that matters is narrower and is reproduced here:
//! * collision filtering by layer/mask bits (`CollisionGroup.cs`, verbatim);
//! * movers (mobs, thrown items) slide against hard static fixtures —
//!   walls, closed doors, tables, machines — instead of tunnelling;
//! * contacts are recorded so doors can bump-open, etc.
//!
//! Positions and velocities are in the entity's *parent* frame (usually its
//! grid), which keeps everything on a moving shuttle consistent.

use bevy_ecs::prelude::*;
use bevy_math::{IVec2, Vec2};
use grimoire::Node;
use serde::Deserialize;

use crate::content::ContentData;
use crate::spawn::ComponentFactories;
use crate::transform::{reparent_to_grid, rotate, tile_of, Grid, Xform};
use crate::SimClock;

/// `Content.Shared/Physics/CollisionGroup.cs`, bit for bit.
pub mod group {
    pub const NONE: u32 = 0;
    pub const OPAQUE: u32 = 1 << 0;
    pub const IMPASSABLE: u32 = 1 << 1;
    pub const MID_IMPASSABLE: u32 = 1 << 2;
    pub const HIGH_IMPASSABLE: u32 = 1 << 3;
    pub const LOW_IMPASSABLE: u32 = 1 << 4;
    pub const GHOST_IMPASSABLE: u32 = 1 << 5;
    pub const BULLET_IMPASSABLE: u32 = 1 << 6;
    pub const INTERACT_IMPASSABLE: u32 = 1 << 7;
    pub const DOOR_PASSABLE: u32 = 1 << 8;
    /// Robust's `MapGridHelpers.CollisionGroup`.
    pub const MAP_GRID: u32 = 1 << 31;

    pub fn by_name(name: &str) -> Option<u32> {
        let o = OPAQUE;
        let i = IMPASSABLE;
        let m = MID_IMPASSABLE;
        let h = HIGH_IMPASSABLE;
        let l = LOW_IMPASSABLE;
        let b = BULLET_IMPASSABLE;
        let x = INTERACT_IMPASSABLE;
        let d = DOOR_PASSABLE;
        Some(match name {
            "None" => NONE,
            "Opaque" => o,
            "Impassable" => i,
            "MidImpassable" => m,
            "HighImpassable" => h,
            "LowImpassable" => l,
            "GhostImpassable" => GHOST_IMPASSABLE,
            "BulletImpassable" => b,
            "InteractImpassable" => x,
            "DoorPassable" => d,
            "MapGrid" => MAP_GRID,
            "AllMask" => u32::MAX,
            "SingularityLayer" => o | i | m | h | l | b | x | d,
            "MobMask" => i | h | m | l,
            "MobLayer" => o | b,
            "SmallMobMask" => i | l,
            "SmallMobLayer" => o | b,
            "FlyingMobMask" => i | h,
            "FlyingMobLayer" => o | b,
            "LargeMobMask" => i | h | m | l,
            "LargeMobLayer" => o | h | m | l | b,
            "MachineMask" => i | m | l,
            "MachineLayer" => o | m | l | b,
            "ConveyorMask" => i | m | l | d,
            "CrateMask" => i | h | l,
            "TableMask" => i | m,
            "TableLayer" => m,
            "TabletopMachineMask" => i | h,
            "TabletopMachineLayer" => o | b,
            "GlassAirlockLayer" => h | m | b | x,
            "AirlockLayer" => o | h | m | b | x,
            "HumanoidBlockLayer" => h | m,
            "SlipLayer" => m | l,
            "ItemMask" => i | h,
            "ThrownItem" => i | h | b,
            "WallLayer" => o | i | h | m | l | b | x,
            "GlassLayer" => i | h | m | l | b | x,
            "HalfWallLayer" => m | l,
            "FlimsyLayer" => o | h | m | l | x,
            "SpecialWallLayer" => o | h | m | l | b,
            "FullTileMask" => i | h | m | l | x,
            "FullTileLayer" => o | h | m | l | b | x,
            "SubfloorMask" => i | l,
            _ => return None,
        })
    }

    pub fn from_names<S: AsRef<str>>(names: &[S]) -> u32 {
        names.iter().filter_map(|n| by_name(n.as_ref())).fold(0, |a, b| a | b)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BodyType {
    #[default]
    Static,
    Dynamic,
    Kinematic,
    KinematicController,
}

impl BodyType {
    fn parse(s: &str) -> BodyType {
        match s.to_ascii_lowercase().as_str() {
            "dynamic" => BodyType::Dynamic,
            "kinematic" => BodyType::Kinematic,
            "kinematiccontroller" => BodyType::KinematicController,
            _ => BodyType::Static,
        }
    }

    pub fn moves(self) -> bool {
        !matches!(self, BodyType::Static)
    }
}

/// `PhysicsComponent`.
#[derive(Component, Debug, Clone, Copy)]
pub struct Body {
    pub body_type: BodyType,
    /// Parent-frame velocity, tiles/second.
    pub velocity: Vec2,
    pub can_collide: bool,
    pub fixed_rotation: bool,
    pub on_ground: bool,
    pub linear_damping: f32,
}

impl Default for Body {
    fn default() -> Self {
        Body {
            body_type: BodyType::Static,
            velocity: Vec2::ZERO,
            can_collide: true,
            fixed_rotation: true,
            on_ground: true,
            linear_damping: 0.2,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Circle { center: Vec2, radius: f32 },
    Aabb { min: Vec2, max: Vec2 },
    Polygon(Vec<Vec2>),
}

impl Shape {
    /// Local-space bounding box.
    pub fn bounds(&self) -> (Vec2, Vec2) {
        match self {
            Shape::Circle { center, radius } => (*center - Vec2::splat(*radius), *center + Vec2::splat(*radius)),
            Shape::Aabb { min, max } => (*min, *max),
            Shape::Polygon(v) => {
                let mut lo = Vec2::splat(f32::MAX);
                let mut hi = Vec2::splat(f32::MIN);
                for p in v {
                    lo = lo.min(*p);
                    hi = hi.max(*p);
                }
                (lo, hi)
            }
        }
    }

    /// Bounding box after rotating by `rot` and translating by `pos`.
    pub fn world_bounds(&self, pos: Vec2, rot: f32) -> (Vec2, Vec2) {
        let (lo, hi) = self.bounds();
        let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
        let mut a = Vec2::splat(f32::MAX);
        let mut b = Vec2::splat(f32::MIN);
        for c in corners {
            let w = pos + rotate(c, rot);
            a = a.min(w);
            b = b.max(w);
        }
        (a, b)
    }

    /// Radius of a circle that contains the shape around the origin.
    pub fn mover_radius(&self) -> f32 {
        match self {
            Shape::Circle { radius, center } => radius + center.length(),
            other => {
                let (lo, hi) = other.bounds();
                // Movers use the inscribed size, so square mobs fit doors.
                ((hi.x - lo.x).min(hi.y - lo.y) * 0.5).max(0.05)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fixture {
    pub id: String,
    pub shape: Shape,
    pub hard: bool,
    pub layer: u32,
    pub mask: u32,
    pub density: f32,
}

/// `FixturesComponent`.
#[derive(Component, Debug, Clone, Default)]
pub struct Fixtures(pub Vec<Fixture>);

impl Fixtures {
    pub fn layer(&self) -> u32 {
        self.0.iter().filter(|f| f.hard).fold(0, |a, f| a | f.layer)
    }

    pub fn mask(&self) -> u32 {
        self.0.iter().filter(|f| f.hard).fold(0, |a, f| a | f.mask)
    }

    pub fn mover_radius(&self) -> f32 {
        self.0
            .iter()
            .filter(|f| f.hard)
            .map(|f| f.shape.mover_radius())
            .fold(None, |a: Option<f32>, r| Some(a.map_or(r, |a| a.max(r))))
            .unwrap_or(0.35)
    }

    /// Union of hard fixture bounds in parent frame.
    pub fn bounds(&self, pos: Vec2, rot: f32) -> Option<(Vec2, Vec2)> {
        self.0.iter().filter(|f| f.hard).fold(None, |acc, f| {
            let (a, b) = f.shape.world_bounds(pos, rot);
            Some(match acc {
                None => (a, b),
                Some((x, y)) => (x.min(a), y.max(b)),
            })
        })
    }
}

/// Robust's collision test between two fixtures' filter bits.
pub fn should_collide(layer_a: u32, mask_a: u32, layer_b: u32, mask_b: u32) -> bool {
    (mask_a & layer_b) != 0 || (mask_b & layer_a) != 0
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureYaml {
    #[serde(default)]
    shape: Option<ShapeYaml>,
    #[serde(default)]
    hard: Option<bool>,
    #[serde(default)]
    layer: Vec<String>,
    #[serde(default)]
    mask: Vec<String>,
    #[serde(default)]
    density: Option<f32>,
}

#[derive(Deserialize)]
#[allow(clippy::enum_variant_names)]
enum ShapeYaml {
    PhysShapeCircle {
        #[serde(default)]
        radius: Option<f32>,
        #[serde(default)]
        position: Option<(f32, f32)>,
    },
    PhysShapeAabb {
        #[serde(default)]
        bounds: Option<(f32, f32, f32, f32)>,
    },
    PolygonShape {
        #[serde(default)]
        vertices: Vec<(f32, f32)>,
    },
}

fn parse_fixtures(node: &Node) -> Fixtures {
    let mut out = Vec::new();
    let Some(map) = node.get("fixtures").and_then(Node::as_map) else {
        return Fixtures(out);
    };
    for (id, f) in map {
        let parsed: FixtureYaml = match grimoire::from_node(f) {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("fixture {id}: {e}");
                continue;
            }
        };
        let shape = match parsed.shape {
            Some(ShapeYaml::PhysShapeCircle { radius, position }) => Shape::Circle {
                center: position.map(|(x, y)| Vec2::new(x, y)).unwrap_or(Vec2::ZERO),
                radius: radius.unwrap_or(0.5),
            },
            Some(ShapeYaml::PhysShapeAabb { bounds }) => {
                let (l, b, r, t) = bounds.unwrap_or((-0.5, -0.5, 0.5, 0.5));
                Shape::Aabb {
                    min: Vec2::new(l.min(r), b.min(t)),
                    max: Vec2::new(l.max(r), b.max(t)),
                }
            }
            Some(ShapeYaml::PolygonShape { vertices }) if vertices.len() >= 3 => {
                Shape::Polygon(vertices.into_iter().map(|(x, y)| Vec2::new(x, y)).collect())
            }
            _ => Shape::Aabb {
                min: Vec2::splat(-0.5),
                max: Vec2::splat(0.5),
            },
        };
        out.push(Fixture {
            id: id.clone(),
            shape,
            hard: parsed.hard.unwrap_or(true),
            layer: group::from_names(&parsed.layer),
            mask: group::from_names(&parsed.mask),
            density: parsed.density.unwrap_or(1.0),
        });
    }
    Fixtures(out)
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("Physics", |e, node, _: &ContentData| {
        let mut b = Body::default();
        if let Some(t) = node.get_str("bodyType") {
            b.body_type = BodyType::parse(t);
        }
        if let Some(c) = node.get("canCollide").and_then(Node::as_bool) {
            b.can_collide = c;
        }
        if let Some(f) = node.get("fixedRotation").and_then(Node::as_bool) {
            b.fixed_rotation = f;
        }
        if let Some(s) = node.get_str("bodyStatus") {
            b.on_ground = s != "InAir";
        }
        if let Some(d) = node.get("linearDamping").and_then(Node::as_f32) {
            b.linear_damping = d;
        }
        e.insert(b);
    });
    reg.register("Fixtures", |e, node, _| {
        e.insert(parse_fixtures(node));
    });
}

/// Pairs of (mover, obstacle) that touched this tick.
#[derive(Resource, Debug, Default)]
pub struct Contacts(pub Vec<(Entity, Entity)>);

/// An obstacle as seen by the integrator: a hard fixture in parent frame.
#[derive(Clone, Copy)]
struct Obstacle {
    entity: Entity,
    min: Vec2,
    max: Vec2,
    circle: Option<(Vec2, f32)>,
}

/// Gather hard obstacles near `pos` on `grid`, filtered against the mover.
fn obstacles_near(world: &World, grid: Entity, pos: Vec2, reach: f32, me: Entity, layer: u32, mask: u32, out: &mut Vec<Obstacle>) {
    out.clear();
    let Some(g) = world.get::<Grid>(grid) else { return };
    let lo = tile_of(pos - Vec2::splat(reach));
    let hi = tile_of(pos + Vec2::splat(reach));
    for x in lo.x..=hi.x {
        for y in lo.y..=hi.y {
            for &e in g.anchored_at(IVec2::new(x, y)) {
                if e == me {
                    continue;
                }
                push_obstacle(world, e, layer, mask, out);
            }
        }
    }
}

fn push_obstacle(world: &World, e: Entity, layer: u32, mask: u32, out: &mut Vec<Obstacle>) {
    let (Some(body), Some(fx), Some(x)) = (world.get::<Body>(e), world.get::<Fixtures>(e), world.get::<Xform>(e)) else {
        return;
    };
    if !body.can_collide {
        return;
    }
    for f in fx.0.iter().filter(|f| f.hard) {
        if !should_collide(layer, mask, f.layer, f.mask) {
            continue;
        }
        let (min, max) = f.shape.world_bounds(x.pos, x.rot);
        let circle = match &f.shape {
            Shape::Circle { center, radius } => Some((x.pos + rotate(*center, x.rot), *radius)),
            _ => None,
        };
        out.push(Obstacle { entity: e, min, max, circle });
    }
}

/// Obstacles for a mover floating in a map (off-grid): hard fixtures of every
/// nearby grid, transformed into world space. Without this, things in space
/// would pass straight through hulls.
fn obstacles_off_grid(world: &World, map: Entity, pos: Vec2, reach: f32, me: Entity, layer: u32, mask: u32, out: &mut Vec<Obstacle>) {
    out.clear();
    let grids = world.get_resource::<crate::transform::GridIndex>().map(|g| g.grids.clone()).unwrap_or_default();
    let mut local = Vec::new();
    for g in grids {
        let gp = crate::transform::world_pose_of(world, g);
        if gp.map != Some(map) {
            continue;
        }
        let lp = crate::transform::to_local(&gp, pos);
        // Cheap reject: is the point near any of this grid's tiles?
        let Some(grid) = world.get::<Grid>(g) else { continue };
        let t = tile_of(lp);
        let near = (-1..=1).any(|dx| (-1..=1).any(|dy| grid.has_tile(t + IVec2::new(dx, dy))));
        if !near {
            continue;
        }
        obstacles_near(world, g, lp, reach, me, layer, mask, &mut local);
        for o in local.drain(..) {
            let corners = [o.min, Vec2::new(o.max.x, o.min.y), o.max, Vec2::new(o.min.x, o.max.y)];
            let mut a = Vec2::splat(f32::MAX);
            let mut b = Vec2::splat(f32::MIN);
            for c in corners {
                let w = crate::transform::to_world(&gp, c);
                a = a.min(w);
                b = b.max(w);
            }
            out.push(Obstacle {
                entity: o.entity,
                min: a,
                max: b,
                circle: o.circle.map(|(c, r)| (crate::transform::to_world(&gp, c), r)),
            });
        }
    }
}

/// Push a circle out of an obstacle. Returns the correction vector.
fn resolve_circle(center: Vec2, r: f32, o: &Obstacle) -> Option<Vec2> {
    if let Some((c, cr)) = o.circle {
        let d = center - c;
        let dist = d.length();
        let min = r + cr;
        if dist >= min {
            return None;
        }
        let n = if dist > 1e-5 { d / dist } else { Vec2::Y };
        return Some(n * (min - dist));
    }
    let closest = center.clamp(o.min, o.max);
    let d = center - closest;
    let dist = d.length();
    if dist > 1e-5 {
        if dist >= r {
            return None;
        }
        return Some(d / dist * (r - dist));
    }
    // Centre inside the box: leave along the shallowest axis.
    let left = center.x - o.min.x;
    let right = o.max.x - center.x;
    let down = center.y - o.min.y;
    let up = o.max.y - center.y;
    let m = left.min(right).min(down).min(up);
    Some(if m == left {
        Vec2::new(-(left + r), 0.0)
    } else if m == right {
        Vec2::new(right + r, 0.0)
    } else if m == down {
        Vec2::new(0.0, -(down + r))
    } else {
        Vec2::new(0.0, up + r)
    })
}

/// Integrate every moving body for one tick, resolving collisions against
/// hard static fixtures on its grid.
pub fn physics_step(world: &mut World) {
    let dt = world.resource::<SimClock>().dt;
    world.get_resource_or_init::<Contacts>().0.clear();

    let movers: Vec<(Entity, Body, Xform, u32, u32, f32)> = {
        let mut q = world.query::<(Entity, &Body, &Xform, Option<&Fixtures>)>();
        q.iter(world)
            .filter(|(_, b, x, _)| b.body_type.moves() && !x.anchored && b.velocity.length_squared() > 1e-10)
            .map(|(e, b, x, f)| {
                let (layer, mask, r) = f.map_or((0, 0, 0.35), |f| (f.layer(), f.mask(), f.mover_radius()));
                (e, *b, *x, layer, mask, r)
            })
            .collect()
    };

    let mut obstacles = Vec::new();
    let mut contacts = Vec::new();
    for (e, mut body, x, layer, mask, r) in movers {
        // Skip things in containers (held items, contents of lockers).
        if world.get::<crate::interaction::InContainer>(e).is_some() {
            continue;
        }
        let Some(parent) = x.parent else { continue };
        let mut pos = x.pos;
        let delta = body.velocity * dt;
        let is_grid = world.get::<Grid>(parent).is_some();
        let is_map = world.get::<crate::transform::MapRoot>(parent).is_some();
        let collides = body.can_collide && (is_grid || is_map) && mask != 0;
        let steps = ((delta.length() / (r * 0.5).max(0.05)).ceil() as usize).clamp(1, 16);
        let step = delta / steps as f32;
        for _ in 0..steps {
            pos += step;
            if !collides {
                continue;
            }
            if is_grid {
                obstacles_near(world, parent, pos, r + 1.0, e, layer, mask, &mut obstacles);
            } else {
                obstacles_off_grid(world, parent, pos, r + 1.0, e, layer, mask, &mut obstacles);
            }
            for _ in 0..4 {
                let mut moved = false;
                for o in &obstacles {
                    if let Some(push) = resolve_circle(pos, r, o) {
                        pos += push;
                        let n = push.normalize_or_zero();
                        let into = body.velocity.dot(n);
                        if into < 0.0 {
                            body.velocity -= n * into; // slide
                        }
                        contacts.push((e, o.entity));
                        moved = true;
                    }
                }
                if !moved {
                    break;
                }
            }
        }
        if let Some(mut xm) = world.get_mut::<Xform>(e) {
            xm.pos = pos;
        }
        if let Some(mut bm) = world.get_mut::<Body>(e) {
            bm.velocity = body.velocity;
        }
        // Moved off the grid (or onto one)? Re-parent like Robust does.
        let off_grid = !is_grid || !world.get::<Grid>(parent).is_some_and(|g| g.has_tile(tile_of(pos)));
        if off_grid {
            reparent_to_grid(world, e);
        }
    }
    contacts.sort_by_key(|(a, b)| (a.to_bits(), b.to_bits()));
    contacts.dedup();
    world.resource_mut::<Contacts>().0 = contacts;
}

/// Is a hard, filter-matching fixture anchored on this tile?
pub fn tile_blocked(world: &World, grid: Entity, tile: IVec2, mask: u32) -> bool {
    let Some(g) = world.get::<Grid>(grid) else { return true };
    g.anchored_at(tile).iter().any(|&e| {
        let (Some(b), Some(f)) = (world.get::<Body>(e), world.get::<Fixtures>(e)) else { return false };
        b.can_collide && f.0.iter().any(|fx| fx.hard && (mask & fx.layer) != 0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_match_upstream() {
        assert_eq!(group::by_name("MobMask"), Some(2 | 8 | 4 | 16));
        assert_eq!(group::by_name("WallLayer"), Some(1 | 2 | 8 | 4 | 16 | 64 | 128));
        // Mobs walk through each other but not walls.
        let (ml, mm) = (group::by_name("MobLayer").unwrap(), group::by_name("MobMask").unwrap());
        assert!(!should_collide(ml, mm, ml, mm));
        let wl = group::by_name("WallLayer").unwrap();
        assert!(should_collide(ml, mm, wl, 0));
        // ...and not items.
        assert!(!should_collide(ml, mm, 0, group::by_name("ItemMask").unwrap()));
    }

    #[test]
    fn circle_pushout() {
        let o = Obstacle {
            entity: Entity::PLACEHOLDER,
            min: Vec2::new(1.0, 0.0),
            max: Vec2::new(2.0, 1.0),
            circle: None,
        };
        let p = resolve_circle(Vec2::new(0.8, 0.5), 0.35, &o).unwrap();
        assert!((p.x + 0.15).abs() < 1e-5 && p.y.abs() < 1e-5, "{p}");
        assert!(resolve_circle(Vec2::new(0.5, 0.5), 0.35, &o).is_none());
    }
}
