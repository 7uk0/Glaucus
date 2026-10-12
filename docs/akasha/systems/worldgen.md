# World generation (`demiurge`)

**What it's for:** Frontier is an open sector you fly around for money.
- **Points of interest** (POIs) are the destinations: cargo depots to haul
  to, the Trade Mall market, and stations with services (medical, the
  expedition lodge, the casino…).
- **Chunked asteroid fields** are the resource: rock to mine and wrecks to
  salvage. Density rises with distance from the outpost.

Both are ports of upstream behaviour. Upstream sources:
- `Content.Server/_NF/GameRule/NfAdventureRuleSystem.cs` (`Started`)
- `Content.Server/_NF/GameRule/PointOfInterestSystem.cs`
- `Content.Server/Worldgen/**` (SS14): `WorldControllerSystem`,
  `BiomeSelectionSystem`, `NoiseIndexSystem`, `DebrisFeaturePlacerSystem`,
  `BlobFloorPlanBuilderSystem`, `LocalityLoaderSystem`, `SimpleFloorPlanPopulatorSystem`,
  `NoiseRangeCarverSystem`, `GCQueueSystem`, `Tools/PoissonDiskSampler.cs`
- Frontier: `Content.Server/_NF/Worldgen/**` (`PointSetDistanceCarverSystem`,
  `RandomEntityPopulatorSystem`)

## POIs (`demiurge/src/poi.rs`)

The outpost (`/Maps/_NF/Outpost/frontier.yml`) is the game map at the origin.
`NfAdventureRuleSystem.Started` sorts every `pointOfInterest` prototype
allowed by the preset (`spawnGamePreset` empty or containing `NFAdventure`)
into lists by `spawnGroup`, then places them in this order:

1. **CargoDepot**: `nf14.worldgen.cargo_depots` = 4 depots.
   - Each picks a random depot prototype (`_random.Pick`).
   - Distance is `Next(min, max)`, an integer with max exclusive.
   - Depots sit on a ring at angles `rotationOffset + i·2π/4`, where
     `rotationOffset = NextAngle()/4`.
   - Each offset is `Vector2i` rotated, then truncated.
   - Named "{name} A..Z", with trade destination `Cargo{A..Z}`.
2. **MarketStation**: shuffle the list, take `nf14.worldgen.market_stations` = 1.
3. **Required**: place every one.
4. **Optional**: shuffle, then place. ⚠ Upstream never increments
   `optionalsAdded`, so **every** optional POI spawns and
   `nf14.worldgen.optional_stations` (6) has no effect. We port that faithfully.
5. **Any other group**: shuffle each group. Walk it, rolling
   `NextFloat() <= spawnChance`, and place the first success.

Placement details:
- Position comes from `NextVector2(min, max)`: a random angle with a uniform
  magnitude in [min, max).
- `GetRandomPOICoord` retries up to `nf14.worldgen.poi_placement_retries`
  (10) times while the point is closer than
  max(own `minimumClearance`, other's) to an already placed POI. After the
  last retry it uses the point anyway.
- Grids load with a random rotation (`TryLoadGrid(..., rot: NextAngle())`).
- `addComponents` are added to the grid (`EntityManager.AddComponents`,
  replacing components of the same type). Frontier uses this, for example,
  to add `WorldGenDistanceCarver` to the depots and the Trade Mall.

Result with seed 0xCA9D0D7E: 18 POIs.
- Depots A–D at 4.0–4.8 km.
- Trade Mall at 2.0 km.
- NFSD Outpost at 1.0 km.
- Courthouse, Medical Dispatch, Expeditionary Lodge, Edison Power Plant,
  Barrier Gate (9.3 km), casino, church, geode, Bahama Mama's, Grifty's,
  Listening Point Bravo, The Pit.

## Chunked fields (`demiurge/src/world.rs`)

**Chunks**
- 128 m chunks.
- Loaded within radius 2 (chunks) of a player, and within
  ceil(r/128)+1 of every enabled `WorldLoader`. `ComputerRadar` has a
  `WorldLoader` with radius 256, so chunks around every radar console
  (outpost, POIs, ships) stay loaded.
- Unloading a chunk sends its debris to the `SpaceDebris` GC.

**Biome selection** (config `NFDefault`, CVar `worldgen.worldgen_config`)
- Candidates in priority order: NFAsteroidsNear (0–5000 m), Mid (5000–9500 m),
  Far (beyond).
- The first biome whose distance range contains the chunk centre and whose
  `noiseRanges` match wins.

**Noise**
- Each chunk holds its own randomly seeded noise generator per channel
  (`NoiseIndexComponent` on the chunk). So the noise is *not* continuous
  across chunks. We seed from hash(world seed, chunk, channel) so a sector
  regenerates identically.
- FastNoiseLite runs with its default frequency of 0.01.
- `Density` is Perlin remapped to 0..1. Values in 0.4–0.6 are clipped to
  1.658. The result is ×50 +45, giving the Poisson minimum distance in metres.
  Clipped chunks get exactly one point.

**Debris placement** (`DebrisFeaturePlacerSystem.OnChunkLoaded`)
1. Poisson-disk points in the chunk's inner 112 m square.
2. For each point, in order:
   - skip if density is 0 and `densityClip` is set;
   - skip with chance 0.35 (`randomCancellationChance`);
   - skip if any grid's AABB intersects a box of half-size 0.5 + 24
     around the point.
   - Carvers:
     - `NoiseRangeCarver`: Carver noise in 0.4–0.6 drops the point. That is
       a lot of points, which is why fields come in streaks.
     - Frontier's `PointSetDistanceCarver`: each `WorldGenDistanceCarver`
       gives a probability by distance. The outpost's is 0 within 1000 m,
       then 0.1 at 1400 … 0.6 at 3400 m.
   - Selection: `SimpleDebrisSelector` (the biome's weighted orGroup table)
     picks first. Then `NoiseDrivenDebrisSelector` (Wreck noise) may override.
3. Reloading a chunk whose debris still exists spawns nothing new.

**Debris contents**
- Debris grids get floors from `BlobFloorPlanBuilder`: a random walk from
  (0,0) within `radius`, plus `blobDrawProb` neighbours. Tiles take a weighted
  `placementVariants` pick (`TileSystem.PickVariant`).
- Contents (rock walls, ore, scrap) spawn lazily: `LocalityLoader` fires when
  a chunk loader comes within 32 m. Then `SimpleFloorPlanPopulator` (per
  tile type) and Frontier's `RandomEntityPopulator` run.

**Garbage collection**
- `TryGCEntity` with `trySkipQueue` deletes at once. `TryCancelGC` cancels
  only if the owning chunk is loaded again.
- The queue (depth 256, processing from 128) is the fallback. We also keep
  debris that a mob is standing on.

**RNG semantics** (easy to get wrong)
- `IRobustRandom.Next(n)` is [0, n) → `SimRng::below`.
- `Next(min, max)` is [min, max) → `SimRng::next_range`.
- `SimRng::range_u32` is **inclusive**, matching `RangeNumberSelector` and
  `Next(min, max + 1)`.

## Tests

`crates/demiurge/tests/worldgen.rs` checks:
- depot ring, letters and spacing;
- one market;
- all requireds present;
- one POI per unique group;
- grids named and placed at their offsets;
- belt debris near a moving player, with only the Near biome inside 5 km;
- debris collected when the player leaves;
- nothing inside the outpost's dead zone.

## Not yet

- `OnDebrisMove`: debris re-parented to a new chunk when pushed.
- `SpaceDebrisComponent`.
- Station records and warp-point naming for POIs.
- `NFAsteroidsMid`/`Far` and the wreck tables, beyond what the tables load.
