//! Reject duplicate keys before a JSON map can silently overwrite an observation.

use anyhow::{Result, ensure};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Number, Value};
use std::fmt;

struct Unique(Value);

impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = Unique;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("JSON without duplicate object keys")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Unique, E> {
                Ok(Unique(Value::Bool(value)))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Unique, E> {
                Ok(Unique(Value::Number(value.into())))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Unique, E> {
                Ok(Unique(Value::Number(value.into())))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Unique, E> {
                Number::from_f64(value).map(|n| Unique(Value::Number(n))).ok_or_else(|| E::custom("non-finite number"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Unique, E> {
                Ok(Unique(Value::String(value.to_owned())))
            }
            fn visit_string<E: de::Error>(self, value: String) -> Result<Unique, E> {
                Ok(Unique(Value::String(value)))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Unique, A::Error> {
                let mut result = Vec::new();
                while let Some(Unique(value)) = seq.next_element()? { result.push(value); }
                Ok(Unique(Value::Array(result)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Unique, A::Error> {
                let mut result = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if result.contains_key(&key) { return Err(de::Error::custom("duplicate JSON key")); }
                    let Unique(value) = map.next_value()?;
                    result.insert(key, value);
                }
                Ok(Unique(Value::Object(result)))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

pub fn parse(bytes: &[u8], limit: usize) -> Result<Value> {
    ensure!(!bytes.is_empty() && bytes.len() <= limit, "JSON input size outside limit");
    let Unique(value) = serde_json::from_slice(bytes)?;
    Ok(value)
}
