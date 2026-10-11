//! Inventories, clothing and starting gear. Ports of `InventoryComponent`
//! (slots from `inventoryTemplate` prototypes), `ClothingComponent` data,
//! `LoadoutComponent` and `StartingGearPrototype` equipping.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use grimoire::Node;

use crate::content::{Content, ContentData};
use crate::interaction::{Hands, InContainer};
use crate::rng::SimRng;
use crate::spawn::{spawn_proto, ComponentFactories};
use crate::transform::Xform;

#[derive(Debug, Clone)]
pub struct Slot {
    pub name: String,
    pub flags: String,
    pub item: Option<Entity>,
}

#[derive(Component, Debug, Clone, Default)]
pub struct Inventory {
    pub template: String,
    pub species: Option<String>,
    pub slots: Vec<Slot>,
}

impl Inventory {
    pub fn get(&self, slot: &str) -> Option<Entity> {
        self.slots.iter().find(|s| s.name == slot).and_then(|s| s.item)
    }

    pub fn slot_flags(&self, slot: &str) -> Option<&str> {
        self.slots.iter().find(|s| s.name == slot).map(|s| s.flags.as_str())
    }
}

/// One clothing visual layer (`clothingVisuals: { slot: [ {state, color} ] }`).
#[derive(Debug, Clone)]
pub struct ClothingLayer {
    pub rsi: Option<String>,
    pub state: String,
    pub color: Option<String>,
}

#[derive(Component, Debug, Clone, Default)]
pub struct Clothing {
    pub sprite: Option<String>,
    pub equipped_prefix: Option<String>,
    pub equipped_state: Option<String>,
    pub visuals: Vec<(String, Vec<ClothingLayer>)>,
}

impl Clothing {
    pub fn visuals_for(&self, slot: &str) -> Option<&[ClothingLayer]> {
        self.visuals.iter().find(|(s, _)| s == slot).map(|(_, v)| v.as_slice())
    }
}

/// `TemporarySlotMap` from the client clothing system: slot → state suffix.
pub fn slot_state_suffix(slot: &str) -> Option<&'static str> {
    Some(match slot {
        "head" => "HELMET",
        "eyes" => "EYES",
        "ears" => "EARS",
        "mask" => "MASK",
        "outerClothing" => "OUTERCLOTHING",
        "jumpsuit" => "INNERCLOTHING",
        "neck" => "NECK",
        "back" => "BACKPACK",
        "belt" => "BELT",
        "gloves" => "HAND",
        "shoes" => "FEET",
        "id" => "IDCARD",
        "pocket1" => "POCKET1",
        "pocket2" => "POCKET2",
        "suitstorage" => "SUITSTORAGE",
        _ => return None,
    })
}

#[derive(Component, Debug, Clone)]
pub struct PendingLoadout(pub Vec<String>);

fn template_slots(content: &ContentData, template: &str) -> Vec<Slot> {
    content
        .protos
        .get("inventoryTemplate", template)
        .and_then(|p| p.get("slots"))
        .and_then(Node::as_seq)
        .unwrap_or_default()
        .iter()
        .filter_map(|s| {
            Some(Slot {
                name: s.get_str("name")?.to_string(),
                flags: s.get_str("slotFlags").unwrap_or("").trim().to_string(),
                item: None,
            })
        })
        .collect()
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("Inventory", |e, node, content| {
        let template = node.get_str("templateId").unwrap_or("human").to_string();
        e.insert(Inventory {
            slots: template_slots(content, &template),
            template,
            species: node.get_str("speciesId").map(str::to_owned),
        });
    });
    reg.register("Clothing", |e, node, _| {
        let mut visuals = Vec::new();
        if let Some(m) = node.get("clothingVisuals").and_then(Node::as_map) {
            for (slot, layers) in m {
                let ls = layers
                    .as_seq()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|l| {
                        Some(ClothingLayer {
                            rsi: l.get_str("sprite").map(str::to_owned),
                            state: l.get_str("state")?.to_string(),
                            color: l.get_str("color").map(str::to_owned),
                        })
                    })
                    .collect();
                visuals.push((slot.clone(), ls));
            }
        }
        e.insert(Clothing {
            sprite: node.get_str("sprite").map(str::to_owned),
            equipped_prefix: node.get_str("equippedPrefix").map(str::to_owned),
            equipped_state: node.get_str("equippedState").map(str::to_owned),
            visuals,
        });
    });
    reg.register("Loadout", |e, node, _| {
        let protos = node.get("prototypes").map(Node::as_str_list).unwrap_or_default();
        if !protos.is_empty() {
            e.insert(PendingLoadout(protos));
        }
    });
}

/// Put `item` into `slot` on `wearer`. Returns false if the slot is taken
/// or missing.
pub fn equip(world: &mut World, wearer: Entity, slot: &str, item: Entity) -> bool {
    let Some(mut inv) = world.get_mut::<Inventory>(wearer) else { return false };
    let Some(s) = inv.slots.iter_mut().find(|s| s.name == slot) else { return false };
    if s.item.is_some() {
        return false;
    }
    s.item = Some(item);
    if let Some(mut x) = world.get_mut::<Xform>(item) {
        x.parent = Some(wearer);
        x.pos = Vec2::ZERO;
        x.anchored = false;
    }
    world.entity_mut(item).insert(InContainer { owner: wearer });
    true
}

/// Spawn and equip a `startingGear` prototype (`equipment` + `inhand`).
pub fn equip_starting_gear(world: &mut World, wearer: Entity, gear: &str) {
    let content = world.resource::<Content>().clone();
    let Some(p) = content.protos.get("startingGear", gear) else {
        tracing::warn!("unknown startingGear {gear}");
        return;
    };
    let Some(parent) = world.get::<Xform>(wearer).and_then(|x| x.parent) else { return };
    let pos = world.get::<Xform>(wearer).map(|x| x.pos).unwrap_or_default();
    if let Some(eq) = p.get("equipment").and_then(Node::as_map) {
        for (slot, item) in eq {
            let Some(id) = item.as_str() else { continue };
            if let Some(ent) = spawn_proto(world, id, parent, pos)
                && !equip(world, wearer, slot, ent)
            {
                crate::spawn::delete(world, ent);
            }
        }
    }
    for id in p.get("inhand").map(Node::as_str_list).unwrap_or_default() {
        if let Some(ent) = spawn_proto(world, &id, parent, pos) {
            let free = world.get::<Hands>(wearer).and_then(Hands::free_hand);
            match free {
                Some(h) => {
                    world.get_mut::<Hands>(wearer).unwrap().hands[h].held = Some(ent);
                    if let Some(mut x) = world.get_mut::<Xform>(ent) {
                        x.parent = Some(wearer);
                        x.pos = Vec2::ZERO;
                    }
                    world.entity_mut(ent).insert(InContainer { owner: wearer });
                }
                None => crate::spawn::delete(world, ent),
            }
        }
    }
}

/// Resolve `Loadout` components: pick one gear set and equip it.
pub fn resolve_loadouts(world: &mut World) {
    let pending: Vec<(Entity, Vec<String>)> = {
        let mut q = world.query::<(Entity, &PendingLoadout)>();
        q.iter(world).map(|(e, p)| (e, p.0.clone())).collect()
    };
    for (e, list) in pending {
        world.entity_mut(e).remove::<PendingLoadout>();
        let pick = world.get_resource_or_init::<SimRng>().pick(&list).cloned();
        if let Some(g) = pick {
            equip_starting_gear(world, e, &g);
        }
    }
}
