//! Interaction: hands, items, picking up, dropping, using and examining.
//! Ports of the core of `SharedHandsSystem`, `SharedInteractionSystem` and
//! `ExamineSystem`.
//!
//! Interactions are verbs — functions taking `&mut World` — that the client
//! calls for the player and NPC operators call for AI. Other modules hook
//! "interact with this entity" by registering a handler in
//! [`InteractionHandlers`], the equivalent of subscribing to
//! `InteractHandEvent` and setting `Handled`.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use grimoire::Node;

use crate::events::{ChatChannel, ChatLine, Popup, PopupKind};
use crate::physics::Body;
use crate::spawn::{ComponentFactories, MetaData};
use crate::transform::{world_pose_of, Xform};

/// `SharedInteractionSystem.InteractionRange`.
pub const INTERACTION_RANGE: f32 = 1.5;

#[derive(Component, Debug, Clone, Default)]
pub struct Item {
    pub size: String,
    pub held_prefix: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Hand {
    pub name: String,
    pub held: Option<Entity>,
}

/// `HandsComponent`. Upstream hands come from body parts; a humanoid gets a
/// right and left hand.
#[derive(Component, Debug, Clone)]
pub struct Hands {
    pub hands: Vec<Hand>,
    pub active: usize,
}

impl Default for Hands {
    fn default() -> Self {
        Hands {
            hands: vec![
                Hand {
                    name: "right hand".into(),
                    held: None,
                },
                Hand {
                    name: "left hand".into(),
                    held: None,
                },
            ],
            active: 0,
        }
    }
}

impl Hands {
    pub fn active_item(&self) -> Option<Entity> {
        self.hands.get(self.active).and_then(|h| h.held)
    }

    pub fn active_free(&self) -> bool {
        self.hands.get(self.active).is_some_and(|h| h.held.is_none())
    }

    pub fn holding(&self, e: Entity) -> bool {
        self.hands.iter().any(|h| h.held == Some(e))
    }

    pub fn free_hand(&self) -> Option<usize> {
        if self.active_free() {
            return Some(self.active);
        }
        self.hands.iter().position(|h| h.held.is_none())
    }
}

/// The entity is inside something (a hand, a backpack, a locker) and is not
/// physically in the world.
#[derive(Component, Debug, Clone, Copy)]
pub struct InContainer {
    pub owner: Entity,
}

/// Something that can be "used" or interacted with by hand.
pub type InteractionHandler = fn(&mut World, Entity, Entity) -> bool;

/// Handlers tried in order by [`interact`]; the first returning `true` wins.
#[derive(Resource, Default)]
pub struct InteractionHandlers {
    pub on_interact_hand: Vec<InteractionHandler>,
    pub on_use_in_hand: Vec<InteractionHandler>,
    /// `(user, used item, target)`.
    pub on_interact_using: Vec<fn(&mut World, Entity, Entity, Entity) -> bool>,
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("Item", |e, node, _| {
        e.insert(Item {
            size: node.get_str("size").unwrap_or("Small").to_string(),
            held_prefix: node.get_str("heldPrefix").map(str::to_owned),
        });
    });
    reg.register("Hands", |e, _, _| {
        e.insert(Hands::default());
    });
}

pub fn distance(world: &World, a: Entity, b: Entity) -> f32 {
    let pa = world_pose_of(world, a);
    let pb = world_pose_of(world, b);
    if pa.map != pb.map {
        return f32::INFINITY;
    }
    pa.pos.distance(pb.pos)
}

pub fn in_range(world: &World, a: Entity, b: Entity, range: f32) -> bool {
    distance(world, a, b) <= range
}

fn name_of(world: &World, e: Entity) -> String {
    world.get::<MetaData>(e).map(|m| m.name.clone()).unwrap_or_else(|| "something".into())
}

fn popup(world: &mut World, at: Entity, text: String) {
    world.write_message(Popup {
        text,
        at,
        kind: PopupKind::Small,
    });
}

/// Put `item` into one of `user`'s hands.
pub fn try_pickup(world: &mut World, user: Entity, item: Entity) -> Result<(), String> {
    if world.get::<Item>(item).is_none() {
        return Err("can't pick that up".into());
    }
    if world.get::<InContainer>(item).is_some() {
        return Err("it's already held".into());
    }
    if world.get::<Xform>(item).is_some_and(|x| x.anchored) {
        return Err("it's fastened down".into());
    }
    if !in_range(world, user, item, INTERACTION_RANGE) {
        return Err("too far away".into());
    }
    let Some(hand) = world.get::<Hands>(user).and_then(Hands::free_hand) else {
        return Err("your hands are full".into());
    };
    world.get_mut::<Hands>(user).unwrap().hands[hand].held = Some(item);
    if let Some(mut x) = world.get_mut::<Xform>(item) {
        x.parent = Some(user);
        x.pos = Vec2::ZERO;
        x.rot = 0.0;
    }
    if let Some(mut b) = world.get_mut::<Body>(item) {
        b.velocity = Vec2::ZERO;
    }
    world.entity_mut(item).insert(InContainer { owner: user });
    let n = name_of(world, item);
    popup(world, user, format!("You pick up the {n}."));
    Ok(())
}

/// Drop whatever is in the active hand at the user's feet.
pub fn try_drop(world: &mut World, user: Entity) -> Option<Entity> {
    let (idx, item) = {
        let hands = world.get::<Hands>(user)?;
        (hands.active, hands.active_item()?)
    };
    drop_from_hand(world, user, idx, item);
    Some(item)
}

fn drop_from_hand(world: &mut World, user: Entity, idx: usize, item: Entity) {
    world.get_mut::<Hands>(user).unwrap().hands[idx].held = None;
    let ux = world.get::<Xform>(user).copied();
    if let (Some(ux), Some(mut x)) = (ux, world.get_mut::<Xform>(item)) {
        x.parent = ux.parent;
        x.pos = ux.pos;
    }
    world.entity_mut(item).remove::<InContainer>();
}

/// Remove an item from whichever hand holds it, without dropping it to the
/// floor (it is about to go somewhere else).
pub fn take_from_hands(world: &mut World, user: Entity, item: Entity) -> bool {
    let Some(mut hands) = world.get_mut::<Hands>(user) else { return false };
    let Some(h) = hands.hands.iter_mut().find(|h| h.held == Some(item)) else { return false };
    h.held = None;
    world.entity_mut(item).remove::<InContainer>();
    true
}

pub fn swap_hands(world: &mut World, user: Entity) {
    if let Some(mut h) = world.get_mut::<Hands>(user) {
        if !h.hands.is_empty() {
            h.active = (h.active + 1) % h.hands.len();
        }
    }
}

/// Click on something with an empty hand (or the held item). The upstream
/// flow: `InteractUsing` if holding something, else pick up items, else
/// `InteractHand` handlers (doors, lockers, machines...).
pub fn interact(world: &mut World, user: Entity, target: Entity) -> bool {
    if user == target || world.get_entity(target).is_err() {
        return false;
    }
    if !in_range(world, user, target, INTERACTION_RANGE) {
        popup(world, user, "You can't reach that.".into());
        return false;
    }
    let held = world.get::<Hands>(user).and_then(Hands::active_item);
    if let Some(used) = held {
        let handlers = world.resource::<InteractionHandlers>().on_interact_using.clone();
        for h in handlers {
            if h(world, user, used, target) {
                return true;
            }
        }
    }
    if held.is_none() && world.get::<Item>(target).is_some() {
        return match try_pickup(world, user, target) {
            Ok(()) => true,
            Err(e) => {
                popup(world, user, capitalize(&e) + ".");
                false
            }
        };
    }
    let handlers = world.resource::<InteractionHandlers>().on_interact_hand.clone();
    for h in handlers {
        if h(world, user, target) {
            return true;
        }
    }
    false
}

/// Use the item in the active hand (Z by default in SS14).
pub fn use_in_hand(world: &mut World, user: Entity) -> bool {
    let Some(item) = world.get::<Hands>(user).and_then(Hands::active_item) else { return false };
    let handlers = world.resource::<InteractionHandlers>().on_use_in_hand.clone();
    for h in handlers {
        if h(world, user, item) {
            return true;
        }
    }
    false
}

/// Examine text (`ExamineSystem`): name, description and component details.
pub fn examine(world: &mut World, user: Entity, target: Entity) -> String {
    let (name, desc) = world
        .get::<MetaData>(target)
        .map(|m| (m.name.clone(), m.desc.clone()))
        .unwrap_or_default();
    let mut text = format!("This is {}.", with_article(&name));
    if !desc.is_empty() {
        text.push('\n');
        text.push_str(&desc);
    }
    if let Some(st) = world.get::<crate::damage::MobState>(target) {
        match st.0 {
            crate::damage::MobStateKind::Critical => text.push_str("\nThey are barely clinging to life."),
            crate::damage::MobStateKind::Dead => text.push_str("\nThey are dead."),
            _ => {}
        }
    }
    if let Some(d) = world.get::<crate::damage::Damageable>(target) {
        let t = d.total();
        if t > 0.0 && world.get::<crate::damage::MobState>(target).is_none() {
            text.push_str(&format!("\nIt looks damaged ({t:.0})."));
        }
    }
    world.write_message(ChatLine {
        speaker: Some(user),
        channel: ChatChannel::Examine,
        text: text.clone(),
    });
    text
}

fn with_article(name: &str) -> String {
    let first = name.chars().next().unwrap_or('x');
    if first.is_uppercase() {
        return name.to_string();
    }
    let art = if "aeiou".contains(first.to_ascii_lowercase()) { "an" } else { "a" };
    format!("{art} {name}")
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Unused here but kept for component parity: reads `Item.size` weights.
pub fn item_size_weight(size: &str) -> u32 {
    match size {
        "Tiny" => 1,
        "Small" => 2,
        "Normal" => 4,
        "Large" => 8,
        "Huge" => 16,
        "Ginormous" => 32,
        _ => 2,
    }
}

#[allow(dead_code)]
fn node_bool(n: Option<&Node>) -> Option<bool> {
    n.and_then(Node::as_bool)
}
