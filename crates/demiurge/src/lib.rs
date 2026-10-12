//! Demiurge: the craftsman who shapes the cosmos out of formless matter.
//!
//! World generation for the Frontier sector:
//! - [`poi`]: Frontier's points of interest (outposts, depots, the market,
//!   wrecks and stations at their distances from the origin).
//! - [`world`]: SS14's chunked space worldgen. Noise-driven biomes fill the
//!   void with asteroids and debris around players and shuttle consoles,
//!   then garbage-collect it when nobody's near.

pub mod noise;
pub mod poi;
pub mod poisson;
pub mod table;
pub mod world;

use athanor::{SimSet, SimTick};
use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;

pub use poi::{generate_pois, PlacedPoi, PoiConfig};
pub use world::Cosmos;

/// Registers the worldgen components and runs chunk loading, locality
/// population and debris GC every tick once a [`Cosmos`] exists.
pub struct DemiurgePlugin;

impl Plugin for DemiurgePlugin {
    fn build(&self, app: &mut App) {
        world::register(&mut app.world_mut().resource_mut::<athanor::spawn::ComponentFactories>());
        app.add_systems(
            SimTick,
            (world::world_controller, world::locality_loader, world::gc_queue)
                .chain()
                .in_set(SimSet::Gameplay)
                .run_if(resource_exists::<Cosmos>),
        );
    }
}
