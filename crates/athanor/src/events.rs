//! Messages the simulation emits for presentation: popups, sounds, chat,
//! attack effects. The client drains these each frame; headless runs can
//! inspect them in tests.

use bevy_ecs::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupKind {
    Small,
    Medium,
    Large,
}

/// A floating text popup over an entity (`SharedPopupSystem`).
#[derive(Message, Debug, Clone)]
pub struct Popup {
    pub text: String,
    pub at: Entity,
    pub kind: PopupKind,
}

/// Play a sound at an entity. `path` is a resource path (`/Audio/...`) or a
/// `soundCollection` id.
#[derive(Message, Debug, Clone)]
pub struct SoundAt {
    pub path: String,
    pub at: Entity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatChannel {
    Local,
    Emote,
    Server,
    Examine,
}

/// A line for the chat/log panel.
#[derive(Message, Debug, Clone)]
pub struct ChatLine {
    pub speaker: Option<Entity>,
    pub channel: ChatChannel,
    pub text: String,
}

/// A melee swing, for the client's attack arc / lunge animation.
#[derive(Message, Debug, Clone)]
pub struct MeleeSwing {
    pub attacker: Entity,
    pub target: Option<Entity>,
    pub hit: bool,
    pub animation: Option<String>,
}
