//! Pow3r: upstream's power flow solver. A faithful port of
//! `Content.Server/Power/Pow3r/{PowerState,BatteryRampPegSolver,PowerSolverShared}.cs`.
//!
//! Each tick, every network adds up the demand of its loads and charging
//! batteries, the (ramp-limited) output of its supplies, then of its
//! discharging batteries if supplies fall short, and shares what's met
//! proportionally. Supplies and batteries *ramp* toward the output asked of
//! them, so generators spin up and down rather than jumping. Batteries pass
//! demand upward: a battery discharging into a starved network asks its
//! charging network for that much more next tick, which is how power flows
//! HV → substation → MV → APC → machines. Networks are solved leaves-first
//! (by "height") so that pass-through demand propagates within one tick.

#[derive(Debug, Clone, PartialEq)]
pub struct Supply {
    pub enabled: bool,
    pub max_supply: f32,
    pub supply_ramp_rate: f32,
    pub supply_ramp_tolerance: f32,
    pub current_supply: f32,
    pub supply_ramp_target: f32,
    pub supply_ramp_position: f32,
    pub available_supply: f32,
}

impl Default for Supply {
    fn default() -> Self {
        Supply {
            enabled: true,
            max_supply: 0.0,
            supply_ramp_rate: 5000.0,
            supply_ramp_tolerance: 5000.0,
            current_supply: 0.0,
            supply_ramp_target: 0.0,
            supply_ramp_position: 0.0,
            available_supply: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Load {
    pub enabled: bool,
    pub desired_power: f32,
    pub receiving_power: f32,
}

impl Default for Load {
    fn default() -> Self {
        Load {
            enabled: true,
            desired_power: 0.0,
            receiving_power: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Battery {
    pub enabled: bool,
    pub can_discharge: bool,
    pub can_charge: bool,
    pub capacity: f32,
    pub max_charge_rate: f32,
    pub max_supply: f32,
    /// Always-available supply on top of the ramp. Must be > 0.
    pub supply_ramp_tolerance: f32,
    pub supply_ramp_rate: f32,
    pub efficiency: f32,
    pub supply_ramp_position: f32,
    pub current_supply: f32,
    pub current_storage: f32,
    pub current_receiving: f32,
    pub loading_network_demand: f32,
    pub supplying_marked: bool,
    pub loading_marked: bool,
    pub available_supply: f32,
    pub desired_power: f32,
    pub supply_ramp_target: f32,
    /// Network index this battery charges from / discharges into.
    pub linked_charging: Option<usize>,
    pub linked_discharging: Option<usize>,
    pub max_effective_supply: f32,
}

impl Default for Battery {
    fn default() -> Self {
        Battery {
            enabled: true,
            can_discharge: true,
            can_charge: true,
            capacity: 0.0,
            max_charge_rate: 0.0,
            max_supply: 0.0,
            supply_ramp_tolerance: 5000.0,
            supply_ramp_rate: 5000.0,
            efficiency: 1.0,
            supply_ramp_position: 0.0,
            current_supply: 0.0,
            current_storage: 0.0,
            current_receiving: 0.0,
            loading_network_demand: 0.0,
            supplying_marked: false,
            loading_marked: false,
            available_supply: 0.0,
            desired_power: 0.0,
            supply_ramp_target: 0.0,
            linked_charging: None,
            linked_discharging: None,
            max_effective_supply: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Network {
    pub supplies: Vec<usize>,
    pub loads: Vec<usize>,
    /// Batteries charging from this network (their input port).
    pub battery_loads: Vec<usize>,
    /// Batteries discharging into this network (their output port).
    pub battery_supplies: Vec<usize>,
    pub last_combined_load: f32,
    pub last_combined_supply: f32,
    pub last_combined_max_supply: f32,
    pub height: i32,
}

#[derive(Debug, Clone, Default)]
pub struct PowerState {
    pub supplies: Vec<Supply>,
    pub loads: Vec<Load>,
    pub batteries: Vec<Battery>,
    pub networks: Vec<Network>,
}

/// `BatteryRampPegSolver.Tick`.
pub fn tick(frame_time: f32, state: &mut PowerState) {
    clear_loads_and_supplies(state);
    for layer in group_by_network_depth(state) {
        for n in layer {
            update_network(n, state, frame_time);
        }
    }
    clear_batteries(state);
    update_ramp_positions(frame_time, state);
}

fn clear_loads_and_supplies(state: &mut PowerState) {
    for l in &mut state.loads {
        l.receiving_power = 0.0;
    }
    for s in &mut state.supplies {
        s.current_supply = 0.0;
        s.supply_ramp_target = 0.0;
    }
}

fn update_network(ni: usize, state: &mut PowerState, frame_time: f32) {
    let PowerState { supplies, loads, batteries, networks } = state;
    let network = &mut networks[ni];

    // Demand from loads.
    let mut demand = 0.0f32;
    for &l in &network.loads {
        let load = &loads[l];
        if !load.enabled {
            continue;
        }
        demand += load.desired_power;
    }

    // Demand from charging batteries.
    for &b in &network.battery_loads {
        let battery = &mut batteries[b];
        if !battery.enabled || !battery.can_charge {
            continue;
        }
        let battery_space = ((battery.capacity - battery.current_storage) * (1.0 / battery.efficiency)).max(0.0);
        let scaled_space = battery_space / frame_time;
        let charge_rate = battery.max_charge_rate + battery.loading_network_demand / battery.efficiency;
        battery.desired_power = charge_rate.min(scaled_space);
        demand += battery.desired_power;
    }

    // Supply.
    let mut total_supply = 0.0f32;
    let mut total_max_supply = 0.0f32;
    for &s in &network.supplies {
        let supply = &mut supplies[s];
        if !supply.enabled {
            continue;
        }
        let ramp_max = supply.supply_ramp_position + supply.supply_ramp_tolerance;
        let effective = ramp_max.min(supply.max_supply);
        supply.available_supply = effective;
        total_supply += effective;
        total_max_supply += supply.max_supply;
    }

    let unmet = (demand - total_supply).max(0.0);

    // Discharging batteries, after local supplies so those are preferred.
    let mut total_battery_supply = 0.0f32;
    let mut total_max_battery_supply = 0.0f32;
    if unmet > 0.0 {
        for &b in &network.battery_supplies {
            let battery = &mut batteries[b];
            if !battery.enabled || !battery.can_discharge {
                continue;
            }
            let scaled_space = battery.current_storage / frame_time;
            let supply_cap = battery.max_supply.min(battery.supply_ramp_position + battery.supply_ramp_tolerance);
            let supply_and_passthrough = supply_cap + battery.current_receiving * battery.efficiency;
            battery.available_supply = scaled_space.min(supply_and_passthrough);
            battery.loading_network_demand = unmet;
            battery.max_effective_supply = (battery.current_storage / frame_time).min(battery.max_supply + battery.current_receiving * battery.efficiency);
            total_battery_supply += battery.available_supply;
            total_max_battery_supply += battery.max_effective_supply;
        }
    }

    network.last_combined_load = demand;
    network.last_combined_supply = total_supply + total_battery_supply;
    network.last_combined_max_supply = total_max_supply + total_max_battery_supply;

    let met = demand.min(network.last_combined_supply);
    if met == 0.0 {
        return;
    }
    let supply_ratio = met / demand;

    for &l in &network.loads {
        let load = &mut loads[l];
        if !load.enabled || load.desired_power == 0.0 {
            continue;
        }
        load.receiving_power = load.desired_power * supply_ratio;
    }

    for &b in &network.battery_loads {
        let battery = &mut batteries[b];
        if !battery.enabled || battery.desired_power == 0.0 || !battery.can_charge {
            continue;
        }
        battery.loading_marked = true;
        battery.current_receiving = battery.desired_power * supply_ratio;
        battery.current_storage += frame_time * battery.current_receiving * battery.efficiency;
        battery.current_storage = battery.current_storage.min(battery.capacity);
    }

    let met_supply = demand.min(total_supply);
    if met_supply > 0.0 {
        let relative_supply_output = met_supply / total_supply;
        let target_relative_supply_output = demand.min(total_max_supply) / total_max_supply;
        for &s in &network.supplies {
            let supply = &mut supplies[s];
            if !supply.enabled {
                continue;
            }
            supply.current_supply = supply.available_supply * relative_supply_output;
            supply.supply_ramp_target = supply.max_supply * target_relative_supply_output;
        }
    }

    if unmet <= 0.0 || total_max_battery_supply <= 0.0 {
        return;
    }

    let relative_battery_output = unmet.min(total_battery_supply) / total_battery_supply;
    let relative_target_battery_output = unmet.min(total_max_battery_supply) / total_max_battery_supply;
    for &b in &network.battery_supplies {
        let battery = &mut batteries[b];
        if !battery.enabled || !battery.can_discharge {
            continue;
        }
        battery.supplying_marked = true;
        battery.current_supply = battery.available_supply * relative_battery_output;
        battery.current_storage = (battery.current_storage - frame_time * battery.current_supply).max(0.0);
        battery.supply_ramp_target =
            battery.max_effective_supply * relative_target_battery_output - battery.current_receiving * battery.efficiency;
    }
}

fn clear_batteries(state: &mut PowerState) {
    for b in &mut state.batteries {
        if !b.supplying_marked {
            b.current_supply = 0.0;
            b.supply_ramp_target = 0.0;
            b.loading_network_demand = 0.0;
        }
        if !b.loading_marked {
            b.current_receiving = 0.0;
        }
        b.supplying_marked = false;
        b.loading_marked = false;
    }
}

/// `GroupByNetworkDepth`: a network's height is one more than the highest
/// network its charging batteries discharge into. Layers are solved low to high.
fn group_by_network_depth(state: &mut PowerState) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for n in &mut state.networks {
        n.height = -1;
    }
    for i in 0..state.networks.len() {
        if state.networks[i].height == -1 {
            estimate_depth(state, i, &mut groups);
        }
    }
    groups
}

fn estimate_depth(state: &mut PowerState, ni: usize, groups: &mut Vec<Vec<usize>>) {
    state.networks[ni].height = -2;
    let mut height = -1;
    let battery_loads = state.networks[ni].battery_loads.clone();
    for b in battery_loads {
        let Some(sub) = state.batteries[b].linked_discharging else { continue };
        if sub == ni {
            continue;
        }
        match state.networks[sub].height {
            -1 => estimate_depth(state, sub, groups),
            -2 => continue, // a loop
            _ => {}
        }
        height = height.max(state.networks[sub].height);
    }
    let h = 1 + height;
    state.networks[ni].height = h;
    let h = h as usize;
    if h >= groups.len() {
        groups.push(vec![ni]);
    } else {
        groups[h].push(ni);
    }
}

/// `PowerSolverShared.UpdateRampPositions`.
fn update_ramp_positions(frame_time: f32, state: &mut PowerState) {
    fn step(enabled: bool, target: f32, pos: &mut f32, rate: f32, max: f32, frame_time: f32) {
        if !enabled {
            *pos = 0.0;
            return;
        }
        let dev = target - *pos;
        if dev.abs() > 0.001 {
            let new = if dev > 0.0 { target.min(*pos + rate * frame_time) } else { target.max(*pos - rate * frame_time) };
            *pos = new.clamp(0.0, max.max(0.0));
        } else {
            *pos = target;
        }
    }
    for s in &mut state.supplies {
        step(s.enabled, s.supply_ramp_target, &mut s.supply_ramp_position, s.supply_ramp_rate, s.max_supply, frame_time);
    }
    for b in &mut state.batteries {
        step(b.enabled, b.supply_ramp_target, &mut b.supply_ramp_position, b.supply_ramp_rate, b.max_supply, frame_time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(state: &mut PowerState, secs: f32) {
        let dt = 1.0 / 30.0;
        for _ in 0..(secs / dt) as usize {
            tick(dt, state);
        }
    }

    #[test]
    fn generator_ramps_up_to_meet_a_load() {
        let mut s = PowerState {
            supplies: vec![Supply { max_supply: 20_000.0, supply_ramp_rate: 5000.0, supply_ramp_tolerance: 1500.0, ..Default::default() }],
            loads: vec![Load { desired_power: 10_000.0, ..Default::default() }],
            networks: vec![Network { supplies: vec![0], loads: vec![0], ..Default::default() }],
            ..Default::default()
        };
        tick(1.0 / 30.0, &mut s);
        // Only the ramp tolerance is available on the first tick.
        assert!((s.loads[0].receiving_power - 1500.0).abs() < 1.0, "{}", s.loads[0].receiving_power);
        run(&mut s, 3.0);
        assert!((s.loads[0].receiving_power - 10_000.0).abs() < 1.0, "{}", s.loads[0].receiving_power);
        assert!((s.supplies[0].supply_ramp_position - 10_000.0).abs() < 1.0);
    }

    #[test]
    fn battery_passes_power_down_a_chain() {
        // HV net: generator. Substation-like battery charges from HV, discharges into LV net with a load.
        let mut s = PowerState {
            supplies: vec![Supply { max_supply: 50_000.0, ..Default::default() }],
            loads: vec![Load { desired_power: 3000.0, ..Default::default() }],
            batteries: vec![Battery {
                capacity: 100_000.0,
                current_storage: 0.0,
                max_charge_rate: 1000.0,
                max_supply: 10_000.0,
                linked_charging: Some(0),
                linked_discharging: Some(1),
                ..Default::default()
            }],
            networks: vec![
                Network { supplies: vec![0], battery_loads: vec![0], ..Default::default() },
                Network { loads: vec![0], battery_supplies: vec![0], ..Default::default() },
            ],
        };
        run(&mut s, 10.0);
        assert!((s.loads[0].receiving_power - 3000.0).abs() < 1.0, "load gets {}", s.loads[0].receiving_power);
        assert!(s.batteries[0].current_storage > 1000.0, "battery charges too: {}", s.batteries[0].current_storage);
        // Pull the generator: the battery carries the load from storage.
        s.supplies[0].enabled = false;
        let before = s.batteries[0].current_storage;
        run(&mut s, 2.0);
        assert!((s.loads[0].receiving_power - 3000.0).abs() < 1.0);
        assert!(s.batteries[0].current_storage < before);
    }

    #[test]
    fn short_supply_is_shared_proportionally() {
        let mut s = PowerState {
            supplies: vec![Supply { max_supply: 1000.0, supply_ramp_tolerance: 1000.0, ..Default::default() }],
            loads: vec![Load { desired_power: 1500.0, ..Default::default() }, Load { desired_power: 500.0, ..Default::default() }],
            networks: vec![Network { supplies: vec![0], loads: vec![0, 1], ..Default::default() }],
            ..Default::default()
        };
        run(&mut s, 1.0);
        assert!((s.loads[0].receiving_power - 750.0).abs() < 1.0);
        assert!((s.loads[1].receiving_power - 250.0).abs() < 1.0);
    }
}
