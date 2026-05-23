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
- **Min SDK:** 24 | **Target SDK:** 36
- **AGP:** 9.2.1 | **Kotlin:** 2.2.10
- **Package:** `com.user.mycelime`

Key library additions needed for future phases: `androidx.window` (fold detection / cover screen), `androidx.room` (persistence), `androidx.lifecycle:lifecycle-viewmodel-compose` (state management).

## Architecture & Current State

The app is in early Phase 1. Only the shell exists: working bottom tab navigation with 5 placeholder screens.

```
MainActivity
└── NurseryApp()          # NurseryApp.kt — tab state, Scaffold + NavigationBar
    ├── HabitatScreen()   # Screens.kt — stub
    ├── MapScreen()       # Screens.kt — stub
    ├── EvolutionScreen() # Screens.kt — stub
    ├── CommandScreen()   # Screens.kt — stub
    └── NurseryScreen()   # Screens.kt — stub
```

`InnerScreen` enum and `tabs` list live in `NurseryApp.kt`. All 5 screen composables are in `Screens.kt`.

## Screen Architecture (from GDD)

**Cover screen (phone closed):** Passive nursery view — egg bay, clone vat status, alerts. Requires `androidx.window` FoldingFeature API + a separate Activity/presentation targeting the cover display.

**Inner screen (phone open) — 5 tabs:**
- `HABITAT` — Two-layer view: outer CRT/terminal grid (admin/edit), inner room view (creature's perspective). Tap a room node to enter the inner layer.
- `MAP` — Digital World map. Data-driven location system — locations are config, not hardcoded. Player assigns creatures to expeditions here.
- `EVOLUTION` — Lineage tree visualization, code injection mechanic for evolution choices.
- `COMMAND` — Direct creature management: assign expeditions, call back, manage equipment.
- `NURSERY` — Infrastructure: eggs, clone vats, research queue, upgrades.

## Key Design Constraints

- **Fold detection:** Use `androidx.window` `WindowInfoTracker` + `FoldingFeature` — do not poll or use deprecated `DisplayFeature` APIs directly.
- **Asset system:** All sprite assets must be swappable from day one. Build with a layer of indirection (resource IDs or an asset registry) so Phase 2 art drops require no structural changes.
- **Data-driven content:** Locations, evolution paths, and creature definitions are config/data, not hardcoded logic. NPC nurseries are the intentional exception — they are hardcoded for multiplayer system testing.
- **No internet in Phase 1:** All systems must work offline. Multiplayer (Bluetooth/NFC breeding, real raids) is Phase 3.
- **Byte currency:** Single underlying Long value displayed contextually as Bytes / KB / MB / GB / TB based on magnitude.
- **Death is permanent for individuals, not lineage:** DNA extraction → clone vat is the resurrection path, not reversal.

## Open / Not Yet Designed

Do not implement these until the GDD is updated: full combat system, Digital World map locations, evolution tree branch count/stages, expedition timer values, clone vat Byte costs, opening lore message text.
