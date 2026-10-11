# Map of upstream

Pins in `upstream.lock`; fetched by `tools/summon-upstream.sh` into `upstream/`.

| Checkout | What it is |
|---|---|
| `upstream/frontier` | new-frontiers-14/frontier-station-14 — **the content we run** (C# game code + `Resources/`) |
| `upstream/RobustToolbox` | the engine at Frontier's pinned commit (ECS, physics, transforms, maps, rendering, serialization) |
| `upstream/ss14` | vanilla space-wizards/space-station-14, for comparison (Frontier is a fork of it) |
| `upstream/ss14-docs` | space-wizards/docs, the developer book |
| `upstream/wiki/frontier` | Frontier wiki mirror (see [wiki.md](wiki.md)) |

## Frontier's C# layout

- `Content.Shared/` (334 dirs, ~3.4k files): components and systems that run on
  both sides; most component *data* lives here. Predicted logic.
- `Content.Server/` (266 dirs, ~2.7k files): authoritative logic: atmos, power,
  NPCs, chemistry reactions, game rules, cargo, shuttles' FTL, station events.
- `Content.Client/` (255 dirs, ~1.5k files): visuals (visualizers, overlays, UI windows).
- `_NF` subfolders in each (`Content.Server/_NF/Bank`, `.../Shipyard`, ...) are
  **Frontier's own systems**: Bank, BountyContracts, Cargo, CryoSleep, Market,
  Pirate, PublicTransit, Salvage, SectorServices, Shipyard, ShuttleRecords,
  Smuggling, Trade, Worldgen, Mail, Contraband, Manufacturing, Mech, ...
- Other fork tags appear too (`_DV` DeltaV, `_EE` Einstein Engines, `_Goobstation`...)
  for code Frontier pulled in from other forks. Comments `# Frontier` / `// Frontier`
  mark local edits to upstream files.
- `Resources/`: `Prototypes/` (YAML), `Maps/`, `Textures/` (RSIs), `Audio/`,
  `Locale/` (Fluent), `ServerInfo/Guidebook/` (in-game docs), `migration.yml`.

## How to find the code for a component

1. YAML `type: Foo` ↔ C# `class FooComponent` (or `[ComponentProtoName("Foo")]`).
   `docs/akasha/census.md` lists where every used component is defined.
2. The logic is in a `FooSystem` (often `SharedFooSystem` + `FooSystem` on the server).
3. Data fields: `[DataField]` with the C# default; YAML uses camelCase of the
   field name unless `[DataField("name")]` overrides it.
4. CVars (tunables) are in `Content.Shared/CCVar/CCVars*.cs`; Frontier overrides
   some in `Resources/ConfigPresets/` and `_NF` CVar files.

## Conventions worth knowing

- Times are `TimeSpan` (YAML `2` = 2 seconds, or `"0:00:02"`); "next update" fields
  are absolute game times (`IGameTiming.CurTime`).
- `DamageSpecifier` YAML: `{ types: {Blunt: 5}, groups: {Brute: 10} }`; groups
  split evenly over their types.
- `EntityTable` selectors (`!type:AllSelector`, `GroupSelector`, `NestedSelector`, `EntSelector`).
- Events: `RaiseLocalEvent` (directed at an entity) vs broadcast; `ByRefEvent`
  structs are mutated by handlers (e.g. `InhaleLocationEvent.Gas`).
