//! Port of `IconSmoothSystem` (Content.Client/IconSmoothing, with Frontier's
//! `DiagonalNF` mode and `EntityIsSmoothOnEdge` rule) and
//! `IconSmoothSystem.Edge` (`SmoothEdgeComponent`).
//!
//! Walls, windows, tables, carpets and asteroid rock pick sprite states from
//! which neighbours on the same grid share their smoothing key:
//!
//! * `Corners`: four extra layers (SE, NE, NW, SW) each showing `{base}{fill}`
//!   where fill is CCW=1 | Diagonal=2 | CW=4. Every corner layer draws the
//!   same 4-directional state; its `DirectionOffset` picks which quarter.
//! * `CardinalFlags`: layer 0 = `{base}{N=1|S=2|E=4|W=8}`.
//! * `Diagonal`: layer 0 = `{base}1` if E, SE and S (rotated) all match.
//! * `DiagonalNF`: layer 0 = `{base}{0..4}` from E, S and the SE corner.
//! * `NoSprite`: no states of its own; only edges.
//!
//! Only anchored entities smooth; an unanchored one shows state 0.

use std::collections::HashMap;
use std::sync::Arc;

use athanor::transform::Grid;
use bevy::prelude::*;
use grimoire::Node;
use vellum::rsi::{Direction, RsiDirection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmoothMode {
    Corners,
    CardinalFlags,
    Diagonal,
    DiagonalNF,
    NoSprite,
}

/// `IconSmoothComponent` (+ whether the entity has a `SmoothEdge`).
#[derive(Debug, Clone)]
pub struct SmoothSpec {
    pub key: Option<String>,
    pub additional_keys: Vec<String>,
    pub base: String,
    pub mode: SmoothMode,
    pub enabled: bool,
    pub edge: bool,
}

impl SmoothSpec {
    pub fn parse(icon_smooth: &Node, smooth_edge: bool) -> SmoothSpec {
        let mode = match icon_smooth.get_str("mode").unwrap_or("Corners") {
            "CardinalFlags" => SmoothMode::CardinalFlags,
            "Diagonal" => SmoothMode::Diagonal,
            "DiagonalNF" => SmoothMode::DiagonalNF,
            "NoSprite" => SmoothMode::NoSprite,
            _ => SmoothMode::Corners,
        };
        SmoothSpec {
            key: icon_smooth.get_str("key").map(str::to_owned),
            additional_keys: icon_smooth.get("additionalKeys").map(Node::as_str_list).unwrap_or_default(),
            base: icon_smooth.get_str("base").unwrap_or("").to_owned(),
            mode,
            enabled: icon_smooth.get("enabled").and_then(Node::as_bool).unwrap_or(true),
            edge: smooth_edge,
        }
    }
}

/// `SpriteComponent.DirectionOffset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DirOffset {
    #[default]
    None,
    Clockwise,
    CounterClockwise,
    Flip,
}

/// `DirExt.OffsetRsiDir` (cardinal directions only, like upstream).
pub fn offset_rsi_dir(dir: RsiDirection, offset: DirOffset) -> RsiDirection {
    use RsiDirection::*;
    match (offset, dir) {
        (DirOffset::None, d) => d,
        (DirOffset::Clockwise, North) => East,
        (DirOffset::Clockwise, East) => South,
        (DirOffset::Clockwise, South) => West,
        (DirOffset::Clockwise, West) => North,
        (DirOffset::CounterClockwise, North) => West,
        (DirOffset::CounterClockwise, East) => North,
        (DirOffset::CounterClockwise, South) => East,
        (DirOffset::CounterClockwise, West) => South,
        (DirOffset::Flip, North) => South,
        (DirOffset::Flip, East) => West,
        (DirOffset::Flip, South) => North,
        (DirOffset::Flip, West) => East,
        (_, d) => d,
    }
}

/// `CornerLayers` in the order `SetCornerLayers` adds them, with their offsets.
pub const CORNER_LAYERS: [(&str, Corner, DirOffset); 4] = [
    ("enum.CornerLayers.SE", Corner::SE, DirOffset::None),
    ("enum.CornerLayers.NE", Corner::NE, DirOffset::CounterClockwise),
    ("enum.CornerLayers.NW", Corner::NW, DirOffset::Flip),
    ("enum.CornerLayers.SW", Corner::SW, DirOffset::Clockwise),
];

/// Index into [`Smoothed::corners`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    NE = 0,
    NW = 1,
    SW = 2,
    SE = 3,
}

/// `DirectionFlag` bits used by edges.
pub const FLAG_SOUTH: u8 = 1;
pub const FLAG_EAST: u8 = 2;
pub const FLAG_NORTH: u8 = 4;
pub const FLAG_WEST: u8 = 8;

/// `EdgeLayer` map key → the direction flag it shows for.
pub fn edge_layer_flag(map: &str) -> Option<u8> {
    match map {
        "enum.EdgeLayer.South" => Some(FLAG_SOUTH),
        "enum.EdgeLayer.East" => Some(FLAG_EAST),
        "enum.EdgeLayer.North" => Some(FLAG_NORTH),
        "enum.EdgeLayer.West" => Some(FLAG_WEST),
        _ => None,
    }
}

/// Every entity with an `IconSmooth` component, for neighbour matching.
#[derive(Resource, Default)]
pub struct SmoothIndex {
    pub specs: HashMap<Entity, Arc<SmoothSpec>>,
    /// Bumped on every insert/remove so cached results can invalidate.
    pub generation: u64,
}

/// What smoothing decided for one entity.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Smoothed {
    /// State for layer 0 (CardinalFlags / Diagonal / DiagonalNF).
    pub layer0: Option<String>,
    /// Corner fills, indexed by [`Corner`] (NE, NW, SW, SE).
    pub corners: Option<[u8; 4]>,
    /// Directions with a matching neighbour: those edge layers hide.
    pub edges: Option<u8>,
}

/// Where the entity being smoothed sits.
pub struct Placement<'a> {
    /// The grid, if the entity is anchored to one.
    pub grid: Option<&'a Grid>,
    pub tile: IVec2,
    /// `LocalRotation` (relative to the grid).
    pub rot: f32,
}

/// `Angle.RotateVec` then the `(Vector2i)` cast upstream applies.
fn rotate_offset(rot: f32, v: Vec2) -> IVec2 {
    let (s, c) = (rot as f64).sin_cos();
    let (x, y) = (v.x as f64, v.y as f64);
    IVec2::new((c * x - s * y).round() as i32, (s * x + c * y).round() as i32)
}

/// Frontier's `EntityIsSmoothOnEdge`: diagonal pieces only smooth on their
/// two straight sides (south and east in their own frame).
fn smooth_on_edge(target: &SmoothSpec, target_rot: f32, source_dir: Vec2) -> bool {
    if !matches!(target.mode, SmoothMode::Diagonal | SmoothMode::DiagonalNF) {
        return true;
    }
    let (s, c) = (-target_rot as f64).sin_cos();
    let (x, y) = (-source_dir.x as f64, -source_dir.y as f64);
    let (vx, vy) = (c * x - s * y, s * x + c * y);
    let angle = vy.atan2(vx) + std::f64::consts::FRAC_PI_2;
    matches!(Direction::cardinal_from_angle(angle), Direction::South | Direction::East)
}

/// `MatchingEntity`.
fn matching(
    me: &SmoothSpec,
    grid: &Grid,
    tile: IVec2,
    offset: IVec2,
    index: &SmoothIndex,
    rot_of: &impl Fn(Entity) -> f32,
) -> bool {
    grid.anchored_at(tile + offset).iter().any(|&e| {
        let Some(other) = index.specs.get(&e) else { return false };
        let Some(key) = &other.key else { return false };
        (me.key.as_ref() == Some(key) || me.additional_keys.contains(key))
            && other.enabled
            && smooth_on_edge(other, rot_of(e), offset.as_vec2())
    })
}

/// `CalculateNewSprite` for one entity.
pub fn calculate(spec: &SmoothSpec, at: &Placement, index: &SmoothIndex, rot_of: impl Fn(Entity) -> f32) -> Smoothed {
    let m = |off: IVec2| at.grid.is_some_and(|g| matching(spec, g, at.tile, off, index, &rot_of));
    let mut out = Smoothed::default();
    if !spec.enabled {
        return out;
    }
    match spec.mode {
        SmoothMode::NoSprite => {
            if spec.edge && at.grid.is_some() {
                let mut d = 0;
                for (off, flag) in [(IVec2::Y, FLAG_NORTH), (IVec2::NEG_Y, FLAG_SOUTH), (IVec2::X, FLAG_EAST), (IVec2::NEG_X, FLAG_WEST)] {
                    if m(off) {
                        d |= flag;
                    }
                }
                out.edges = Some(d);
            } else if spec.edge {
                out.edges = Some(0);
            }
        }
        SmoothMode::Corners => {
            let c = if at.grid.is_some() { corner_fill(at.rot, &m) } else { [0; 4] };
            let [ne, nw, sw, se] = c;
            if spec.edge {
                let mut d = 0;
                if se & sw != 0 {
                    d |= FLAG_SOUTH;
                }
                if se & ne != 0 {
                    d |= FLAG_EAST;
                }
                if ne & nw != 0 {
                    d |= FLAG_NORTH;
                }
                if nw & sw != 0 {
                    d |= FLAG_WEST;
                }
                out.edges = Some(d);
            }
            out.corners = Some(c);
        }
        SmoothMode::CardinalFlags => {
            let mut dirs = 0u8;
            if at.grid.is_some() {
                // CardinalConnectDirs: N=1, S=2, E=4, W=8.
                for (off, bit) in [(IVec2::Y, 1), (IVec2::NEG_Y, 2), (IVec2::X, 4), (IVec2::NEG_X, 8)] {
                    if m(off) {
                        dirs |= bit;
                    }
                }
            }
            out.layer0 = Some(format!("{}{}", spec.base, dirs));
            if spec.edge {
                let mut d = 0;
                for (bit, flag) in [(2, FLAG_SOUTH), (4, FLAG_EAST), (1, FLAG_NORTH), (8, FLAG_WEST)] {
                    if dirs & bit != 0 {
                        d |= flag;
                    }
                }
                out.edges = Some(d);
            }
        }
        SmoothMode::Diagonal => {
            let all = at.grid.is_some()
                && [Vec2::new(1.0, 0.0), Vec2::new(1.0, -1.0), Vec2::new(0.0, -1.0)]
                    .iter()
                    .all(|v| m(rotate_offset(at.rot, *v)));
            out.layer0 = Some(format!("{}{}", spec.base, if all { 1 } else { 0 }));
        }
        SmoothMode::DiagonalNF => {
            let mut value = 0;
            if at.grid.is_some() {
                for (i, v) in [Vec2::new(1.0, 0.0), Vec2::new(0.0, -1.0)].iter().enumerate() {
                    if m(rotate_offset(at.rot, *v)) {
                        value |= 1 << i;
                    }
                }
                if value == 3 && m(rotate_offset(at.rot, Vec2::new(1.0, -1.0))) {
                    value = 4;
                }
            }
            out.layer0 = Some(format!("{}{}", spec.base, value));
        }
    }
    out
}

/// `CalculateCornerFill` → (NE, NW, SW, SE), rotated by the entity's
/// cardinal local rotation like upstream.
fn corner_fill(rot: f32, m: &impl Fn(IVec2) -> bool) -> [u8; 4] {
    const CCW: u8 = 1;
    const DIAG: u8 = 2;
    const CW: u8 = 4;
    let (mut ne, mut se, mut sw, mut nw) = (0u8, 0u8, 0u8, 0u8);
    if m(IVec2::new(0, 1)) {
        ne |= CCW;
        nw |= CW;
    }
    if m(IVec2::new(1, 1)) {
        ne |= DIAG;
    }
    if m(IVec2::new(1, 0)) {
        ne |= CW;
        se |= CCW;
    }
    if m(IVec2::new(1, -1)) {
        se |= DIAG;
    }
    if m(IVec2::new(0, -1)) {
        se |= CW;
        sw |= CCW;
    }
    if m(IVec2::new(-1, -1)) {
        sw |= DIAG;
    }
    if m(IVec2::new(-1, 0)) {
        sw |= CW;
        nw |= CCW;
    }
    if m(IVec2::new(-1, 1)) {
        nw |= DIAG;
    }
    match Direction::cardinal_from_angle(rot as f64) {
        Direction::North => [sw, se, ne, nw],
        Direction::West => [se, ne, nw, sw],
        Direction::South => [ne, nw, sw, se],
        _ => [nw, sw, se, ne],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(n: [bool; 8]) -> [u8; 4] {
        // N, NE, E, SE, S, SW, W, NW
        let offs = [(0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0), (-1, 1)];
        corner_fill(0.0, &|o: IVec2| offs.iter().position(|&(x, y)| IVec2::new(x, y) == o).is_some_and(|i| n[i]))
    }

    #[test]
    fn corners_match_upstream() {
        assert_eq!(fill([false; 8]), [0, 0, 0, 0]);
        // Neighbour to the north: NE gets CCW(1), NW gets CW(4).
        let mut n = [false; 8];
        n[0] = true;
        assert_eq!(fill(n), [1, 4, 0, 0]);
        assert_eq!(fill([true; 8]), [7, 7, 7, 7]);
    }

    #[test]
    fn dir_offsets() {
        use RsiDirection::*;
        // Eye-aligned (South) corner layers show S, E, N, W frames.
        let frames: Vec<_> = CORNER_LAYERS.iter().map(|(_, _, o)| offset_rsi_dir(South, *o)).collect();
        assert_eq!(frames, vec![South, East, North, West]);
    }

    #[test]
    fn diagonal_edges() {
        let spec = SmoothSpec {
            key: Some("walls".into()),
            additional_keys: vec![],
            base: "state".into(),
            mode: SmoothMode::DiagonalNF,
            enabled: true,
            edge: false,
        };
        // An unrotated diagonal accepts neighbours from its east and south only.
        assert!(smooth_on_edge(&spec, 0.0, Vec2::new(-1.0, 0.0))); // source is east of target
        assert!(smooth_on_edge(&spec, 0.0, Vec2::new(0.0, 1.0))); // source is south of target
        assert!(!smooth_on_edge(&spec, 0.0, Vec2::new(1.0, 0.0)));
        assert!(!smooth_on_edge(&spec, 0.0, Vec2::new(0.0, -1.0)));
        assert_eq!(rotate_offset(std::f32::consts::FRAC_PI_2, Vec2::new(1.0, -1.0)), IVec2::new(1, 1));
    }
}
