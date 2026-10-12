//! Frontier's sector layout: `NfAdventureRuleSystem.Started` (sorting
//! `pointOfInterest` prototypes into spawn groups) and
//! `Content.Server/_NF/GameRule/PointOfInterestSystem.cs` (placing them).
//!
//! The outpost sits at the origin. Around it, in this order:
//!
//! 1. `CargoDepot`: `nf14.worldgen.cargo_depots` (4) depots, evenly spaced
//!    on a randomly rotated ring, named "… A", "… B", ….
//! 2. `MarketStation`: `nf14.worldgen.market_stations` (1), shuffled.
//! 3. `Required`: every one.
//! 4. `Optional`: `nf14.worldgen.optional_stations` (6), shuffled.
//! 5. Any other spawn group: at most one per group, each rolled against
//!    its `spawnChance` in shuffled order.
//!
//! Positions come from `NextVector2(min, max)` (random angle, uniform
//! magnitude). A position is retried up to `poi_placement_retries` (10)
//! times while it is closer than max(own, other) `minimumClearance` to an
//! already placed POI. Every POI grid gets a random rotation.

use athanor::map_loader::{load_map_path, LoadOptions};
use athanor::rng::SimRng;
use athanor::spawn::{add_components, MetaData};
use athanor::Content;
use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use grimoire::Node;

/// The `nf14.worldgen.*` CVars that shape the sector.
#[derive(Debug, Clone)]
pub struct PoiConfig {
    pub market_stations: usize,
    pub cargo_depots: usize,
    pub optional_stations: usize,
    pub placement_retries: usize,
    /// Current game preset (`NFAdventure`); POIs listing other presets are skipped.
    pub preset: String,
}

impl Default for PoiConfig {
    fn default() -> Self {
        PoiConfig {
            market_stations: 1,
            cargo_depots: 4,
            optional_stations: 6,
            placement_retries: 10,
            preset: "NFAdventure".into(),
        }
    }
}

/// `PointOfInterestPrototype`.
#[derive(Debug, Clone)]
pub struct PoiProto {
    pub id: String,
    pub name: String,
    pub spawn_group: String,
    pub min_distance: i32,
    pub max_distance: i32,
    pub min_clearance: i32,
    pub spawn_presets: Vec<String>,
    pub spawn_chance: f32,
    pub grid_path: String,
    pub add_components: Vec<Node>,
}

impl PoiProto {
    fn from_node(id: &str, n: &Node) -> Option<PoiProto> {
        Some(PoiProto {
            id: id.to_string(),
            name: n.get_str("name").unwrap_or(id).to_string(),
            spawn_group: n.get_str("spawnGroup").unwrap_or("Optional").to_string(),
            min_distance: n.get("minimumDistance").and_then(Node::as_i64).unwrap_or(5000) as i32,
            max_distance: n.get("maximumDistance").and_then(Node::as_i64).unwrap_or(10000) as i32,
            min_clearance: n.get("minimumClearance").and_then(Node::as_i64).unwrap_or(400) as i32,
            spawn_presets: n.get("spawnGamePreset").map(Node::as_str_list).unwrap_or_default(),
            spawn_chance: n.get("spawnChance").and_then(Node::as_f32).unwrap_or(1.0),
            grid_path: n.get_str("gridPath")?.to_string(),
            add_components: n.get("addComponents").and_then(Node::as_seq).map(|s| s.to_vec()).unwrap_or_default(),
        })
    }

    fn allowed(&self, preset: &str) -> bool {
        self.spawn_presets.is_empty() || self.spawn_presets.iter().any(|p| p == preset)
    }
}

/// A placed point of interest.
#[derive(Debug, Clone)]
pub struct PlacedPoi {
    pub proto: String,
    pub name: String,
    pub group: String,
    pub grid: Entity,
    pub pos: Vec2,
    /// `TradeCrateDestinationComponent.DestinationProto` for depots.
    pub cargo_destination: Option<String>,
}

/// Marks a POI grid (the `NFAdventureRuleComponent` lists, per entity).
#[derive(Component, Debug, Clone)]
pub struct PointOfInterest {
    pub proto: String,
    pub group: String,
}

/// All non-abstract `pointOfInterest` prototypes, in id order (upstream
/// enumerates its prototype dictionary; the order only feeds shuffles).
pub fn poi_protos(content: &Content) -> Vec<PoiProto> {
    let mut v: Vec<PoiProto> = content
        .protos
        .iter_kind("pointOfInterest")
        .filter(|p| !p.is_abstract)
        .filter_map(|p| PoiProto::from_node(&p.id, &Node::Map(p.data.clone())))
        .collect();
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

/// `NextFloat(min, max)` / `NextAngle` / `NextVector2(min, max)` on our RNG.
fn next_vector2(rng: &mut SimRng, min: f32, max: f32) -> Vec2 {
    let angle = rng.next_f32() * std::f32::consts::TAU;
    let mag = min + (max - min) * rng.next_f32();
    let (s, c) = angle.sin_cos();
    Vec2::new(c * mag, s * mag)
}

fn shuffle<T>(rng: &mut SimRng, v: &mut [T]) {
    // `IRobustRandom.Shuffle`: Fisher–Yates from the back.
    let mut n = v.len();
    while n > 1 {
        n -= 1;
        let k = rng.below(n as u32 + 1) as usize;
        v.swap(k, n);
    }
}

struct Placer<'a> {
    world: &'a mut World,
    map: Entity,
    rng: SimRng,
    retries: usize,
    taken: Vec<(Vec2, f32)>,
    out: Vec<PlacedPoi>,
}

impl Placer<'_> {
    /// `GetRandomPOICoord`.
    fn random_coord(&mut self, min: f32, max: f32, clearance: f32) -> Vec2 {
        let mut coords = next_vector2(&mut self.rng, min, max);
        for _ in 0..self.retries {
            let valid = self.taken.iter().all(|(p, c)| p.distance(coords) >= clearance.max(*c));
            if valid {
                break;
            }
            coords = next_vector2(&mut self.rng, min, max);
        }
        coords
    }

    /// `TrySpawnPoiGrid`.
    fn spawn(&mut self, proto: &PoiProto, offset: Vec2, name: Option<String>) -> Option<Entity> {
        let rotation = self.rng.next_f32() * std::f32::consts::TAU;
        let loaded = match load_map_path(
            self.world,
            &proto.grid_path,
            Some(self.map),
            LoadOptions { offset, rotation, ..Default::default() },
        ) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("POI {}: {e}", proto.id);
                return None;
            }
        };
        let grid = *loaded.grids.first()?;
        let name = name.unwrap_or_else(|| proto.name.clone());
        if let Some(mut m) = self.world.get_mut::<MetaData>(grid) {
            m.name = name.clone();
        }
        add_components(self.world, grid, &proto.add_components);
        self.world.entity_mut(grid).insert(PointOfInterest { proto: proto.id.clone(), group: proto.spawn_group.clone() });
        self.taken.push((offset, proto.min_clearance as f32));
        self.out.push(PlacedPoi {
            proto: proto.id.clone(),
            name,
            group: proto.spawn_group.clone(),
            grid,
            pos: offset,
            cargo_destination: None,
        });
        Some(grid)
    }
}

/// `NfAdventureRuleSystem.Started` → `PointOfInterestSystem.Generate*`.
pub fn generate_pois(world: &mut World, map: Entity, rng: SimRng, cfg: &PoiConfig) -> Vec<PlacedPoi> {
    let content = world.resource::<Content>().clone();
    let (mut depots, mut markets, mut requireds, mut optionals) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut uniques: Vec<(String, Vec<PoiProto>)> = Vec::new();
    for p in poi_protos(&content) {
        if !p.allowed(&cfg.preset) {
            continue;
        }
        match p.spawn_group.as_str() {
            "CargoDepot" => depots.push(p),
            "MarketStation" => markets.push(p),
            "Required" => requireds.push(p),
            "Optional" => optionals.push(p),
            g => match uniques.iter_mut().find(|(k, _)| k == g) {
                Some((_, v)) => v.push(p),
                None => uniques.push((g.to_string(), vec![p])),
            },
        }
    }
    let mut pl = Placer { world, map, rng, retries: cfg.placement_retries, taken: Vec::new(), out: Vec::new() };

    // GenerateDepots.
    if cfg.cargo_depots > 0 {
        let rotation = std::f32::consts::TAU / cfg.cargo_depots as f32;
        let mut rotation_offset = pl.rng.next_f32() * std::f32::consts::TAU / cfg.cargo_depots as f32;
        for i in 0..cfg.cargo_depots {
            if depots.is_empty() {
                break;
            }
            let proto = depots[pl.rng.below(depots.len() as u32) as usize].clone();
            let r = pl.rng.next_range(proto.min_distance, proto.max_distance) as f32;
            // `Vector2i.Rotate`: rotate, then truncate to integers.
            let (s, c) = rotation_offset.sin_cos();
            let offset = Vec2::new((c * r).trunc(), (s * r).trunc());
            rotation_offset += rotation;
            let suffix = if i < 26 { ((b'A' + i as u8) as char).to_string() } else { (i + 1).to_string() };
            let name = format!("{} {suffix}", proto.name);
            if pl.spawn(&proto, offset, Some(name)).is_some()
                && let Some(last) = pl.out.last_mut()
            {
                last.cargo_destination = Some(if i < 26 { format!("Cargo{suffix}") } else { "CargoOther".into() });
            }
        }
    }
    // GenerateMarkets.
    shuffle(&mut pl.rng, &mut markets);
    let mut added = 0;
    for proto in &markets {
        if added >= cfg.market_stations {
            break;
        }
        let off = pl.random_coord(proto.min_distance as f32, proto.max_distance as f32, proto.min_clearance as f32);
        if pl.spawn(proto, off, None).is_some() {
            added += 1;
        }
    }
    // GenerateRequireds.
    for proto in &requireds {
        let off = pl.random_coord(proto.min_distance as f32, proto.max_distance as f32, proto.min_clearance as f32);
        pl.spawn(proto, off, None);
    }
    // GenerateOptionals. Upstream never increments its counter, so every
    // optional POI spawns; `optional_stations` only matters if that's fixed.
    shuffle(&mut pl.rng, &mut optionals);
    for proto in &optionals {
        let off = pl.random_coord(proto.min_distance as f32, proto.max_distance as f32, proto.min_clearance as f32);
        pl.spawn(proto, off, None);
    }
    // GenerateUniques.
    for (_, list) in &mut uniques {
        shuffle(&mut pl.rng, list);
        for proto in list.iter() {
            if pl.rng.next_f32() <= proto.spawn_chance {
                let off = pl.random_coord(proto.min_distance as f32, proto.max_distance as f32, proto.min_clearance as f32);
                if pl.spawn(proto, off, None).is_some() {
                    break;
                }
            }
        }
    }
    pl.out
}
