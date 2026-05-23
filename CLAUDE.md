# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

MYcelime is a creature-raising Android game built for the Samsung Galaxy Z Flip 7's dual screens and folding mechanic. The creature species name is TBD — use placeholder `[CREATURE]` in all code and comments. The game's tone is dark/cosmic-horror, not cute. See `GDD_v1.md` for the full game design document, which is the single source of truth.

## Build & Run

```bash
# Build debug APK
./gradlew assembleDebug

# Install on connected device
./gradlew installDebug

# Run unit tests
./gradlew test

# Run instrumented tests (requires connected device/emulator)
./gradlew connectedAndroidTest

# Run a single test class
./gradlew test --tests "com.user.mycelime.ExampleUnitTest"
```

Build via Android Studio is standard. There is no CI configured yet.

## Tech Stack

- **Language:** Kotlin
- **UI:** Jetpack Compose (Compose BOM 2026.02.01)
- **Min SDK:** 31 | **Target SDK:** 36
- **AGP:** 9.2.1 | **Kotlin:** 2.2.10 | **KSP:** 2.2.10-2.0.2
- **Database:** Room 2.7.1 (via KSP — do NOT use kapt)
- **Package:** `com.user.mycelime`

KSP note: `android.disallowKotlinSourceSets=false` is set in `gradle.properties` — required for KSP to work with AGP 9.x built-in Kotlin. Do not remove it.

## Architecture & Current State

Phase 1 data layer is complete. The app has working tab navigation and a live Room database. The Nursery tab reads from the database (currently empty, so shows 0 eggs / 0 Bytes). All other screens are still stubs.

```
MainActivity
└── MYcelimeApp (Application)      # MYcelimeApp.kt — holds DB + repository singletons
    └── NurseryApp()               # NurseryApp.kt — tab state, Scaffold + NavigationBar
        ├── HabitatScreen()        # Screens.kt — stub
        ├── MapScreen()            # Screens.kt — stub
        ├── EvolutionScreen()      # Screens.kt — stub
        ├── CommandScreen()        # Screens.kt — stub
        └── NurseryScreen()        # Screens.kt — live: shows egg count + byte balance

Data layer (all under data/):
  AppDatabase.kt                   # Room singleton, version 1
  NurseryRepository.kt             # Exposes DAO Flows + suspend write functions
  entity/  Creature, Egg, Expedition, NurseryState
  dao/     CreatureDao, EggDao, ExpeditionDao, NurseryStateDao

Support:
  GameConfig.kt                    # All balance constants (TBD values — do not hardcode elsewhere)
  util/ByteDisplay.kt              # Long.toByteDisplay() — Bytes/KB/MB/GB/TB formatter
  viewmodel/NurseryViewModel.kt    # StateFlows: eggs, creatures, activeExpeditions, bytesDisplay
```

`InnerScreen` enum and `tabs` list live in `NurseryApp.kt`. All 5 screen composables are in `Screens.kt`.

## Adding New Game Data

- Add fields to an entity in `data/entity/` → increment `AppDatabase` version + write a `Migration`
- Add queries to the relevant DAO → expose via `NurseryRepository` → surface in `NurseryViewModel`
- All balance values go in `GameConfig.kt` as named constants, never inline

## Screen Architecture (from GDD)

**Cover screen (phone closed):** Passive nursery view — egg bay, clone vat status, alerts. Not yet built. Requires `androidx.window` FoldingFeature API + a separate presentation targeting the cover display.

**Inner screen (phone open) — 5 tabs:**
- `HABITAT` — Two-layer view: outer CRT/terminal grid (admin/edit), inner room view (creature's perspective). Tap a room node to enter the inner layer.
- `MAP` — Digital World map. Data-driven location system — locations are config, not hardcoded. Player assigns creatures to expeditions here.
- `EVOLUTION` — Lineage tree visualization, code injection mechanic for evolution choices.
- `COMMAND` — Direct creature management: assign expeditions, call back, manage equipment.
- `NURSERY` — Infrastructure: eggs, clone vats, research queue, upgrades.

## Key Design Constraints

- **Fold detection:** Use `androidx.window` `WindowInfoTracker` + `FoldingFeature` — library is already in deps. Do not poll or use deprecated `DisplayFeature` APIs.
- **Asset system:** All sprite assets must be swappable from day one. Build with a layer of indirection (resource IDs or an asset registry) so Phase 2 art drops require no structural changes.
- **Data-driven content:** Locations, evolution paths, and creature definitions are config/data, not hardcoded logic. NPC nurseries are the intentional exception — hardcoded for multiplayer system testing.
- **No internet in Phase 1:** All systems must work offline. Multiplayer (Bluetooth/NFC breeding, real raids) is Phase 3.
- **Byte currency:** Single underlying `Long` (`bytesRaw` in `NurseryState`). Display via `Long.toByteDisplay()` — never store the formatted string.
- **Death is permanent for individuals, not lineage:** `isAlive = false` + `dnaStored = true` on the `Creature` entity. Clone vat grows a new creature from stored DNA.

## Open / Not Yet Designed

Do not implement these until the GDD is updated: full combat system, Digital World map locations, evolution tree branch count/stages, expedition timer values, clone vat Byte costs, opening lore message text.

## What's Next (Phase 1 remaining)

Per GDD phase 1 checklist, still to build:
- Egg spawning timer (writes `Egg` rows, driven by `GameConfig.EGG_SPAWN_INTERVAL_MS`)
- Hatching system (reads incubation time, writes `Creature` row)
- Habitat view — outer CRT layer with room grid
- Habitat view — inner layer (room → creature perspective)
- Expedition system (send creature, timer, text result, Bytes return)
- Breeding system stub (two nurseries → new egg)
- Death + DNA extraction + clone vat system
- Basic raid system
- Fold detection — trigger UI state on open/close
- Cover screen
- 2 hardcoded NPC nurseries
