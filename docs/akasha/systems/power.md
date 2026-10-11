# Power (`prana`)

**What it's for in play:** every ship is a power plant. A generator (fuelled
P.A.C.M.A.N. on most Frontier ships; static generators on stations) feeds an
HV net; an SMES stores energy; a substation steps down to MV; APCs step down
to LV and feed machines within reach of an LV cable. Without power: thrusters
and gyroscopes stop (the ship can't fly or brake), airlocks freeze (prying
required), consoles go dark, lights go out, and gravity fails once the
gravity generator's charge drains. Fuel management is part of the salvage
loop: uranium/plasma found on wrecks feeds the generator.

## Upstream sources read

- Dev book: `upstream/ss14-docs/src/en/space-station-14/core-tech/node-networks.md`.
- `Content.Shared/NodeContainer/Node.cs`; `Content.Server/Power/Nodes/{CableNode,CableDeviceNode,CableTerminalNode,CableTerminalPortNode}.cs`.
- `Content.Server/Power/Pow3r/{PowerState,BatteryRampPegSolver,PowerSolverShared}.cs` (ported line for line).
- `Content.Server/Power/EntitySystems/{PowerNetSystem,ExtensionCableSystem,PowerReceiverSystem,BatterySystem,PowerChargeSystem}.cs`.
- `Content.Server/Power/NodeGroups/{PowerNet,ApcNet,BasePowerNet,BaseNetConnectorNodeGroup}.cs`.
- `Content.Server/Power/Generator/GeneratorSystem.cs`, `Content.Shared/Power/Generator/{FuelGeneratorComponent,SharedGeneratorSystem}.cs`.
- `Content.Server/Gravity/{GravitySystem,GravityGeneratorSystem}.cs`.
- Doors: `SharedAirlockSystem.CanChangeState` (powered && !bolted), `AirlockSystem.OnPowerChanged`.
- Thrusters: `ThrusterSystem.CanEnable` (anchored && powered).
- Prototypes: `Entities/Structures/Power/{cables,apc,smes,substation,cable_terminal}.yml`,
  `Generation/{generators,portable_generator}.yml`, `_NF/.../portable_generator.yml`,
  `Machines/gravity_generator.yml`.

## How upstream works

**Nodes.** `NodeContainer.nodes` is a map name → `!type:<Kind>Node` with a
`nodeGroupID` (HVPower, MVPower, Apc, Pipe...). Same-group nodes that reach
each other form a group (a network). Reach rules:

| Node | Reaches |
|---|---|
| `CableNode` (wire) | cables on its tile and 4 neighbours; device nodes on its tile; **not** toward a direction a cable terminal faces (on its tile, or a neighbour terminal facing back) |
| `CableDeviceNode` (machine) | cables on its tile (if `enabled`) |
| `CableTerminalNode` | cables on its tile + the `CableTerminalPortNode` on the tile it faces |
| `CableTerminalPortNode` (SMES input) | neighbour terminals facing it |
| `AdjacentNode` | everything of its group on its tile + 4 neighbours |

Angles: 0 rad faces **south**, +π/2 east (`GetCardinalDir`).

**Connectors.** `PowerSupplier`, `PowerConsumer`, `BatteryCharger`,
`BatteryDischarger`, `Apc`, `PowerProvider` (= `ApcPowerProviderComponent`) each have
`voltage` (High→HVPower, Medium→MVPower, Apc→Apc; default High) and an optional
`node` name; they join the network of the entity's first node matching both.

**APC nets.** LV cables (`CableApcExtension`) carry the Apc node group *and* are
`PowerProvider` + `ExtensionCableProvider` (transferRange 3). Machines have
`ApcPowerReceiver` (`powerLoad` default 5 W, `needsPower` true) +
`ExtensionCableReceiver` (receptionRange 3): an anchored receiver links to the
**closest** anchored provider in a square search, if within
min(reception, transfer) range. The receiver's load then sits on that
provider's Apc net. The APC itself is also a provider, so machines next to it
need no cable. Powered = `!powerDisabled && (!needsPower || CloseToPercent(received, load))`.

**Batteries.** `Battery` (joules: `maxCharge`, `startingCharge`) +
`PowerNetworkBattery` (maxSupply, maxChargeRate, ramp tolerance/rate, efficiency).
Charger side = load on one net, discharger side = supply on another. Substation:
HV in, MV out. APC: MV in (`BatteryCharger voltage Medium`), Apc out (`Apc`).
SMES: input via cable terminal port, output via device node, both HV.
`BatterySystem` syncs `Battery.CurrentCharge` ↔ `NetworkBattery.CurrentStorage`
around each solve.

**Pow3r solve** (every tick, networks low-to-high by height): demand = loads +
charging batteries (`min(maxChargeRate + loadingNetworkDemand/eff, space/dt)`);
supplies give `min(rampPosition + rampTolerance, maxSupply)`; if short,
discharging batteries add `min(storage/dt, min(maxSupply, ramp + tol) + receiving·eff)`
and record the unmet demand (`LoadingNetworkDemand`) so their charging net
asks for it next — that is the pass-through. Met power is shared
proportionally; supplies' ramp targets move toward the share of max supply
needed, at `rampRate` W/s.

**Fuel generators.** On: `PowerSupplier.MaxSupply = TargetPower`, burn
`OptimalBurnRate · dt / (optimal/target)^k` fuel units per tick regardless of
actual draw. Solid fuel = material units × `multiplier` (0.01): the Kestrel's
SUPERPACMAN starts with 1000 FuelGradeUranium = 10 fuel = 15 min at 30 kW.
Shuttle generators have `PortableGenerator.startOnMapInit: true` and start
anchored. Out of fuel → off.

**Gravity.** Grids have a `Gravity` component (`enabled` false unless saved,
`inherent`). `GravityGenerator` + `PowerCharge`: charge 0..1 at `chargeRate` 0.01/s
up while powered, down while not; `Active` at full → grid gravity on; at empty → off
(≈100 s of gravity after losing power). Grids without `Gravity` (asteroids,
wrecks) never have gravity.

## How we ported it

`crates/prana`: `nadi.rs` (node graph, union-find per grid), `pow3r.rs` (exact
solver), `net.rs` (components, topology rebuild on any grid revision change,
receiver linking, solve + write-back, `Unpowered` markers, `PowerChanged`
messages), `devices.rs` (fuel generators + adapters, `MaterialStorage`,
`PowerCharge`, gravity). athanor exposes `power::Unpowered` (thrusters, airlocks,
consoles honour it) and `power::Weightless` (movement honours it).

Simplifications: no wires panel / power switches / UI, no solar/AME/TEG/PA,
no battery-backed receivers (`ApcPowerReceiverBattery`), no radiation from
generators, upgrade multipliers ignored, chemical fuel tanks tracked
in the adapter (chemistry not ported).

## Verified (tests in `crates/prana/tests/power.rs`)

Kestrel: 56 networks; 87/87 receivers powered; SUPERPACMAN at 24.75/30 kW;
burns 66 material units/min; out of fuel → SMES carries all 87; all 5 APCs
removed → 87/87 unpowered, zero thrust, airlocks frozen, gravity gone after ~100 s.
Frontier Outpost: 711/717 receivers powered (the rest are loose items).
