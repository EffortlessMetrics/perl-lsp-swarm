//! Duplicate-key refusal before serde can normalize an object into a map.

use std::collections::BTreeSet;
use std::fmt::Formatter;

use serde::de::{DeserializeOwned, Error, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use super::CloseProofError;

/// Decode a public document only after its raw wire representation is
/// unambiguous. The first pass retains object keys, not values, and covers
/// nested objects and arrays, including keys hidden in unused payloads.
pub(super) fn from_json_str<T: DeserializeOwned>(
    json: &str,
    field: &str,
) -> Result<T, CloseProofError> {
    let decode = || -> Result<T, serde_json::Error> {
        let mut decoder = serde_json::Deserializer::from_str(json);
        UniqueKeys::deserialize(&mut decoder)?;
        decoder.end()?;
        serde_json::from_str(json)
    };
    decode().map_err(|error| CloseProofError::Schema {
        field: field.to_string(),
        message: error.to_string(),
    })
}

struct UniqueKeys;

impl<'de> Deserialize<'de> for UniqueKeys {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        decoder.deserialize_any(Self)
    }
}

impl<'de> Visitor<'de> for UniqueKeys {
    type Value = Self;

    fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_bool<E: Error>(self, _: bool) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_i64<E: Error>(self, _: i64) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_u64<E: Error>(self, _: u64) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_f64<E: Error>(self, _: f64) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_str<E: Error>(self, _: &str) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_unit<E: Error>(self) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self, A::Error> {
        while sequence.next_element::<Self>()?.is_some() {}
        Ok(self)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Self, A::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = object.next_key::<String>()? {
            // MapAccess returns decoded strings: escaped-equivalent spellings
            // collide, while the same key in separate objects remains valid.
            if !keys.insert(key.clone()) {
                return Err(A::Error::custom(format!("duplicate field `{key}`")));
            }
            object.next_value::<Self>()?;
        }
        Ok(self)
    }
}
