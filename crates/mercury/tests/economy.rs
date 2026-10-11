//! The Frontier money loop on real content. Passes trivially without upstream.

use std::sync::OnceLock;

use athanor::map_loader::{load_map_path, LoadOptions};
use athanor::sector::{build_sector, SectorConfig};
use athanor::spawn::{spawn_proto, EntityData, MetaData};
use athanor::transform::{tile_center, tile_of, Xform};
use athanor::{AthanorPlugin, Content};
use bevy_app::App;
use bevy_ecs::prelude::*;
use mercury::{BankAccount, MercuryPlugin, Stack, Vessels};

fn content() -> Option<Content> {
    static C: OnceLock<Option<Content>> = OnceLock::new();
    C.get_or_init(Content::load_upstream).clone()
}

fn app(c: Content) -> App {
    let mut app = App::new();
    app.add_plugins((AthanorPlugin::headless(c), MercuryPlugin));
    app.finish();
    app.cleanup();
    app
}

fn find(world: &mut World, pred: impl Fn(&MetaData, &EntityData) -> bool) -> Option<Entity> {
    let mut q = world.query::<(Entity, &MetaData, &EntityData)>();
    q.iter(world).find(|(_, m, d)| pred(m, d)).map(|(e, ..)| e)
}

#[test]
fn every_vessel_is_listed_with_a_map() {
    let Some(c) = content() else { return };
    let v = Vessels::from_prototypes(&c.protos);
    let shipyard: Vec<_> = v.group("Shipyard").collect();
    eprintln!("{} vessels, {} at the main shipyard; cheapest: {} ({}), priciest: {} ({})",
        v.0.len(), shipyard.len(), shipyard[0].name, shipyard[0].price,
        shipyard.last().unwrap().name, shipyard.last().unwrap().price);
    assert!(shipyard.len() > 40);
    for vessel in &v.0 {
        assert!(c.res.exists(&vellum::ResPath::new(&vessel.path)), "{} map {} missing", vessel.id, vessel.path);
    }
}

#[test]
fn buy_a_ship_at_the_outpost() {
    let Some(c) = content() else { return };
    let mut app = app(c);
    let sector = build_sector(app.world_mut(), &SectorConfig { contractors: 0, vermin: 0, carp: 0, shuttle: None, ..Default::default() }).unwrap();
    app.update();
    let world = app.world_mut();
    assert_eq!(world.get::<BankAccount>(sector.player).unwrap().balance, 30_000);
    let console = find(world, |m, _| m.proto.as_deref() == Some("ComputerShipyard")).expect("outpost has a shipyard console");
    // Too expensive first.
    let r = mercury::buy_vessel(world, sector.player, console, "Kestrel");
    assert!(matches!(r, Err(mercury::shipyard::PurchaseError::InsufficientFunds { .. })), "{r:?}");
    // The cheapest Shipyard vessel is affordable.
    let cheapest = world.resource::<Vessels>().group("Shipyard").next().unwrap().clone();
    let ship = mercury::buy_vessel(world, sector.player, console, &cheapest.id).expect("purchase");
    assert_eq!(world.get::<BankAccount>(sector.player).unwrap().balance, 30_000 - cheapest.price);
    let name = world.get::<MetaData>(ship).unwrap().name.clone();
    eprintln!("bought {} as {name} for {}", cheapest.name, cheapest.price);
    app.update();
    assert!(athanor::shuttles::overlapping_grid(app.world(), ship).is_none(), "new ship spawned in clear space");
}

#[test]
fn sell_cargo_and_bank_the_cash() {
    let Some(c) = content() else { return };
    let mut app = app(c);
    let depot = load_map_path(app.world_mut(), "/Maps/_NF/POI/cargodepot.yml", None, LoadOptions::default()).unwrap().grids[0];
    let world = app.world_mut();
    let console = find(world, |m, _| m.proto.as_deref() == Some("ComputerPalletConsoleNFLowMarket")).expect("pallet console");
    let pallet_tile = {
        let ct = tile_of(world.get::<Xform>(console).unwrap().pos);
        let mut q = world.query::<(&MetaData, &Xform)>();
        q.iter(world)
            .filter(|(m, x)| m.proto.as_deref() == Some("CargoPalletSell") && x.parent == Some(depot))
            .map(|(_, x)| tile_of(x.pos))
            .min_by_key(|t| (*t - ct).abs().max_element())
            .expect("a sell pallet")
    };
    // A crate of something with a known StaticPrice.
    let goods = ["CrateGenericSteel", "ClothingShoesBootsMercenary", "ClothingBackpackMercenary"];
    let mut expected = 0.0;
    for g in goods {
        let e = spawn_proto(world, g, depot, tile_center(pallet_tile)).unwrap_or_else(|| panic!("{g}"));
        expected += mercury::pricing::price_of(world, e, true);
    }
    let report = mercury::cargo::sell(world, console).unwrap();
    eprintln!("sold {} items worth {expected} for {} (low market)", report.items, report.value);
    assert_eq!(report.items, goods.len());
    assert_eq!(report.value, (expected * 0.75).floor() as i64, "low market pays 75%");
    // Cash appeared at the console; bank it at an ATM.
    let cash = {
        let mut q = world.query::<(Entity, &Stack)>();
        q.iter(world).find(|(_, s)| s.stack_type == "Credit").map(|(e, _)| e).expect("cash")
    };
    let you = spawn_proto(world, "MobHuman", depot, tile_center(pallet_tile)).unwrap();
    world.entity_mut(you).insert((athanor::Player, BankAccount { balance: 0 }));
    let atm = spawn_proto(world, "ComputerBankATM", depot, tile_center(pallet_tile)).expect("ATM prototype");
    athanor::interaction::try_pickup(world, you, cash).expect("pick up cash");
    assert!(mercury::bank::on_atm_using(world, you, cash, atm));
    assert_eq!(world.get::<BankAccount>(you).unwrap().balance, report.value);
}
