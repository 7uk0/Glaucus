# Where we are, what's next

The handoff page. **Update it at the end of every work block.** It should
always let a fresh session resume without the chat history.

## State (2026-10-12)

**Where everything lives**
- Code and memory: `7uk0/Glaucus`, branch `ccr-9978bc94-j2gdmx`. Local
  remote name: `glaucus`. The history is unrelated to Glaucus `master`; never
  open a PR into it. Move to `7uk0/Caput-Mortuum` once the user creates that
  repo; see `environment.md`.
- Local checkout: `/home/user/Caput-Mortuum`. `upstream/` is fetched by
  `tools/summon-upstream.sh` at the commits in `upstream.lock`.

**Playable now** (single player, no netcode)
- Spawn at Frontier Outpost with 30 000 spesos. The Kestrel starts docked.
- Buy ships at the shipyard console; they arrive docked.
- Pilot ships: WASD, Q/E, Space to brake. **F** docks or undocks.
- Fly to 18 generated POIs and the asteroid belts.
- Sell cargo.
- Atmospherics, power, doors and airlocks work.
- NPC contractors, mice and carp.

**Ported systems** (one Akasha page each)
- Atmospherics (`alembic`): `systems/atmospherics.md`
- Power (`prana`): `systems/power.md`
- Worldgen (`demiurge`): `systems/worldgen.md`
- Docking: `systems/docking.md`
- Rendering fidelity: `systems/rendering.md`
- Economy and shipyard (`mercury`): see `PORTING.md`
- NPC AI (`homunculus`): see `PORTING.md`

**Verified**
- 112 ship renders compared with the official shipyard renders: all sizes
  match, and the remaining differences are explained.
- `cargo test --workspace` is green.

## Next, in order

The goal is the Frontier loop: buy ship → mine/salvage → sell → upgrade.
1. **Mining**: melee gathering and ore veins are ported (`systems/mining.md`).
   Next comes the client side: try it in game (a pickaxe from a locker or
   `--player` loadout), then the mining scanner overlay.
2. **Guns and projectiles**: hitscan, projectiles, PKA, NPC gun operators.
   Mining's PKA needs this.
3. **Ore processing and material sales**: ore processor, lathe materials.
4. **Salvage machines**: techfab, reclaimer. **Expeditions.**
5. **Lighting**: point lights and occluders (we render fullbright now).
6. **Internals, jetpacks, magboots**: space walks to the asteroids.
7. **Cargo crate runs**: Frontier depots A–D already exist as POIs.
8. Chemistry and medical, food, construction.

## Open threads

- `7uk0/Caput-Mortuum` doesn't exist yet (`add_repo` → not found). Retry now
  and then.
- SS14 wiki was down (Cloudflare 530). Retry with `tools/summon-wiki.py ss14`.
- Client rendering not yet ported:
  - `GenericVisualizer` (appearance-driven states);
  - pipe connector stubs;
  - lighting.
- Docking not yet ported: pathfinding portals across docked grids, radar
  dock UI, FTL travel.

## Handy commands

```bash
cargo run -q --release -p speculum --bin scry -- AsteroidRock --only Sprite,Gatherable   # resolved components
target/debug/caput-mortuum --render-map /Maps/_NF/Shuttles/surveyor.yml --screenshot out.png --frames 60
tools/conjure-fleet.sh target/fleet                                  # render all 112 ships (≈30 min)
python3 -I tools/catoptromancy.py target/fleet reference/images/frontier-ships          # score them
python3 -I tools/catoptromancy.py target/fleet reference/images/frontier-ships kestrel 2 # diff image
target/debug/caput-mortuum --no-worldgen --spawn shuttle --screenshot out.png   # quick start aboard the Kestrel
```
