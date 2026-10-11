//! Space parallax, after upstream's `Default` parallax prototype: the
//! `layer1.png` nebula plus two procedurally generated star fields (upstream
//! generates its star layers from TOML noise configs; we scatter stars with
//! a seeded RNG), each scrolling with its own `slowness`.

use athanor::rng::SimRng;
use athanor::Content;
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use vellum::ResPath;

use crate::input::MainCamera;
use crate::textures::TextureCache;

#[derive(Component)]
pub struct ParallaxLayer {
    pub slowness: f32,
    pub tile: Vec2,
}

fn star_image(size: u32, count: usize, brightness: f32, seed: u64) -> Image {
    let mut data = vec![0u8; (size * size * 4) as usize];
    let mut rng = SimRng(seed);
    for _ in 0..count {
        let x = rng.range_u32(0, size - 1);
        let y = rng.range_u32(0, size - 1);
        let b = (brightness * (0.5 + 0.5 * rng.next_f32()) * 255.0) as u8;
        let tint = rng.next_f32();
        let (r, g, bl) = if tint < 0.15 {
            (b, (b as f32 * 0.85) as u8, (b as f32 * 0.7) as u8)
        } else if tint < 0.3 {
            ((b as f32 * 0.75) as u8, (b as f32 * 0.85) as u8, b)
        } else {
            (b, b, b)
        };
        let i = ((y * size + x) * 4) as usize;
        data[i..i + 4].copy_from_slice(&[r, g, bl, 255]);
    }
    let mut img = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::nearest();
    img
}

pub fn setup_parallax(
    mut commands: Commands,
    content: Res<Content>,
    mut cache: ResMut<TextureCache>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut layers: Vec<(Handle<Image>, Vec2, f32, f32)> = Vec::new();
    if let Some(t) = cache.image(&content, &mut images, &ResPath::new("/Textures/Parallaxes/layer1.png")) {
        layers.push((t.handle, t.size.as_vec2(), 0.998_046_9, 0.0));
    }
    let bright = images.add(star_image(512, 260, 1.0, 1));
    let dim = images.add(star_image(512, 700, 0.45, 2));
    layers.push((dim, Vec2::splat(512.0), 0.989_375, 1.0));
    layers.push((bright, Vec2::splat(512.0), 0.996_625, 2.0));
    for (image, tile, slowness, order) in layers {
        commands.spawn((
            Sprite {
                image,
                custom_size: Some(Vec2::splat(8192.0)),
                image_mode: SpriteImageMode::Tiled {
                    tile_x: true,
                    tile_y: true,
                    stretch_value: 1.0,
                },
                ..default()
            },
            Transform::from_translation(Vec3::new(0.0, 0.0, -990.0 + order)),
            ParallaxLayer { slowness, tile },
        ));
    }
}

pub fn update_parallax(
    cam: Query<(&Transform, &Projection), (With<MainCamera>, Without<ParallaxLayer>)>,
    mut layers: Query<(&ParallaxLayer, &mut Transform, &mut Sprite)>,
) {
    let Ok((ct, proj)) = cam.single() else { return };
    let scale = match proj {
        Projection::Orthographic(o) => o.scale,
        _ => 1.0,
    };
    let cam_pos = ct.translation.truncate();
    for (l, mut tf, mut sprite) in layers.iter_mut() {
        // Screen-space drift is (1 - slowness) of camera movement; keep the
        // layer at screen pixel scale regardless of zoom.
        let tile = l.tile * scale;
        let drift = (cam_pos * (1.0 - l.slowness)) / 1.0;
        let off = Vec2::new(drift.x.rem_euclid(tile.x), drift.y.rem_euclid(tile.y));
        tf.translation.x = cam_pos.x - off.x;
        tf.translation.y = cam_pos.y - off.y;
        tf.scale = Vec3::new(scale, scale, 1.0);
        tf.translation.z = -990.0 + l.slowness;
        sprite.custom_size = Some(Vec2::splat(8192.0));
    }
}
