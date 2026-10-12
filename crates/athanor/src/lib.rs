//! # athanor — the furnace
//!
//! The alchemist's athanor keeps a steady, self-feeding heat; this crate is
//! the steady tick of the world. It is the Rust port of SS14's game systems
//! (Robust.Shared + Content.Shared + Content.Server), built on `bevy_ecs`
//! and fed by upstream prototypes and maps through `grimoire`/`astrolabe`.
//!
//! The simulation is headless. It runs in its own [`SimTick`] schedule at a
//! fixed rate (30 Hz, upstream's default tickrate) and is driven either once
//! per `App::update` (tests, servers) or from real time (the client).
//!
//! Module map (upstream equivalents in parentheses):
//! * [`content`] — loaded prototypes/tiles/locale (`IPrototypeManager`, ...)
//! * [`transform`] — transforms, maps, grids, anchoring (`SharedTransformSystem`)
//! * [`spawn`] — prototype spawning, component factories (`EntityManager`)
//! * [`map_loader`] — map files → entities (`MapLoaderSystem`)
//! * [`physics`] — bodies, fixtures, collision (`SharedPhysicsSystem`)
//! * [`movement`] — mob movement (`SharedMoverController`)
//! * [`interaction`] — hands, items, examine (`SharedHandsSystem`, ...)
//! * [`combat`] — melee (`SharedMeleeWeaponSystem`)
//! * [`damage`] — damage, mob state, destructibles
//! * [`doors`] — doors and airlocks (`SharedDoorSystem`)
//! * [`shuttles`] — shuttles, thrusters, piloting (`ShuttleSystem`, `MoverController`)
//! * [`factions`] — NPC factions (`NpcFactionSystem`)
//! * [`storage`] — lockers, crates, backpacks, fills, entity tables
//! * [`spawners`] — map-init spawners (`ConditionalSpawnerSystem`)
//! * [`inventory`] — inventories, clothing, loadouts (`InventorySystem`)
//! * [`naming`] — random names from datasets (`RandomMetadata`)
//! * [`sector`] — single-player sector setup (stand-in for `NFAdventureRule`)

pub mod combat;
pub mod content;
pub mod damage;
pub mod docking;
pub mod doors;
pub mod events;
pub mod factions;
pub mod interaction;
pub mod inventory;
pub mod map_loader;
pub mod movement;
pub mod naming;
pub mod physics;
pub mod power;
pub mod rng;
pub mod sector;
pub mod shuttles;
pub mod spawn;
pub mod spawners;
pub mod storage;
pub mod transform;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;

pub use content::Content;

/// Upstream's default `net.tickrate`.
pub const TICK_RATE: f32 = 30.0;

/// One simulation tick.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimTick;

/// Ordering inside a tick, mirroring upstream's `UpdatesBefore/After` chains.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimSet {
    /// Player/NPC intent is applied (input movers, verbs queued by the client).
    Input,
    /// NPC planning and operators (`homunculus` plugs in here).
    Ai,
    /// Mob movement controllers.
    Movement,
    /// Integration and collision.
    Physics,
    /// World poses, re-parenting.
    PostPhysics,
    /// Doors, damage reactions, everything else.
    Gameplay,
}

#[derive(Resource, Debug, Clone)]
pub struct SimClock {
    pub tick: u64,
    /// Seconds per tick.
    pub dt: f32,
    /// Simulation time in seconds (`IGameTiming.CurTime`).
    pub time: f64,
    pub paused: bool,
}

impl Default for SimClock {
    fn default() -> Self {
        SimClock {
            tick: 0,
            dt: 1.0 / TICK_RATE,
            time: 0.0,
            paused: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DriveMode {
    /// One tick per `App::update()` (headless/tests).
    EveryUpdate,
    /// Accumulate real seconds pushed into `pending` by the client.
    RealTime { pending: f32, max_ticks_per_frame: u32 },
}

#[derive(Resource, Debug, Clone, Copy)]
pub struct SimDrive(pub DriveMode);

/// The entity the local player controls.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Player;

pub struct AthanorPlugin {
    pub content: Content,
    pub drive: DriveMode,
}

impl AthanorPlugin {
    pub fn headless(content: Content) -> Self {
        AthanorPlugin {
            content,
            drive: DriveMode::EveryUpdate,
        }
    }
}

impl Plugin for AthanorPlugin {
    fn build(&self, app: &mut App) {
        let factions = factions::FactionTable::from_prototypes(&self.content.protos);
        app.insert_resource(self.content.clone())
            .insert_resource(SimClock::default())
            .insert_resource(SimDrive(self.drive))
            .insert_resource(factions::Factions(factions))
            .init_resource::<rng::SimRng>()
            .init_resource::<physics::Contacts>()
            .init_resource::<transform::GridIndex>()
            .init_resource::<transform::MapIndex>()
            .init_resource::<spawn::ProtoCache>()
            .init_resource::<interaction::InteractionHandlers>()
            .add_message::<events::Popup>()
            .add_message::<events::SoundAt>()
            .add_message::<events::ChatLine>()
            .add_message::<events::MeleeSwing>()
            .add_message::<events::OpenStorage>();

        let mut reg = spawn::ComponentFactories::default();
        register_components(&mut reg);
        app.insert_resource(reg);

        app.init_schedule(SimTick);
        app.configure_sets(
            SimTick,
            (
                SimSet::Input,
                SimSet::Ai,
                SimSet::Movement,
                SimSet::Physics,
                SimSet::PostPhysics,
                SimSet::Gameplay,
            )
                .chain(),
        );
        app.add_systems(SimTick, advance_clock.before(SimSet::Input));
        app.add_systems(SimTick, movement::mover_system.in_set(SimSet::Movement));
        app.add_systems(
            SimTick,
            (
                shuttles::update_grid_bodies,
                shuttles::update_thrust,
                shuttles::shuttle_movement,
                docking::solve_welds,
                physics::physics_step,
            )
                .chain()
                .in_set(SimSet::Physics),
        );
        app.add_systems(SimTick, transform::update_world_poses.in_set(SimSet::PostPhysics));
        app.add_systems(
            SimTick,
            (
                doors::fixup_pending,
                doors::door_sync_collision,
                doors::bump_open,
                doors::door_update,
                naming::resolve_names,
                inventory::resolve_loadouts,
                storage::resolve_fills,
            )
                .chain()
                .in_set(SimSet::Gameplay),
        );
        app.add_systems(Update, drive_sim);

        doors::install(app.world_mut());
        shuttles::install(app.world_mut());
        storage::install(app.world_mut());
    }
}

/// Register every ported component factory.
pub fn register_components(reg: &mut spawn::ComponentFactories) {
    physics::register(reg);
    movement::register(reg);
    damage::register(reg);
    interaction::register(reg);
    combat::register(reg);
    doors::register(reg);
    docking::register(reg);
    factions::register(reg);
    spawners::register(reg);
    naming::register(reg);
    inventory::register(reg);
    shuttles::register(reg);
    storage::register(reg);
}

fn advance_clock(mut clock: ResMut<SimClock>) {
    clock.tick += 1;
    clock.time += clock.dt as f64;
}

/// Run `SimTick` as many times as the drive mode calls for.
pub fn drive_sim(world: &mut World) {
    if world.resource::<SimClock>().paused {
        return;
    }
    let dt = world.resource::<SimClock>().dt;
    let ticks = {
        let mut drive = world.resource_mut::<SimDrive>();
        match &mut drive.0 {
            DriveMode::EveryUpdate => 1,
            DriveMode::RealTime {
                pending,
                max_ticks_per_frame,
            } => {
                let mut n = 0;
                while *pending >= dt && n < *max_ticks_per_frame {
                    *pending -= dt;
                    n += 1;
                }
                if n == *max_ticks_per_frame {
                    // Falling behind: drop the backlog rather than spiral.
                    *pending = pending.min(dt);
                }
                n
            }
        }
    };
    for _ in 0..ticks {
        world.run_schedule(SimTick);
    }
}

/// Build a headless app around some content. `app.update()` = one tick.
pub fn headless_app(content: Content) -> App {
    let mut app = App::new();
    app.add_plugins(AthanorPlugin::headless(content));
    app.finish();
    app.cleanup();
    app
}
