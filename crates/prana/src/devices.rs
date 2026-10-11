//! Things on the power networks that do more than draw a constant load:
//! fuel generators (`GeneratorSystem`, `PortableGeneratorSystem`, solid and
//! chemical fuel adapters), charging machines (`PowerChargeSystem`), gravity
//! generators and grid gravity (`GravityGeneratorSystem`, `GravitySystem`).

use std::collections::{BTreeMap, HashMap};

use athanor::power::Weightless;
use athanor::spawn::{ComponentFactories, EntityData};
use athanor::transform::{Grid, Xform};
use athanor::SimClock;
use bevy_ecs::prelude::*;
use grimoire::Node;

use crate::net::{ApcPowerReceiver, PowerSupplier};

/// `FuelGeneratorComponent`.
#[derive(Component, Debug, Clone)]
pub struct FuelGenerator {
    pub on: bool,
    pub target_power: f32,
    pub max_target_power: f32,
    pub min_target_power: f32,
    pub optimal_power: f32,
    pub optimal_burn_rate: f32,
    pub fuel_efficiency_constant: f32,
}

impl FuelGenerator {
    /// `SharedGeneratorSystem.CalcFuelEfficiency`.
    pub fn fuel_efficiency(&self) -> f32 {
        (self.optimal_power / self.target_power).powf(self.fuel_efficiency_constant)
    }
}

/// `PortableGeneratorComponent.StartOnMapInit` (shuttle generators come running).
#[derive(Component, Debug, Clone, Copy)]
pub struct StartOnMapInit;

/// `SolidFuelGeneratorAdapterComponent`: burns a material from `MaterialStorage`.
#[derive(Component, Debug, Clone)]
pub struct SolidFuelGeneratorAdapter {
    pub fuel_material: String,
    pub multiplier: f32,
    pub fractional: f32,
}

/// `ChemicalFuelGeneratorAdapterComponent`: burns reagents from a solution.
/// Without chemistry ported, the tank's reagents are tracked here.
#[derive(Component, Debug, Clone)]
pub struct ChemicalFuelGeneratorAdapter {
    pub solution: String,
    pub reagents: Vec<(String, f32)>,
    /// Reagent → units in the tank (seeded from `SolutionContainerManager`).
    pub tank: Option<HashMap<String, f32>>,
}

/// `MaterialStorageComponent` (just the stored amounts for now; lathes will own it later).
#[derive(Component, Debug, Clone, Default)]
pub struct MaterialStorage {
    pub storage: BTreeMap<String, i64>,
}

/// `PowerChargeComponent`: a machine that charges up before it works.
#[derive(Component, Debug, Clone)]
pub struct PowerCharge {
    pub charge_rate: f32,
    pub idle_power: f32,
    pub active_power: f32,
    pub active_charging_power: f32,
    pub intact: bool,
    pub switched_on: bool,
    pub active: bool,
    pub max_charge: f32,
    pub charge: f32,
}

/// `GravityGeneratorComponent`.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct GravityGenerator {
    pub gravity_active: bool,
}

/// `GravityComponent` on a grid.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Gravity {
    pub enabled: bool,
    pub inherent: bool,
}

fn f(node: &Node, key: &str) -> Option<f32> {
    node.get(key).and_then(Node::as_f32)
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("FuelGenerator", |e, node, _| {
        e.insert(FuelGenerator {
            on: node.get("on").and_then(Node::as_bool).unwrap_or(false),
            target_power: f(node, "targetPower").unwrap_or(15_000.0),
            max_target_power: f(node, "maxTargetPower").unwrap_or(30_000.0),
            min_target_power: f(node, "minTargetPower").unwrap_or(1_000.0),
            optimal_power: f(node, "optimalPower").unwrap_or(15_000.0),
            optimal_burn_rate: f(node, "optimalBurnRate").unwrap_or(1.0 / 60.0),
            fuel_efficiency_constant: f(node, "fuelEfficiencyConstant").unwrap_or(1.3),
        });
    });
    reg.register("PortableGenerator", |e, node, _| {
        if node.get("startOnMapInit").and_then(Node::as_bool).unwrap_or(false) {
            e.insert(StartOnMapInit);
        }
    });
    reg.register("SolidFuelGeneratorAdapter", |e, node, _| {
        e.insert(SolidFuelGeneratorAdapter {
            fuel_material: node.get_str("fuelMaterial").unwrap_or("Plasma").to_string(),
            multiplier: f(node, "multiplier").unwrap_or(0.0),
            fractional: f(node, "fractionalMaterial").unwrap_or(0.0),
        });
    });
    reg.register("ChemicalFuelGeneratorAdapter", |e, node, _| {
        e.insert(ChemicalFuelGeneratorAdapter {
            solution: node.get_str("solution").unwrap_or("tank").to_string(),
            reagents: node
                .get("reagents")
                .and_then(Node::as_map)
                .map(|m| m.iter().filter_map(|(k, v)| Some((k.to_string(), v.as_f32()?))).collect())
                .unwrap_or_default(),
            tank: None,
        });
    });
    reg.register("MaterialStorage", |e, node, _| {
        let storage = node
            .get("storage")
            .and_then(Node::as_map)
            .map(|m| m.iter().filter_map(|(k, v)| Some((k.to_string(), v.as_i64()?))).collect())
            .unwrap_or_default();
        e.insert(MaterialStorage { storage });
    });
    reg.register("PowerCharge", |e, node, _| {
        e.insert(PowerCharge {
            charge_rate: f(node, "chargeRate").unwrap_or(0.01),
            idle_power: f(node, "idlePower").unwrap_or(0.0),
            active_power: f(node, "activePower").unwrap_or(0.0),
            active_charging_power: f(node, "activeChargingPower").unwrap_or(0.0),
            intact: node.get("intact").and_then(Node::as_bool).unwrap_or(true),
            switched_on: node.get("switchedOn").and_then(Node::as_bool).unwrap_or(true),
            active: node.get("active").and_then(Node::as_bool).unwrap_or(false),
            max_charge: f(node, "maxCharge").unwrap_or(1.0),
            charge: f(node, "charge").unwrap_or(1.0),
        });
    });
    reg.register("GravityGenerator", |e, _, _| {
        e.insert(GravityGenerator::default());
    });
    reg.register("Gravity", |e, node, _| {
        e.insert(Gravity {
            enabled: node.get("enabled").and_then(Node::as_bool).unwrap_or(false),
            inherent: node.get("inherent").and_then(Node::as_bool).unwrap_or(false),
        });
    });
}

/// `PortableGeneratorSystem` map-init start: anchored shuttle generators switch on.
pub fn start_generators(world: &mut World) {
    let ents: Vec<Entity> = {
        let mut q = world.query_filtered::<Entity, (With<StartOnMapInit>, With<FuelGenerator>)>();
        q.iter(world).collect()
    };
    for e in ents {
        let anchored = world.get::<Xform>(e).is_some_and(|x| x.anchored);
        if let Some(mut g) = world.get_mut::<FuelGenerator>(e) {
            g.on = anchored;
        }
        world.entity_mut(e).remove::<StartOnMapInit>();
    }
}

/// Seed chemical generator tanks from their `SolutionContainerManager` YAML.
fn seed_tank(world: &World, e: Entity, a: &ChemicalFuelGeneratorAdapter) -> HashMap<String, f32> {
    let mut tank = HashMap::new();
    let sol = world
        .get::<EntityData>(e)
        .and_then(|d| d.get("SolutionContainerManager"))
        .and_then(|s| s.get("solutions"))
        .and_then(|s| s.get(&a.solution));
    for r in sol.and_then(|s| s.get("reagents")).and_then(Node::as_seq).unwrap_or_default() {
        if let (Some(id), Some(q)) = (r.get_str("ReagentId"), r.get("Quantity").and_then(Node::as_f32)) {
            *tank.entry(id.to_string()).or_insert(0.0) += q;
        }
    }
    tank
}

/// Fuel left, in the generator's fuel units (`GeneratorGetFuelEvent`).
pub fn fuel_of(world: &World, e: Entity) -> f32 {
    let mut fuel = 0.0;
    if let (Some(a), Some(m)) = (world.get::<SolidFuelGeneratorAdapter>(e), world.get::<MaterialStorage>(e)) {
        let amount = m.storage.get(&a.fuel_material).copied().unwrap_or(0) as f32;
        fuel += (a.fractional + amount) * a.multiplier;
    }
    if let Some(a) = world.get::<ChemicalFuelGeneratorAdapter>(e) {
        let tank = a.tank.clone().unwrap_or_else(|| seed_tank(world, e, a));
        for (r, mult) in &a.reagents {
            fuel += tank.get(r).copied().unwrap_or(0.0) * mult;
        }
    }
    fuel
}

/// `GeneratorSystem.RemoveFractionalFuel`.
fn remove_fractional(fractional: &mut f32, fuel_used: f32, multiplier: f32, available: i64) -> i64 {
    if !fractional.is_finite() {
        *fractional = 0.0;
    }
    *fractional -= fuel_used / multiplier;
    if *fractional >= 0.0 {
        return 0;
    }
    let to_remove = (-(fractional.floor()) as i64).min(available);
    *fractional = (*fractional + to_remove as f32).max(0.0);
    to_remove
}

fn use_fuel(world: &mut World, e: Entity, used: f32) {
    if let Some(a) = world.get::<SolidFuelGeneratorAdapter>(e).cloned()
        && a.multiplier > 0.0
    {
        let available = world.get::<MaterialStorage>(e).and_then(|m| m.storage.get(&a.fuel_material).copied()).unwrap_or(0);
        let mut frac = a.fractional;
        let removed = remove_fractional(&mut frac, used, a.multiplier, available);
        world.get_mut::<SolidFuelGeneratorAdapter>(e).unwrap().fractional = frac;
        if let Some(mut m) = world.get_mut::<MaterialStorage>(e) {
            *m.storage.entry(a.fuel_material.clone()).or_insert(0) -= removed;
        }
    }
    if let Some(a) = world.get::<ChemicalFuelGeneratorAdapter>(e).cloned() {
        let mut tank = a.tank.clone().unwrap_or_else(|| seed_tank(world, e, &a));
        let total: f32 = a.reagents.iter().map(|(r, _)| tank.get(r).copied().unwrap_or(0.0)).sum();
        if total > 0.0 {
            for (r, mult) in &a.reagents {
                let have = tank.get(r).copied().unwrap_or(0.0);
                let share = used * have / total;
                tank.insert(r.clone(), (have - share / mult.max(1e-6)).max(0.0));
            }
        }
        world.get_mut::<ChemicalFuelGeneratorAdapter>(e).unwrap().tank = Some(tank);
    }
}

/// `GeneratorSystem.Update`: running generators supply their target power and burn fuel.
pub fn generator_system(world: &mut World) {
    let dt = world.resource::<SimClock>().dt;
    let gens: Vec<(Entity, FuelGenerator)> = {
        let mut q = world.query::<(Entity, &FuelGenerator)>();
        q.iter(world).map(|(e, g)| (e, g.clone())).collect()
    };
    for (e, g) in gens {
        let anchored = world.get::<Xform>(e).is_some_and(|x| x.anchored);
        let mut on = g.on && anchored;
        if on && fuel_of(world, e) <= 0.0 {
            on = false;
        }
        if !on {
            if g.on {
                world.get_mut::<FuelGenerator>(e).unwrap().on = false;
            }
            if let Some(mut s) = world.get_mut::<PowerSupplier>(e) {
                s.supply.enabled = false;
            }
            continue;
        }
        if let Some(mut s) = world.get_mut::<PowerSupplier>(e) {
            s.supply.enabled = true;
            s.supply.max_supply = g.target_power;
        }
        let eff = 1.0 / g.fuel_efficiency();
        use_fuel(world, e, g.optimal_burn_rate * dt * eff);
    }
}

/// `PowerChargeSystem.Update` (+ `UpdatePowerState`): charge up while powered,
/// run down when not; flip `active` at full/empty.
pub fn power_charge_system(world: &mut World) {
    let dt = world.resource::<SimClock>().dt;
    let mut q = world.query::<(&mut PowerCharge, &mut ApcPowerReceiver, Option<&mut GravityGenerator>)>();
    for (mut c, mut r, grav) in q.iter_mut(world) {
        if !c.intact {
            continue;
        }
        let rate = if c.switched_on {
            if r.powered {
                c.charge_rate
            } else {
                let main = (r.load.receiving_power - c.idle_power).max(0.0);
                let span = c.active_charging_power - c.idle_power;
                let ratio = if span > 0.0 { 1.0 - main / span } else { 1.0 };
                -(ratio * c.charge_rate)
            }
        } else {
            -c.charge_rate
        };
        c.charge = (c.charge + dt * rate).clamp(0.0, c.max_charge);
        if rate > 0.0 {
            if (c.charge - c.max_charge).abs() < 1e-6 && !c.active {
                c.active = true;
            }
        } else if c.charge.abs() < 1e-6 && c.active {
            c.active = false;
        }
        let load = if c.switched_on {
            if c.max_charge == c.charge { c.active_power } else { c.active_charging_power }
        } else {
            c.idle_power
        };
        r.set_load(load);
        if let Some(mut g) = grav {
            g.gravity_active = c.active;
        }
    }
}

/// `GravitySystem.RefreshGravity` for every grid, then mark weightless grids
/// for athanor's movement. A grid without a `Gravity` component (asteroids,
/// debris) has none.
pub fn gravity_system(world: &mut World) {
    let mut active_on: HashMap<Entity, bool> = HashMap::new();
    {
        let mut q = world.query::<(&GravityGenerator, &Xform)>();
        for (g, x) in q.iter(world) {
            if let Some(p) = x.parent {
                *active_on.entry(p).or_insert(false) |= g.gravity_active;
            }
        }
    }
    let grids: Vec<(Entity, Option<Gravity>, bool)> = {
        let mut q = world.query::<(Entity, &Grid, Option<&Gravity>, Has<Weightless>)>();
        q.iter(world).map(|(e, _, g, w)| (e, g.copied(), w)).collect()
    };
    for (e, grav, was_weightless) in grids {
        let has_gravity = match grav {
            Some(g) if g.inherent => true,
            Some(mut g) => {
                // Generators decide once there are any; saved state otherwise.
                if let Some(&active) = active_on.get(&e) {
                    g.enabled = active;
                    world.entity_mut(e).insert(g);
                }
                g.enabled
            }
            None => false,
        };
        if has_gravity && was_weightless {
            world.entity_mut(e).remove::<Weightless>();
        } else if !has_gravity && !was_weightless {
            world.entity_mut(e).insert(Weightless);
        }
    }
}
