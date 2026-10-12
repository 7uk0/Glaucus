//! `EntitySpawnCollectionCache` (`Content.Server/Worldgen/Tools`): entries are
//! grouped by `orGroup` (no group = the "" group). A group with one entry
//! always spawns it; otherwise one entry is picked by weight (`prob`).
//! Unlike `StorageFill`'s `EntitySpawnCollection`, ungrouped entries are not
//! rolled independently — they compete in the "" group.

use athanor::rng::SimRng;
use grimoire::Node;

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: Option<String>,
    pub prob: f32,
    pub amount: u32,
    pub max_amount: u32,
}

#[derive(Debug, Clone, Default)]
pub struct SpawnCache {
    groups: Vec<(String, Vec<Entry>, f32)>,
}

impl SpawnCache {
    pub fn from_entries(entries: &[Node]) -> SpawnCache {
        let mut c = SpawnCache::default();
        for e in entries {
            let entry = Entry {
                id: e.get_str("id").map(str::to_string),
                prob: e.get("prob").and_then(Node::as_f32).unwrap_or(1.0),
                amount: e.get("amount").and_then(Node::as_i64).unwrap_or(1).max(0) as u32,
                max_amount: e.get("maxAmount").and_then(Node::as_i64).unwrap_or(1).max(0) as u32,
            };
            let g = e.get_str("orGroup").unwrap_or("").to_string();
            match c.groups.iter_mut().find(|(n, ..)| *n == g) {
                Some((_, v, total)) => {
                    *total += entry.prob;
                    v.push(entry);
                }
                None => {
                    let p = entry.prob;
                    c.groups.push((g, vec![entry], p));
                }
            }
        }
        c
    }

    pub fn from_node(n: Option<&Node>) -> SpawnCache {
        SpawnCache::from_entries(n.and_then(Node::as_seq).unwrap_or_default())
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// `GetSpawns`: prototype ids (an entry without an id spawns nothing).
    pub fn spawns(&self, rng: &mut SimRng) -> Vec<String> {
        let mut out = Vec::new();
        let mut push = |e: &Entry, rng: &mut SimRng, out: &mut Vec<String>| {
            let mut amount = e.amount;
            if e.max_amount > amount {
                amount = rng.range_u32(amount, e.max_amount);
            }
            if let Some(id) = &e.id {
                for _ in 0..amount {
                    out.push(id.clone());
                }
            }
        };
        for (_, entries, total) in &self.groups {
            if entries.len() == 1 {
                push(&entries[0], rng, &mut out);
                continue;
            }
            let roll = rng.next_f32() as f64 * *total as f64;
            let mut cum = 0.0f64;
            for e in entries {
                cum += e.prob as f64;
                if roll > cum {
                    continue;
                }
                push(e, rng, &mut out);
                break;
            }
        }
        out
    }
}
