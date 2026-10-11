//! Nadis — the channels power flows through. Port of the node-network layer
//! (`Content.Shared/NodeContainer/Node.cs`, `Content.Server/NodeContainer`,
//! `Content.Server/Power/Nodes/*`), restricted to the power node groups.
//!
//! An entity's `NodeContainer` holds named nodes, each of a kind (how it finds
//! neighbours) and a group (HV, MV, Apc...). Nodes of the same group that can
//! reach each other form one network:
//!
//! - `CableNode` (wires): cables on the same tile and the four neighbours, and
//!   device nodes on its own tile — except toward a direction a cable terminal
//!   faces, which is how an SMES keeps its input and output nets apart.
//! - `CableDeviceNode` (machines): cables on the same tile.
//! - `CableTerminalNode`: cables on its tile + the terminal *port* on the tile it faces.
//! - `CableTerminalPortNode` (SMES input): terminals on neighbouring tiles facing it.
//! - `AdjacentNode`: anything of its group on its tile and the four neighbours.
//!
//! Only anchored nodes connect.

use std::collections::HashMap;

use athanor::spawn::ComponentFactories;
use athanor::transform::{tile_of, Grid, Xform};
use bevy_ecs::prelude::*;
use bevy_math::IVec2;
use grimoire::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeGroupId {
    HvPower,
    MvPower,
    Apc,
    Other,
}

impl NodeGroupId {
    pub fn parse(s: &str) -> NodeGroupId {
        match s {
            "HVPower" => NodeGroupId::HvPower,
            "MVPower" => NodeGroupId::MvPower,
            "Apc" => NodeGroupId::Apc,
            _ => NodeGroupId::Other,
        }
    }

    /// `Voltage` enum values cast to `NodeGroupID`.
    pub fn from_voltage(s: &str) -> NodeGroupId {
        match s {
            "High" | "HV" => NodeGroupId::HvPower,
            "Medium" | "MV" => NodeGroupId::MvPower,
            "Apc" | "LV" => NodeGroupId::Apc,
            _ => NodeGroupId::Other,
        }
    }

    pub fn is_power(self) -> bool {
        self != NodeGroupId::Other
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Cable,
    CableDevice,
    CableTerminal,
    CableTerminalPort,
    Adjacent,
    Other,
}

impl NodeKind {
    fn is_device(self) -> bool {
        matches!(self, NodeKind::CableDevice | NodeKind::CableTerminal)
    }
}

#[derive(Debug, Clone)]
pub struct NodeDef {
    pub name: String,
    pub kind: NodeKind,
    pub group: NodeGroupId,
    /// `CableDeviceNode.enabled` (switchable generators turn outputs off).
    pub enabled: bool,
    pub need_anchored: bool,
}

/// `NodeContainerComponent`.
#[derive(Component, Debug, Clone, Default)]
pub struct NodeContainer {
    pub nodes: Vec<NodeDef>,
}

impl NodeContainer {
    /// The node a connector component attaches to (`BaseNetConnectorComponent.TryFindNet`).
    pub fn find(&self, group: NodeGroupId, name: Option<&str>) -> Option<usize> {
        self.nodes
            .iter()
            .position(|n| n.group == group && name.is_none_or(|nm| nm == n.name))
    }
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("NodeContainer", |e, node, _| {
        let mut nc = NodeContainer::default();
        for (name, n) in node.get("nodes").and_then(Node::as_map).into_iter().flatten() {
            let kind = match n.type_tag() {
                Some("CableNode") => NodeKind::Cable,
                Some("CableDeviceNode") => NodeKind::CableDevice,
                Some("CableTerminalNode") => NodeKind::CableTerminal,
                Some("CableTerminalPortNode") => NodeKind::CableTerminalPort,
                Some("AdjacentNode") => NodeKind::Adjacent,
                _ => NodeKind::Other,
            };
            nc.nodes.push(NodeDef {
                name: name.to_string(),
                kind,
                group: n.get_str("nodeGroupID").map(NodeGroupId::parse).unwrap_or(NodeGroupId::Other),
                enabled: n.get("enabled").and_then(Node::as_bool).unwrap_or(true),
                need_anchored: n.get("needAnchored").and_then(Node::as_bool).unwrap_or(true),
            });
        }
        e.insert(nc);
    });
}

/// A node's identity: (entity, index in its container).
pub type NodeRef = (Entity, usize);

/// Connected components of the power node graph.
#[derive(Debug, Default, Clone)]
pub struct NodeNets {
    /// Node → network index.
    pub net_of: HashMap<NodeRef, usize>,
    /// Network index → (group, grid).
    pub nets: Vec<(NodeGroupId, Entity)>,
}

#[derive(Clone, Copy)]
struct Placed {
    entity: Entity,
    idx: usize,
    kind: NodeKind,
    group: NodeGroupId,
    /// Cardinal facing as a tile offset (from `LocalRotation.GetCardinalDir()`).
    facing: IVec2,
}

const CARDINALS: [IVec2; 4] = [IVec2::new(0, -1), IVec2::new(1, 0), IVec2::new(0, 1), IVec2::new(-1, 0)];

/// SS14 angle → cardinal tile offset: 0 rad faces south, +π/2 east.
pub fn facing(rot: f32) -> IVec2 {
    let q = ((rot / std::f32::consts::FRAC_PI_2).round() as i32).rem_euclid(4);
    CARDINALS[q as usize]
}

struct Uf(Vec<usize>);
impl Uf {
    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra] = rb;
        }
    }
}

/// Flood-fill every grid's power nodes into networks.
pub fn build(world: &mut World) -> NodeNets {
    let mut out = NodeNets::default();
    let placed: Vec<(Entity, IVec2, Placed)> = {
        let mut q = world.query::<(Entity, &NodeContainer, &Xform)>();
        let mut v = Vec::new();
        for (e, nc, x) in q.iter(world) {
            let Some(grid) = x.parent else { continue };
            for (i, n) in nc.nodes.iter().enumerate() {
                if !n.group.is_power() || n.kind == NodeKind::Other || !n.enabled || (n.need_anchored && !x.anchored) {
                    continue;
                }
                v.push((
                    grid,
                    tile_of(x.pos),
                    Placed { entity: e, idx: i, kind: n.kind, group: n.group, facing: facing(x.rot) },
                ));
            }
        }
        v
    };
    // Per grid: tile → node indices into `placed`.
    let mut by_grid: HashMap<Entity, HashMap<IVec2, Vec<usize>>> = HashMap::new();
    for (i, (g, t, _)) in placed.iter().enumerate() {
        if world.get::<Grid>(*g).is_none() {
            continue;
        }
        by_grid.entry(*g).or_default().entry(*t).or_default().push(i);
    }
    let mut uf = Uf((0..placed.len()).collect());
    let empty: Vec<usize> = Vec::new();
    for tiles in by_grid.values() {
        let at = |t: IVec2| tiles.get(&t).unwrap_or(&empty);
        for (&tile, here) in tiles {
            for &a in here {
                let pa = placed[a].2;
                let link = |b: usize, uf: &mut Uf| {
                    if b != a && placed[b].2.group == pa.group {
                        uf.union(a, b);
                    }
                };
                match pa.kind {
                    NodeKind::Cable => {
                        // Directions blocked by terminals (`CableNode.GetReachableNodes`).
                        let mut blocked: Vec<IVec2> = Vec::new();
                        for &b in here {
                            if placed[b].2.kind == NodeKind::CableTerminal {
                                blocked.push(placed[b].2.facing);
                            }
                        }
                        for d in CARDINALS {
                            for &b in at(tile + d) {
                                if placed[b].2.kind == NodeKind::CableTerminal && placed[b].2.facing == -d {
                                    blocked.push(d);
                                }
                            }
                        }
                        for &b in here {
                            let k = placed[b].2.kind;
                            if k == NodeKind::Cable || k.is_device() {
                                link(b, &mut uf);
                            }
                        }
                        for d in CARDINALS {
                            if blocked.contains(&d) {
                                continue;
                            }
                            for &b in at(tile + d) {
                                if placed[b].2.kind == NodeKind::Cable {
                                    link(b, &mut uf);
                                }
                            }
                        }
                    }
                    NodeKind::CableDevice => {
                        for &b in here {
                            if placed[b].2.kind == NodeKind::Cable {
                                link(b, &mut uf);
                            }
                        }
                    }
                    NodeKind::CableTerminal => {
                        for &b in here {
                            if placed[b].2.kind == NodeKind::Cable {
                                link(b, &mut uf);
                            }
                        }
                        for &b in at(tile + pa.facing) {
                            if placed[b].2.kind == NodeKind::CableTerminalPort {
                                link(b, &mut uf);
                            }
                        }
                    }
                    NodeKind::CableTerminalPort => {
                        for d in CARDINALS {
                            for &b in at(tile + d) {
                                if placed[b].2.kind == NodeKind::CableTerminal && placed[b].2.facing == -d {
                                    link(b, &mut uf);
                                }
                            }
                        }
                    }
                    NodeKind::Adjacent => {
                        for &b in here {
                            link(b, &mut uf);
                        }
                        for d in CARDINALS {
                            for &b in at(tile + d) {
                                link(b, &mut uf);
                            }
                        }
                    }
                    NodeKind::Other => {}
                }
            }
        }
    }
    let mut root_net: HashMap<usize, usize> = HashMap::new();
    for (i, (g, _, p)) in placed.iter().enumerate() {
        if !by_grid.contains_key(g) {
            continue;
        }
        let r = uf.find(i);
        let n = *root_net.entry(r).or_insert_with(|| {
            out.nets.push((p.group, *g));
            out.nets.len() - 1
        });
        out.net_of.insert((p.entity, p.idx), n);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facing_matches_ss14_angles() {
        assert_eq!(facing(0.0), IVec2::new(0, -1));
        assert_eq!(facing(std::f32::consts::FRAC_PI_2), IVec2::new(1, 0));
        assert_eq!(facing(std::f32::consts::PI), IVec2::new(0, 1));
        assert_eq!(facing(-std::f32::consts::FRAC_PI_2), IVec2::new(-1, 0));
    }
}
