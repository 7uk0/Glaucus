//! Appraisal. Port of `PricingSystem.GetPrice`: materials, then stack price
//! *or* static price, then everything contained, plus Frontier's `MobPrice`.
//! Prices are read from the entity's YAML components, so every one of the
//! ~670 `StaticPrice`d prototypes is already priced correctly.

use athanor::interaction::InContainer;
use athanor::spawn::EntityData;
use athanor::Content;
use bevy_ecs::prelude::*;
use grimoire::Node;

use crate::Stack;

fn comp_f64(data: &EntityData, comp: &str, field: &str) -> Option<f64> {
    data.get(comp).and_then(|c| c.get(field)).and_then(Node::as_f64)
}

fn stack_count(world: &World, e: Entity, data: &EntityData) -> f64 {
    world
        .get::<Stack>(e)
        .map(|s| s.count as f64)
        .or_else(|| comp_f64(data, "Stack", "count"))
        .unwrap_or(1.0)
}

/// `GetMaterialPrice`: Σ composition × material price, times stack count.
fn materials_price(world: &World, content: &Content, e: Entity, data: &EntityData) -> f64 {
    if !data.has("Material") {
        return 0.0;
    }
    let Some(comp) = data.get("PhysicalComposition").and_then(|c| c.get("materialComposition")).and_then(Node::as_map) else {
        return 0.0;
    };
    let per_unit: f64 = comp
        .iter()
        .map(|(mat, amount)| {
            let price = content.protos.get("material", mat).and_then(|m| m.get("price")).and_then(Node::as_f64).unwrap_or(0.0);
            amount.as_f64().unwrap_or(0.0) * price
        })
        .sum();
    per_unit * stack_count(world, e, data)
}

/// Price of an entity and (optionally) everything inside it.
pub fn price_of(world: &mut World, e: Entity, include_contents: bool) -> f64 {
    let content = world.resource::<Content>().clone();
    price_inner(world, &content, e, include_contents, 0)
}

fn price_inner(world: &mut World, content: &Content, e: Entity, contents: bool, depth: u32) -> f64 {
    let Some(data) = world.get::<EntityData>(e).cloned() else { return 0.0 };
    let mut price = 0.0;

    // Frontier MobPrice: price, scaled down when dead.
    if let Some(mp) = data.get("MobPrice") {
        let base = mp.get("price").and_then(Node::as_f64).unwrap_or(0.0);
        let dead = world
            .get::<athanor::damage::MobState>(e)
            .is_some_and(|s| s.0 == athanor::damage::MobStateKind::Dead);
        let penalty = mp.get("deathPenalty").and_then(Node::as_f64).unwrap_or(0.2);
        price += if dead { base * penalty } else { base };
    }

    price += materials_price(world, content, e, &data);
    let before = price;
    if data.has("StackPrice") && !data.has("Material") {
        price += stack_count(world, e, &data) * comp_f64(&data, "StackPrice", "price").unwrap_or(0.0);
    }
    if before == price {
        price += comp_f64(&data, "StaticPrice", "price").unwrap_or(0.0);
    }

    if contents && depth < 8 {
        let inner: Vec<Entity> = {
            let mut q = world.query::<(Entity, &InContainer)>();
            q.iter(world).filter(|(_, c)| c.owner == e).map(|(x, _)| x).collect()
        };
        for c in inner {
            price += price_inner(world, content, c, contents, depth + 1);
        }
    }
    price
}

/// Frontier `MarketModifier.mod` on an entity (1.0 if absent).
pub fn market_modifier(world: &World, e: Entity) -> f64 {
    world
        .get::<EntityData>(e)
        .and_then(|d| comp_f64(d, "MarketModifier", "mod"))
        .unwrap_or(1.0)
}
