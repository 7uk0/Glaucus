//! HUD: status panel (health, hands, combat mode), hover name, chat/log
//! panel, and floating popups over entities.

use std::collections::VecDeque;

use athanor::combat::CombatMode;
use athanor::damage::{Damageable, MobState, MobStateKind, MobThresholds};
use athanor::events::{ChatChannel, ChatLine, Popup, PopupKind};
use athanor::interaction::Hands;
use athanor::spawn::MetaData;
use athanor::transform::WorldPose;
use athanor::Player;
use bevy::prelude::*;

use crate::input::Hover;
use crate::PX;

#[derive(Component)]
pub struct StatusText;

#[derive(Component)]
pub struct LogText;

#[derive(Component)]
pub struct HoverText;

#[derive(Component)]
pub struct FloatingPopup {
    pub born: f32,
    pub at: Vec2,
}

#[derive(Resource, Default)]
pub struct ChatLog(pub VecDeque<String>);

/// SS14's own UI font (NotoSans), loaded from the upstream resources.
#[derive(Resource, Clone, Default)]
pub struct UiFont(pub Option<Handle<Font>>);

impl UiFont {
    pub fn sized(&self, size: f32) -> TextFont {
        let f = TextFont::from_font_size(size);
        match &self.0 {
            Some(h) => f.with_font(h.clone()),
            None => f,
        }
    }
}

pub fn load_font(mut commands: Commands, content: Res<athanor::Content>, mut fonts: ResMut<Assets<Font>>) {
    let handle = content
        .res
        .read(&vellum::ResPath::new("/Fonts/NotoSans/NotoSans-Regular.ttf"))
        .ok()
        .map(|bytes| fonts.add(Font::from_bytes(bytes)));
    commands.insert_resource(UiFont(handle));
}

const PANEL: Color = Color::srgba(0.05, 0.06, 0.08, 0.78);
const TEXT: Color = Color::srgb(0.86, 0.86, 0.82);

pub fn setup_hud(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(10.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(PANEL),
        ))
        .with_child((Text::new("..."), font.sized(15.0), TextColor(TEXT), StatusText));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                bottom: Val::Px(10.0),
                width: Val::Px(560.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(PANEL),
        ))
        .with_child((Text::new(""), font.sized(14.0), TextColor(TEXT), LogText));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(10.0),
                top: Val::Px(10.0),
                padding: UiRect::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(PANEL),
        ))
        .with_child((Text::new(""), font.sized(14.0), TextColor(TEXT), HoverText));
}

fn health_line(dmg: Option<&Damageable>, th: Option<&MobThresholds>, st: Option<&MobState>) -> String {
    let total = dmg.map_or(0.0, Damageable::total);
    let crit = th.and_then(|t| t.threshold_for(MobStateKind::Critical).or(t.threshold_for(MobStateKind::Dead)));
    let state = match st.map(|s| s.0) {
        Some(MobStateKind::Critical) => " [CRITICAL]",
        Some(MobStateKind::Dead) => " [DEAD]",
        _ => "",
    };
    match crit {
        Some(c) => format!("Health: {:.0}%{state}", ((1.0 - total / c) * 100.0).clamp(0.0, 100.0)),
        None => format!("Damage: {total:.0}{state}"),
    }
}

#[allow(clippy::type_complexity)]
pub fn update_status(
    player: Query<
        (
            &MetaData,
            Option<&Damageable>,
            Option<&MobThresholds>,
            Option<&MobState>,
            Option<&Hands>,
            Option<&CombatMode>,
        ),
        With<Player>,
    >,
    names: Query<&MetaData>,
    hover: Res<Hover>,
    mut status: Query<&mut Text, (With<StatusText>, Without<HoverText>)>,
    mut hover_text: Query<&mut Text, (With<HoverText>, Without<StatusText>)>,
) {
    if let (Ok((meta, dmg, th, st, hands, combat)), Ok(mut t)) = (player.single(), status.single_mut()) {
        let mut s = format!("{}\n{}", meta.name, health_line(dmg, th, st));
        if let Some(h) = hands {
            for (i, hand) in h.hands.iter().enumerate() {
                let held = hand.held.and_then(|e| names.get(e).ok()).map_or("empty".to_string(), |m| m.name.clone());
                let mark = if i == h.active { ">" } else { " " };
                s.push_str(&format!("\n{mark} {}: {held}", hand.name));
            }
        }
        if combat.is_some_and(|c| c.active) {
            s.push_str("\nCOMBAT MODE");
        }
        s.push_str("\n\nWASD move · click use · shift+click examine\nR combat · Q drop · X swap · Z use · wheel zoom");
        if t.0 != s {
            t.0 = s;
        }
    }
    if let Ok(mut t) = hover_text.single_mut() {
        let s = hover.entity.and_then(|e| names.get(e).ok()).map(|m| m.name.clone()).unwrap_or_default();
        if t.0 != s {
            t.0 = s;
        }
    }
}

pub fn collect_chat(
    mut chat: MessageReader<ChatLine>,
    mut popups: MessageReader<Popup>,
    names: Query<&MetaData>,
    player: Query<Entity, With<Player>>,
    poses: Query<&WorldPose>,
    mut log: ResMut<ChatLog>,
    mut text: Query<&mut Text, With<LogText>>,
    mut commands: Commands,
    time: Res<Time>,
    font: Res<UiFont>,
) {
    let me = player.single().ok();
    let mut changed = false;
    for line in chat.read() {
        let s = match line.channel {
            ChatChannel::Local => {
                let who = line.speaker.and_then(|e| names.get(e).ok()).map_or("Someone".into(), |m| m.name.clone());
                format!("{who} says, \"{}\"", line.text)
            }
            _ => line.text.clone(),
        };
        for l in s.lines() {
            log.0.push_back(l.to_string());
        }
        changed = true;
    }
    for p in popups.read() {
        // Popups shown to the player also land in the log.
        if Some(p.at) == me || p.kind == PopupKind::Large {
            log.0.push_back(p.text.clone());
            changed = true;
        }
        if let Ok(pose) = poses.get(p.at) {
            let size = match p.kind {
                PopupKind::Small => 13.0,
                PopupKind::Medium => 15.0,
                PopupKind::Large => 18.0,
            };
            commands.spawn((
                Text2d::new(p.text.clone()),
                font.sized(size),
                TextColor(Color::srgb(1.0, 0.95, 0.85)),
                Transform::from_translation(Vec3::new(pose.pos.x * PX, pose.pos.y * PX + 20.0, 900.0)),
                FloatingPopup {
                    born: time.elapsed_secs(),
                    at: pose.pos,
                },
            ));
        }
    }
    while log.0.len() > 10 {
        log.0.pop_front();
        changed = true;
    }
    if changed && let Ok(mut t) = text.single_mut() {
        t.0 = log.0.iter().cloned().collect::<Vec<_>>().join("\n");
    }
}

pub fn animate_popups(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &FloatingPopup, &mut Transform, &mut TextColor)>,
) {
    let now = time.elapsed_secs();
    for (e, p, mut tf, mut c) in q.iter_mut() {
        let age = now - p.born;
        if age > 2.0 {
            commands.entity(e).despawn();
            continue;
        }
        tf.translation.y = p.at.y * PX + 20.0 + age * 14.0;
        c.0.set_alpha((1.0 - (age - 1.2).max(0.0) / 0.8).clamp(0.0, 1.0));
    }
}
