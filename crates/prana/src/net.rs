//! Power networks: which supplies, loads and batteries sit on which network,
//! and the per-tick solve. Port of `PowerNetSystem`, `PowerNet`/`ApcNet`
//! (`Content.Server/Power/NodeGroups`), `BaseNetConnectorComponent`,
//! `ExtensionCableSystem` + `PowerReceiverSystem` (machines link to the
//! nearest LV cable or APC within range) and `BatterySystem`'s sync.

use std::collections::HashMap;

use athanor::power::Unpowered;
use athanor::spawn::ComponentFactories;
use athanor::transform::{tile_of, Grid, Xform};
use athanor::SimClock;
use bevy_ecs::prelude::*;
use bevy_math::IVec2;
use grimoire::Node;

use crate::nadi::{self, NodeContainer, NodeGroupId, NodeNets};
use crate::pow3r::{self, PowerState};

/// Which network a connector joins: voltage + optional node name (`BaseNetConnectorComponent`).
#[derive(Debug, Clone, PartialEq)]
pub struct Connector {
    pub group: NodeGroupId,
    pub node: Option<String>,
}

impl Connector {
    fn parse(node: &Node) -> Connector {
        Connector {
            group: NodeGroupId::from_voltage(node.get_str("voltage").unwrap_or("High")),
            node: node.get_str("node").map(str::to_string),
        }
    }
}

/// `PowerSupplierComponent`.
#[derive(Component, Debug, Clone)]
pub struct PowerSupplier {
    pub conn: Connector,
    pub supply: pow3r::Supply,
}

/// `PowerConsumerComponent`.
#[derive(Component, Debug, Clone)]
pub struct PowerConsumer {
    pub conn: Connector,
    pub load: pow3r::Load,
}

/// `PowerNetworkBatteryComponent`.
#[derive(Component, Debug, Clone, Default)]
pub struct PowerNetworkBattery {
    pub battery: pow3r::Battery,
}

/// `BatteryChargerComponent`: the battery charges from this connector's network.
#[derive(Component, Debug, Clone)]
pub struct BatteryCharger(pub Connector);

/// `BatteryDischargerComponent`: the battery discharges into this connector's network.
#[derive(Component, Debug, Clone)]
pub struct BatteryDischarger(pub Connector);

/// `BatteryComponent`: stored energy in joules.
#[derive(Component, Debug, Clone, Default)]
pub struct Battery {
    pub max_charge: f32,
    pub charge: f32,
}

/// `ApcComponent`: its network battery discharges into its APC net.
#[derive(Component, Debug, Clone)]
pub struct Apc {
    pub conn: Connector,
    pub main_breaker: bool,
}

/// `ApcPowerProviderComponent` (YAML `PowerProvider`): feeds the receivers linked to it.
#[derive(Component, Debug, Clone)]
pub struct PowerProvider(pub Connector);

/// `ExtensionCableProviderComponent`.
#[derive(Component, Debug, Clone)]
pub struct ExtensionCableProvider {
    pub transfer_range: f32,
}

/// `ExtensionCableReceiverComponent`.
#[derive(Component, Debug, Clone)]
pub struct ExtensionCableReceiver {
    pub reception_range: f32,
}

/// `ApcPowerReceiverComponent`: a machine that runs on APC power.
#[derive(Component, Debug, Clone)]
pub struct ApcPowerReceiver {
    pub load: pow3r::Load,
    pub needs_power: bool,
    pub powered: bool,
    /// The provider (LV cable or APC) it is linked to.
    pub provider: Option<Entity>,
}

impl ApcPowerReceiver {
    pub fn set_load(&mut self, w: f32) {
        self.load.desired_power = w;
    }
    pub fn power_disabled(&self) -> bool {
        !self.load.enabled
    }
}

/// A receiver's powered state changed (`PowerChangedEvent`).
#[derive(Message, Debug, Clone, Copy)]
pub struct PowerChanged {
    pub entity: Entity,
    pub powered: bool,
}

fn f(node: &Node, key: &str) -> Option<f32> {
    node.get(key).and_then(Node::as_f32)
}

fn b(node: &Node, key: &str) -> Option<bool> {
    node.get(key).and_then(Node::as_bool)
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("PowerSupplier", |e, node, _| {
        let mut s = pow3r::Supply::default();
        s.max_supply = f(node, "supplyRate").unwrap_or(0.0);
        s.supply_ramp_tolerance = f(node, "supplyRampTolerance").unwrap_or(s.supply_ramp_tolerance);
        s.supply_ramp_rate = f(node, "supplyRampRate").unwrap_or(s.supply_ramp_rate);
        s.supply_ramp_position = f(node, "supplyRampPosition").unwrap_or(0.0);
        s.enabled = b(node, "enabled").unwrap_or(true);
        e.insert(PowerSupplier { conn: Connector::parse(node), supply: s });
    });
    reg.register("PowerConsumer", |e, node, _| {
        e.insert(PowerConsumer {
            conn: Connector::parse(node),
            load: pow3r::Load { desired_power: f(node, "drawRate").unwrap_or(0.0), ..Default::default() },
        });
    });
    reg.register("PowerNetworkBattery", |e, node, _| {
        let mut bt = pow3r::Battery::default();
        bt.max_charge_rate = f(node, "maxChargeRate").unwrap_or(0.0);
        bt.max_supply = f(node, "maxSupply").unwrap_or(0.0);
        bt.supply_ramp_tolerance = f(node, "supplyRampTolerance").unwrap_or(bt.supply_ramp_tolerance);
        bt.supply_ramp_rate = f(node, "supplyRampRate").unwrap_or(bt.supply_ramp_rate);
        bt.supply_ramp_position = f(node, "supplyRampPosition").unwrap_or(0.0);
        bt.current_supply = f(node, "currentSupply").unwrap_or(0.0);
        bt.current_receiving = f(node, "currentReceiving").unwrap_or(0.0);
        bt.loading_network_demand = f(node, "loadingNetworkDemand").unwrap_or(0.0);
        bt.enabled = b(node, "enabled").unwrap_or(true);
        bt.can_charge = b(node, "canCharge").unwrap_or(true);
        bt.can_discharge = b(node, "canDischarge").unwrap_or(true);
        bt.efficiency = f(node, "efficiency").unwrap_or(1.0);
        e.insert(PowerNetworkBattery { battery: bt });
    });
    reg.register("BatteryCharger", |e, node, _| {
        e.insert(BatteryCharger(Connector::parse(node)));
    });
    reg.register("BatteryDischarger", |e, node, _| {
        e.insert(BatteryDischarger(Connector::parse(node)));
    });
    reg.register("Battery", |e, node, _| {
        let max = f(node, "maxCharge").unwrap_or(0.0);
        e.insert(Battery { max_charge: max, charge: f(node, "startingCharge").unwrap_or(0.0).clamp(0.0, max.max(0.0)) });
    });
    reg.register("Apc", |e, node, _| {
        e.insert(Apc { conn: Connector::parse(node), main_breaker: b(node, "enabled").unwrap_or(true) });
    });
    reg.register("PowerProvider", |e, node, _| {
        e.insert(PowerProvider(Connector::parse(node)));
    });
    reg.register("ExtensionCableProvider", |e, node, _| {
        e.insert(ExtensionCableProvider { transfer_range: f(node, "transferRange").unwrap_or(3.0) });
    });
    reg.register("ExtensionCableReceiver", |e, node, _| {
        e.insert(ExtensionCableReceiver { reception_range: f(node, "receptionRange").unwrap_or(3.0) });
    });
    reg.register("ApcPowerReceiver", |e, node, _| {
        e.insert(ApcPowerReceiver {
            load: pow3r::Load {
                desired_power: f(node, "powerLoad").unwrap_or(5.0),
                enabled: !b(node, "powerDisabled").unwrap_or(false),
                receiving_power: 0.0,
            },
            needs_power: b(node, "needsPower").unwrap_or(true),
            powered: false,
            provider: None,
        });
    });
}

/// Members of one network, by entity.
#[derive(Debug, Default, Clone)]
pub struct NetMembers {
    pub group: Option<NodeGroupId>,
    pub supplies: Vec<Entity>,
    pub consumers: Vec<Entity>,
    pub receivers: Vec<Entity>,
    pub battery_loads: Vec<Entity>,
    pub battery_supplies: Vec<Entity>,
}

/// The cached topology, rebuilt when any grid's anchors change.
#[derive(Resource, Default)]
pub struct PowerTopology {
    pub revisions: Vec<(Entity, u64)>,
    pub nodes: NodeNets,
    pub nets: Vec<NetMembers>,
    pub dirty: bool,
}

impl PowerTopology {
    /// Index of the network an entity's connector attaches to.
    pub fn net_of(&self, world: &World, e: Entity, conn: &Connector) -> Option<usize> {
        let nc = world.get::<NodeContainer>(e)?;
        let idx = nc.find(conn.group, conn.node.as_deref())?;
        self.nodes.net_of.get(&(e, idx)).copied()
    }
}

fn grid_revisions(world: &mut World) -> Vec<(Entity, u64)> {
    let mut q = world.query::<(Entity, &Grid)>();
    let mut v: Vec<_> = q.iter(world).map(|(e, g)| (e, g.revision)).collect();
    v.sort();
    v
}

/// Rebuild networks and receiver links (`NodeGroupSystem` + `PowerNetSystem.ReconnectNetworks`).
pub fn rebuild(world: &mut World) {
    let nodes = nadi::build(world);
    let mut topo = PowerTopology { nodes, ..Default::default() };
    topo.nets = topo.nodes.nets.iter().map(|(g, _)| NetMembers { group: Some(*g), ..Default::default() }).collect();

    macro_rules! join {
        ($comp:ty, $conn:expr, $field:ident) => {{
            let ents: Vec<(Entity, Connector)> = {
                let mut q = world.query::<(Entity, &$comp)>();
                q.iter(world).map(|(e, c)| (e, $conn(c))).collect()
            };
            for (e, conn) in ents {
                if let Some(n) = topo.net_of(world, e, &conn) {
                    topo.nets[n].$field.push(e);
                }
            }
        }};
    }
    join!(PowerSupplier, |c: &PowerSupplier| c.conn.clone(), supplies);
    join!(PowerConsumer, |c: &PowerConsumer| c.conn.clone(), consumers);
    join!(BatteryCharger, |c: &BatteryCharger| c.0.clone(), battery_loads);
    join!(BatteryDischarger, |c: &BatteryDischarger| c.0.clone(), battery_supplies);
    join!(Apc, |c: &Apc| c.conn.clone(), battery_supplies);

    // Receivers link to the nearest anchored extension-cable provider in range.
    let providers: HashMap<Entity, HashMap<IVec2, Vec<(Entity, bevy_math::Vec2, f32)>>> = {
        let mut q = world.query::<(Entity, &ExtensionCableProvider, &Xform)>();
        let mut m: HashMap<Entity, HashMap<IVec2, Vec<_>>> = HashMap::new();
        for (e, p, x) in q.iter(world) {
            let Some(g) = x.parent else { continue };
            if !x.anchored {
                continue;
            }
            m.entry(g).or_default().entry(tile_of(x.pos)).or_default().push((e, x.pos, p.transfer_range));
        }
        m
    };
    let receivers: Vec<(Entity, Xform, f32)> = {
        let mut q = world.query::<(Entity, &ApcPowerReceiver, &Xform, Option<&ExtensionCableReceiver>)>();
        q.iter(world).map(|(e, _, x, r)| (e, *x, r.map_or(-1.0, |r| r.reception_range))).collect()
    };
    for (e, x, range) in receivers {
        let mut provider = None;
        if range >= 0.0
            && x.anchored
            && let Some(g) = x.parent
            && let Some(tiles) = providers.get(&g)
        {
            let r = range.ceil() as i32;
            let t = tile_of(x.pos);
            let mut best: Option<(f32, Entity, f32)> = None;
            for dy in -r..=r {
                for dx in -r..=r {
                    for &(pe, pos, transfer) in tiles.get(&(t + IVec2::new(dx, dy))).into_iter().flatten() {
                        if pe == e {
                            continue;
                        }
                        let d = (pos - x.pos).length();
                        if best.is_none_or(|(bd, ..)| d < bd) {
                            best = Some((d, pe, transfer));
                        }
                    }
                }
            }
            if let Some((d, pe, transfer)) = best
                && d <= range.min(transfer)
            {
                provider = Some(pe);
            }
        }
        if let Some(mut rc) = world.get_mut::<ApcPowerReceiver>(e) {
            rc.provider = provider;
        }
        // The provider's network carries the receiver's load.
        if let Some(pe) = provider
            && let Some(pp) = world.get::<PowerProvider>(pe).cloned()
            && let Some(n) = topo.net_of(world, pe, &pp.0)
        {
            topo.nets[n].receivers.push(e);
        }
    }
    topo.revisions = grid_revisions(world);
    world.insert_resource(topo);
}

/// `PowerNetSystem.Update`: rebuild if needed, sync batteries, solve, publish.
pub fn power_system(world: &mut World) {
    let dt = world.resource::<SimClock>().dt;
    let revs = grid_revisions(world);
    let stale = world.get_resource::<PowerTopology>().is_none_or(|t| t.dirty || t.revisions != revs);
    if stale {
        rebuild(world);
    }

    // Gather every supply, load and battery into the solver's arrays.
    let mut state = PowerState::default();
    let mut supply_ix: HashMap<Entity, usize> = HashMap::new();
    let mut load_ix: HashMap<Entity, usize> = HashMap::new();
    let mut battery_ix: HashMap<Entity, usize> = HashMap::new();
    {
        let mut q = world.query::<(Entity, &PowerSupplier)>();
        for (e, s) in q.iter(world) {
            supply_ix.insert(e, state.supplies.len());
            state.supplies.push(s.supply.clone());
        }
        let mut q = world.query::<(Entity, &PowerConsumer)>();
        for (e, c) in q.iter(world) {
            load_ix.insert(e, state.loads.len());
            state.loads.push(c.load.clone());
        }
        let mut q = world.query::<(Entity, &ApcPowerReceiver)>();
        for (e, r) in q.iter(world) {
            load_ix.insert(e, state.loads.len());
            state.loads.push(r.load.clone());
        }
        let mut q = world.query::<(Entity, &PowerNetworkBattery, Option<&Battery>, Option<&Apc>)>();
        for (e, nb, bat, apc) in q.iter(world) {
            let mut bt = nb.battery.clone();
            // `BatterySystem.PreSync`.
            if let Some(bat) = bat {
                bt.capacity = bat.max_charge;
                bt.current_storage = bat.charge;
            }
            if let Some(apc) = apc {
                bt.can_discharge = apc.main_breaker;
            }
            bt.linked_charging = None;
            bt.linked_discharging = None;
            battery_ix.insert(e, state.batteries.len());
            state.batteries.push(bt);
        }
    }
    {
        let topo = world.resource::<PowerTopology>();
        for (ni, m) in topo.nets.iter().enumerate() {
            let mut n = pow3r::Network::default();
            n.supplies = m.supplies.iter().filter_map(|e| supply_ix.get(e).copied()).collect();
            n.loads = m.consumers.iter().chain(m.receivers.iter()).filter_map(|e| load_ix.get(e).copied()).collect();
            n.battery_loads = m.battery_loads.iter().filter_map(|e| battery_ix.get(e).copied()).collect();
            n.battery_supplies = m.battery_supplies.iter().filter_map(|e| battery_ix.get(e).copied()).collect();
            for &bi in &n.battery_loads {
                state.batteries[bi].linked_charging = Some(ni);
            }
            for &bi in &n.battery_supplies {
                state.batteries[bi].linked_discharging = Some(ni);
            }
            state.networks.push(n);
        }
    }

    pow3r::tick(dt, &mut state);

    // Write back.
    for (e, i) in supply_ix {
        if let Some(mut s) = world.get_mut::<PowerSupplier>(e) {
            s.supply = state.supplies[i].clone();
        }
    }
    for (e, i) in battery_ix {
        let bt = state.batteries[i].clone();
        if let Some(mut bat) = world.get_mut::<Battery>(e) {
            // `BatterySystem.PostSync`.
            bat.charge = bt.current_storage.clamp(0.0, bat.max_charge);
        }
        if let Some(mut nb) = world.get_mut::<PowerNetworkBattery>(e) {
            nb.battery = bt;
        }
    }
    let mut changed = Vec::new();
    for (e, i) in load_ix {
        let load = state.loads[i].clone();
        if let Some(mut c) = world.get_mut::<PowerConsumer>(e) {
            c.load = load;
        } else if let Some(mut r) = world.get_mut::<ApcPowerReceiver>(e) {
            r.load = load;
            // `PowerNetSystem.IsPoweredCalculate`.
            let powered = !r.power_disabled() && (!r.needs_power || close_to_percent(r.load.receiving_power, r.load.desired_power));
            if powered != r.powered {
                r.powered = powered;
                changed.push((e, powered));
            }
        }
    }
    // Keep the athanor marker in step; first tick marks every unpowered receiver.
    let unmarked: Vec<Entity> = {
        let mut q = world.query_filtered::<(Entity, &ApcPowerReceiver), Without<Unpowered>>();
        q.iter(world).filter(|(_, r)| !r.powered).map(|(e, _)| e).collect()
    };
    for e in unmarked {
        world.entity_mut(e).insert(Unpowered);
    }
    for (e, powered) in changed {
        if powered {
            world.entity_mut(e).remove::<Unpowered>();
            athanor::doors::on_power_restored(world, e);
        } else {
            world.entity_mut(e).insert(Unpowered);
        }
        world.write_message(PowerChanged { entity: e, powered });
    }
}

/// `MathHelper.CloseToPercent` with its default tolerance.
pub fn close_to_percent(a: f32, b: f32) -> bool {
    let (a, b) = (a as f64, b as f64);
    let tolerance = 0.00001;
    let eps = (a.abs().max(b.abs()) * tolerance).max(tolerance);
    (a - b).abs() <= eps
}
