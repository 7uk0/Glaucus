//! # astrolabe — charts of the void
//!
//! Maps, grids and tiles. Port of RobustToolbox's map layer:
//!
//! * [`tiles`] — tile definitions (`tile` prototypes), `Tile`, 16×16 chunks
//!   and the base64 chunk codec used by map files.
//! * [`map`] — reading `Resources/Maps/**.yml` (formats 6 and 7): the
//!   tilemap, entity groups, per-entity component deltas.
//! * [`decals`] — floor decals stored on grids.

pub mod decals;
pub mod map;
pub mod tiles;

pub use decals::{decal_defs, read_decal_grid, Decal, DecalDef};
pub use map::{MapEntity, MapError, MapFile, MigrationReport, Migrations, TransformData};
pub use tiles::{GridTiles, Tile, TileDef, TileRegistry, CHUNK_SIZE};
