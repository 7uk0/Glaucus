//! The prototype manager. Port of Robust's `PrototypeManager` loading and
//! inheritance rules (`SerializationManager.Composition.cs`,
//! `ComponentRegistrySerializer.PushInheritance`).
//!
//! Inheritance semantics, matching Robust:
//! * Parents are applied in declaration order; for each parent, every field
//!   the child lacks is copied from the parent (child wins).
//! * `id`, `parent`, `abstract` (and `categories` on entities) never inherit.
//! * Fields marked `[AlwaysPushInheritance]` merge instead of override:
//!   sequences become `child ++ parent`, mappings combine child-first.
//! * Entity `components` merge per component `type`: a component present on
//!   both is merged field-by-field (child wins), parent-only components are
//!   appended.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use vellum::{ResPath, Resources};

use crate::node::{parse_yaml, Mapping, Node};

#[derive(Debug, Clone)]
pub struct Prototype {
    pub kind: String,
    pub id: String,
    pub parents: Vec<String>,
    pub is_abstract: bool,
    /// Fully resolved (inheritance applied) data, including `type`/`id`.
    pub data: Mapping,
    pub source: ResPath,
}

impl Prototype {
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.data.get(key)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.data.get(key).and_then(Node::as_str)
    }

    /// For entity prototypes: the resolved component list, keyed by type.
    pub fn components(&self) -> IndexMap<&str, &Node> {
        let mut out = IndexMap::new();
        if let Some(seq) = self.data.get("components").and_then(Node::as_seq) {
            for c in seq {
                if let Some(t) = c.get_str("type") {
                    out.insert(t, c);
                }
            }
        }
        out
    }

    pub fn component(&self, ty: &str) -> Option<&Node> {
        self.data
            .get("components")
            .and_then(Node::as_seq)?
            .iter()
            .find(|c| c.get_str("type") == Some(ty))
    }

    pub fn has_component(&self, ty: &str) -> bool {
        self.component(ty).is_some()
    }

    /// Deserialize this prototype's data into a typed struct.
    pub fn read<'a, T: serde::Deserialize<'a>>(&'a self) -> Result<T, crate::DeError> {
        T::deserialize(MapRef(&self.data))
    }
}

/// Helper so a bare `Mapping` can be deserialized without cloning into a Node.
struct MapRef<'a>(&'a Mapping);

impl<'de> serde::Deserializer<'de> for MapRef<'de> {
    type Error = crate::DeError;
    fn deserialize_any<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        use serde::de::value::MapDeserializer;
        v.visit_map(MapDeserializer::new(self.0.iter().map(|(k, v)| (k.as_str(), v))))
    }
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

impl<'de> serde::de::IntoDeserializer<'de, crate::DeError> for &'de Node {
    type Deserializer = &'de Node;
    fn into_deserializer(self) -> Self::Deserializer {
        self
    }
}

#[derive(Debug, Clone)]
pub enum Diagnostic {
    Yaml { source: ResPath, error: String },
    NotAMapping { source: ResPath },
    MissingType { source: ResPath },
    MissingId { source: ResPath, kind: String },
    Duplicate { kind: String, id: String, first: ResPath, second: ResPath },
    MissingParent { kind: String, id: String, parent: String },
    Cycle { kind: String, id: String },
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Diagnostic::Yaml { source, error } => write!(f, "{source}: YAML error: {error}"),
            Diagnostic::NotAMapping { source } => write!(f, "{source}: prototype entry is not a mapping"),
            Diagnostic::MissingType { source } => write!(f, "{source}: prototype without `type`"),
            Diagnostic::MissingId { source, kind } => write!(f, "{source}: `{kind}` prototype without `id`"),
            Diagnostic::Duplicate { kind, id, first, second } => {
                write!(f, "duplicate {kind} '{id}' in {first} and {second} (later wins)")
            }
            Diagnostic::MissingParent { kind, id, parent } => {
                write!(f, "{kind} '{id}' has unknown parent '{parent}'")
            }
            Diagnostic::Cycle { kind, id } => write!(f, "{kind} '{id}' has an inheritance cycle"),
        }
    }
}

#[derive(Default)]
struct KindTable {
    raw: IndexMap<String, Prototype>,
    resolved: IndexMap<String, Prototype>,
}

#[derive(Default)]
pub struct PrototypeManager {
    kinds: HashMap<String, KindTable>,
    ignored: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
    files_loaded: usize,
}

/// Fields that never inherit, for every kind.
const NEVER_INHERIT: &[&str] = &["type", "id", "parent", "abstract"];

/// Per-kind extra rules: (kind, never-inherit fields, always-push fields).
const KIND_RULES: &[(&str, &[&str], &[&str])] = &[
    ("entity", &["categories"], &[]),
    ("startingGear", &[], &["equipment", "inhand", "storage", "encryptionKeys", "implants", "cartridges"]),
    ("loadout", &[], &["encryptionKeys", "implants", "cartridges"]),
];

/// `ComponentRegistry` fields, merged per component type on inheritance
/// (entity `components`; `[AlwaysPushInheritance]` registries on other kinds).
const REGISTRY_FIELDS: &[(&str, &str)] = &[
    ("entity", "components"),
    ("spaceBiome", "chunkComponents"),
    ("pointOfInterest", "addComponents"),
];

/// Component fields with `[AlwaysPushInheritance]`.
const COMPONENT_ALWAYS_PUSH: &[(&str, &str)] = &[
    ("UserInterface", "interfaces"),
    ("ActionGrant", "actions"),
    ("ItemActionGrant", "actions"),
];

impl PrototypeManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load `/EnginePrototypes` and `/Prototypes` from every resource root,
    /// honouring `/IgnoredPrototypes/ignoredPrototypes.yml`, then resolve
    /// inheritance.
    pub fn load_all(res: &Resources) -> Self {
        let mut pm = PrototypeManager::new();
        if let Ok(text) = res.read_string(&ResPath::new("/IgnoredPrototypes/ignoredPrototypes.yml")) {
            if let Ok(docs) = parse_yaml(&text) {
                for d in docs {
                    pm.ignored.extend(d.as_str_list());
                }
            }
        }
        for dir in ["/EnginePrototypes", "/Prototypes"] {
            for path in res.find_files(&ResPath::new(dir), "yml") {
                match res.read_string(&path) {
                    Ok(text) => pm.load_str(&text, path),
                    Err(e) => pm.diagnostics.push(Diagnostic::Yaml {
                        source: path,
                        error: e.to_string(),
                    }),
                }
            }
        }
        pm.resolve();
        pm
    }

    pub fn files_loaded(&self) -> usize {
        self.files_loaded
    }

    fn is_ignored(&self, source: &ResPath) -> bool {
        let s = source.as_str();
        self.ignored.iter().any(|ig| {
            let ig = ig.trim_end_matches('/');
            s == ig || s.starts_with(&format!("{ig}/"))
        })
    }

    /// Parse one YAML file worth of prototypes (unresolved until [`resolve`]).
    pub fn load_str(&mut self, text: &str, source: ResPath) {
        self.files_loaded += 1;
        let docs = match parse_yaml(text) {
            Ok(d) => d,
            Err(e) => {
                self.diagnostics.push(Diagnostic::Yaml {
                    source,
                    error: e.to_string(),
                });
                return;
            }
        };
        let force_abstract = self.is_ignored(&source);
        for doc in docs {
            let entries: Vec<Node> = match doc {
                Node::Seq(s) => s,
                Node::Null => continue,
                other => vec![other],
            };
            for entry in entries {
                let Node::Map(map) = entry else {
                    self.diagnostics.push(Diagnostic::NotAMapping { source: source.clone() });
                    continue;
                };
                let Some(kind) = map.get("type").and_then(Node::as_str).map(str::to_owned) else {
                    self.diagnostics.push(Diagnostic::MissingType { source: source.clone() });
                    continue;
                };
                let Some(id) = map.get("id").and_then(Node::as_str).map(str::to_owned) else {
                    self.diagnostics.push(Diagnostic::MissingId {
                        source: source.clone(),
                        kind,
                    });
                    continue;
                };
                let parents = map.get("parent").map(Node::as_str_list).unwrap_or_default();
                let is_abstract =
                    force_abstract || map.get("abstract").and_then(Node::as_bool).unwrap_or(false);
                let proto = Prototype {
                    kind: kind.clone(),
                    id: id.clone(),
                    parents,
                    is_abstract,
                    data: map,
                    source: source.clone(),
                };
                let table = self.kinds.entry(kind.clone()).or_default();
                if let Some(prev) = table.raw.get(&id) {
                    self.diagnostics.push(Diagnostic::Duplicate {
                        kind,
                        id: id.clone(),
                        first: prev.source.clone(),
                        second: source.clone(),
                    });
                }
                table.raw.insert(id, proto);
            }
        }
    }

    /// Apply inheritance to every loaded prototype.
    pub fn resolve(&mut self) {
        let kinds: Vec<String> = self.kinds.keys().cloned().collect();
        for kind in kinds {
            let table = self.kinds.get_mut(&kind).unwrap();
            let ids: Vec<String> = table.raw.keys().cloned().collect();
            let mut resolved: HashMap<String, Mapping> = HashMap::new();
            let mut diags = Vec::new();
            for id in &ids {
                let mut visiting = HashSet::new();
                resolve_one(&kind, id, &table.raw, &mut resolved, &mut visiting, &mut diags);
            }
            table.resolved = ids
                .iter()
                .map(|id| {
                    let raw = &table.raw[id];
                    let mut p = raw.clone();
                    if let Some(data) = resolved.remove(id) {
                        p.data = data;
                    }
                    (id.clone(), p)
                })
                .collect();
            self.diagnostics.extend(diags);
        }
    }

    pub fn kinds(&self) -> impl Iterator<Item = &str> {
        self.kinds.keys().map(String::as_str)
    }

    pub fn count(&self, kind: &str) -> usize {
        self.kinds.get(kind).map_or(0, |t| t.resolved.len())
    }

    pub fn total(&self) -> usize {
        self.kinds.values().map(|t| t.resolved.len()).sum()
    }

    pub fn get(&self, kind: &str, id: &str) -> Option<&Prototype> {
        self.kinds.get(kind)?.resolved.get(id)
    }

    pub fn has(&self, kind: &str, id: &str) -> bool {
        self.get(kind, id).is_some()
    }

    /// All resolved prototypes of a kind (abstract ones included).
    pub fn iter_kind<'a>(&'a self, kind: &str) -> impl Iterator<Item = &'a Prototype> + 'a {
        self.kinds.get(kind).into_iter().flat_map(|t| t.resolved.values())
    }

    pub fn entity(&self, id: &str) -> Option<&Prototype> {
        self.get("entity", id)
    }

    /// Is `id` equal to or descended from `ancestor`?
    pub fn inherits(&self, kind: &str, id: &str, ancestor: &str) -> bool {
        let mut stack = vec![id.to_string()];
        let mut seen = HashSet::new();
        while let Some(cur) = stack.pop() {
            if cur == ancestor {
                return true;
            }
            if !seen.insert(cur.clone()) {
                continue;
            }
            if let Some(p) = self.get(kind, &cur) {
                stack.extend(p.parents.iter().cloned());
            }
        }
        false
    }

    /// Insert or replace a single, already-resolved prototype (tests, mods).
    pub fn insert_resolved(&mut self, proto: Prototype) {
        let t = self.kinds.entry(proto.kind.clone()).or_default();
        t.raw.insert(proto.id.clone(), proto.clone());
        t.resolved.insert(proto.id.clone(), proto);
    }
}

fn resolve_one(
    kind: &str,
    id: &str,
    raw: &IndexMap<String, Prototype>,
    resolved: &mut HashMap<String, Mapping>,
    visiting: &mut HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Mapping> {
    if let Some(done) = resolved.get(id) {
        return Some(done.clone());
    }
    let proto = raw.get(id)?;
    if !visiting.insert(id.to_string()) {
        diags.push(Diagnostic::Cycle {
            kind: kind.into(),
            id: id.into(),
        });
        return None;
    }
    let mut data = proto.data.clone();
    for parent in &proto.parents {
        match resolve_one(kind, parent, raw, resolved, visiting, diags) {
            Some(pdata) => data = push_inheritance(kind, &data, &pdata),
            None => {
                if !raw.contains_key(parent) {
                    diags.push(Diagnostic::MissingParent {
                        kind: kind.into(),
                        id: id.into(),
                        parent: parent.clone(),
                    });
                }
            }
        }
    }
    visiting.remove(id);
    resolved.insert(id.to_string(), data.clone());
    Some(data)
}

fn kind_rules(kind: &str) -> (&'static [&'static str], &'static [&'static str]) {
    KIND_RULES
        .iter()
        .find(|(k, _, _)| *k == kind)
        .map(|(_, n, a)| (*n, *a))
        .unwrap_or((&[], &[]))
}

/// Push one parent's data into a child mapping.
pub fn push_inheritance(kind: &str, child: &Mapping, parent: &Mapping) -> Mapping {
    let (never, always) = kind_rules(kind);
    let mut out = child.clone();
    for (k, pv) in parent {
        if NEVER_INHERIT.contains(&k.as_str()) || never.contains(&k.as_str()) {
            continue;
        }
        match out.get_mut(k) {
            None => {
                out.insert(k.clone(), pv.clone());
            }
            Some(cv) => {
                if REGISTRY_FIELDS.contains(&(kind, k.as_str())) {
                    *cv = merge_component_registry(cv, pv);
                } else if always.contains(&k.as_str()) {
                    *cv = combine(cv, pv);
                }
            }
        }
    }
    out
}

/// `CombineMappings` / `PushInheritanceSequence`.
fn combine(child: &Node, parent: &Node) -> Node {
    match (child.untagged(), parent.untagged()) {
        (Node::Seq(c), Node::Seq(p)) => {
            let mut v = c.clone();
            v.extend(p.iter().cloned());
            Node::Seq(v)
        }
        (Node::Map(c), Node::Map(p)) => {
            let mut m = c.clone();
            for (k, v) in p {
                if !m.contains_key(k) {
                    m.insert(k.clone(), v.clone());
                }
            }
            Node::Map(m)
        }
        _ => child.clone(),
    }
}

fn merge_component_registry(child: &Node, parent: &Node) -> Node {
    let (Some(cseq), Some(pseq)) = (child.as_seq(), parent.as_seq()) else {
        return child.clone();
    };
    let mut out: Vec<Node> = cseq.to_vec();
    for pcomp in pseq {
        let Some(ptype) = pcomp.get_str("type") else { continue };
        match out.iter_mut().find(|c| c.get_str("type") == Some(ptype)) {
            Some(ccomp) => {
                let (Some(cm), Some(pm)) = (ccomp.as_map(), pcomp.as_map()) else {
                    continue;
                };
                let mut merged = cm.clone();
                for (k, pv) in pm {
                    match merged.get_mut(k) {
                        None => {
                            merged.insert(k.clone(), pv.clone());
                        }
                        Some(cv) => {
                            if COMPONENT_ALWAYS_PUSH.iter().any(|(t, f)| *t == ptype && f == k) {
                                *cv = combine(cv, pv);
                            }
                        }
                    }
                }
                *ccomp = Node::Map(merged);
            }
            None => out.push(pcomp.clone()),
        }
    }
    Node::Seq(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAML: &str = r#"
- type: entity
  id: BaseMob
  abstract: true
  name: mob
  categories: [ HideSpawnMenu ]
  components:
  - type: Sprite
    sprite: Mobs/base.rsi
    state: alive
  - type: Damageable
    damageContainer: Biological
  - type: UserInterface
    interfaces:
      enum.A: { type: AUi }

- type: entity
  id: BaseHostile
  abstract: true
  components:
  - type: NpcFactionMember
    factions: [ SimpleHostile ]

- type: entity
  parent: [ BaseMob, BaseHostile ]
  id: MobCarp
  description: fish
  components:
  - type: Sprite
    sprite: Mobs/carp.rsi
  - type: UserInterface
    interfaces:
      enum.B: { type: BUi }

- type: entity
  parent: MobCarp
  id: MobCarpRainbow
  name: rainbow carp
"#;

    fn pm() -> PrototypeManager {
        let mut pm = PrototypeManager::new();
        pm.load_str(YAML, ResPath::new("/Prototypes/test.yml"));
        pm.resolve();
        pm
    }

    #[test]
    fn inheritance_merges_like_robust() {
        let pm = pm();
        assert!(pm.diagnostics.is_empty(), "{:?}", pm.diagnostics);
        let carp = pm.entity("MobCarp").unwrap();
        assert!(!carp.is_abstract);
        assert_eq!(carp.get_str("name"), Some("mob"));
        assert_eq!(carp.get_str("description"), Some("fish"));
        assert!(carp.get("categories").is_none(), "categories never inherit");
        let sprite = carp.component("Sprite").unwrap();
        assert_eq!(sprite.get_str("sprite"), Some("Mobs/carp.rsi"));
        assert_eq!(sprite.get_str("state"), Some("alive"));
        assert!(carp.has_component("Damageable"));
        assert!(carp.has_component("NpcFactionMember"));
        let ui = carp.component("UserInterface").unwrap().get("interfaces").unwrap();
        assert!(ui.get("enum.A").is_some() && ui.get("enum.B").is_some(), "always-push merges");
        // Component order: child's first, then inherited.
        let order: Vec<_> = carp.components().keys().copied().collect();
        assert_eq!(order, ["Sprite", "UserInterface", "Damageable", "NpcFactionMember"]);

        let rainbow = pm.entity("MobCarpRainbow").unwrap();
        assert_eq!(rainbow.get_str("name"), Some("rainbow carp"));
        assert_eq!(rainbow.component("Sprite").unwrap().get_str("sprite"), Some("Mobs/carp.rsi"));
        assert!(pm.inherits("entity", "MobCarpRainbow", "BaseHostile"));
        assert!(!pm.inherits("entity", "BaseMob", "MobCarp"));
    }

    #[test]
    fn reports_missing_parent() {
        let mut pm = PrototypeManager::new();
        pm.load_str("- type: entity\n  id: A\n  parent: Nope\n", ResPath::new("/x.yml"));
        pm.resolve();
        assert!(matches!(pm.diagnostics[0], Diagnostic::MissingParent { .. }));
        assert!(pm.entity("A").is_some());
    }
}
