//! # alembic — the still where air is made and lost
//!
//! The alembic is the alchemist's still: vapour rises, condenses, and runs
//! off. This crate is atmospherics — ports of `Content.Server/Atmos` and the
//! breathing half of `Content.Server/Body`:
//!
//! - [`gas`]: `GasMixture` and its math, with Frontier's CVars;
//! - [`atmos`]: per-tile air loaded from each grid's saved
//!   `GridAtmosphere`, airtight walls and doors, LINDA gas sharing at
//!   Frontier's 15 Hz atmos tick, space as a vacuum sink, space wind;
//! - [`breathing`]: `Respirator` (lungs, saturation, suffocation),
//!   `Barotrauma` with `PressureProtection` suits and helmets.
//!
//! Open an airlock to space and the room bleeds out, loose things are
//! dragged toward the breach, and anyone without a hardsuit starts to suffocate
//! and bruise.

pub mod atmos;
pub mod breathing;
pub mod gas;

use athanor::spawn::ComponentFactories;
use athanor::{Content, SimClock, SimSet, SimTick};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

pub use atmos::{air_at, GridAtmos, Gases};
pub use breathing::{Barotrauma, PressureProtection, Respirator};
pub use gas::{GasMixture, GasTable};

/// Simulation ticks per atmos tick: 30 Hz / Frontier's `atmos.tickrate` 15.
pub const TICKS_PER_ATMOS: u64 = 2;

fn atmos_tick(clock: Res<SimClock>) -> bool {
    clock.tick.is_multiple_of(TICKS_PER_ATMOS)
}

pub struct AlembicPlugin;

impl Plugin for AlembicPlugin {
    fn build(&self, app: &mut App) {
        let content = app.world().resource::<Content>().clone();
        app.insert_resource(Gases(GasTable::from_prototypes(&content.protos)))
            .insert_resource(breathing::Lungs(std::sync::Arc::new(breathing::LungData::from_prototypes(&content.protos))))
            .init_resource::<breathing::BarotraumaTimer>();
        app.add_systems(
            SimTick,
            (
                atmos::init_grid_atmos,
                (atmos::update_airtight, atmos::process_atmos, atmos::space_wind).chain().run_if(atmos_tick),
                breathing::respirator_system,
                breathing::barotrauma_system,
            )
                .chain()
                .in_set(SimSet::Gameplay)
                .after(athanor::doors::door_update),
        );
        register(&mut app.world_mut().resource_mut::<ComponentFactories>());
    }
}

pub fn register(reg: &mut ComponentFactories) {
    breathing::register(reg);
}
