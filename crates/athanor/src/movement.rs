//! Mob movement. Port of `SharedMoverController` (Content.Shared/Movement):
//! wish direction → acceleration/friction on the body's velocity, with tile
//! friction, sprint/walk speeds and weightlessness off-grid.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use serde::Deserialize;

use crate::content::Content;
use crate::physics::Body;
use crate::spawn::ComponentFactories;
use crate::transform::{tile_of, vec_to_angle, Grid, Xform};
use crate::{data_component, SimClock};

/// `InputMoverComponent`: what direction the controlling player/NPC wants.
#[derive(Component, Debug, Clone, Copy)]
pub struct InputMover {
    /// Desired direction in the parent (grid) frame; length ≤ 1.
    pub wish: Vec2,
    /// SS14 defaults to running; holding walk slows you.
    pub walking: bool,
    pub can_move: bool,
}

impl Default for InputMover {
    fn default() -> Self {
        InputMover {
            wish: Vec2::ZERO,
            walking: false,
            can_move: true,
        }
    }
}

/// `MovementSpeedModifierComponent`, with upstream defaults.
#[derive(Component, Debug, Clone, Copy, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MovementSpeedModifier {
    pub base_walk_speed: f32,
    pub base_sprint_speed: f32,
    pub base_acceleration: f32,
    pub base_friction: f32,
    pub friction_no_input: Option<f32>,
    pub minimum_friction_speed: f32,
    pub base_weightless_friction: f32,
    pub base_weightless_modifier: f32,
    pub base_weightless_acceleration: f32,
    pub off_grid_friction: Option<f32>,
    #[serde(skip)]
    pub walk_modifier: f32,
    #[serde(skip)]
    pub sprint_modifier: f32,
}

impl Default for MovementSpeedModifier {
    fn default() -> Self {
        MovementSpeedModifier {
            base_walk_speed: 2.5,
            base_sprint_speed: 4.5,
            base_acceleration: 20.0,
            base_friction: 2.5,
            friction_no_input: None,
            minimum_friction_speed: 0.005,
            base_weightless_friction: 1.0,
            base_weightless_modifier: 0.7,
            base_weightless_acceleration: 1.0,
            off_grid_friction: None,
            walk_modifier: 1.0,
            sprint_modifier: 1.0,
        }
    }
}

impl MovementSpeedModifier {
    pub fn walk_speed(&self) -> f32 {
        self.base_walk_speed * self.walk_modifier
    }
    pub fn sprint_speed(&self) -> f32 {
        self.base_sprint_speed * self.sprint_modifier
    }
}

/// `MovementAlwaysTouching`: can push off nothing (flying mobs, carp).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct AlwaysTouching;

/// Marker for mobs that use mob movement (`MobMoverComponent`).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct MobMover;

pub const AIR_FRICTION: f32 = 0.2;
pub const OFF_GRID_FRICTION: f32 = 0.05;

pub fn register(reg: &mut ComponentFactories) {
    reg.register("InputMover", |e, _, _| {
        e.insert(InputMover::default());
    });
    reg.register("MobMover", |e, _, _| {
        e.insert(MobMover);
    });
    reg.register("MovementAlwaysTouching", |e, _, _| {
        e.insert(AlwaysTouching);
    });
    data_component!(reg, "MovementSpeedModifier", MovementSpeedModifier);
}

/// `SharedMoverController.HandleMobMovement`, per tick.
pub fn mover_system(
    clock: Res<SimClock>,
    content: Res<Content>,
    grids: Query<&Grid>,
    weightless_grids: Query<(), With<crate::power::Weightless>>,
    mut q: Query<(&InputMover, Option<&MovementSpeedModifier>, &mut Body, &mut Xform, Has<AlwaysTouching>)>,
) {
    let dt = clock.dt;
    let def = MovementSpeedModifier::default();
    for (mover, speed, mut body, mut xform, always_touching) in q.iter_mut() {
        if xform.anchored {
            continue;
        }
        let speed = speed.unwrap_or(&def);
        let wish_in = if mover.can_move { mover.wish.clamp_length_max(1.0) } else { Vec2::ZERO };

        // Weightless = not standing on a tile of a grid, or the grid has no
        // gravity (`power::Weightless`, set by the power plugin from gravity generators).
        let tile_def = xform
            .parent
            .and_then(|p| grids.get(p).ok())
            .map(|g| g.tiles.get(tile_of(xform.pos).x, tile_of(xform.pos).y))
            .filter(|t| !t.is_empty())
            .and_then(|t| content.tiles.get(t.type_id));
        let no_gravity = xform.parent.is_some_and(|p| weightless_grids.contains(p));
        let weightless = tile_def.is_none() || no_gravity;
        let on_grid = xform.parent.is_some_and(|p| grids.contains(p));

        let mut velocity = body.velocity;
        let (wish, mut friction, accel) = if weightless {
            let walk = speed.base_weightless_modifier * speed.walk_speed();
            let sprint = speed.base_weightless_modifier * speed.sprint_speed();
            let wish = wish_in * if mover.walking { walk } else { sprint };
            let friction = if on_grid || always_touching {
                if wish != Vec2::ZERO {
                    speed.base_weightless_friction
                } else {
                    speed.base_weightless_friction
                }
            } else {
                speed.off_grid_friction.unwrap_or(OFF_GRID_FRICTION)
            };
            (wish, friction, speed.base_weightless_acceleration)
        } else {
            let wish = wish_in * if mover.walking { speed.walk_speed() } else { speed.sprint_speed() };
            let td = tile_def.expect("weightless when no tile");
            let friction = if wish != Vec2::ZERO {
                speed.base_friction * td.mob_friction.or(td.friction).unwrap_or(1.0)
            } else {
                speed.friction_no_input.unwrap_or(speed.base_friction) * td.friction.unwrap_or(1.0)
            };
            let accel = speed.base_acceleration * td.mob_acceleration.unwrap_or(1.0);
            (wish, friction, accel)
        };
        if wish != Vec2::ZERO {
            friction = friction.min(accel);
        }
        apply_friction(speed.minimum_friction_speed, dt, friction, &mut velocity);
        let touching = !weightless || on_grid || always_touching;
        if touching {
            accelerate(&mut velocity, wish, accel, dt);
        }
        body.velocity = velocity;

        // Face the way we are trying to go.
        if wish != Vec2::ZERO {
            xform.rot = vec_to_angle(wish);
        }
    }
}

pub fn apply_friction(min_speed: f32, dt: f32, friction: f32, v: &mut Vec2) {
    if v.length() < min_speed {
        return;
    }
    *v *= (1.0 - dt * friction).clamp(0.0, 1.0);
}

/// Quake-style acceleration toward a wish velocity (upstream `Accelerate`).
pub fn accelerate(v: &mut Vec2, wish_velocity: Vec2, accel: f32, dt: f32) {
    let wish_dir = wish_velocity.normalize_or_zero();
    let wish_speed = wish_velocity.length();
    let current = v.dot(wish_dir);
    let add = wish_speed - current;
    if add <= 0.0 {
        return;
    }
    let accel_speed = (accel * dt * wish_speed).min(add);
    *v += wish_dir * accel_speed;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerates_to_speed_and_stops() {
        let mut v = Vec2::ZERO;
        for _ in 0..30 {
            apply_friction(0.005, 1.0 / 30.0, 2.5, &mut v);
            accelerate(&mut v, Vec2::new(4.5, 0.0), 20.0, 1.0 / 30.0);
        }
        assert!(v.x > 4.0 && v.x <= 4.5 + 1e-4, "{v}");
        for _ in 0..90 {
            apply_friction(0.005, 1.0 / 30.0, 2.5, &mut v);
        }
        assert!(v.x < 0.05, "{v}");
    }
}
