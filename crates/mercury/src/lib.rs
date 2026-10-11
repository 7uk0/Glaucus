//! # mercury — commerce and the roads between stars
//!
//! Mercury was the god of merchants and travellers; this crate is Frontier's
//! economy. Ports of the `_NF` bank (`BankSystem`, `ATMSystem`), the
//! appraisal `PricingSystem`, Frontier's pallet-console selling
//! (`NFCargoSystem`) and the shipyard (`ShipyardSystem`).
//!
//! The loop: start with Frontier's 30,000-speso default balance, buy a
//! shuttle at the shipyard console, haul salvage back to a sell pallet,
//! sell it for cash at the pallet console, deposit the cash at an ATM.

pub mod bank;
pub mod cargo;
pub mod pricing;
pub mod shipyard;

use athanor::spawn::ComponentFactories;
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use grimoire::Node;

pub use bank::BankAccount;
pub use shipyard::{buy_vessel, Vessel, Vessels};

/// Frontier's `HumanoidCharacterProfile.DefaultBalance`.
pub const DEFAULT_BALANCE: i64 = 30_000;

/// `StackComponent`: how many of a stackable thing this is.
#[derive(Component, Debug, Clone)]
pub struct Stack {
    pub stack_type: String,
    pub count: u32,
}

/// The client opens a window in response to these.
#[derive(Message, Debug, Clone)]
pub enum UiRequest {
    Shipyard { user: Entity, console: Entity },
}

pub struct MercuryPlugin;

impl Plugin for MercuryPlugin {
    fn build(&self, app: &mut App) {
        let content = app.world().resource::<athanor::Content>().clone();
        app.insert_resource(Vessels::from_prototypes(&content.protos));
        app.add_message::<UiRequest>();
        app.add_systems(athanor::SimTick, open_accounts.in_set(athanor::SimSet::Gameplay));
        let mut reg = app.world_mut().resource_mut::<ComponentFactories>();
        register(&mut reg);
        let world = app.world_mut();
        let mut h = world.get_resource_or_init::<athanor::interaction::InteractionHandlers>();
        h.on_interact_hand.push(cargo::on_interact_pallet_console);
        h.on_interact_hand.push(shipyard::on_interact_shipyard);
        h.on_interact_hand.push(bank::on_interact_atm);
        h.on_interact_using.push(bank::on_atm_using);
    }
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("Stack", |e, node, _| {
        e.insert(Stack {
            stack_type: node.get_str("stackType").unwrap_or("").to_string(),
            count: node.get("count").and_then(Node::as_i64).unwrap_or(1).max(0) as u32,
        });
    });
}

/// Players get a bank account with Frontier's default balance.
pub fn open_accounts(mut commands: Commands, q: Query<Entity, (With<athanor::Player>, Without<BankAccount>)>) {
    for e in q.iter() {
        commands.entity(e).insert(BankAccount { balance: DEFAULT_BALANCE });
    }
}

/// Spawn a stack of something (e.g. `Credit` → `SpaceCash`) on a grid.
pub fn spawn_stack(world: &mut World, stack_id: &str, count: u32, parent: Entity, pos: bevy_math::Vec2) -> Option<Entity> {
    let content = world.resource::<athanor::Content>().clone();
    let proto = content.protos.get("stack", stack_id)?.get_str("spawn")?.to_string();
    let e = athanor::spawn::spawn_proto(world, &proto, parent, pos)?;
    if let Some(mut s) = world.get_mut::<Stack>(e) {
        s.count = count;
    } else {
        world.entity_mut(e).insert(Stack {
            stack_type: stack_id.to_string(),
            count,
        });
    }
    Some(e)
}
