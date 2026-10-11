//! Running NPC minds: the `HTN` component, operator lifecycles and the
//! per-tick update. Port of `HTNSystem.UpdateNPC`/`Update` plus the parts of
//! `NPCCombatSystem.Melee` the melee operator relies on.

use athanor::combat::{try_melee, weapon_of, AttackResult, CombatMode, MeleeWeapon};
use athanor::damage::MobState;
use athanor::events::{ChatChannel, ChatLine, MeleeSwing};
use athanor::interaction::{distance, Hands, InContainer};
use athanor::movement::InputMover;
use athanor::rng::SimRng;
use athanor::spawn::{ComponentFactories, EntityData};
use athanor::transform::{tile_center, tile_of, vec_to_angle, world_pose_of, Xform};
use athanor::{Player, SimClock};
use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use grimoire::Node;

use crate::blackboard::{Blackboard, Effects, Value};
use crate::htn::{new_plan_better, plan, Domain, Plan, PlanContext};
use crate::nav::{reachable_tiles, NpcSteering, SteerTarget, SteeringStatus};
use crate::ops::{Operator, Precondition};
use crate::utility::{los, run_query};

/// `HTNComponent`.
#[derive(Component, Debug, Clone)]
pub struct Htn {
    pub root: String,
    pub blackboard: Blackboard,
    pub plan: Option<Plan>,
    pub plan_cooldown: f32,
    pub plan_accumulator: f32,
    pub check_services: bool,
    pub constantly_replan: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatStatus {
    Normal,
    TargetOutOfRange,
    TargetUnreachable,
}

/// `NPCMeleeCombatComponent`.
#[derive(Component, Debug, Clone, Copy)]
pub struct NpcMeleeCombat {
    pub target: Entity,
    pub miss_chance: f32,
    pub status: CombatStatus,
}

pub fn register(reg: &mut ComponentFactories) {
    reg.register("HTN", |e, node, _| {
        let root = node.get("rootTask").and_then(|r| r.get_str("task")).unwrap_or("IdleCompound").to_string();
        e.insert((
            Htn {
                root,
                blackboard: Blackboard::from_yaml(node.get("blackboard")),
                plan: None,
                plan_cooldown: node.get("planCooldown").and_then(Node::as_f32).unwrap_or(0.45),
                plan_accumulator: 0.0,
                check_services: true,
                constantly_replan: node.get("constantlyReplan").and_then(Node::as_bool).unwrap_or(true),
                enabled: node.get("enabled").and_then(Node::as_bool).unwrap_or(true),
            },
            NpcSteering::default(),
        ));
    });
}

// ---------------------------------------------------------------------------
// Planning context
// ---------------------------------------------------------------------------

struct Ctx<'w> {
    world: &'w mut World,
    domain: &'w Domain,
    owner: Entity,
}

fn coords_of(world: &World, e: Entity) -> Option<Value> {
    let x = world.get::<Xform>(e)?;
    Some(Value::Coords {
        parent: x.parent?,
        pos: x.pos,
    })
}

/// World position (and map) of a coordinates value.
fn coords_world(world: &World, v: &Value) -> Option<(Option<Entity>, Vec2)> {
    match v {
        Value::Coords { parent, pos } => {
            world.get_entity(*parent).ok()?;
            let pp = world_pose_of(world, *parent);
            Some((pp.map, athanor::transform::to_world(&pp, *pos)))
        }
        Value::Entity(e) => {
            world.get_entity(*e).ok()?;
            let p = world_pose_of(world, *e);
            Some((p.map, p.pos))
        }
        _ => None,
    }
}

/// `NPCBlackboard.OwnerCoordinates`: the planned position if a MoveTo has
/// set one during planning, else where the owner actually is.
fn owner_coords(world: &World, owner: Entity, bb: &Blackboard) -> Option<Value> {
    if bb.has("OwnerCoordinates") {
        return bb.get("OwnerCoordinates");
    }
    coords_of(world, owner)
}

fn values_distance(world: &World, a: &Value, b: &Value) -> Option<f32> {
    let (ma, pa) = coords_world(world, a)?;
    let (mb, pb) = coords_world(world, b)?;
    (ma == mb).then(|| pa.distance(pb))
}

impl PlanContext for Ctx<'_> {
    fn precondition_met(&mut self, pc: &Precondition, bb: &Blackboard) -> bool {
        let w = &mut *self.world;
        let owner = self.owner;
        match pc {
            Precondition::KeyExists { key, invert } => bb.has(key) != *invert,
            Precondition::TargetInRange { target_key, range_key } => {
                let (Some(t), Some(r)) = (bb.entity(target_key), bb.f32(range_key)) else { return false };
                let Some(oc) = owner_coords(w, owner, bb) else { return false };
                values_distance(w, &oc, &Value::Entity(t)).is_some_and(|d| d <= r)
            }
            Precondition::TargetInLos { target_key, range_key } => {
                let (Some(t), Some(r)) = (bb.entity(target_key), bb.f32(range_key)) else { return false };
                w.get_entity(t).is_ok() && los(w, owner, t, r)
            }
            Precondition::CoordinatesInRange { target_key, range_key } => {
                let (Some(c), Some(r)) = (bb.get(target_key), bb.f32(range_key)) else { return false };
                let Some(oc) = owner_coords(w, owner, bb) else { return false };
                values_distance(w, &oc, &c).is_some_and(|d| d <= r)
            }
            Precondition::CoordinatesNotInRange { target_key, range_key } => {
                let (Some(c), Some(r)) = (bb.get(target_key), bb.f32(range_key)) else { return false };
                let Some(oc) = owner_coords(w, owner, bb) else { return false };
                values_distance(w, &oc, &c).is_none_or(|d| d > r)
            }
            // Buckling and pulling are not simulated yet: never buckled/pulled.
            Precondition::Buckled { is_buckled } => !*is_buckled,
            Precondition::Pulled { is_pulled } => !*is_pulled,
            Precondition::InContainer { is_in_container } => w.get::<InContainer>(owner).is_some() == *is_in_container,
            Precondition::ActiveHandFree { invert } => {
                let free = w.get::<Hands>(owner).is_some_and(Hands::active_free);
                free != *invert
            }
            Precondition::ActiveHandComponent { components, invert } => {
                let Some(item) = w.get::<Hands>(owner).and_then(Hands::active_item) else { return *invert };
                let has = w.get::<EntityData>(item).is_some_and(|d| components.iter().all(|c| d.has(c)));
                has != *invert
            }
            Precondition::KeyFloatLess { key, value } => bb.f32(key).is_some_and(|v| v < *value),
            Precondition::Unknown(_) => false,
        }
    }

    fn plan_operator(&mut self, op: &Operator, bb: &Blackboard) -> Option<Effects> {
        let w = &mut *self.world;
        let owner = self.owner;
        match op {
            Operator::Wait { .. }
            | Operator::Juke
            | Operator::No
            | Operator::Speak { .. }
            | Operator::SayKey { .. }
            | Operator::RotateToTarget { .. } => Some(vec![]),
            Operator::Random {
                target_key,
                min_key,
                max_key,
            } => {
                let (lo, hi) = (bb.f32(min_key).unwrap_or(0.0), bb.f32(max_key).unwrap_or(1.0));
                let v = w.resource_mut::<SimRng>().range_f32(lo, hi);
                Some(vec![(target_key.clone(), Value::Float(v))])
            }
            Operator::SetFloat { target_key, amount } => Some(vec![(target_key.clone(), Value::Float(*amount))]),
            Operator::AddFloat { target_key, amount } => {
                Some(vec![(target_key.clone(), Value::Float(bb.f32(target_key).unwrap_or(0.0) + amount))])
            }
            Operator::SetRandomFloat {
                target_key,
                min_amount,
                max_amount,
            } => {
                let v = w.resource_mut::<SimRng>().range_f32(*min_amount, *max_amount);
                Some(vec![(target_key.clone(), Value::Float(v))])
            }
            Operator::PickRandomRotation { target_key } => {
                let v = w.resource_mut::<SimRng>().range_f32(-std::f32::consts::PI, std::f32::consts::PI);
                Some(vec![(target_key.clone(), Value::Float(v))])
            }
            Operator::PickAccessible {
                range_key,
                target_coordinates,
                pathfind_key: _,
            } => {
                let x = *w.get::<Xform>(owner)?;
                let parent = x.parent?;
                w.get::<athanor::transform::Grid>(parent)?;
                let range = bb.f32(range_key).unwrap_or(7.0);
                let mask = w
                    .get::<athanor::physics::Fixtures>(owner)
                    .map(|f| f.mask())
                    .filter(|m| *m != 0)
                    .unwrap_or(athanor::physics::group::by_name("MobMask").unwrap());
                let tiles = reachable_tiles(w, parent, tile_of(x.pos), range, mask);
                let t = *w.resource_mut::<SimRng>().pick(&tiles)?;
                Some(vec![(
                    target_coordinates.clone(),
                    Value::Coords {
                        parent,
                        pos: tile_center(t),
                    },
                )])
            }
            Operator::MoveTo {
                target_key,
                range_key,
                pathfind_in_planning,
                pathfind_key,
                ..
            } => {
                let target = bb.get(target_key)?;
                let range = bb.f32(range_key).unwrap_or(1.5);
                let oc = owner_coords(w, owner, bb)?;
                let d = values_distance(w, &oc, &target)?;
                if d <= range {
                    return Some(vec![("OwnerCoordinates".into(), oc)]);
                }
                if !*pathfind_in_planning {
                    return Some(vec![("OwnerCoordinates".into(), target)]);
                }
                // Same grid → the path must exist now (upstream fails the
                // plan on PathResult != Path). Off-grid → move directly.
                let x = *w.get::<Xform>(owner)?;
                let parent = x.parent?;
                if let (Some(_), Value::Coords { parent: tp, pos }) = (w.get::<athanor::transform::Grid>(parent), &target)
                    && *tp == parent
                {
                    let mask = w
                        .get::<athanor::physics::Fixtures>(owner)
                        .map(|f| f.mask())
                        .filter(|m| *m != 0)
                        .unwrap_or(athanor::physics::group::by_name("MobMask").unwrap());
                    let path = crate::nav::find_path(w, parent, tile_of(x.pos), *pos, range, mask)?;
                    return Some(vec![
                        ("OwnerCoordinates".into(), target.clone()),
                        (pathfind_key.clone(), Value::Path(path)),
                    ]);
                }
                Some(vec![("OwnerCoordinates".into(), target)])
            }
            Operator::Utility {
                proto,
                key,
                key_coordinates,
            } => {
                let best = run_query(w, self.domain, owner, bb, proto).into_iter().next()?.0;
                let coords = coords_of(w, best)?;
                Some(vec![(key.clone(), Value::Entity(best)), (key_coordinates.clone(), coords)])
            }
            Operator::Melee { target_key, target_state } => {
                let t = bb.entity(target_key)?;
                w.get_entity(t).ok()?;
                if w.get::<MobState>(t).is_some_and(|s| s.0 > *target_state) {
                    return None;
                }
                Some(vec![])
            }
            Operator::InteractWith { target_key } => bb.entity(target_key).map(|_| vec![]),
            Operator::Unknown(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Operator lifecycle
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Continuing,
    Failed,
    Finished,
}

fn startup(world: &mut World, owner: Entity, op: &Operator, bb: &mut Blackboard) {
    let now = world.resource::<SimClock>().time;
    match op {
        Operator::MoveTo {
            target_key,
            range_key,
            pathfind_key,
            ..
        } => {
            // The planning-only owner position must not leak into execution.
            bb.remove("OwnerCoordinates");
            if let Some(Value::Coords { parent, pos }) = bb.get(target_key) {
                let range = bb.f32(range_key).unwrap_or(1.5);
                let path = match bb.get(pathfind_key) {
                    Some(Value::Path(p)) => Some(p),
                    _ => None,
                };
                if let Some(mut s) = world.get_mut::<NpcSteering>(owner) {
                    s.go(SteerTarget::Coords { parent, pos }, range, path, now);
                }
            }
        }
        Operator::Melee { target_key, .. } => {
            if let Some(t) = bb.entity(target_key) {
                let miss = bb.f32("MeleeMissChance").unwrap_or(0.3);
                world.entity_mut(owner).insert(NpcMeleeCombat {
                    target: t,
                    miss_chance: miss,
                    status: CombatStatus::Normal,
                });
                if let Some(mut c) = world.get_mut::<CombatMode>(owner) {
                    c.active = true;
                }
            }
        }
        Operator::Speak { speech } => {
            let content = world.resource::<athanor::Content>().clone();
            let text = content.localize(speech);
            world.write_message(ChatLine {
                speaker: Some(owner),
                channel: ChatChannel::Local,
                text,
            });
        }
        Operator::RotateToTarget { target_key } => {
            if let (Some(a), Some(mut x)) = (bb.f32(target_key), world.get_mut::<Xform>(owner)) {
                x.rot = a;
            }
        }
        Operator::InteractWith { target_key } => {
            if let Some(t) = bb.entity(target_key) {
                athanor::interaction::interact(world, owner, t);
            }
        }
        _ => {}
    }
}

fn update(world: &mut World, owner: Entity, op: &Operator, bb: &mut Blackboard, dt: f32) -> Status {
    match op {
        Operator::Wait { key } => {
            let Some(t) = bb.f32(key) else { return Status::Finished };
            let t = t - dt;
            bb.set(key.clone(), Value::Float(t));
            if t <= 0.0 { Status::Finished } else { Status::Continuing }
        }
        Operator::MoveTo { .. } => match world.get::<NpcSteering>(owner).map(|s| s.status) {
            Some(SteeringStatus::InRange) => Status::Finished,
            Some(SteeringStatus::NoPath) | None => Status::Failed,
            Some(SteeringStatus::Moving) => Status::Continuing,
        },
        Operator::Melee { target_key, target_state } => {
            let Some(t) = bb.entity(target_key) else { return Status::Failed };
            if world.get_entity(t).is_err() {
                return Status::Failed;
            }
            if let Some(mut c) = world.get_mut::<NpcMeleeCombat>(owner) {
                c.target = t;
            }
            if world.get::<MobState>(t).is_some_and(|s| s.0 > *target_state) {
                return Status::Finished;
            }
            match world.get::<NpcMeleeCombat>(owner).map(|c| c.status) {
                Some(CombatStatus::Normal) | Some(CombatStatus::TargetOutOfRange) => Status::Continuing,
                _ => Status::Failed,
            }
        }
        Operator::Unknown(_) => Status::Failed,
        _ => Status::Finished,
    }
}

fn shutdown(world: &mut World, owner: Entity, op: &Operator, bb: &mut Blackboard) {
    match op {
        Operator::Wait { key } => bb.remove(key),
        Operator::MoveTo {
            target_key,
            remove_key_on_finish,
            pathfind_key,
            ..
        } => {
            if let Some(mut s) = world.get_mut::<NpcSteering>(owner) {
                s.stop();
            }
            if *remove_key_on_finish {
                bb.remove(target_key);
            }
            bb.remove(pathfind_key);
        }
        Operator::Melee { .. } => {
            world.entity_mut(owner).remove::<NpcMeleeCombat>();
            if let Some(mut c) = world.get_mut::<CombatMode>(owner) {
                c.active = false;
            }
            if let Some(mut s) = world.get_mut::<NpcSteering>(owner) {
                s.stop();
            }
        }
        _ => {}
    }
}

fn start_task(world: &mut World, owner: Entity, plan: &Plan, bb: &mut Blackboard) {
    let task = &plan.tasks[plan.index];
    if task.apply_effects_on_startup
        && let Some(Some(eff)) = plan.effects.get(plan.index)
    {
        bb.apply(eff);
    }
    startup(world, owner, &task.operator, bb);
}

fn shutdown_plan(world: &mut World, owner: Entity, plan: &Plan, bb: &mut Blackboard) {
    if let Some(t) = plan.current() {
        shutdown(world, owner, &t.operator, bb);
    }
}

/// One NPC's tick (`HTNSystem.Update`).
fn update_npc(world: &mut World, domain: &Domain, owner: Entity, dt: f32) {
    let Some(mut htn) = world.get::<Htn>(owner).cloned() else { return };
    if !htn.enabled {
        return;
    }

    htn.plan_accumulator -= dt;
    if (htn.constantly_replan || htn.plan.is_none()) && htn.plan_accumulator <= 0.0 {
        htn.plan_accumulator = htn.plan_cooldown;
        let root = htn.root.clone();
        let bb = htn.blackboard.clone();
        let new = {
            let mut ctx = Ctx { world, domain, owner };
            plan(domain, &root, &bb, &mut ctx)
        };
        let better = match (&htn.plan, &new) {
            (None, _) => true,
            (Some(old), Some(n)) => new_plan_better(&old.btr, &n.btr),
            (Some(_), None) => false,
        };
        if better {
            htn.check_services = false;
            if let Some(old) = htn.plan.take() {
                shutdown_plan(world, owner, &old, &mut htn.blackboard);
            }
            htn.plan = new;
            if let Some(p) = htn.plan.clone() {
                start_task(world, owner, &p, &mut htn.blackboard);
            }
        } else {
            htn.check_services = true;
        }
    }

    let mut guard = 0;
    while let Some(p) = htn.plan.clone() {
        guard += 1;
        if guard > 16 {
            break;
        }
        let task = p.tasks[p.index].clone();
        if htn.check_services {
            for s in &task.services {
                let best = run_query(world, domain, owner, &htn.blackboard, &s.query).into_iter().next();
                match best {
                    Some((e, _)) => htn.blackboard.set(s.key.clone(), Value::Entity(e)),
                    None => htn.blackboard.remove(&s.key),
                }
            }
            htn.check_services = false;
        }
        let status = update(world, owner, &task.operator, &mut htn.blackboard, dt);
        match status {
            Status::Continuing => break,
            Status::Failed => {
                shutdown(world, owner, &task.operator, &mut htn.blackboard);
                htn.plan = None;
            }
            Status::Finished => {
                shutdown(world, owner, &task.operator, &mut htn.blackboard);
                let mut np = p.clone();
                np.index += 1;
                if np.index >= np.tasks.len() {
                    htn.plan = None;
                } else {
                    start_task(world, owner, &np, &mut htn.blackboard);
                    htn.plan = Some(np);
                }
            }
        }
    }
    world.entity_mut(owner).insert(htn);
}

/// All NPC minds, once per tick.
pub fn htn_system(world: &mut World) {
    let dt = world.resource::<SimClock>().dt;
    let npcs: Vec<Entity> = {
        let mut q = world.query_filtered::<(Entity, Option<&MobState>), (With<Htn>, Without<Player>)>();
        q.iter(world)
            .filter(|(_, s)| s.is_none_or(|s| !s.is_incapacitated()))
            .map(|(e, _)| e)
            .collect()
    };
    world.resource_scope(|world, domain: Mut<Domain>| {
        for e in npcs {
            update_npc(world, &domain, e, dt);
        }
    });
}

/// Dead or critical NPCs stop thinking and moving (`NPCSystem.SleepNPC`).
pub fn sleep_incapacitated(world: &mut World) {
    let sleepers: Vec<Entity> = {
        let mut q = world.query_filtered::<(Entity, &MobState), With<Htn>>();
        q.iter(world).filter(|(_, s)| s.is_incapacitated()).map(|(e, _)| e).collect()
    };
    for e in sleepers {
        if let Some(mut h) = world.get_mut::<Htn>(e) {
            if h.plan.is_none() {
                continue;
            }
            h.plan = None;
        }
        world.entity_mut(e).remove::<NpcMeleeCombat>();
        if let Some(mut s) = world.get_mut::<NpcSteering>(e) {
            s.stop();
        }
        if let Some(mut m) = world.get_mut::<InputMover>(e) {
            m.wish = Vec2::ZERO;
        }
    }
}

/// NPC melee: close the distance, face the target, swing with a miss
/// chance (`NPCCombatSystem.Melee`).
pub fn melee_system(world: &mut World) {
    let now = world.resource::<SimClock>().time;
    let fighters: Vec<(Entity, NpcMeleeCombat)> = {
        let mut q = world.query::<(Entity, &NpcMeleeCombat)>();
        q.iter(world).map(|(e, c)| (e, *c)).collect()
    };
    for (e, mut c) in fighters {
        if world.get_entity(c.target).is_err() {
            c.status = CombatStatus::TargetUnreachable;
            world.entity_mut(e).insert(c);
            continue;
        }
        let Some(weapon) = weapon_of(world, e) else {
            c.status = CombatStatus::TargetUnreachable;
            world.entity_mut(e).insert(c);
            continue;
        };
        let range = world.get::<MeleeWeapon>(weapon).map_or(1.5, |w| w.range);
        let d = distance(world, e, c.target);
        if !d.is_finite() {
            c.status = CombatStatus::TargetUnreachable;
            world.entity_mut(e).insert(c);
            continue;
        }
        if d > range {
            c.status = CombatStatus::TargetOutOfRange;
            let target = c.target;
            if let Some(mut s) = world.get_mut::<NpcSteering>(e) {
                let need = s.target != Some(SteerTarget::Entity(target)) || s.status == SteeringStatus::InRange;
                if need {
                    s.go(SteerTarget::Entity(target), (range * 0.7).max(0.5), None, now);
                }
                if s.status == SteeringStatus::NoPath {
                    c.status = CombatStatus::TargetUnreachable;
                }
            }
            world.entity_mut(e).insert(c);
            continue;
        }
        c.status = CombatStatus::Normal;
        world.entity_mut(e).insert(c);
        if let Some(mut s) = world.get_mut::<NpcSteering>(e) {
            s.stop();
        }
        // Face the target.
        let (op, tp) = (world_pose_of(world, e), world_pose_of(world, c.target));
        let dir = tp.pos - op.pos;
        if dir.length_squared() > 1e-6
            && let Some(mut x) = world.get_mut::<Xform>(e)
        {
            x.rot = vec_to_angle(dir);
        }
        let ready = world.get::<MeleeWeapon>(weapon).is_some_and(|w| now >= w.next_attack);
        if !ready {
            continue;
        }
        if world.resource_mut::<SimRng>().chance(c.miss_chance) {
            let rate = world.get::<MeleeWeapon>(weapon).map_or(1.0, |w| w.attack_rate);
            if let Some(mut w) = world.get_mut::<MeleeWeapon>(weapon) {
                w.next_attack = now + 1.0 / rate.max(0.01) as f64;
            }
            world.write_message(MeleeSwing {
                attacker: e,
                target: Some(c.target),
                hit: false,
                animation: None,
            });
            continue;
        }
        let r = try_melee(world, e, c.target);
        debug_assert!(r != AttackResult::NoWeapon);
    }
}
