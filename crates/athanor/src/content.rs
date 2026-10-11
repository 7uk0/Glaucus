//! The loaded game content: prototypes, tiles, locale, migrations and
//! derived tables. Shared (cheaply cloned) between simulation and client.

use std::ops::Deref;
use std::sync::Arc;

use astrolabe::{Migrations, TileRegistry};
use bevy_ecs::prelude::*;
use grimoire::{Node, PrototypeManager};
use vellum::locale::Locale;
use vellum::Resources;

use crate::damage::DamageTables;

pub struct ContentData {
    pub res: Resources,
    pub protos: PrototypeManager,
    pub tiles: TileRegistry,
    pub locale: Locale,
    pub migrations: Migrations,
    pub damage: DamageTables,
}

#[derive(Resource, Clone)]
pub struct Content(pub Arc<ContentData>);

impl Deref for Content {
    type Target = ContentData;
    fn deref(&self) -> &ContentData {
        &self.0
    }
}

impl Content {
    pub fn load(res: Resources) -> Content {
        let protos = PrototypeManager::load_all(&res);
        Self::from_parts(res, protos)
    }

    pub fn from_parts(res: Resources, protos: PrototypeManager) -> Content {
        let tiles = TileRegistry::from_prototypes(&protos);
        let locale = Locale::load(&res, "en-US");
        let migrations = Migrations::load(&res);
        let damage = DamageTables::from_prototypes(&protos);
        Content(Arc::new(ContentData {
            res,
            protos,
            tiles,
            locale,
            migrations,
            damage,
        }))
    }

    /// Content from the summoned `upstream/` checkout, if present.
    pub fn load_upstream() -> Option<Content> {
        let up = vellum::locate_upstream()?;
        Some(Self::load(Resources::from_upstream_dir(&up)))
    }
}

impl ContentData {
    /// Display name of an entity prototype: locale `ent-<id>` first (that is
    /// how upstream localizes prototypes), then the YAML `name`, then the id.
    pub fn entity_name(&self, proto: &str) -> String {
        if let Some(n) = self.locale.try_get(&format!("ent-{proto}"), &[]) {
            return n;
        }
        match self.protos.entity(proto).and_then(|p| p.get_str("name")) {
            Some(n) => self.localize(n),
            None => proto.to_string(),
        }
    }

    pub fn entity_description(&self, proto: &str) -> String {
        if let Some(d) = self.locale.get_attr(&format!("ent-{proto}"), "desc", &[]) {
            return d;
        }
        match self.protos.entity(proto).and_then(|p| p.get_str("description")) {
            Some(d) => self.localize(d),
            None => String::new(),
        }
    }

    /// Many prototype string fields hold locale keys (`tiles-steel-floor`);
    /// resolve them if they are, pass them through otherwise.
    pub fn localize(&self, s: &str) -> String {
        if self.locale.has(s) {
            self.locale.get(s, &[])
        } else {
            s.to_string()
        }
    }

    /// The resolved component node of an entity prototype.
    pub fn proto_component(&self, proto: &str, comp: &str) -> Option<&Node> {
        self.protos.entity(proto)?.component(comp)
    }
}
