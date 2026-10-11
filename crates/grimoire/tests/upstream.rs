//! Integration tests against real upstream content. Skipped (passing) when
//! `upstream/` has not been summoned — run `tools/summon-upstream.sh` first.

use grimoire::{Diagnostic, PrototypeManager};
use vellum::Resources;

fn resources() -> Option<Resources> {
    let up = vellum::locate_upstream()?;
    Some(Resources::from_upstream_dir(&up))
}

#[test]
fn loads_every_frontier_prototype() {
    let Some(res) = resources() else {
        eprintln!("upstream not present; skipping");
        return;
    };
    let t = std::time::Instant::now();
    let pm = PrototypeManager::load_all(&res);
    eprintln!(
        "loaded {} prototypes ({} kinds) from {} files in {:?}",
        pm.total(),
        pm.kinds().count(),
        pm.files_loaded(),
        t.elapsed()
    );
    let mut yaml_errors = 0;
    let mut missing_parents = 0;
    let mut other = 0;
    for d in &pm.diagnostics {
        match d {
            Diagnostic::Yaml { .. } => yaml_errors += 1,
            Diagnostic::MissingParent { .. } => missing_parents += 1,
            _ => other += 1,
        }
    }
    for d in pm.diagnostics.iter().take(40) {
        eprintln!("  diag: {d}");
    }
    eprintln!("yaml errors: {yaml_errors}, missing parents: {missing_parents}, other: {other}");

    assert!(pm.count("entity") > 15_000, "only {} entities", pm.count("entity"));
    assert_eq!(yaml_errors, 0, "every upstream YAML file must parse");
    assert_eq!(missing_parents, 0, "every parent must resolve");

    // Spot-check a few inherited entities.
    let carp = pm.entity("MobCarp").expect("MobCarp");
    assert!(carp.has_component("HTN"), "carp should inherit an HTN brain");
    assert!(carp.has_component("MobState"));
    let human = pm.entity("MobHuman").expect("MobHuman");
    assert!(human.has_component("Hands"));
    assert!(pm.count("tile") > 100);
    assert!(pm.count("htnCompound") > 20);
    assert!(pm.count("vessel") > 50, "Frontier shipyard vessels");
}
