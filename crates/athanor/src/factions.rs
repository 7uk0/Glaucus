//! NPC factions (`NpcFactionSystem`): who is hostile to whom, from
//! `npcFaction` prototypes and `NpcFactionMember` components.

use std::collections::{HashMap, HashSet};

use bevy_ecs::prelude::*;
use grimoire::{Node, PrototypeManager};

use crate::spawn::ComponentFactories;

#[derive(Debug, Default, Clone)]
pub struct FactionTable {
    pub hostile: HashMap<String, HashSet<String>>,
    pub friendly: HashMap<String, HashSet<String>>,
}

impl FactionTable {
    pub fn from_prototypes(pm: &PrototypeManager) -> Self {
        let mut t = FactionTable::default();
        for p in pm.iter_kind("npcFaction") {
            let h = p.get("hostile").map(Node::as_str_list).unwrap_or_default();
            let f = p.get("friendly").map(Node::as_str_list).unwrap_or_default();
            t.hostile.insert(p.id.clone(), h.into_iter().collect());
            t.friendly.insert(p.id.clone(), f.into_iter().collect());
        }
        t
    }

    pub fn is_hostile(&self, a: &FactionMember, b: &FactionMember) -> bool {
        a.factions.iter().any(|fa| {
            self.hostile
                .get(fa)
                .is_some_and(|hs| b.factions.iter().any(|fb| hs.contains(fb)))
        }) || a.hostile_to.iter().any(|f| b.factions.contains(f))
    }
}

/// `NpcFactionMemberComponent`.
#[derive(Component, Debug, Clone, Default)]
pub struct FactionMember {
    pub factions: HashSet<String>,
    /// Extra per-entity hostilities (`hostileFactions` / exceptions).
    pub hostile_to: HashSet<String>,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct Factions(pub FactionTable);

pub fn register(reg: &mut ComponentFactories) {
    reg.register("NpcFactionMember", |e, node, _| {
        e.insert(FactionMember {
            factions: node.get("factions").map(Node::as_str_list).unwrap_or_default().into_iter().collect(),
            hostile_to: node.get("hostileFactions").map(Node::as_str_list).unwrap_or_default().into_iter().collect(),
        });
    });
}

pub fn hostile(world: &World, a: Entity, b: Entity) -> bool {
    let (Some(fa), Some(fb)) = (world.get::<FactionMember>(a), world.get::<FactionMember>(b)) else {
        return false;
    };
    world.get_resource::<Factions>().is_some_and(|t| t.0.is_hostile(fa, fb))
}
