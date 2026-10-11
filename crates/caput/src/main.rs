//! # Caput Mortuum — client
//!
//! A single-player Space Station 14 / Frontier experience, rebuilt in Rust.
//! The simulation (`athanor`) and NPC minds (`homunculus`) run in-process
//! at upstream's 30 Hz; this crate draws them with upstream's own sprites.
//!
//! ```text
//! cargo run --release -p caput -- [--shuttle /Maps/_NF/Shuttles/kestrel.yml]
//!     [--outpost /Maps/_NF/Outpost/frontier.yml] [--player MobHuman]
//!     [--npcs 12] [--carp 4] [--seed N]
//!     [--screenshot out.png --frames 120]   # render N frames, save, exit
//! ```

mod audio;
mod hud;
mod input;
mod parallax;
mod sprites;
mod textures;
mod world_render;

use athanor::sector::{build_sector, SectorConfig};
use athanor::{AthanorPlugin, Content, DriveMode, SimDrive};
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::WindowResolution;
use homunculus::HomunculusPlugin;

/// Pixels per tile. RSI sprites are 32×32 per tile.
pub const PX: f32 = 32.0;

/// Upstream `DrawDepth` → z. Floors at the bottom, overlays on top.
pub fn z_for(depth: &str) -> f32 {
    let d: i32 = match depth {
        "LowFloors" => -20,
        "ThickPipe" => -19,
        "ThickWire" => -18,
        "ThinPipeAlt2" => -17,
        "ThinPipeAlt1" => -16,
        "ThinPipe" => -15,
        "ThinWire" => -14,
        "BelowFloor" => -13,
        "FloorTiles" => -12,
        "FloorObjects" => -11,
        "Puddles" => -10,
        "HighFloorObjects" => -5,
        "DeadMobs" => -4,
        "SmallMobs" => -3,
        "Walls" => -2,
        "WallTops" => -1,
        "Objects" => 0,
        "SmallObjects" => 1,
        "WallMountedItems" => 2,
        "LargeObjects" => 3,
        "Items" => 4,
        "BelowMobs" => 5,
        "Mobs" => 6,
        "OverMobs" => 7,
        "Doors" => 8,
        "BlastDoors" => 9,
        "Overdoors" => 10,
        "Effects" => 11,
        "Ghosts" => 12,
        "Overlays" => 13,
        _ => 0,
    };
    d as f32 * 10.0
}

#[derive(Resource, Clone)]
pub struct Args {
    pub cfg: SectorConfig,
    pub screenshot: Option<String>,
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    pub zoom: f32,
}

fn parse_args() -> Args {
    let mut a = Args {
        cfg: SectorConfig::default(),
        screenshot: None,
        frames: 90,
        width: 1600,
        height: 900,
        zoom: 0.5,
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        let v = argv.get(i + 1).cloned().unwrap_or_default();
        match argv[i].as_str() {
            "--outpost" => a.cfg.outpost = v,
            "--shuttle" => a.cfg.shuttle = if v == "none" { None } else { Some(v) },
            "--player" => a.cfg.player_proto = v,
            "--npcs" => a.cfg.contractors = v.parse().unwrap_or(a.cfg.contractors),
            "--carp" => a.cfg.carp = v.parse().unwrap_or(a.cfg.carp),
            "--seed" => a.cfg.seed = v.parse().unwrap_or(a.cfg.seed),
            "--screenshot" => a.screenshot = Some(v),
            "--frames" => a.frames = v.parse().unwrap_or(a.frames),
            "--zoom" => a.zoom = v.parse().unwrap_or(a.zoom),
            "--size" => {
                if let Some((w, h)) = v.split_once('x') {
                    a.width = w.parse().unwrap_or(a.width);
                    a.height = h.parse().unwrap_or(a.height);
                }
            }
            "-h" | "--help" => {
                println!("{}", include_str!("../USAGE.txt"));
                std::process::exit(0);
            }
            _ => {
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    a
}

fn main() {
    let args = parse_args();
    eprintln!("⚗  Caput Mortuum — opening the grimoire...");
    let t = std::time::Instant::now();
    let Some(content) = Content::load_upstream() else {
        eprintln!("No upstream content found. Run tools/summon-upstream.sh first (or set CAPUT_UPSTREAM).");
        std::process::exit(1);
    };
    eprintln!(
        "   {} prototypes, {} tiles, {} locale strings in {:.1?}",
        content.protos.total(),
        content.tiles.len(),
        content.locale.len(),
        t.elapsed()
    );

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Caput Mortuum".into(),
                    resolution: WindowResolution::new(args.width, args.height),
                    ..default()
                }),
                ..default()
            })
            .set(ImagePlugin::default_nearest()),
    )
    .insert_resource(ClearColor(Color::srgb(0.01, 0.01, 0.025)))
    .add_plugins(AthanorPlugin {
        content,
        drive: DriveMode::RealTime {
            pending: 0.0,
            max_ticks_per_frame: 4,
        },
    })
    .add_plugins(HomunculusPlugin)
    .insert_resource(args.clone())
    .init_resource::<textures::TextureCache>()
    .init_resource::<world_render::GridViews>()
    .init_resource::<sprites::SpriteViews>()
    .init_resource::<input::Hover>()
    .init_resource::<hud::ChatLog>()
    .init_resource::<audio::SoundCache>()
    .add_systems(Startup, (setup_world, input::setup_camera, (hud::load_font, hud::setup_hud).chain(), parallax::setup_parallax))
    .add_systems(
        Update,
        (
            feed_clock,
            input::movement_keys,
            input::update_hover,
            input::verbs,
        )
            .chain()
            .before(athanor::drive_sim),
    )
    .add_systems(
        Update,
        (
            world_render::sync_grids,
            sprites::sync_sprite_views,
            sprites::update_sprites,
            input::camera_follow,
            parallax::update_parallax,
            hud::update_status,
            hud::collect_chat,
            hud::animate_popups,
            audio::play_sounds,
        )
            .chain()
            .after(athanor::drive_sim),
    )
    .add_systems(Last, auto_screenshot);
    app.run();
}

fn setup_world(world: &mut World) {
    let args = world.resource::<Args>().clone();
    match build_sector(world, &args.cfg) {
        Ok(s) => {
            let n = world.query::<&athanor::spawn::EntityData>().iter(world).count();
            eprintln!("   sector ready: {n} entities, {} NPCs", s.npcs.len());
            world.write_message(athanor::events::ChatLine {
                speaker: None,
                channel: athanor::events::ChatChannel::Server,
                text: "Welcome to Frontier Outpost. Contractors mill about; carp circle in the dark.".into(),
            });
        }
        Err(e) => {
            eprintln!("failed to build the sector: {e}");
            std::process::exit(1);
        }
    }
}

/// Push real frame time into the fixed-rate simulation driver.
fn feed_clock(time: Res<Time>, mut drive: ResMut<SimDrive>) {
    if let DriveMode::RealTime { pending, .. } = &mut drive.0 {
        *pending += time.delta_secs().min(0.25);
    }
}

/// `--screenshot out.png --frames N`: render N frames, save, quit.
fn auto_screenshot(
    mut commands: Commands,
    args: Res<Args>,
    mut frame: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = &args.screenshot else { return };
    *frame += 1;
    if *frame == args.frames {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
    }
    if *frame == args.frames + 15 {
        exit.write(AppExit::Success);
    }
}
