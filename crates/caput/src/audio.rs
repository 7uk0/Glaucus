//! Sounds. Plays `SoundAt` messages from the simulation using upstream
//! `.ogg` files and `soundCollection` prototypes, attenuated by distance
//! from the player.

use std::collections::HashMap;
use std::sync::Arc;

use athanor::events::SoundAt;
use athanor::rng::SimRng;
use athanor::transform::WorldPose;
use athanor::{Content, Player};
use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;
use grimoire::Node;

#[derive(Resource, Default)]
pub struct SoundCache(HashMap<String, Option<Handle<AudioSource>>>);

/// Resolve a path or collection id to a concrete file path.
fn resolve(content: &Content, id: &str, rng: &mut SimRng) -> Option<String> {
    if id.starts_with('/') {
        return Some(id.to_string());
    }
    let coll = content.protos.get("soundCollection", id)?;
    let files = coll.get("files").map(Node::as_str_list).unwrap_or_default();
    rng.pick(&files).cloned()
}

pub fn play_sounds(
    mut commands: Commands,
    content: Res<Content>,
    mut cache: ResMut<SoundCache>,
    mut sources: ResMut<Assets<AudioSource>>,
    mut msgs: MessageReader<SoundAt>,
    poses: Query<&WorldPose>,
    player: Query<&WorldPose, With<Player>>,
    mut rng: Local<SimRng>,
) {
    let listener = player.single().ok().map(|p| p.pos);
    for s in msgs.read() {
        let Some(path) = resolve(&content, &s.path, &mut rng) else { continue };
        let handle = cache
            .0
            .entry(path.clone())
            .or_insert_with(|| {
                content.res.read(&vellum::ResPath::new(&path)).ok().map(|bytes| {
                    sources.add(AudioSource {
                        bytes: Arc::from(bytes.into_boxed_slice()),
                    })
                })
            })
            .clone();
        let Some(handle) = handle else { continue };
        let dist = match (listener, poses.get(s.at)) {
            (Some(l), Ok(p)) => l.distance(p.pos),
            _ => 0.0,
        };
        if dist > 15.0 {
            continue;
        }
        let vol = (1.0 - dist / 15.0).clamp(0.0, 1.0) * 0.6;
        commands.spawn((
            AudioPlayer(handle),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(vol),
                ..default()
            },
        ));
    }
}
