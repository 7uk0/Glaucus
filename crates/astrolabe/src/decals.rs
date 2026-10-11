//! Decals: the floor markings, warning stripes and grime layered on tiles.
//!
//! Reads the `DecalGrid` component of a map grid (`chunkCollection` v2) and
//! `decal` prototypes. Port of Content's `DecalGridComponent` serializer.

use grimoire::{parse_color, Node, PrototypeManager};

#[derive(Debug, Clone, PartialEq)]
pub struct Decal {
    pub uid: u32,
    /// `decal` prototype id.
    pub id: String,
    /// Bottom-left corner in grid-local tile coordinates.
    pub pos: (f32, f32),
    pub color: [u8; 4],
    pub angle: f64,
    pub z_index: i32,
    pub cleanable: bool,
}

#[derive(Debug, Clone)]
pub struct DecalDef {
    pub id: String,
    /// RSI path relative to `/Textures` and state.
    pub sprite: Option<(String, String)>,
    /// Single-texture decals use `sprite: { texture: ... }` instead.
    pub texture: Option<String>,
    pub z_index: i32,
}

pub fn decal_defs(pm: &PrototypeManager) -> Vec<DecalDef> {
    pm.iter_kind("decal")
        .map(|p| {
            let sprite = p.get("sprite");
            let rsi = sprite.and_then(|s| Some((s.get_str("sprite")?.to_string(), s.get_str("state")?.to_string())));
            let texture = sprite.and_then(|s| s.get_str("texture")).map(str::to_owned);
            DecalDef {
                id: p.id.clone(),
                sprite: rsi,
                texture,
                z_index: p.get("zIndex").and_then(Node::as_i64).unwrap_or(0) as i32,
            }
        })
        .collect()
}

/// Parse a grid's `DecalGrid` component.
pub fn read_decal_grid(decal_grid: &Node) -> Vec<Decal> {
    let mut out = Vec::new();
    let Some(coll) = decal_grid.get("chunkCollection") else {
        return out;
    };
    for entry in coll.get("nodes").and_then(Node::as_seq).unwrap_or_default() {
        let Some(node) = entry.get("node") else { continue };
        let Some(id) = node.get_str("id") else { continue };
        let color = node.get_str("color").and_then(parse_color).unwrap_or([255; 4]);
        let angle = node.get("angle").and_then(Node::as_angle).unwrap_or(0.0);
        let z_index = node.get("zIndex").and_then(Node::as_i64).unwrap_or(0) as i32;
        let cleanable = node.get("cleanable").and_then(Node::as_bool).unwrap_or(false);
        let Some(decals) = entry.get("decals").and_then(Node::as_map) else { continue };
        for (uid, pos) in decals {
            let (Ok(uid), Some(pos)) = (uid.parse::<u32>(), pos.as_vec2()) else { continue };
            out.push(Decal {
                uid,
                id: id.to_string(),
                pos,
                color,
                angle,
                z_index,
                cleanable,
            });
        }
    }
    out.sort_by_key(|d| (d.z_index, d.uid));
    out
}
