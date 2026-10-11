//! Entity sprites. Reads each entity's upstream `Sprite` component (RSI,
//! layers, draw depth, noRot...) and renders it with Bevy sprites, applying
//! ports of the client visualizers that matter most:
//!
//! * `IconSmoothSystem` (walls, windows, tables: 4 corner layers),
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
    /// Fixed RSI direction (icon-smoothing corners).
    pub fixed_dir: Option<RsiDirection>,
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
    pub offset: Vec2,
    pub scale: Vec2,
    pub visible: bool,
    pub smooth: Option<SmoothSpec>,
}

#[derive(Debug, Clone)]
pub struct SmoothSpec {
    pub key: String,
    pub base: String,
    pub mode: String,
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
        fixed_dir: None,
        slot: None,
    }
}

/// Clothing can have several visual layers per slot; reserve this many.
const SLOT_SUBLAYERS: usize = 3;

impl SpriteSpec {
    pub fn parse(sprite: &Node, data: &EntityData) -> SpriteSpec {
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
                fixed_dir: None,
                slot: None,
            });
        }
        let smooth = data.get("IconSmooth").and_then(|s| {
            Some(SmoothSpec {
                key: s.get_str("key")?.to_string(),
                base: s.get_str("base").unwrap_or("").to_string(),
                mode: s.get_str("mode").unwrap_or("Corners").to_string(),
            })
        });
        if let Some(sm) = &smooth
            && sm.mode == "Corners"
        {
            // SetCornerLayers: SE, NE, NW, SW with direction offsets.
            for dir in [RsiDirection::South, RsiDirection::East, RsiDirection::North, RsiDirection::West] {
                layers.push(LayerSpec {
                    rsi: None,
                    state: Some(format!("{}0", sm.base)),
                    texture: None,
                    map: vec![format!("corner:{}", dir as u8)],
                    visible: true,
                    color: [255; 4],
                    offset: Vec2::ZERO,
                    scale: Vec2::ONE,
                    rotation: 0.0,
                    fixed_dir: Some(dir),
                    slot: None,
                });
            }
        }
        SpriteSpec {
            rsi: sprite.get_str("sprite").map(str::to_owned),
            layers,
            depth: z_for(sprite.get_str("drawdepth").unwrap_or("Objects")),
            color: sprite.get_str("color").and_then(parse_color).unwrap_or([255; 4]),
            no_rot: sprite.get("noRot").and_then(Node::as_bool).unwrap_or(false),
            offset: parse_vec2(sprite.get("offset"), Vec2::ZERO),
            scale: parse_vec2(sprite.get("scale"), Vec2::ONE),
            visible: sprite.get("visible").and_then(Node::as_bool).unwrap_or(true),
            smooth,
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
];

/// Render-side state for one simulation entity.
#[derive(Component)]
pub struct SpriteView {
    pub sim: Entity,
    pub spec: Arc<SpriteSpec>,
    pub layers: Vec<Entity>,
    pub layer_state: Vec<(String, f32)>,
    pub smooth_cache: Option<(u64, [u8; 4])>,
}

/// sim entity → view entity.
#[derive(Resource, Default)]
pub struct SpriteViews(pub HashMap<Entity, Entity>);

/// Create views for new sprite-bearing sim entities; drop views of deleted ones.
pub fn sync_sprite_views(
    mut commands: Commands,
    mut views: ResMut<SpriteViews>,
    new: Query<(Entity, &EntityData), (Added<EntityData>, Without<Grid>)>,
    all: Query<(), With<EntityData>>,
    mut spec_cache: Local<HashMap<usize, Arc<SpriteSpec>>>,
) {
    for (e, data) in new.iter() {
        let Some(sprite) = data.get("Sprite") else { continue };
        let key = Arc::as_ptr(&data.components) as usize;
        let spec = spec_cache.entry(key).or_insert_with(|| Arc::new(SpriteSpec::parse(sprite, data))).clone();
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
    let dead: Vec<Entity> = views.0.keys().copied().filter(|e| !all.contains(*e)).collect();
    for e in dead {
        if let Some(v) = views.0.remove(&e) {
            commands.entity(v).despawn();
        }
    }
}

fn door_state_name(d: &Door) -> &'static str {
    match d.state {
        DoorState::Opening => "opening",
        DoorState::Open => "open",
        DoorState::Closing => "closing",
        DoorState::Denying => "deny",
        _ => "closed",
    }
}

fn smooth_neighbours(grid: &Grid, tile: IVec2, key: &str, smooth_keys: &HashMap<Entity, String>) -> [bool; 8] {
    // N, NE, E, SE, S, SW, W, NW
    let offs = [
        IVec2::new(0, 1),
        IVec2::new(1, 1),
        IVec2::new(1, 0),
        IVec2::new(1, -1),
        IVec2::new(0, -1),
        IVec2::new(-1, -1),
        IVec2::new(-1, 0),
        IVec2::new(-1, 1),
    ];
    let mut out = [false; 8];
    for (i, o) in offs.iter().enumerate() {
        out[i] = grid
            .anchored_at(tile + *o)
            .iter()
            .any(|e| smooth_keys.get(e).is_some_and(|k| k == key));
    }
    out
}

/// `CalculateCornerFill` → states for corners (SE, NE, NW, SW).
fn corner_fill(n: [bool; 8]) -> [u8; 4] {
    let [north, ne, east, se, south, sw, west, nw] = n;
    let (ccw, diag, cw) = (1u8, 2u8, 4u8);
    let mut c_ne = 0;
    let mut c_se = 0;
    let mut c_sw = 0;
    let mut c_nw = 0;
    if north {
        c_ne |= ccw;
        c_nw |= cw;
    }
    if ne {
        c_ne |= diag;
    }
    if east {
        c_ne |= cw;
        c_se |= ccw;
    }
    if se {
        c_se |= diag;
    }
    if south {
        c_se |= cw;
        c_sw |= ccw;
    }
    if sw {
        c_sw |= diag;
    }
    if west {
        c_sw |= cw;
        c_nw |= ccw;
    }
    if nw {
        c_nw |= diag;
    }
    [c_se, c_ne, c_nw, c_sw]
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
    )>,
    grids: Query<&Grid>,
    inventories: Query<&athanor::inventory::Inventory>,
    clothes: Query<(&athanor::inventory::Clothing, &EntityData)>,
    smoothers: Query<(Entity, &EntityData)>,
    mut smooth_keys: Local<HashMap<Entity, String>>,
    mut smooth_keys_frame: Local<u32>,
) {
    let now = time.elapsed_secs();
    // Refresh the smoothing key table occasionally (it only changes when
    // things are built or destroyed).
    *smooth_keys_frame = smooth_keys_frame.wrapping_add(1);
    if smooth_keys.is_empty() || (*smooth_keys_frame).is_multiple_of(120) {
        smooth_keys.clear();
        for (e, d) in smoothers.iter() {
            if let Some(k) = d.get("IconSmooth").and_then(|s| s.get_str("key")) {
                smooth_keys.insert(e, k.to_string());
            }
        }
    }

    for (mut view, mut root_tf, mut root_vis) in views.iter_mut() {
        let Ok((pose, xform, data, door, mob, contained)) = sims.get(view.sim) else {
            continue;
        };
        let spec = view.spec.clone();
        let parent_grid = xform.parent.and_then(|p| grids.get(p).ok());
        let hidden = !spec.visible
            || contained
            || data.has("Marker")
            || (data.has("SubFloorHide")
                && parent_grid.is_some_and(|g| {
                    let t = g.tiles.get(tile_of(xform.pos).x, tile_of(xform.pos).y);
                    content.tiles.get(t.type_id).is_some_and(|d| !d.is_subfloor)
                }));
        let want_vis = if hidden { Visibility::Hidden } else { Visibility::Visible };
        if *root_vis != want_vis {
            *root_vis = want_vis;
        }
        if hidden {
            continue;
        }

        // Mobs that are down lie on their side, like upstream's standing state.
        let lying = mob.is_some_and(|m| m.is_incapacitated()) && data.has("HumanoidAppearance");
        let base_rot = if spec.no_rot { 0.0 } else { pose.rot };
        let depth = if mob.is_some_and(|m| m.0 == MobStateKind::Dead) { z_for("DeadMobs") } else { spec.depth };
        // Within a depth, things further south draw on top.
        let z = depth - pose.pos.y * 0.0001;
        *root_tf = Transform::from_translation(Vec3::new(
            (pose.pos.x + spec.offset.x) * PX,
            (pose.pos.y + spec.offset.y) * PX,
            z,
        ))
        .with_rotation(Quat::from_rotation_z(base_rot + if lying { std::f32::consts::FRAC_PI_2 } else { 0.0 }))
        .with_scale(spec.scale.extend(1.0));

        // Icon smoothing corners.
        let corners = match (&spec.smooth, parent_grid) {
            (Some(sm), Some(g)) if sm.mode == "Corners" => {
                let rev = g.revision;
                match view.smooth_cache {
                    Some((r, c)) if r == rev => Some(c),
                    _ => {
                        let c = corner_fill(smooth_neighbours(g, tile_of(xform.pos), &sm.key, &smooth_keys));
                        view.smooth_cache = Some((rev, c));
                        Some(c)
                    }
                }
            }
            _ => None,
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
            if let Some(d) = door {
                let is_base = layer.map.iter().any(|m| m == "enum.DoorVisualLayers.Base")
                    || (layer.map.is_empty() && i == 0 && state.as_deref() == Some("closed"));
                if is_base {
                    state = Some(door_state_name(d).to_string());
                }
                if layer.map.iter().any(|m| m == "enum.WeldableLayers.BaseWelded") {
                    visible = d.state == DoorState::Welded;
                }
                if layer.map.iter().any(|m| m == "enum.DoorVisualLayers.BaseBolted") {
                    visible = d.bolted;
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
            // Corner states for smoothed sprites.
            let fixed_dir = layer.fixed_dir;
            if let (Some(c), Some(dir)) = (corners, fixed_dir)
                && let Some(sm) = &spec.smooth
            {
                let idx = match dir {
                    RsiDirection::South => 0,
                    RsiDirection::East => 1,
                    RsiDirection::North => 2,
                    _ => 3,
                };
                state = Some(format!("{}{}", sm.base, c[idx]));
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
                    let dir = fixed_dir.unwrap_or_else(|| rsi_direction_for(rs.directions, pose.rot as f64));
                    // Restart one-shot animations (door opening) on state change.
                    if view.layer_state[i].0 != *st {
                        view.layer_state[i] = (st.clone(), now);
                    }
                    let since = now - view.layer_state[i].1;
                    let frame = if door.is_some() && matches!(st.as_str(), "opening" | "closing") {
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
                    // Directional states are not rotated with the entity.
                    let undo = if rs.directions > 1 && !spec.no_rot { -pose.rot } else { 0.0 };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_match_upstream() {
        // Lone wall: all corners empty.
        assert_eq!(corner_fill([false; 8]), [0, 0, 0, 0]);
        // Wall with a neighbour to the north: NE gets CCW(1), NW gets CW(4).
        let mut n = [false; 8];
        n[0] = true;
        assert_eq!(corner_fill(n), [0, 1, 4, 0]);
        // Fully surrounded: every corner 7.
        assert_eq!(corner_fill([true; 8]), [7, 7, 7, 7]);
    }
}
