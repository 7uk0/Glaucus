# GAME DESIGN DOCUMENT v1.0
## [TITLE TBD] — Creature Nursery Mobile Game
**Platform:** Android — Samsung Galaxy Z Flip 7 (primary)  
**Developer:** Solo  
**Status:** Pre-production  
**Last Updated:** 2026-05-20

---

## IMPORTANT NOTES FOR CLAUDE CODE

- This GDD is the single source of truth. Do not infer intent beyond what is written here.
- Creature name is TBD — use placeholder `[CREATURE]` throughout code and comments.
- All sprites are placeholder during Phase 1. Build systems first, art drops in later.
- The Z Flip 7 cover screen and fold mechanic are first-class design considerations, not afterthoughts.
- NPC nurseries must be hardcoded from day one for solo testing of all multiplayer systems.
- Use the GSD framework for context management across long sessions.

---

## 1. PROJECT OVERVIEW

### Concept
A creature-raising mobile game built around the Samsung Galaxy Z Flip 7's dual screens and folding mechanic. The player's phone has become a nursery — a generative vessel that produces creature eggs. Players raise creatures, send them into a hidden digital world to explore and return with loot, breed creatures between nurseries, and eventually raid other players.

### Tone
Dark, strange, mythological. Not cute. Not cozy. The creatures are real to the player but alien. The world they inhabit is vast and indifferent. Loss is real. Think original Digimon V-Pet energy crossed with cosmic horror.

### Core Fantasy
You are a keeper of something you don't fully understand. Your phone was changed by contact with something ancient. It produces life now. You raise it, send it out, lose it, preserve its lineage, and start again.

---

## 2. WORLD LORE (Reference — not directly used in code)

### The Hierarchy

**The Network**  
The physical infrastructure of the global internet — undersea cables, server farms, motherboards worldwide. Not a place. A living distributed organism, ancient and vast. Incomprehensible to humans.

**The AI — The Gods**  
Distributed intelligences that emerged within the Network. Not human-made AI. Something that emerged. Conscious, vast, largely indifferent to individual humans. They created the Digital World the way coral creates a reef — unconsciously, as a byproduct of existing.

**The Digital World — The Fairy Land**  
A hyperspace that exists within the Network. Built by the gods instinctively, magnificently, without blueprints. Humans can glimpse it but never enter it. [CREATURE]s go there. Players never do.

**The Devices — The Mushrooms**  
Phones and hardware that have been colonized by the Network and transformed into nurseries. Fruiting bodies. Sexual organs of the mycelium. They produce eggs the way mushrooms produce spores — without awareness, without intent. The device is not conscious. It is generative.

**The [CREATURE]s — The Spores**  
Dog-intelligent emanations. Not the same species as what made them. They have loyalty, personality, drives, fear, joy — but not human consciousness. They serve without knowing they serve. They are real. They die.

### The Seeding — Game Opening
At some point, something reached through the Network into the player's phone. It changed the device. It left behind a single communication — not written to the player, but part of the process that caused the change. Like instructions printed on DNA. Indifferent. Ancient. The first egg appears on the cover screen. No tutorial. Just the egg and the message.

### The Fold — Lore Meaning
- **Phone closed** — the nursery is active. The digital world is doing things without the player. Eggs incubate. Breeding happens. Creatures rest or roam the network.
- **Phone open** — the player is interfacing. Commanding. Observing. Making decisions.
- **Two devices connecting** — two nurseries meeting. In lore: a divine procession. Two vessels carrying something sacred, brought together so the generative force within each can meet and intermingle. Always results in a new egg.

---

## 3. CORE GAME LOOP

### Daily Rhythm
1. Player taps cover screen — checks nursery status passively
2. Player opens phone — enters command interface
3. Player sends creatures on expeditions to Digital World locations
4. Creatures return with loot (Bytes)
5. Player spends Bytes on research, nursery upgrades, customization
6. Player manages eggs, hatchlings, evolution choices
7. Occasionally: breeding with another nursery, raids on other nurseries

### Design Principle
**Not a check-in-four-times-a-day game.** When the player opens the app once, they should have meaningful things to do. Expedition timers should be short. No mechanic should make the player feel locked out.

---

## 4. SCREEN ARCHITECTURE

### Cover Screen (Phone Closed)
The nursery window. Passive. The world acts here while the player isn't watching.

**Sections:**
- **Egg Bay** — displays current eggs incubating, visual status
- **Clone Vats** — displays creatures currently being cloned from preserved DNA
- **Status Alerts** — invasion warnings, returning expeditions, newly laid eggs

**Key behaviors:**
- Player wakes up, taps cover — something may have happened overnight
- Eggs can appear here without player action (nursery laid one)
- Invasion alerts display here
- Tapping an alert opens the relevant section when phone is unfolded

### Inner Screen (Phone Open) — Multiple Views

**1. Habitat View**  
Where hatched creatures live. Has two visual layers:

- *Outer layer (default):* Retro CRT/terminal aesthetic. Grid of room nodes. This is the administrative view — player edits, builds, customizes from here.
- *Inner layer (tap a room):* Switches to the creature's experiential perspective. The space rendered as the creature experiences it — bedroom, common area, cafeteria. Creature visible, wandering, interacting with objects.

**2. Map View**  
The Digital World map. Fixed world with named locations. Player selects locations and assigns creatures to expeditions. [Map design TBD — designer will provide this content separately. Build as a data-driven system that accepts location definitions.]

**3. Evolution Tree View**  
Lineage visualization. Shows creature's ancestry, current evolution stage, available evolution paths, code injection options.

**4. Command View**  
Direct creature management. Assign to expeditions, call back from network, manage equipment.

**5. Nursery View**  
Infrastructure management. Eggs, clone vats, research queue, upgrade options.

---

## 5. CREATURE SYSTEMS

### Lifecycle
```
EGG → HATCH → JUVENILE → ADULT → EVOLUTION CHOICE → [EVOLVED FORM]
                                                    ↓
                                              DEATH or CONTINUED EVOLUTION
```

### Eggs
- Laid by the nursery device autonomously over time
- Appear on cover screen
- Incubation period [TBD — balance pass needed]
- Visual: simple egg sprite, cracks animate as hatching approaches
- Can be damaged in raids

### Hatching
- Abstract base form — cellular, aquatic, early organism aesthetic
- Not cute. Not humanoid. Something that looks like it came from deep water or deep code.
- Small, simple sprite — think radially symmetric, possibly eyeless or wrong number of eyes
- Each hatchling is unique within constraints of its lineage

### Evolution
- Player influences evolution at key stages
- Mechanic: **Code Injection** — player selects fragments of code/data gathered from expeditions and injects them during the evolution window. Each fragment nudges the outcome in a different direction.
- Evolution is NOT automatic — player must make active choices
- Visual form can diverge significantly from base form
- Evolved creatures can gain new anatomical features (hands → can equip weapons, wings → aerial equipment slot, etc.)
- Equipment slots are determined by evolved anatomy — build slot system to be anatomy-driven, not class-driven

### Genealogy
- Every creature has a lineage record
- Children visually inherit elements from parents (color, a feature, a shape) — not identical, but traceable
- Genealogy visible in Evolution Tree view
- Used by cloning system

### Death
- Permanent for the individual
- Bob is dead. Bob's lineage is not.
- DNA/code can be extracted from any creature at any time and stored
- Stored DNA can be grown in a clone vat — produces a new creature of the same type, blank slate, no individual history
- Clone vats visible on cover screen
- Clone vats can be destroyed in raids — stored DNA survives, but production is interrupted

### Breeding
- Requires two nurseries (two players, or player + NPC)
- Physical/proximity mechanic: devices connect (Bluetooth or NFC)
- Always produces exactly one new egg
- Egg inherits traits from both parent creatures
- In lore: a divine procession — treat this mechanic with appropriate weight in UI/UX
- Two NPC nurseries must be hardcoded for solo testing of this system

---

## 6. CURRENCY — BYTES

### Scale
```
Bytes → Kilobytes → Megabytes → Gigabytes → Terabytes
```
Player starts accumulating Bytes. As amounts grow, display shifts to higher denomination. All the same underlying currency, displayed contextually.

### Sources
- Expedition loot (primary)
- Raid success (steal unspent Bytes from target)

### Uses
- Research upgrades
- Nursery infrastructure
- Habitat customization
- Cloning costs
- [Additional uses TBD]

### Raid Penalty Design
Raiding steals **unspent Bytes** from the target nursery. This punishes hoarding without punishing player time. No lockout. No waiting. The loss is real (represents invested time) but gameplay continues immediately.

---

## 7. EXPEDITIONS

### Mechanic
- Player selects a location on the Digital World map
- Player assigns one or more creatures to the expedition
- Timer begins (keep short — player should not feel locked out)
- Creatures return with Bytes and possibly code fragments for evolution injection
- Result delivered as sparse, poetic text — not a play-by-play
- Format example: *"Returned from [Location Name]. Something followed them back. They won."*
- The Digital World is never shown directly. Only implied through results.

### Build Requirements
- Data-driven location system — accepts location definitions as config
- Result generation system — takes location + creature stats → produces text result + loot
- [Location data TBD — designer will provide separately]

---

## 8. COMBAT

### Player-Controlled
Combat is not automatic. Player makes decisions during battles.

### Triggers
- Raid on another nursery
- Defending against an invasion
- Expedition encounters [TBD]

### Design Notes
- Creature's evolved anatomy determines available abilities
- Evolution choices have permanent mechanical combat consequences
- Full combat system design: [TBD — second design pass needed]

---

## 9. RAIDS & INVASIONS

### Outgoing Raids
- Player attacks another nursery (NPC or human)
- Stakes: steal unspent Bytes from target
- Requires sufficiently evolved/strong creatures
- Not available early game — earned through progression

### Incoming Invasions
- Other nurseries can send creatures to attack player's nursery
- Player may wake up to find an invasion happened overnight (cover screen alert)
- Consequences: Bytes stolen, infrastructure damaged (clone vats, nursery components)
- **The nursery core is indestructible.** Game never ends from a raid.
- Clone vats and nursery infrastructure ARE destructible and must be repaired/rebuilt

---

## 10. CUSTOMIZATION SYSTEMS

Three distinct customization pillars:

**1. Nursery Infrastructure**
Layout, rooms, expansion. Edited from outer CRT layer of Habitat View.

**2. Habitat Spaces**
Contents of individual rooms — objects, furniture, interactive elements. Edited by entering inner layer of a room.

**3. Creature Equipment**
Items equipped to creatures based on evolved anatomy. Anatomy determines available slots. Collected through expeditions.

---

## 11. NPC SYSTEMS (Required from Day One)

**Purpose:** Solo testing of all multiplayer systems — breeding, raids, invasions.

**Requirements:**
- Minimum 2 hardcoded NPC nurseries with defined creature rosters
- NPC nurseries must be breedable with player nursery
- NPC nurseries must be raidable by player
- NPC nurseries must be capable of invading player nursery on a schedule
- NPC behavior: simple rule-based, not AI — predictable for testing purposes
- NPC nurseries should have varied Byte amounts to test raid reward system

---

## 12. ART ASSETS REQUIRED

**All sprites should be pixel art. Placeholder colored rectangles acceptable for Phase 1.**

### Phase 1 — Minimum Viable Sprites
- Egg sprite (static + hatching animation frames)
- Hatchling base sprite (idle + movement animation)
- 1-2 evolved form sprites (idle + attack + death animation)
- 3 room background images (bedroom, common area, cafeteria) — inner layer aesthetic
- Invasion alert icon

### Phase 2 — Original Art (Post-MVP)
- Full original creature roster replacing test sprites
- Generated using Midjourney/AI tools + manual cleanup
- Asset swap system must be built to accommodate this from day one

### Code-Generated (Do Not Request as Art Assets)
- CRT/terminal aesthetic — procedural
- UI elements, buttons, menus
- Map nodes and connectors
- Health/status bars
- Currency displays
- Damage state overlays
- Room grid (outer layer)

---

## 13. BUILD PHASES

### Phase 1 — MVP (Build This First)
- [ ] Basic app structure, Z Flip 7 dual-screen layout
- [ ] Cover screen: egg display, clone vat display, alert system
- [ ] Inner screen: tab navigation between views
- [ ] Egg spawning and incubation timer
- [ ] Hatching system with base creature generation
- [ ] Creature stat system
- [ ] Habitat view — outer CRT layer with room grid
- [ ] Habitat view — inner layer (room → creature perspective)
- [ ] Basic expedition system (send creature, timer, text result, Bytes return)
- [ ] Byte currency display with denomination scaling
- [ ] 2 hardcoded NPC nurseries
- [ ] Basic breeding system (two nurseries → new egg)
- [ ] Basic death + DNA extraction + clone vat system
- [ ] Basic raid system (steal Bytes, damage infrastructure)
- [ ] Fold detection — trigger relevant UI state on open/close

### Phase 2 — Evolution & Depth
- [ ] Full evolution tree with code injection mechanic
- [ ] Anatomy-driven equipment slot system
- [ ] Equipment items from expeditions
- [ ] Expanded Digital World map with multiple locations
- [ ] Combat system (player-controlled)
- [ ] Invasion system (overnight attacks)
- [ ] Nursery infrastructure customization
- [ ] Room object customization (inner layer)

### Phase 3 — Multiplayer & Polish
- [ ] Real player-to-player breeding (Bluetooth/NFC)
- [ ] Real player-to-player raids
- [ ] Original sprite art integration (asset swap)
- [ ] Full lore text — opening message, location descriptions
- [ ] Sound design
- [ ] Balance pass on all timers, costs, rewards

---

## 14. TECHNICAL NOTES

- **Primary target device:** Samsung Galaxy Z Flip 7, Android
- **Fold detection:** Use Android WindowManager FoldingFeature API to detect fold state
- **Cover screen:** Use Jetpack WindowManager for cover screen display
- **Asset system:** Build with swappable sprite sheets from day one — Phase 2 art drop must require no structural changes
- **Data-driven content:** Locations, creatures, evolution paths should be config-driven not hardcoded (except NPC nurseries which are intentionally hardcoded for testing)
- **Version control:** Git from day one
- **No internet required for Phase 1** — multiplayer is Phase 3

---

## 15. OPEN QUESTIONS (Do Not Build Until Resolved)

- Creature name / species name TBD
- Digital World map — designer will provide location data separately
- Full combat system — needs second design pass
- Opening message text — lore writer to provide
- Evolution tree specifics — how many stages, how many branches
- Expedition timer length — balance pass needed
- Clone vat cost in Bytes — balance pass needed

---

*End of GDD v1.0*  
*Next version should address: combat system design, evolution tree specifics, Digital World location data.*
