//! Hooks that let a power simulation (the `prana` crate) gate athanor's
//! systems without athanor depending on it. Mirrors upstream's
//! `IsPowered(uid)`, which is true for anything that has no power receiver:
//! nothing here is marked until a power plugin marks it.

use bevy_ecs::prelude::*;

/// The entity needs power and isn't getting it (`ApcPowerReceiver.Powered == false`).
/// Thrusters and gyroscopes stop, airlocks freeze, consoles go dark.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Unpowered;

/// On a grid: no artificial gravity (`GravityComponent.Enabled == false`).
/// Movement on its tiles is weightless.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Weightless;

pub fn is_powered(world: &World, e: Entity) -> bool {
    world.get::<Unpowered>(e).is_none()
}
