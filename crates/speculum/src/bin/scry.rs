//! `scry`: print entity prototypes' fully resolved components (inheritance
//! applied), optionally only some component types.
//!
//! ```bash
//! cargo run -p speculum --bin scry -- WallSolid ReinforcedWindow --only Sprite,IconSmooth
//! ```

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut only: Option<Vec<String>> = None;
    let mut ids = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--only" {
            only = args.get(i + 1).map(|s| s.split(',').map(str::to_string).collect());
            i += 2;
            continue;
        }
        ids.push(args[i].clone());
        i += 1;
    }
    let content = athanor::Content::load_upstream().expect("upstream content");
    for id in ids {
        let Some(p) = content.protos.entity(&id) else {
            println!("# {id}: not found\n");
            continue;
        };
        println!("# {id}  (parents: {:?})", p.parents);
        for (ty, node) in p.components() {
            if only.as_ref().is_some_and(|o| !o.iter().any(|t| t == ty)) {
                continue;
            }
            println!("{ty}: {node:?}");
        }
        println!();
    }
}
