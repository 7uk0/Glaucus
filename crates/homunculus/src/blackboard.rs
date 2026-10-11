//! The NPC blackboard (`NPCBlackboard`): a typed key/value store that the
//! planner reads preconditions from and operators write effects into.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use bevy_math::{IVec2, Vec2};
use grimoire::Node;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Float(f32),
    Bool(bool),
    Str(String),
    Entity(Entity),
    /// A position local to a grid (`EntityCoordinates`).
    Coords { parent: Entity, pos: Vec2 },
    Path(Vec<IVec2>),
}

impl Value {
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }
    pub fn as_entity(&self) -> Option<Entity> {
        match self {
            Value::Entity(e) => Some(*e),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

pub type Effects = Vec<(String, Value)>;

/// Upstream `NPCBlackboard.BlackboardDefaults`.
pub fn default_value(key: &str) -> Option<Value> {
    let f = match key {
        "BufferRange" => 10.0,
        "FollowCloseRange" => 3.0,
        "FollowRange" => 7.0,
        "IdleRange" => 7.0,
        "InteractRange" => athanor::interaction::INTERACTION_RANGE,
        "MaximumIdleTime" => 7.0,
        "MedibotInjectRange" => 4.0,
        "MeleeMissChance" => 0.3,
        "MeleeRange" => 1.0,
        "MinimumIdleTime" => 2.0,
        "MovementRangeClose" => 0.2,
        "MovementRange" => 1.5,
        "RangedRange" => 10.0,
        "RotateSpeed" => f32::MAX,
        "VisionRadius" => 10.0,
        "AggroVisionRadius" => 10.0,
        _ => return None,
    };
    Some(Value::Float(f))
}

#[derive(Debug, Clone, Default)]
pub struct Blackboard {
    map: HashMap<String, Value>,
}

impl Blackboard {
    pub fn get(&self, key: &str) -> Option<Value> {
        self.map.get(key).cloned().or_else(|| default_value(key))
    }

    pub fn has(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    pub fn f32(&self, key: &str) -> Option<f32> {
        self.get(key).and_then(|v| v.as_f32())
    }

    pub fn entity(&self, key: &str) -> Option<Entity> {
        self.map.get(key).and_then(Value::as_entity)
    }

    pub fn set(&mut self, key: impl Into<String>, v: Value) {
        self.map.insert(key.into(), v);
    }

    pub fn remove(&mut self, key: &str) {
        self.map.remove(key);
    }

    pub fn apply(&mut self, effects: &Effects) {
        for (k, v) in effects {
            self.set(k.clone(), v.clone());
        }
    }

    /// Load the `blackboard:` mapping of an `HTN` component
    /// (`!type:Single 3.1`, `!type:Bool true`, `!type:String foo`).
    pub fn from_yaml(node: Option<&Node>) -> Blackboard {
        let mut bb = Blackboard::default();
        let Some(m) = node.and_then(Node::as_map) else { return bb };
        for (k, v) in m {
            let val = match v.type_tag() {
                Some("Single") | Some("Double") | Some("Int32") => v.as_f32().map(Value::Float),
                Some("Bool") => v.as_bool().map(Value::Bool),
                Some("String") => v.as_str().map(|s| Value::Str(s.to_string())),
                _ => None,
            };
            if let Some(val) = val {
                bb.set(k.clone(), val);
            }
        }
        bb
    }
}
