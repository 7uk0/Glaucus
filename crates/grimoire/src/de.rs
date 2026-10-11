//! `serde` support: deserialize any `#[derive(Deserialize)]` type straight
//! from a [`Node`]. This is the Rust stand-in for Robust's
//! `ISerializationManager.Read<T>()` and is how ported components get their
//! YAML data:
//!
//! ```ignore
//! #[derive(Deserialize, Default)]
//! #[serde(default, rename_all = "camelCase")]
//! struct MovementSpeedModifier { base_walk_speed: f32, base_sprint_speed: f32 }
//! let m: MovementSpeedModifier = grimoire::from_node(&comp_node)?;
//! ```
//!
//! Conventions mirrored from Robust:
//! * scalars are strings and are parsed on demand (`"0.35"` → `f32`,
//!   `"True"` → `bool`);
//! * `"x,y"` scalars deserialize into 2-tuples / `[f32; 2]`, which is how
//!   `Vector2`, `Vector2i` and `Box2` are written;
//! * `!type:Foo` tagged nodes deserialize into enum variant `Foo` — the
//!   polymorphic `[ImplicitDataDefinitionForInheritors]` pattern;
//! * plain scalars deserialize into unit enum variants by name, and the
//!   `enum.SomeEnum.Value` spelling is accepted too.

use std::fmt;

use serde::de::{self, DeserializeSeed, Deserializer, IntoDeserializer, MapAccess, SeqAccess, Visitor};

use crate::node::{parse_bool, parse_f64, Node};

#[derive(Debug, Clone, PartialEq)]
pub struct DeError(pub String);

impl fmt::Display for DeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DeError {}

impl de::Error for DeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        DeError(msg.to_string())
    }
}

/// Deserialize a value of type `T` from a node.
pub fn from_node<'de, T: serde::Deserialize<'de>>(node: &'de Node) -> Result<T, DeError> {
    T::deserialize(node)
}

fn scalar<'a>(node: &'a Node, what: &str) -> Result<&'a str, DeError> {
    match node.untagged() {
        Node::Value(s) => Ok(s),
        other => Err(DeError(format!("expected {what} scalar, found {other:?}"))),
    }
}

macro_rules! de_int {
    ($fn:ident, $visit:ident, $ty:ty) => {
        fn $fn<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
            let s = scalar(self, stringify!($ty))?.trim();
            let parsed: $ty = match s.parse::<$ty>() {
                Ok(x) => x,
                Err(_) => {
                    // Accept "3.0" for integers, as C# does via double parse in some serializers.
                    let f = parse_f64(s).ok_or_else(|| DeError(format!("bad {}: {s:?}", stringify!($ty))))?;
                    if f.fract() != 0.0 {
                        return Err(DeError(format!("bad {}: {s:?}", stringify!($ty))));
                    }
                    f as $ty
                }
            };
            v.$visit(parsed)
        }
    };
}

impl<'de> Deserializer<'de> for &'de Node {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self {
            Node::Null => v.visit_unit(),
            Node::Value(s) => v.visit_borrowed_str(s),
            Node::Seq(s) => v.visit_seq(SeqDe { iter: s.iter() }),
            Node::Map(m) => v.visit_map(MapDe {
                iter: m.iter(),
                value: None,
            }),
            Node::Tagged(_, inner) => inner.as_ref().deserialize_any(v),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        let s = scalar(self, "bool")?;
        v.visit_bool(parse_bool(s).ok_or_else(|| DeError(format!("bad bool: {s:?}")))?)
    }

    de_int!(deserialize_i8, visit_i8, i8);
    de_int!(deserialize_i16, visit_i16, i16);
    de_int!(deserialize_i32, visit_i32, i32);
    de_int!(deserialize_i64, visit_i64, i64);
    de_int!(deserialize_u8, visit_u8, u8);
    de_int!(deserialize_u16, visit_u16, u16);
    de_int!(deserialize_u32, visit_u32, u32);
    de_int!(deserialize_u64, visit_u64, u64);

    fn deserialize_f32<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        let s = scalar(self, "f32")?;
        v.visit_f32(parse_f64(s).ok_or_else(|| DeError(format!("bad float: {s:?}")))? as f32)
    }

    fn deserialize_f64<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        let s = scalar(self, "f64")?;
        v.visit_f64(parse_f64(s).ok_or_else(|| DeError(format!("bad float: {s:?}")))?)
    }

    fn deserialize_char<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        let s = scalar(self, "char")?;
        let mut it = s.chars();
        match (it.next(), it.next()) {
            (Some(c), None) => v.visit_char(c),
            _ => Err(DeError(format!("bad char: {s:?}"))),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.untagged() {
            Node::Null => v.visit_borrowed_str(""),
            Node::Value(s) => v.visit_borrowed_str(s),
            other => Err(DeError(format!("expected string, found {other:?}"))),
        }
    }

    fn deserialize_string<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_str(v)
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_str(v)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_str(v)
    }

    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.untagged() {
            Node::Null => v.visit_none(),
            Node::Value(s) if s == "null" => v.visit_none(),
            _ => v.visit_some(self),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        v.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(self, _name: &'static str, v: V) -> Result<V::Value, DeError> {
        v.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _name: &'static str, v: V) -> Result<V::Value, DeError> {
        v.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.untagged() {
            Node::Seq(s) => v.visit_seq(SeqDe { iter: s.iter() }),
            Node::Null => v.visit_seq(SeqDe { iter: [].iter() }),
            other => Err(DeError(format!("expected sequence, found {other:?}"))),
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, v: V) -> Result<V::Value, DeError> {
        match self.untagged() {
            // "x,y" / "l,b,r,t" packed scalars.
            Node::Value(s) => {
                let parts: Vec<&str> = s.split(',').map(str::trim).collect();
                if parts.len() != len {
                    return Err(DeError(format!("expected {len} comma-separated values, got {s:?}")));
                }
                v.visit_seq(StrSeqDe { iter: parts.into_iter() })
            }
            _ => self.deserialize_seq(v),
        }
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(self, _name: &'static str, len: usize, v: V) -> Result<V::Value, DeError> {
        self.deserialize_tuple(len, v)
    }

    fn deserialize_map<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        match self.untagged() {
            Node::Map(m) => v.visit_map(MapDe {
                iter: m.iter(),
                value: None,
            }),
            Node::Null => v.visit_map(MapDe {
                iter: indexmap::map::Iter::default(),
                value: None,
            }),
            other => Err(DeError(format!("expected mapping, found {other:?}"))),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, DeError> {
        self.deserialize_map(v)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, DeError> {
        match self {
            Node::Tagged(tag, inner) => {
                let variant = tag.strip_prefix("type:").unwrap_or(tag);
                v.visit_enum(EnumDe {
                    variant,
                    content: Some(inner),
                })
            }
            Node::Value(s) => {
                // `enum.DoorVisuals.State` → `State`
                let variant = if s.starts_with("enum.") {
                    s.rsplit('.').next().unwrap_or(s)
                } else {
                    s.as_str()
                };
                v.visit_enum(EnumDe { variant, content: None })
            }
            Node::Map(m) if m.len() == 1 => {
                let (k, val) = m.iter().next().unwrap();
                v.visit_enum(EnumDe {
                    variant: k,
                    content: Some(val),
                })
            }
            other => Err(DeError(format!("expected enum, found {other:?}"))),
        }
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        self.deserialize_str(v)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        v.visit_unit()
    }
}

struct SeqDe<'de> {
    iter: std::slice::Iter<'de, Node>,
}

impl<'de> SeqAccess<'de> for SeqDe<'de> {
    type Error = DeError;
    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, DeError> {
        match self.iter.next() {
            Some(n) => seed.deserialize(n).map(Some),
            None => Ok(None),
        }
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

struct StrSeqDe<'a> {
    iter: std::vec::IntoIter<&'a str>,
}

impl<'de> SeqAccess<'de> for StrSeqDe<'de> {
    type Error = DeError;
    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, DeError> {
        match self.iter.next() {
            Some(s) => seed.deserialize(ScalarDe(s)).map(Some),
            None => Ok(None),
        }
    }
}

struct MapDe<'de> {
    iter: indexmap::map::Iter<'de, String, Node>,
    value: Option<&'de Node>,
}

impl<'de> MapAccess<'de> for MapDe<'de> {
    type Error = DeError;
    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, DeError> {
        match self.iter.next() {
            Some((k, v)) => {
                self.value = Some(v);
                seed.deserialize(ScalarDe(k)).map(Some)
            }
            None => Ok(None),
        }
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, DeError> {
        let v = self.value.take().ok_or_else(|| DeError("value without key".into()))?;
        seed.deserialize(v).map_err(|e| e)
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

struct EnumDe<'de> {
    variant: &'de str,
    content: Option<&'de Node>,
}

impl<'de> de::EnumAccess<'de> for EnumDe<'de> {
    type Error = DeError;
    type Variant = VariantDe<'de>;
    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, VariantDe<'de>), DeError> {
        let val = seed.deserialize(ScalarDe(self.variant))?;
        Ok((val, VariantDe { content: self.content }))
    }
}

struct VariantDe<'de> {
    content: Option<&'de Node>,
}

static NULL: Node = Node::Null;

impl<'de> de::VariantAccess<'de> for VariantDe<'de> {
    type Error = DeError;
    fn unit_variant(self) -> Result<(), DeError> {
        Ok(())
    }
    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, DeError> {
        seed.deserialize(self.content.unwrap_or(&NULL))
    }
    fn tuple_variant<V: Visitor<'de>>(self, len: usize, v: V) -> Result<V::Value, DeError> {
        self.content.unwrap_or(&NULL).deserialize_tuple(len, v)
    }
    fn struct_variant<V: Visitor<'de>>(self, _fields: &'static [&'static str], v: V) -> Result<V::Value, DeError> {
        self.content.unwrap_or(&NULL).deserialize_map(v)
    }
}

/// Deserializer over a bare string (map keys, tuple parts, variant names).
struct ScalarDe<'a>(&'a str);

impl<'de> IntoDeserializer<'de, DeError> for ScalarDe<'de> {
    type Deserializer = Self;
    fn into_deserializer(self) -> Self {
        self
    }
}

macro_rules! scalar_int {
    ($fn:ident, $visit:ident, $ty:ty) => {
        fn $fn<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
            let s = self.0.trim();
            v.$visit(s.parse::<$ty>().map_err(|_| DeError(format!("bad {}: {s:?}", stringify!($ty))))?)
        }
    };
}

impl<'de> Deserializer<'de> for ScalarDe<'de> {
    type Error = DeError;
    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        v.visit_borrowed_str(self.0)
    }
    fn deserialize_bool<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        v.visit_bool(parse_bool(self.0).ok_or_else(|| DeError(format!("bad bool: {:?}", self.0)))?)
    }
    scalar_int!(deserialize_i8, visit_i8, i8);
    scalar_int!(deserialize_i16, visit_i16, i16);
    scalar_int!(deserialize_i32, visit_i32, i32);
    scalar_int!(deserialize_i64, visit_i64, i64);
    scalar_int!(deserialize_u8, visit_u8, u8);
    scalar_int!(deserialize_u16, visit_u16, u16);
    scalar_int!(deserialize_u32, visit_u32, u32);
    scalar_int!(deserialize_u64, visit_u64, u64);
    fn deserialize_f32<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        v.visit_f32(parse_f64(self.0).ok_or_else(|| DeError(format!("bad float: {:?}", self.0)))? as f32)
    }
    fn deserialize_f64<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        v.visit_f64(parse_f64(self.0).ok_or_else(|| DeError(format!("bad float: {:?}", self.0)))?)
    }
    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value, DeError> {
        v.visit_some(self)
    }
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, DeError> {
        let s = self.0;
        let variant = if s.starts_with("enum.") { s.rsplit('.').next().unwrap_or(s) } else { s };
        v.visit_enum(EnumDe { variant, content: None })
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _name: &'static str, v: V) -> Result<V::Value, DeError> {
        v.visit_newtype_struct(self)
    }
    serde::forward_to_deserialize_any! {
        char str string bytes byte_buf unit unit_struct seq tuple
        tuple_struct map struct identifier ignored_any
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::parse_yaml;
    use serde::Deserialize;
    use std::collections::HashMap;

    #[derive(Debug, Deserialize, PartialEq)]
    enum PhysShape {
        PhysShapeCircle {
            radius: f32,
        },
        PhysShapeAabb {
            #[serde(default)]
            bounds: Option<(f32, f32, f32, f32)>,
        },
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct Fixture {
        shape: PhysShape,
        #[serde(default)]
        density: f32,
        #[serde(default)]
        hard: Option<bool>,
        #[serde(default)]
        mask: Vec<String>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    enum BodyType {
        Static,
        Dynamic,
        KinematicController,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct Comp {
        fixtures: HashMap<String, Fixture>,
        body_type: BodyType,
        offset: (f32, f32),
        layer: Option<String>,
        visual: BodyType,
    }

    #[test]
    fn derives_from_nodes() {
        let doc = parse_yaml(
            r#"
fixtures:
  fix1:
    shape: !type:PhysShapeCircle
      radius: 0.35
    density: 10
    hard: False
    mask: [ MobMask ]
  fix2:
    shape:
      !type:PhysShapeAabb
      bounds: "-0.5,-0.5,0.5,0.5"
bodyType: KinematicController
offset: 0.5,-1
visual: enum.Foo.Dynamic
"#,
        )
        .unwrap();
        let c: Comp = from_node(&doc[0]).unwrap();
        assert_eq!(c.fixtures["fix1"].shape, PhysShape::PhysShapeCircle { radius: 0.35 });
        assert_eq!(c.fixtures["fix1"].density, 10.0);
        assert_eq!(c.fixtures["fix1"].hard, Some(false));
        assert_eq!(c.fixtures["fix1"].mask, vec!["MobMask".to_string()]);
        assert_eq!(
            c.fixtures["fix2"].shape,
            PhysShape::PhysShapeAabb {
                bounds: Some((-0.5, -0.5, 0.5, 0.5))
            }
        );
        assert_eq!(c.body_type, BodyType::KinematicController);
        assert_eq!(c.offset, (0.5, -1.0));
        assert_eq!(c.layer, None);
        assert_eq!(c.visual, BodyType::Dynamic);
    }
}
