//! Shuttles — the heart of Frontier. Ports of `ShuttleComponent`,
//! `ThrusterSystem` (thrust pools per cardinal direction, gyroscopes),
//! `ShuttleConsoleSystem` (piloting) and the shuttle half of the server
//! `MoverController` (`HandleShuttleMovement`: Frontier's max-velocity
//! ellipse, braking, angular clamp).
//!
//! A grid with a `Shuttle` component gets a [`GridBody`]. Its mass and
//! inertia come from its tiles at `SharedShuttleSystem.TileDensityMultiplier`
//! (0.5 per 1 m² tile), as upstream sets grid fixture density. Everything on
//! the grid is parented to it, so moving the grid moves the crew and cargo.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use grimoire::Node;

use crate::interaction::{in_range, InteractionHandlers};
use crate::movement::InputMover;
use crate::spawn::ComponentFactories;
use crate::transform::{rotate, tile_of, vec_to_angle, world_pose_of, Grid, Xform};
use crate::SimClock;

/// `SharedShuttleSystem.TileDensityMultiplier`.
pub const TILE_DENSITY: f32 = 0.5;
/// `ShuttleComponent.BrakeCoefficient`.
pub const BRAKE_COEFFICIENT: f32 = 1.5;
/// `ShuttleComponent.MaxAngularVelocity`.
pub const MAX_ANGULAR_VELOCITY: f32 = 4.0;

/// `ShuttleComponent` (server).
#[derive(Component, Debug, Clone)]
pub struct Shuttle {
    pub enabled: bool,
    /// Frontier: 23.07 (upstream 60).
    pub base_max_linear_velocity: f32,
    /// Thrust available per direction index: 0 S, 1 E, 2 N, 3 W.
    pub linear_thrust: [f32; 4],
    pub base_linear_thrust: [f32; 4],
    pub angular_thrust: f32,
    /// Which linear directions are firing (for thruster visuals).
    pub firing: [bool; 4],
    pub firing_angular: bool,
}

impl Default for Shuttle {
    fn default() -> Self {
        Shuttle {
            enabled: true,
            base_max_linear_velocity: 23.07,
            linear_thrust: [0.0; 4],
            base_linear_thrust: [0.0; 4],
            angular_thrust: 0.0,
            firing: [false; 4],
            firing_angular: false,
        }
    }
}

/// Rigid-body state of a moving grid (the grid's `PhysicsComponent`).
#[derive(Component, Debug, Clone, Default)]
pub struct GridBody {
    /// Map-frame velocity, m/s.
    pub velocity: Vec2,
    /// rad/s, CCW positive.
    pub angular_velocity: f32,
    pub mass: f32,
    pub inertia: f32,
    /// Grid-local centre of mass.
    pub com: Vec2,
    pub linear_damping: f32,
    pub angular_damping: f32,
    /// Tile revision the mass properties were computed for.
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThrusterType {
    #[default]
    Linear,
    Angular,
}

/// `ThrusterComponent` (thrusters and gyroscopes).
#[derive(Component, Debug, Clone)]
pub struct Thruster {
    pub enabled: bool,
    pub thrust: f32,
    pub base_thrust: f32,
    pub kind: ThrusterType,
}

/// Frontier `ForceAnchorComponent`: the grid's body is static and its
/// shuttle disabled (`ForceAnchorSystem.OnForceAnchorMapInit`). Outposts and
/// POIs have it; nothing moves them.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ForceAnchor;

/// Can this grid move (a dynamic shuttle body, not force-anchored)?
pub fn is_movable(world: &World, g: Entity) -> bool {
    world.get::<Shuttle>(g).is_some_and(|s| s.enabled) && world.get::<GridBody>(g).is_some() && world.get::<ForceAnchor>(g).is_none()
}

/// `ShuttleConsoleComponent`.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ShuttleConsole;

/// `PilotComponent`: the entity is flying a shuttle from a console.
#[derive(Component, Debug, Clone, Copy)]
pub struct Pilot {
    pub console: Entity,
    pub shuttle: Entity,
    /// Shuttle-frame strafe input (x right, y forward), each in [-1, 1].
    pub strafe: Vec2,
    /// +1 rotates counter-clockwise ("RotateLeft"), -1 clockwise.
    pub rotation: f32,
    pub brake: f32,
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("Shuttle", |e, node, _| {
        let mut s = Shuttle::default();
        if let Some(v) = node.get("baseMaxLinearVelocity").and_then(Node::as_f32) {
            s.base_max_linear_velocity = v;
        }
        if let Some(v) = node.get("enabled").and_then(Node::as_bool) {
            s.enabled = v;
        }
        e.insert(s);
    });
    reg.register("Thruster", |e, node, _| {
        let thrust = node.get("thrust").and_then(Node::as_f32).unwrap_or(100.0);
        e.insert(Thruster {
            enabled: node.get("enabled").and_then(Node::as_bool).unwrap_or(true),
            thrust,
            base_thrust: node.get("baseThrust").and_then(Node::as_f32).unwrap_or(thrust),
            kind: match node.get_str("thrusterType") {
                Some("Angular") => ThrusterType::Angular,
                _ => ThrusterType::Linear,
            },
        });
    });
    reg.register("ForceAnchor", |e, _, _| {
        e.insert(ForceAnchor);
        if let Some(mut s) = e.get_mut::<Shuttle>() {
            s.enabled = false;
        }
    });
    reg.register("ShuttleConsole", |e, _, _| {
        e.insert(ShuttleConsole);
    });
}

pub fn install(world: &mut World) {
    let mut h = world.get_resource_or_init::<InteractionHandlers>();
    h.on_interact_hand.push(|world, user, target| {
        if world.get::<ShuttleConsole>(target).is_none() {
            return false;
        }
        toggle_pilot(world, user, target);
        true
    });
}

/// Start or stop piloting the shuttle a console sits on.
pub fn toggle_pilot(world: &mut World, user: Entity, console: Entity) -> bool {
    if world.get::<Pilot>(user).is_some() {
        stop_piloting(world, user);
        return false;
    }
    let Some(grid) = world.get::<Xform>(console).and_then(|x| x.parent) else { return false };
    if !crate::power::is_powered(world, console) {
        // `ActivatableUIRequiresPower`: a dark console can't be used.
        world.write_message(crate::events::Popup {
            text: "The console is unpowered.".into(),
            at: user,
            kind: crate::events::PopupKind::Small,
        });
        return false;
    }
    let name = world.get::<crate::spawn::MetaData>(grid).map(|m| m.name.clone()).unwrap_or_default();
    if world.get::<Shuttle>(grid).is_none() {
        world.write_message(crate::events::Popup {
            text: "This console isn't connected to a shuttle.".into(),
            at: user,
            kind: crate::events::PopupKind::Small,
        });
        return false;
    }
    world.entity_mut(user).insert(Pilot {
        console,
        shuttle: grid,
        strafe: Vec2::ZERO,
        rotation: 0.0,
        brake: 0.0,
    });
    if let Some(mut m) = world.get_mut::<InputMover>(user) {
        m.wish = Vec2::ZERO;
    }
    world.write_message(crate::events::Popup {
        text: format!("You take the helm of the {name}."),
        at: user,
        kind: crate::events::PopupKind::Small,
    });
    true
}

pub fn stop_piloting(world: &mut World, user: Entity) {
    if world.entity_mut(user).take::<Pilot>().is_some() {
        world.write_message(crate::events::Popup {
            text: "You step away from the helm.".into(),
            at: user,
            kind: crate::events::PopupKind::Small,
        });
    }
}

/// Make sure every `Shuttle` grid has a body with current mass properties
/// (upstream recomputes these when grid fixtures change).
pub fn update_grid_bodies(mut commands: Commands, mut q: Query<(Entity, &Grid, Option<&mut GridBody>), With<Shuttle>>) {
    for (e, grid, body) in q.iter_mut() {
        if body.as_ref().is_some_and(|b| b.revision == grid.revision && b.mass > 0.0) {
            continue;
        }
        let tiles: Vec<Vec2> = grid
            .tiles
            .iter()
            .map(|((x, y), _)| Vec2::new(x as f32 + 0.5, y as f32 + 0.5))
            .collect();
        if tiles.is_empty() {
            continue;
        }
        let m_tile = TILE_DENSITY;
        let mass = tiles.len() as f32 * m_tile;
        let com = tiles.iter().copied().sum::<Vec2>() / tiles.len() as f32;
        // Unit squares: I = m (r² + 1/6) about the centre of mass.
        let inertia = tiles.iter().map(|t| m_tile * ((*t - com).length_squared() + 1.0 / 6.0)).sum::<f32>();
        match body {
            Some(mut b) => {
                b.mass = mass;
                b.inertia = inertia;
                b.com = com;
                b.revision = grid.revision;
            }
            None => {
                commands.entity(e).insert(GridBody {
                    mass,
                    inertia,
                    com,
                    linear_damping: 0.05,
                    angular_damping: 0.05,
                    revision: grid.revision,
                    ..Default::default()
                });
            }
        }
    }
}

/// Recompute thrust pools from anchored thrusters (`ThrusterSystem`).
pub fn update_thrust(world: &mut World) {
    // Unpowered thrusters and gyroscopes don't fire (`ThrusterSystem.CanEnable`).
    let thrusters: Vec<(Entity, Thruster, Xform)> = {
        let mut q = world.query_filtered::<(Entity, &Thruster, &Xform), Without<crate::power::Unpowered>>();
        q.iter(world).map(|(e, t, x)| (e, t.clone(), *x)).collect()
    };
    let shuttles: Vec<Entity> = {
        let mut q = world.query_filtered::<Entity, With<Shuttle>>();
        q.iter(world).collect()
    };
    for s in &shuttles {
        if let Some(mut sh) = world.get_mut::<Shuttle>(*s) {
            sh.linear_thrust = [0.0; 4];
            sh.base_linear_thrust = [0.0; 4];
            sh.angular_thrust = 0.0;
        }
    }
    for (_, t, x) in thrusters {
        if !t.enabled || !x.anchored {
            continue;
        }
        let Some(grid) = x.parent else { continue };
        let Some(mut sh) = world.get_mut::<Shuttle>(grid) else { continue };
        match t.kind {
            ThrusterType::Angular => sh.angular_thrust += t.thrust,
            ThrusterType::Linear => {
                // (int)LocalRotation.GetCardinalDir() / 2 → S=0, E=1, N=2, W=3.
                let dir = vellum::rsi::Direction::cardinal_from_angle(x.rot as f64) as usize / 2;
                sh.linear_thrust[dir] += t.thrust;
                sh.base_linear_thrust[dir] += t.base_thrust;
            }
        }
    }
}

/// `MoverController.ObtainMaxVel`: Frontier's per-direction velocity ellipse.
fn max_vel(v: Vec2, sh: &Shuttle) -> Vec2 {
    if v.length() == 0.0 {
        return Vec2::ZERO;
    }
    let hi = if v.x > 0.0 { 1 } else { 3 };
    let vi = if v.y > 0.0 { 2 } else { 0 };
    let ratio = |i: usize| {
        if sh.linear_thrust[i] > 0.0 {
            sh.base_linear_thrust[i] / sh.linear_thrust[i]
        } else {
            1.0
        }
    };
    let h = if v.x != 0.0 { (v.x * ratio(hi)).powi(2) } else { 0.0 };
    let vv = if v.y != 0.0 { (v.y * ratio(vi)).powi(2) } else { 0.0 };
    let denom = (h + vv).sqrt();
    if denom <= 0.0 {
        return Vec2::ZERO;
    }
    sh.base_max_linear_velocity * v / denom
}

/// Index of a cardinal direction flag in thrust arrays.
const S: usize = 0;
const E: usize = 1;
const N: usize = 2;
const W: usize = 3;

/// Apply pilot input to shuttles and integrate grid motion.
pub fn shuttle_movement(world: &mut World) {
    let dt = world.resource::<SimClock>().dt;

    // Drop pilots who walked off or whose console vanished.
    let pilots: Vec<(Entity, Pilot)> = {
        let mut q = world.query::<(Entity, &Pilot)>();
        q.iter(world).map(|(e, p)| (e, *p)).collect()
    };
    for (e, p) in &pilots {
        let gone = world.get_entity(p.console).is_err() || !in_range(world, *e, p.console, 1.6);
        if gone {
            stop_piloting(world, *e);
        }
    }

    let shuttles: Vec<Entity> = {
        let mut q = world.query_filtered::<Entity, (With<Shuttle>, With<GridBody>)>();
        q.iter(world).collect()
    };
    for s in shuttles {
        // Average the inputs of everyone piloting this shuttle.
        let (mut strafe, mut rot, mut brake, mut n) = (Vec2::ZERO, 0.0f32, 0.0f32, 0u32);
        {
            let mut q = world.query::<&Pilot>();
            for p in q.iter(world).filter(|p| p.shuttle == s) {
                strafe += p.strafe;
                rot += p.rotation;
                brake += p.brake;
                n += 1;
            }
        }
        if n > 0 {
            strafe /= n as f32;
            rot /= n as f32;
            brake /= n as f32;
        }
        let north = world_pose_of(world, s).rot;
        let sh = world.get::<Shuttle>(s).cloned().unwrap();
        let mut body = world.get::<GridBody>(s).cloned().unwrap();
        if !sh.enabled || body.mass <= 0.0 || world.get::<ForceAnchor>(s).is_some() {
            continue;
        }
        let inv_mass = 1.0 / body.mass;
        let inv_i = if body.inertia > 0.0 { 1.0 / body.inertia } else { 0.0 };
        let mut firing = [false; 4];
        let mut firing_angular = false;

        // Brakes.
        if brake > 0.0 {
            if body.velocity.length() > 0.0 {
                let local = rotate(body.velocity, -north);
                let mut force = Vec2::ZERO;
                if local.x < 0.0 {
                    force.x += sh.linear_thrust[E];
                    firing[E] = local.x < -0.1;
                } else if local.x > 0.0 {
                    force.x -= sh.linear_thrust[W];
                    firing[W] = local.x > 0.1;
                }
                if local.y < 0.0 {
                    force.y += sh.linear_thrust[N];
                    firing[N] = local.y < -0.1;
                } else if local.y > 0.0 {
                    force.y -= sh.linear_thrust[S];
                    firing[S] = local.y > 0.1;
                }
                let mut impulse = rotate(force * brake * BRAKE_COEFFICIENT, north);
                let force_mul = dt * inv_mass;
                let max = body.velocity.length() / force_mul;
                if impulse.length() > max {
                    impulse = impulse.normalize_or_zero() * max;
                }
                body.velocity += impulse * force_mul;
            }
            if body.angular_velocity != 0.0 {
                let mut torque = sh.angular_thrust * brake * -body.angular_velocity.signum() * BRAKE_COEFFICIENT;
                let tm = inv_i * dt;
                if tm > 0.0 {
                    torque = if body.angular_velocity > 0.0 {
                        torque.max(-body.angular_velocity / tm)
                    } else {
                        torque.min(-body.angular_velocity / tm)
                    };
                    body.angular_velocity += torque * tm;
                    firing_angular = torque != 0.0;
                }
            }
        }

        // Linear thrust.
        if strafe.length() > 0.0 {
            let dir = vellum::rsi::Direction::from_angle(vec_to_angle(strafe) as f64);
            use vellum::rsi::Direction as D;
            let dirs: &[usize] = match dir {
                D::North => &[N],
                D::South => &[S],
                D::East => &[E],
                D::West => &[W],
                D::NorthEast => &[N, E],
                D::NorthWest => &[N, W],
                D::SouthEast => &[S, E],
                D::SouthWest => &[S, W],
            };
            let mut total = Vec2::ZERO;
            for &i in dirs {
                let t = sh.linear_thrust[i];
                let f = match i {
                    N => Vec2::new(0.0, t),
                    S => Vec2::new(0.0, -t),
                    E => Vec2::new(t, 0.0),
                    _ => Vec2::new(-t, 0.0),
                };
                firing[i] = t > 0.0;
                total += f * strafe.length().min(1.0);
            }
            let force_mul = dt * inv_mass;
            let local_vel = rotate(body.velocity, -north);
            let max_v = max_vel(local_vel, &sh);
            let max_wish = max_vel(total, &sh);
            let proper = (max_wish - local_vel) / force_mul;
            let pn = proper.normalize_or_zero();
            let mut final_force = total.dot(pn) * pn;
            if local_vel.length() >= max_v.length() && total.dot(local_vel) > 0.0 {
                let ln = local_vel.normalize_or_zero();
                final_force -= total.dot(ln) * ln;
            }
            if final_force.length() > proper.length() {
                final_force = proper;
            }
            body.velocity += rotate(final_force, north) * force_mul;
        }

        // Angular thrust.
        if rot.abs() > 1e-4 {
            let tm = inv_i * dt;
            if tm > 0.0 {
                // Upstream negates the input: RotateLeft is a negative axis
                // there; here +rotation already means counter-clockwise.
                let mut torque = sh.angular_thrust * rot;
                torque = torque.clamp(
                    (-MAX_ANGULAR_VELOCITY - body.angular_velocity) / tm,
                    (MAX_ANGULAR_VELOCITY - body.angular_velocity) / tm,
                );
                body.angular_velocity += torque * tm;
                firing_angular |= torque != 0.0;
            }
        }

        // Damping (Box2D style), then integrate about the centre of mass.
        body.velocity *= 1.0 / (1.0 + dt * body.linear_damping);
        body.angular_velocity *= 1.0 / (1.0 + dt * body.angular_damping);
        if body.velocity.length_squared() < 1e-8 {
            body.velocity = Vec2::ZERO;
        }
        if body.angular_velocity.abs() < 1e-5 {
            body.angular_velocity = 0.0;
        }
        let old = *world.get::<Xform>(s).unwrap();
        if body.velocity != Vec2::ZERO || body.angular_velocity != 0.0 {
            let com_world = old.pos + rotate(body.com, old.rot);
            let new_rot = old.rot + body.angular_velocity * dt;
            let new_com = com_world + body.velocity * dt;
            let new_pos = new_com - rotate(body.com, new_rot);
            if let Some(mut x) = world.get_mut::<Xform>(s) {
                x.rot = new_rot;
                x.pos = new_pos;
            }
            if let Some(other) = overlapping_grid(world, s) {
                // Hit something: undo the step and resolve the impact.
                if let Some(mut x) = world.get_mut::<Xform>(s) {
                    *x = old;
                }
                impact(world, s, &mut body, other);
            }
        }
        world.entity_mut(s).insert(body);
        if let Some(mut sh) = world.get_mut::<Shuttle>(s) {
            sh.firing = firing;
            sh.firing_angular = firing_angular;
        }
    }
}

/// Fraction of relative velocity kept after a hull-on-hull hit.
pub const RESTITUTION: f32 = 0.15;

/// Does any tile of grid `g` overlap a tile of another grid on its map?
pub fn overlapping_grid(world: &World, g: Entity) -> Option<Entity> {
    let gp = world_pose_of(world, g);
    let grid = world.get::<Grid>(g)?;
    let (x0, y0, x1, y1) = grid.tiles.bounds()?;
    let center_local = Vec2::new((x0 + x1 + 1) as f32, (y0 + y1 + 1) as f32) * 0.5;
    let radius = Vec2::new((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32).length() * 0.5 + 1.0;
    let center = crate::transform::to_world(&gp, center_local);
    let others = world.get_resource::<crate::transform::GridIndex>().map(|i| i.grids.clone()).unwrap_or_default();
    let welded = crate::docking::docked_grids(world, g);
    for o in others {
        if o == g || welded.contains(&o) {
            continue;
        }
        let op = world_pose_of(world, o);
        if op.map != gp.map {
            continue;
        }
        let Some(og) = world.get::<Grid>(o) else { continue };
        let Some((a0, b0, a1, b1)) = og.tiles.bounds() else { continue };
        let oc_local = Vec2::new((a0 + a1 + 1) as f32, (b0 + b1 + 1) as f32) * 0.5;
        let oradius = Vec2::new((a1 - a0 + 1) as f32, (b1 - b0 + 1) as f32).length() * 0.5 + 1.0;
        if crate::transform::to_world(&op, oc_local).distance(center) > radius + oradius {
            continue;
        }
        // Narrow phase: tile centres of ours inside a tile of theirs, with a
        // little slack so grazing corners do not count.
        for ((x, y), _) in grid.tiles.iter() {
            let w = crate::transform::to_world(&gp, Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
            let l = crate::transform::to_local(&op, w);
            let t = tile_of(l);
            if og.has_tile(t) {
                let inner = l - Vec2::new(t.x as f32 + 0.5, t.y as f32 + 0.5);
                if inner.x.abs() < 0.45 && inner.y.abs() < 0.45 {
                    return Some(o);
                }
            }
        }
    }
    None
}

/// Inelastic hull impact (a slim port of `ShuttleSystem.Impact`): exchange
/// momentum with a moving shuttle, or rebound off a station.
fn impact(world: &mut World, s: Entity, body: &mut GridBody, other: Entity) {
    let speed = body.velocity.length();
    match world.get::<GridBody>(other).cloned() {
        Some(mut ob) if is_movable(world, other) => {
            let m1 = body.mass;
            let m2 = ob.mass.max(0.001);
            let p = body.velocity * m1 + ob.velocity * m2;
            let vcm = p / (m1 + m2);
            body.velocity = vcm + (vcm - body.velocity) * RESTITUTION;
            ob.velocity = vcm + (vcm - ob.velocity) * RESTITUTION;
            ob.angular_velocity *= 0.5;
            world.entity_mut(other).insert(ob);
        }
        _ => {
            body.velocity = -body.velocity * RESTITUTION;
        }
    }
    body.angular_velocity = -body.angular_velocity * RESTITUTION;
    if speed > 0.5 {
        world.write_message(crate::events::SoundAt {
            path: "MetalThud".into(),
            at: s,
        });
        let name = world.get::<crate::spawn::MetaData>(other).map(|m| m.name.clone()).unwrap_or_default();
        world.write_message(crate::events::ChatLine {
            speaker: None,
            channel: crate::events::ChatChannel::Server,
            text: format!("The hull shudders as it strikes the {name}!"),
        });
    }
}

/// Is `e` standing on a grid tile? (Helper for UIs.)
pub fn on_grid_tile(world: &World, e: Entity) -> bool {
    let Some(x) = world.get::<Xform>(e) else { return false };
    x.parent
        .and_then(|p| world.get::<Grid>(p))
        .is_some_and(|g| g.has_tile(tile_of(x.pos)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_velocity_is_frontier_cap() {
        let sh = Shuttle {
            linear_thrust: [100.0; 4],
            base_linear_thrust: [100.0; 4],
            ..Default::default()
        };
        let v = max_vel(Vec2::new(3.0, 4.0), &sh);
        assert!((v.length() - 23.07).abs() < 1e-3, "{v}");
        assert!((v.normalize() - Vec2::new(0.6, 0.8)).length() < 1e-5);
    }
}
