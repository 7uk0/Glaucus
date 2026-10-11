//! Random names from `localizedDataset`s — upstream's `RandomMetadata`
//! component (`nameSegments`) and our `CaputRandomName` (first + last).

use bevy_ecs::prelude::*;
use grimoire::Node;

use crate::content::{Content, ContentData};
use crate::rng::SimRng;
use crate::spawn::{ComponentFactories, MetaData};

#[derive(Component, Debug, Clone)]
pub struct PendingName {
    pub segments: Vec<String>,
    pub separator: String,
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("RandomMetadata", |e, node, _: &ContentData| {
        let segments = node.get("nameSegments").map(Node::as_str_list).unwrap_or_default();
        if !segments.is_empty() {
            e.insert(PendingName {
                segments,
                separator: node.get_str("nameSeparator").unwrap_or(" ").to_string(),
            });
        }
    });
    reg.register("CaputRandomName", |e, _, _| {
        e.insert(PendingName {
            segments: vec!["NamesFirst".into(), "NamesLast".into()],
            separator: " ".into(),
        });
    });
}

/// Pick one entry from a `localizedDataset` (or plain `dataset`).
pub fn pick_from_dataset(content: &ContentData, id: &str, rng: &mut SimRng) -> Option<String> {
    if let Some(p) = content.protos.get("localizedDataset", id) {
        let v = p.get("values")?;
        let prefix = v.get_str("prefix")?;
        let count = v.get("count").and_then(Node::as_i64)?.max(1) as u32;
        let i = rng.range_u32(1, count);
        return content.locale.try_get(&format!("{prefix}{i}"), &[]);
    }
    let p = content.protos.get("dataset", id)?;
    let values = p.get("values").map(Node::as_str_list).unwrap_or_default();
    rng.pick(&values).map(|s| content.localize(s))
}

pub fn resolve_names(world: &mut World) {
    let pending: Vec<(Entity, PendingName)> = {
        let mut q = world.query::<(Entity, &PendingName)>();
        q.iter(world).map(|(e, p)| (e, p.clone())).collect()
    };
    if pending.is_empty() {
        return;
    }
    let content = world.resource::<Content>().clone();
    for (e, p) in pending {
        let mut rng = world.get_resource_or_init::<SimRng>().clone();
        let parts: Vec<String> = p.segments.iter().filter_map(|s| pick_from_dataset(&content, s, &mut rng)).collect();
        *world.resource_mut::<SimRng>() = rng;
        if !parts.is_empty()
            && let Some(mut m) = world.get_mut::<MetaData>(e)
        {
            m.name = parts.join(&p.separator);
        }
        world.entity_mut(e).remove::<PendingName>();
    }
}
