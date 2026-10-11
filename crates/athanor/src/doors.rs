//! Doors and airlocks. Port of `SharedDoorSystem` / `SharedAirlockSystem`:
//! the two-phase open/close state machine (collision turns off part-way
//! through opening and back on part-way through closing), bump-to-open,
//! airlock auto-close, and refusing to close on someone standing in the way.

use bevy_ecs::prelude::*;
use grimoire::Node;

use crate::events::SoundAt;
use crate::interaction::InteractionHandlers;
use crate::movement::InputMover;
use crate::physics::{Body, Contacts};
use crate::spawn::ComponentFactories;
use crate::transform::{tile_of, Grid, Xform};
use crate::SimClock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorState {
    Closed,
    Closing,
    Open,
    Opening,
    Welded,
    Denying,
    Emagging,
}

/// `DoorComponent` (+ the bits of `AirlockComponent` we need).
#[derive(Component, Debug, Clone)]
pub struct Door {
    pub state: DoorState,
    /// True between the two phases of opening/closing.
    pub partial: bool,
    pub next_change: Option<f64>,
    pub open_time_one: f64,
    pub open_time_two: f64,
    pub close_time_one: f64,
    pub close_time_two: f64,
    pub deny_duration: f64,
    pub bump_open: bool,
    pub open_sound: Option<String>,
    pub close_sound: Option<String>,
    pub deny_sound: Option<String>,
    /// Airlocks close themselves after this many seconds open.
    pub auto_close: Option<f64>,
    pub bolted: bool,
    /// Has an `AirlockComponent`: needs power to open or close (`CanChangeState`).
    pub airlock: bool,
}

impl Default for Door {
    fn default() -> Self {
        Door {
            state: DoorState::Closed,
            partial: false,
            next_change: None,
            open_time_one: 0.4,
            open_time_two: 0.2,
            close_time_one: 0.4,
            close_time_two: 0.2,
            deny_duration: 0.45,
            bump_open: true,
            open_sound: None,
            close_sound: None,
            deny_sound: None,
            auto_close: None,
            bolted: false,
            airlock: false,
        }
    }
}

impl Door {
    pub fn is_closed(&self) -> bool {
        matches!(self.state, DoorState::Closed | DoorState::Welded | DoorState::Denying)
    }

    /// Should the door's fixture collide right now?
    pub fn collidable(&self) -> bool {
        match self.state {
            DoorState::Closed | DoorState::Welded | DoorState::Denying | DoorState::Emagging => true,
            DoorState::Opening => !self.partial,
            DoorState::Closing => self.partial,
            DoorState::Open => false,
        }
    }
}

fn sound(node: &Node, key: &str) -> Option<String> {
    let s = node.get(key)?;
    s.get_str("path")
        .or_else(|| s.get_str("collection"))
        .or_else(|| s.as_str())
        .map(str::to_owned)
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("Door", |e, node, _| {
        let mut d = Door::default();
        let secs = |k: &str| node.get(k).and_then(Node::as_seconds);
        if let Some(v) = secs("openTimeOne") {
            d.open_time_one = v;
        }
        if let Some(v) = secs("openTimeTwo") {
            d.open_time_two = v;
        }
        if let Some(v) = secs("closeTimeOne") {
            d.close_time_one = v;
        }
        if let Some(v) = secs("closeTimeTwo") {
            d.close_time_two = v;
        }
        if let Some(v) = secs("denyDuration") {
            d.deny_duration = v;
        }
        if let Some(b) = node.get("bumpOpen").and_then(Node::as_bool) {
            d.bump_open = b;
        }
        if let Some(s) = node.get_str("state") {
            d.state = match s {
                "Open" => DoorState::Open,
                "Welded" => DoorState::Welded,
                _ => DoorState::Closed,
            };
        }
        d.open_sound = sound(node, "openSound");
        d.close_sound = sound(node, "closeSound");
        d.deny_sound = sound(node, "denySound");
        let collide = d.collidable();
        e.insert(d);
        if let Some(mut b) = e.get_mut::<Body>() {
            b.can_collide = collide;
        }
    });
    reg.register("Airlock", |e, node, _| {
        let delay = node.get("autoCloseDelay").and_then(Node::as_seconds).unwrap_or(5.0);
        if let Some(mut d) = e.get_mut::<Door>() {
            d.auto_close = Some(delay);
            d.airlock = true;
        } else {
            // Door comes later in the component list; stash for the Door factory.
            e.insert(PendingAutoClose(delay));
        }
    });
    reg.register("DoorBolt", |e, node, _| {
        let bolted = node.get("boltsDown").and_then(Node::as_bool).unwrap_or(false);
        if let Some(mut d) = e.get_mut::<Door>() {
            d.bolted = bolted;
        }
    });
}

#[derive(Component)]
pub struct PendingAutoClose(pub f64);

pub fn install(world: &mut World) {
    let mut h = world.get_resource_or_init::<InteractionHandlers>();
    h.on_interact_hand.push(|world, user, target| {
        if world.get::<Door>(target).is_none() {
            return false;
        }
        toggle(world, target, Some(user));
        true
    });
}

/// Fold airlock auto-close settings that arrived before the Door component.
pub fn fixup_pending(mut commands: Commands, mut q: Query<(Entity, &PendingAutoClose, &mut Door)>) {
    for (e, p, mut d) in q.iter_mut() {
        d.auto_close = Some(p.0);
        d.airlock = true;
        commands.entity(e).remove::<PendingAutoClose>();
    }
}

pub fn toggle(world: &mut World, door: Entity, user: Option<Entity>) -> bool {
    let Some(d) = world.get::<Door>(door) else { return false };
    if d.is_closed() {
        try_open(world, door, user)
    } else {
        try_close(world, door, user)
    }
}

/// `SharedAirlockSystem.CanChangeState`: airlocks need power.
fn airlock_frozen(world: &World, door: Entity) -> bool {
    world.get::<Door>(door).is_some_and(|d| d.airlock) && !crate::power::is_powered(world, door)
}

pub fn try_open(world: &mut World, door: Entity, _user: Option<Entity>) -> bool {
    let now = world.resource::<SimClock>().time;
    if airlock_frozen(world, door) {
        return false;
    }
    let Some(mut d) = world.get_mut::<Door>(door) else { return false };
    if d.state != DoorState::Closed {
        return false;
    }
    if d.bolted {
        d.state = DoorState::Denying;
        d.next_change = Some(now + d.deny_duration);
        let s = d.deny_sound.clone();
        if let Some(s) = s {
            world.write_message(SoundAt { path: s, at: door });
        }
        return false;
    }
    d.state = DoorState::Opening;
    d.partial = false;
    d.next_change = Some(now + d.open_time_one);
    let s = d.open_sound.clone();
    if let Some(s) = s {
        world.write_message(SoundAt { path: s, at: door });
    }
    true
}

pub fn try_close(world: &mut World, door: Entity, _user: Option<Entity>) -> bool {
    let now = world.resource::<SimClock>().time;
    if airlock_frozen(world, door) || door_blocked(world, door) {
        return false;
    }
    let Some(mut d) = world.get_mut::<Door>(door) else { return false };
    if d.state != DoorState::Open || d.bolted {
        return false;
    }
    d.state = DoorState::Closing;
    d.partial = false;
    d.next_change = Some(now + d.close_time_one);
    let s = d.close_sound.clone();
    if let Some(s) = s {
        world.write_message(SoundAt { path: s, at: door });
    }
    true
}

/// Is a mob standing in the doorway? (Upstream checks physics overlaps;
/// mobs-on-tile is the case that matters.)
fn door_blocked(world: &mut World, door: Entity) -> bool {
    let Some(x) = world.get::<Xform>(door).copied() else { return false };
    let Some(parent) = x.parent else { return false };
    let tile = tile_of(x.pos);
    if world.get::<Grid>(parent).is_none() {
        return false;
    }
    let mut q = world.query::<(&Xform, &InputMover)>();
    q.iter(world)
        .any(|(mx, _)| mx.parent == Some(parent) && tile_of(mx.pos) == tile)
}

/// Mobs bumping a closed door open it (`DoorComponent.BumpOpen`).
pub fn bump_open(world: &mut World) {
    let contacts = world.get_resource::<Contacts>().map(|c| c.0.clone()).unwrap_or_default();
    for (mover, obstacle) in contacts {
        if world.get::<InputMover>(mover).is_none() {
            continue;
        }
        let Some(d) = world.get::<Door>(obstacle) else { continue };
        if d.bump_open && d.state == DoorState::Closed {
            try_open(world, obstacle, Some(mover));
        }
    }
}

/// Advance door state machines.
pub fn door_update(world: &mut World) {
    let now = world.resource::<SimClock>().time;
    let doors: Vec<Entity> = {
        let mut q = world.query::<(Entity, &Door)>();
        q.iter(world).filter(|(_, d)| d.next_change.is_some()).map(|(e, _)| e).collect()
    };
    for e in doors {
        let Some(d) = world.get::<Door>(e).cloned() else { continue };
        let Some(t) = d.next_change else { continue };
        if now < t {
            continue;
        }
        let mut nd = d.clone();
        match d.state {
            DoorState::Opening if !d.partial => {
                nd.partial = true;
                nd.next_change = Some(now + d.open_time_two);
            }
            DoorState::Opening => {
                nd.state = DoorState::Open;
                nd.partial = false;
                nd.next_change = d.auto_close.map(|a| now + a);
            }
            DoorState::Closing if !d.partial => {
                nd.partial = true;
                nd.next_change = Some(now + d.close_time_two);
            }
            DoorState::Closing => {
                nd.state = DoorState::Closed;
                nd.partial = false;
                nd.next_change = None;
            }
            DoorState::Open => {
                // Auto-close timer fired. Unpowered airlocks stay open until
                // power returns (`AirlockSystem.OnPowerChanged`).
                if airlock_frozen(world, e) {
                    nd.next_change = None;
                } else if door_blocked(world, e) {
                    nd.next_change = Some(now + 1.0);
                } else {
                    let _ = world.get_mut::<Door>(e).map(|mut x| x.next_change = None);
                    try_close(world, e, None);
                    let collide = world.get::<Door>(e).map(Door::collidable).unwrap_or(true);
                    if let Some(mut b) = world.get_mut::<Body>(e) {
                        b.can_collide = collide;
                    }
                    continue;
                }
            }
            DoorState::Denying | DoorState::Emagging => {
                nd.state = DoorState::Closed;
                nd.next_change = None;
            }
            _ => nd.next_change = None,
        }
        let collide = nd.collidable();
        world.entity_mut(e).insert(nd);
        if let Some(mut b) = world.get_mut::<Body>(e) {
            b.can_collide = collide;
        }
    }
}

/// Keep door collision in sync with state (covers freshly spawned doors whose
/// `Physics` component arrived after `Door`).
pub fn door_sync_collision(mut q: Query<(&Door, &mut Body), Changed<Door>>) {
    for (d, mut b) in q.iter_mut() {
        let c = d.collidable();
        if b.can_collide != c {
            b.can_collide = c;
        }
    }
}

/// Power came back to an airlock: an open one resumes its auto-close
/// countdown (`AirlockSystem.OnPowerChanged` → `UpdateAutoClose`).
pub fn on_power_restored(world: &mut World, door: Entity) {
    let now = world.resource::<SimClock>().time;
    if let Some(mut d) = world.get_mut::<Door>(door)
        && d.state == DoorState::Open
        && d.next_change.is_none()
        && let Some(delay) = d.auto_close
    {
        d.next_change = Some(now + delay);
    }
}
