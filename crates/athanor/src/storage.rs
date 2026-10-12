//! Containers and storage. Ports of:
//! * `EntityStorage` (lockers, crates, closets): open/close, swallowing what
//!   stands on the tile, dumping contents when opened;
//! * `Storage` (backpacks, boxes, belts): insert/remove items;
//! * `StorageFill` (`EntitySpawnEntry` lists with `orGroup`s) and
//!   `EntityTableContainerFill` / `EntityTableSpawner` over `entityTable`
//!   selectors (All/Group/Nested/Ent selectors, rolls/prob/weight).

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use grimoire::Node;

use crate::content::{Content, ContentData};
use crate::events::{Popup, PopupKind, SoundAt};
use crate::interaction::{take_from_hands, Hands, InContainer, InteractionHandlers, Item};
use crate::rng::SimRng;
use crate::spawn::{spawn_proto, ComponentFactories};
use crate::transform::{tile_of, Xform};

// ---------------------------------------------------------------------------
// Entity tables
// ---------------------------------------------------------------------------

/// `NumberSelector`: `3`, `"1,4"`, or a tagged selector.
fn number(n: Option<&Node>, rng: &mut SimRng) -> u32 {
    let Some(n) = n else { return 1 };
    match n.type_tag() {
        Some("ConstantNumberSelector") => n.get("value").and_then(Node::as_i64).unwrap_or(1).max(0) as u32,
        Some("RangeNumberSelector") => {
            let (a, b) = n.get("range").and_then(Node::as_vec2).unwrap_or((1.0, 1.0));
            rng.range_u32(a as u32, b as u32)
        }
        Some("BinomialNumberSelector") => {
            let trials = n.get("trials").and_then(Node::as_i64).unwrap_or(1).max(0) as u32;
            let chance = n.get("chance").and_then(Node::as_f32).unwrap_or(0.5);
            (0..trials).filter(|_| rng.chance(chance)).count() as u32
        }
        _ => {
            if let Some(i) = n.as_i64() {
                return i.max(0) as u32;
            }
            match n.as_vec2() {
                Some((a, b)) => rng.range_u32(a as u32, b as u32),
                None => 1,
            }
        }
    }
}

/// Evaluate an `EntityTableSelector` node into prototype ids.
pub fn table_spawns(content: &ContentData, sel: &Node, rng: &mut SimRng, out: &mut Vec<String>, depth: u32) {
    if depth > 16 {
        return;
    }
    let rolls = number(sel.get("rolls"), rng);
    let prob = sel.get("prob").and_then(Node::as_f32).unwrap_or(1.0);
    for _ in 0..rolls {
        if prob < 1.0 && !rng.chance(prob) {
            continue;
        }
        match sel.type_tag() {
            Some("AllSelector") => {
                for c in sel.get("children").and_then(Node::as_seq).unwrap_or_default() {
                    table_spawns(content, c, rng, out, depth + 1);
                }
            }
            Some("GroupSelector") => {
                let children = sel.get("children").and_then(Node::as_seq).unwrap_or_default();
                let total: f32 = children.iter().map(|c| c.get("weight").and_then(Node::as_f32).unwrap_or(1.0)).sum();
                if total <= 0.0 {
                    continue;
                }
                let mut roll = rng.next_f32() * total;
                for c in children {
                    let w = c.get("weight").and_then(Node::as_f32).unwrap_or(1.0);
                    if roll < w {
                        table_spawns(content, c, rng, out, depth + 1);
                        break;
                    }
                    roll -= w;
                }
            }
            Some("NestedSelector") => {
                if let Some(t) = sel.get_str("tableId").and_then(|id| content.protos.get("entityTable", id))
                    && let Some(table) = t.get("table")
                {
                    table_spawns(content, table, rng, out, depth + 1);
                }
            }
            Some("NoneSelector") => {}
            _ => {
                // EntSelector (untagged, has `id`).
                if let Some(id) = sel.get_str("id") {
                    for _ in 0..number(sel.get("amount"), rng) {
                        out.push(id.to_string());
                    }
                }
            }
        }
    }
}

/// `EntitySpawnEntry.GetSpawns` (StorageFill `contents`).
pub fn spawn_entries(entries: &[Node], rng: &mut SimRng) -> Vec<String> {
    let mut out = Vec::new();
    let mut groups: Vec<(String, Vec<&Node>)> = Vec::new();
    let amount = |e: &Node, rng: &mut SimRng| {
        let a = e.get("amount").and_then(Node::as_i64).unwrap_or(1).max(0) as u32;
        let m = e.get("maxAmount").and_then(Node::as_i64).unwrap_or(1).max(0) as u32;
        // `random.Next(Amount, MaxAmount)`: max exclusive.
        if m > a { rng.next_range(a as i32, m as i32) as u32 } else { a }
    };
    for e in entries {
        if let Some(g) = e.get_str("orGroup") {
            match groups.iter_mut().find(|(n, _)| n == g) {
                Some((_, v)) => v.push(e),
                None => groups.push((g.to_string(), vec![e])),
            }
            continue;
        }
        let p = e.get("prob").and_then(Node::as_f32).unwrap_or(1.0);
        if p != 1.0 && !rng.chance(p) {
            continue;
        }
        let Some(id) = e.get_str("id") else { continue };
        for _ in 0..amount(e, rng) {
            out.push(id.to_string());
        }
    }
    for (_, members) in groups {
        let total: f32 = members.iter().map(|e| e.get("prob").and_then(Node::as_f32).unwrap_or(1.0)).sum();
        let roll = rng.next_f32() * total;
        let mut cum = 0.0;
        for e in members {
            cum += e.get("prob").and_then(Node::as_f32).unwrap_or(1.0);
            if roll > cum {
                continue;
            }
            if let Some(id) = e.get_str("id") {
                for _ in 0..amount(e, rng) {
                    out.push(id.to_string());
                }
            }
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

/// `EntityStorageComponent`.
#[derive(Component, Debug, Clone)]
pub struct EntityStorage {
    pub open: bool,
    pub capacity: u32,
    pub open_sound: Option<String>,
    pub close_sound: Option<String>,
}

/// `StorageComponent` (backpacks, boxes, belts).
#[derive(Component, Debug, Clone)]
pub struct Storage {
    pub max_item_size: String,
    pub capacity: u32,
}

#[derive(Component, Debug, Clone)]
pub enum PendingFill {
    Entries(Vec<Node>),
    Table(Node),
}

/// `EntityTableSpawner`: spawn from a table at map init.
#[derive(Component, Debug, Clone)]
pub struct TableSpawner {
    pub table: Node,
    pub offset: f32,
    pub delete_after: bool,
}

fn sound(n: &Node, key: &str) -> Option<String> {
    let s = n.get(key)?;
    s.get_str("path").or_else(|| s.get_str("collection")).map(str::to_owned)
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("EntityStorage", |e, node, _: &ContentData| {
        e.insert(EntityStorage {
            open: node.get("open").and_then(Node::as_bool).unwrap_or(false),
            capacity: node.get("capacity").and_then(Node::as_i64).unwrap_or(30) as u32,
            open_sound: sound(node, "openSound").or(Some("/Audio/Effects/closetopen.ogg".into())),
            close_sound: sound(node, "closeSound").or(Some("/Audio/Effects/closetclose.ogg".into())),
        });
    });
    reg.register("Storage", |e, node, _| {
        // Capacity from the grid rectangles (`grid: [ 0,0,6,3 ]` → 7×4 cells).
        let cells: u32 = node
            .get("grid")
            .map(Node::as_str_list)
            .unwrap_or_default()
            .iter()
            .filter_map(|r| {
                let v: Vec<i32> = r.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                (v.len() == 4).then(|| ((v[2] - v[0] + 1) * (v[3] - v[1] + 1)).max(0) as u32)
            })
            .sum();
        e.insert(Storage {
            max_item_size: node.get_str("maxItemSize").unwrap_or("Normal").to_string(),
            capacity: if cells > 0 { cells } else { 28 },
        });
    });
    reg.register("StorageFill", |e, node, _| {
        let contents = node.get("contents").and_then(Node::as_seq).map(<[Node]>::to_vec).unwrap_or_default();
        if !contents.is_empty() {
            e.insert(PendingFill::Entries(contents));
        }
    });
    reg.register("EntityTableContainerFill", |e, node, _| {
        // Fill the main container (`entity_storage` or `storage-default`).
        if let Some(m) = node.get("containers").and_then(Node::as_map)
            && let Some((_, sel)) = m.iter().find(|(k, _)| k.as_str() == "entity_storage" || k.as_str() == "storagebase")
                .or_else(|| m.iter().next())
        {
            e.insert(PendingFill::Table(sel.clone()));
        }
    });
    reg.register("EntityTableSpawner", |e, node, _| {
        if let Some(t) = node.get("table") {
            e.insert(TableSpawner {
                table: t.clone(),
                offset: node.get("offset").and_then(Node::as_f32).unwrap_or(0.2),
                delete_after: node.get("deleteSpawnerAfterSpawn").and_then(Node::as_bool).unwrap_or(true),
            });
        }
    });
}

pub fn install(world: &mut World) {
    let mut h = world.get_resource_or_init::<InteractionHandlers>();
    h.on_use_in_hand.push(|world, user, item| {
        if world.get::<Storage>(item).is_none() {
            return false;
        }
        world.write_message(crate::events::OpenStorage { user, storage: item });
        true
    });
    h.on_interact_hand.push(|world, user, target| {
        if world.get::<EntityStorage>(target).is_none() {
            return false;
        }
        toggle(world, user, target);
        true
    });
    h.on_interact_using.push(|world, user, used, target| {
        if world.get::<Storage>(target).is_some() {
            insert(world, user, used, target);
            return true;
        }
        // Put things into an open locker by dropping them on its tile.
        if world.get::<EntityStorage>(target).is_some_and(|s| s.open) {
            if take_from_hands(world, user, used) {
                let tx = *world.get::<Xform>(target).unwrap();
                if let Some(mut x) = world.get_mut::<Xform>(used) {
                    x.parent = tx.parent;
                    x.pos = tx.pos;
                }
            }
            return true;
        }
        false
    });
}

// ---------------------------------------------------------------------------
// Verbs
// ---------------------------------------------------------------------------

/// Everything directly inside `owner`.
pub fn contents(world: &mut World, owner: Entity) -> Vec<Entity> {
    let mut q = world.query::<(Entity, &InContainer)>();
    let mut v: Vec<Entity> = q.iter(world).filter(|(_, c)| c.owner == owner).map(|(e, _)| e).collect();
    v.sort();
    v
}

fn put_inside(world: &mut World, owner: Entity, e: Entity) {
    if let Some(mut x) = world.get_mut::<Xform>(e) {
        x.parent = Some(owner);
        x.pos = Vec2::ZERO;
    }
    if let Some(mut b) = world.get_mut::<crate::physics::Body>(e) {
        b.velocity = Vec2::ZERO;
    }
    world.entity_mut(e).insert(InContainer { owner });
}

pub fn toggle(world: &mut World, user: Entity, target: Entity) {
    let Some(st) = world.get::<EntityStorage>(target).cloned() else { return };
    let Some(tx) = world.get::<Xform>(target).copied() else { return };
    if st.open {
        // Close: swallow loose things on our tile (items, and bodies that
        // are not the player).
        let tile = tile_of(tx.pos);
        let swallow: Vec<Entity> = {
            let mut q = world.query::<(Entity, &Xform, Option<&Item>, Option<&crate::damage::MobState>)>();
            q.iter(world)
                .filter(|(e, x, item, mob)| {
                    *e != target
                        && x.parent == tx.parent
                        && !x.anchored
                        && tile_of(x.pos) == tile
                        && (item.is_some() || mob.is_some_and(|m| !m.is_alive()))
                })
                .map(|(e, ..)| e)
                .filter(|e| world.get::<crate::Player>(*e).is_none() && world.get::<InContainer>(*e).is_none())
                .take(st.capacity as usize)
                .collect()
        };
        for e in swallow {
            put_inside(world, target, e);
        }
        world.get_mut::<EntityStorage>(target).unwrap().open = false;
        if let Some(s) = st.close_sound {
            world.write_message(SoundAt { path: s, at: target });
        }
    } else {
        // Open: spill the contents onto the tile.
        let mut rng = world.get_resource_or_init::<SimRng>().clone();
        for e in contents(world, target) {
            world.entity_mut(e).remove::<InContainer>();
            if let Some(mut x) = world.get_mut::<Xform>(e) {
                x.parent = tx.parent;
                x.pos = tx.pos + Vec2::new(rng.range_f32(-0.15, 0.15), rng.range_f32(-0.15, 0.15));
            }
        }
        *world.resource_mut::<SimRng>() = rng;
        world.get_mut::<EntityStorage>(target).unwrap().open = true;
        if let Some(s) = st.open_sound {
            world.write_message(SoundAt { path: s, at: target });
        }
    }
    let _ = user;
}

fn size_rank(s: &str) -> u32 {
    crate::interaction::item_size_weight(s)
}

/// Put a held item into a `Storage`.
pub fn insert(world: &mut World, user: Entity, item: Entity, storage: Entity) -> bool {
    let Some(st) = world.get::<Storage>(storage).cloned() else { return false };
    // Only items go into storage (no crates in backpacks).
    let Some(size) = world.get::<Item>(item).map(|i| i.size.clone()) else { return false };
    let popup = |world: &mut World, text: String| {
        world.write_message(Popup {
            text,
            at: user,
            kind: PopupKind::Small,
        })
    };
    if size_rank(&size) > size_rank(&st.max_item_size) || item == storage {
        popup(world, "That won't fit.".into());
        return false;
    }
    let used: u32 = contents(world, storage)
        .iter()
        .map(|e| size_rank(&world.get::<Item>(*e).map(|i| i.size.clone()).unwrap_or_default()))
        .sum();
    if used + size_rank(&size) > st.capacity {
        popup(world, "It's full.".into());
        return false;
    }
    if world.get::<Hands>(user).is_some_and(|h| h.holding(item)) {
        take_from_hands(world, user, item);
    }
    put_inside(world, storage, item);
    true
}

/// Take an item out of a storage into the user's hand.
pub fn remove_to_hand(world: &mut World, user: Entity, item: Entity) -> bool {
    let Some(free) = world.get::<Hands>(user).and_then(Hands::free_hand) else { return false };
    world.entity_mut(item).remove::<InContainer>();
    world.get_mut::<Hands>(user).unwrap().hands[free].held = Some(item);
    world.entity_mut(item).insert(InContainer { owner: user });
    if let Some(mut x) = world.get_mut::<Xform>(item) {
        x.parent = Some(user);
        x.pos = Vec2::ZERO;
    }
    true
}

/// Resolve `StorageFill` / `EntityTableContainerFill` (map init).
pub fn resolve_fills(world: &mut World) {
    let pending: Vec<(Entity, PendingFill)> = {
        let mut q = world.query::<(Entity, &PendingFill)>();
        q.iter(world).map(|(e, p)| (e, p.clone())).collect()
    };
    if pending.is_empty() {
        return;
    }
    let content = world.resource::<Content>().clone();
    for (e, fill) in pending {
        world.entity_mut(e).remove::<PendingFill>();
        let mut rng = world.get_resource_or_init::<SimRng>().clone();
        let ids = match &fill {
            PendingFill::Entries(entries) => spawn_entries(entries, &mut rng),
            PendingFill::Table(sel) => {
                let mut out = Vec::new();
                table_spawns(&content, sel, &mut rng, &mut out, 0);
                out
            }
        };
        *world.resource_mut::<SimRng>() = rng;
        let Some(x) = world.get::<Xform>(e).copied() else { continue };
        let Some(parent) = x.parent else { continue };
        for id in ids {
            if let Some(c) = spawn_proto(world, &id, parent, x.pos) {
                put_inside(world, e, c);
            }
        }
    }
}

/// Run `EntityTableSpawner`s.
pub fn run_table_spawners(world: &mut World) -> usize {
    let spawners: Vec<(Entity, TableSpawner, Xform)> = {
        let mut q = world.query::<(Entity, &TableSpawner, &Xform)>();
        q.iter(world).map(|(e, s, x)| (e, s.clone(), *x)).collect()
    };
    let content = world.resource::<Content>().clone();
    let mut n = 0;
    for (e, s, x) in spawners {
        let mut rng = world.get_resource_or_init::<SimRng>().clone();
        let mut ids = Vec::new();
        table_spawns(&content, &s.table, &mut rng, &mut ids, 0);
        let offsets: Vec<Vec2> = ids
            .iter()
            .map(|_| Vec2::new(rng.range_f32(-s.offset, s.offset), rng.range_f32(-s.offset, s.offset)))
            .collect();
        *world.resource_mut::<SimRng>() = rng;
        if let Some(parent) = x.parent {
            for (id, off) in ids.iter().zip(offsets) {
                if spawn_proto(world, id, parent, x.pos + off).is_some() {
                    n += 1;
                }
            }
        }
        if s.delete_after {
            crate::spawn::delete(world, e);
        } else {
            world.entity_mut(e).remove::<TableSpawner>();
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_entries_or_groups() {
        let docs = grimoire::parse_yaml(
            r#"
- id: A
  amount: 2
- id: B
  orGroup: G
  prob: 0.5
- id: C
  orGroup: G
  prob: 0.5
- id: D
  prob: 0.0
"#,
        )
        .unwrap();
        let entries = docs[0].as_seq().unwrap().to_vec();
        let mut rng = SimRng(1);
        for _ in 0..20 {
            let s = spawn_entries(&entries, &mut rng);
            assert_eq!(s.iter().filter(|x| *x == "A").count(), 2);
            assert_eq!(s.iter().filter(|x| *x == "B" || *x == "C").count(), 1, "exactly one of the or-group");
            assert!(!s.contains(&"D".to_string()));
        }
    }
}
