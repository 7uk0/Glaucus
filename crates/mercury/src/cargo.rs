//! Selling cargo. Port of Frontier's `NFCargoSystem` pallet console: every
//! sellable thing standing on a `sell` `CargoPallet` within `palletDistance`
//! tiles of the console is appraised, scaled by the console's (and the
//! item's) `MarketModifier`, removed, and paid out as a stack of cash.

use athanor::events::{Popup, PopupKind, SoundAt};
use athanor::interaction::InContainer;
use athanor::spawn::{delete, EntityData};
use athanor::transform::{tile_center, tile_of, Xform};
use bevy_ecs::prelude::*;
use grimoire::Node;

use crate::pricing::{market_modifier, price_of};

fn console_info(world: &World, e: Entity) -> Option<(i32, String)> {
    let d = world.get::<EntityData>(e)?;
    if let Some(c) = d.get("NFCargoPalletConsole") {
        return Some((
            c.get("palletDistance").and_then(Node::as_i64).unwrap_or(8) as i32,
            c.get_str("cashType").unwrap_or("Credit").to_string(),
        ));
    }
    d.get("CargoPalletConsole").map(|c| (8, c.get_str("cashType").unwrap_or("Credit").to_string()))
}

fn is_sell_pallet(world: &World, e: Entity) -> bool {
    world
        .get::<EntityData>(e)
        .and_then(|d| d.get("CargoPallet"))
        .is_some_and(|p| p.get_str("palletType").unwrap_or("all") != "buy")
}

#[derive(Debug, Clone, Default)]
pub struct SaleReport {
    pub items: usize,
    pub value: i64,
}

/// Sell everything on the console's pallets. Returns what was sold.
pub fn sell(world: &mut World, console: Entity) -> Option<SaleReport> {
    let (distance, cash) = console_info(world, console)?;
    let cx = *world.get::<Xform>(console)?;
    let grid = cx.parent?;
    let ct = tile_of(cx.pos);

    let pallets: Vec<bevy_math::IVec2> = {
        let mut q = world.query::<(Entity, &Xform)>();
        q.iter(world)
            .filter(|(e, x)| x.parent == Some(grid) && is_sell_pallet(world, *e))
            .map(|(_, x)| tile_of(x.pos))
            .filter(|t| (*t - ct).abs().max_element() <= distance)
            .collect()
    };
    if pallets.is_empty() {
        return Some(SaleReport::default());
    }
    let goods: Vec<Entity> = {
        let mut q = world.query::<(Entity, &Xform, &EntityData, Option<&InContainer>)>();
        q.iter(world)
            .filter(|(_, x, d, c)| {
                c.is_none()
                    && x.parent == Some(grid)
                    && !x.anchored
                    && pallets.contains(&tile_of(x.pos))
                    && !d.has("CargoPallet")
                    && d.has("Item") | d.has("MobPrice") | d.has("StaticPrice")
            })
            .filter(|(e, ..)| world.get::<athanor::Player>(*e).is_none())
            .filter(|(e, ..)| {
                world
                    .get::<athanor::damage::MobState>(*e)
                    .is_none_or(|s| s.0 == athanor::damage::MobStateKind::Dead)
            })
            .map(|(e, ..)| e)
            .collect()
    };
    let console_mod = market_modifier(world, console);
    let mut total = 0.0;
    for g in &goods {
        total += price_of(world, *g, true) * market_modifier(world, *g) * console_mod;
    }
    for g in &goods {
        delete(world, *g);
    }
    let value = total.floor() as i64;
    if value > 0 {
        crate::spawn_stack(world, &cash, value as u32, grid, tile_center(ct));
        world.write_message(SoundAt {
            path: "/Audio/Effects/Cargo/ping.ogg".into(),
            at: console,
        });
    }
    Some(SaleReport {
        items: goods.len(),
        value,
    })
}

pub fn on_interact_pallet_console(world: &mut World, user: Entity, target: Entity) -> bool {
    if console_info(world, target).is_none() {
        return false;
    }
    let text = match sell(world, target) {
        Some(r) if r.items > 0 => format!("Sold {} items for {} spesos. Collect the cash at the console.", r.items, r.value),
        _ => "Nothing on the sell pallets.".into(),
    };
    world.write_message(Popup {
        text,
        at: user,
        kind: PopupKind::Small,
    });
    true
}
