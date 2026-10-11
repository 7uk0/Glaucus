//! # grimoire — the book of prototypes
//!
//! Every thing that can exist in the world is first written down here.
//! A Rust port of RobustToolbox's prototype + serialization layer:
//!
//! * [`node`] — `DataNode` equivalents parsed from YAML, with `!type:` tags.
//! * [`de`] — a `serde` deserializer over nodes, so ported components are
//!   plain `#[derive(Deserialize)]` structs.
//! * [`proto`] — the prototype manager: loading every `.yml` under
//!   `/Prototypes`, Robust's inheritance/merge rules, lookups.

pub mod de;
pub mod node;
pub mod proto;

pub use de::{from_node, DeError};
pub use node::{parse_color, parse_yaml, Mapping, Node};
pub use proto::{Diagnostic, Prototype, PrototypeManager};
