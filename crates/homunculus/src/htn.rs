//! The HTN domain (parsed from `htnCompound` prototypes) and the planner.
//! The planner is a direct port of `HTNPlanJob.Process`: depth-first
//! decomposition with rollback to the last decomposed compound, recording
//! the branch traversal so plans can be compared by priority.

use std::collections::HashMap;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use grimoire::{Node, PrototypeManager};

use crate::blackboard::{Blackboard, Effects};
use crate::ops::{Operator, Precondition};
use crate::utility::{Curve, UtilityQuery};

#[derive(Debug, Clone)]
pub enum Task {
    Compound(String),
    Primitive(Arc<Primitive>),
}

#[derive(Debug)]
pub struct Primitive {
    pub operator: Operator,
    pub preconditions: Vec<Precondition>,
    pub services: Vec<Service>,
    pub apply_effects_on_startup: bool,
}

#[derive(Debug, Clone)]
pub struct Service {
    pub id: String,
    pub query: String,
    pub key: String,
}

#[derive(Debug, Clone)]
pub struct Branch {
    pub preconditions: Vec<Precondition>,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone)]
pub struct Compound {
    pub id: String,
    pub branches: Vec<Branch>,
}

/// Everything the planner needs from content, parsed once.
#[derive(Resource, Debug, Default)]
pub struct Domain {
    pub compounds: HashMap<String, Compound>,
    pub queries: HashMap<String, UtilityQuery>,
    pub curves: HashMap<String, Curve>,
    /// Operator/precondition types seen in content but not ported yet.
    pub unported: Vec<String>,
}

impl Domain {
    pub fn from_prototypes(pm: &PrototypeManager) -> Domain {
        let mut d = Domain::default();
        let mut unported = std::collections::BTreeSet::new();
        for p in pm.iter_kind("htnCompound") {
            let mut branches = Vec::new();
            for b in p.get("branches").and_then(Node::as_seq).unwrap_or_default() {
                branches.push(Branch {
                    preconditions: parse_preconditions(b.get("preconditions"), &mut unported),
                    tasks: b
                        .get("tasks")
                        .and_then(Node::as_seq)
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|t| parse_task(t, &mut unported))
                        .collect(),
                });
            }
            d.compounds.insert(
                p.id.clone(),
                Compound {
                    id: p.id.clone(),
                    branches,
                },
            );
        }
        for p in pm.iter_kind("utilityCurvePreset") {
            if let Some(c) = p.get("curve").and_then(Curve::parse) {
                d.curves.insert(p.id.clone(), c);
            }
        }
        for p in pm.iter_kind("utilityQuery") {
            d.queries.insert(p.id.clone(), UtilityQuery::parse(p, &mut unported));
        }
        d.unported = unported.into_iter().collect();
        d
    }
}

fn parse_preconditions(n: Option<&Node>, unported: &mut std::collections::BTreeSet<String>) -> Vec<Precondition> {
    n.and_then(Node::as_seq)
        .unwrap_or_default()
        .iter()
        .map(|p| {
            let pc = Precondition::parse(p);
            if let Precondition::Unknown(name) = &pc {
                unported.insert(name.clone());
            }
            pc
        })
        .collect()
}

fn parse_task(n: &Node, unported: &mut std::collections::BTreeSet<String>) -> Option<Task> {
    match n.type_tag()? {
        "HTNCompoundTask" => Some(Task::Compound(n.get_str("task")?.to_string())),
        "HTNPrimitiveTask" => {
            let op = Operator::parse(n.get("operator")?);
            if let Operator::Unknown(name) = &op {
                unported.insert(name.clone());
            }
            let services = n
                .get("services")
                .and_then(Node::as_seq)
                .unwrap_or_default()
                .iter()
                .filter(|s| s.type_tag() == Some("UtilityService"))
                .filter_map(|s| {
                    Some(Service {
                        id: s.get_str("id").unwrap_or("").to_string(),
                        query: s.get_str("proto")?.to_string(),
                        key: s.get_str("key").unwrap_or("Target").to_string(),
                    })
                })
                .collect();
            Some(Task::Primitive(Arc::new(Primitive {
                operator: op,
                preconditions: parse_preconditions(n.get("preconditions"), unported),
                services,
                apply_effects_on_startup: n.get("applyEffectsOnStartup").and_then(Node::as_bool).unwrap_or(true),
            })))
        }
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub tasks: Vec<Arc<Primitive>>,
    /// Branch index chosen at each decomposed compound, in order.
    pub btr: Vec<usize>,
    pub effects: Vec<Option<Effects>>,
    pub index: usize,
}

impl Plan {
    pub fn current(&self) -> Option<&Arc<Primitive>> {
        self.tasks.get(self.index)
    }
}

/// Upstream's comparison: the new plan is better if, at some depth, the old
/// plan took a later (lower-priority) branch than the new one.
pub fn new_plan_better(old: &[usize], new: &[usize]) -> bool {
    for i in 0..old.len() {
        if i < new.len() && old[i] > new[i] {
            return true;
        }
    }
    false
}

/// What the planner needs from the world while planning.
pub trait PlanContext {
    fn precondition_met(&mut self, pc: &Precondition, bb: &Blackboard) -> bool;
    /// `HTNOperator.Plan`: validity + effects.
    fn plan_operator(&mut self, op: &Operator, bb: &Blackboard) -> Option<Effects>;
}

struct Decomp {
    bb: Blackboard,
    compound: String,
    btr: usize,
    prim_count: usize,
}

const MAX_TASKS: usize = 2000;

pub fn plan(domain: &Domain, root: &str, bb: &Blackboard, ctx: &mut dyn PlanContext) -> Option<Plan> {
    let mut bb = bb.clone();
    let mut history: Vec<Decomp> = Vec::new();
    let mut btr_index = 0usize;
    let mut applied: Vec<Option<Effects>> = Vec::new();
    let mut stack: Vec<Task> = vec![Task::Compound(root.to_string())];
    let mut final_plan: Vec<Arc<Primitive>> = Vec::new();
    let mut prim_count = 0usize;
    let mut processed = 0usize;

    let restore = |history: &mut Vec<Decomp>,
                   stack: &mut Vec<Task>,
                   applied: &mut Vec<Option<Effects>>,
                   final_plan: &mut Vec<Arc<Primitive>>,
                   prim_count: &mut usize,
                   bb: &mut Blackboard,
                   btr_index: &mut usize| {
        stack.clear();
        let Some(last) = history.pop() else { return };
        *btr_index = last.btr + 1;
        let keep = final_plan.len() - *prim_count;
        final_plan.truncate(keep);
        applied.truncate(keep);
        *prim_count = last.prim_count;
        *bb = last.bb;
        stack.push(Task::Compound(last.compound));
    };

    while let Some(task) = stack.pop() {
        processed += 1;
        if processed > MAX_TASKS {
            tracing::warn!("HTN planner exceeded {MAX_TASKS} tasks for root {root}");
            return None;
        }
        match task {
            Task::Compound(id) => {
                let found = domain.compounds.get(&id).and_then(|c| {
                    (btr_index..c.branches.len()).find(|&i| {
                        c.branches[i].preconditions.iter().all(|p| ctx.precondition_met(p, &bb))
                    })
                });
                match found {
                    Some(i) => {
                        let c = &domain.compounds[&id];
                        for t in c.branches[i].tasks.iter().rev() {
                            stack.push(t.clone());
                        }
                        history.push(Decomp {
                            bb: bb.clone(),
                            compound: id,
                            btr: i,
                            prim_count,
                        });
                        prim_count = 0;
                        btr_index = 0;
                    }
                    None => restore(
                        &mut history,
                        &mut stack,
                        &mut applied,
                        &mut final_plan,
                        &mut prim_count,
                        &mut bb,
                        &mut btr_index,
                    ),
                }
            }
            Task::Primitive(p) => {
                let ok = p.preconditions.iter().all(|pc| ctx.precondition_met(pc, &bb));
                let effects = if ok { ctx.plan_operator(&p.operator, &bb) } else { None };
                match effects {
                    Some(eff) => {
                        bb.apply(&eff);
                        applied.push(if eff.is_empty() { None } else { Some(eff) });
                        prim_count += 1;
                        final_plan.push(p);
                    }
                    None => restore(
                        &mut history,
                        &mut stack,
                        &mut applied,
                        &mut final_plan,
                        &mut prim_count,
                        &mut bb,
                        &mut btr_index,
                    ),
                }
            }
        }
    }

    if final_plan.is_empty() {
        return None;
    }
    Some(Plan {
        tasks: final_plan,
        btr: history.iter().map(|d| d.btr).collect(),
        effects: applied,
        index: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackboard::Value;

    /// A context where `KeyExists` reads the blackboard and every operator
    /// plans successfully unless it is `Unknown`.
    struct TestCtx;
    impl PlanContext for TestCtx {
        fn precondition_met(&mut self, pc: &Precondition, bb: &Blackboard) -> bool {
            match pc {
                Precondition::KeyExists { key, invert } => bb.has(key) != *invert,
                _ => false,
            }
        }
        fn plan_operator(&mut self, op: &Operator, _bb: &Blackboard) -> Option<Effects> {
            match op {
                Operator::Unknown(_) => None,
                Operator::Random { target_key, .. } => Some(vec![(target_key.clone(), Value::Float(3.0))]),
                _ => Some(vec![]),
            }
        }
    }

    fn domain(yaml: &str) -> Domain {
        let mut pm = PrototypeManager::new();
        pm.load_str(yaml, vellum::ResPath::new("/test.yml"));
        pm.resolve();
        Domain::from_prototypes(&pm)
    }

    const IDLE: &str = r#"
- type: htnCompound
  id: Root
  branches:
  - tasks:
    - !type:HTNCompoundTask
      task: Combat
  - tasks:
    - !type:HTNCompoundTask
      task: Idle

- type: htnCompound
  id: Combat
  branches:
  - tasks:
    - !type:HTNPrimitiveTask
      operator: !type:NotPortedOperator
        foo: 1

- type: htnCompound
  id: Idle
  branches:
  - tasks:
    - !type:HTNPrimitiveTask
      operator: !type:WaitOperator
        key: IdleTime
      preconditions:
      - !type:KeyExistsPrecondition
        key: IdleTime
  - tasks:
    - !type:HTNPrimitiveTask
      operator: !type:RandomOperator
        targetKey: IdleTime
        minKey: MinimumIdleTime
        maxKey: MaximumIdleTime
    - !type:HTNPrimitiveTask
      operator: !type:WaitOperator
        key: IdleTime
      preconditions:
      - !type:KeyExistsPrecondition
        key: IdleTime
"#;

    #[test]
    fn plans_with_rollback() {
        let d = domain(IDLE);
        assert!(d.unported.contains(&"NotPortedOperator".to_string()));
        let bb = Blackboard::default();
        let p = plan(&d, "Root", &bb, &mut TestCtx).expect("a plan");
        // Combat fails → roll back → Idle; Idle branch 0 needs IdleTime → branch 1.
        assert_eq!(p.btr, vec![1, 1]);
        assert_eq!(p.tasks.len(), 2, "random then wait (wait's precondition satisfied by random's effect)");
        assert!(matches!(p.tasks[0].operator, Operator::Random { .. }));
        assert!(matches!(p.tasks[1].operator, Operator::Wait { .. }));

        let mut bb2 = Blackboard::default();
        bb2.set("IdleTime", Value::Float(1.0));
        let p2 = plan(&d, "Root", &bb2, &mut TestCtx).unwrap();
        assert_eq!(p2.btr, vec![1, 0]);
        assert!(new_plan_better(&p.btr, &p2.btr));
        assert!(!new_plan_better(&p2.btr, &p.btr));
    }
}
