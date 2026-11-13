//! Reject duplicate object keys before serde maps can overwrite evidence.

use anyhow::{Result, ensure};
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value};
use std::fmt;

struct Unique(Value);

impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Walk;
        impl<'de> Visitor<'de> for Walk {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON with unique keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_f64<E: de::Error>(self, _v: f64) -> Result<Unique, E> {
                Err(E::custom("floating-point fields are not admitted by this identity profile"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Unique, A::Error> {
                let mut out = Vec::new();
                while let Some(Unique(value)) = sequence.next_element()? {
                    out.push(value);
                }
                Ok(Unique(Value::Array(out)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Unique, A::Error> {
                let mut out = Map::new();
                while let Some(key) = object.next_key::<String>()? {
                    if out.contains_key(&key) {
                        return Err(de::Error::custom("duplicate JSON key"));
                    }
                    let Unique(value) = object.next_value()?;
                    out.insert(key, value);
                }
                Ok(Unique(Value::Object(out)))
            }
        }
        d.deserialize_any(Walk)
    }
}

pub fn parse(bytes: &[u8]) -> Result<Value> {
    ensure!(bytes.len() <= super::MAX_DOCUMENT, "JSON document exceeds ceiling");
    // serde_json's recursion limit remains enabled.
    Ok(serde_json::from_slice::<Unique>(bytes)?.0)
}
