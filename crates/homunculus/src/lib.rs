//! # homunculus — artificial minds
//!
//! The alchemists' homunculus was a small made being, animated by art. These
//! are the NPCs: a Rust port of SS14's `Content.Server/NPC` driven directly
//! by upstream data — `htnCompound` task networks, `utilityQuery` target
//! selection and `utilityCurvePreset` curves are read from the prototypes,
//! so every hostile carp, wandering mouse and xeno behaves the way its YAML
//! says it should.
//!
//! * [`htn`] — the domain and the planner (`HTNPlanJob`)
//! * [`ops`] — operators and preconditions as data
//! * [`mind`] — running plans (`HTNSystem`), NPC melee (`NPCCombatSystem`)
//! * [`utility`] — utility queries and considerations (`NPCUtilitySystem`)
//! * [`nav`] — A* pathfinding, line of sight, steering
//! * [`blackboard`] — `NPCBlackboard`

pub mod blackboard;
pub mod htn;
pub mod mind;
pub mod nav;
pub mod ops;
pub mod utility;

use athanor::{Content, SimSet, SimTick};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

pub use htn::Domain;
pub use mind::Htn;

pub struct HomunculusPlugin;

impl Plugin for HomunculusPlugin {
    fn build(&self, app: &mut App) {
        let content = app.world().resource::<Content>().clone();
        let domain = Domain::from_prototypes(&content.protos);
        if !domain.unported.is_empty() {
            tracing::info!("HTN: {} operator/precondition types not ported yet", domain.unported.len());
        }
        app.insert_resource(domain);
        app.world_mut()
            .resource_mut::<athanor::spawn::ComponentFactories>()
            .register_with(mind::register);
        app.add_systems(
            SimTick,
            (mind::sleep_incapacitated, mind::htn_system, mind::melee_system, nav::steering_system)
                .chain()
                .in_set(SimSet::Ai),
        );
    }
}

/// Helper so plugins can register factories through a `Mut`.
trait RegisterWith {
    fn register_with(&mut self, f: fn(&mut athanor::spawn::ComponentFactories));
}

impl RegisterWith for Mut<'_, athanor::spawn::ComponentFactories> {
    fn register_with(&mut self, f: fn(&mut athanor::spawn::ComponentFactories)) {
        f(self);
    }
}
