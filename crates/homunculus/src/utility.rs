//! Utility queries (`utilityQuery` prototypes) — how NPCs pick targets.
//! Port of `NPCUtilitySystem`: gather candidates, filter, then score each by
//! the running geometric mean of its curve-mapped considerations.

use athanor::damage::{Damageable, MobState, MobStateKind, MobThresholds};
use athanor::interaction::InContainer;
use athanor::spawn::EntityData;
use athanor::transform::{to_local, world_pose_of, WorldPose, Xform};
use bevy_ecs::prelude::*;
use grimoire::{Node, Prototype};

use crate::blackboard::Blackboard;
use crate::htn::Domain;
use crate::nav;

#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    Bool,
    InverseBool,
    Preset(String),
    Quadratic {
        slope: f32,
        exponent: f32,
        x_offset: f32,
        y_offset: f32,
    },
}

impl Curve {
    pub fn parse(n: &Node) -> Option<Curve> {
        Some(match n.type_tag()? {
            "BoolCurve" => Curve::Bool,
            "InverseBoolCurve" => Curve::InverseBool,
            "PresetCurve" => Curve::Preset(n.get_str("preset")?.to_string()),
            "QuadraticCurve" => Curve::Quadratic {
                slope: n.get("slope").and_then(Node::as_f32).unwrap_or(1.0),
                exponent: n.get("exponent").and_then(Node::as_f32).unwrap_or(1.0),
                x_offset: n.get("xOffset").and_then(Node::as_f32).unwrap_or(0.0),
                y_offset: n.get("yOffset").and_then(Node::as_f32).unwrap_or(0.0),
            },
            _ => return None,
        })
    }

    pub fn eval(&self, x: f32, domain: &Domain) -> f32 {
        match self {
            Curve::Bool => {
                if x > 0.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Curve::InverseBool => {
                if x > 0.0 {
                    0.0
                } else {
                    1.0
                }
            }
            Curve::Preset(id) => domain.curves.get(id).map_or(0.0, |c| c.eval(x, domain)),
            Curve::Quadratic {
                slope,
                exponent,
                x_offset,
                y_offset,
            } => (slope * (x - x_offset).powf(*exponent) + y_offset).clamp(0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    NearbyHostiles,
    Component(Vec<String>),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    RemoveAnchored,
    Component { components: Vec<String>, retain_with: bool },
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Consideration {
    TargetIsAlive,
    TargetIsCrit,
    TargetDistance,
    TargetHealth { target_state: MobStateKind },
    TargetAccessible,
    TargetInLos,
    TargetInLosOrCurrent,
    Unknown(String),
}

#[derive(Debug, Clone)]
pub struct UtilityQuery {
    pub id: String,
    pub sources: Vec<Source>,
    pub filters: Vec<Filter>,
    pub considerations: Vec<(Consideration, Curve)>,
    pub limit: usize,
}

fn comp_names(n: &Node) -> Vec<String> {
    n.get("components")
        .and_then(Node::as_seq)
        .unwrap_or_default()
        .iter()
        .filter_map(|c| c.get_str("type").map(str::to_owned))
        .collect()
}

impl UtilityQuery {
    pub fn parse(p: &Prototype, unported: &mut std::collections::BTreeSet<String>) -> UtilityQuery {
        let mut q = UtilityQuery {
            id: p.id.clone(),
            sources: Vec::new(),
            filters: Vec::new(),
            considerations: Vec::new(),
            limit: p.get("limit").and_then(Node::as_i64).unwrap_or(128) as usize,
        };
        for item in p.get("query").and_then(Node::as_seq).unwrap_or_default() {
            match item.type_tag() {
                Some("NearbyHostilesQuery") => q.sources.push(Source::NearbyHostiles),
                Some("ComponentQuery") => q.sources.push(Source::Component(comp_names(item))),
                Some("RemoveAnchoredFilter") => q.filters.push(Filter::RemoveAnchored),
                Some("ComponentFilter") => q.filters.push(Filter::Component {
                    components: comp_names(item),
                    retain_with: item.get("retainWithComp").and_then(Node::as_bool).unwrap_or(true),
                }),
                Some(other) => {
                    unported.insert(other.to_string());
                    if other.ends_with("Filter") {
                        q.filters.push(Filter::Unknown(other.into()));
                    } else {
                        q.sources.push(Source::Unknown(other.into()));
                    }
                }
                None => {}
            }
        }
        for c in p.get("considerations").and_then(Node::as_seq).unwrap_or_default() {
            let curve = c.get("curve").and_then(Curve::parse).unwrap_or(Curve::Bool);
            let con = match c.type_tag() {
                Some("TargetIsAliveCon") => Consideration::TargetIsAlive,
                Some("TargetIsCritCon") => Consideration::TargetIsCrit,
                Some("TargetDistanceCon") => Consideration::TargetDistance,
                Some("TargetHealthCon") => Consideration::TargetHealth {
                    target_state: match c.get_str("targetState") {
                        Some("Critical") => MobStateKind::Critical,
                        Some("Dead") => MobStateKind::Dead,
                        _ => MobStateKind::Invalid,
                    },
                },
                Some("TargetAccessibleCon") => Consideration::TargetAccessible,
                Some("TargetInLOSCon") => Consideration::TargetInLos,
                Some("TargetInLOSOrCurrentCon") => Consideration::TargetInLosOrCurrent,
                Some(other) => {
                    unported.insert(other.to_string());
                    Consideration::Unknown(other.into())
                }
                None => continue,
            };
            q.considerations.push((con, curve));
        }
        q
    }
}

fn vision(bb: &Blackboard) -> f32 {
    bb.f32("VisionRadius").unwrap_or(10.0)
}

fn gather(world: &mut World, owner: Entity, src: &Source, range: f32, out: &mut Vec<Entity>) {
    let op = world_pose_of(world, owner);
    let mut q = world.query::<(Entity, &WorldPose)>();
    let near: Vec<Entity> = q
        .iter(world)
        .filter(|(e, p)| *e != owner && p.map == op.map && p.pos.distance(op.pos) <= range)
        .map(|(e, _)| e)
        .collect();
    match src {
        Source::NearbyHostiles => {
            for e in near {
                if athanor::factions::hostile(world, owner, e) && world.get::<InContainer>(e).is_none() {
                    out.push(e);
                }
            }
        }
        Source::Component(comps) => {
            for e in near {
                if let Some(d) = world.get::<EntityData>(e)
                    && comps.iter().all(|c| d.has(c))
                {
                    out.push(e);
                }
            }
        }
        Source::Unknown(_) => {}
    }
}

/// Score candidates and return them best-first.
pub fn run_query(world: &mut World, domain: &Domain, owner: Entity, bb: &Blackboard, query_id: &str) -> Vec<(Entity, f32)> {
    let Some(q) = domain.queries.get(query_id) else { return Vec::new() };
    let radius = vision(bb);
    let mut ents = Vec::new();
    for s in &q.sources {
        gather(world, owner, s, radius, &mut ents);
    }
    ents.sort();
    ents.dedup();
    for f in &q.filters {
        match f {
            Filter::RemoveAnchored => ents.retain(|e| !world.get::<Xform>(*e).is_some_and(|x| x.anchored)),
            Filter::Component { components, retain_with } => ents.retain(|e| {
                let has = world.get::<EntityData>(*e).is_some_and(|d| components.iter().all(|c| d.has(c)));
                has == *retain_with
            }),
            Filter::Unknown(_) => {}
        }
    }
    let n = q.considerations.len().max(1) as f32;
    let mut results = Vec::new();
    for e in ents.into_iter().take(q.limit) {
        let mut score = 1.0f32;
        for (con, curve) in &q.considerations {
            let raw = consideration(world, owner, bb, e, con);
            let c = curve.eval(raw, domain);
            score *= c.powf(1.0 / n).clamp(0.0, 1.0);
            if score <= 0.0 {
                break;
            }
        }
        if score > 0.0 {
            results.push((e, score));
        }
    }
    results.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    results
}

fn consideration(world: &mut World, owner: Entity, bb: &Blackboard, target: Entity, con: &Consideration) -> f32 {
    match con {
        Consideration::TargetIsAlive => world.get::<MobState>(target).map_or(0.0, |s| (s.0 == MobStateKind::Alive) as u8 as f32),
        Consideration::TargetIsCrit => world.get::<MobState>(target).map_or(0.0, |s| (s.0 == MobStateKind::Critical) as u8 as f32),
        Consideration::TargetDistance => {
            let d = athanor::interaction::distance(world, owner, target);
            if !d.is_finite() {
                return 0.0;
            }
            (d / vision(bb)).clamp(0.0, 1.0)
        }
        Consideration::TargetHealth { target_state } => {
            let Some(dmg) = world.get::<Damageable>(target).map(Damageable::total) else { return 0.0 };
            let Some(th) = world.get::<MobThresholds>(target) else { return 0.0 };
            let state = if *target_state == MobStateKind::Invalid { MobStateKind::Critical } else { *target_state };
            let limit = th.threshold_for(state).or_else(|| th.threshold_for(MobStateKind::Dead));
            match limit {
                Some(l) if l > 0.0 => (1.0 - dmg / l).clamp(0.0, 1.0),
                _ => 0.0,
            }
        }
        Consideration::TargetAccessible => {
            if let Some(c) = world.get::<InContainer>(target) {
                return if c.owner == owner { 0.0 } else { 1.0 };
            }
            accessible(world, owner, target) as u8 as f32
        }
        Consideration::TargetInLos => los(world, owner, target, vision(bb) + 0.5) as u8 as f32,
        Consideration::TargetInLosOrCurrent => {
            if bb.entity("Target") == Some(target) {
                let d = athanor::interaction::distance(world, owner, target);
                return (d <= vision(bb) + 0.5) as u8 as f32;
            }
            los(world, owner, target, vision(bb) + 0.5) as u8 as f32
        }
        Consideration::Unknown(_) => 1.0,
    }
}

/// Can the owner path to the target? Same grid → A*; otherwise assume yes.
pub fn accessible(world: &mut World, owner: Entity, target: Entity) -> bool {
    let (Some(ox), Some(_)) = (world.get::<Xform>(owner).copied(), world.get::<Xform>(target)) else { return false };
    let Some(parent) = ox.parent else { return false };
    if world.get::<athanor::transform::Grid>(parent).is_none() {
        return true;
    }
    let pp = world_pose_of(world, parent);
    let tp = world_pose_of(world, target);
    let goal = to_local(&pp, tp.pos);
    let mask = world
        .get::<athanor::physics::Fixtures>(owner)
        .map(|f| f.mask())
        .filter(|m| *m != 0)
        .unwrap_or(athanor::physics::group::by_name("MobMask").unwrap());
    nav::find_path(world, parent, athanor::transform::tile_of(ox.pos), goal, 1.5, mask).is_some()
}

pub fn los(world: &mut World, owner: Entity, target: Entity, range: f32) -> bool {
    let d = athanor::interaction::distance(world, owner, target);
    if d > range {
        return false;
    }
    let Some(parent) = world.get::<Xform>(owner).and_then(|x| x.parent) else { return false };
    if world.get::<athanor::transform::Grid>(parent).is_none() {
        return true;
    }
    let pp = world_pose_of(world, parent);
    let a = to_local(&pp, world_pose_of(world, owner).pos);
    let b = to_local(&pp, world_pose_of(world, target).pos);
    nav::line_of_sight(world, parent, a, b)
}
