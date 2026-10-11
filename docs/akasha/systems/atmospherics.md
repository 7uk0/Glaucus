# Atmospherics and breathing (`alembic`)

**What it's for in play:** air is a resource and a hazard. Hull breaches vent
rooms and pull things toward space; vacuum kills without a suit; ships carry
their own air; expeditions vary the atmosphere (none, toxic, cold...).

## Upstream sources read

- `Content.Shared/Atmos/Atmospherics.cs`: constants (R, ONE_ATMOSPHERE 101.325,
  T20C 293.15, CELL_VOLUME 2500 L, MolesCellStandard, hazard pressures 20/550 kPa,
  warning = 2.5× low / 0.7× high, `BreathMolesToReagentMultiplier` 1144, `Gas` enum).
- `Content.Shared/Atmos/GasMixture.cs` (`RemoveRatio`, `RemoveVolume = RemoveRatio(vol/Volume)`).
- `Content.Server/Atmos/EntitySystems/AtmosphereSystem.{LINDA,Gases,...}.cs`
  (`ProcessCell`, `Share`, `TemperatureShare`, `Merge`).
- `Content.Server/Atmos/Serialization/TileAtmosCollectionSerializer.cs` (map format).
- `Content.Server/Atmos/EntitySystems/BarotraumaSystem.cs`, `Components/BarotraumaComponent.cs`,
  `PressureProtectionComponent.cs`.
- `Content.Server/Body/Systems/RespiratorSystem.cs`, `LungSystem.cs`,
  `MetabolizerSystem.cs`; `Content.Server/EntityEffects/EntityEffectSystem.cs`
  (`OnExecuteOxygenate`, `OnExecuteModifyLungGas`); `Content.Shared/EntityEffects/Effects/HealthChange.cs`.
- Frontier CVars: `atmos.heat_scale` 2 (specific heats ÷ 2), `atmos.tickrate` 15 Hz.
  Frontier has **11 gases** (adds Respiron 9, Helium 10).

## How upstream works

- Each grid has a `GridAtmosphere`: one `GasMixture` per tile (moles per gas,
  temperature, volume 2500 L). Saved in maps as unique mixes + per-chunk
  bitmasks (v2) or a tile→mix-index map (v1).
- **Airtight** entities (walls, windows, closed doors) block air in directions
  (`airBlockedDirection`, rotated with the entity unless
  `fixAirBlockedDirectionInitialize: false`). Doors toggle airtightness as they
  close/open. Docking airlocks carry a **tiny fan** (`AtmosDeviceFanDirectional`):
  airtight, but not collidable, so people pass and air doesn't.
- **LINDA**: active (excited) tiles share gas with open neighbours each atmos
  tick: delta per gas = (a − b) / (adjacentCount + 1), with heat capacities
  carried across; then `TemperatureShare`. Tiles go to sleep when nothing moves.
  Space (tiles with no floor) is an immutable vacuum: sharing with it removes gas.
- **Monstermos** (not ported yet) equalizes whole rooms at once and makes
  explosive decompression fast; LINDA alone is gradual.
- **Respirator**: every `UpdateInterval` (2 s), saturation −2; then inhale
  (`RemoveVolume(0.5 L)` from the containing mixture into the lungs) or exhale
  (lungs merge back into the environment). Below `SuffocationThreshold` (0):
  `Damage` (Asphyxiation; species base 1.0, MobRespirator 2) and a gasp emote
  (8 s cooldown); otherwise `DamageRecovery`. Dead mobs skip; crit mobs can't breathe.
- **Lungs**: inhaled gas → lung solution reagents (moles × 1144); the
  metabolizer removes up to `rate = metabolismRate (0.5) × rateModifier (lungs 100)`
  per update; effects scale by `removed / rate`. Oxygen's `Gas` metabolism has
  `Oxygenate` (saturation += quantity × factor) for Human/Animal/Rat/Plant organs,
  `ModifyLungGas` (O₂ → CO₂) for non-Vox, `HealthChange` Poison for Vox.
  What a mob breathes is purely data: `Body` → organs → `Lung` + `Metabolizer.metabolizerTypes`.
- **Barotrauma** (every 1 s): pressure = max(env, 1); below the warning level the
  pressure *felt* is `(p + lowMod) × lowMul` (capped at 1 atm); above, `(p + highMod) × highMul`.
  Protection comes from `head` + `outerClothing`: the weakest part wins; the mob's
  own `PressureProtection` stacks. ≤ 20 kPa: `Damage × 4` (base: Blunt 0.5, Heat 0.1);
  ≥ 550: scaled up to ×4. Damage **ignores resistances**. Stops at `maxDamage` 200.
  EVA helmet/suit: `lowPressureMultiplier: 1000`.

## How we ported it

`crates/alembic`: `gas.rs` (mixture math), `atmos.rs` (grid atmos, airtightness,
LINDA, space wind), `breathing.rs` (respirator, lungs, barotrauma). Simplifications:
breaths metabolize instantly; space wind is a simple shove (8 kPa items / 30 kPa mobs);
no Monstermos, internals, temperature, fire, reactions, pipes.

## Verified behaviour (tests in `crates/alembic/tests/atmos.rs`)

Outpost 3,163 air tiles at 101.3 kPa / 21% O₂, no leak in 20 s. Kestrel docking
airlock held open: the fan holds the air; fan removed: vestibule 101 → 12 kPa in
10 s. Human in vacuum 10 s: 20 Blunt, 2 Asphyxiation; EVA suit+helmet: 0 Blunt.
Breathing makes CO₂. Carp take nothing in space; mice breathe; Vox take ~1.75
Poison per breath of station air.

## Traps hit

- Pick airlocks by prototype in tests: `ShuttersNormalOpen` is also a `Door`.
- Iterating `HashMap`s makes tests nondeterministic; sort first.
- Removing an airtight entity must **wake** neighbouring tiles, or nothing flows.
