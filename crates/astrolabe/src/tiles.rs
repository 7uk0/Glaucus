//! Tile definitions and chunked tile storage.
//!
//! Port of `ContentTileDefinition` + Robust's `ITileDefinitionManager`,
//! `Tile` and `MapChunk`. Tile type `0` is always `Space`, like Robust.

use std::collections::HashMap;

use grimoire::{Node, PrototypeManager};
use serde::Deserialize;
use vellum::ResPath;

pub const CHUNK_SIZE: i32 = 16;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct TileDef {
    #[serde(skip)]
    pub id: String,
    #[serde(skip)]
    pub type_id: u16,
    /// Locale key, e.g. `tiles-steel-floor`.
    pub name: String,
    pub sprite: Option<String>,
    pub variants: Option<u8>,
    /// Weights for `TileSystem.PickVariant` (default `[1]`).
    pub placement_variants: Option<Vec<f32>>,
    pub base_turf: Option<String>,
    pub is_subfloor: bool,
    pub is_space: bool,
    pub friction: Option<f32>,
    pub mob_friction: Option<f32>,
    pub mob_acceleration: Option<f32>,
    pub sturdy: Option<bool>,
    pub indestructible: bool,
    pub item_drop: Option<String>,
    pub heat_capacity: Option<f32>,
    pub thermal_conductivity: Option<f32>,
    pub edge_sprite_priority: i32,
    pub allow_rotation_mirror: bool,
}

impl TileDef {
    pub fn sprite_path(&self) -> Option<ResPath> {
        self.sprite.as_deref().map(ResPath::new)
    }

    /// `TileSystem.PickVariant`: weighted by `placementVariants`, given a
    /// uniform `[0, 1)` roll.
    pub fn pick_variant(&self, roll: f32) -> u8 {
        let weights: &[f32] = self.placement_variants.as_deref().unwrap_or(&[1.0]);
        let sum: f32 = weights.iter().sum();
        let rand = roll * sum;
        let mut acc = 0.0;
        for (i, w) in weights.iter().enumerate() {
            acc += w;
            if acc >= rand {
                return i as u8;
            }
        }
        0
    }

    pub fn variant_count(&self) -> u8 {
        self.variants.unwrap_or(1).max(1)
    }

    /// Default tile friction is 0.3 in Robust/Content (`TileFriction`).
    pub fn friction(&self) -> f32 {
        self.friction.unwrap_or(if self.is_space { 0.0 } else { 0.3 })
    }
}

#[derive(Debug, Clone, Default)]
pub struct TileRegistry {
    defs: Vec<TileDef>,
    by_name: HashMap<String, u16>,
    aliases: HashMap<String, String>,
}

impl TileRegistry {
    pub fn from_prototypes(pm: &PrototypeManager) -> Self {
        let mut reg = TileRegistry::default();
        // Space first so it is id 0.
        let mut protos: Vec<_> = pm.iter_kind("tile").filter(|p| !p.is_abstract).collect();
        protos.sort_by_key(|p| (p.id != "Space", p.id.clone()));
        for p in protos {
            let mut def: TileDef = p.read().unwrap_or_default();
            def.id = p.id.clone();
            reg.push(def);
        }
        if reg.by_name.get("Space") != Some(&0) {
            // Content without a Space tile still needs one at id 0.
            let mut space = TileDef {
                id: "Space".into(),
                is_space: true,
                is_subfloor: true,
                ..Default::default()
            };
            space.name = "tiles-space".into();
            reg.defs.insert(0, space);
            reg.reindex();
        }
        for a in pm.iter_kind("tileAlias") {
            if let Some(t) = a.get_str("target") {
                reg.aliases.insert(a.id.clone(), t.to_string());
            }
        }
        reg
    }

    fn push(&mut self, mut def: TileDef) {
        def.type_id = self.defs.len() as u16;
        self.by_name.insert(def.id.clone(), def.type_id);
        self.defs.push(def);
    }

    fn reindex(&mut self) {
        self.by_name.clear();
        for (i, d) in self.defs.iter_mut().enumerate() {
            d.type_id = i as u16;
            self.by_name.insert(d.id.clone(), i as u16);
        }
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    pub fn get(&self, type_id: u16) -> Option<&TileDef> {
        self.defs.get(type_id as usize)
    }

    /// Resolve a tile name, following `tileAlias` prototypes.
    pub fn id_of(&self, name: &str) -> Option<u16> {
        if let Some(&id) = self.by_name.get(name) {
            return Some(id);
        }
        let mut cur = name;
        for _ in 0..8 {
            cur = self.aliases.get(cur)?;
            if let Some(&id) = self.by_name.get(cur) {
                return Some(id);
            }
        }
        None
    }

    pub fn by_name(&self, name: &str) -> Option<&TileDef> {
        self.id_of(name).and_then(|id| self.get(id))
    }

    pub fn iter(&self) -> impl Iterator<Item = &TileDef> {
        self.defs.iter()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Tile {
    pub type_id: u16,
    pub flags: u8,
    pub variant: u8,
    pub rotation_mirroring: u8,
}

impl Tile {
    pub const EMPTY: Tile = Tile {
        type_id: 0,
        flags: 0,
        variant: 0,
        rotation_mirroring: 0,
    };

    pub fn is_empty(&self) -> bool {
        self.type_id == 0
    }
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub tiles: Box<[Tile; (CHUNK_SIZE * CHUNK_SIZE) as usize]>,
}

impl Default for Chunk {
    fn default() -> Self {
        Chunk {
            tiles: Box::new([Tile::EMPTY; (CHUNK_SIZE * CHUNK_SIZE) as usize]),
        }
    }
}

impl Chunk {
    pub fn get(&self, x: i32, y: i32) -> Tile {
        self.tiles[(y * CHUNK_SIZE + x) as usize]
    }

    pub fn set(&mut self, x: i32, y: i32, t: Tile) {
        self.tiles[(y * CHUNK_SIZE + x) as usize] = t;
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.iter().all(Tile::is_empty)
    }
}

/// A grid's tiles, stored in 16×16 chunks keyed by chunk index.
#[derive(Debug, Clone, Default)]
pub struct GridTiles {
    pub chunks: HashMap<(i32, i32), Chunk>,
}

pub fn chunk_of(x: i32, y: i32) -> ((i32, i32), (i32, i32)) {
    (
        (x.div_euclid(CHUNK_SIZE), y.div_euclid(CHUNK_SIZE)),
        (x.rem_euclid(CHUNK_SIZE), y.rem_euclid(CHUNK_SIZE)),
    )
}

impl GridTiles {
    pub fn get(&self, x: i32, y: i32) -> Tile {
        let (c, (lx, ly)) = chunk_of(x, y);
        self.chunks.get(&c).map_or(Tile::EMPTY, |ch| ch.get(lx, ly))
    }

    pub fn set(&mut self, x: i32, y: i32, t: Tile) {
        let (c, (lx, ly)) = chunk_of(x, y);
        if t.is_empty() && !self.chunks.contains_key(&c) {
            return;
        }
        self.chunks.entry(c).or_default().set(lx, ly, t);
    }

    /// Every non-empty tile as `((x, y), tile)`.
    pub fn iter(&self) -> impl Iterator<Item = ((i32, i32), Tile)> + '_ {
        self.chunks.iter().flat_map(|(&(cx, cy), ch)| {
            ch.tiles.iter().enumerate().filter(|(_, t)| !t.is_empty()).map(move |(i, t)| {
                let i = i as i32;
                ((cx * CHUNK_SIZE + i % CHUNK_SIZE, cy * CHUNK_SIZE + i / CHUNK_SIZE), *t)
            })
        })
    }

    pub fn tile_count(&self) -> usize {
        self.iter().count()
    }

    /// Tile-space bounding box `(min_x, min_y, max_x, max_y)` inclusive.
    pub fn bounds(&self) -> Option<(i32, i32, i32, i32)> {
        self.iter().fold(None, |acc, ((x, y), _)| {
            Some(match acc {
                None => (x, y, x, y),
                Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x), d.max(y)),
            })
        })
    }
}

/// Decode a `MapGrid` component's `chunks` (map file versions 1–7).
pub fn decode_chunks(
    map_grid: &Node,
    tilemap: &HashMap<i32, String>,
    registry: &TileRegistry,
) -> Result<GridTiles, String> {
    use base64::Engine;
    let mut grid = GridTiles::default();
    let Some(chunks) = map_grid.get("chunks").and_then(Node::as_map) else {
        return Ok(grid);
    };
    let mut unknown: Vec<String> = Vec::new();
    for (_, chunk) in chunks {
        let ind = chunk
            .get("ind")
            .and_then(Node::as_vec2)
            .ok_or("chunk without ind")?;
        let (cx, cy) = (ind.0 as i32, ind.1 as i32);
        let size = chunk.get("size").and_then(Node::as_i64).unwrap_or(CHUNK_SIZE as i64) as i32;
        let version = chunk.get("version").and_then(Node::as_i64).unwrap_or(1);
        let b64 = chunk.get_str("tiles").ok_or("chunk without tiles")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|e| format!("bad base64 in chunk {cx},{cy}: {e}"))?;
        let stride = match version {
            v if v >= 7 => 7,
            6 => 6,
            _ => 4,
        };
        let need = (size * size) as usize * stride;
        if bytes.len() < need {
            return Err(format!("chunk {cx},{cy}: {} bytes, need {need}", bytes.len()));
        }
        for i in 0..(size * size) as usize {
            let b = &bytes[i * stride..];
            let (raw_id, rest) = if stride == 4 {
                (u16::from_le_bytes([b[0], b[1]]) as i32, &b[2..])
            } else {
                (i32::from_le_bytes([b[0], b[1], b[2], b[3]]), &b[4..])
            };
            let flags = rest[0];
            let variant = rest[1];
            let rotation_mirroring = if stride == 7 { rest[2] } else { 0 };
            let x = (i as i32) % size;
            let y = (i as i32) / size;
            let name = tilemap.get(&raw_id).map(String::as_str).unwrap_or("Space");
            let type_id = match registry.id_of(name) {
                Some(id) => id,
                None => {
                    if !unknown.iter().any(|u| u == name) {
                        unknown.push(name.to_string());
                    }
                    0
                }
            };
            if type_id == 0 {
                continue;
            }
            grid.set(
                cx * size + x,
                cy * size + y,
                Tile {
                    type_id,
                    flags,
                    variant,
                    rotation_mirroring,
                },
            );
        }
    }
    if !unknown.is_empty() {
        return Err(format!("unknown tile names: {}", unknown.join(", ")));
    }
    Ok(grid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_math() {
        assert_eq!(chunk_of(0, 0), ((0, 0), (0, 0)));
        assert_eq!(chunk_of(-1, -1), ((-1, -1), (15, 15)));
        assert_eq!(chunk_of(17, -16), ((1, -1), (1, 0)));
        let mut g = GridTiles::default();
        let t = Tile {
            type_id: 3,
            ..Tile::EMPTY
        };
        g.set(-1, 5, t);
        assert_eq!(g.get(-1, 5), t);
        assert_eq!(g.tile_count(), 1);
        assert_eq!(g.bounds(), Some((-1, 5, -1, 5)));
    }
}
