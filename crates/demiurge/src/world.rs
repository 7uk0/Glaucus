//! Chunked space worldgen. Ports of `Content.Server/Worldgen`:
//! `WorldControllerSystem` (128 m chunks loaded around players and
//! `WorldLoader`s), `BiomeSelectionSystem`, `DebrisFeaturePlacerSystem`
//! (+ `PoissonDiskSampler`, `NoiseRangeCarver`, Frontier's
//! `PointSetDistanceCarver`, `SimpleDebrisSelector`,
//! `NoiseDrivenDebrisSelector`), `BlobFloorPlanBuilderSystem`,
//! `LocalityLoaderSystem` → `SimpleFloorPlanPopulator` / Frontier's
//! `RandomEntityPopulator`, and the `SpaceDebris` GC queue.
//!
//! Upstream gives every chunk its own randomly seeded noise generators
//! (`NoiseIndexComponent` on the chunk); here the seeds are derived from the
//! world seed, chunk and channel, so a sector regenerates identically.

use std::collections::{HashMap, HashSet, VecDeque};

use athanor::movement::InputMover;
use athanor::rng::SimRng;
use athanor::spawn::{spawn_proto, ComponentFactories, EntityData};
use athanor::transform::{make_grid, Grid, WorldPose, Xform};
use athanor::{Content, Player};
use bevy_ecs::prelude::*;
use bevy_math::{IVec2, Vec2};
use grimoire::Node;

use crate::noise::{NoiseChannel, NoiseGen};
use crate::table::SpawnCache;

pub const CHUNK_SIZE: f32 = 128.0;
/// `WorldControllerSystem.PlayerLoadRadius`, in chunks.
pub const PLAYER_LOAD_RADIUS: i32 = 2;
/// `SpaceDebris` gc queue (`Resources/Prototypes/GC/world.yml`, Frontier values).
pub const GC_DEPTH: usize = 256;
pub const GC_MIN_DEPTH_TO_PROCESS: usize = 128;

/// What a biome puts on a chunk (its `chunkComponents`).
#[derive(Debug, Clone)]
pub struct ChunkGen {
    pub density_channel: Option<String>,
    pub density_clip: bool,
    pub random_cancel: f32,
    pub safety_radius: f32,
    pub simple: Option<SpawnCache>,
    pub noise_selector: Option<(String, SpawnCache)>,
    pub noise_carvers: Vec<(String, Vec<(f32, f32)>)>,
    pub point_set_carver: bool,
}

impl ChunkGen {
    fn from_components(comps: &[Node]) -> ChunkGen {
        let mut g = ChunkGen {
            density_channel: None,
            density_clip: true,
            random_cancel: 0.35,
            safety_radius: 24.0,
            simple: None,
            noise_selector: None,
            noise_carvers: Vec::new(),
            point_set_carver: false,
        };
        for c in comps {
            match c.get_str("type") {
                Some("DebrisFeaturePlacerController") => {
                    g.density_channel = c.get_str("densityNoiseChannel").map(str::to_string);
                    g.density_clip = c.get("densityClip").and_then(Node::as_bool).unwrap_or(true);
                    g.random_cancel = c.get("randomCancelChance").and_then(Node::as_f32).unwrap_or(0.35);
                    g.safety_radius = c.get("safetyZoneRadius").and_then(Node::as_f32).unwrap_or(24.0);
                }
                Some("SimpleDebrisSelector") => g.simple = Some(SpawnCache::from_node(c.get("debrisTable"))),
                Some("NoiseDrivenDebrisSelector") => {
                    if let Some(ch) = c.get_str("noiseChannel") {
                        g.noise_selector = Some((ch.to_string(), SpawnCache::from_node(c.get("debrisTable"))));
                    }
                }
                Some("NoiseRangeCarver") => {
                    if let Some(ch) = c.get_str("noiseChannel") {
                        let ranges = c.get("ranges").and_then(Node::as_seq).unwrap_or_default().iter().filter_map(Node::as_vec2).collect();
                        g.noise_carvers.push((ch.to_string(), ranges));
                    }
                }
                Some("PointSetDistanceCarver") => g.point_set_carver = true,
                _ => {}
            }
        }
        g
    }
}

#[derive(Debug, Clone)]
pub struct Biome {
    pub id: String,
    pub priority: i64,
    pub distance_range_sq: Option<(f32, f32)>,
    pub noise_ranges: Vec<(String, Vec<(f32, f32)>)>,
    pub chunkgen: ChunkGen,
}

pub struct Chunk {
    pub coords: IVec2,
    pub biome: Option<String>,
    pub chunkgen: Option<ChunkGen>,
    noise: HashMap<String, NoiseGen>,
    pub loaded: bool,
    pub loaders: Vec<Entity>,
    /// Debris this chunk placed: (point, entity).
    pub owned: Vec<(Vec2, Entity)>,
    pub do_spawns: bool,
}

/// The world controller for the sector map (`WorldControllerComponent` +
/// `BiomeSelectionComponent` + the noise index).
#[derive(Resource)]
pub struct Cosmos {
    pub map: Entity,
    pub seed: u64,
    pub biomes: Vec<Biome>,
    pub channels: HashMap<String, NoiseChannel>,
    pub chunks: HashMap<IVec2, Chunk>,
    pub gc_queue: VecDeque<Entity>,
    pub rng: SimRng,
}

/// Debris placed by a chunk (`OwnedDebrisComponent`).
#[derive(Component, Debug, Clone, Copy)]
pub struct OwnedDebris {
    pub chunk: IVec2,
    pub point: Vec2,
}

/// `LocalityLoaderComponent`: populate when a loader comes within range.
#[derive(Component, Debug, Clone, Copy)]
pub struct LocalityLoader {
    pub loading_distance: f32,
}

/// `WorldLoaderComponent`: keeps chunks loaded around it (shuttle consoles).
#[derive(Component, Debug, Clone, Copy)]
pub struct WorldLoader {
    pub radius: f32,
    pub disabled: bool,
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("WorldLoader", |e, node, _| {
        e.insert(WorldLoader {
            radius: node.get("radius").and_then(Node::as_f32).unwrap_or(128.0),
            disabled: node.get("disabled").and_then(Node::as_bool).unwrap_or(false),
        });
    });
    reg.register("LocalityLoader", |e, node, _| {
        e.insert(LocalityLoader { loading_distance: node.get("loadingDistance").and_then(Node::as_f32).unwrap_or(32.0) });
    });
}

fn hash3(a: u64, b: i64, c: i64, s: &str) -> u64 {
    let mut h = a ^ 0x9E37_79B9_7F4A_7C15;
    for v in [b as u64, c as u64].into_iter().chain(s.bytes().map(u64::from)) {
        h ^= v.wrapping_add(0x9E37_79B9_7F4A_7C15).wrapping_add(h << 6).wrapping_add(h >> 2);
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    }
    h
}

impl Cosmos {
    /// Load the worldgen config (`worldgenConfig` → `BiomeSelection.biomes`).
    pub fn new(content: &Content, map: Entity, config: &str, seed: u64) -> Cosmos {
        let pm = &content.protos;
        let mut biomes = Vec::new();
        let biome_ids: Vec<String> = pm
            .get("worldgenConfig", config)
            .and_then(|p| p.get("components"))
            .and_then(Node::as_seq)
            .unwrap_or_default()
            .iter()
            .find(|c| c.get_str("type") == Some("BiomeSelection"))
            .and_then(|c| c.get("biomes"))
            .map(Node::as_str_list)
            .unwrap_or_default();
        for id in biome_ids {
            let Some(p) = pm.get("spaceBiome", &id) else {
                tracing::warn!("unknown spaceBiome {id}");
                continue;
            };
            let d = Node::Map(p.data.clone());
            biomes.push(Biome {
                id: id.clone(),
                priority: d.get("priority").and_then(Node::as_i64).unwrap_or(0),
                distance_range_sq: d.get("distanceRange").and_then(Node::as_vec2).map(|(a, b)| (a * a, b * b)),
                noise_ranges: d
                    .get("noiseRanges")
                    .and_then(Node::as_map)
                    .map(|m| {
                        m.iter()
                            .map(|(k, v)| (k.to_string(), v.as_seq().unwrap_or_default().iter().filter_map(Node::as_vec2).collect()))
                            .collect()
                    })
                    .unwrap_or_default(),
                chunkgen: ChunkGen::from_components(d.get("chunkComponents").and_then(Node::as_seq).unwrap_or_default()),
            });
        }
        biomes.sort_by(|a, b| b.priority.cmp(&a.priority));
        let channels = pm.iter_kind("noiseChannel").map(|p| (p.id.clone(), NoiseChannel::from_node(&Node::Map(p.data.clone())))).collect();
        Cosmos {
            map,
            seed,
            biomes,
            channels,
            chunks: HashMap::new(),
            gc_queue: VecDeque::new(),
            rng: SimRng(seed ^ 0xD1B5_4A32_D192_ED03),
        }
    }

    /// `NoiseIndexSystem.Evaluate` with a chunk as the holder.
    pub fn noise(&mut self, chunk: IVec2, channel: &str, at: Vec2) -> f32 {
        let seed = self.seed;
        let Some(cfg) = self.channels.get(channel).cloned() else { return 0.0 };
        let c = self.chunks.get_mut(&chunk).expect("chunk exists");
        let g = c
            .noise
            .entry(channel.to_string())
            .or_insert_with(|| NoiseGen::new(cfg, hash3(seed, chunk.x as i64, chunk.y as i64, channel) as i32));
        g.evaluate(at.x, at.y)
    }

    /// `GetOrCreateChunk` + `BiomeSelectionSystem.OnWorldChunkAdded`.
    pub fn chunk(&mut self, coords: IVec2) -> &mut Chunk {
        if !self.chunks.contains_key(&coords) {
            self.chunks.insert(
                coords,
                Chunk { coords, biome: None, chunkgen: None, noise: HashMap::new(), loaded: false, loaders: Vec::new(), owned: Vec::new(), do_spawns: true },
            );
            let center = coords.as_vec2() * CHUNK_SIZE + Vec2::splat(CHUNK_SIZE / 2.0);
            let len_sq = center.length_squared();
            let biomes = self.biomes.clone();
            for b in biomes {
                if let Some((lo, hi)) = b.distance_range_sq
                    && !(len_sq >= lo && len_sq <= hi)
                {
                    continue;
                }
                let mut ok = true;
                for (ch, ranges) in &b.noise_ranges {
                    let v = self.noise(coords, ch, coords.as_vec2());
                    if !ranges.iter().any(|&(lo, hi)| lo < v && v < hi) {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    continue;
                }
                let c = self.chunks.get_mut(&coords).unwrap();
                c.biome = Some(b.id.clone());
                c.chunkgen = Some(b.chunkgen.clone());
                break;
            }
        }
        self.chunks.get_mut(&coords).unwrap()
    }
}

pub fn chunk_of(world_pos: Vec2) -> IVec2 {
    (world_pos / CHUNK_SIZE).floor().as_ivec2()
}

/// `GridPointsNearEnumerator`: chunk coords within a circle of `radius` chunks.
pub fn points_near(center: IVec2, radius: i32) -> impl Iterator<Item = IVec2> {
    (-radius..=radius).flat_map(move |y| (-radius..=radius).map(move |x| IVec2::new(x, y))).filter(move |p| p.x * p.x + p.y * p.y <= radius * radius).map(move |p| center + p)
}

/// `WorldControllerSystem.Update`: load chunks near players and world
/// loaders, unload the rest, place debris in newly loaded chunks.
pub fn world_controller(world: &mut World) {
    let Some(map) = world.get_resource::<Cosmos>().map(|c| c.map) else { return };
    let mut wanted: HashMap<IVec2, Vec<Entity>> = HashMap::new();
    let mut sources: Vec<(Entity, Vec2, i32)> = Vec::new();
    {
        let mut q = world.query_filtered::<(Entity, &WorldPose), With<Player>>();
        sources.extend(q.iter(world).filter(|(_, p)| p.map == Some(map)).map(|(e, p)| (e, p.pos, PLAYER_LOAD_RADIUS)));
        let mut q = world.query::<(Entity, &WorldPose, &WorldLoader)>();
        sources.extend(
            q.iter(world)
                .filter(|(_, p, l)| p.map == Some(map) && !l.disabled)
                .map(|(e, p, l)| (e, p.pos, (l.radius / CHUNK_SIZE).ceil() as i32 + 1)),
        );
    }
    for (e, pos, radius) in sources {
        for c in points_near(chunk_of(pos), radius) {
            wanted.entry(c).or_default().push(e);
        }
    }
    // Unload.
    let unloading: Vec<IVec2> = {
        let cosmos = world.resource::<Cosmos>();
        cosmos.chunks.values().filter(|c| c.loaded && !wanted.contains_key(&c.coords)).map(|c| c.coords).collect()
    };
    for coords in unloading {
        let owned: Vec<Entity> = {
            let mut cosmos = world.resource_mut::<Cosmos>();
            let c = cosmos.chunks.get_mut(&coords).unwrap();
            c.loaded = false;
            c.loaders.clear();
            c.do_spawns = true;
            c.owned.iter().map(|(_, e)| *e).collect()
        };
        for e in owned {
            try_gc(world, e);
        }
    }
    // Load.
    let mut to_place = Vec::new();
    {
        let mut cosmos = world.resource_mut::<Cosmos>();
        for (coords, loaders) in wanted {
            let c = cosmos.chunk(coords);
            if !c.loaded {
                c.loaded = true;
                if c.do_spawns {
                    to_place.push(coords);
                }
            }
            c.loaders = loaders;
        }
    }
    to_place.sort_by_key(|c| (c.x, c.y));
    for coords in to_place {
        place_debris(world, coords);
    }
}

/// World-space AABBs of every grid on the map.
fn grid_boxes(world: &mut World, map: Entity) -> Vec<(Vec2, Vec2)> {
    let mut q = world.query::<(&Grid, &Xform)>();
    let mut out = Vec::new();
    for (g, pose) in q.iter(world) {
        if pose.parent != Some(map) {
            continue;
        }
        let (mut lo, mut hi) = (IVec2::MAX, IVec2::MIN);
        for ((x, y), _) in g.tiles.iter() {
            lo = lo.min(IVec2::new(x, y));
            hi = hi.max(IVec2::new(x + 1, y + 1));
        }
        if lo.x > hi.x {
            continue;
        }
        let (s, c) = pose.rot.sin_cos();
        let corners = [lo.as_vec2(), Vec2::new(hi.x as f32, lo.y as f32), hi.as_vec2(), Vec2::new(lo.x as f32, hi.y as f32)];
        let (mut a, mut b) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in corners {
            let w = pose.pos + Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c);
            a = a.min(w);
            b = b.max(w);
        }
        out.push((a, b));
    }
    out
}

/// `WorldGenDistanceCarver`s (the outpost's dead zone etc.): (world pos, squared thresholds high→low).
fn distance_carvers(world: &mut World, map: Entity) -> Vec<(Vec2, Vec<(f32, f32)>)> {
    let mut q = world.query::<(&EntityData, &WorldPose)>();
    let mut out = Vec::new();
    for (d, pose) in q.iter(world) {
        if pose.map != Some(map) {
            continue;
        }
        let Some(c) = d.get("WorldGenDistanceCarver") else { continue };
        let mut t: Vec<(f32, f32)> = c
            .get("distanceThresholds")
            .and_then(Node::as_seq)
            .unwrap_or_default()
            .iter()
            .map(|n| {
                let m = n.get("maxDistance").and_then(Node::as_f32).unwrap_or(0.0);
                (m * m, n.get("prob").and_then(Node::as_f32).unwrap_or(1.0))
            })
            .collect();
        t.sort_by(|a, b| b.0.total_cmp(&a.0));
        out.push((pose.pos, t));
    }
    out
}

/// `DebrisFeaturePlacerSystem.OnChunkLoaded`.
pub fn place_debris(world: &mut World, coords: IVec2) {
    let (map, chunkgen, existing) = {
        let mut cosmos = world.resource_mut::<Cosmos>();
        let map = cosmos.map;
        let c = cosmos.chunks.get_mut(&coords).unwrap();
        c.do_spawns = false;
        let Some(chunkgen) = c.chunkgen.clone() else { return };
        (map, chunkgen, c.owned.clone())
    };
    let Some(density_ch) = chunkgen.density_channel.clone() else { return };
    let existing: Vec<(Vec2, Entity)> = existing.into_iter().filter(|(_, e)| world.get_entity(*e).is_ok()).collect();
    if !existing.is_empty() {
        // Debris still standing from last time: nothing new (upstream reuses its points).
        world.resource_mut::<Cosmos>().chunks.get_mut(&coords).unwrap().owned = existing;
        return;
    }
    let density = world.resource_mut::<Cosmos>().noise(coords, &density_ch, coords.as_vec2() + Vec2::splat(0.5));
    if density == 0.0 {
        return;
    }
    let points = {
        let offs = ((CHUNK_SIZE - CHUNK_SIZE / 8.0) / 2.0) as i32 as f32;
        let mut cosmos = world.resource_mut::<Cosmos>();
        let center = coords.as_vec2() * CHUNK_SIZE + Vec2::splat(CHUNK_SIZE / 2.0);
        crate::poisson::sample_rectangle(Vec2::splat(-offs), Vec2::splat(offs), density, &mut cosmos.rng)
            .into_iter()
            .map(|p| center + p)
            .collect::<Vec<_>>()
    };
    let mut boxes = grid_boxes(world, map);
    let carvers = if chunkgen.point_set_carver { distance_carvers(world, map) } else { Vec::new() };
    let half = 0.5 + chunkgen.safety_radius;
    let mut owned = Vec::new();
    for point in points {
        let (proto, keep) = {
            let mut cosmos = world.resource_mut::<Cosmos>();
            let pd = cosmos.noise(coords, &density_ch, point / CHUNK_SIZE);
            let cancel = cosmos.rng.chance(chunkgen.random_cancel);
            if (pd == 0.0 && chunkgen.density_clip) || cancel {
                continue;
            }
            let (a, b) = (point - Vec2::splat(half), point + Vec2::splat(half));
            if boxes.iter().any(|(lo, hi)| lo.x < b.x && hi.x > a.x && lo.y < b.y && hi.y > a.y) {
                continue;
            }
            // Carvers (`PrePlaceDebrisFeatureEvent`).
            let mut carved = false;
            for (ch, ranges) in &chunkgen.noise_carvers {
                let v = cosmos.noise(coords, ch, point / CHUNK_SIZE);
                if ranges.iter().any(|&(lo, hi)| !(lo > v || hi < v)) {
                    carved = true;
                    break;
                }
            }
            if carved {
                continue;
            }
            if chunkgen.point_set_carver {
                let mut prob = 1.0f32;
                for (pos, thresholds) in &carvers {
                    let d2 = pos.distance_squared(point);
                    let mut newp = None;
                    for &(max2, p) in thresholds {
                        if d2 > max2 {
                            break;
                        }
                        newp = Some(p);
                    }
                    if let Some(p) = newp {
                        prob = prob.min(p);
                    }
                }
                if !cosmos.rng.chance(prob) {
                    continue;
                }
            }
            // Selection: the simple table first, then the noise-driven one may override.
            let mut proto = chunkgen.simple.as_ref().and_then(|t| t.spawns(&mut cosmos.rng).into_iter().next());
            if let Some((ch, table)) = &chunkgen.noise_selector {
                let p = cosmos.noise(coords, ch, point / CHUNK_SIZE);
                if (0.0..=1.0).contains(&p) && cosmos.rng.chance(p)
                    && let Some(w) = table.spawns(&mut cosmos.rng).into_iter().next()
                {
                    proto = Some(w);
                }
            }
            (proto, true)
        };
        let Some(proto) = proto.filter(|_| keep) else { continue };
        if let Some(e) = spawn_debris(world, &proto, map, point) {
            world.entity_mut(e).insert(OwnedDebris { chunk: coords, point });
            owned.push((point, e));
            boxes = grid_boxes(world, map);
        }
    }
    world.resource_mut::<Cosmos>().chunks.get_mut(&coords).unwrap().owned = owned;
}

/// Spawn a debris prototype as a grid and lay its floor (`BlobFloorPlanBuilder`).
pub fn spawn_debris(world: &mut World, proto: &str, map: Entity, pos: Vec2) -> Option<Entity> {
    let e = spawn_proto(world, proto, map, pos)?;
    make_grid(world, e);
    let blob = world.get::<EntityData>(e).and_then(|d| d.get("BlobFloorPlanBuilder").cloned());
    if let Some(b) = blob {
        build_blob(world, e, &b);
    }
    Some(e)
}

/// `BlobFloorPlanBuilderSystem.PlaceFloorplanTiles`.
pub fn build_blob(world: &mut World, grid: Entity, b: &Node) {
    let content = world.resource::<Content>().clone();
    let radius = b.get("radius").and_then(Node::as_f32).unwrap_or(4.0);
    let placements = b.get("floorPlacements").and_then(Node::as_i64).unwrap_or(0).max(0) as usize;
    let draw = b.get("blobDrawProb").and_then(Node::as_f32).unwrap_or(0.0);
    let tileset: Vec<u16> = b.get("floorTileset").map(Node::as_str_list).unwrap_or_default().iter().filter_map(|t| content.tiles.id_of(t)).collect();
    if tileset.is_empty() {
        return;
    }
    let mut rng = {
        let mut c = world.resource_mut::<Cosmos>();
        SimRng(c.rng.next_u64())
    };
    let r2 = (radius as f64).powi(2);
    let mut taken: HashMap<IVec2, astrolabe::tiles::Tile> = HashMap::new();
    let mut spawn_points: Vec<IVec2> = Vec::new();
    let mut in_points: HashSet<IVec2> = HashSet::new();
    let dirs = [IVec2::Y, IVec2::NEG_Y, IVec2::X, IVec2::NEG_X];
    let place = |p: IVec2, rng: &mut SimRng, taken: &mut HashMap<IVec2, astrolabe::tiles::Tile>, sp: &mut Vec<IVec2>, inp: &mut HashSet<IVec2>| {
        if inp.remove(&p) {
            if let Some(i) = sp.iter().position(|q| *q == p) {
                sp.swap_remove(i);
            }
        }
        for d in dirs {
            let n = p + d;
            if !taken.contains_key(&n) && ((n.x as f64).powi(2) + (n.y as f64).powi(2)) <= r2 && inp.insert(n) {
                sp.push(n);
            }
        }
        let id = tileset[rng.below(tileset.len() as u32) as usize];
        let variant = content.tiles.get(id).map_or(0, |d| d.pick_variant(rng.next_f32()));
        taken.insert(p, astrolabe::tiles::Tile { type_id: id, flags: 0, variant, rotation_mirroring: 0 });
    };
    place(IVec2::ZERO, &mut rng, &mut taken, &mut spawn_points, &mut in_points);
    for _ in 0..placements {
        if spawn_points.is_empty() {
            break;
        }
        let p = spawn_points[rng.below(spawn_points.len() as u32) as usize];
        place(p, &mut rng, &mut taken, &mut spawn_points, &mut in_points);
        if draw > 0.0 {
            for d in dirs {
                if !taken.contains_key(&(p + d)) && rng.chance(draw) {
                    place(p + d, &mut rng, &mut taken, &mut spawn_points, &mut in_points);
                }
            }
        }
    }
    let mut g = world.get_mut::<Grid>(grid).unwrap();
    for (p, t) in taken {
        g.tiles.set(p.x, p.y, t);
    }
    g.revision += 1;
}

/// `LocalityLoaderSystem.Update`: debris near a loader gets its contents.
pub fn locality_loader(world: &mut World) {
    let Some(map) = world.get_resource::<Cosmos>().map(|c| c.map) else { return };
    let pending: Vec<(Entity, Vec2, f32)> = {
        let mut q = world.query::<(Entity, &LocalityLoader, &WorldPose)>();
        q.iter(world).filter(|(_, _, p)| p.map == Some(map)).map(|(e, l, p)| (e, p.pos, l.loading_distance)).collect()
    };
    for (e, pos, dist) in pending {
        let chunk = chunk_of(pos);
        let loaders: Vec<Entity> = {
            let cosmos = world.resource::<Cosmos>();
            (-1..=1)
                .flat_map(|j| (-1..=1).map(move |i| chunk + IVec2::new(i, j)))
                .filter_map(|c| cosmos.chunks.get(&c))
                .filter(|c| c.loaded)
                .flat_map(|c| c.loaders.iter().copied())
                .collect()
        };
        let near = loaders.iter().any(|l| world.get::<WorldPose>(*l).is_some_and(|lp| lp.pos.distance(pos) <= dist));
        if near {
            world.entity_mut(e).remove::<LocalityLoader>();
            populate(world, e);
        }
    }
}

/// `LocalStructureLoadedEvent` handlers: `SimpleFloorPlanPopulator` and
/// Frontier's `RandomEntityPopulator`.
pub fn populate(world: &mut World, grid: Entity) {
    let Some(data) = world.get::<EntityData>(grid).cloned() else { return };
    let content = world.resource::<Content>().clone();
    let mut rng = {
        let mut c = world.resource_mut::<Cosmos>();
        SimRng(c.rng.next_u64())
    };
    let tiles: Vec<(IVec2, u16)> = {
        let g = world.get::<Grid>(grid).unwrap();
        let mut v: Vec<_> = g.tiles.iter().map(|((x, y), t)| (IVec2::new(x, y), t.type_id)).collect();
        v.sort_by_key(|(p, _)| (p.x, p.y));
        v
    };
    let center = |p: IVec2| p.as_vec2() + Vec2::splat(0.5);
    if let Some(pop) = data.get("SimpleFloorPlanPopulator") {
        let caches: HashMap<String, SpawnCache> = pop
            .get("entries")
            .and_then(Node::as_map)
            .map(|m| m.iter().map(|(k, v)| (k.to_string(), SpawnCache::from_node(Some(v)))).collect())
            .unwrap_or_default();
        for (p, tid) in &tiles {
            let Some(def) = content.tiles.get(*tid) else { continue };
            let Some(cache) = caches.get(&def.id) else { continue };
            for proto in cache.spawns(&mut rng) {
                spawn_proto(world, &proto, grid, center(*p));
            }
        }
    }
    if let Some(pop) = data.get("RandomEntityPopulator") {
        let mut free: Vec<IVec2> = tiles.iter().map(|(p, _)| *p).collect();
        for set in pop.get("entries").and_then(Node::as_seq).unwrap_or_default() {
            let params = set.get("params");
            let pf = |k: &str, d: f32| params.and_then(|p| p.get(k)).and_then(Node::as_f32).unwrap_or(d);
            let prob = pf("prob", 1.0);
            let (min, max) = (pf("min", 1.0) as u32, pf("max", 1.0) as u32);
            let sealed_ok = params.and_then(|p| p.get("canBeAirSealed")).and_then(Node::as_bool).unwrap_or(false);
            if !rng.chance(prob) {
                continue;
            }
            let cache = SpawnCache::from_node(set.get("entries"));
            let n = rng.next_range(min as i32, max as i32 + 1).max(0) as u32;
            for _ in 0..n {
                let mut chosen = None;
                for _ in 0..10 {
                    if free.is_empty() {
                        break;
                    }
                    let i = rng.below(free.len() as u32) as usize;
                    if !sealed_ok && tile_air_blocked(world, grid, free[i]) {
                        continue;
                    }
                    chosen = Some(free.swap_remove(i));
                    break;
                }
                let Some(t) = chosen else { break };
                for proto in cache.spawns(&mut rng) {
                    spawn_proto(world, &proto, grid, center(t));
                }
            }
        }
    }
    athanor::spawners::run_spawners(world);
    athanor::storage::run_table_spawners(world);
}

fn tile_air_blocked(world: &World, grid: Entity, t: IVec2) -> bool {
    let Some(g) = world.get::<Grid>(grid) else { return false };
    g.anchored_at(t).iter().any(|e| world.get::<EntityData>(*e).is_some_and(|d| d.has("Airtight")))
}

/// `GCQueueSystem.TryGCEntity` for the `SpaceDebris` queue (`trySkipQueue`:
/// nothing cancels the immediate path upstream, so unloaded debris goes now,
/// unless someone is standing on it).
pub fn try_gc(world: &mut World, e: Entity) {
    if world.get_entity(e).is_err() {
        return;
    }
    let occupied = {
        let mut q = world.query_filtered::<&Xform, With<InputMover>>();
        q.iter(world).any(|x| x.parent == Some(e))
    };
    let full = world.resource::<Cosmos>().gc_queue.len() > GC_DEPTH;
    if full || !occupied {
        athanor::spawn::delete(world, e);
    } else {
        world.resource_mut::<Cosmos>().gc_queue.push_back(e);
    }
}

/// `GCQueueSystem.Update`: once the queue is deep enough, delete debris whose
/// chunk hasn't been reloaded (`TryCancelGC`).
pub fn gc_queue(world: &mut World) {
    loop {
        let next = {
            let mut c = world.resource_mut::<Cosmos>();
            if c.gc_queue.len() < GC_MIN_DEPTH_TO_PROCESS {
                return;
            }
            c.gc_queue.pop_front()
        };
        let Some(e) = next else { return };
        let Some(owned) = world.get::<OwnedDebris>(e).copied() else { continue };
        let loaded = world.resource::<Cosmos>().chunks.get(&owned.chunk).is_some_and(|c| c.loaded);
        if !loaded {
            athanor::spawn::delete(world, e);
        }
    }
}
