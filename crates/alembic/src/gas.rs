//! Gas mixtures. Port of `GasMixture` and the gas math in
//! `AtmosphereSystem.Gases` with Frontier's CVars (heat scale 2).

use grimoire::{Node, PrototypeManager};

/// `Atmospherics` constants.
pub mod consts {
    pub const R: f32 = 8.314_463;
    pub const ONE_ATMOSPHERE: f32 = 101.325;
    pub const TCMB: f32 = 2.7;
    pub const T20C: f32 = 293.15;
    pub const CELL_VOLUME: f32 = 2500.0;
    pub const MOLES_CELL_STANDARD: f32 = ONE_ATMOSPHERE * CELL_VOLUME / (T20C * R);
    pub const OXYGEN_STANDARD: f32 = 0.21;
    pub const NITROGEN_STANDARD: f32 = 0.79;
    pub const OPEN_HEAT_TRANSFER_COEFFICIENT: f32 = 0.4;
    pub const MINIMUM_AIR_RATIO_TO_SUSPEND: f32 = 0.1;
    pub const MINIMUM_AIR_RATIO_TO_MOVE: f32 = 0.001;
    pub const MINIMUM_AIR_TO_SUSPEND: f32 = MOLES_CELL_STANDARD * MINIMUM_AIR_RATIO_TO_SUSPEND;
    pub const MINIMUM_TEMPERATURE_TO_MOVE: f32 = T20C + 100.0;
    pub const MINIMUM_MOLES_DELTA_TO_MOVE: f32 = MOLES_CELL_STANDARD * MINIMUM_AIR_RATIO_TO_MOVE;
    pub const MINIMUM_TEMPERATURE_DELTA_TO_SUSPEND: f32 = 4.0;
    pub const MINIMUM_TEMPERATURE_DELTA_TO_CONSIDER: f32 = 0.01;
    pub const MINIMUM_HEAT_CAPACITY: f32 = 0.0003;
    pub const SPACE_HEAT_CAPACITY: f32 = 7000.0;
    pub const GAS_MIN_MOLES: f32 = 0.000_000_05;
    pub const BREATH_VOLUME: f32 = 0.5;
    pub const HAZARD_HIGH_PRESSURE: f32 = 550.0;
    pub const HAZARD_LOW_PRESSURE: f32 = 20.0;
    pub const WARNING_LOW_PRESSURE: f32 = 2.5 * HAZARD_LOW_PRESSURE;
    pub const WARNING_HIGH_PRESSURE: f32 = 0.7 * HAZARD_HIGH_PRESSURE;
    pub const PRESSURE_DAMAGE_COEFFICIENT: f32 = 4.0;
    pub const MAX_HIGH_PRESSURE_DAMAGE: f32 = 4.0;
    pub const LOW_PRESSURE_DAMAGE: f32 = 4.0;
    /// Frontier `atmos.heat_scale`.
    pub const HEAT_SCALE: f32 = 2.0;
    /// Frontier `atmos.tickrate`.
    pub const ATMOS_TICK_RATE: f32 = 15.0;
}

use consts::*;

/// Frontier's `TotalNumberOfGases`.
pub const NUM_GASES: usize = 11;

pub const OXYGEN: usize = 0;
pub const NITROGEN: usize = 1;
pub const CARBON_DIOXIDE: usize = 2;
pub const PLASMA: usize = 3;

/// The `Gas` enum, by index.
pub const GAS_NAMES: [&str; NUM_GASES] = [
    "Oxygen",
    "Nitrogen",
    "CarbonDioxide",
    "Plasma",
    "Tritium",
    "WaterVapor",
    "Ammonia",
    "NitrousOxide",
    "Frezon",
    "Respiron",
    "Helium",
];

pub fn gas_index(name: &str) -> Option<usize> {
    GAS_NAMES.iter().position(|n| *n == name)
}

#[derive(Debug, Clone)]
pub struct GasTable {
    /// Specific heats already divided by the heat scale, like upstream.
    pub specific_heats: [f32; NUM_GASES],
    pub names: Vec<String>,
}

impl GasTable {
    pub fn from_prototypes(pm: &PrototypeManager) -> GasTable {
        let mut t = GasTable {
            specific_heats: [20.0 / HEAT_SCALE; NUM_GASES],
            names: vec![String::new(); NUM_GASES],
        };
        for p in pm.iter_kind("gas") {
            let Ok(i) = p.id.parse::<usize>() else { continue };
            if i >= NUM_GASES {
                continue;
            }
            if let Some(h) = p.get("specificHeat").and_then(Node::as_f32) {
                t.specific_heats[i] = h / HEAT_SCALE;
            }
            t.names[i] = p.get_str("name").unwrap_or("").to_string();
        }
        t
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GasMixture {
    pub moles: [f32; NUM_GASES],
    pub temperature: f32,
    pub volume: f32,
    pub immutable: bool,
}

impl GasMixture {
    pub fn vacuum() -> GasMixture {
        GasMixture {
            moles: [0.0; NUM_GASES],
            temperature: TCMB,
            volume: CELL_VOLUME,
            immutable: true,
        }
    }

    /// An empty, mutable mixture.
    pub fn empty() -> GasMixture {
        GasMixture {
            immutable: false,
            ..GasMixture::vacuum()
        }
    }

    /// Standard station air at 20 °C.
    pub fn station_air() -> GasMixture {
        let mut m = GasMixture {
            moles: [0.0; NUM_GASES],
            temperature: T20C,
            volume: CELL_VOLUME,
            immutable: false,
        };
        m.moles[OXYGEN] = MOLES_CELL_STANDARD * OXYGEN_STANDARD;
        m.moles[NITROGEN] = MOLES_CELL_STANDARD * NITROGEN_STANDARD;
        m
    }

    pub fn total_moles(&self) -> f32 {
        self.moles.iter().sum()
    }

    /// kPa.
    pub fn pressure(&self) -> f32 {
        if self.volume <= 0.0 {
            return 0.0;
        }
        self.total_moles() * R * self.temperature / self.volume
    }

    pub fn partial_pressure(&self, gas: usize) -> f32 {
        if self.volume <= 0.0 {
            return 0.0;
        }
        self.moles[gas] * R * self.temperature / self.volume
    }

    pub fn heat_capacity(&self, t: &GasTable) -> f32 {
        let total = self.total_moles();
        if self.immutable && total.abs() < 1e-6 {
            return SPACE_HEAT_CAPACITY;
        }
        let c: f32 = self.moles.iter().zip(t.specific_heats.iter()).map(|(m, h)| m * h).sum();
        c.max(MINIMUM_HEAT_CAPACITY)
    }

    /// Fraction of a gas in the mix.
    pub fn fraction(&self, gas: usize) -> f32 {
        let t = self.total_moles();
        if t <= 0.0 { 0.0 } else { self.moles[gas] / t }
    }

    /// Take `ratio` of the mixture out (e.g. a breath).
    pub fn remove_ratio(&mut self, ratio: f32) -> GasMixture {
        let ratio = ratio.clamp(0.0, 1.0);
        let mut out = self.clone();
        out.immutable = false;
        for i in 0..NUM_GASES {
            out.moles[i] = self.moles[i] * ratio;
            if !self.immutable {
                self.moles[i] -= out.moles[i];
            }
        }
        out
    }

    /// `AtmosphereSystem.Merge`: add `giver` into `self`, mixing temperature.
    pub fn merge(&mut self, giver: &GasMixture, t: &GasTable) {
        if self.immutable {
            return;
        }
        let dt = (self.temperature - giver.temperature).abs();
        if dt > MINIMUM_TEMPERATURE_DELTA_TO_CONSIDER {
            let c_self = self.heat_capacity(t);
            let c_giver = giver.heat_capacity(t);
            let combined = c_self + c_giver;
            if combined > 0.0 {
                self.temperature = (giver.temperature * c_giver + self.temperature * c_self) / combined;
            }
        }
        for i in 0..NUM_GASES {
            self.moles[i] += giver.moles[i];
        }
    }

    pub fn from_node(n: &Node) -> GasMixture {
        let mut m = GasMixture {
            moles: [0.0; NUM_GASES],
            temperature: n.get("temperature").and_then(Node::as_f32).unwrap_or(T20C),
            volume: n.get("volume").and_then(Node::as_f32).unwrap_or(CELL_VOLUME),
            immutable: n.get("immutable").and_then(Node::as_bool).unwrap_or(false),
        };
        for (i, v) in n.get("moles").and_then(Node::as_seq).unwrap_or_default().iter().enumerate().take(NUM_GASES) {
            m.moles[i] = v.as_f32().unwrap_or(0.0);
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn station_air_is_one_atmosphere() {
        let a = GasMixture::station_air();
        assert!((a.pressure() - ONE_ATMOSPHERE).abs() < 0.01, "{}", a.pressure());
        assert!((a.fraction(OXYGEN) - 0.21).abs() < 1e-4);
        assert_eq!(GasMixture::vacuum().pressure(), 0.0);
    }
}
