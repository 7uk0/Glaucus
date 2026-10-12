//! `EntityWhitelist` and `EntityWhitelistSystem.IsValid`
//! (`Content.Shared/Whitelist/`), plus tag lookups (`TagSystem`).
//!
//! A whitelist matches an entity by component names, item sizes and tags.
//! The order matters, as upstream:
//! 1. Components: without `requireAll`, the first present one passes;
//!    with it, the first missing one fails.
//! 2. Sizes: an item of a listed size passes.
//! 3. Tags: any (or, with `requireAll`, all) of them.
//! 4. Otherwise the result is `requireAll`.
//!
//! Components are checked against the entity's YAML (`EntityData`), which
//! holds every component the prototype and map gave it, ported or not.

use bevy_ecs::prelude::*;
use grimoire::Node;

use crate::spawn::EntityData;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Whitelist {
    pub components: Vec<String>,
    pub sizes: Vec<String>,
    pub tags: Vec<String>,
    pub require_all: bool,
}

impl Whitelist {
    pub fn from_node(n: &Node) -> Whitelist {
        let list = |k: &str| n.get(k).map(Node::as_str_list).unwrap_or_default();
        Whitelist {
            components: list("components"),
            sizes: list("sizes"),
            tags: list("tags"),
            require_all: n.get("requireAll").and_then(Node::as_bool).unwrap_or(false),
        }
    }

    /// `IsValid`.
    pub fn is_valid(&self, world: &World, e: Entity) -> bool {
        let data = world.get::<EntityData>(e);
        let has = |c: &str| data.is_some_and(|d| d.has(c));
        for c in &self.components {
            if has(c) {
                if !self.require_all {
                    return true;
                }
            } else if self.require_all {
                return false;
            }
        }
        if !self.sizes.is_empty()
            && let Some(item) = data.and_then(|d| d.get("Item"))
        {
            let size = item.get_str("size").unwrap_or("Small");
            if self.sizes.iter().any(|s| s == size) {
                return true;
            }
        }
        if !self.tags.is_empty() {
            let tags = tags_of(world, e);
            return if self.require_all {
                self.tags.iter().all(|t| tags.contains(t))
            } else {
                self.tags.iter().any(|t| tags.contains(t))
            };
        }
        self.require_all
    }
}

/// `IsWhitelistPass`: false without a whitelist.
pub fn pass(list: Option<&Whitelist>, world: &World, e: Entity) -> bool {
    list.is_some_and(|l| l.is_valid(world, e))
}

/// `IsWhitelistFail`: false without a whitelist.
pub fn fail(list: Option<&Whitelist>, world: &World, e: Entity) -> bool {
    list.is_some_and(|l| !l.is_valid(world, e))
}

/// `IsWhitelistFailOrNull`: true without a whitelist.
pub fn fail_or_null(list: Option<&Whitelist>, world: &World, e: Entity) -> bool {
    list.is_none_or(|l| !l.is_valid(world, e))
}

/// The entity's `TagComponent.Tags`.
pub fn tags_of(world: &World, e: Entity) -> Vec<String> {
    world
        .get::<EntityData>(e)
        .and_then(|d| d.get("Tag"))
        .and_then(|t| t.get("tags"))
        .map(Node::as_str_list)
        .unwrap_or_default()
}

/// `TagSystem.HasTag`.
pub fn has_tag(world: &World, e: Entity, tag: &str) -> bool {
    tags_of(world, e).iter().any(|t| t == tag)
}
