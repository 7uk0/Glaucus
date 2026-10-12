//! Noise channels: `Content.Server/Worldgen/Prototypes/NoiseChannelPrototype.cs`
//! (`NoiseChannelConfig` + `NoiseGenerator`) over FastNoiseLite, the same
//! library upstream uses (Robust ships the C# port; this is the official Rust one).

use fastnoise_lite::{FastNoiseLite, FractalType, NoiseType};
use grimoire::{Node, PrototypeManager};

#[derive(Debug, Clone)]
pub struct NoiseChannel {
    pub noise_type: NoiseType,
    pub fractal_type: FractalType,
    pub fractal_lacunarity_by_pi: f32,
    pub clipping_ranges: Vec<(f32, f32)>,
    pub clipped_value: f32,
    pub output_multiplier: f32,
    pub input_multiplier: f32,
    pub remap_to_0_through_1: bool,
    pub minimum: f32,
}

fn noise_type(s: &str) -> NoiseType {
    match s {
        "OpenSimplex2" => NoiseType::OpenSimplex2,
        "OpenSimplex2S" => NoiseType::OpenSimplex2S,
        "Perlin" => NoiseType::Perlin,
        "ValueCubic" => NoiseType::ValueCubic,
        "Value" => NoiseType::Value,
        _ => NoiseType::Cellular,
    }
}

fn fractal_type(s: &str) -> FractalType {
    match s {
        "None" => FractalType::None,
        "Ridged" => FractalType::Ridged,
        "PingPong" => FractalType::PingPong,
        "DomainWarpProgressive" => FractalType::DomainWarpProgressive,
        "DomainWarpIndependent" => FractalType::DomainWarpIndependent,
        _ => FractalType::FBm,
    }
}

impl NoiseChannel {
    pub fn from_node(n: &Node) -> NoiseChannel {
        let f = |k: &str, d: f32| n.get(k).and_then(Node::as_f32).unwrap_or(d);
        NoiseChannel {
            noise_type: noise_type(n.get_str("noiseType").unwrap_or("Cellular")),
            fractal_type: fractal_type(n.get_str("fractalType").unwrap_or("FBm")),
            fractal_lacunarity_by_pi: f("fractalLacunarityByPi", 2.0 / 3.0),
            clipping_ranges: n
                .get("clippingRanges")
                .and_then(Node::as_seq)
                .unwrap_or_default()
                .iter()
                .filter_map(|r| r.as_vec2())
                .collect(),
            clipped_value: f("clippedValue", 0.0),
            output_multiplier: f("outputMultiplier", 1.0),
            input_multiplier: f("inputMultiplier", 1.0),
            remap_to_0_through_1: n.get("remapTo0Through1").and_then(Node::as_bool).unwrap_or(false),
            minimum: f("minimum", 0.0),
        }
    }

    pub fn load(pm: &PrototypeManager, id: &str) -> Option<NoiseChannel> {
        let p = pm.get("noiseChannel", id)?;
        Some(NoiseChannel::from_node(&Node::Map(p.data.clone())))
    }
}

/// `NoiseGenerator`: one seeded FastNoiseLite configured from a channel.
#[derive(Clone)]
pub struct NoiseGen {
    cfg: NoiseChannel,
    noise: FastNoiseLite,
}

impl NoiseGen {
    pub fn new(cfg: NoiseChannel, seed: i32) -> NoiseGen {
        let mut noise = FastNoiseLite::with_seed(seed);
        noise.set_noise_type(Some(cfg.noise_type));
        noise.set_fractal_type(Some(cfg.fractal_type));
        noise.set_fractal_lacunarity(Some(cfg.fractal_lacunarity_by_pi * std::f32::consts::PI));
        NoiseGen { cfg, noise }
    }

    /// `NoiseGenerator.Evaluate`.
    pub fn evaluate(&self, x: f32, y: f32) -> f32 {
        let (x, y) = (x * self.cfg.input_multiplier, y * self.cfg.input_multiplier);
        let mut value = self.noise.get_noise_2d(x, y);
        if self.cfg.remap_to_0_through_1 {
            value = (value + 1.0) / 2.0;
        }
        for &(lo, hi) in &self.cfg.clipping_ranges {
            if lo < value && value < hi {
                value = self.cfg.clipped_value;
                break;
            }
        }
        value * self.cfg.output_multiplier + self.cfg.minimum
    }
}
