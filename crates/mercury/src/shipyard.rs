//! The shipyard. Port of Frontier's `ShipyardSystem` purchase path over
//! `vessel` prototypes: pay the price from your bank account, and the ship
//! is loaded from its map file into clear space beside the shipyard's
//! station, registered to you.

use athanor::events::{ChatChannel, ChatLine, Popup, PopupKind};
use athanor::map_loader::{load_map_path, LoadOptions};
use athanor::spawn::{EntityData, MetaData};
use athanor::transform::{world_pose_of, Grid, GridIndex, Xform};
use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use grimoire::{Node, PrototypeManager};

use crate::bank::{balance, try_withdraw};

#[derive(Debug, Clone)]
pub struct Vessel {
    pub id: String,
    pub name: String,
    pub description: String,
    pub price: i64,
    pub category: String,
    pub group: String,
    pub path: String,
    pub classes: Vec<String>,
}

/// Every purchasable `vessel` prototype, cheapest first.
#[derive(Resource, Debug, Clone, Default)]
pub struct Vessels(pub Vec<Vessel>);

impl Vessels {
    pub fn from_prototypes(pm: &PrototypeManager) -> Vessels {
        let mut v: Vec<Vessel> = pm
            .iter_kind("vessel")
            .filter(|p| !p.is_abstract)
            .filter_map(|p| {
                Some(Vessel {
                    id: p.id.clone(),
                    name: p.get_str("name").unwrap_or(&p.id).to_string(),
                    description: p.get_str("description").unwrap_or("").to_string(),
                    price: p.get("price").and_then(Node::as_i64)?,
                    category: p.get_str("category").unwrap_or("Small").to_string(),
                    group: p.get_str("group").unwrap_or("Shipyard").to_string(),
                    path: p.get_str("shuttlePath")?.to_string(),
                    classes: p.get("class").map(Node::as_str_list).unwrap_or_default(),
                })
            })
            .collect();
        v.sort_by(|a, b| a.price.cmp(&b.price).then(a.name.cmp(&b.name)));
        Vessels(v)
    }

    /// Vessels sold at a given shipyard group (`Shipyard`, `Security`, ...).
    pub fn group<'a>(&'a self, group: &'a str) -> impl Iterator<Item = &'a Vessel> + 'a {
        self.0.iter().filter(move |v| v.group == group)
    }

    pub fn get(&self, id: &str) -> Option<&Vessel> {
        self.0.iter().find(|v| v.id == id)
    }
}

/// Ships a character owns (`ShuttleDeedComponent`, simplified).
#[derive(Component, Debug, Clone, Default)]
pub struct Deeds(pub Vec<Entity>);

/// Which vessel group a console sells (`ShipyardConsoleUiKey`).
pub fn console_group(world: &World, console: Entity) -> Option<String> {
    let d = world.get::<EntityData>(console)?;
    d.get("ShipyardConsole")?;
    // The UI key encodes the group: enum.ShipyardConsoleUiKey.Shipyard etc.
    let key = d
        .get("ActivatableUI")
        .and_then(|a| a.get_str("key"))
        .and_then(|k| k.strip_prefix("enum.ShipyardConsoleUiKey."))
        .unwrap_or("Shipyard");
    Some(key.to_string())
}

pub fn on_interact_shipyard(world: &mut World, user: Entity, target: Entity) -> bool {
    if console_group(world, target).is_none() {
        return false;
    }
    world.write_message(crate::UiRequest::Shipyard { user, console: target });
    true
}

/// Clear space for a new ship: east of every grid on the map, level with
/// the shipyard.
fn free_spot(world: &World, map: Entity, near: Vec2) -> Vec2 {
    let mut max_x = near.x;
    let grids = world.get_resource::<GridIndex>().map(|g| g.grids.clone()).unwrap_or_default();
    for g in grids {
        let p = world_pose_of(world, g);
        if p.map != Some(map) {
            continue;
        }
        if let Some((_, _, x1, y1)) = world.get::<Grid>(g).and_then(|gr| gr.tiles.bounds()) {
            let r = Vec2::new(x1 as f32, y1 as f32).length();
            max_x = max_x.max(p.pos.x + r);
        }
    }
    Vec2::new(max_x + 12.0, near.y)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PurchaseError {
    UnknownVessel,
    NotAShipyard,
    NoAccount,
    InsufficientFunds { price: i64, balance: i64 },
    LoadFailed(String),
}

/// Buy a vessel at a shipyard console. Returns the new ship's grid.
pub fn buy_vessel(world: &mut World, buyer: Entity, console: Entity, vessel_id: &str) -> Result<Entity, PurchaseError> {
    let group = console_group(world, console).ok_or(PurchaseError::NotAShipyard)?;
    let vessel = world
        .resource::<Vessels>()
        .get(vessel_id)
        .filter(|v| v.group == group)
        .cloned()
        .ok_or(PurchaseError::UnknownVessel)?;
    let bal = balance(world, buyer).ok_or(PurchaseError::NoAccount)?;
    if bal < vessel.price {
        world.write_message(Popup {
            text: format!("Insufficient funds: the {} costs {} spesos.", vessel.name, vessel.price),
            at: buyer,
            kind: PopupKind::Small,
        });
        return Err(PurchaseError::InsufficientFunds {
            price: vessel.price,
            balance: bal,
        });
    }
    let cp = world_pose_of(world, console);
    let map = cp.map.ok_or(PurchaseError::NotAShipyard)?;
    let spot = free_spot(world, map, cp.pos);
    let loaded = load_map_path(
        world,
        &vessel.path,
        Some(map),
        LoadOptions {
            offset: spot,
            ..Default::default()
        },
    )
    .map_err(PurchaseError::LoadFailed)?;
    let ship = *loaded.grids.first().ok_or_else(|| PurchaseError::LoadFailed("no grid".into()))?;
    try_withdraw(world, buyer, vessel.price);
    // `ShipyardSystem.TryPurchaseShuttle` → `TryFTLDock` to the console's grid;
    // if no dock fits, the ship stays where it was loaded (proximity).
    let docked = cp.grid.is_some_and(|g| athanor::docking::try_ftl_dock(world, ship, g, None));
    // Frontier names ships "<model> <prefix>-<number>".
    let n = world.get_resource_or_init::<athanor::rng::SimRng>().range_u32(100, 999);
    let short = vessel.name.split_whitespace().last().unwrap_or(&vessel.name).to_string();
    if let Some(mut m) = world.get_mut::<MetaData>(ship) {
        m.name = format!("{short} 14-{n}");
    }
    match world.get_mut::<Deeds>(buyer) {
        Some(mut d) => d.0.push(ship),
        None => {
            world.entity_mut(buyer).insert(Deeds(vec![ship]));
        }
    }
    let ship_name = world.get::<MetaData>(ship).map(|m| m.name.clone()).unwrap_or_default();
    let left = balance(world, buyer).unwrap_or(0);
    world.write_message(ChatLine {
        speaker: None,
        channel: ChatChannel::Server,
        text: format!(
            "Purchased the {} for {} spesos. The {ship_name} is {}. Balance: {left}.",
            vessel.name,
            vessel.price,
            if docked { "docked at the station" } else { "waiting east of the station" }
        ),
    });
    let _ = world.get::<Xform>(ship);
    Ok(ship)
}
