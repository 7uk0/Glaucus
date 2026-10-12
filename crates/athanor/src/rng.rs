//! Deterministic simulation randomness (SplitMix64), so a seed reproduces a
//! whole run — useful for tests and, later, replays.

use bevy_ecs::prelude::*;

#[derive(Resource, Debug, Clone)]
pub struct SimRng(pub u64);

impl Default for SimRng {
    fn default() -> Self {
        SimRng(0x5EED_CA9D_0D7E_u64)
    }
}

impl SimRng {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }

    /// Inclusive range.
    pub fn range_u32(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (hi - lo + 1) as u64) as u32
    }

    /// `IRobustRandom.Next(n)`: uniform in `[0, n)` (0 when n is 0).
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % n as u64) as u32
    }

    /// `IRobustRandom.Next(min, max)`: uniform in `[min, max)`.
    pub fn next_range(&mut self, min: i32, max: i32) -> i32 {
        if max <= min {
            return min;
        }
        min + self.below((max - min) as u32) as i32
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(&items[(self.next_u64() % items.len() as u64) as usize])
        }
    }
}

pub fn range_u32(world: &mut World, lo: u32, hi: u32) -> u32 {
    world.get_resource_or_init::<SimRng>().range_u32(lo, hi)
}

pub fn chance(world: &mut World, p: f32) -> bool {
    world.get_resource_or_init::<SimRng>().chance(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_in_range() {
        let mut a = SimRng(42);
        let mut b = SimRng(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
            let r = a.range_u32(3, 5);
            assert!((3..=5).contains(&r));
            let f = a.next_f32();
            assert!((0.0..1.0).contains(&f));
            b.range_u32(3, 5);
            b.next_f32();
        }
    }
}
