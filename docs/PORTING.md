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
| Containers | 🟡 | `InContainer` for held/worn items; no general `ContainerManager`, no storage UIs |
| Hands / items / pick up / drop | ✅ | |
| Interaction verbs, examine | 🟡 | Interact, use-in-hand, interact-using hooks; no verb menu yet |
| Inventory & clothing | 🟡 | Slots from templates, `Loadout`/`startingGear`; no armor/modifiers from clothing yet |
| Damage / modifier sets / containers | ✅ | Group splitting, coefficients, flat reductions |
| Mob thresholds & state | ✅ | Alive/Critical/Dead |
| Destructible | 🟡 | `DamageTrigger` + destruction/spawn/sound behaviours |
| Melee | ✅ | Attack rate, range, unarmed via mob's own weapon |
| Doors & airlocks | 🟡 | Two-phase state machine, bump-open, auto-close, blocked-closing, bolts. No access readers/power yet |
| Spawners | 🟡 | `ConditionalSpawner`, `RandomSpawner`; `EntityTableSpawner` pending |
| Names | ✅ | `RandomMetadata`, localized datasets |
| Factions | ✅ | `npcFaction` hostility |
| Shuttles | 🟡 | Grid bodies (mass/inertia from tiles at upstream density), thruster/gyroscope pools, consoles & piloting, Frontier velocity ellipse, brakes, angular clamp. No grid collisions, docking or FTL yet |

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

## Layer 3 — Client (`caput`)

| Upstream system | Status | Notes |
|---|---|---|
| Sprite rendering, layers, draw depth | ✅ | Directional states, animations, `noRot`, offsets |
| Tile & decal rendering | ✅ | Variants, rotation/mirroring |
| Icon smoothing | 🟡 | Corners mode (walls/windows/tables). CardinalFlags/Diagonal pending |
| Door / damage-state visuals | ✅ | |
| Humanoid appearance | 🟡 | Species base sprites + skin; no hair/markings yet |
| Clothing visuals | ✅ | `clothingVisuals` and default `equipped-<SLOT>` states, species variants |
| Parallax | 🟡 | Image layer + generated star fields |
| HUD, popups, chat log, examine | 🟡 | Minimal panels |
| Audio | 🟡 | One-shot sounds, collections, distance attenuation |
| Lighting & occlusion | ⬜ | Fullbright for now |
| Generic visualizers (Appearance data) | ⬜ | |
| UIs (vending, cargo, shipyard, PDA...) | ⬜ | |

## Layer 4 — Game systems (roadmap, roughly in order)

1. **Shuttles** (Frontier's heart): ~~piloting~~ ✅, grid-grid collisions, docking, FTL.
2. **Economy**: spesos, bank, `StaticPrice` selling, cargo, shipyard purchases, bounties.
3. **Atmospherics** (`alembic`): gas mixtures per tile, pressure, breathing, airtight, vents/scrubbers.
4. **Power**: APCs, SMES, cables, powered machines (lights, doors, consoles).
5. **Lighting** with occluders.
6. **Containers & storage**: backpacks, lockers, crates, storage UI.
7. **Construction** graphs and tools.
8. **Chemistry & medicine**: solutions, reagents, reactions, metabolism.
9. **Food, drink, hunger/thirst.**
10. **Ranged weapons**, projectiles, hitscan; NPC gun operators.
11. **Frontier world**: sector POIs, expeditions, salvage, NFSD/pirate NPC crews.
12. Everything else (1,600+ remaining component types).

The upstream content does not change shape as systems are ported: each new
system registers factories for its components and starts acting on the data
that has been loading all along.
