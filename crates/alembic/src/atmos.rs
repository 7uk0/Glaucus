//! Grid atmospheres. Port of `GridAtmosphereComponent` loading
//! (`TileAtmosCollectionSerializer`), airtightness (`AirtightComponent`,
//! doors' `ChangeAirtight`), and LINDA (`AtmosphereSystem.LINDA`:
//! `ProcessCell`, `Share`, `TemperatureShare`). Space is an immutable vacuum
//! sink: open a door to it and the room bleeds out.
//!
//! Monstermos (upstream's explosive-equalization pass) is not ported; LINDA
//! alone decompresses rooms more gradually than upstream does.

use std::collections::{HashMap, HashSet};

use athanor::doors::Door;
use athanor::spawn::EntityData;
use athanor::transform::{tile_of, Grid, Xform};
use bevy_ecs::prelude::*;
use bevy_math::{IVec2, Vec2};
use grimoire::Node;

use crate::gas::{consts::*, GasMixture, GasTable, NUM_GASES};

/// `AtmosDirection` bits.
pub const NORTH: u8 = 1;
pub const SOUTH: u8 = 2;
pub const EAST: u8 = 4;
pub const WEST: u8 = 8;
pub const ALL: u8 = 15;

pub const DIRS: [(u8, IVec2); 4] = [
    (NORTH, IVec2::new(0, 1)),
    (SOUTH, IVec2::new(0, -1)),
    (EAST, IVec2::new(1, 0)),
    (WEST, IVec2::new(-1, 0)),
];

pub fn opposite(d: u8) -> u8 {
    match d {
        NORTH => SOUTH,
        SOUTH => NORTH,
        EAST => WEST,
        WEST => EAST,
        _ => 0,
    }
}

/// Rotate direction bits by an entity's rotation (counter-clockwise quarter
/// turns): S→E→N→W.
pub fn rotate_dirs(bits: u8, rot: f32) -> u8 {
    let quarter = ((rot / std::f32::consts::FRAC_PI_2).round() as i32).rem_euclid(4);
    let mut b = bits;
    for _ in 0..quarter {
        let mut n = 0;
        if b & SOUTH != 0 {
            n |= EAST;
        }
        if b & EAST != 0 {
            n |= NORTH;
        }
        if b & NORTH != 0 {
            n |= WEST;
        }
        if b & WEST != 0 {
            n |= SOUTH;
        }
        b = n;
    }
    b
}

#[derive(Component, Debug, Default)]
pub struct GridAtmos {
    pub tiles: HashMap<IVec2, GasMixture>,
    pub active: HashSet<IVec2>,
    /// Airtight directions per tile from static things (walls, windows).
    pub static_blocked: HashMap<IVec2, u8>,
    /// Including doors, recomputed each atmos tick.
    pub blocked: HashMap<IVec2, u8>,
    pub static_revision: u64,
    /// Pressure gradient per tile this cycle (direction × kPa).
    pub wind: HashMap<IVec2, Vec2>,
}

impl GridAtmos {
    /// The air on a tile; tiles without a mixture are open space.
    pub fn mix_at(&self, tile: IVec2) -> GasMixture {
        self.tiles.get(&tile).cloned().unwrap_or_else(GasMixture::vacuum)
    }

    fn open(&self, a: IVec2, d: u8) -> bool {
        let n = a + dir_vec(d);
        self.blocked.get(&a).is_none_or(|b| b & d == 0) && self.blocked.get(&n).is_none_or(|b| b & opposite(d) == 0)
    }
}

fn dir_vec(d: u8) -> IVec2 {
    DIRS.iter().find(|(b, _)| *b == d).map(|(_, v)| *v).unwrap_or(IVec2::ZERO)
}

/// Decode a `GridAtmosphere` component (versions 1 and 2).
pub fn decode_grid_atmosphere(node: &Node) -> HashMap<IVec2, GasMixture> {
    let mut out = HashMap::new();
    let version = node.get("version").and_then(Node::as_i64).unwrap_or(1);
    if version == 1 {
        let unique: Vec<GasMixture> = node.get("uniqueMixes").and_then(Node::as_seq).unwrap_or_default().iter().map(GasMixture::from_node).collect();
        if let Some(t) = node.get("tiles").and_then(Node::as_map) {
            for (k, v) in t {
                if let (Some((x, y)), Some(i)) = (grimoire::node::parse_vec2(k), v.as_i64())
                    && let Some(m) = unique.get(i as usize)
                {
                    out.insert(IVec2::new(x as i32, y as i32), m.clone());
                }
            }
        }
        return out;
    }
    let Some(data) = node.get("data") else { return out };
    let chunk = data.get("chunkSize").and_then(Node::as_i64).unwrap_or(4) as i32;
    let unique: Vec<GasMixture> = data.get("uniqueMixes").and_then(Node::as_seq).unwrap_or_default().iter().map(GasMixture::from_node).collect();
    let Some(tiles) = data.get("tiles").and_then(Node::as_map) else { return out };
    for (origin, mixes) in tiles {
        let Some((cx, cy)) = grimoire::node::parse_vec2(origin) else { continue };
        let Some(mixes) = mixes.as_map() else { continue };
        for (mix_idx, bits) in mixes {
            let (Ok(mi), Some(bits)) = (mix_idx.parse::<usize>(), bits.as_i64()) else { continue };
            let Some(mix) = unique.get(mi) else { continue };
            for y in 0..chunk {
                for x in 0..chunk {
                    if bits as u64 & (1u64 << (x + y * chunk)) != 0 {
                        out.insert(IVec2::new(x + cx as i32 * chunk, y + cy as i32 * chunk), mix.clone());
                    }
                }
            }
        }
    }
    out
}

/// Airtight bits contributed by an entity (`AirtightComponent`).
fn airtight_bits(data: &EntityData, rot: f32) -> u8 {
    let Some(a) = data.get("Airtight") else { return 0 };
    if !a.get("airBlocked").and_then(Node::as_bool).unwrap_or(true) {
        return 0;
    }
    let dirs = a.get("airBlockedDirection").map(Node::as_str_list).unwrap_or_default();
    let mut bits = if dirs.is_empty() { ALL } else { 0 };
    for d in dirs {
        bits |= match d.as_str() {
            "North" => NORTH,
            "South" => SOUTH,
            "East" => EAST,
            "West" => WEST,
            "All" => ALL,
            _ => 0,
        };
    }
    let fix = a.get("fixAirBlockedDirectionInitialize").and_then(Node::as_bool).unwrap_or(true);
    if fix && bits != ALL { rotate_dirs(bits, rot) } else { bits }
}

/// Give every grid that has a saved `GridAtmosphere` a live one.
pub fn init_grid_atmos(mut commands: Commands, grids: Query<(Entity, &Grid, &EntityData), Without<GridAtmos>>) {
    for (e, grid, data) in grids.iter() {
        let Some(node) = data.get("GridAtmosphere") else { continue };
        let mut tiles = decode_grid_atmosphere(node);
        // Floor tiles with no saved air start as (mutable) vacuum.
        for ((x, y), _) in grid.tiles.iter() {
            tiles.entry(IVec2::new(x, y)).or_insert_with(GasMixture::empty);
        }
        let active = tiles.keys().copied().collect();
        commands.entity(e).insert(GridAtmos {
            tiles,
            active,
            static_revision: u64::MAX,
            ..Default::default()
        });
    }
}

/// `CompareExchange`: is there anything to share between two mixtures?
fn should_exchange(a: &GasMixture, b: &GasMixture) -> bool {
    for i in 0..NUM_GASES {
        if (a.moles[i] - b.moles[i]).abs() > MINIMUM_MOLES_DELTA_TO_MOVE {
            return true;
        }
    }
    let total = a.total_moles().max(b.total_moles());
    total > MINIMUM_AIR_TO_SUSPEND && (a.temperature - b.temperature).abs() > MINIMUM_TEMPERATURE_DELTA_TO_SUSPEND
}

/// `AtmosphereSystem.Share`. Returns the pressure difference (receiver − sharer) after.
pub fn share(receiver: &mut GasMixture, rx_arch: &GasMixture, sharer: &mut GasMixture, sh_arch: &GasMixture, adjacent: usize, t: &GasTable) -> f32 {
    let temp_delta = rx_arch.temperature - sh_arch.temperature;
    let abs_td = temp_delta.abs();
    let (mut old_c, mut old_sc) = (0.0, 0.0);
    if abs_td > MINIMUM_TEMPERATURE_DELTA_TO_CONSIDER {
        old_c = receiver.heat_capacity(t);
        old_sc = sharer.heat_capacity(t);
    }
    let mut c_to_sharer = 0.0;
    let mut c_sharer_to_this = 0.0;
    for i in 0..NUM_GASES {
        let delta = (receiver.moles[i] - sharer.moles[i]) / (adjacent as f32 + 1.0);
        if delta.abs() < GAS_MIN_MOLES {
            continue;
        }
        if abs_td > MINIMUM_TEMPERATURE_DELTA_TO_CONSIDER {
            let gc = delta * t.specific_heats[i];
            if delta > 0.0 {
                c_to_sharer += gc;
            } else {
                c_sharer_to_this -= gc;
            }
        }
        if !receiver.immutable {
            receiver.moles[i] -= delta;
        }
        if !sharer.immutable {
            sharer.moles[i] += delta;
        }
    }
    if abs_td > MINIMUM_TEMPERATURE_DELTA_TO_CONSIDER {
        let new_c = old_c + c_sharer_to_this - c_to_sharer;
        let new_sc = old_sc + c_to_sharer - c_sharer_to_this;
        if !receiver.immutable && new_c > MINIMUM_HEAT_CAPACITY {
            receiver.temperature = ((old_c * receiver.temperature) - (c_to_sharer * rx_arch.temperature) + (c_sharer_to_this * sh_arch.temperature)) / new_c;
        }
        if !sharer.immutable && new_sc > MINIMUM_HEAT_CAPACITY {
            sharer.temperature = ((old_sc * sharer.temperature) - (c_sharer_to_this * sh_arch.temperature) + (c_to_sharer * rx_arch.temperature)) / new_sc;
        }
        if old_sc.abs() > MINIMUM_HEAT_CAPACITY && (new_sc / old_sc - 1.0).abs() < 0.1 {
            temperature_share(receiver, rx_arch, sharer, sh_arch, OPEN_HEAT_TRANSFER_COEFFICIENT, t);
        }
    }
    receiver.pressure() - sharer.pressure()
}

/// `AtmosphereSystem.TemperatureShare`.
pub fn temperature_share(receiver: &mut GasMixture, rx_arch: &GasMixture, sharer: &mut GasMixture, sh_arch: &GasMixture, coeff: f32, t: &GasTable) {
    let td = rx_arch.temperature - sh_arch.temperature;
    if td.abs() <= MINIMUM_TEMPERATURE_DELTA_TO_CONSIDER {
        return;
    }
    let c = rx_arch.heat_capacity(t);
    let sc = sh_arch.heat_capacity(t);
    if sc > MINIMUM_HEAT_CAPACITY && c > MINIMUM_HEAT_CAPACITY {
        let heat = coeff * td * (c * sc / (c + sc));
        if !receiver.immutable {
            receiver.temperature = (receiver.temperature - heat / c).max(TCMB).abs();
        }
        if !sharer.immutable {
            sharer.temperature = (sharer.temperature + heat / sc).max(TCMB).abs();
        }
    }
}

#[derive(Resource)]
pub struct Gases(pub GasTable);

/// Recompute airtightness: static (walls/windows) when the grid's anchors
/// change, doors every tick. Tiles whose blocking changed are woken.
pub fn update_airtight(world: &mut World) {
    let grids: Vec<(Entity, u64, u64)> = {
        let mut q = world.query::<(Entity, &Grid, &GridAtmos)>();
        q.iter(world).map(|(e, g, a)| (e, g.revision, a.static_revision)).collect()
    };
    for (ge, rev, srev) in grids {
        let anchors: Vec<(IVec2, Entity)> = {
            let g = world.get::<Grid>(ge).unwrap();
            g.anchors.iter().flat_map(|(t, es)| es.iter().map(move |e| (*t, *e))).collect()
        };
        if rev != srev {
            let mut sb: HashMap<IVec2, u8> = HashMap::new();
            for (t, e) in &anchors {
                if world.get::<Door>(*e).is_some() {
                    continue;
                }
                let (Some(d), Some(x)) = (world.get::<EntityData>(*e), world.get::<Xform>(*e)) else { continue };
                let b = airtight_bits(d, x.rot);
                if b != 0 {
                    *sb.entry(*t).or_insert(0) |= b;
                }
            }
            let mut ga = world.get_mut::<GridAtmos>(ge).unwrap();
            ga.static_blocked = sb;
            ga.static_revision = rev;
        }
        let mut blocked = world.get::<GridAtmos>(ge).unwrap().static_blocked.clone();
        for (t, e) in &anchors {
            let Some(door) = world.get::<Door>(*e) else { continue };
            // Doors seal while (mostly) shut, like `ChangeAirtight`; a door
            // that doesn't change airtightness (docked) stays sealed.
            if door.collidable() || !door.change_airtight {
                let (Some(d), Some(x)) = (world.get::<EntityData>(*e), world.get::<Xform>(*e)) else { continue };
                let b = airtight_bits(d, x.rot);
                if b != 0 {
                    *blocked.entry(*t).or_insert(0) |= b;
                }
            }
        }
        let mut ga = world.get_mut::<GridAtmos>(ge).unwrap();
        // Wake tiles whose airtightness changed, and their neighbours.
        let changed: Vec<IVec2> = blocked
            .iter()
            .filter(|(t, b)| ga.blocked.get(t) != Some(b))
            .map(|(t, _)| *t)
            .chain(ga.blocked.keys().filter(|t| !blocked.contains_key(t)).copied())
            .collect();
        for t in changed {
            for p in std::iter::once(t).chain(DIRS.iter().map(|(_, v)| t + *v)) {
                if ga.tiles.contains_key(&p) {
                    ga.active.insert(p);
                }
            }
        }
        ga.blocked = blocked;
    }
}

/// One LINDA cycle over every grid's active tiles.
pub fn process_atmos(world: &mut World) {
    let table = world.resource::<Gases>().0.clone();
    let grids: Vec<Entity> = {
        let mut q = world.query_filtered::<Entity, With<GridAtmos>>();
        q.iter(world).collect()
    };
    for ge in grids {
        if world.get::<GridAtmos>(ge).unwrap().active.is_empty() {
            world.get_mut::<GridAtmos>(ge).unwrap().wind.clear();
            continue;
        }
        let has_tile: HashSet<IVec2> = {
            let g = world.get::<Grid>(ge).unwrap();
            g.tiles.iter().map(|((x, y), _)| IVec2::new(x, y)).collect()
        };
        let mut ga = world.get_mut::<GridAtmos>(ge).unwrap();
        ga.wind.clear();
        let mut cycle: Vec<IVec2> = ga.active.iter().copied().collect();
        cycle.sort_by_key(|t| (t.x, t.y));
        // Archive everything this cycle touches.
        let mut archive: HashMap<IVec2, GasMixture> = HashMap::new();
        for t in &cycle {
            for p in std::iter::once(*t).chain(DIRS.iter().map(|(_, v)| *t + *v)) {
                if let Some(m) = ga.tiles.get(&p) {
                    archive.entry(p).or_insert_with(|| m.clone());
                }
            }
        }
        let vacuum = GasMixture::vacuum();
        let mut processed: HashSet<IVec2> = HashSet::new();
        let mut wake = Vec::new();
        let mut sleep = Vec::new();
        for t in cycle {
            if !ga.tiles.contains_key(&t) || ga.blocked.get(&t) == Some(&ALL) {
                sleep.push(t);
                continue;
            }
            let open: Vec<u8> = DIRS.iter().map(|(d, _)| *d).filter(|d| ga.open(t, *d)).collect();
            let adjacent = open.len();
            let mut exchanged = false;
            for d in open {
                let n = t + dir_vec(d);
                if processed.contains(&n) {
                    continue;
                }
                let rx_arch = archive[&t].clone();
                if !has_tile.contains(&n) || !ga.tiles.contains_key(&n) {
                    // Open to space: vacuum sink.
                    let mut rx = ga.tiles[&t].clone();
                    let mut sink = vacuum.clone();
                    if should_exchange(&rx, &sink) {
                        let diff = share(&mut rx, &rx_arch, &mut sink, &vacuum, adjacent, &table);
                        ga.tiles.insert(t, rx);
                        record_wind(&mut ga.wind, t, d, diff);
                        exchanged = true;
                    }
                    continue;
                }
                let sh_arch = archive.get(&n).cloned().unwrap_or_else(|| ga.tiles[&n].clone());
                let mut rx = ga.tiles[&t].clone();
                let mut sh = ga.tiles[&n].clone();
                if !should_exchange(&rx, &sh) {
                    continue;
                }
                let diff = share(&mut rx, &rx_arch, &mut sh, &sh_arch, adjacent, &table);
                ga.tiles.insert(t, rx);
                ga.tiles.insert(n, sh);
                if diff >= 0.0 {
                    record_wind(&mut ga.wind, t, d, diff);
                } else {
                    record_wind(&mut ga.wind, n, opposite(d), -diff);
                }
                wake.push(n);
                exchanged = true;
            }
            processed.insert(t);
            if !exchanged {
                sleep.push(t);
            }
        }
        for t in sleep {
            ga.active.remove(&t);
        }
        for t in wake {
            ga.active.insert(t);
        }
    }
}

fn record_wind(wind: &mut HashMap<IVec2, Vec2>, t: IVec2, d: u8, diff: f32) {
    let v = dir_vec(d).as_vec2() * diff;
    let e = wind.entry(t).or_insert(Vec2::ZERO);
    if v.length() > e.length() {
        *e = v;
    }
}

/// The grid and tile whose air an entity is in (`GetContainingMixture`):
/// things inside containers use their outermost holder's tile. `None` off-grid.
pub fn containing_tile(world: &World, e: Entity) -> Option<(Entity, IVec2)> {
    let mut cur = e;
    for _ in 0..8 {
        match world.get::<athanor::interaction::InContainer>(cur) {
            Some(c) => cur = c.owner,
            None => break,
        }
    }
    let x = world.get::<Xform>(cur)?;
    let parent = x.parent?;
    world.get::<GridAtmos>(parent)?;
    Some((parent, tile_of(x.pos)))
}

/// The air an entity is standing in (vacuum in open space).
pub fn air_at(world: &World, e: Entity) -> GasMixture {
    containing_tile(world, e)
        .and_then(|(g, t)| world.get::<GridAtmos>(g).map(|a| a.mix_at(t)))
        .unwrap_or_else(GasMixture::vacuum)
}

/// Space wind: loose things are shoved down steep pressure gradients
/// (`MovedByPressure`, simplified).
pub fn space_wind(world: &mut World) {
    let movers: Vec<(Entity, Xform, bool)> = {
        let mut q = world.query::<(Entity, &Xform, &EntityData, Option<&athanor::movement::InputMover>)>();
        q.iter(world)
            .filter(|(_, x, d, _)| !x.anchored && d.has("MovedByPressure"))
            .map(|(e, x, _, m)| (e, *x, m.is_some()))
            .collect()
    };
    for (e, x, is_mob) in movers {
        if world.get::<athanor::interaction::InContainer>(e).is_some() {
            continue;
        }
        let Some(parent) = x.parent else { continue };
        let Some(ga) = world.get::<GridAtmos>(parent) else { continue };
        let Some(w) = ga.wind.get(&tile_of(x.pos)).copied() else { continue };
        let kpa = w.length();
        let threshold = if is_mob { 30.0 } else { 8.0 };
        if kpa < threshold {
            continue;
        }
        let push = w.normalize_or_zero() * ((kpa - threshold) / 15.0).min(if is_mob { 3.0 } else { 6.0 });
        if let Some(mut b) = world.get_mut::<athanor::physics::Body>(e) {
            b.velocity += push;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_directions() {
        assert_eq!(rotate_dirs(SOUTH, std::f32::consts::FRAC_PI_2), EAST);
        assert_eq!(rotate_dirs(SOUTH, std::f32::consts::PI), NORTH);
        assert_eq!(rotate_dirs(SOUTH | EAST, -std::f32::consts::FRAC_PI_2), WEST | SOUTH);
    }

    #[test]
    fn sharing_conserves_moles_and_equalizes() {
        let t = GasTable {
            specific_heats: [10.0; NUM_GASES],
            names: vec![String::new(); NUM_GASES],
        };
        let mut a = GasMixture::station_air();
        let mut b = GasMixture::empty();
        let total = a.total_moles() + b.total_moles();
        for _ in 0..50 {
            let (aa, ba) = (a.clone(), b.clone());
            share(&mut a, &aa, &mut b, &ba, 1, &t);
        }
        assert!((a.total_moles() + b.total_moles() - total).abs() < 1e-2);
        assert!((a.pressure() - b.pressure()).abs() < 1.0, "{} vs {}", a.pressure(), b.pressure());
    }
}
