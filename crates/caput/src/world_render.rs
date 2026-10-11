//! Grid rendering: floor tiles and decals. Each grid gets a render root that
//! follows the grid's world pose; tiles and decals are children of it, so a
//! moving shuttle carries its floor with it.

use std::collections::HashMap;

use athanor::transform::{Grid, WorldPose};
use athanor::Content;
use bevy::prelude::*;
use grimoire::Node;

use crate::textures::TextureCache;
use crate::{z_for, PX};

/// Links a render root to its simulation grid.
#[derive(Component)]
#[allow(dead_code)]
pub struct GridView {
    pub grid: Entity,
    pub revision: u64,
}

#[derive(Resource, Default)]
pub struct GridViews(pub HashMap<Entity, Entity>);

/// Spawn render roots for grids that do not have one yet, and (re)build
/// their tile sprites when the grid's tiles change.
pub fn sync_grids(
    mut commands: Commands,
    content: Res<Content>,
    mut cache: ResMut<TextureCache>,
    mut images: ResMut<Assets<Image>>,
    mut views: ResMut<GridViews>,
    grids: Query<(Entity, &Grid, &WorldPose, Option<&athanor::spawn::EntityData>)>,
    mut roots: Query<(&mut GridView, &mut Transform)>,
) {
    for (ge, grid, pose, data) in grids.iter() {
        let tf = Transform::from_translation(Vec3::new(pose.pos.x * PX, pose.pos.y * PX, 0.0))
            .with_rotation(Quat::from_rotation_z(pose.rot));
        let root = match views.0.get(&ge) {
            Some(&r) => {
                let Ok((mut view, mut t)) = roots.get_mut(r) else { continue };
                if *t != tf {
                    *t = tf;
                }
                if view.revision == grid.revision {
                    continue;
                }
                view.revision = grid.revision;
                commands.entity(r).despawn_related::<Children>();
                r
            }
            None => {
                let r = commands
                    .spawn((
                        GridView {
                            grid: ge,
                            revision: grid.revision,
                        },
                        tf,
                        Visibility::Visible,
                    ))
                    .id();
                views.0.insert(ge, r);
                r
            }
        };
        build_tiles(&mut commands, root, grid, &content, &mut cache, &mut images);
        if let Some(dg) = data.and_then(|d| d.get("DecalGrid")) {
            build_decals(&mut commands, root, dg, &content, &mut cache, &mut images);
        }
    }
}

fn build_tiles(
    commands: &mut Commands,
    root: Entity,
    grid: &Grid,
    content: &Content,
    cache: &mut TextureCache,
    images: &mut Assets<Image>,
) {
    let z = z_for("FloorTiles");
    let mut children = Vec::new();
    for ((x, y), tile) in grid.tiles.iter() {
        let Some(def) = content.tiles.get(tile.type_id) else { continue };
        let Some(path) = def.sprite_path() else { continue };
        let Some(tex) = cache.image(content, images, &path) else { continue };
        let variants = def.variant_count() as u32;
        let w = tex.size.x / variants.max(1);
        let v = (tile.variant as u32).min(variants - 1);
        let rect = Rect::new((v * w) as f32, 0.0, ((v + 1) * w) as f32, tex.size.y as f32);
        let rot = match tile.rotation_mirroring & 3 {
            1 => std::f32::consts::FRAC_PI_2,
            2 => std::f32::consts::PI,
            3 => -std::f32::consts::FRAC_PI_2,
            _ => 0.0,
        };
        children.push(
            commands
                .spawn((
                    Sprite {
                        image: tex.handle.clone(),
                        rect: Some(rect),
                        custom_size: Some(Vec2::splat(PX)),
                        flip_x: tile.rotation_mirroring & 4 != 0,
                        ..default()
                    },
                    Transform::from_translation(Vec3::new((x as f32 + 0.5) * PX, (y as f32 + 0.5) * PX, z))
                        .with_rotation(Quat::from_rotation_z(rot)),
                ))
                .id(),
        );
    }
    commands.entity(root).add_children(&children);
}

fn build_decals(
    commands: &mut Commands,
    root: Entity,
    decal_grid: &Node,
    content: &Content,
    cache: &mut TextureCache,
    images: &mut Assets<Image>,
) {
    let decals = astrolabe::read_decal_grid(decal_grid);
    let base_z = z_for("FloorTiles") + 0.5;
    let mut children = Vec::new();
    for d in decals {
        let Some(proto) = content.protos.get("decal", &d.id) else { continue };
        let Some(sprite) = proto.get("sprite") else { continue };
        let tex_and_rect = if let (Some(rsi_path), Some(state)) = (sprite.get_str("sprite"), sprite.get_str("state")) {
            let Some(rsi) = cache.rsi(content, rsi_path) else { continue };
            let Some(tex) = cache.state_image(content, images, &rsi, state) else { continue };
            let (x, y, w, h) = rsi.frame_rect(tex.size.x, 0);
            Some((tex, Rect::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32)))
        } else if let Some(t) = sprite.get_str("texture") {
            cache
                .image(content, images, &vellum::ResPath::texture(t))
                .map(|tex| {
                    let r = Rect::new(0.0, 0.0, tex.size.x as f32, tex.size.y as f32);
                    (tex, r)
                })
        } else {
            None
        };
        let Some((tex, rect)) = tex_and_rect else { continue };
        let [r, g, b, a] = d.color;
        children.push(
            commands
                .spawn((
                    Sprite {
                        image: tex.handle,
                        rect: Some(rect),
                        custom_size: Some(Vec2::splat(PX)),
                        color: Color::srgba_u8(r, g, b, a),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(
                        (d.pos.0 + 0.5) * PX,
                        (d.pos.1 + 0.5) * PX,
                        base_z + d.z_index as f32 * 0.01,
                    ))
                    .with_rotation(Quat::from_rotation_z(d.angle as f32)),
                ))
                .id(),
        );
    }
    commands.entity(root).add_children(&children);
}
