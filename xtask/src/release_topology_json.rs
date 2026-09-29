//! Exact schema admission for the two bounded release-topology file consumers.

use color_eyre::eyre::{Result, bail};
use serde::Deserialize;
use serde_json::{Value, value::RawValue};

#[derive(Deserialize)]
struct SchemaEnvelope<'a> {
    #[serde(borrow)]
    schema: &'a RawValue,
}

/// Decode ordinary JSON only after admitting the original top-level schema token.
/// Serde owns object shape, escaped names, duplicate schema fields and JSON grammar.
pub(crate) fn load_topology_json(bytes: &[u8]) -> Result<Value> {
    let envelope: SchemaEnvelope<'_> = serde_json::from_slice(bytes)?;
    if !is_supported_schema(envelope.schema.get()) {
        bail!("release topology schema must be exactly 1 or 2");
    }
    Ok(serde_json::from_slice(bytes)?)
}

/// A valid JSON number equals 1 or 2 only when its nonzero significant
/// coefficient is that single digit and its decimal exponent cancels exactly.
fn is_supported_schema(token: &str) -> bool {
    let (coefficient, exponent) = match token.split_once(['e', 'E']) {
        Some(parts) => parts,
        None => (token, "0"),
    };
    let Ok(exponent) = exponent.parse::<i64>() else { return false };
    let fractional_places = coefficient.split_once('.').map_or(0, |(_, tail)| tail.len());
    let digits: String = coefficient.chars().filter(|character| *character != '.').collect();
    let significant = digits.trim_start_matches('0').trim_end_matches('0');
    if significant != "1" && significant != "2" {
        return false;
    }
    let trailing_zeroes = digits.len() - digits.trim_end_matches('0').len();
    let (Ok(fractional_places), Ok(trailing_zeroes)) =
        (i64::try_from(fractional_places), i64::try_from(trailing_zeroes))
    else {
        return false;
    };
    exponent.checked_sub(fractional_places).and_then(|value| value.checked_add(trailing_zeroes))
        == Some(0)
}
