# Porting status and roadmap

Upstream is large: Frontier's content code is ~830k lines of C# on top of
RobustToolbox's ~370k, with **1,669 component types** used across **17,900
entity prototypes**. A rewrite of that size is done in layers, and the order
matters: data formats first (so all content loads), then the simulation spine,
then systems in order of how much of the game they unlock.

Legend: ✅ ported · 🟡 partial · ⬜ not started

## Layer 0 — Content formats (complete)

| Area | Status | Notes |
|---|---|---|
| Resource roots / `ResPath` | ✅ | Layered roots: engine → Frontier → `resources/` overlay |
| YAML data nodes, `!type:` tags | ✅ | Direct-from-event-stream parsing; out-of-range numbers kept as strings like `ValueDataNode` |
| serde over data nodes | ✅ | Components are `#[derive(Deserialize)]`; `"x,y"` tuples, `!type:` → enum variants, `enum.X.Y` |
| Prototype manager + inheritance | ✅ | Multi-parent, never/always-push fields, component registry merge, `IgnoredPrototypes` |
| Map files (format 6 & 7) | ✅ | Tile chunks v1–v7, component deltas, `missingComponents`, migrations |
| RSI sprite sheets | ✅ | All 4,244 load; frame layout, directions, delays |
| Fluent locale | 🟡 | Messages, terms, attributes, selects; SS14 helper functions pass-through |
| Decals | ✅ | `DecalGrid` v2 |
| Entity migrations | ✅ | `migration.yml` + `nf_migration.yml` |

Verified by tests: 35,592 prototypes / 195 kinds, 374 maps, 4,244 RSIs, zero errors.

## Layer 1 — Simulation spine (`athanor`)

| Upstream system | Status | Notes |
|---|---|---|
| EntityManager / spawning | ✅ | Component-factory registry; raw YAML kept in `EntityData` for unported comps |
| Transform hierarchy, maps, grids | ✅ | Parent-relative; grid re-parenting when walking on/off grids |
| Anchoring | ✅ | Per-tile anchored lookup |
| Physics | 🟡 | Kinematic movers vs static fixtures, upstream collision groups bit-exact, cross-grid hulls. No dynamic-dynamic solver, no throwing yet |
| `SharedMoverController` | ✅ | Upstream acceleration/friction, tile friction, sprint/walk, weightless, `MovementAlwaysTouching` |
| Tick loop | ✅ | 30 Hz `SimTick` schedule; headless or real-time driven |
| Containers & storage | 🟡 | `EntityStorage` lockers/crates (open/close, swallow/spill), `Storage` backpacks with size/capacity, `StorageFill`, `EntityTableContainerFill`, `EntityTableSpawner`, entity tables (All/Group/Nested/Ent). No locks/welding, no grid-shaped inventory |
| Hands / items / pick up / drop | ✅ | |
| Interaction verbs, examine | 🟡 | Interact, use-in-hand, interact-using hooks; no verb menu yet |
| Inventory & clothing | 🟡 | Slots from templates, `Loadout`/`startingGear`; no armor/modifiers from clothing yet |
| Damage / modifier sets / containers | ✅ | Group splitting, coefficients, flat reductions |
| Mob thresholds & state | ✅ | Alive/Critical/Dead |
| Destructible | 🟡 | `DamageTrigger` + destruction/spawn/sound behaviours |
| Melee | ✅ | Attack rate, range, unarmed via mob's own weapon |
| Doors & airlocks | 🟡 | Two-phase state machine, bump-open, auto-close, blocked-closing, bolts. No access readers/power yet |
| Spawners | ✅ | `ConditionalSpawner`, `RandomSpawner`, `EntityTableSpawner` |
| Names | ✅ | `RandomMetadata`, localized datasets |
| Factions | ✅ | `npcFaction` hostility |
| Shuttles | 🟡 | Grid bodies (mass/inertia from tiles at upstream density), thruster/gyroscope pools, consoles & piloting, Frontier velocity ellipse, brakes, angular clamp. Hull-on-hull impacts (inelastic, momentum exchange). No impact damage or FTL yet |
| Docking | ✅ | Dock types, range/alignment, weld, airlock open+bolt+stay airtight, FTL-dock placement (shipyard), `ForceAnchor` stations |

## Layer 2 — NPCs (`homunculus`)

| Upstream system | Status | Notes |
|---|---|---|
| HTN planner (`HTNPlanJob`) | ✅ | Exact decomposition/rollback/branch-traversal semantics |
| HTN execution (`HTNSystem`) | ✅ | Operator lifecycle, services, replanning, plan priority |
| Utility queries | 🟡 | Hostiles/component queries, alive/distance/health/accessible/LOS considerations, curve presets |
| Pathfinding | 🟡 | Tile A* with doors; upstream uses a per-tile navmesh |
| Steering | 🟡 | Path following + stuck repath; upstream uses context steering |
| NPC melee | ✅ | Miss chance, chase, face target |
| NPC ranged combat, interaction operators | ⬜ | 28 operator/precondition/consideration types reported as unported by the domain loader |

## Layer 2b — Frontier economy (`mercury`)

| Upstream system | Status | Notes |
|---|---|---|
| Bank accounts | 🟡 | 30,000 default balance; ATM deposit by using cash, balance on click. No withdrawals/ledger |
| `PricingSystem` | 🟡 | Materials, stack price, static price, contents, Frontier `MobPrice`. No solutions yet |
| Pallet-console selling (`NFCargoSystem`) | ✅ | Sell pallets within `palletDistance`, console + item `MarketModifier`, cash stacks |
| Shipyard purchase | 🟡 | All 115 `vessel` prototypes by group; buy → ship spawned in clear space + deed. No selling ships back, no docking at purchase |
| Bounties, black market, contraband | ⬜ | |

## Layer 2c — Atmospherics (`alembic`)

| Upstream system | Status | Notes |
|---|---|---|
| `GasMixture`, gas prototypes | ✅ | All 11 Frontier gases; specific heats ÷ Frontier `heat_scale` 2 |
| `GridAtmosphere` loading | ✅ | Saved tile mixes (serializer v1 and v2 chunk bitmasks); unsaved floor starts as vacuum |
| Airtightness | ✅ | `Airtight` with `airBlockedDirection` + rotation; doors seal while shut; tiny fans pass people but not air |
| LINDA (`ProcessCell`, `Share`, `TemperatureShare`) | ✅ | Active-tile set, archive, pressure-difference wind, at Frontier's 15 Hz |
| Space | ✅ | Tiles with no floor are an immutable vacuum sink |
| Monstermos (explosive equalization) | ⬜ | Rooms vent through LINDA alone, so more slowly than upstream |
| Space wind (`MovedByPressure`) | 🟡 | Loose items and mobs shoved down steep gradients; simplified thresholds |
| `RespiratorSystem` | ✅ | Inhale/exhale cycle, saturation, suffocation damage + recovery, gasps |
| Lungs (`LungSystem`, `Gas` metabolism) | 🟡 | Data-driven from `Body` → lungs' `metabolizerTypes` → reagent `Oxygenate`/`ModifyLungGas`/`HealthChange` (Vox choke on O₂, everyone exhales CO₂). Breaths metabolize instantly instead of on the metabolizer clock |
| `BarotraumaSystem` | ✅ | Low/high pressure damage ignoring resistances, `PressureProtection` from head + outer clothing, `PressureImmunity`, `maxDamage` cap |
| Internals, gas tanks | ⬜ | A hardsuit stops barotrauma but you still need air |
| Temperature, fire, reactions | ⬜ | |
| Vents, scrubbers, pipes, canisters | ⬜ | |

## Layer 2d — Power (`prana`)

| Upstream system | Status | Notes |
|---|---|---|
| Node networks (`NodeContainer`, cable/device/terminal/port/adjacent nodes) | ✅ | Power groups (HV, MV, Apc); flood fill per grid, rebuilt when anchors change. Pipes not yet |
| Pow3r `BatteryRampPegSolver` | ✅ | Line-for-line: ramping, proportional sharing, battery pass-through, height ordering |
| Suppliers, consumers, batteries, chargers/dischargers | ✅ | Substations, SMES (via cable terminals), APCs; `Battery` ↔ network sync |
| APC nets, LV extension cables, `ApcPowerReceiver` | ✅ | Nearest provider within range; powered = received ≈ load |
| Fuel generators (P.A.C.M.A.N. family) | ✅ | Solid (material) and chemical fuel, burn rate by target power, start-on-map-init |
| `PowerCharge`, gravity generators, grid gravity | ✅ | Charge up/down, ~100 s of gravity on battery; asteroids/wrecks have none |
| Effects: thrusters, gyroscopes, airlocks, consoles | ✅ | Via `athanor::power::Unpowered` |
| Solar, AME, TEG, PA/singularity, radiation collectors | ⬜ | |
| Wires panels, power switches, battery-backed receivers, power monitoring UI | ⬜ | |
| Lights responding to power | ⬜ | Waits for lighting |

## Layer 2e — World generation (`demiurge`)

| Upstream system | Status | Notes |
|---|---|---|
| `NfAdventureRuleSystem` + `PointOfInterestSystem` | ✅ | Depots ring A–D, market, requireds, optionals, unique groups; clearance retries; random rotation; `addComponents` |
| `WorldControllerSystem`, `BiomeSelectionSystem`, `NoiseIndexSystem` | ✅ | 128 m chunks around players and `WorldLoader`s; NF near/mid/far biomes |
| `DebrisFeaturePlacerSystem` + selectors + carvers | ✅ | Poisson points, density/wreck/carver noise, Frontier `PointSetDistanceCarver` dead zone |
| `BlobFloorPlanBuilder`, `LocalityLoader`, populators | ✅ | Incl. Frontier `RandomEntityPopulator` |
| `GCQueueSystem` (`SpaceDebris`) | ✅ | |
| Station records / warp naming for POIs, debris re-chunking on move | ⬜ | |

## Layer 3 — Client (`caput`)

| Upstream system | Status | Notes |
|---|---|---|
| Sprite rendering, layers, draw depth | ✅ | Directional states, animations, `noRot`, offsets |
| Tile & decal rendering | ✅ | Variants, rotation/mirroring |
| Icon smoothing | ✅ | All modes incl. Frontier `DiagonalNF`, `SmoothEdge`, edge rule, DirOffsets (`smoothing.rs`) |
| Door / damage-state visuals | ✅ | Open/closed draw depth, airlock unlit/bolt lights |
| SubFloorHide, cable/pipe visuals | ✅ | `visibleLayers`, cable connection masks, pipe colours. No pipe connector stubs yet |
| Humanoid appearance | 🟡 | Species base sprites + skin; no hair/markings yet |
| Clothing visuals | ✅ | `clothingVisuals` and default `equipped-<SLOT>` states, species variants |
| Parallax | 🟡 | Image layer + generated star fields |
| HUD, popups, chat log, examine | 🟡 | Minimal panels; air readout with pressure/suffocation alerts |
| Audio | 🟡 | One-shot sounds, collections, distance attenuation |
| Lighting & occlusion | ⬜ | Fullbright for now |
| Generic visualizers (Appearance data) | ⬜ | |
| Map render mode (`--render-map`) | ✅ | 1:1 renders compared against official renders in `reference/` |
| Shipyard console window | ✅ | Paged registry, balance, buy |
| Storage window, locker visuals | ✅ | `EntityStorageVisuals`; take/put buttons |
| Other UIs (vending, ATM menu, PDA...) | ⬜ | |

## Layer 4 — Game systems (roadmap, roughly in order)

1. **Shuttles** (Frontier's heart): ~~piloting~~ ✅, ~~grid collisions~~ ✅, ~~docking~~ ✅, impact damage, FTL.
2. **Economy**: ~~spesos, bank, selling, shipyard purchases~~ ✅; bounties, vending, ship resale.
3. **Atmospherics** (`alembic`): ~~gas mixtures per tile, pressure, breathing, airtight~~ ✅; Monstermos, internals, temperature, fire, vents/scrubbers.
4. **Power** (`prana`): ~~APCs, SMES, cables, powered machines, generators, gravity~~ ✅; solar, AME, wires.
5. **Lighting** with occluders.
6. **Containers & storage**: ~~backpacks, lockers, crates, storage UI~~ ✅; locks/access, welding, crate pulling.
7. **Construction** graphs and tools.
8. **Chemistry & medicine**: solutions, reagents, reactions, metabolism.
9. **Food, drink, hunger/thirst.**
10. **Ranged weapons**, projectiles, hitscan; NPC gun operators.
11. **Frontier world**: ~~sector POIs, asteroid fields~~ ✅; expeditions, salvage, NFSD/pirate NPC crews.
12. Everything else (1,600+ remaining component types).

The upstream content does not change shape as systems are ported: each new
system registers factories for its components and starts acting on the data
that has been loading all along.
