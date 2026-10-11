//! HTN operators and preconditions as data, parsed from their `!type:` tags.
//! Their behaviour lives in [`crate::mind`], which has world access.
//! Field names and defaults follow the upstream C# classes.

use athanor::damage::MobStateKind;
use grimoire::Node;

#[derive(Debug, Clone, PartialEq)]
pub enum Operator {
    Wait {
        key: String,
    },
    Random {
        target_key: String,
        min_key: String,
        max_key: String,
    },
    SetFloat {
        target_key: String,
        amount: f32,
    },
    AddFloat {
        target_key: String,
        amount: f32,
    },
    SetRandomFloat {
        target_key: String,
        min_amount: f32,
        max_amount: f32,
    },
    PickAccessible {
        range_key: String,
        target_coordinates: String,
        pathfind_key: String,
    },
    MoveTo {
        target_key: String,
        pathfind_in_planning: bool,
        range_key: String,
        pathfind_key: String,
        remove_key_on_finish: bool,
    },
    Utility {
        proto: String,
        key: String,
        key_coordinates: String,
    },
    Melee {
        target_key: String,
        target_state: MobStateKind,
    },
    Juke,
    Speak {
        speech: String,
    },
    SayKey {
        key: String,
    },
    RotateToTarget {
        target_key: String,
    },
    PickRandomRotation {
        target_key: String,
    },
    InteractWith {
        target_key: String,
    },
    No,
    Unknown(String),
}

fn s(n: &Node, k: &str, d: &str) -> String {
    n.get_str(k).unwrap_or(d).to_string()
}

fn f(n: &Node, k: &str, d: f32) -> f32 {
    n.get(k).and_then(Node::as_f32).unwrap_or(d)
}

fn b(n: &Node, k: &str, d: bool) -> bool {
    n.get(k).and_then(Node::as_bool).unwrap_or(d)
}

impl Operator {
    pub fn parse(n: &Node) -> Operator {
        let Some(tag) = n.type_tag() else {
            return Operator::Unknown("<untagged>".into());
        };
        match tag {
            "WaitOperator" => Operator::Wait { key: s(n, "key", "") },
            "RandomOperator" => Operator::Random {
                target_key: s(n, "targetKey", ""),
                min_key: s(n, "minKey", ""),
                max_key: s(n, "maxKey", ""),
            },
            "SetFloatOperator" => Operator::SetFloat {
                target_key: s(n, "targetKey", ""),
                amount: f(n, "amount", 0.0),
            },
            "AddFloatOperator" => Operator::AddFloat {
                target_key: s(n, "targetKey", ""),
                amount: f(n, "amount", 0.0),
            },
            "SetRandomFloatOperator" => Operator::SetRandomFloat {
                target_key: s(n, "targetKey", ""),
                min_amount: f(n, "minAmount", 0.0),
                max_amount: f(n, "maxAmount", 1.0),
            },
            "PickAccessibleOperator" => Operator::PickAccessible {
                range_key: s(n, "rangeKey", "IdleRange"),
                target_coordinates: s(n, "targetCoordinates", "TargetCoordinates"),
                pathfind_key: s(n, "pathfindKey", "MovementPathfind"),
            },
            "MoveToOperator" => Operator::MoveTo {
                target_key: s(n, "targetKey", "TargetCoordinates"),
                pathfind_in_planning: b(n, "pathfindInPlanning", true),
                range_key: s(n, "rangeKey", "MovementRange"),
                pathfind_key: s(n, "pathfindKey", "MovementPathfind"),
                remove_key_on_finish: b(n, "removeKeyOnFinish", true),
            },
            "UtilityOperator" => Operator::Utility {
                proto: s(n, "proto", ""),
                key: s(n, "key", "Target"),
                key_coordinates: s(n, "keyCoordinates", "TargetCoordinates"),
            },
            "MeleeOperator" => Operator::Melee {
                target_key: s(n, "targetKey", "Target"),
                target_state: match n.get_str("targetState") {
                    Some("Critical") => MobStateKind::Critical,
                    Some("Dead") => MobStateKind::Dead,
                    _ => MobStateKind::Alive,
                },
            },
            "JukeOperator" => Operator::Juke,
            "SpeakOperator" => Operator::Speak {
                speech: s(n, "speech", ""),
            },
            "SayKeyOperator" => Operator::SayKey { key: s(n, "key", "") },
            "RotateToTargetOperator" => Operator::RotateToTarget {
                target_key: s(n, "targetKey", "RotateTarget"),
            },
            "PickRandomRotationOperator" => Operator::PickRandomRotation {
                target_key: s(n, "targetKey", "RotateTarget"),
            },
            "InteractWithOperator" => Operator::InteractWith {
                target_key: s(n, "targetKey", "Target"),
            },
            "NoOperator" => Operator::No,
            other => Operator::Unknown(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Precondition {
    KeyExists { key: String, invert: bool },
    TargetInRange { target_key: String, range_key: String },
    TargetInLos { target_key: String, range_key: String },
    CoordinatesInRange { target_key: String, range_key: String },
    CoordinatesNotInRange { target_key: String, range_key: String },
    Buckled { is_buckled: bool },
    Pulled { is_pulled: bool },
    InContainer { is_in_container: bool },
    ActiveHandFree { invert: bool },
    ActiveHandComponent { components: Vec<String>, invert: bool },
    KeyFloatLess { key: String, value: f32 },
    Unknown(String),
}

impl Precondition {
    pub fn parse(n: &Node) -> Precondition {
        let Some(tag) = n.type_tag() else {
            return Precondition::Unknown("<untagged>".into());
        };
        match tag {
            "KeyExistsPrecondition" => Precondition::KeyExists {
                key: s(n, "key", ""),
                invert: b(n, "invert", false),
            },
            "TargetInRangePrecondition" => Precondition::TargetInRange {
                target_key: s(n, "targetKey", "Target"),
                range_key: s(n, "rangeKey", "MeleeRange"),
            },
            "TargetInLOSPrecondition" => Precondition::TargetInLos {
                target_key: s(n, "targetKey", "Target"),
                range_key: s(n, "rangeKey", "VisionRadius"),
            },
            "CoordinatesInRangePrecondition" => Precondition::CoordinatesInRange {
                target_key: s(n, "targetKey", "TargetCoordinates"),
                range_key: s(n, "rangeKey", "MovementRange"),
            },
            "CoordinatesNotInRangePrecondition" => Precondition::CoordinatesNotInRange {
                target_key: s(n, "targetKey", "TargetCoordinates"),
                range_key: s(n, "rangeKey", "MovementRange"),
            },
            "BuckledPrecondition" => Precondition::Buckled {
                is_buckled: b(n, "isBuckled", true),
            },
            "PulledPrecondition" => Precondition::Pulled {
                is_pulled: b(n, "isPulled", true),
            },
            "InContainerPrecondition" => Precondition::InContainer {
                is_in_container: b(n, "isInContainer", true),
            },
            "ActiveHandFreePrecondition" => Precondition::ActiveHandFree {
                invert: b(n, "invert", false),
            },
            "ActiveHandComponentPrecondition" => Precondition::ActiveHandComponent {
                components: n
                    .get("components")
                    .and_then(Node::as_seq)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|c| c.get_str("type").map(str::to_owned))
                    .collect(),
                invert: b(n, "invert", false),
            },
            "KeyFloatLessPrecondition" => Precondition::KeyFloatLess {
                key: s(n, "key", ""),
                value: f(n, "value", 0.0),
            },
            other => Precondition::Unknown(other.to_string()),
        }
    }
}
