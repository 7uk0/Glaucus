//! Player input, camera and mouse picking. Keybinds follow SS14 defaults
//! where it makes sense:
//!
//! | key            | action                              |
//! |----------------|-------------------------------------|
//! | WASD / arrows  | move                                |
//! | Shift (hold)   | walk                                |
//! | Left click     | interact (pick up, open doors...)   |
//! | Shift + click  | examine                             |
//! | R              | toggle combat mode (click = attack) |
//! | Q              | drop held item                      |
//! | X              | swap hands                          |
//! | Z              | use item in hand                    |
//! | Mouse wheel    | zoom                                |
//! | F12            | screenshot                          |

use athanor::combat::{try_melee, CombatMode};
use athanor::interaction::{examine, interact, swap_hands, try_drop, use_in_hand};
use athanor::movement::InputMover;
use athanor::transform::WorldPose;
use athanor::Player;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::PrimaryWindow;

use crate::sprites::SpriteView;
use crate::PX;

#[derive(Component)]
pub struct MainCamera;

/// What the mouse is over, refreshed each frame.
#[derive(Resource, Default)]
pub struct Hover {
    pub world: Option<Vec2>,
    pub entity: Option<Entity>,
}

pub fn setup_camera(mut commands: Commands, args: Res<crate::Args>) {
    commands.spawn((
        Camera2d,
        MainCamera,
        Projection::Orthographic(OrthographicProjection {
            scale: args.zoom,
            ..OrthographicProjection::default_2d()
        }),
    ));
}

pub fn camera_follow(
    player: Query<(&WorldPose, &athanor::transform::Xform), With<Player>>,
    poses: Query<&WorldPose>,
    mut cam: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
    mut wheel: MessageReader<MouseWheel>,
    view: Option<Res<crate::RenderView>>,
) {
    let Ok((mut tf, mut proj)) = cam.single_mut() else { return };
    if let Some(v) = view {
        tf.translation.x = v.center.x * PX;
        tf.translation.y = v.center.y * PX;
        tf.rotation = Quat::IDENTITY;
    } else if let Ok((p, x)) = player.single() {
        tf.translation.x = p.pos.x * PX;
        tf.translation.y = p.pos.y * PX;
        // Like SS14's eye: rotate with the grid you stand on, so "up" is
        // always the ship's (and your movement's) north.
        let grid_rot = x.parent.and_then(|g| poses.get(g).ok()).filter(|_| p.grid.is_some()).map_or(0.0, |g| g.rot);
        tf.rotation = Quat::from_rotation_z(grid_rot);
    }
    let scroll: f32 = wheel.read().map(|w| w.y).sum();
    if scroll != 0.0
        && let Projection::Orthographic(o) = proj.as_mut()
    {
        o.scale = (o.scale * (1.0 - scroll.signum() * 0.1)).clamp(0.15, 3.0);
    }
}

pub fn movement_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut player: Query<(&mut InputMover, Option<&mut athanor::shuttles::Pilot>), With<Player>>,
) {
    let Ok((mut m, pilot)) = player.single_mut() else { return };
    let mut dir = Vec2::ZERO;
    if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
        dir.y += 1.0;
    }
    if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
        dir.y -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        dir.x -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        dir.x += 1.0;
    }
    if let Some(mut p) = pilot {
        // At the helm, WASD strafes the shuttle, Q/E rotate, Space brakes
        // (SS14's ShuttleStrafe*/ShuttleRotate*/ShuttleBrake binds).
        let mut rot = 0.0;
        if keys.pressed(KeyCode::KeyQ) {
            rot += 1.0;
        }
        if keys.pressed(KeyCode::KeyE) {
            rot -= 1.0;
        }
        p.strafe = dir;
        p.rotation = rot;
        p.brake = if keys.pressed(KeyCode::Space) { 1.0 } else { 0.0 };
        if m.wish != Vec2::ZERO {
            m.wish = Vec2::ZERO;
        }
        return;
    }
    let wish = dir.normalize_or_zero();
    let walking = keys.pressed(KeyCode::ShiftLeft) && !keys.pressed(KeyCode::ControlLeft);
    if m.wish != wish || m.walking != walking {
        m.wish = wish;
        m.walking = walking;
    }
}

pub fn update_hover(
    window: Query<&Window, With<PrimaryWindow>>,
    cam: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    views: Query<(&SpriteView, &Visibility, &Transform)>,
    poses: Query<&WorldPose>,
    player: Query<Entity, With<Player>>,
    mut hover: ResMut<Hover>,
) {
    hover.world = None;
    hover.entity = None;
    let (Ok(win), Ok((camera, cam_tf))) = (window.single(), cam.single()) else { return };
    let Some(cursor) = win.cursor_position() else { return };
    let Ok(world) = camera.viewport_to_world_2d(cam_tf, cursor) else { return };
    let wp = world / PX;
    hover.world = Some(wp);
    let me = player.single().ok();
    let mut best: Option<(f32, f32, Entity)> = None;
    for (view, vis, tf) in views.iter() {
        if *vis == Visibility::Hidden || Some(view.sim) == me {
            continue;
        }
        let Ok(p) = poses.get(view.sim) else { continue };
        let d = (p.pos - wp).abs();
        if d.x > 0.5 || d.y > 0.5 {
            continue;
        }
        let z = tf.translation.z;
        let dist = d.length();
        if best.is_none_or(|(bz, bd, _)| z > bz + 0.5 || (z > bz - 0.5 && dist < bd)) {
            best = Some((z, dist, view.sim));
        }
    }
    hover.entity = best.map(|b| b.2);
}

/// Mouse buttons and verb keys. These touch the simulation world directly,
/// the way upstream input handlers raise events on the server.
pub fn verbs(world: &mut World) {
    let Some(player) = world.query_filtered::<Entity, With<Player>>().iter(world).next() else { return };
    let keys_dock = world.resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::KeyF);
    let (click, shift, keys_drop, keys_swap, keys_use, keys_combat, keys_shot) = {
        let mouse = world.resource::<ButtonInput<MouseButton>>();
        let keys = world.resource::<ButtonInput<KeyCode>>();
        (
            mouse.just_pressed(MouseButton::Left),
            keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
            keys.just_pressed(KeyCode::KeyQ),
            keys.just_pressed(KeyCode::KeyX),
            keys.just_pressed(KeyCode::KeyZ),
            keys.just_pressed(KeyCode::KeyR),
            keys.just_pressed(KeyCode::F12),
        )
    };
    let piloting = world.get::<athanor::shuttles::Pilot>(player).is_some();
    // The console's dock / undock-all buttons (`DockRequestMessage`,
    // Frontier's `UndockAllRequestMessage`), as one key.
    if keys_dock && let Some(p) = world.get::<athanor::shuttles::Pilot>(player).copied() {
        let docked = !athanor::docking::docked_grids(world, p.shuttle).is_empty();
        let text = if docked {
            let n = athanor::docking::undock_all(world, p.shuttle);
            format!("Undocked ({n} port{}).", if n == 1 { "" } else { "s" })
        } else if athanor::docking::try_dock_nearest(world, p.shuttle) {
            "Docking clamps engaged.".to_string()
        } else {
            "Docking failed: no aligned dock within range.".to_string()
        };
        world.write_message(athanor::events::ChatLine { speaker: None, channel: athanor::events::ChatChannel::Server, text });
    }
    if keys_drop && !piloting {
        try_drop(world, player);
    }
    if keys_swap {
        swap_hands(world, player);
    }
    if keys_use {
        use_in_hand(world, player);
    }
    if keys_combat
        && let Some(mut c) = world.get_mut::<CombatMode>(player)
    {
        c.active = !c.active;
    }
    if keys_shot {
        let n = world.resource::<Time>().elapsed_secs() as u64;
        world.commands().spawn(Screenshot::primary_window()).observe(save_to_disk(format!("caput-{n}.png")));
    }
    // A modal window owns the mouse.
    if world.get_resource::<crate::shipyard_ui::ShipyardWindow>().is_some_and(|w| w.is_open()) {
        return;
    }
    if click {
        let target = world.resource::<Hover>().entity;
        let Some(target) = target else { return };
        if shift {
            examine(world, player, target);
        } else if world.get::<CombatMode>(player).is_some_and(|c| c.active) {
            try_melee(world, player, target);
        } else {
            interact(world, player, target);
        }
    }
}
