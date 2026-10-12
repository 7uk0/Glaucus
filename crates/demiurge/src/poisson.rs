//! `PoissonDiskSampler.SampleRectangle` (Bridson's algorithm as upstream
//! writes it: 30 candidates per active point, annulus [r, 2r)).

use athanor::rng::SimRng;
use bevy_math::Vec2;

pub const POINTS_PER_ITERATION: usize = 30;

pub fn sample_rectangle(top_left: Vec2, lower_right: Vec2, min_dist: f32, rng: &mut SimRng) -> Vec<Vec2> {
    assert!(min_dist > 0.0);
    let dims = lower_right - top_left;
    let cell = min_dist / std::f32::consts::SQRT_2;
    let gw = (dims.x / cell) as usize + 1;
    let gh = (dims.y / cell) as usize + 1;
    let mut grid: Vec<Option<Vec2>> = vec![None; gw * gh];
    let index = |p: Vec2| (((p.x - top_left.x) / cell) as usize, ((p.y - top_left.y) / cell) as usize);
    let mut active: Vec<Vec2> = Vec::new();
    let mut out = Vec::new();

    let first = Vec2::new(top_left.x + dims.x * rng.next_f32(), top_left.y + dims.y * rng.next_f32());
    let (ix, iy) = index(first);
    grid[iy.min(gh - 1) * gw + ix.min(gw - 1)] = Some(first);
    active.push(first);
    out.push(first);

    while !active.is_empty() {
        let i = rng.below(active.len() as u32) as usize;
        let base = active[i];
        let mut found = false;
        for _ in 0..POINTS_PER_ITERATION {
            let radius = min_dist + min_dist * rng.next_f32();
            let angle = std::f32::consts::TAU * rng.next_f32();
            let q = base + Vec2::new(radius * angle.sin(), radius * angle.cos());
            if !(q.x >= top_left.x && q.x < lower_right.x && q.y > top_left.y && q.y < lower_right.y) {
                continue;
            }
            let (qx, qy) = index(q);
            let mut too_close = false;
            'scan: for gx in qx.saturating_sub(2)..(qx + 3).min(gw) {
                for gy in qy.saturating_sub(2)..(qy + 3).min(gh) {
                    if let Some(p) = grid[gy * gw + gx]
                        && (p - q).length() < min_dist
                    {
                        too_close = true;
                        break 'scan;
                    }
                }
            }
            if !too_close {
                grid[qy * gw + qx] = Some(q);
                active.push(q);
                out.push(q);
                found = true;
            }
        }
        if !found {
            active.remove(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_keep_their_distance() {
        let mut rng = SimRng(7);
        let pts = sample_rectangle(Vec2::splat(-56.0), Vec2::splat(56.0), 45.0, &mut rng);
        assert!(pts.len() >= 2 && pts.len() < 20, "{}", pts.len());
        for (i, a) in pts.iter().enumerate() {
            for b in &pts[i + 1..] {
                assert!((*a - *b).length() >= 45.0 - 1e-3);
            }
        }
    }
}
