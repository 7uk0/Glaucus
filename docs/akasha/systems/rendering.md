# Rendering (client sprites)

**What it's for:** the game is read entirely through sprites. Walls that don't
join, doors drawn under their firelocks or vents missing from the floor make
a ship unreadable. Port the engine's and Content's visual rules exactly.

**How we check it:** `caput-mortuum --render-map /Maps/... --screenshot out.png`
renders a map 1:1 (32 px per tile, no HUD, black background) for comparison
with official renders (`reference/wiki/frontier/images/{Surveyor,Druid,Apothecary}.png`).

Official renders come from `Content.MapRenderer`. Its output differs from the
game in known ways:
- It draws ships **unpowered**: screens off, no door lights.
- `DecalPainter` sorts decals by zIndex only, with an unstable sort. The game's
  `DecalOverlay` sorts by (zIndex, decal id). We follow the game.
- Some wiki renders are of older map versions (Druid, Apothecary sizes differ).

## Engine rules (RobustToolbox `SpriteSystem.Render.cs`)

- The on-screen angle is world rotation + eye rotation. It picks each layer's
  RSI direction: 4-dir states use `GetCardinalDir`, 8-dir states use `GetDir`.
- `noRot`: the sprite stays upright on screen.
- `snapCardinals`: the sprite rotates by world rotation − RoundToCardinal(angle).
- `GetLayerDrawMatrix(dir)`: unless the direction is South or the sprite is
  noRot, the layer is counter-rotated by −dir.ToAngle(). **`dir` here is the
  angle-derived direction, before `DirOffset`.** The offset only chooses which
  frame is drawn: `dir.OffsetRsiDir(layer.DirOffset)`. We once applied the
  offset direction to the matrix, which broke every smoothed wall corner.
- Draw order (`Clyde.HLR`):
  1. every grid's tiles (`_drawGrids`);
  2. `WorldSpaceBelowEntities` overlays (decals);
  3. entity sprites, sorted by draw depth.

  We use `Z_TILES = -400` and `Z_DECALS = -350..-310`, below every
  `DrawDepth` (LowFloors = −20 ×10).
- `DrawDepth` values: `Content.Shared/DrawDepth/DrawDepth.cs`, from
  LowFloors −20 to Overlays +13. YAML can give raw integers
  (`ConstantSerializer`), e.g. `openDrawDepth: 0`.

## IconSmooth (`Content.Client/IconSmoothing/IconSmoothSystem.cs` → `caput/src/smoothing.rs`)

- Modes:
  - **Corners** (default): adds four layers SE, NE, NW, SW, each `{base}0`,
    with DirOffsets None, CCW, Flip and CW.
  - **CardinalFlags:** layer 0 shows `{base}{N1|S2|E4|W8}`.
  - **Diagonal:** layer 0 shows `{base}1` if the E, SE and S neighbours
    (rotated by LocalRotation) all match.
  - **DiagonalNF** (Frontier): layer 0 shows `{base}{0..4}`. E sets 1, S
    sets 2, and when both are set, a matching SE neighbour makes it 4.
  - **NoSprite:** no sprite changes, edges only.
- Frontier's `SetCornerLayers` reuses YAML layers already mapped to
  `enum.CornerLayers.*` (ore walls put an overlay above them).
- Corner fill: CCW=1, Diagonal=2, CW=4. The (NE, NW, SW, SE) tuple is rotated
  by the entity's cardinal LocalRotation:
  - North → (SW, SE, NE, NW);
  - West → (SE, NE, NW, SW);
  - South → as is;
  - East → (NW, SW, SE, NE).
- `MatchingEntity` checks the **anchored** entities on the neighbour tile. A
  neighbour matches when:
  - its key equals ours or is in our `additionalKeys`;
  - it is `enabled`;
  - it passes Frontier's `EntityIsSmoothOnEdge`: a Diagonal or DiagonalNF
    target only smooths on its own South and East sides.
- An unanchored entity shows state 0.
- `SmoothEdge` (asteroid rock, facades) shows `enum.EdgeLayer.{South,East,North,West}`
  layers only where no matching neighbour exists.

## Content visualizers ported (all in `caput/src/sprites.rs`)

- **SubFloorHide** (`Content.Client/SubFloor/SubFloorHideSystem.cs`)
  - An entity is covered when it is anchored on a tile whose def has
    `isSubfloor: false`.
  - When covered, only layers in `visibleLayers` are drawn (e.g.
    `enum.SubfloorLayers.FirstLayer` on vents and pumps).
  - The system owns sprite visibility, overriding the `visible: false` that
    pipes and cables start with.
- **Doors** (`Content.Client/Doors/DoorSystem.cs`, `AirlockSystem.cs`)
  - Draw depth is `openDrawDepth` when open, otherwise `closedDrawDepth`; both
    default to `Doors`. Firelocks use `openDrawDepth: WallTops`, so an open
    firelock sits under the airlock.
  - Base layer state comes from the YAML open/closed/opening/closing state names.
  - The airlock unlit layer is shown only when powered and Opening, Closing or
    Denying (or Open with `openUnlitVisible`).
  - Bolt lights show when powered, bolted, and Closed or Welded.
- **Cables** (`CableVisualizerSystem` + server `CableVisSystem`)
  - Layer 0 shows `{statePrefix}{mask}`, where the mask is `WireVisDirFlags`
    (N1 S2 E4 W8) of reachable cables on other tiles.
  - prana computes the mask in `nadi::build` and stores it in `nadi::CableVis`.
- **Pipes** (`PipeColorVisualizerSystem`): `AtmosPipeColor.color` tints the
  `enum.PipeVisualLayers.Pipe` layer.
- **Decals:** drawn in (zIndex, id) order.

## Not yet ported

- `GenericVisualizer` (appearance-data driven layer states, e.g. scrubber modes).
- `PipeAppearance` connection stubs.
- Frontier `AtmosPipeLayers` offsets.
- Firelock warning lights.
- Snap-cardinal decals on rotated grids.
- Lighting (`PointLight`, unshaded layers).
- Solar panel rotation.
