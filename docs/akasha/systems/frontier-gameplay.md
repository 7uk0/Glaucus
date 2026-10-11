# Frontier: what the game is *for*

Sources: Frontier wiki (`upstream/wiki/frontier/html/`): Getting_Started, Piloting,
Asteroids, Basic_Salvaging, Expeditions, Shipyard; in-game guidebook
`Resources/ServerInfo/_NF/Guidebook/`. Mirrored 2026-10-11.

## The loop

Frontier is SS14 turned into an open, player-driven sector economy. A round
("shift") lasts 6–12 hours. **Only the bank balance persists between rounds**;
ships and stations reset.

1. Spawn at the **Frontier Outpost (FO)**, at sector coordinates (0, 0), with
   **30,000 spesos** in the bank (new players).
2. **Buy a ship** at a shipyard console. Real flow: take the ID card out of the
   PDA, insert it into the console, buy; the console writes a *deed* onto the ID.
   Buy price = base appraisal + 15–25% (maker markup; expedition ships ~+30%).
3. **Earn**, typically:
   - **Salvage** wrecks 5–9 km out: steel, plasma, uranium, glass, cloth,
     plasteel, gold, silver; valuables (gem rings, gold masks, coins). Feed
     materials to the ship's salvage techfab (magnet), trash to a matter reclaimer.
   - **Mining** asteroids: ore deposits can spawn ore crabs/golems; scrap
     asteroids spawn malfunctioning bots (some with lasers).
   - **Cargo runs**: crates bought at the Trade Mall carry a destination depot
     (A/B/C/D, at opposite ends of the sector); delivered crates pay 1–1.5k
     (express 2–4k).
   - **Expeditions** (expedition-capable ships only, bought at the Expedition
     Lodge): planet dungeons with difficulty tiers, atmosphere/light/temperature
     modifiers, faction enemies and loot-tier rewards.
   - Station jobs (janitor, mail carrier...), trading with other players,
     food ships, science (artifacts are worth nothing at the market, a lot to science).
4. **Sell**: the **Trade Mall** cargo pads pay **125%** of appraisal; other sell
   points (cargo depots) pay **75%**. Examine the sale computer to see the rate.
5. At shift end, **sell the ship back** (appraised at 100% of all grid contents),
   then cryo. Unsold ships are lost with their value.

## Sector layout

- FO at (0,0) with a 200 m safe zone (no antagonist activity).
- 0–1 km empty; **1–5 km** small asteroids, small blue wrecks, small scrap asteroids;
  **5–9 km** regular asteroids, larger and brass wrecks, larger scrap;
  **9–15 km** more snow/ice asteroids (bluespace ore, artifact fragments), clockwork debris.
- POIs: FO, NFSD Outpost, Medical Dispatch, Trade Mall, Cargo Depots A–D,
  Expedition Lodge (plus others to discover). Defined in `_NF` `pointOfInterest`
  prototypes and spawned by the sector generator.

## Piloting (as players experience it)

- Click the shuttle console to pilot: WASD strafe, Q/E rotate, Space brake.
  Docking mode turns A/D into lateral thrust.
- Gyroscopes give rotation and help braking; thrusters give linear thrust and
  braking. If they're off or destroyed, the ship won't respond.
- Docking: two docking airlocks face to face, "Dock" verb, both open and the
  grids join.

## Dangers

Space fauna on wrecks (carp, spiders whose venom sleeps you, bears, wyverns
with ranged breath), ore elementals, rogue bots; getting flung off a wreck into
space (jetpacks, magboots, PKA recoil); running out of oxygen (internals);
death → medical implant / suit sensors call medics.

## What this means for single-player (Caput Mortuum)

Multiplayer roles (other ships buying your goods, medics rescuing you, the
Station Representative, NFSD) have to become **NPCs or systems**:

- NPC contractors and traders at POIs; NPC ships flying the sector.
- NPC buyers at the Trade Mall / depots (the 125% / 75% split already exists in `mercury`).
- A rescue mechanic (medical implant → NPC medic ship, or respawn with a fee).
- Antagonists as NPC pirate crews; NFSD as NPC patrols.
- Persistence: keep the bank balance across sessions (a save file), reset the sector.

Port priorities this implies, after atmos: **power** (ships' machines, techfab,
reclaimer), **docking**, **sector generation** (asteroids, wrecks, POIs at the
right distances), **mining + salvage machines** (techfab, reclaimer, ore),
**internals/jetpacks/magboots**, **ranged weapons** (PKA), cargo crate runs,
then expeditions.
