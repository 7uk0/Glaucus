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
mod shipyard_ui;
mod storage_ui;
mod smoothing;
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
/// Clyde draws every grid's tiles first (`_drawGrids`), then
/// `WorldSpaceBelowEntities` overlays (decals), then all entity sprites
/// sorted by draw depth, so tiles and decals sit below every `DrawDepth`.
pub const Z_TILES: f32 = -400.0;
pub const Z_DECALS: f32 = -350.0;

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
        // `ConstantSerializer` also takes raw values (DrawDepth.Default = 0).
        other => other.parse().unwrap_or(0),
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
    pub demo_pilot: bool,
    pub demo_shipyard: bool,
    /// `--render-map PATH`: draw one map file top-down at 32 px/tile with no
    /// HUD, player or stars, like upstream's `Content.MapRenderer`.
    pub render_map: Option<String>,
}

/// Where the camera sits in `--render-map` mode.
#[derive(Resource, Clone, Copy)]
pub struct RenderView {
    pub center: Vec2,
}

fn parse_args() -> Args {
    let mut a = Args {
        cfg: SectorConfig::default(),
        screenshot: None,
        frames: 90,
        width: 1600,
        height: 900,
        zoom: 0.5,
        demo_pilot: false,
        demo_shipyard: false,
        render_map: None,
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
            "--spawn" => a.cfg.spawn_on_shuttle = v == "shuttle",
            "--demo-shipyard" => {
                a.demo_shipyard = true;
                i += 1;
                continue;
            }
            "--demo-pilot" => {
                a.demo_pilot = true;
                a.cfg.spawn_on_shuttle = true;
                i += 1;
                continue;
            }
            "--render-map" => {
                a.render_map = Some(v.clone());
                a.cfg.outpost = v;
                a.cfg.shuttle = None;
                a.cfg.contractors = 0;
                a.cfg.vermin = 0;
                a.cfg.carp = 0;
                a.zoom = 1.0;
            }
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

    let mut args = args;
    if let Some(path) = &args.render_map
        && let Ok(file) = astrolabe::map::MapFile::load(&content.res, &vellum::ResPath::new(path))
        && let Some(g) = file.grids().next()
        && let Ok(tiles) = file.grid_tiles(g, &content.tiles)
    {
        let (mut lo, mut hi) = (bevy::math::IVec2::MAX, bevy::math::IVec2::MIN);
        for ((x, y), _) in tiles.iter() {
            lo = lo.min(bevy::math::IVec2::new(x, y));
            hi = hi.max(bevy::math::IVec2::new(x + 1, y + 1));
        }
        args.width = ((hi.x - lo.x) as f32 * PX) as u32;
        args.height = ((hi.y - lo.y) as f32 * PX) as u32;
    }
    let render_mode = args.render_map.is_some();

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
    .insert_resource(ClearColor(if render_mode { Color::BLACK } else { Color::srgb(0.01, 0.01, 0.025) }))
    .add_plugins(AthanorPlugin {
        content,
        drive: DriveMode::RealTime {
            pending: 0.0,
            max_ticks_per_frame: 4,
        },
    })
    .add_plugins(HomunculusPlugin)
    .add_plugins(mercury::MercuryPlugin)
    .add_plugins(alembic::AlembicPlugin)
    .add_plugins(prana::PranaPlugin)
    .init_resource::<hud::PlayerAir>()
    .init_resource::<shipyard_ui::ShipyardWindow>()
    .init_resource::<storage_ui::StorageWindow>()
    .insert_resource(args.clone())
    .init_resource::<textures::TextureCache>()
    .init_resource::<world_render::GridViews>()
    .init_resource::<sprites::SpriteViews>()
    .init_resource::<smoothing::SmoothIndex>()
    .init_resource::<input::Hover>()
    .init_resource::<hud::ChatLog>()
    .init_resource::<audio::SoundCache>()
    .add_systems(Startup, (setup_world, input::setup_camera, hud::load_font))
    .add_systems(Startup, (hud::setup_hud.after(hud::load_font), parallax::setup_parallax).run_if(move || !render_mode))
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
            input::camera_follow,
            sprites::update_sprites,
            parallax::update_parallax,
            hud::sample_player_air,
            hud::update_status,
            hud::collect_chat,
            hud::animate_popups,
            audio::play_sounds,
            shipyard_ui::open_requests,
            shipyard_ui::close_on_escape,
            shipyard_ui::rebuild,
            storage_ui::requests,
            storage_ui::rebuild,
        )
            .chain()
            .after(athanor::drive_sim),
    )
    .add_systems(Last, auto_screenshot)
    .add_systems(Update, demo_pilot.after(input::movement_keys).before(athanor::drive_sim));
    app.run();
}

fn setup_world(world: &mut World) {
    let args = world.resource::<Args>().clone();
    match build_sector(world, &args.cfg) {
        Ok(s) if args.render_map.is_some() => {
            athanor::spawn::delete(world, s.player);
            let g = world.get::<athanor::transform::Grid>(s.outpost).unwrap();
            let (mut lo, mut hi) = (bevy::math::IVec2::MAX, bevy::math::IVec2::MIN);
            for ((x, y), _) in g.tiles.iter() {
                lo = lo.min(bevy::math::IVec2::new(x, y));
                hi = hi.max(bevy::math::IVec2::new(x + 1, y + 1));
            }
            let pos = world.get::<athanor::transform::Xform>(s.outpost).unwrap().pos;
            world.insert_resource(RenderView { center: pos + (lo + hi).as_vec2() / 2.0 });
        }
        Ok(s) => {
            let n = world.query::<&athanor::spawn::EntityData>().iter(world).count();
            eprintln!("   sector ready: {n} entities, {} NPCs", s.npcs.len());
            if args.demo_shipyard {
                let console = world
                    .query::<(Entity, &athanor::spawn::MetaData)>()
                    .iter(world)
                    .find(|(_, m)| m.proto.as_deref() == Some("ComputerShipyard"))
                    .map(|(e, _)| e);
                if let Some(c) = console {
                    world.write_message(mercury::UiRequest::Shipyard { user: s.player, console: c });
                }
            }
            if args.demo_pilot
                && let Some(sg) = s.shuttle
            {
                let console = world
                    .query::<(Entity, &athanor::shuttles::ShuttleConsole, &athanor::transform::Xform)>()
                    .iter(world)
                    .find(|(_, _, x)| x.parent == Some(sg))
                    .map(|(e, _, _)| e);
                if let Some(c) = console {
                    athanor::shuttles::toggle_pilot(world, s.player, c);
                }
            }
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

/// `--demo-pilot`: fly a lazy arc away from the outpost (for screenshots).
fn demo_pilot(args: Res<Args>, mut q: Query<&mut athanor::shuttles::Pilot>, mut frame: Local<u32>) {
    if !args.demo_pilot {
        return;
    }
    *frame += 1;
    for mut p in q.iter_mut() {
        p.strafe = Vec2::new(0.0, 1.0);
        p.rotation = if *frame % 240 < 60 { 0.4 } else { 0.0 };
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
