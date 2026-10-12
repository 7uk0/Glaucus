# Journal

## 2026-10-11 — Session 1

Asked for a full Rust rewrite of SS14 + Frontier for single-player with NPCs.

Built, in order (each a commit):

1. `vellum`, `grimoire`, `astrolabe`: all 35,592 prototypes (195 kinds), 374 maps,
   4,244 RSIs load with zero errors; Robust inheritance; map formats 6/7;
   migrations; Fluent locale.
2. `athanor`: bevy_ecs simulation at 30 Hz — transforms/grids, physics with
   upstream collision groups, `SharedMoverController` movement, hands, damage,
   mob states, destructibles, melee, doors, inventory/clothing, spawners, names.
3. `homunculus`: HTN planner port (`HTNPlanJob`), utility queries, A*, steering.
   Key lesson: preconditions during planning read *planned* `OwnerCoordinates`.
4. `caput`: Bevy client, upstream sprites, icon smoothing, humanoid/clothing
   layers, HUD, parallax, sound. Screenshots via Xvfb + lavapipe.
5. Shuttles: Frontier's velocity ellipse, thrusters/gyros, consoles, piloting,
   hull-on-hull impacts.
6. `mercury`: bank (30,000 default), pricing, pallet selling, shipyard (115 vessels).
7. Storage: lockers/crates, backpacks, `StorageFill`, entity tables.
8. `alembic`: atmospherics — tile air from maps, airtightness, LINDA at 15 Hz,
   space sink, breathing (data-driven lungs), barotrauma with suits.

Then asked: save memory and code as we go, learn every system, pull both
wikis with images, don't stop. Started this Akasha, `tools/summon-wiki.py`,
code bundles.

Then (same day, continued):

- Frontier wiki mirrored (613 pages, 232 images); SS14 wiki origin down
  (Cloudflare 530), web.archive.org unreachable from the container; SS14 dev
  docs cloned; guidebooks found in-repo. Wrote `systems/frontier-gameplay.md`.
- `speculum` census: 1,675 component types; coverage 32.3% → 36.2% of map
  placements after power.
- `prana` (power): node networks, exact Pow3r solver, APCs/SMES/substations,
  LV extension cables, fuel generators, PowerCharge, gravity generators and
  grid gravity; athanor honours `Unpowered` (thrusters, airlocks, consoles) and
  `Weightless` (movement). Kestrel fully powered; outpost 711/717.
- Code delivered to the user as a git bundle (GitHub repo still missing).

Next (priority for the single-player Frontier loop, see systems/frontier-gameplay.md):
sector generation (POIs, asteroid belts, wrecks) → docking → mining + PKA
(ranged weapons) → salvage machines (techfab, reclaimer) → internals,
jetpacks, magboots → lighting → cargo crate runs → expeditions →
chemistry/medical → food → construction.

## 2026-10-12 — Session 1, continued

- Started `demiurge` (world generation): noise channels (FastNoiseLite),
  `EntitySpawnCollectionCache`, Poisson sampler, chunk controller, biomes,
  debris placement, blob floors, locality population, debris GC. **Work in
  progress: `world.rs` is written but not yet wired into `lib.rs` or compiled;
  `poi.rs` (Frontier POI placement) not written yet.** See
  `systems/frontier-gameplay.md` for the rules being ported (POI groups and
  distances, NF biomes near/mid/far, outpost dead zone).
- grimoire: `chunkComponents` (spaceBiome) and `addComponents`
  (pointOfInterest) now merge per component on inheritance.
- athanor: `spawn::delete` is linear in subtree size; `transform::make_grid`.
- The worker restarted several times; the user asked for plain status answers
  and for the code + memory to be committed to one of their existing repos,
  since `7uk0/Caput-Mortuum` can't be created from here. Pushed to
  `7uk0/Glaucus`, branch `ccr-9978bc94-j2gdmx` (unrelated history; `master`
  untouched). Move it to its own repo once one exists.

## 2026-10-12 — Rendering fidelity pass

- User: textures "rotating weirdly" in screenshots → compared against the
  official Surveyor render. Fixed in order:
  - noRot/snapCardinals/direction rotation (engine rules);
  - IconSmooth rewritten as `caput/src/smoothing.rs`: all modes, DirOffset, edge rule;
  - tiles and decals moved below every DrawDepth;
  - decals drawn in (zIndex, id) order;
  - SubFloorHide `visibleLayers` (vents and pumps show on floors);
  - door draw depth and airlock lights;
  - cable connection masks (prana `CableVis`);
  - pipe colours.
- Surveyor now matches the official render except power state and decal order.
  The official render is unpowered, and MapRenderer's decal sort is unstable.
- Rules written up in `systems/rendering.md`.
- `--render-map PATH` renders a map 1:1 for comparisons. `speculum --bin scry`
  prints an entity's resolved components.

## 2026-10-12 — Worldgen wired in, reference images

- `demiurge` finished and wired into the game:
  - POI placement (`poi.rs`);
  - chunked asteroid fields (`world.rs`);
  - `DemiurgePlugin`;
  - tests on real content.

  The game now starts with 18 POIs around the outpost and generates belts
  as you fly. `--no-worldgen` skips it. See `systems/worldgen.md`.
- Found and fixed RNG range bugs: `SimRng::range_u32` is inclusive. Added
  `below(n)` and `next_range(min, max)` for upstream's exclusive `Next`.
  Spawn-table amounts (`EntitySpawnCollection`) were off by one.
- `astrolabe` `TileDef::pick_variant` (weighted `placementVariants`).
- `athanor::spawn::add_components` (`EntityManager.AddComponents`).
- `reference/images/`: 184 reference images.
  - Sources: the Frontier shipyard site (ship and POI renders), an SS14
    mapserver mirror (station renders), and the Steam page and
    spacestation14.com (screenshots).
  - Use them with `--render-map` for 1:1 comparisons.

## 2026-10-12 — Docking; 112-ship render check

- Docking ported (`athanor/src/docking.rs`, `systems/docking.md`):
  - dock/undock, weld, FTL-dock placement;
  - bought ships and the starting Kestrel arrive docked to the outpost;
  - pilots press F to dock or undock;
  - Frontier `ForceAnchor` stations are static.
- Rendered all 112 ships with shipyard renders and compared per pixel. All
  sizes match. 103 ships differ on fewer than 3% of pixels, and the
  differences are explained (`systems/rendering.md`).

## 2026-10-12 — Memory save after a worker restart

- The worker restarted while mining research was in progress. Nothing was
  lost: docking was already committed (1bf5cd9).
- The user asked to "save memory as much as you can". Added:
  - `next.md`: handoff page with state, next steps, open threads and commands;
  - `systems/mining.md`: everything read about upstream mining, with a port plan;
  - `tools/conjure-fleet.sh` and `tools/catoptromancy.py`: the batch render
    and compare tools, moved out of the scratchpad;
  - disk and batch-render gotchas in `environment.md`.

## 2026-10-12 — Mining (melee)

- Ported the `EntityWhitelist` helper, `GatherableSystem` and `MiningSystem`
  ore veins. A pickaxe mines rock in one hit, like upstream. Ore drops on any
  destruction.
- Tested on real content. All sim crate tests are green.
