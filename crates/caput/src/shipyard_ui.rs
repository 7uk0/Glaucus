//! The shipyard console window: a paged list of `vessel` prototypes for the
//! console's group, with the player's balance. Buying calls
//! `mercury::buy_vessel` on the simulation world.

use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use mercury::{BankAccount, UiRequest, Vessels};

use crate::hud::UiFont;

const PAGE: usize = 12;

#[derive(Resource, Default)]
pub struct ShipyardWindow {
    pub open: Option<(Entity, Entity, String)>,
    pub page: usize,
    pub root: Option<Entity>,
    pub dirty: bool,
    pub shown_balance: i64,
}

impl ShipyardWindow {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

pub fn open_requests(mut reqs: MessageReader<UiRequest>, mut win: ResMut<ShipyardWindow>, world_group: Query<&athanor::spawn::EntityData>) {
    for r in reqs.read() {
        let UiRequest::Shipyard { user, console } = r;
        let group = world_group
            .get(*console)
            .ok()
            .and_then(|d| d.get("ActivatableUI"))
            .and_then(|a| a.get_str("key"))
            .and_then(|k| k.strip_prefix("enum.ShipyardConsoleUiKey."))
            .unwrap_or("Shipyard")
            .to_string();
        win.open = Some((*user, *console, group));
        win.page = 0;
        win.dirty = true;
    }
}

pub fn close_on_escape(keys: Res<ButtonInput<KeyCode>>, mut win: ResMut<ShipyardWindow>) {
    if win.is_open() && keys.just_pressed(KeyCode::Escape) {
        win.open = None;
        win.dirty = true;
    }
}

const BG: Color = Color::srgba(0.06, 0.07, 0.09, 0.95);
const ROW: Color = Color::srgba(0.13, 0.15, 0.19, 1.0);
const ACCENT: Color = Color::srgb(0.95, 0.75, 0.35);
const TEXT: Color = Color::srgb(0.88, 0.88, 0.84);

fn button(commands: &mut Commands, font: &UiFont, label: String, color: Color) -> Entity {
    commands
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                margin: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(ROW),
        ))
        .with_child((Text::new(label), font.sized(14.0), TextColor(color)))
        .id()
}

#[allow(clippy::too_many_arguments)]
pub fn rebuild(
    mut commands: Commands,
    mut win: ResMut<ShipyardWindow>,
    vessels: Res<Vessels>,
    font: Res<UiFont>,
    accounts: Query<&BankAccount>,
) {
    let live_balance = win.open.as_ref().and_then(|(u, _, _)| accounts.get(*u).ok()).map_or(0, |a| a.balance);
    if win.is_open() && live_balance != win.shown_balance {
        win.dirty = true;
    }
    if !win.dirty {
        return;
    }
    win.shown_balance = live_balance;
    win.dirty = false;
    if let Some(r) = win.root.take() {
        commands.entity(r).despawn();
    }
    let Some((user, console, group)) = win.open.clone() else { return };
    let balance = accounts.get(user).map(|a| a.balance).unwrap_or(0);
    let list: Vec<_> = vessels.group(&group).cloned().collect();
    let pages = list.len().div_ceil(PAGE).max(1);
    let page = win.page.min(pages - 1);

    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Px(60.0),
                margin: UiRect::left(Val::Px(-330.0)),
                width: Val::Px(660.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(12.0)),
                row_gap: Val::Px(2.0),
                ..default()
            },
            BackgroundColor(BG),
        ))
        .id();
    let title = commands
        .spawn((
            Text::new(format!("{group} — ship registry      Balance: {balance} spesos")),
            font.sized(17.0),
            TextColor(ACCENT),
        ))
        .id();
    commands.entity(root).add_child(title);

    for v in list.iter().skip(page * PAGE).take(PAGE) {
        let affordable = balance >= v.price;
        let label = format!(
            "{:<28} {:>8} sp   {} {}",
            v.name,
            v.price,
            v.category,
            if v.classes.is_empty() { String::new() } else { format!("· {}", v.classes.join(", ")) }
        );
        let b = button(&mut commands, &font, label, if affordable { TEXT } else { Color::srgb(0.5, 0.5, 0.5) });
        let id = v.id.clone();
        commands.entity(b).observe(move |_: On<Activate>, mut commands: Commands, mut win: ResMut<ShipyardWindow>| {
            let id = id.clone();
            commands.queue(move |world: &mut World| {
                let _ = mercury::buy_vessel(world, user, console, &id);
            });
            win.open = None;
            win.dirty = true;
        });
        commands.entity(root).add_child(b);
    }

    let nav = commands
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            margin: UiRect::top(Val::Px(8.0)),
            column_gap: Val::Px(8.0),
            ..default()
        })
        .id();
    let prev = button(&mut commands, &font, "< Prev".into(), TEXT);
    commands.entity(prev).observe(|_: On<Activate>, mut win: ResMut<ShipyardWindow>| {
        win.page = win.page.saturating_sub(1);
        win.dirty = true;
    });
    let info = commands
        .spawn((Text::new(format!("page {}/{}", page + 1, pages)), font.sized(14.0), TextColor(TEXT)))
        .id();
    let next = button(&mut commands, &font, "Next >".into(), TEXT);
    commands.entity(next).observe(move |_: On<Activate>, mut win: ResMut<ShipyardWindow>| {
        if win.page + 1 < pages {
            win.page += 1;
        }
        win.dirty = true;
    });
    let close = button(&mut commands, &font, "Close (Esc)".into(), TEXT);
    commands.entity(close).observe(|_: On<Activate>, mut win: ResMut<ShipyardWindow>| {
        win.open = None;
        win.dirty = true;
    });
    commands.entity(nav).add_children(&[prev, info, next, close]);
    commands.entity(root).add_child(nav);
    win.root = Some(root);
}
