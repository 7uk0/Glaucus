//! Data nodes — the untyped tree every prototype, map and component is read
//! from. Mirrors Robust's `DataNode` family (`ValueDataNode`,
//! `SequenceDataNode`, `MappingDataNode`): scalars stay as strings until a
//! typed reader asks for something more specific, and YAML tags such as
//! `!type:PhysShapeCircle` are preserved.

use std::fmt;

use indexmap::IndexMap;

pub type Mapping = IndexMap<String, Node>;

#[derive(Clone, PartialEq)]
pub enum Node {
    Null,
    Value(String),
    Seq(Vec<Node>),
    Map(Mapping),
    /// A tagged node. The tag is stored without the leading `!`
    /// (`type:PhysShapeCircle`).
    Tagged(String, Box<Node>),
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Node::Null => f.write_str("~"),
            Node::Value(v) => write!(f, "{v:?}"),
            Node::Seq(s) => f.debug_list().entries(s).finish(),
            Node::Map(m) => f.debug_map().entries(m.iter()).finish(),
            Node::Tagged(t, n) => write!(f, "!{t} {n:?}"),
        }
    }
}

impl Node {
    pub fn value(s: impl Into<String>) -> Node {
        Node::Value(s.into())
    }

    /// Strip any tag wrappers.
    pub fn untagged(&self) -> &Node {
        let mut n = self;
        while let Node::Tagged(_, inner) = n {
            n = inner;
        }
        n
    }

    pub fn tag(&self) -> Option<&str> {
        match self {
            Node::Tagged(t, _) => Some(t),
            _ => None,
        }
    }

    /// For `!type:Foo` tags, returns `Foo`.
    pub fn type_tag(&self) -> Option<&str> {
        self.tag().and_then(|t| t.strip_prefix("type:"))
    }

    pub fn is_null(&self) -> bool {
        matches!(self.untagged(), Node::Null)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self.untagged() {
            Node::Value(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&Mapping> {
        match self.untagged() {
            Node::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_map_mut(&mut self) -> Option<&mut Mapping> {
        let mut n = self;
        while let Node::Tagged(_, inner) = n {
            n = inner;
        }
        match n {
            Node::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_seq(&self) -> Option<&[Node]> {
        match self.untagged() {
            Node::Seq(s) => Some(s),
            _ => None,
        }
    }

    /// Mapping lookup (through tags).
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.as_map().and_then(|m| m.get(key))
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Node::as_str)
    }

    pub fn as_bool(&self) -> Option<bool> {
        parse_bool(self.as_str()?)
    }

    pub fn as_f64(&self) -> Option<f64> {
        parse_f64(self.as_str()?)
    }

    pub fn as_f32(&self) -> Option<f32> {
        self.as_f64().map(|v| v as f32)
    }

    pub fn as_i64(&self) -> Option<i64> {
        let s = self.as_str()?.trim();
        s.parse::<i64>().ok().or_else(|| parse_f64(s).map(|f| f as i64))
    }

    /// `"x,y"` → `(x, y)`.
    pub fn as_vec2(&self) -> Option<(f32, f32)> {
        parse_vec2(self.as_str()?)
    }

    /// Robust angle: `"1.57 rad"` or plain degrees. Returns radians.
    pub fn as_angle(&self) -> Option<f64> {
        parse_angle(self.as_str()?)
    }

    /// Robust `TimeSpan`: plain seconds or `1.5s` / `2m` / `1h` / `1d`.
    pub fn as_seconds(&self) -> Option<f64> {
        parse_timespan(self.as_str()?)
    }

    /// A scalar or a sequence of scalars, as a list of strings.
    pub fn as_str_list(&self) -> Vec<String> {
        match self.untagged() {
            Node::Value(v) => vec![v.clone()],
            Node::Seq(s) => s.iter().filter_map(|n| n.as_str().map(str::to_owned)).collect(),
            _ => Vec::new(),
        }
    }

    /// Deep-ish size estimate (number of nodes), handy for diagnostics.
    pub fn node_count(&self) -> usize {
        match self {
            Node::Null | Node::Value(_) => 1,
            Node::Seq(s) => 1 + s.iter().map(Node::node_count).sum::<usize>(),
            Node::Map(m) => 1 + m.values().map(Node::node_count).sum::<usize>(),
            Node::Tagged(_, n) => n.node_count(),
        }
    }
}

pub fn parse_bool(s: &str) -> Option<bool> {
    match s.trim() {
        t if t.eq_ignore_ascii_case("true") => Some(true),
        t if t.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

pub fn parse_f64(s: &str) -> Option<f64> {
    let s = s.trim();
    match s {
        ".inf" | ".Inf" | ".INF" | "inf" | "Infinity" => Some(f64::INFINITY),
        "-.inf" | "-.Inf" | "-.INF" | "-inf" | "-Infinity" => Some(f64::NEG_INFINITY),
        ".nan" | ".NaN" | "NaN" => Some(f64::NAN),
        _ => s.parse::<f64>().ok(),
    }
}

/// Port of `TimeSpanExt.TryTimeSpan`. Returns seconds.
pub fn parse_timespan(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.contains(',') || s.contains(' ') || s.contains(':') {
        return None;
    }
    if let Ok(v) = s.parse::<f64>() {
        return Some(v);
    }
    if s.len() <= 1 {
        return None;
    }
    let (num, unit) = s.split_at(s.len() - 1);
    let n: f64 = num.parse().ok()?;
    Some(match unit {
        "s" | "S" => n,
        "m" | "M" => n * 60.0,
        "h" | "H" => n * 3600.0,
        "d" | "D" => n * 86400.0,
        _ => return None,
    })
}

pub fn parse_vec2(s: &str) -> Option<(f32, f32)> {
    let (a, b) = s.split_once(',')?;
    Some((parse_f64(a)? as f32, parse_f64(b)? as f32))
}

pub fn parse_angle(s: &str) -> Option<f64> {
    let s = s.trim();
    if let Some(r) = s.strip_suffix("rad") {
        parse_f64(r)
    } else if let Some(d) = s.strip_suffix("deg") {
        parse_f64(d).map(f64::to_radians)
    } else {
        parse_f64(s).map(f64::to_radians)
    }
}

/// Parse a Robust colour: `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA` or a
/// named colour. Returns linear-agnostic sRGB bytes `[r, g, b, a]`.
pub fn parse_color(s: &str) -> Option<[u8; 4]> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let nib = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        let b = hex.as_bytes();
        return match b.len() {
            3 | 4 => {
                let mut out = [255u8; 4];
                for (i, c) in b.iter().enumerate() {
                    let v = nib(*c)?;
                    out[i] = v * 17;
                }
                Some(out)
            }
            6 | 8 => {
                let mut out = [255u8; 4];
                for i in 0..b.len() / 2 {
                    out[i] = nib(b[2 * i])? * 16 + nib(b[2 * i + 1])?;
                }
                Some(out)
            }
            _ => None,
        };
    }
    named_color(s)
}

fn named_color(s: &str) -> Option<[u8; 4]> {
    let c = match s.to_ascii_lowercase().as_str() {
        "white" => [255, 255, 255, 255],
        "black" => [0, 0, 0, 255],
        "red" => [255, 0, 0, 255],
        "green" => [0, 128, 0, 255],
        "lime" => [0, 255, 0, 255],
        "blue" => [0, 0, 255, 255],
        "yellow" => [255, 255, 0, 255],
        "cyan" | "aqua" => [0, 255, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255, 255],
        "orange" => [255, 165, 0, 255],
        "purple" => [128, 0, 128, 255],
        "pink" => [255, 192, 203, 255],
        "brown" => [165, 42, 42, 255],
        "gray" | "grey" => [128, 128, 128, 255],
        "darkgray" | "darkgrey" => [169, 169, 169, 255],
        "lightgray" | "lightgrey" => [211, 211, 211, 255],
        "dimgray" | "dimgrey" => [105, 105, 105, 255],
        "transparent" => [0, 0, 0, 0],
        "gold" => [255, 215, 0, 255],
        "silver" => [192, 192, 192, 255],
        "violet" => [238, 130, 238, 255],
        "navy" => [0, 0, 128, 255],
        "teal" => [0, 128, 128, 255],
        "olive" => [128, 128, 0, 255],
        "maroon" => [128, 0, 0, 255],
        "crimson" => [220, 20, 60, 255],
        "lightblue" => [173, 216, 230, 255],
        "lightgreen" => [144, 238, 144, 255],
        "darkred" => [139, 0, 0, 255],
        "darkgreen" => [0, 100, 0, 255],
        "darkblue" => [0, 0, 139, 255],
        "tomato" => [255, 99, 71, 255],
        "salmon" => [250, 128, 114, 255],
        "khaki" => [240, 230, 140, 255],
        "tan" => [210, 180, 140, 255],
        "beige" => [245, 245, 220, 255],
        "turquoise" => [64, 224, 208, 255],
        "indigo" => [75, 0, 130, 255],
        "orchid" => [218, 112, 214, 255],
        "plum" => [221, 160, 221, 255],
        "coral" => [255, 127, 80, 255],
        "chocolate" => [210, 105, 30, 255],
        "deepskyblue" => [0, 191, 255, 255],
        "dodgerblue" => [30, 144, 255, 255],
        "limegreen" => [50, 205, 50, 255],
        "springgreen" => [0, 255, 127, 255],
        "hotpink" => [255, 105, 180, 255],
        "deeppink" => [255, 20, 147, 255],
        "orangered" => [255, 69, 0, 255],
        "darkorange" => [255, 140, 0, 255],
        "goldenrod" => [218, 165, 32, 255],
        "slategray" | "slategrey" => [112, 128, 144, 255],
        "steelblue" => [70, 130, 180, 255],
        "royalblue" => [65, 105, 225, 255],
        "mediumpurple" => [147, 112, 219, 255],
        "darkviolet" => [148, 0, 211, 255],
        "firebrick" => [178, 34, 34, 255],
        "sienna" => [160, 82, 45, 255],
        "peru" => [205, 133, 63, 255],
        "wheat" => [245, 222, 179, 255],
        "ivory" => [255, 255, 240, 255],
        "lavender" => [230, 230, 250, 255],
        "mintcream" => [245, 255, 250, 255],
        "skyblue" => [135, 206, 235, 255],
        "seagreen" => [46, 139, 87, 255],
        "forestgreen" => [34, 139, 34, 255],
        "chartreuse" => [127, 255, 0, 255],
        "aquamarine" => [127, 255, 212, 255],
        _ => return None,
    };
    Some(c)
}

// ---------------------------------------------------------------------------
// YAML → Node
//
// Nodes are built straight from the YAML event stream through serde rather
// than via an intermediate `Value`, so out-of-range numbers (upstream writes
// `float.MaxValue` as a 39-digit integer) survive as plain strings, the way
// Robust's `ValueDataNode` keeps them.
// ---------------------------------------------------------------------------

struct NodeVisitor;

impl<'de> serde::de::Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any YAML node")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Node, E> {
        Ok(Node::Value(if v { "true" } else { "false" }.into()))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Node, E> {
        Ok(Node::Value(v.to_string()))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Node, E> {
        Ok(Node::Value(v.to_string()))
    }
    fn visit_i128<E>(self, v: i128) -> Result<Node, E> {
        Ok(Node::Value(v.to_string()))
    }
    fn visit_u128<E>(self, v: u128) -> Result<Node, E> {
        Ok(Node::Value(v.to_string()))
    }
    fn visit_f64<E>(self, v: f64) -> Result<Node, E> {
        Ok(Node::Value(if v.is_infinite() {
            if v > 0.0 { ".inf".into() } else { "-.inf".into() }
        } else if v.is_nan() {
            ".nan".into()
        } else {
            v.to_string()
        }))
    }
    fn visit_str<E>(self, v: &str) -> Result<Node, E> {
        Ok(Node::Value(v.to_owned()))
    }
    fn visit_string<E>(self, v: String) -> Result<Node, E> {
        Ok(Node::Value(v))
    }
    fn visit_unit<E>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }
    fn visit_none<E>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }
    fn visit_some<D: serde::Deserializer<'de>>(self, d: D) -> Result<Node, D::Error> {
        serde::Deserialize::deserialize(d)
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(n) = seq.next_element::<Node>()? {
            out.push(n);
        }
        Ok(Node::Seq(out))
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let mut out = Mapping::with_capacity(map.size_hint().unwrap_or(0));
        while let Some(k) = map.next_key::<Node>()? {
            let v = map.next_value::<Node>()?;
            out.insert(key_string(&k), v);
        }
        Ok(Node::Map(out))
    }
    fn visit_enum<A: serde::de::EnumAccess<'de>>(self, data: A) -> Result<Node, A::Error> {
        // yaml_serde surfaces `!tag value` as an enum whose variant is the tag.
        use serde::de::VariantAccess;
        let (tag, variant): (String, _) = data.variant()?;
        let inner: Node = variant.newtype_variant()?;
        let tag = tag.strip_prefix('!').unwrap_or(&tag).to_string();
        Ok(Node::Tagged(tag, Box::new(inner)))
    }
}

impl<'de> serde::Deserialize<'de> for Node {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Node, D::Error> {
        d.deserialize_any(NodeVisitor)
    }
}

fn key_string(k: &Node) -> String {
    match k.untagged() {
        Node::Null => "null".into(),
        Node::Value(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

/// Parse a YAML document stream into nodes (one per document).
pub fn parse_yaml(text: &str) -> Result<Vec<Node>, yaml_serde::Error> {
    use serde::Deserialize;
    let text = text.trim_start_matches('\u{feff}');
    let mut docs = Vec::new();
    for doc in yaml_serde::Deserializer::from_str(text) {
        docs.push(Node::deserialize(doc)?);
    }
    Ok(docs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tags_and_scalars() {
        let docs = parse_yaml(
            r#"
- type: entity
  id: Foo
  components:
  - type: Fixtures
    fixtures:
      fix1:
        shape:
          !type:PhysShapeCircle
          radius: 0.35
        hard: True
  - type: Transform
    pos: -0.5,1.5
    rot: 1.5707963267948966 rad
"#,
        )
        .unwrap();
        let root = &docs[0].as_seq().unwrap()[0];
        assert_eq!(root.get_str("id"), Some("Foo"));
        let comps = root.get("components").unwrap().as_seq().unwrap();
        let shape = comps[0].get("fixtures").unwrap().get("fix1").unwrap().get("shape").unwrap();
        assert_eq!(shape.type_tag(), Some("PhysShapeCircle"));
        assert_eq!(shape.get("radius").unwrap().as_f32(), Some(0.35));
        assert_eq!(comps[0].get("fixtures").unwrap().get("fix1").unwrap().get("hard").unwrap().as_bool(), Some(true));
        assert_eq!(comps[1].get("pos").unwrap().as_vec2(), Some((-0.5, 1.5)));
        assert!((comps[1].get("rot").unwrap().as_angle().unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    }

    #[test]
    fn colors() {
        assert_eq!(parse_color("#FF000080"), Some([255, 0, 0, 128]));
        assert_eq!(parse_color("#0f0"), Some([0, 255, 0, 255]));
        assert_eq!(parse_color("White"), Some([255, 255, 255, 255]));
    }
}
