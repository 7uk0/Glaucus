//! RSI — "Robust Station Image" sprite sheets.
//!
//! An `.rsi` is a directory holding `meta.json` plus one PNG per state. Each
//! state PNG packs its frames row-major (`width / size.x` columns), ordered
//! direction-major: every frame of direction 0, then direction 1, ...
//! Direction order is the RSI order: S, N, E, W, SE, SW, NE, NW.
//!
//! Port of `Robust.Shared/Resources/RsiLoading.cs`.

use std::collections::HashMap;
use std::f64::consts::PI;

use serde::Deserialize;

use crate::{ResPath, Resources, VellumError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RsiDirection {
    South = 0,
    North = 1,
    East = 2,
    West = 3,
    SouthEast = 4,
    SouthWest = 5,
    NorthEast = 6,
    NorthWest = 7,
}

/// Robust's world `Direction` (angle 0 = South, counter-clockwise positive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    South = 0,
    SouthEast = 1,
    East = 2,
    NorthEast = 3,
    North = 4,
    NorthWest = 5,
    West = 6,
    SouthWest = 7,
}

impl Direction {
    const ALL: [Direction; 8] = [
        Direction::South,
        Direction::SouthEast,
        Direction::East,
        Direction::NorthEast,
        Direction::North,
        Direction::NorthWest,
        Direction::West,
        Direction::SouthWest,
    ];

    /// `Angle.GetDir()`: nearest of 8 directions.
    pub fn from_angle(theta: f64) -> Direction {
        let seg = 2.0 * PI / 8.0;
        let ang = theta.rem_euclid(2.0 * PI);
        Self::ALL[(((ang + seg / 2.0) / seg).floor() as usize) % 8]
    }

    /// `Angle.GetCardinalDir()`: nearest of 4 directions.
    pub fn cardinal_from_angle(theta: f64) -> Direction {
        let seg = 2.0 * PI / 4.0;
        let ang = theta.rem_euclid(2.0 * PI);
        Self::ALL[((((ang + seg / 2.0) / seg).floor() as usize) * 2) % 8]
    }

    pub fn to_angle(self) -> f64 {
        self as u8 as f64 * PI / 4.0
    }

    pub fn to_rsi(self) -> RsiDirection {
        match self {
            Direction::South => RsiDirection::South,
            Direction::North => RsiDirection::North,
            Direction::East => RsiDirection::East,
            Direction::West => RsiDirection::West,
            Direction::SouthEast => RsiDirection::SouthEast,
            Direction::SouthWest => RsiDirection::SouthWest,
            Direction::NorthEast => RsiDirection::NorthEast,
            Direction::NorthWest => RsiDirection::NorthWest,
        }
    }
}

/// The direction a state with `dirs` directions should show for a world angle.
pub fn rsi_direction_for(dirs: u8, theta: f64) -> RsiDirection {
    match dirs {
        4 => Direction::cardinal_from_angle(theta).to_rsi(),
        8 => Direction::from_angle(theta).to_rsi(),
        _ => RsiDirection::South,
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub struct RsiSize {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct RsiStateJson {
    name: String,
    #[serde(default)]
    directions: Option<u8>,
    #[serde(default)]
    delays: Option<Vec<Vec<f32>>>,
}

#[derive(Debug, Clone, Deserialize)]
struct RsiJson {
    #[serde(default)]
    version: Option<u32>,
    size: RsiSize,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    copyright: Option<String>,
    states: Vec<RsiStateJson>,
}

#[derive(Debug, Clone)]
pub struct RsiState {
    pub name: String,
    /// 1, 4 or 8.
    pub directions: u8,
    /// `delays[dir][frame]` in seconds. Always `directions` entries, each non-empty.
    pub delays: Vec<Vec<f32>>,
}

impl RsiState {
    pub fn frame_count(&self) -> usize {
        self.delays.first().map_or(1, Vec::len)
    }

    pub fn is_animated(&self) -> bool {
        self.frame_count() > 1
    }

    /// Index of `(dir, frame)` within the state PNG's frame sequence.
    pub fn frame_index(&self, dir: RsiDirection, frame: usize) -> usize {
        let dir = (dir as usize).min(self.directions as usize - 1);
        let before: usize = self.delays[..dir].iter().map(Vec::len).sum();
        before + frame.min(self.delays[dir].len() - 1)
    }

    /// Which frame is showing `t` seconds into a looping animation.
    pub fn frame_at(&self, dir: RsiDirection, t: f32) -> usize {
        let dir = (dir as usize).min(self.directions as usize - 1);
        let delays = &self.delays[dir];
        let total: f32 = delays.iter().sum();
        if delays.len() <= 1 || total <= 0.0 {
            return 0;
        }
        let mut t = t.rem_euclid(total);
        for (i, d) in delays.iter().enumerate() {
            if t < *d {
                return i;
            }
            t -= d;
        }
        delays.len() - 1
    }
}

#[derive(Debug, Clone)]
pub struct Rsi {
    pub path: ResPath,
    pub size: RsiSize,
    pub license: Option<String>,
    pub copyright: Option<String>,
    pub states: Vec<RsiState>,
    index: HashMap<String, usize>,
}

impl Rsi {
    pub fn parse(path: ResPath, json: &str) -> Result<Rsi, VellumError> {
        let err = |msg: String| VellumError::Rsi {
            path: path.clone(),
            msg,
        };
        let json = json.trim_start_matches('\u{feff}');
        // Go through `Value` first: System.Text.Json tolerates duplicate keys
        // (last wins) and at least one upstream RSI relies on that.
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| err(e.to_string()))?;
        let raw: RsiJson = serde_json::from_value(value).map_err(|e| err(e.to_string()))?;
        if let Some(v) = raw.version
            && v != 1
        {
            return Err(err(format!("unsupported RSI version {v}")));
        }
        let mut states = Vec::with_capacity(raw.states.len());
        let mut index = HashMap::new();
        for s in raw.states {
            let directions = s.directions.unwrap_or(1);
            if !matches!(directions, 1 | 4 | 8) {
                return Err(err(format!("state '{}' has {directions} directions", s.name)));
            }
            let delays = match s.delays {
                Some(mut d) => {
                    if d.len() != directions as usize {
                        return Err(err(format!(
                            "state '{}' has {} delay lists for {directions} directions",
                            s.name,
                            d.len()
                        )));
                    }
                    for l in &mut d {
                        if l.is_empty() {
                            l.push(1.0);
                        }
                    }
                    d
                }
                None => vec![vec![1.0]; directions as usize],
            };
            index.insert(s.name.clone(), states.len());
            states.push(RsiState {
                name: s.name,
                directions,
                delays,
            });
        }
        Ok(Rsi {
            path,
            size: raw.size,
            license: raw.license,
            copyright: raw.copyright,
            states,
            index,
        })
    }

    pub fn load(res: &Resources, rsi_dir: &ResPath) -> Result<Rsi, VellumError> {
        let json = res.read_string(&rsi_dir.join("meta.json"))?;
        Rsi::parse(rsi_dir.clone(), &json)
    }

    pub fn state(&self, name: &str) -> Option<&RsiState> {
        self.index.get(name).map(|&i| &self.states[i])
    }

    /// The PNG for a state: `<rsi>/<state>.png`.
    pub fn state_png(&self, name: &str) -> ResPath {
        self.path.join(format!("{name}.png"))
    }

    /// Pixel rect `(x, y, w, h)` of frame `frame_index` inside a state PNG
    /// that is `image_width` pixels wide.
    pub fn frame_rect(&self, image_width: u32, frame_index: usize) -> (u32, u32, u32, u32) {
        let cols = (image_width / self.size.x).max(1) as usize;
        let col = (frame_index % cols) as u32;
        let row = (frame_index / cols) as u32;
        (col * self.size.x, row * self.size.y, self.size.x, self.size.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARP: &str = r#"{
      "version": 1, "size": {"x": 32, "y": 32}, "license": "CC-BY-SA-3.0",
      "states": [
        {"name": "icon"},
        {"name": "alive", "directions": 4,
         "delays": [[0.2,0.2,0.2],[0.2,0.2,0.2],[0.2,0.2,0.2],[0.2,0.2,0.2]]}
      ]}"#;

    #[test]
    fn parses_and_indexes_frames() {
        let rsi = Rsi::parse(ResPath::new("/Textures/carp.rsi"), CARP).unwrap();
        let alive = rsi.state("alive").unwrap();
        assert_eq!(alive.directions, 4);
        assert_eq!(alive.frame_count(), 3);
        assert_eq!(alive.frame_index(RsiDirection::South, 0), 0);
        assert_eq!(alive.frame_index(RsiDirection::North, 1), 4);
        assert_eq!(alive.frame_index(RsiDirection::West, 2), 11);
        assert_eq!(alive.frame_at(RsiDirection::South, 0.45), 2);
        // 12 frames in a 128px-wide image -> 4 columns.
        assert_eq!(rsi.frame_rect(128, 5), (32, 32, 32, 32));
        let icon = rsi.state("icon").unwrap();
        assert_eq!(icon.frame_index(RsiDirection::East, 0), 0);
    }

    #[test]
    fn angle_to_direction() {
        assert_eq!(Direction::from_angle(0.0), Direction::South);
        assert_eq!(Direction::from_angle(PI / 2.0), Direction::East);
        assert_eq!(Direction::from_angle(PI), Direction::North);
        assert_eq!(Direction::from_angle(-PI / 2.0), Direction::West);
        assert_eq!(Direction::cardinal_from_angle(PI / 4.0 - 0.01), Direction::South);
        assert_eq!(Direction::cardinal_from_angle(PI / 4.0 + 0.01), Direction::East);
        assert_eq!(rsi_direction_for(4, PI), RsiDirection::North);
        assert_eq!(rsi_direction_for(1, PI), RsiDirection::South);
    }
}
