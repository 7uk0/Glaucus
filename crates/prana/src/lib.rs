//! # prana — the breath that runs the machines
//!
//! In yogic physiology *prana* is the life force, carried through the body
//! along channels, the *nadis*, and gathered at centres. Here it is electricity:
//! carried along cables (node networks, [`nadi`]), gathered and released by
//! batteries (SMES, substations, APCs), and drawn by every machine that keeps
//! a ship alive. Ports of `Content.Server/Power`, `Content.Server/NodeContainer`
//! and `Content.Server/Gravity`:
//!
//! - [`nadi`]: `NodeContainer` + cable/device/terminal nodes → networks.
//! - [`pow3r`]: the `BatteryRampPegSolver`, line for line.
//! - [`net`]: supplies, consumers, batteries, APCs, LV providers and the
//!   machines (`ApcPowerReceiver`) that link to them; the per-tick solve.
//! - [`devices`]: fuel generators (P.A.C.M.A.N. family), charge-up machines,
//!   gravity generators and grid gravity.
//!
//! Effects elsewhere go through `athanor::power`: unpowered thrusters stop,
//! airlocks freeze, consoles go dark, and grids lose gravity.

pub mod devices;
pub mod nadi;
pub mod net;
pub mod pow3r;

use athanor::spawn::ComponentFactories;
use athanor::{SimSet, SimTick};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

pub use net::{ApcPowerReceiver, Battery, PowerChanged, PowerNetworkBattery, PowerSupplier, PowerTopology};

pub struct PranaPlugin;

impl Plugin for PranaPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PowerChanged>();
        app.add_systems(
            SimTick,
            (
                devices::start_generators,
                devices::generator_system,
                devices::power_charge_system,
                net::power_system,
                devices::gravity_system,
            )
                .chain()
                .in_set(SimSet::Gameplay)
                .after(athanor::doors::door_update),
        );
        register(&mut app.world_mut().resource_mut::<ComponentFactories>());
    }
}

pub fn register(reg: &mut ComponentFactories) {
    nadi::register(reg);
    net::register(reg);
    devices::register(reg);
}
