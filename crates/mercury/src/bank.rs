//! Bank accounts and ATMs (`BankAccountComponent`, `ATMSystem`).

use athanor::events::{Popup, PopupKind, SoundAt};
use athanor::interaction::take_from_hands;
use athanor::spawn::{delete, EntityData};
use bevy_ecs::prelude::*;

use crate::Stack;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BankAccount {
    pub balance: i64,
}

fn is_atm(world: &World, e: Entity) -> bool {
    world.get::<EntityData>(e).is_some_and(|d| d.has("BankATM"))
}

fn say(world: &mut World, at: Entity, text: String) {
    world.write_message(Popup {
        text,
        at,
        kind: PopupKind::Small,
    });
}

pub fn balance(world: &World, who: Entity) -> Option<i64> {
    world.get::<BankAccount>(who).map(|b| b.balance)
}

pub fn try_withdraw(world: &mut World, who: Entity, amount: i64) -> bool {
    let Some(mut acct) = world.get_mut::<BankAccount>(who) else { return false };
    if acct.balance < amount {
        return false;
    }
    acct.balance -= amount;
    true
}

pub fn deposit(world: &mut World, who: Entity, amount: i64) {
    if let Some(mut acct) = world.get_mut::<BankAccount>(who) {
        acct.balance += amount;
    }
}

/// Click an ATM with an empty hand: show your balance.
pub fn on_interact_atm(world: &mut World, user: Entity, target: Entity) -> bool {
    if !is_atm(world, target) {
        return false;
    }
    match balance(world, user) {
        Some(b) => say(world, user, format!("Your balance: {b} spesos.")),
        None => say(world, user, "The ATM can't find an account for you.".into()),
    }
    true
}

/// Use cash on an ATM: deposit the whole stack.
pub fn on_atm_using(world: &mut World, user: Entity, used: Entity, target: Entity) -> bool {
    if !is_atm(world, target) {
        return false;
    }
    let Some(stack) = world.get::<Stack>(used).cloned() else { return false };
    if stack.stack_type != "Credit" {
        return false;
    }
    if world.get::<BankAccount>(user).is_none() {
        say(world, user, "The ATM can't find an account for you.".into());
        return true;
    }
    take_from_hands(world, user, used);
    delete(world, used);
    deposit(world, user, stack.count as i64);
    let b = balance(world, user).unwrap_or(0);
    say(world, user, format!("Deposited {} spesos. Balance: {b}.", stack.count));
    world.write_message(SoundAt {
        path: "/Audio/Effects/Cargo/ping.ogg".into(),
        at: target,
    });
    true
}
