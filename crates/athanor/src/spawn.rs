//! Spawning entities from prototypes — port of Robust's
//! `EntityManager.SpawnEntity` + `ComponentFactory`.
//!
//! Each ported component registers a *factory* under its upstream name
//! (`"MovementSpeedModifier"`, `"Door"`, ...). Spawning walks the entity's
//! resolved YAML components in order and runs the matching factory. Every
//! entity also keeps its full effective component data in [`EntityData`],
//! so systems that are not ported yet (and the client's sprite renderer)
//! can still read it.

use std::collections::HashMap;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use bevy_ecs::world::EntityWorldMut;
use bevy_math::Vec2;
use grimoire::Node;

use crate::content::{Content, ContentData};
use crate::transform::{tile_center, tile_of, Grid, WorldPose, Xform};

/// Name, description and prototype of an entity (`MetaDataComponent`).
#[derive(Component, Debug, Clone, Default)]
pub struct MetaData {
    pub proto: Option<Arc<str>>,
    pub name: String,
    pub desc: String,
}

impl MetaData {
    pub fn named(name: &str) -> Self {
        MetaData {
            proto: None,
            name: name.to_string(),
            desc: String::new(),
        }
    }
}

/// The entity's effective YAML components (prototype + map overrides).
#[derive(Component, Debug, Clone)]
pub struct EntityData {
    pub components: Arc<Vec<Node>>,
}

impl EntityData {
    pub fn get(&self, ty: &str) -> Option<&Node> {
        self.components.iter().find(|c| c.get_str("type") == Some(ty))
    }

    pub fn has(&self, ty: &str) -> bool {
        self.get(ty).is_some()
    }
}

pub type Factory = fn(&mut EntityWorldMut, &Node, &ContentData);

/// Registry of component factories, keyed by upstream component name.
#[derive(Resource, Default)]
pub struct ComponentFactories {
    map: HashMap<&'static str, Factory>,
}

impl ComponentFactories {
    pub fn register(&mut self, name: &'static str, f: Factory) {
        self.map.insert(name, f);
    }

    pub fn get(&self, name: &str) -> Option<Factory> {
        self.map.get(name).copied()
    }

    pub fn is_ported(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn names(&self) -> impl Iterator<Item = &&'static str> {
        self.map.keys()
    }
}

/// Deserialize a component straight from its node and insert it.
/// Errors are logged, not fatal: one bad field must not stop a map load.
#[macro_export]
macro_rules! data_component {
    ($reg:expr, $name:literal, $ty:ty) => {
        $reg.register($name, |e, node, _| match grimoire::from_node::<$ty>(node) {
            Ok(c) => {
                e.insert(c);
            }
            Err(err) => {
                tracing::warn!("component {} failed to deserialize: {}", $name, err);
                e.insert(<$ty>::default());
            }
        });
    };
}

/// Shared component lists per prototype, so 1000 identical walls share one.
#[derive(Resource, Default)]
pub struct ProtoCache {
    map: HashMap<String, Arc<Vec<Node>>>,
}

impl ProtoCache {
    pub fn components(&mut self, content: &ContentData, proto: &str) -> Option<Arc<Vec<Node>>> {
        if let Some(a) = self.map.get(proto) {
            return Some(a.clone());
        }
        let p = content.protos.entity(proto)?;
        let comps: Vec<Node> = p
            .get("components")
            .and_then(Node::as_seq)
            .map(<[Node]>::to_vec)
            .unwrap_or_default();
        let arc = Arc::new(comps);
        self.map.insert(proto.to_string(), arc.clone());
        Some(arc)
    }
}

/// Spawn a prototype at a position local to `parent`.
pub fn spawn_proto(world: &mut World, proto: &str, parent: Entity, pos: Vec2) -> Option<Entity> {
    let content = world.resource::<Content>().clone();
    let comps = world.get_resource_or_init::<ProtoCache>().components(&content, proto)?;
    Some(spawn_with(world, Some(proto), comps, Xform::at(parent, pos)))
}

/// Spawn an entity from an explicit component list.
pub fn spawn_with(world: &mut World, proto: Option<&str>, comps: Arc<Vec<Node>>, mut xform: Xform) -> Entity {
    let content = world.resource::<Content>().clone();

    // Transform data from YAML: anchored / noRot etc.
    if let Some(t) = comps.iter().find(|c| c.get_str("type") == Some("Transform")) {
        if let Some(a) = t.get("anchored").and_then(Node::as_bool) {
            xform.anchored = a;
        }
    }

    let meta = MetaData {
        proto: proto.map(Arc::from),
        name: proto.map(|p| content.entity_name(p)).unwrap_or_default(),
        desc: proto.map(|p| content.entity_description(p)).unwrap_or_default(),
    };

    let id = world
        .spawn((
            meta,
            xform,
            WorldPose::default(),
            EntityData {
                components: comps.clone(),
            },
        ))
        .id();

    world.resource_scope(|world, factories: Mut<ComponentFactories>| {
        let mut ent = world.entity_mut(id);
        for c in comps.iter() {
            let Some(ty) = c.get_str("type") else { continue };
            if ty == "MetaData" {
                if let Some(n) = c.get_str("name") {
                    ent.get_mut::<MetaData>().unwrap().name = n.to_string();
                }
                if let Some(d) = c.get_str("desc") {
                    ent.get_mut::<MetaData>().unwrap().desc = d.to_string();
                }
                continue;
            }
            if let Some(f) = factories.get(ty) {
                f(&mut ent, c, &content);
            }
        }
    });

    if xform.anchored {
        anchor(world, id);
    }
    world.trigger(Spawned { entity: id });
    id
}

/// `EntityManager.AddComponents(uid, registry)` (removeExisting = true):
/// layer YAML components over an existing entity and run their factories.
pub fn add_components(world: &mut World, e: Entity, comps: &[Node]) {
    if comps.is_empty() || world.get_entity(e).is_err() {
        return;
    }
    let content = world.resource::<Content>().clone();
    let mut merged: Vec<Node> = world.get::<EntityData>(e).map(|d| d.components.as_ref().clone()).unwrap_or_default();
    for c in comps {
        let ty = c.get_str("type");
        merged.retain(|m| m.get_str("type") != ty);
        merged.push(c.clone());
    }
    world.entity_mut(e).insert(EntityData { components: Arc::new(merged) });
    world.resource_scope(|world, factories: Mut<ComponentFactories>| {
        let mut ent = world.entity_mut(e);
        for c in comps {
            if let Some(f) = c.get_str("type").and_then(|ty| factories.get(ty)) {
                f(&mut ent, c, &content);
            }
        }
    });
}

/// Fired after an entity and all its components exist (≈ `MapInitEvent`).
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct Spawned {
    pub entity: Entity,
}

/// Anchor an entity to the tile under it on its parent grid, snapping it to
/// the tile centre. No-op if its parent is not a grid.
pub fn anchor(world: &mut World, e: Entity) -> bool {
    let Some(x) = world.get::<Xform>(e).copied() else { return false };
    let Some(parent) = x.parent else { return false };
    let tile = tile_of(x.pos);
    let Some(mut grid) = world.get_mut::<Grid>(parent) else { return false };
    grid.anchor(tile, e);
    let mut xm = world.get_mut::<Xform>(e).unwrap();
    xm.anchored = true;
    xm.pos = tile_center(tile);
    true
}

pub fn unanchor(world: &mut World, e: Entity) {
    let Some(x) = world.get::<Xform>(e).copied() else { return };
    if let Some(parent) = x.parent
        && let Some(mut grid) = world.get_mut::<Grid>(parent)
    {
        grid.unanchor(tile_of(x.pos), e);
    }
    if let Some(mut xm) = world.get_mut::<Xform>(e) {
        xm.anchored = false;
    }
}

/// Delete an entity and everything parented to it, unhooking grid anchors.
/// Children are found with one pass over transforms, so deleting a grid with
/// thousands of entities is linear.
pub fn delete(world: &mut World, e: Entity) {
    let mut children: HashMap<Entity, Vec<Entity>> = HashMap::new();
    {
        let mut q = world.query::<(Entity, &Xform)>();
        for (c, x) in q.iter(world) {
            if let Some(p) = x.parent {
                children.entry(p).or_default().push(c);
            }
        }
    }
    // Depth-first order, then delete leaves first.
    let mut order = Vec::new();
    let mut stack = vec![e];
    while let Some(n) = stack.pop() {
        order.push(n);
        if let Some(cs) = children.get(&n) {
            stack.extend(cs.iter().copied());
        }
    }
    for n in order.into_iter().rev() {
        if world.get::<Xform>(n).is_some_and(|x| x.anchored) {
            unanchor(world, n);
        }
        if world.get_entity(n).is_ok() {
            world.trigger(Deleting { entity: n });
            world.despawn(n);
        }
    }
}

/// Fired just before an entity is deleted.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct Deleting {
    pub entity: Entity,
}
