# Mining (melee gathering and ore veins ported; PKA waits on guns)

**What it's for:** mining is the first way a new Frontier player earns money.
You fly to the asteroid belts (`demiurge` now generates them), break rock
with a pickaxe, drill or proto-kinetic accelerator (PKA), and collect ore.
You then sell the ore, or refine it into materials for the lathes.

## Upstream sources (read 2026-10-12)

- `Content.Server/Gatherable/GatherableSystem.cs`, `GatherableSystem.Projectile.cs`
- `Content.Server/Gatherable/Components/GatherableComponent.cs`, `GatheringProjectileComponent.cs`
- `Content.Server/Mining/MiningSystem.cs` (ore veins)
- `Content.Shared/Mining/OrePrototype.cs`, `Components/OreVeinComponent.cs`,
  `MiningScannerSystem.cs`
- Prototypes:
  - `Resources/Prototypes/ore.yml` and `_NF/ore.yml` (ore yields, weighted tables);
  - `Entities/Structures/Walls/asteroid.yml` (rocks);
  - `Entities/Objects/Weapons/Melee/mining.yml` (pickaxe, drills);
  - `Entities/Objects/Weapons/Guns/Projectiles/projectiles.yml` (PKA bolts,
    `GatheringProjectile`).

## How it works upstream

**Rock** (`AsteroidRock`, parent `BaseWall`)
- Anchored, airtight, static, `IconSmooth` key `walls` mode `NoSprite` with
  `SmoothEdge` edge layers (already rendered by `caput/src/smoothing.rs`).
- `Damageable` (Inorganic, Metallic modifiers).
- `Destructible`: 150 damage → `Destruction` + `break_stone.ogg`.
- `Gatherable { toolWhitelist: { tags: [Pickaxe], components: [MiningGatheringHard, MiningGatheringSoft] } }`
- `SoundOnGather`, `StaticPrice 25`.

**`GatherableSystem.OnAttacked`**
- If the *used* entity (the weapon, or the attacker when unarmed) passes
  `toolWhitelist`, the rock is gathered at once. **One hit with a valid tool
  mines a rock.** The tool's attack rate is what limits speed.
- Gathering plays `SoundOnGather`, calls `DestructibleSystem.DestroyEntity`
  (which raises `DestructionEventArgs`), then spawns `loot` entity tables
  offset by `NextVector2(gatherOffset = 0.3)`. A loot key other than "All"
  needs the gatherer to have that tag.

**`OnActivate`** (complex interaction): the *user* passes the whitelist,
e.g. a mob with a `MiningGathering*` component.

**PKA projectiles** (`GatheringProjectile.amount`)
- Only hits with the projectile fixture against a hard fixture count.
- If the bolt fails the rock's whitelist, it is deleted.
- If the rock's `OreVein.gatherDestructionWhitelist` passes the bolt,
  `PreventSpawning` is set: the ore is destroyed.
- Otherwise it gathers. `amount--`, and the bolt is deleted at 0.

**`OreVeinComponent`**
- Fields: `oreChance` (0.1), `oreRarityPrototypeId`, `currentOre`,
  `preventSpawning`, `gatherDestructionWhitelist`.
- MapInit: if `currentOre` is null and a rarity id is set, roll
  `Prob(oreChance)`, then pick a weighted ore.
- On destruction: unless `preventSpawning`, spawn the ore's `oreEntity`
  `Next(minOreYield, maxOreYield + 1)` times, offset by `NextVector2(0.2)`.

**Ores**
- `RandomOreDistributionStandard` weights: Steel 10, Coal 10, SpaceQuartz 8,
  Plasma 4, Gold 2, Silver 1, Uranium 1, Bananium 0.5, ArtifactFragment 0.5.
- Yields: steel, quartz, coal 1–5; gold, silver, salt 1–3; plasma 1–2.
- Frontier's `NFOre*` variants use the `*5` stack entities (`SteelOre5`, …).

**Tools** (`mining.yml`)
- `Pickaxe`: tag Pickaxe, attackRate 0.7, Structural 30.
- `MiningDrill`: tag Pickaxe, attackRate 4, Structural 15.
- `MiningDrillDiamond`: attackRate 4, Structural 30, plus `MiningGatheringHard`
  (Frontier).
- PKA bolts: Blunt 15 / Structural 30 with `GatheringProjectile`. The
  shuttle kinetic bolt (`BulletKineticShuttle`) is Blunt 30 / Structural 35.

## Port plan (athanor `mining.rs` + small hooks)

1. `EntityWhitelist` helper (tags, components, requireAll) and a `Tag`
   component. Both are needed in many places later too.
2. `Gatherable`, `SoundOnGather`, `OreVein` factories. Roll the ore for
   `Added<OreVein>` with `SimRng`.
3. Hook into `combat::try_melee` after a hit (the `AttackedEvent`): gather if
   the weapon passes the whitelist.
4. A destruction hook in `damage.rs`, so `OreVein` spawns ore on any
   destruction (guns, explosions), not only on gathering.
5. Later: projectiles/guns (`GatheringProjectile`), mining scanner overlay,
   ore processor and material sales (`mercury` prices already exist for items).
6. Test on real content: break `AsteroidRockMining` with a `Pickaxe`; check
   ore spawns within the yield range and that a non-tool weapon only deals
   damage.

## Ported (2026-10-12)

- `athanor/src/whitelist.rs`: `EntityWhitelist.IsValid`.
  - Components are checked against the entity's YAML. Then item sizes.
    Then tags, any or all.
  - With nothing matched, the result is `requireAll`.
  - Also `pass`, `fail`, `fail_or_null`, `tags_of` and `has_tag`.
- `athanor/src/mining.rs`:
  - `Gatherable`, `OreVein` (rolled by `roll_ore_veins` in Gameplay) and
    `GatheringProjectile`;
  - `gather`, `on_attacked` (hooked into `combat::try_melee` before damage),
    `on_activate` (interaction handler) and `on_projectile_hit` (ready for
    projectiles);
  - `on_destruction`, called by the new `damage::destroy_entity`.
    `check_destructible` uses it too, so guns and explosions drop ore.
- Test: `crates/athanor/tests/mining.rs`. A fist only damages the rock.
  A pickaxe mines `AsteroidRockCoal` in one swing and drops 1–5 `Coal5`.
  Plain rock with a random vein mines too.

Still to do:
- guns and projectiles (PKA);
- the mining scanner overlay (`MiningScannerViewable`);
- ore processing and selling ore.
