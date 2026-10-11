//! SS14 map files (`Resources/Maps/**.yml`), formats 6 and 7.
//!
//! Port of Robust's `EntityDeserializer`. A map file is a YAML document:
//!
//! ```yaml
//! meta: { format: 7, category: Grid, ... }
//! tilemap: { 0: Space, 1: Plating, ... }
//! entities:
//! - proto: ""            # entities with no prototype (maps, grids)
//!   entities:
//!   - uid: 1
//!     components: [ { type: MapGrid, chunks: ... }, ... ]
//! - proto: AirlockCommand
//!   entities:
//!   - uid: 20
//!     components: [ { type: Transform, pos: "-0.5,-1.5", parent: 1 } ]
//!     missingComponents: [ ... ]   # prototype components removed here
//! ```
//!
//! Component entries in the file are *deltas* layered over the prototype's
//! components — see [`MapEntity::effective_components`].

use std::collections::HashMap;

use grimoire::{Mapping, Node, PrototypeManager};
use vellum::{ResPath, Resources};

use crate::tiles::{decode_chunks, GridTiles, TileRegistry};

#[derive(Debug, Clone)]
pub struct MapEntity {
    pub uid: u32,
    /// `None` for prototype-less entities (`proto: ""`).
    pub proto: Option<String>,
    pub components: Vec<Node>,
    pub missing_components: Vec<String>,
    pub map_init: Option<bool>,
    pub paused: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformData {
    pub pos: (f32, f32),
    /// Radians, Robust convention (0 = south-facing, CCW positive).
    pub rot: f64,
    /// `None` = no parent (`invalid`): a map or an orphaned grid.
    pub parent: Option<u32>,
    pub anchored: Option<bool>,
}

impl MapEntity {
    pub fn component(&self, ty: &str) -> Option<&Node> {
        self.components.iter().find(|c| c.get_str("type") == Some(ty))
    }

    pub fn transform(&self) -> TransformData {
        let t = self.component("Transform");
        let pos = t.and_then(|t| t.get("pos")).and_then(Node::as_vec2).unwrap_or((0.0, 0.0));
        let rot = t.and_then(|t| t.get("rot")).and_then(Node::as_angle).unwrap_or(0.0);
        let parent = t
            .and_then(|t| t.get_str("parent"))
            .and_then(|p| p.parse::<u32>().ok());
        let anchored = t.and_then(|t| t.get("anchored")).and_then(Node::as_bool);
        TransformData {
            pos,
            rot,
            parent,
            anchored,
        }
    }

    pub fn name(&self) -> Option<&str> {
        self.component("MetaData").and_then(|m| m.get_str("name"))
    }

    /// The entity's components after layering this file's deltas over its
    /// prototype: prototype components (minus `missingComponents`), each
    /// overridden field-by-field by the file, plus file-only components.
    pub fn effective_components(&self, pm: &PrototypeManager) -> Vec<Node> {
        let base: Vec<Node> = self
            .proto
            .as_deref()
            .and_then(|id| pm.entity(id))
            .and_then(|p| p.get("components"))
            .and_then(Node::as_seq)
            .map(<[Node]>::to_vec)
            .unwrap_or_default();
        let mut out: Vec<Node> = base
            .into_iter()
            .filter(|c| {
                c.get_str("type")
                    .is_some_and(|t| !self.missing_components.iter().any(|m| m == t))
            })
            .collect();
        for delta in &self.components {
            let Some(ty) = delta.get_str("type") else { continue };
            match out.iter_mut().find(|c| c.get_str("type") == Some(ty)) {
                Some(existing) => {
                    if let (Some(em), Some(dm)) = (existing.as_map(), delta.as_map()) {
                        let mut merged: Mapping = em.clone();
                        for (k, v) in dm {
                            merged.insert(k.clone(), v.clone());
                        }
                        *existing = Node::Map(merged);
                    }
                }
                None => out.push(delta.clone()),
            }
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct MapFile {
    pub path: Option<ResPath>,
    pub format: u32,
    pub meta: Mapping,
    pub tilemap: HashMap<i32, String>,
    pub entities: Vec<MapEntity>,
    index: HashMap<u32, usize>,
}

#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("{0}")]
    Resource(#[from] vellum::VellumError),
    #[error("YAML: {0}")]
    Yaml(String),
    #[error("malformed map: {0}")]
    Malformed(String),
    #[error("unsupported map format {0}")]
    Format(u32),
}

impl MapFile {
    pub fn load(res: &Resources, path: &ResPath) -> Result<MapFile, MapError> {
        let text = res.read_string(path)?;
        let mut m = MapFile::parse(&text)?;
        m.path = Some(path.clone());
        Ok(m)
    }

    pub fn parse(text: &str) -> Result<MapFile, MapError> {
        let docs = grimoire::parse_yaml(text).map_err(|e| MapError::Yaml(e.to_string()))?;
        let root = docs.into_iter().next().ok_or_else(|| MapError::Malformed("empty file".into()))?;
        let meta = root.get("meta").and_then(Node::as_map).cloned().unwrap_or_default();
        let format = meta.get("format").and_then(Node::as_i64).unwrap_or(0) as u32;
        if !(6..=7).contains(&format) {
            return Err(MapError::Format(format));
        }
        let mut tilemap = HashMap::new();
        if let Some(tm) = root.get("tilemap").and_then(Node::as_map) {
            for (k, v) in tm {
                if let (Ok(id), Some(name)) = (k.parse::<i32>(), v.as_str()) {
                    tilemap.insert(id, name.to_string());
                }
            }
        }
        let mut entities = Vec::new();
        for group in root.get("entities").and_then(Node::as_seq).unwrap_or_default() {
            let proto = group.get_str("proto").filter(|p| !p.is_empty()).map(str::to_owned);
            for e in group.get("entities").and_then(Node::as_seq).unwrap_or_default() {
                let uid = e
                    .get("uid")
                    .and_then(Node::as_i64)
                    .ok_or_else(|| MapError::Malformed("entity without uid".into()))? as u32;
                entities.push(MapEntity {
                    uid,
                    proto: proto.clone(),
                    components: e.get("components").and_then(Node::as_seq).map(<[Node]>::to_vec).unwrap_or_default(),
                    missing_components: e.get("missingComponents").map(Node::as_str_list).unwrap_or_default(),
                    map_init: e.get("mapInit").and_then(Node::as_bool),
                    paused: e.get("paused").and_then(Node::as_bool),
                });
            }
        }
        let index = entities.iter().enumerate().map(|(i, e)| (e.uid, i)).collect();
        Ok(MapFile {
            path: None,
            format,
            meta,
            tilemap,
            entities,
            index,
        })
    }

    pub fn entity(&self, uid: u32) -> Option<&MapEntity> {
        self.index.get(&uid).map(|&i| &self.entities[i])
    }

    /// Entities carrying a `MapGrid` component.
    pub fn grids(&self) -> impl Iterator<Item = &MapEntity> {
        self.entities.iter().filter(|e| e.component("MapGrid").is_some())
    }

    /// Entities carrying a `Map` component (whole maps rather than grids).
    pub fn maps(&self) -> impl Iterator<Item = &MapEntity> {
        self.entities.iter().filter(|e| e.component("Map").is_some())
    }

    /// Decode the tiles of one grid entity.
    pub fn grid_tiles(&self, grid: &MapEntity, registry: &TileRegistry) -> Result<GridTiles, String> {
        match grid.component("MapGrid") {
            Some(mg) => decode_chunks(mg, &self.tilemap, registry),
            None => Err(format!("entity {} is not a grid", grid.uid)),
        }
    }

    /// Children of an entity (by Transform parent).
    pub fn children_of(&self, uid: u32) -> impl Iterator<Item = &MapEntity> {
        self.entities.iter().filter(move |e| e.transform().parent == Some(uid))
    }

    /// Apply entity prototype migrations: renamed prototypes are re-pointed,
    /// deleted ones are dropped from the map along with nothing else (their
    /// children keep their own transforms, as in Robust).
    pub fn apply_migrations(&mut self, migrations: &Migrations) -> MigrationReport {
        let mut report = MigrationReport::default();
        self.entities.retain_mut(|e| {
            let Some(p) = e.proto.as_deref() else { return true };
            match migrations.map.get(p) {
                Some(Some(new)) => {
                    e.proto = Some(new.clone());
                    report.renamed += 1;
                    true
                }
                Some(None) => {
                    report.deleted += 1;
                    false
                }
                None => true,
            }
        });
        self.index = self.entities.iter().enumerate().map(|(i, e)| (e.uid, i)).collect();
        report
    }

    /// Prototype ids referenced by this map that the prototype manager lacks.
    pub fn missing_prototypes(&self, pm: &PrototypeManager) -> Vec<String> {
        let mut out: Vec<String> = self
            .entities
            .iter()
            .filter_map(|e| e.proto.as_deref())
            .filter(|p| !pm.has("entity", p))
            .map(str::to_owned)
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MigrationReport {
    pub renamed: usize,
    pub deleted: usize,
}

/// Entity prototype migrations from `/migration.yml` and `/nf_migration.yml`
/// (port of Content's `MapMigrationSystem`). `None` means "delete".
#[derive(Debug, Clone, Default)]
pub struct Migrations {
    pub map: HashMap<String, Option<String>>,
}

impl Migrations {
    pub const FILES: [&'static str; 2] = ["/migration.yml", "/nf_migration.yml"];

    pub fn load(res: &Resources) -> Migrations {
        let mut m = Migrations::default();
        for f in Self::FILES {
            if let Ok(text) = res.read_string(&ResPath::new(f)) {
                m.add_source(&text);
            }
        }
        m
    }

    pub fn add_source(&mut self, text: &str) {
        let Ok(docs) = grimoire::parse_yaml(text) else { return };
        let Some(map) = docs.first().and_then(Node::as_map) else { return };
        for (k, v) in map {
            let target = match v.as_str() {
                Some(s) if !s.trim().is_empty() && s != "null" => Some(s.to_string()),
                _ => None,
            };
            self.map.insert(k.clone(), target);
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}
