//! Entity sprites. Reads each entity's upstream `Sprite` component (RSI,
//! layers, draw depth, noRot...) and renders it with Bevy sprites, applying
//! ports of the client visualizers that matter most:
//!
//! * `IconSmoothSystem` (walls, windows, tables, carpets; see [`crate::smoothing`]),
//! * door visuals (`DoorVisualLayers`: closed/opening/open/closing),
//! * `DamageStateVisuals` (alive/crit/dead mob states),
//! * `HumanoidAppearance` (species body-part sprites with skin colour),
//! * markers and sub-floor pipes/cables hidden like upstream.

use std::collections::HashMap;
use std::sync::Arc;

use athanor::damage::{MobState, MobStateKind};
use athanor::doors::{Door, DoorState};
use athanor::interaction::InContainer;
use athanor::spawn::EntityData;
use athanor::transform::{tile_of, Grid, WorldPose, Xform};
use athanor::Content;
use bevy::prelude::*;
use grimoire::{parse_color, Node};
use vellum::rsi::{rsi_direction_for, RsiDirection};

use crate::smoothing::{self, offset_rsi_dir, Corner, DirOffset, Placement, SmoothIndex, SmoothMode, SmoothSpec, Smoothed};
use crate::textures::TextureCache;
use crate::{z_for, PX};

#[derive(Debug, Clone)]
pub struct LayerSpec {
    pub rsi: Option<String>,
    pub state: Option<String>,
    pub texture: Option<String>,
    pub map: Vec<String>,
    pub visible: bool,
    pub color: [u8; 4],
    pub offset: Vec2,
    pub scale: Vec2,
    pub rotation: f32,
    /// `Layer.DirOffset`: shifts which direction's frame is drawn.
    pub dir_offset: DirOffset,
    /// Icon-smoothing corner this layer shows (`CornerLayers`).
    pub corner: Option<Corner>,
    /// `EdgeLayer` direction flag (hidden when that side has a neighbour).
    pub edge: Option<u8>,
    /// Listed in `SubFloorHide.visibleLayers`: stays drawn under a floor.
    pub subfloor_visible: bool,
    /// Inventory slot layer: (slot name, sub-layer index).
    pub slot: Option<(String, usize)>,
}

#[derive(Debug, Clone)]
pub struct SpriteSpec {
    pub rsi: Option<String>,
    pub layers: Vec<LayerSpec>,
    pub depth: f32,
    pub color: [u8; 4],
    pub no_rot: bool,
    /// `SpriteComponent.SnapCardinals`: rotate in quarter turns relative to the eye.
    pub snap_cardinals: bool,
    pub offset: Vec2,
    pub scale: Vec2,
    pub visible: bool,
    pub smooth: Option<Arc<SmoothSpec>>,
    /// Some layer survives `SubFloorHide` covering (vents, pumps...).
    pub subfloor_any_visible: bool,
}

fn parse_vec2(n: Option<&Node>, d: Vec2) -> Vec2 {
    match n {
        Some(n) => {
            if let Some((x, y)) = n.as_vec2() {
                Vec2::new(x, y)
            } else if let Some(f) = n.as_f32() {
                Vec2::splat(f)
            } else {
                d
            }
        }
        None => d,
    }
}

fn parse_layer(n: &Node) -> LayerSpec {
    LayerSpec {
        rsi: n.get_str("sprite").map(str::to_owned),
        state: n.get_str("state").map(str::to_owned),
        texture: n.get_str("texture").map(str::to_owned),
        map: n.get("map").map(Node::as_str_list).unwrap_or_default(),
        visible: n.get("visible").and_then(Node::as_bool).unwrap_or(true),
        color: n.get_str("color").and_then(parse_color).unwrap_or([255; 4]),
        offset: parse_vec2(n.get("offset"), Vec2::ZERO),
        scale: parse_vec2(n.get("scale"), Vec2::ONE),
        rotation: n.get("rotation").and_then(Node::as_angle).unwrap_or(0.0) as f32,
        dir_offset: DirOffset::None,
        corner: None,
        edge: None,
        subfloor_visible: false,
        slot: None,
    }
}

/// Clothing can have several visual layers per slot; reserve this many.
const SLOT_SUBLAYERS: usize = 3;

impl SpriteSpec {
    pub fn parse(sprite: &Node, smooth: Option<Arc<SmoothSpec>>, subfloor: Option<&Node>) -> SpriteSpec {
        let mut layers: Vec<LayerSpec> = Vec::new();
        for n in sprite.get("layers").and_then(Node::as_seq).unwrap_or_default() {
            let l = parse_layer(n);
            let slot = l.map.iter().find(|m| athanor::inventory::slot_state_suffix(m).is_some()).cloned();
            match slot {
                Some(slot) => {
                    for k in 0..SLOT_SUBLAYERS {
                        let mut sl = l.clone();
                        sl.slot = Some((slot.clone(), k));
                        layers.push(sl);
                    }
                }
                None => layers.push(l),
            }
        }
        if layers.is_empty() && (sprite.get("state").is_some() || sprite.get("texture").is_some()) {
            layers.push(LayerSpec {
                rsi: None,
                state: sprite.get_str("state").map(str::to_owned),
                texture: sprite.get_str("texture").map(str::to_owned),
                map: vec![],
                visible: true,
                color: [255; 4],
                offset: Vec2::ZERO,
                scale: Vec2::ONE,
                rotation: 0.0,
                dir_offset: DirOffset::None,
                corner: None,
                edge: None,
                subfloor_visible: false,
                slot: None,
            });
        }
        if let Some(sm) = &smooth {
            if sm.edge {
                for l in &mut layers {
                    l.edge = l.map.iter().find_map(|m| smoothing::edge_layer_flag(m));
                }
            }
            if sm.mode == SmoothMode::Corners {
                // Frontier's `SetCornerLayers`: reuse layers already mapped to
                // a corner, otherwise append SE, NE, NW, SW.
                let state0 = format!("{}0", sm.base);
                for (key, corner, offset) in smoothing::CORNER_LAYERS {
                    let idx = match layers.iter().position(|l| l.map.iter().any(|m| m == key)) {
                        Some(i) => i,
                        None => {
                            layers.push(parse_layer(&Node::Null));
                            layers.last_mut().unwrap().map = vec![key.to_string()];
                            layers.len() - 1
                        }
                    };
                    let l = &mut layers[idx];
                    l.state = Some(state0.clone());
                    l.corner = Some(corner);
                    l.dir_offset = offset;
                }
            }
        }
        let keep: Vec<String> = subfloor.and_then(|n| n.get("visibleLayers")).map(Node::as_str_list).unwrap_or_default();
        for l in &mut layers {
            l.subfloor_visible = l.map.iter().any(|m| keep.contains(m));
        }
        let subfloor_any_visible = layers.iter().any(|l| l.subfloor_visible);
        SpriteSpec {
            rsi: sprite.get_str("sprite").map(str::to_owned),
            layers,
            depth: z_for(sprite.get_str("drawdepth").unwrap_or("Objects")),
            color: sprite.get_str("color").and_then(parse_color).unwrap_or([255; 4]),
            no_rot: sprite.get("noRot").and_then(Node::as_bool).unwrap_or(false),
            snap_cardinals: sprite.get("snapCardinals").and_then(Node::as_bool).unwrap_or(false),
            offset: parse_vec2(sprite.get("offset"), Vec2::ZERO),
            scale: parse_vec2(sprite.get("scale"), Vec2::ONE),
            visible: sprite.get("visible").and_then(Node::as_bool).unwrap_or(true),
            smooth,
            subfloor_any_visible,
        }
    }
}

/// Layers that upstream visualizers keep hidden unless some state turns
/// them on (bolts, welds, open panels, sparks...).
const HIDDEN_BY_DEFAULT: &[&str] = &[
    "enum.DoorVisualLayers.BaseUnlit",
    "enum.DoorVisualLayers.BaseBolted",
    "enum.DoorVisualLayers.BaseEmergencyAccess",
    "enum.WeldableLayers.BaseWelded",
    "enum.WiresVisualLayers.MaintenancePanel",
    "enum.ElectrifiedLayers.Sparks",
    "enum.ElectrifiedLayers.HUD",
    "enum.LockVisualLayers.Lock",
    "enum.EmagLayers.Emagged",
    "enum.PaperLabelVisuals.Layer",
];

/// Render-side state for one simulation entity.
#[derive(Component)]
pub struct SpriteView {
    pub sim: Entity,
    pub spec: Arc<SpriteSpec>,
    pub layers: Vec<Entity>,
    pub layer_state: Vec<(String, f32)>,
    pub smooth_cache: Option<(SmoothKey, Smoothed)>,
}

/// Everything a smoothing result depends on.
type SmoothKey = (u64, u64, u32, bool, IVec2);

/// sim entity → view entity.
#[derive(Resource, Default)]
pub struct SpriteViews(pub HashMap<Entity, Entity>);

/// Create views for new sprite-bearing sim entities; drop views of deleted
/// ones. Also keeps the [`SmoothIndex`] of icon-smoothing entities.
#[allow(clippy::type_complexity)]
pub fn sync_sprite_views(
    mut commands: Commands,
    mut views: ResMut<SpriteViews>,
    mut index: ResMut<SmoothIndex>,
    new: Query<(Entity, &EntityData), (Added<EntityData>, Without<Grid>)>,
    mut removed: RemovedComponents<EntityData>,
    mut spec_cache: Local<HashMap<usize, (Option<Arc<SpriteSpec>>, Option<Arc<SmoothSpec>>)>>,
) {
    for (e, data) in new.iter() {
        let key = Arc::as_ptr(&data.components) as usize;
        let (spec, smooth) = spec_cache
            .entry(key)
            .or_insert_with(|| {
                let smooth = data.get("IconSmooth").map(|s| Arc::new(SmoothSpec::parse(s, data.has("SmoothEdge"))));
                let spec = data.get("Sprite").map(|sprite| Arc::new(SpriteSpec::parse(sprite, smooth.clone(), data.get("SubFloorHide"))));
                (spec, smooth)
            })
            .clone();
        if let Some(sm) = smooth {
            index.specs.insert(e, sm);
            index.generation += 1;
        }
        let Some(spec) = spec else { continue };
        let n = spec.layers.len();
        let layers: Vec<Entity> = (0..n)
            .map(|_| commands.spawn((Sprite::default(), Transform::default(), Visibility::Hidden)).id())
            .collect();
        let view = commands
            .spawn((
                SpriteView {
                    sim: e,
                    spec,
                    layers: layers.clone(),
                    layer_state: vec![(String::new(), 0.0); n],
                    smooth_cache: None,
                },
                Transform::default(),
                Visibility::Hidden,
            ))
            .add_children(&layers)
            .id();
        views.0.insert(e, view);
    }
    for e in removed.read() {
        if let Some(v) = views.0.remove(&e) {
            commands.entity(v).despawn();
        }
        if index.specs.remove(&e).is_some() {
            index.generation += 1;
        }
    }
}

/// What the client `DoorSystem` + `AirlockSystem` appearance handlers decide.
struct DoorVisual {
    /// `DoorVisualLayers.Base` state.
    base: String,
    /// `DoorVisualLayers.BaseUnlit`: (state, visible).
    unlit: (String, bool),
    bolted: bool,
    emergency: bool,
    /// `OpenDrawDepth` / `ClosedDrawDepth` (both `Doors` by default).
    depth: f32,
    /// Base/unlit states are one-shot flicks (opening, closing, deny).
    flick: bool,
}

fn door_visual(d: &Door, data: &EntityData, powered: bool) -> DoorVisual {
    let yaml = data.get("Door");
    let get = |k: &str, def: &str| yaml.and_then(|n| n.get_str(k)).unwrap_or(def).to_string();
    let open = d.state == DoorState::Open;
    let depth_key = if open { "openDrawDepth" } else { "closedDrawDepth" };
    let depth = z_for(yaml.and_then(|n| n.get_str(depth_key)).unwrap_or("Doors"));
    let base = match d.state {
        DoorState::Open => get("openSpriteState", "open"),
        DoorState::Opening => get("openingSpriteState", "opening"),
        DoorState::Closing => get("closingSpriteState", "closing"),
        _ => get("closedSpriteState", "closed"),
    };
    let airlock = data.get("Airlock");
    let aget = |k: &str, def: &str| airlock.and_then(|n| n.get_str(k)).unwrap_or(def).to_string();
    let abool = |k: &str, def: bool| airlock.and_then(|n| n.get(k)).and_then(Node::as_bool).unwrap_or(def);
    let (mut unlit_visible, mut bolted, mut emergency) = (false, false, false);
    if airlock.is_some() && powered {
        bolted = d.bolted && matches!(d.state, DoorState::Closed | DoorState::Welded);
        emergency = abool("emergencyAccess", false);
        unlit_visible = (matches!(d.state, DoorState::Closing | DoorState::Opening | DoorState::Denying)
            || (open && abool("openUnlitVisible", false)))
            && !bolted
            && !emergency;
    }
    let emergency = emergency
        && abool("emergencyAccessLayer", true)
        && !matches!(d.state, DoorState::Open | DoorState::Opening | DoorState::Closing)
        && !bolted;
    let unlit_state = match d.state {
        DoorState::Opening => aget("openingSpriteState", "opening_unlit"),
        DoorState::Closing | DoorState::Open => aget("closingSpriteState", "closing_unlit"),
        DoorState::Denying => aget("denySpriteState", "deny_unlit"),
        _ => aget("openingSpriteState", "opening_unlit"),
    };
    let flick = matches!(d.state, DoorState::Opening | DoorState::Closing | DoorState::Denying);
    DoorVisual { base, unlit: (unlit_state, unlit_visible), bolted, emergency, depth, flick }
}

#[allow(clippy::too_many_arguments)]
pub fn update_sprites(
    content: Res<Content>,
    time: Res<Time>,
    mut cache: ResMut<TextureCache>,
    mut images: ResMut<Assets<Image>>,
    mut views: Query<(&mut SpriteView, &mut Transform, &mut Visibility), Without<Sprite>>,
    mut layers: Query<(&mut Sprite, &mut Transform, &mut Visibility), Without<SpriteView>>,
    sims: Query<(
        &WorldPose,
        &Xform,
        &EntityData,
        Option<&Door>,
        Option<&MobState>,
        Has<InContainer>,
        Option<&athanor::storage::EntityStorage>,
        Has<athanor::power::Unpowered>,
        Option<&prana::nadi::CableVis>,
    )>,
    grids: Query<&Grid>,
    camera: Query<&Transform, (With<crate::input::MainCamera>, Without<SpriteView>, Without<Sprite>)>,
    inventories: Query<&athanor::inventory::Inventory>,
    clothes: Query<(&athanor::inventory::Clothing, &EntityData)>,
    smooth_index: Res<SmoothIndex>,
    xforms: Query<&Xform>,
) {
    let now = time.elapsed_secs();

    let cam_rot = camera.single().map_or(0.0, |t| t.rotation.to_euler(EulerRot::ZYX).0);
    for (mut view, mut root_tf, mut root_vis) in views.iter_mut() {
        let Ok((pose, xform, data, door, mob, contained, estorage, unpowered, cable_vis)) = sims.get(view.sim) else {
            continue;
        };
        let spec = view.spec.clone();
        let parent_grid = xform.parent.and_then(|p| grids.get(p).ok());
        // `SubFloorHideSystem`: anchored on a non-subfloor tile = covered;
        // only `visibleLayers` stay drawn.
        let covered = data.has("SubFloorHide")
            && xform.anchored
            && parent_grid.is_some_and(|g| {
                let t = g.tiles.get(tile_of(xform.pos).x, tile_of(xform.pos).y);
                content.tiles.get(t.type_id).is_some_and(|d| !d.is_subfloor)
            });
        // It also owns sprite visibility (`SetVisible(hasVisibleLayer || revealed)`),
        // overriding the `visible: false` pipes and vents start with.
        let sprite_visible = if data.has("SubFloorHide") { !covered || spec.subfloor_any_visible } else { spec.visible };
        let hidden = !sprite_visible || contained || data.has("Marker");
        let want_vis = if hidden { Visibility::Hidden } else { Visibility::Visible };
        if *root_vis != want_vis {
            *root_vis = want_vis;
        }
        if hidden {
            continue;
        }

        // Mobs that are down lie on their side, like upstream's standing state.
        let lying = mob.is_some_and(|m| m.is_incapacitated()) && data.has("HumanoidAppearance");
        // `SpriteSystem.RenderSprite`: `screen` is the entity's angle on screen
        // (world rotation relative to the camera). noRot sprites stay upright
        // on screen; snapCardinals ones turn in quarter steps of it.
        let screen = pose.rot - cam_rot;
        let base_rot = if spec.no_rot {
            cam_rot
        } else if spec.snap_cardinals {
            pose.rot - round_to_cardinal(screen)
        } else {
            pose.rot
        };
        let door_vis = door.map(|d| door_visual(d, data, !unpowered));
        let depth = if mob.is_some_and(|m| m.0 == MobStateKind::Dead) {
            z_for("DeadMobs")
        } else if let Some(dv) = &door_vis {
            dv.depth
        } else {
            spec.depth
        };
        // Within a depth, things further south draw on top.
        let z = depth - pose.pos.y * 0.0001;
        *root_tf = Transform::from_translation(Vec3::new(
            (pose.pos.x + spec.offset.x) * PX,
            (pose.pos.y + spec.offset.y) * PX,
            z,
        ))
        .with_rotation(Quat::from_rotation_z(base_rot + if lying { std::f32::consts::FRAC_PI_2 } else { 0.0 }))
        .with_scale(spec.scale.extend(1.0));

        // Icon smoothing (recomputed only when the grid or the index changes).
        let tile = tile_of(xform.pos);
        let smoothed = match &spec.smooth {
            Some(sm) => {
                let grid = if xform.anchored { parent_grid } else { None };
                let key = (
                    grid.map_or(0, |g| g.revision),
                    smooth_index.generation,
                    xform.rot.to_bits(),
                    xform.anchored,
                    tile,
                );
                if !matches!(&view.smooth_cache, Some((k, _)) if *k == key) {
                    let at = Placement { grid, tile, rot: xform.rot };
                    let s = smoothing::calculate(sm, &at, &smooth_index, |e| xforms.get(e).map_or(0.0, |x| x.rot));
                    view.smooth_cache = Some((key, s));
                }
                view.smooth_cache.as_ref().map(|(_, s)| s.clone())
            }
            None => None,
        };

        let damage_states = mob.and_then(|m| {
            let dsv = data.get("DamageStateVisuals")?.get("states")?;
            let key = match m.0 {
                MobStateKind::Alive => "Alive",
                MobStateKind::Critical => "Critical",
                MobStateKind::Dead => "Dead",
                MobStateKind::Invalid => "Alive",
            };
            dsv.get(key).or_else(|| if key == "Critical" { dsv.get("Dead") } else { None }).cloned()
        });
        let humanoid = data.get("HumanoidAppearance");

        for (i, layer) in spec.layers.iter().enumerate() {
            let Some(&le) = view.layers.get(i) else { continue };
            let Ok((mut sprite, mut tf, mut vis)) = layers.get_mut(le) else { continue };
            let mut state = layer.state.clone();
            let mut rsi_path = layer.rsi.clone().or_else(|| spec.rsi.clone());
            let mut visible = layer.visible && !layer.map.iter().any(|m| HIDDEN_BY_DEFAULT.contains(&m.as_str()));
            let mut color = layer.color;

            // Door visuals.
            let mut flick = false;
            if let (Some(d), Some(dv)) = (door, &door_vis) {
                let has = |k: &str| layer.map.iter().any(|m| m == k);
                if has("enum.DoorVisualLayers.Base") || (layer.map.is_empty() && i == 0 && state.as_deref() == Some("closed")) {
                    state = Some(dv.base.clone());
                    flick = dv.flick;
                }
                if has("enum.DoorVisualLayers.BaseUnlit") {
                    state = Some(dv.unlit.0.clone());
                    visible = dv.unlit.1;
                    flick = dv.flick;
                }
                if has("enum.WeldableLayers.BaseWelded") {
                    visible = d.state == DoorState::Welded;
                }
                if has("enum.DoorVisualLayers.BaseBolted") {
                    visible = dv.bolted;
                }
                if has("enum.DoorVisualLayers.BaseEmergencyAccess") {
                    visible = dv.emergency;
                }
            }
            // EntityStorageVisualizerSystem: lockers and crates.
            if let (Some(es), Some(v)) = (estorage, data.get("EntityStorageVisuals")) {
                if layer.map.iter().any(|m| m == "enum.StorageVisualLayers.Base") {
                    let key = if es.open { "stateBaseOpen" } else { "stateBaseClosed" };
                    if let Some(st) = v.get_str(key) {
                        state = Some(st.to_string());
                    }
                }
                if layer.map.iter().any(|m| m == "enum.StorageVisualLayers.Door") {
                    let key = if es.open { "stateDoorOpen" } else { "stateDoorClosed" };
                    match v.get_str(key) {
                        Some(st) => {
                            state = Some(st.to_string());
                            visible = true;
                        }
                        None => visible = false,
                    }
                }
            }
            // Mob damage states.
            if let Some(ds) = &damage_states {
                for m in &layer.map {
                    let short = m.rsplit('.').next().unwrap_or(m);
                    if let Some(s) = ds.get_str(short) {
                        state = Some(s.to_string());
                    }
                }
            }
            // Humanoid body parts from species base sprites.
            if let Some(h) = humanoid
                && state.is_none()
                && let Some(part) = layer.map.iter().find_map(|m| m.strip_prefix("enum.HumanoidVisualLayers."))
            {
                match humanoid_part(&content, h, part) {
                    Some((rsi, st, skin)) => {
                        rsi_path = Some(rsi);
                        state = Some(st);
                        if skin {
                            color = h.get_str("skinColor").and_then(parse_color).unwrap_or([0xC0, 0x96, 0x7F, 0xFF]);
                        } else if part == "Eyes" {
                            color = h.get_str("eyeColor").and_then(parse_color).unwrap_or([0x52, 0x3A, 0x26, 0xFF]);
                        }
                    }
                    None => visible = false,
                }
            }
            // Icon smoothing states and edges.
            if let Some(sm) = &smoothed {
                if let (Some(c), Some(corner)) = (sm.corners, layer.corner)
                    && let Some(spec_sm) = &spec.smooth
                {
                    state = Some(format!("{}{}", spec_sm.base, c[corner as usize]));
                }
                if i == 0
                    && let Some(s0) = &sm.layer0
                {
                    state = Some(s0.clone());
                }
                if let (Some(flags), Some(edge)) = (sm.edges, layer.edge) {
                    visible = flags & edge == 0;
                }
            }

            if covered {
                visible = layer.subfloor_visible;
            }
            // `CableVisualizerSystem`: `{statePrefix}{mask}` on layer 0,
            // `{extraLayerPrefix}{mask}` on layer 1.
            if let (Some(cv), Some(viz)) = (cable_vis, data.get("CableVisualizer")) {
                let prefix = match i {
                    0 => viz.get_str("statePrefix"),
                    1 => viz.get_str("extraLayerPrefix"),
                    _ => None,
                };
                if let Some(p) = prefix {
                    state = Some(format!("{p}{}", cv.mask));
                }
            }
            // `PipeColorVisualizerSystem`: `AtmosPipeColor` tints the pipe layer.
            if data.has("PipeColorVisuals")
                && layer.map.iter().any(|m| m == "enum.PipeVisualLayers.Pipe")
                && let Some([r, g, b, _]) = data.get("AtmosPipeColor").and_then(|n| n.get_str("color")).and_then(parse_color)
            {
                color = [r, g, b, color[3]];
            }

            // Equipped clothing (`ClientClothingSystem`).
            if let Some((slot, k)) = &layer.slot {
                visible = false;
                if let Ok(inv) = inventories.get(view.sim)
                    && let Some(item) = inv.get(slot)
                    && let Ok((cl, idata)) = clothes.get(item)
                {
                    let item_rsi = cl
                        .sprite
                        .clone()
                        .or_else(|| idata.get("Sprite").and_then(|s| s.get_str("sprite")).map(str::to_owned));
                    let resolved: Vec<(Option<String>, String, Option<String>)> = match cl.visuals_for(slot) {
                        Some(v) => v.iter().map(|l| (l.rsi.clone().or(item_rsi.clone()), l.state.clone(), l.color.clone())).collect(),
                        None => {
                            let suffix = athanor::inventory::slot_state_suffix(slot).unwrap_or("");
                            let mut st = match (&cl.equipped_state, &cl.equipped_prefix) {
                                (Some(s), _) => s.clone(),
                                (None, Some(p)) => format!("{p}-equipped-{suffix}"),
                                (None, None) => format!("equipped-{suffix}"),
                            };
                            let species = humanoid.and_then(|h| h.get_str("species")).unwrap_or("Human");
                            if let Some(r) = item_rsi.as_deref().and_then(|r| cache.rsi(&content, r))
                                && r.state(&format!("{st}-{species}")).is_some()
                            {
                                st = format!("{st}-{species}");
                            }
                            vec![(item_rsi.clone(), st, None)]
                        }
                    };
                    if let Some((r, st, c)) = resolved.get(*k) {
                        rsi_path = r.clone();
                        state = Some(st.clone());
                        color = c.as_deref().and_then(parse_color).unwrap_or([255; 4]);
                        visible = true;
                    }
                }
            }

            // Resolve the frame.
            let mut drawn = false;
            if visible {
                if let Some(tex_path) = &layer.texture {
                    if let Some(tex) = cache.image(&content, &mut images, &vellum::ResPath::texture(tex_path)) {
                        sprite.image = tex.handle.clone();
                        sprite.rect = None;
                        sprite.custom_size = Some(tex.size.as_vec2() * layer.scale);
                        drawn = true;
                    }
                } else if let (Some(rp), Some(st)) = (&rsi_path, &state)
                    && let Some(rsi) = cache.rsi(&content, rp)
                    && let Some(rs) = rsi.state(st)
                    && let Some(tex) = cache.state_image(&content, &mut images, &rsi, st)
                {
                    // `RenderLayer`: the on-screen angle picks the direction that
                    // sets the layer matrix; `DirOffset` then shifts the frame.
                    let angle_dir = rsi_direction_for(rs.directions, screen as f64);
                    let dir = offset_rsi_dir(angle_dir, layer.dir_offset);
                    // Restart one-shot animations (door opening) on state change.
                    if view.layer_state[i].0 != *st {
                        view.layer_state[i] = (st.clone(), now);
                    }
                    let since = now - view.layer_state[i].1;
                    let frame = if flick {
                        let mut t = since;
                        let delays = &rs.delays[(dir as usize).min(rs.directions as usize - 1)];
                        let mut f = 0;
                        while f + 1 < delays.len() && t >= delays[f] {
                            t -= delays[f];
                            f += 1;
                        }
                        f
                    } else {
                        rs.frame_at(dir, now)
                    };
                    let idx = rs.frame_index(dir, frame);
                    let (x, y, w, h) = rsi.frame_rect(tex.size.x, idx);
                    sprite.image = tex.handle.clone();
                    sprite.rect = Some(Rect::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32));
                    sprite.custom_size = Some(Vec2::new(w as f32, h as f32) * layer.scale);
                    drawn = true;
                    // `GetLayerDrawMatrix`: a direction frame is counter-rotated by
                    // its direction's angle, leaving only the residual rotation.
                    let undo = if rs.directions > 1 && !spec.no_rot { -rsi_dir_angle(angle_dir) } else { 0.0 };
                    *tf = Transform::from_translation(Vec3::new(layer.offset.x * PX, layer.offset.y * PX, i as f32 * 0.001))
                        .with_rotation(Quat::from_rotation_z(layer.rotation + undo));
                }
            }
            if drawn {
                let [r, g, b, a] = color;
                let [sr, sg, sb, sa] = spec.color;
                sprite.color = Color::srgba(
                    (r as f32 / 255.0) * (sr as f32 / 255.0),
                    (g as f32 / 255.0) * (sg as f32 / 255.0),
                    (b as f32 / 255.0) * (sb as f32 / 255.0),
                    (a as f32 / 255.0) * (sa as f32 / 255.0),
                );
            }
            let want = if drawn { Visibility::Inherited } else { Visibility::Hidden };
            if *vis != want {
                *vis = want;
            }
        }
    }
}

/// Species base sprite for a humanoid layer: (rsi, state, uses skin colour).
fn humanoid_part(content: &Content, appearance: &Node, part: &str) -> Option<(String, String, bool)> {
    let species = appearance.get_str("species").unwrap_or("Human");
    let sp = content.protos.get("species", species)?;
    let sprites_id = sp.get_str("sprites")?;
    let base_sprites = content.protos.get("speciesBaseSprites", sprites_id)?;
    let part_proto = base_sprites.get("sprites")?.get_str(part)?;
    let hbs = content.protos.get("humanoidBaseSprite", part_proto)?;
    let bs = hbs.get("baseSprite")?;
    let skin = !matches!(part, "Eyes" | "Hair" | "FacialHair" | "Special" | "UndergarmentTop" | "UndergarmentBottom");
    Some((bs.get_str("sprite")?.to_string(), bs.get_str("state")?.to_string(), skin))
}

/// `Angle.RoundToCardinalAngle`.
fn round_to_cardinal(a: f32) -> f32 {
    (a / std::f32::consts::FRAC_PI_2).round() * std::f32::consts::FRAC_PI_2
}

/// `Direction.ToAngle()` for an RSI direction (south = 0, counter-clockwise).
fn rsi_dir_angle(d: RsiDirection) -> f32 {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    match d {
        RsiDirection::South => 0.0,
        RsiDirection::SouthEast => FRAC_PI_4,
        RsiDirection::East => FRAC_PI_2,
        RsiDirection::NorthEast => 3.0 * FRAC_PI_4,
        RsiDirection::North => PI,
        RsiDirection::NorthWest => 5.0 * FRAC_PI_4,
        RsiDirection::West => 3.0 * FRAC_PI_2,
        RsiDirection::SouthWest => 7.0 * FRAC_PI_4,
    }
}
