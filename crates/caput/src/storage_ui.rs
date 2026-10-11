//! Storage window: what is inside a backpack (or any `Storage`), with
//! buttons to take things out and to put the held item in. Opened by using a
//! storage item in hand (Z) or with B for the backpack you are wearing.

use athanor::events::OpenStorage;
use athanor::interaction::Hands;
use athanor::inventory::Inventory;
use athanor::spawn::MetaData;
use athanor::storage::Storage;
use athanor::Player;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use crate::hud::UiFont;

#[derive(Resource, Default)]
pub struct StorageWindow {
    pub open: Option<(Entity, Entity)>,
    pub root: Option<Entity>,
    pub dirty: bool,
    pub shown: Vec<Entity>,
}

impl StorageWindow {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

pub fn requests(
    mut msgs: MessageReader<OpenStorage>,
    keys: Res<ButtonInput<KeyCode>>,
    player: Query<(Entity, &Inventory), With<Player>>,
    storages: Query<(), With<Storage>>,
    mut win: ResMut<StorageWindow>,
) {
    for m in msgs.read() {
        win.open = Some((m.user, m.storage));
        win.dirty = true;
    }
    if keys.just_pressed(KeyCode::KeyB)
        && let Ok((me, inv)) = player.single()
    {
        if win.is_open() {
            win.open = None;
        } else if let Some(pack) = inv.get("back").filter(|p| storages.contains(*p)) {
            win.open = Some((me, pack));
        }
        win.dirty = true;
    }
    if keys.just_pressed(KeyCode::Escape) && win.is_open() {
        win.open = None;
        win.dirty = true;
    }
}

fn row(commands: &mut Commands, font: &UiFont, label: String) -> Entity {
    commands
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                margin: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.13, 0.15, 0.19, 1.0)),
        ))
        .with_child((Text::new(label), font.sized(14.0), TextColor(Color::srgb(0.88, 0.88, 0.84))))
        .id()
}

pub fn rebuild(
    mut commands: Commands,
    mut win: ResMut<StorageWindow>,
    font: Res<UiFont>,
    names: Query<&MetaData>,
    contents: Query<(Entity, &athanor::interaction::InContainer)>,
    hands: Query<&Hands>,
) {
    // Refresh when the contents change.
    if let Some((_, storage)) = win.open {
        let mut now: Vec<Entity> = contents.iter().filter(|(_, c)| c.owner == storage).map(|(e, _)| e).collect();
        now.sort();
        if now != win.shown {
            win.shown = now;
            win.dirty = true;
        }
    }
    if !win.dirty {
        return;
    }
    win.dirty = false;
    if let Some(r) = win.root.take() {
        commands.entity(r).despawn();
    }
    let Some((user, storage)) = win.open else { return };
    let title = names.get(storage).map(|m| m.name.clone()).unwrap_or_else(|_| "storage".into());
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(10.0),
                top: Val::Px(60.0),
                width: Val::Px(320.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.07, 0.09, 0.95)),
        ))
        .id();
    let t = commands
        .spawn((Text::new(format!("{title} (B / Esc to close)")), font.sized(15.0), TextColor(Color::srgb(0.95, 0.75, 0.35))))
        .id();
    commands.entity(root).add_child(t);
    let shown = win.shown.clone();
    if shown.is_empty() {
        let e = commands.spawn((Text::new("It's empty."), font.sized(14.0), TextColor(Color::srgb(0.6, 0.6, 0.6)))).id();
        commands.entity(root).add_child(e);
    }
    for item in shown {
        let label = names.get(item).map(|m| m.name.clone()).unwrap_or_default();
        let b = row(&mut commands, &font, format!("take {label}"));
        commands.entity(b).observe(move |_: On<Activate>, mut commands: Commands| {
            commands.queue(move |world: &mut World| {
                athanor::storage::remove_to_hand(world, user, item);
            });
        });
        commands.entity(root).add_child(b);
    }
    if let Some(held) = hands.get(user).ok().and_then(Hands::active_item).filter(|h| *h != storage) {
        let label = names.get(held).map(|m| m.name.clone()).unwrap_or_default();
        let b = row(&mut commands, &font, format!("+ put in {label}"));
        commands.entity(b).observe(move |_: On<Activate>, mut commands: Commands| {
            commands.queue(move |world: &mut World| {
                athanor::storage::insert(world, user, held, storage);
            });
        });
        commands.entity(root).add_child(b);
    }
    win.root = Some(root);
}
