//! # speculum — the scrying mirror
//!
//! A census of every component type upstream content uses: how many entity
//! prototypes carry it, how many times it is placed in the shipped maps,
//! where upstream defines it, and whether Caput Mortuum has ported it
//! (a factory is registered under its YAML name).
//!
//! ```bash
//! cargo run --release -p speculum > docs/akasha/census.md
//! ```

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use astrolabe::map::{MapFile, Migrations};
use athanor::spawn::ComponentFactories;
use athanor::{AthanorPlugin, Content};
use bevy_app::App;
use vellum::ResPath;

#[derive(Default)]
struct Row {
    protos: usize,
    placed: usize,
    defined: Vec<String>,
}

/// `class FooComponent` declarations in upstream C#: name → relative paths.
fn component_classes(up: &Path) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for root in ["frontier", "RobustToolbox"] {
        let base = up.join(root);
        for entry in walkdir::WalkDir::new(&base).into_iter().filter_map(Result::ok) {
            let p = entry.path();
            if p.extension().is_none_or(|e| e != "cs") || p.components().any(|c| c.as_os_str() == "Tests" || c.as_os_str() == "bin" || c.as_os_str() == "obj") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(p) else { continue };
            for line in text.lines() {
                let Some(i) = line.find("class ") else { continue };
                let rest = &line[i + 6..];
                let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                if let Some(base_name) = name.strip_suffix("Component")
                    && !base_name.is_empty()
                    && (line.contains("Component :") || line.contains("Component\t:") || text.contains("[RegisterComponent"))
                {
                    let rel = p.strip_prefix(up).unwrap_or(p).to_string_lossy().to_string();
                    out.entry(base_name.to_string()).or_default().push(rel);
                }
            }
        }
    }
    out
}

/// Component names our code reads straight from raw YAML (`EntityData`):
/// string literals passed to `get(..)`, `has(..)`, `component(..)`, `has_component(..)`.
fn read_as_data() -> std::collections::HashSet<String> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let mut out = std::collections::HashSet::new();
    for entry in walkdir::WalkDir::new(&crates).into_iter().filter_map(Result::ok) {
        if entry.path().extension().is_none_or(|e| e != "rs") || entry.path().components().any(|c| c.as_os_str() == "speculum") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
        for pat in ["get(\"", "has(\"", "component(\"", "has_component(\""] {
            for (i, _) in text.match_indices(pat) {
                let rest = &text[i + pat.len()..];
                let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
                if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && rest[name.len()..].starts_with('"') {
                    out.insert(name);
                }
            }
        }
    }
    out
}

/// "Content.Server/Atmos/EntitySystems/Foo.cs" → "Content.Server/Atmos".
fn area(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    // frontier/Content.Server/_NF/Bank/... keeps the _NF level.
    let skip = 1;
    let take = if parts.get(skip + 1).is_some_and(|p| p.starts_with('_')) { 3 } else { 2 };
    parts.iter().skip(skip).take(take).copied().collect::<Vec<_>>().join("/")
}

fn main() {
    let Some(up) = vellum::locate_upstream() else {
        eprintln!("no upstream/ checkout: run tools/summon-upstream.sh");
        std::process::exit(1);
    };
    let content = Content::load_upstream().expect("content");
    let mut app = App::new();
    app.add_plugins((
        AthanorPlugin::headless(content.clone()),
        homunculus::HomunculusPlugin,
        mercury::MercuryPlugin,
        alembic::AlembicPlugin,
    ));
    let reg = app.world().resource::<ComponentFactories>();
    let factories: std::collections::HashSet<String> = reg.names().map(|n| n.to_string()).collect();
    let read = read_as_data();
    // Ported = a factory, or read as data by some system/renderer.
    let ported: std::collections::HashSet<String> = factories.union(&read).cloned().collect();

    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    let mut comps_of: HashMap<&str, Vec<String>> = HashMap::new();
    for p in content.protos.iter_kind("entity") {
        let names: Vec<String> = p.components().keys().map(|s| s.to_string()).collect();
        if !p.is_abstract {
            for n in &names {
                rows.entry(n.clone()).or_default().protos += 1;
            }
        }
        comps_of.insert(p.id.as_str(), names);
    }

    let migrations = Migrations::load(&content.res);
    let mut maps = 0;
    let mut placed_total = 0usize;
    for path in content.res.find_files(&ResPath::new("/Maps"), "yml") {
        let Ok(mut map) = MapFile::load(&content.res, &path) else { continue };
        map.apply_migrations(&migrations);
        maps += 1;
        for e in &map.entities {
            let Some(proto) = e.proto.as_deref() else { continue };
            placed_total += 1;
            for n in comps_of.get(proto).into_iter().flatten() {
                rows.entry(n.clone()).or_default().placed += 1;
            }
        }
    }

    let classes = component_classes(&up);
    for (name, row) in rows.iter_mut() {
        if let Some(paths) = classes.get(name) {
            let mut v = paths.clone();
            v.sort();
            row.defined = v;
        }
    }

    // Summary.
    let n = rows.len();
    let n_ported = rows.keys().filter(|k| ported.contains(*k)).count();
    let proto_uses: usize = rows.values().map(|r| r.protos).sum();
    let proto_ported: usize = rows.iter().filter(|(k, _)| ported.contains(*k)).map(|(_, r)| r.protos).sum();
    let placed_uses: usize = rows.values().map(|r| r.placed).sum();
    let placed_ported: usize = rows.iter().filter(|(k, _)| ported.contains(*k)).map(|(_, r)| r.placed).sum();

    println!("# Component census\n");
    println!("Generated by `cargo run --release -p speculum > docs/akasha/census.md` against the pinned upstream.");
    println!("A component counts as **ported** (✅) when a factory is registered under its YAML name, or");
    println!("(🟡) when some system or the renderer reads it as raw YAML (`EntityData`) without a factory.");
    println!("Either way, something in Caput Mortuum acts on it; how fully is in `docs/PORTING.md`.\n");
    println!("| | total | ported | % |\n|---|---|---|---|");
    println!("| component types used by entity prototypes | {n} | {n_ported} | {:.1}% |", pct(n_ported, n));
    println!("| component uses across non-abstract prototypes | {proto_uses} | {proto_ported} | {:.1}% |", pct(proto_ported, proto_uses));
    println!("| component instances placed in {maps} maps ({placed_total} entities) | {placed_uses} | {placed_ported} | {:.1}% |\n", pct(placed_ported, placed_uses));

    // By upstream area.
    let mut areas: BTreeMap<String, (usize, usize, usize, usize)> = BTreeMap::new();
    for (name, r) in &rows {
        let a = r.defined.first().map(|p| area(p)).unwrap_or_else(|| "(not found in C#)".into());
        let e = areas.entry(a).or_default();
        e.0 += 1;
        e.2 += r.placed;
        if ported.contains(name) {
            e.1 += 1;
            e.3 += r.placed;
        }
    }
    let mut areas: Vec<_> = areas.into_iter().collect();
    areas.sort_by(|a, b| (b.1 .2 - b.1 .3).cmp(&(a.1 .2 - a.1 .3)));
    println!("## By upstream area (most unported map placements first)\n");
    println!("| Area | types | ported | placements | ported placements |\n|---|---|---|---|---|");
    for (a, (t, tp, pl, pp)) in &areas {
        println!("| `{a}` | {t} | {tp} | {pl} | {pp} ({:.0}%) |", pct(*pp, *pl));
    }

    // Every component.
    let mut list: Vec<_> = rows.iter().collect();
    list.sort_by(|a, b| b.1.placed.cmp(&a.1.placed).then(b.1.protos.cmp(&a.1.protos)).then(a.0.cmp(b.0)));
    println!("\n## Every component (by map placements)\n");
    println!("| Component | ported | prototypes | placed | defined in |\n|---|---|---|---|---|");
    for (name, r) in list {
        let def = r.defined.first().map(|s| format!("`{s}`")).unwrap_or_default();
        let mark = if factories.contains(name) { "✅" } else if read.contains(name) { "🟡" } else { "" };
        println!("| {name} | {mark} | {} | {} | {def} |", r.protos, r.placed);
    }
}

fn pct(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { a as f64 * 100.0 / b as f64 }
}
