//! Canonical semantic encoding, fingerprint, and presentation projection
//! for `ci_route_plan.v1` (#10179).
//!
//! This module separates three projections the issue contract names
//! explicitly:
//!
//! ```text
//! semantic domain object          CiRoutePlanV1 (validated, typed)
//! canonical semantic projection   SemanticProjection (fingerprint-bearing fields only)
//! canonical encoded bytes         canonical_json (full payload, canonical spelling)
//! semantic fingerprint            SHA-256(domain || canonical semantic bytes)
//! presentation projection         explain (human text; never semantic)
//! ```
//!
//! ## Canonical JSON encoding (`ci_route_plan.v1`)
//!
//! The encoding is specified here normatively so an independent
//! implementation can reproduce the bytes without this crate:
//!
//! - UTF-8 byte encoding; no byte-order mark; no trailing newline.
//! - Object keys are emitted sorted by UTF-8 byte order (ascending).
//! - Arrays are emitted in the order of the projected value; every
//!   collection's order class is declared below and is either *ordered*
//!   (order is semantic) or *set-like* (normalized to ascending unique
//!   order; duplicates are rejected by the domain validator).
//! - Strings use minimal JSON escaping: `"` and `\` are escaped, control
//!   characters below 0x20 use `\b \f \n \r \t` or `\u00XX`; every other
//!   code unit (including all non-ASCII) is emitted as raw UTF-8.
//! - Integers are emitted as plain decimal (no exponent, sign, or leading
//!   zeros). The domain contains no floating-point values; a float in the
//!   projection fails closed.
//! - `None` optional values are omitted entirely (never `null`); an empty
//!   `Vec`/map is emitted as `[]`/`{}`.
//! - Enum spellings are the serde snake_case identities of the domain
//!   enums; compatibility aliases may exist only upstream of the typed
//!   domain, never in canonical output.
//! - Unknown fields are rejected (`deny_unknown_fields` on every domain
//!   type), so canonical output can never carry them.
//!
//! ## Collection order classes
//!
//! ```text
//! ordered   rows (governed denominator order), package_args (command
//!           token order — never sorted)
//! set-like  included_native_tiers (normalized ascending here; duplicates
//!           rejected by the validator), denominator, scope
//!           direct_crates / reverse_dependencies / architecture_wideners /
//!           risk_tags (validator-enforced ascending unique),
//!           summary.by_policy_role (BTreeMap, ascending by construction)
//! ```
//!
//! The derived `summary` is recomputed-and-checked by the domain validator
//! but does not participate in the fingerprint preimage; the
//! `semantic_fingerprint` field itself is never part of its own preimage.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{CiRoutePlanV1, RoutePlanRow, RouteSelectionEvidence, RouteSubjectRef};

/// Domain separation prefix for the fingerprint preimage. The exact bytes
/// are part of the versioned contract and of every golden vector:
/// `SHA-256("ci_route_plan.v1\0" || canonical_semantic_bytes)`.
pub const FINGERPRINT_DOMAIN: &[u8] = b"ci_route_plan.v1\0";

/// Deserialize an optional domain field while refusing explicit `null`.
///
/// The canonical contract spells absent optionals as omitted keys, never
/// `null` (and the checked-in JSON Schema types every optional as its
/// value type, rejecting `null`). Accepting a null spelling here would
/// let a second, non-canonical byte encoding validate under the same
/// semantic fingerprint, so the input adapter fails closed instead.
pub fn deserialize_option_reject_null<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Err(D::Error::custom(
            "explicit null is not a canonical optional spelling; omit the field instead",
        )),
        present => {
            T::deserialize(present).map(Some).map_err(|error| D::Error::custom(error.to_string()))
        }
    }
}

/// The fingerprint-bearing projection of one compiled route plan.
///
/// Fields excluded from the preimage, each with its classification:
///
/// - `summary`: derived from rows; validator-required and recomputed, but
///   presentation/derived movement must not move semantic identity;
/// - `semantic_fingerprint`: the digest itself; a digest can never be part
///   of its own preimage.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticProjection<'a> {
    pub schema: &'a str,
    pub producer: &'a str,
    pub subject: &'a RouteSubjectRef,
    pub requested_profile: &'a str,
    /// Set-like: normalized to ascending order here; duplicates are
    /// rejected by the domain validator before projection runs.
    pub included_native_tiers: Vec<&'a str>,
    pub expansion_fingerprint: &'a str,
    pub policy_digest: &'a str,
    pub disposition_digest: &'a str,
    pub workflow_digest: &'a str,
    /// Set-like, validator-enforced ascending unique.
    pub denominator: &'a [String],
    /// Selection facts that affect planned outcomes.
    pub selection: &'a RouteSelectionEvidence,
    /// Ordered (governed denominator order); every row field is
    /// load-bearing.
    pub rows: &'a [RoutePlanRow],
}

/// The complete published payload in canonical form: the semantic
/// projection (set-like fields normalized) plus the derived `summary` and
/// the `semantic_fingerprint` field itself. The projection is embedded
/// verbatim — not re-listed — so the fingerprint preimage and the
/// published payload share one semantic field list by construction and
/// cannot drift apart. Serializing this view — never the raw domain
/// object — is what makes published bytes canonical: two semantically
/// equal plans with different source orders produce identical artifacts.
#[derive(Debug, Clone, Serialize)]
pub struct CanonicalPayload<'a> {
    #[serde(flatten)]
    pub semantic: SemanticProjection<'a>,
    pub summary: &'a super::RoutePlanSummary,
    pub semantic_fingerprint: &'a str,
}

impl CiRoutePlanV1 {
    /// Project the fingerprint-bearing semantic fields. `summary` and
    /// `semantic_fingerprint` are excluded by construction.
    pub fn semantic_projection(&self) -> SemanticProjection<'_> {
        let included_native_tiers = self.canonical_tiers();
        SemanticProjection {
            schema: &self.schema,
            producer: &self.producer,
            subject: &self.subject,
            requested_profile: &self.requested_profile,
            included_native_tiers,
            expansion_fingerprint: &self.expansion_fingerprint,
            policy_digest: &self.policy_digest,
            disposition_digest: &self.disposition_digest,
            workflow_digest: &self.workflow_digest,
            denominator: &self.denominator,
            selection: &self.selection,
            rows: &self.rows,
        }
    }

    /// The complete payload in canonical form (set-like fields normalized,
    /// summary and fingerprint included). Built by embedding the semantic
    /// projection, so the published field list is the fingerprint preimage
    /// field list plus exactly the two derived fields.
    pub fn canonical_payload(&self) -> CanonicalPayload<'_> {
        CanonicalPayload {
            semantic: self.semantic_projection(),
            summary: &self.summary,
            semantic_fingerprint: &self.semantic_fingerprint,
        }
    }

    /// `included_native_tiers` is the one set-like field whose order the
    /// domain validator does not pin (duplicates are rejected; order is
    /// free), so both canonical projections normalize it here. Every other
    /// collection's canonical order is validator-enforced.
    fn canonical_tiers(&self) -> Vec<&str> {
        let mut tiers: Vec<&str> = self.included_native_tiers.iter().map(String::as_str).collect();
        tiers.sort_unstable();
        tiers
    }

    /// Canonical bytes of the semantic projection — the exact fingerprint
    /// preimage (before domain separation).
    pub fn canonical_semantic_bytes(&self) -> Result<Vec<u8>, String> {
        let value = serde_json::to_value(self.semantic_projection())
            .map_err(|error| format!("semantic projection failed: {error}"))?;
        canonical_json(&value)
    }

    /// Domain-separated SHA-256 fingerprint of the canonical semantic
    /// bytes: `SHA-256("ci_route_plan.v1\0" || bytes)`, lowercase hex.
    pub fn semantic_fingerprint_of(&self) -> Result<String, String> {
        let mut hasher = Sha256::new();
        hasher.update(FINGERPRINT_DOMAIN);
        hasher.update(self.canonical_semantic_bytes()?);
        Ok(hex(&hasher.finalize()))
    }

    /// Canonical encoded bytes of the complete published payload
    /// (including `summary` and `semantic_fingerprint`, with set-like
    /// fields normalized). Validates first: no invalid plan is ever
    /// encoded.
    pub fn canonical_json(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let value = serde_json::to_value(self.canonical_payload())
            .map_err(|error| format!("payload projection failed: {error}"))?;
        canonical_json(&value)
    }

    /// Presentation projection: bounded human explanation. Never part of
    /// the canonical bytes or the fingerprint preimage.
    pub fn explain(&self, gate_id: Option<&str>) -> Result<String, String> {
        self.validate()?;
        let Some(gate_id) = gate_id else {
            return serde_json::to_string_pretty(&self.summary).map_err(|error| error.to_string());
        };
        let row = self
            .rows
            .iter()
            .find(|row| row.gate_id == gate_id)
            .ok_or_else(|| format!("unknown gate {gate_id:?}"))?;
        serde_json::to_string_pretty(row).map_err(|error| error.to_string())
    }
}

/// Emit the canonical JSON encoding of a projected value.
///
/// Key order is recomputed here (sorted by UTF-8 byte order) regardless of
/// the input map's iteration order, so map insertion order can never move
/// the bytes. Numbers are restricted to unsigned 64-bit integers: the
/// domain has no other numeric shape, and anything else fails closed.
pub(crate) fn canonical_json(value: &Value) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    write_canonical(value, &mut out)?;
    Ok(out)
}

fn write_canonical(value: &Value, out: &mut Vec<u8>) -> Result<(), String> {
    match value {
        Value::Null => {
            // Optional domain fields are omitted, never null; a null in the
            // projection is a projection bug and fails closed.
            return Err("canonical projection produced a null value".to_string());
        }
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(number) => {
            let Some(integer) = number.as_u64() else {
                return Err(format!(
                    "canonical projection produced a non-unsigned-integer number {number}"
                ));
            };
            out.extend_from_slice(integer.to_string().as_bytes());
        }
        Value::String(text) => write_canonical_string(text, out),
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(item, out)?;
            }
            out.push(b']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push(b'{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical_string(key, out);
                out.push(b':');
                write_canonical(&map[*key], out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

fn write_canonical_string(text: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for unit in text.chars() {
        match unit {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{8}' => out.extend_from_slice(b"\\b"),
            '\u{c}' => out.extend_from_slice(b"\\f"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            control if (control as u32) < 0x20 => {
                let code = control as u32;
                out.extend_from_slice(format!("\\u{code:04x}").as_bytes());
            }
            other => {
                let mut buffer = [0u8; 4];
                out.extend_from_slice(other.encode_utf8(&mut buffer).as_bytes());
            }
        }
    }
    out.push(b'"');
}

/// Lowercase hex encoding with one allocation (per-byte `format!` is the
/// repository fallback for the missing `LowerHex` impl under the current
/// sha2/generic-array pair; the golden digests pin this table's exact
/// output).
fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod canonical_spec {
    use super::*;

    #[test]
    fn canonical_json_sorts_object_keys_by_byte_order() {
        let value: Value = serde_json::from_str(r#"{"b":1,"a":2,"A":3,"aa":4}"#).expect("fixture");
        let bytes = canonical_json(&value).expect("canonical");
        assert_eq!(String::from_utf8(bytes).expect("utf-8"), r#"{"A":3,"a":2,"aa":4,"b":1}"#);
    }

    #[test]
    fn canonical_json_escapes_minimally() {
        let value: Value =
            serde_json::from_str("\"quote\\\" back\\\\ newline\\n tab\\t unit\\u001f é北斗\"")
                .expect("fixture");
        let bytes = canonical_json(&value).expect("canonical");
        assert_eq!(
            String::from_utf8(bytes).expect("utf-8"),
            "\"quote\\\" back\\\\ newline\\n tab\\t unit\\u001f é北斗\""
        );
    }

    #[test]
    fn canonical_json_refuses_floats_and_nulls() {
        let float: Value = serde_json::from_str("1.5").expect("fixture");
        assert!(canonical_json(&float).is_err());
        let null: Value = Value::Null;
        assert!(canonical_json(&null).is_err());
    }

    #[test]
    fn canonical_json_emits_integers_without_exponent() {
        let value: Value = serde_json::from_str("0").expect("fixture");
        assert_eq!(canonical_json(&value).expect("canonical"), b"0");
        let value: Value = serde_json::from_str("9007199254740993").expect("fixture");
        assert_eq!(canonical_json(&value).expect("canonical"), b"9007199254740993");
    }
}
