# Docking

**What it's for:** in Frontier you buy a ship at the outpost, fly it to a
POI, dock, walk aboard, trade, and fly back. Docking is what makes ships and
stations one walkable place, and the shipyard relies on it: bought ships
arrive docked.

Upstream:
- `Content.Server/Shuttles/Systems/DockingSystem.cs`
- `DockingSystem.Shuttle.cs`
- `Content.Shared/Shuttles/Systems/SharedDockingSystem.cs`
- `ShuttleSystem.FasterThanLight.cs` (`TryFTLDock`, `FTLDock`)
- Frontier `ForceAnchorSystem`

Port: `athanor/src/docking.rs`.

## Rules

**Docks and types**
- A dock is an airlock with `Docking`.
- Frontier `dockType` flags: Airlock=1, Gas=2, Transit=4. The default is
  Airlock|Transit. Two docks can join only if their types share a flag.
  Shuttle airlocks are `Airlock` only, so they can't use transit-only ports.
- `receiveOnly` docks can't start a dock.

**Can two docks join** (`CanDock`)
- Neither is already docked.
- Both are anchored and on the same map.
- They are within 1.2 m (`DockRange` = 1 + 0.2).
- Each faces the other to within 15°: |reduced(rotA − angle(A→B))| ≤ 15°,
  and the same from B. A dock "faces" its rotation's world vector, where
  rotation 0 faces south.

**Docking** (`Dock`)
- The pair is ordered by uid.
- A weld joint "docking{A}" joins the grids at each dock's outer edge
  (`localPos + rot.ToWorldVec()/2`). It keeps the grids' relative rotation
  at the moment of docking. Upstream it is soft (2 Hz, damping 0.7); ours is
  rigid, solved after shuttle motion (`solve_welds`).
- Both doors `TryOpen`. If they open, their bolts go down.
- `ChangeAirtight = false`, so the open airlocks keep holding air.
- Auto-close is cancelled while docked (`BeforeDoorAutoCloseEvent`).

**Undocking**
- Bolts up, `TryClose`; if the door closes, `ChangeAirtight = true`.
- Our pilots press **F** to dock to the nearest valid pair, or undock all
  when already docked. Upstream this is the console's radar dock list and
  Frontier's "undock all".

**FTL dock placement** (`GetDockingConfigs`): for each shuttle dock × grid dock
1. Compute the transform that puts the shuttle dock one tile out from the
   grid dock, facing it:
   - `stationDockPos = shuttleDock.pos + R(shuttleDock.rot)(0,−1)`;
   - `gridDockAngle = gridDock.rot + π`;
   - `offset = gridDockAngle − shuttleDock.rot`;
   - shuttle origin in grid-local space = `gridDock.pos + R(offset)(−stationDockPos)`.
2. Reject if any shuttle tile's AABB (shrunk 0.01) overlaps a target-grid
   tile (`ValidSpawn`).
3. Reject if the docked bounds hit any other grid.
4. Collect the other dock pairs that land on the same angle and rounded AABB.
5. Choose by priority tag (`PriorityDock`), then most docks, then the angle
   closest to the grid's.
- `FTLDock` then places the shuttle and docks every pair.
- Shipyard purchases (`ShipyardSystem`) call `TryFTLDock` with the console's
  grid. Our starting shuttle does the same.

**ForceAnchor** (Frontier)
- Outposts and POIs have `ForceAnchor`: a static body with its shuttle disabled.
- They never move, and they hold docked ships still.
- Impacts against them rebound instead of exchanging momentum.

## Tests

`crates/mercury/tests/economy.rs::bought_ships_arrive_docked_and_hold_until_undocked`
checks:
- a bought ship docks;
- the dock pairs are within 1.05 m;
- the airlocks are open, bolted and still airtight;
- a shove doesn't move the docked ship;
- after undocking, the ship drifts away and the airlocks unbolt and close.

## Not yet

- Pathfinding portals between docked grids (`TryCreatePortal`).
- `DockEvent`/`UndockEvent` consumers (signals, docking lights).
- The radar dock UI.
- FTL travel itself (only the docking placement).
