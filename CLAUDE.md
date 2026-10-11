# CLAUDE.md

Guidance for Claude Code when working in this repository.

## What this is

Caput Mortuum is a Rust rewrite of Space Station 14 + Frontier Station for
single-player with NPCs. Engine and game systems are reimplemented; **content
(prototypes, maps, RSIs, locale, audio) is read from upstream checkouts, never
vendored or converted.**

## Setup

```bash
tools/summon-upstream.sh            # fetch pinned upstream into ./upstream (gitignored)
cargo build -p caput                # client (Bevy 0.20; first build ~10 min)
cargo run --release -p caput        # play
cargo test --workspace              # tests; upstream-backed ones skip without ./upstream
```

Linux build deps: `pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev`.
Headless screenshots (no GPU): install `mesa-vulkan-drivers libxkbcommon-x11-0`, then
`xvfb-run -a -s "-screen 0 1600x900x24" env WGPU_BACKEND=vulkan target/debug/caput-mortuum --screenshot out.png --frames 150`.

## Crates

- `vellum` — resource roots, RSI, Fluent locale
- `grimoire` — YAML nodes, serde deserializer, prototype manager + inheritance
- `astrolabe` — tiles, grids, map files, decals, migrations
- `athanor` — simulation on `bevy_ecs` (all game systems)
- `homunculus` — NPC HTN planner/utility AI/pathfinding
- `mercury` — Frontier economy: bank, pricing, selling, shipyard
- `alembic` — atmospherics: tile gas, LINDA, airtightness, breathing, barotrauma
- `caput` — Bevy client

## Porting conventions

- **Port behaviour from upstream source, not memory.** Read the C# in
  `upstream/frontier` (Content.*) and `upstream/RobustToolbox` and mirror its
  defaults, formulas and edge cases. Cite the upstream class in module docs.
- A new component = a factory registered under its **upstream YAML name** in the
  module's `register()` (see `spawn::ComponentFactories`, `data_component!`).
  Prefer `#[derive(Deserialize)] #[serde(default, rename_all = "camelCase")]`
  structs with upstream defaults in `Default`.
- Unported components stay available as raw YAML via `EntityData`.
- Upstream-backed tests must pass trivially when `upstream/` is missing.
- Game content we add goes in `resources/` as upstream-format YAML, never in Rust.
- Update `docs/PORTING.md` when a system's status changes.
- Crate/module names follow an alchemical/esoteric theme (vellum, grimoire,
  athanor, homunculus, mercury, alembic...).

## License

AGPL-3.0 (Frontier is AGPL). Keep the MIT notices for SS14/RobustToolbox.
