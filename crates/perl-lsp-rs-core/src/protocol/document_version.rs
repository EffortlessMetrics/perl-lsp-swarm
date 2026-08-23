//! Typed document version decoder for LSP `textDocument/version` fields.
//!
//! The LSP 3.18 specification requires `textDocument.version` to be an
//! `integer` (i32). Clients may omit the field, supply `null`, use a wrong
//! JSON type, or pass values outside the i32 domain. A strict decoder must
//! represent every case distinctly so that upstream lifecycle decisions cannot
//! accidentally route explicit decode failures through the versionless path.
//!
//! ## Type relationships
//!
//! ```text
//! decode_document_version(params)
//!   Ok(DocumentVersionField::Absent)               ← field not present
//!   Ok(DocumentVersionField::Explicit(v))          ← valid i32
//!   Err(DocumentVersionDecodeError::Null)           ← explicit null
//!   Err(DocumentVersionDecodeError::NonInteger(k))  ← wrong JSON type / float
//!   Err(DocumentVersionDecodeError::OutOfRange{…})  ← integer out of i32 range
//! ```
//!
//! ## Non-goals
//!
//! - No production `didChange` behavior change (see issue #8293).
//! - No stale/equal version policy (see issue #7075).
//! - No missing-version compatibility decision.
//! - No parser-generation or document-state mutation.
//!
//! Related issues: #10240 (version decoder), #8293 (desynchronization
//! consumer), #7075 (lifecycle controller).

use serde_json::Value;

/// The JSON value kind that was observed at the version field.
///
/// Bounded: carries no raw text, only the kind label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum JsonValueKind {
    /// JSON `null` — carried by [`DocumentVersionDecodeError::Null`] directly,
    /// not through this variant (reserved for future combinators).
    Null,
    /// JSON `true` or `false`.
    Bool,
    /// A JSON number whose representation is non-integral or float.
    Float,
    /// A JSON string.
    String,
    /// A JSON array.
    Array,
    /// A JSON object.
    Object,
}

impl std::fmt::Display for JsonValueKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Bool => f.write_str("boolean"),
            Self::Float => f.write_str("float/non-integral-number"),
            Self::String => f.write_str("string"),
            Self::Array => f.write_str("array"),
            Self::Object => f.write_str("object"),
        }
    }
}

/// Sign of an integer value that is outside the i32 protocol domain.
///
/// Used inside [`IntegerRangeClass`] to classify whether the value was
/// positive or negative, without retaining the raw integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signedness {
    /// The integer was zero or positive.
    Positive,
    /// The integer was negative.
    Negative,
}

impl std::fmt::Display for Signedness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Positive => f.write_str("positive"),
            Self::Negative => f.write_str("negative"),
        }
    }
}

/// Bounded class for integers that were in i64 or u64 range but outside
/// the LSP `integer` (i32) domain.
///
/// Retains only the class, not the raw value, so diagnostic output is bounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegerRangeClass {
    /// Negative value below `i32::MIN` (in i64 range).
    BelowI32Min,
    /// Positive value above `i32::MAX` but within i64 range.
    AboveI32Max,
    /// Positive u64 value above `i64::MAX`.
    AboveI64Max,
}

impl std::fmt::Display for IntegerRangeClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BelowI32Min => write!(f, "below i32::MIN ({})", i32::MIN),
            Self::AboveI32Max => write!(f, "above i32::MAX ({})", i32::MAX),
            Self::AboveI64Max => write!(f, "above i64::MAX ({})", i64::MAX),
        }
    }
}

/// Error returned when a document version field is present but invalid.
///
/// Every variant carries only bounded classification metadata — no raw JSON
/// source text or unbounded integer values.
///
/// A caller cannot pattern-match this as [`DocumentVersionField::Absent`]
/// without writing an explicit compatibility adapter (see §8293).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DocumentVersionDecodeError {
    /// The version field was present and explicitly `null`.
    Null,
    /// The version field was present but held a non-integer JSON type.
    NonInteger(JsonValueKind),
    /// The version field held a JSON integer outside the LSP i32 domain.
    OutOfRange {
        /// Sign of the out-of-range value.
        sign: Signedness,
        /// Bounded class of the range violation.
        bounded_class: IntegerRangeClass,
    },
}

impl std::fmt::Display for DocumentVersionDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => f.write_str(
                "textDocument/version was explicitly null; \
                 use DocumentVersionField::Absent for absent fields",
            ),
            Self::NonInteger(kind) => {
                write!(f, "textDocument/version has non-integer JSON type: {kind}")
            }
            Self::OutOfRange { sign, bounded_class } => write!(
                f,
                "textDocument/version integer is {sign} and out of i32 range: {bounded_class}",
            ),
        }
    }
}

impl std::error::Error for DocumentVersionDecodeError {}

/// A validated LSP client document version (`integer`, i.e. i32).
///
/// Constructed only by [`decode_document_version`] or
/// [`decode_version_value`]; no public constructor bypasses range validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientDocumentVersion(i32);

impl ClientDocumentVersion {
    /// Return the raw i32 protocol value.
    ///
    /// Prefer consuming the typed value; call this only at the boundary where
    /// a raw integer is needed by the LSP dispatch layer.
    #[must_use]
    pub fn as_i32(self) -> i32 {
        self.0
    }
}

impl std::fmt::Display for ClientDocumentVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Whether the `textDocument/version` field was present in the notification
/// envelope and, if so, the validated value.
///
/// Distinct from `Option<ClientDocumentVersion>` to prevent a caller from
/// pattern-matching a decode error as absence without an explicit adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DocumentVersionField {
    /// The `version` field was not present in the `textDocument` object.
    Absent,
    /// The field was present and held a valid i32 protocol version.
    Explicit(ClientDocumentVersion),
}

// ─────────────────────────────────────────────────────────────────────────────
// Decoder implementation
// ─────────────────────────────────────────────────────────────────────────────

/// Decode a single JSON value as an LSP document version integer.
///
/// Does not inspect field presence — call this only when the caller has
/// already established that the field exists. Use [`decode_document_version`]
/// for the full envelope including absence handling.
///
/// Accepts only exact JSON integers representable within the i32 domain.
/// Positive u64 values above `i64::MAX` are rejected with a typed
/// [`IntegerRangeClass::AboveI64Max`] error rather than silently truncating.
pub fn decode_version_value(
    value: &Value,
) -> Result<ClientDocumentVersion, DocumentVersionDecodeError> {
    match value {
        Value::Null => Err(DocumentVersionDecodeError::Null),
        Value::Bool(_) => Err(DocumentVersionDecodeError::NonInteger(JsonValueKind::Bool)),
        Value::String(_) => Err(DocumentVersionDecodeError::NonInteger(JsonValueKind::String)),
        Value::Array(_) => Err(DocumentVersionDecodeError::NonInteger(JsonValueKind::Array)),
        Value::Object(_) => Err(DocumentVersionDecodeError::NonInteger(JsonValueKind::Object)),
        Value::Number(n) => {
            // Attempt i64 first: covers negative values and positive values
            // up to i64::MAX.
            if let Some(i) = n.as_i64() {
                i32::try_from(i).map(ClientDocumentVersion).map_err(|_| {
                    if i < 0 {
                        DocumentVersionDecodeError::OutOfRange {
                            sign: Signedness::Negative,
                            bounded_class: IntegerRangeClass::BelowI32Min,
                        }
                    } else {
                        DocumentVersionDecodeError::OutOfRange {
                            sign: Signedness::Positive,
                            bounded_class: IntegerRangeClass::AboveI32Max,
                        }
                    }
                })
            } else if n.as_u64().is_some() {
                // u64 value present but as_i64() returned None → above i64::MAX.
                Err(DocumentVersionDecodeError::OutOfRange {
                    sign: Signedness::Positive,
                    bounded_class: IntegerRangeClass::AboveI64Max,
                })
            } else {
                // Float or any non-integral JSON number representation.
                Err(DocumentVersionDecodeError::NonInteger(JsonValueKind::Float))
            }
        }
    }
}

/// Decode the `textDocument/version` field from an LSP notification params
/// envelope.
///
/// Navigates `params["textDocument"]["version"]` and returns:
///
/// - `Ok(DocumentVersionField::Absent)` when the `textDocument` object is
///   missing entirely or when the `version` key is absent from it.
/// - `Ok(DocumentVersionField::Explicit(v))` when a valid i32 version is
///   found.
/// - `Err(e)` when the version key is present but holds an invalid value.
///
/// # Policy neutrality
///
/// This function performs no monotonicity, staleness, or equal-version
/// policy check. Such decisions belong to the lifecycle controller (#7075)
/// and desynchronization consumer (#8293), not to the decoder.
///
/// # Absence vs. error
///
/// An explicit decode error cannot be pattern-matched as
/// [`DocumentVersionField::Absent`] without writing an explicit named
/// compatibility adapter. This is intentional: callers must acknowledge the
/// distinction at the policy join point.
pub fn decode_document_version(
    params: &Value,
) -> Result<DocumentVersionField, DocumentVersionDecodeError> {
    let text_document = match params.get("textDocument") {
        Some(doc) => doc,
        None => return Ok(DocumentVersionField::Absent),
    };

    match text_document.get("version") {
        None => Ok(DocumentVersionField::Absent),
        Some(v) => decode_version_value(v).map(DocumentVersionField::Explicit),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── absence ──────────────────────────────────────────────────────────────

    #[test]
    fn absent_no_textdocument_key() {
        let params = json!({});
        assert_eq!(decode_document_version(&params), Ok(DocumentVersionField::Absent));
    }

    #[test]
    fn absent_textdocument_has_no_version_key() {
        let params = json!({"textDocument": {"uri": "file:///foo.pl"}});
        assert_eq!(decode_document_version(&params), Ok(DocumentVersionField::Absent));
    }

    #[test]
    fn absent_empty_textdocument_object() {
        let params = json!({"textDocument": {}});
        assert_eq!(decode_document_version(&params), Ok(DocumentVersionField::Absent));
    }

    // ── valid i32 values ─────────────────────────────────────────────────────

    #[test]
    fn valid_i32_min() {
        let params = json!({"textDocument": {"version": i32::MIN}});
        assert_eq!(
            decode_document_version(&params),
            Ok(DocumentVersionField::Explicit(ClientDocumentVersion(i32::MIN)))
        );
    }

    #[test]
    fn valid_negative_ordinary() {
        let params = json!({"textDocument": {"version": -1}});
        assert_eq!(
            decode_document_version(&params),
            Ok(DocumentVersionField::Explicit(ClientDocumentVersion(-1)))
        );
    }

    #[test]
    fn valid_zero() {
        let params = json!({"textDocument": {"version": 0}});
        assert_eq!(
            decode_document_version(&params),
            Ok(DocumentVersionField::Explicit(ClientDocumentVersion(0)))
        );
    }

    #[test]
    fn valid_positive_ordinary() {
        let params = json!({"textDocument": {"version": 42}});
        assert_eq!(
            decode_document_version(&params),
            Ok(DocumentVersionField::Explicit(ClientDocumentVersion(42)))
        );
    }

    #[test]
    fn valid_i32_max() {
        let params = json!({"textDocument": {"version": i32::MAX}});
        assert_eq!(
            decode_document_version(&params),
            Ok(DocumentVersionField::Explicit(ClientDocumentVersion(i32::MAX)))
        );
    }

    // ── explicit null ─────────────────────────────────────────────────────────

    #[test]
    fn explicit_null_is_error_not_absent() {
        let params = json!({"textDocument": {"version": null}});
        assert_eq!(decode_document_version(&params), Err(DocumentVersionDecodeError::Null));
    }

    // ── non-integer JSON types ────────────────────────────────────────────────

    #[test]
    fn version_is_string() {
        let params = json!({"textDocument": {"version": "1"}});
        assert_eq!(
            decode_document_version(&params),
            Err(DocumentVersionDecodeError::NonInteger(JsonValueKind::String))
        );
    }

    #[test]
    fn version_is_boolean_true() {
        let params = json!({"textDocument": {"version": true}});
        assert_eq!(
            decode_document_version(&params),
            Err(DocumentVersionDecodeError::NonInteger(JsonValueKind::Bool))
        );
    }

    #[test]
    fn version_is_boolean_false() {
        let params = json!({"textDocument": {"version": false}});
        assert_eq!(
            decode_document_version(&params),
            Err(DocumentVersionDecodeError::NonInteger(JsonValueKind::Bool))
        );
    }

    #[test]
    fn version_is_fractional_float() {
        let params = json!({"textDocument": {"version": 1.5}});
        assert_eq!(
            decode_document_version(&params),
            Err(DocumentVersionDecodeError::NonInteger(JsonValueKind::Float))
        );
    }

    #[test]
    fn version_is_integral_looking_float() {
        // 1.0 serialized as a float — serde_json stores this as N::Float.
        let value = serde_json::Number::from_f64(1.0).expect("1.0 is finite");
        let params = json!({"textDocument": {"version": value}});
        assert_eq!(
            decode_document_version(&params),
            Err(DocumentVersionDecodeError::NonInteger(JsonValueKind::Float))
        );
    }

    #[test]
    fn version_is_array() {
        let params = json!({"textDocument": {"version": [1, 2]}});
        assert_eq!(
            decode_document_version(&params),
            Err(DocumentVersionDecodeError::NonInteger(JsonValueKind::Array))
        );
    }

    #[test]
    fn version_is_object() {
        let params = json!({"textDocument": {"version": {"major": 1}}});
        assert_eq!(
            decode_document_version(&params),
            Err(DocumentVersionDecodeError::NonInteger(JsonValueKind::Object))
        );
    }

    // ── out-of-range integers ─────────────────────────────────────────────────

    #[test]
    fn below_i32_min_by_one() {
        let v = i64::from(i32::MIN) - 1;
        let params = json!({"textDocument": {"version": v}});
        assert_eq!(
            decode_document_version(&params),
            Err(DocumentVersionDecodeError::OutOfRange {
                sign: Signedness::Negative,
                bounded_class: IntegerRangeClass::BelowI32Min
            })
        );
    }

    #[test]
    fn above_i32_max_by_one() {
        let v = i64::from(i32::MAX) + 1;
        let params = json!({"textDocument": {"version": v}});
        assert_eq!(
            decode_document_version(&params),
            Err(DocumentVersionDecodeError::OutOfRange {
                sign: Signedness::Positive,
                bounded_class: IntegerRangeClass::AboveI32Max
            })
        );
    }

    #[test]
    fn i64_min() {
        let params = json!({"textDocument": {"version": i64::MIN}});
        assert_eq!(
            decode_document_version(&params),
            Err(DocumentVersionDecodeError::OutOfRange {
                sign: Signedness::Negative,
                bounded_class: IntegerRangeClass::BelowI32Min
            })
        );
    }

    #[test]
    fn i64_max() {
        let params = json!({"textDocument": {"version": i64::MAX}});
        assert_eq!(
            decode_document_version(&params),
            Err(DocumentVersionDecodeError::OutOfRange {
                sign: Signedness::Positive,
                bounded_class: IntegerRangeClass::AboveI32Max
            })
        );
    }

    #[test]
    fn u64_above_i64_max() {
        // Construct a u64 value above i64::MAX directly in the JSON value.
        // serde_json stores this as N::PosInt(u64) with value > i64::MAX.
        let big: u64 = u64::from(i64::MAX as u64) + 1; // i64::MAX + 1
        let value = serde_json::Value::Number(serde_json::Number::from(big));
        let params = json!({"textDocument": {"version": value}});
        assert_eq!(
            decode_document_version(&params),
            Err(DocumentVersionDecodeError::OutOfRange {
                sign: Signedness::Positive,
                bounded_class: IntegerRangeClass::AboveI64Max
            })
        );
    }

    #[test]
    fn u64_max() {
        let value = serde_json::Value::Number(serde_json::Number::from(u64::MAX));
        let params = json!({"textDocument": {"version": value}});
        assert_eq!(
            decode_document_version(&params),
            Err(DocumentVersionDecodeError::OutOfRange {
                sign: Signedness::Positive,
                bounded_class: IntegerRangeClass::AboveI64Max
            })
        );
    }

    // ── shuffled field order does not change classification ───────────────────

    #[test]
    fn shuffled_object_field_order_valid_version() {
        // Extra fields before and after version; order must not affect result.
        let params = json!({
            "textDocument": {
                "languageId": "perl",
                "version": 7,
                "uri": "file:///foo.pl",
                "text": "use strict;\n"
            }
        });
        assert_eq!(
            decode_document_version(&params),
            Ok(DocumentVersionField::Explicit(ClientDocumentVersion(7)))
        );
    }

    #[test]
    fn shuffled_object_field_order_null_version() {
        let params = json!({
            "textDocument": {
                "uri": "file:///foo.pl",
                "version": null,
                "languageId": "perl"
            }
        });
        assert_eq!(decode_document_version(&params), Err(DocumentVersionDecodeError::Null));
    }

    // ── bounded evidence — Debug/Display carry no raw source ─────────────────

    #[test]
    fn decode_error_display_contains_no_raw_json_source() {
        let error = DocumentVersionDecodeError::NonInteger(JsonValueKind::String);
        let rendered = format!("{error}");
        // Must not contain JSON delimiters or embedded content.
        assert!(!rendered.contains('{'));
        assert!(!rendered.contains('}'));
        assert!(!rendered.contains('"'));
        // Must contain the classification keyword.
        assert!(rendered.contains("string"));
    }

    #[test]
    fn decode_error_debug_is_bounded() {
        let error = DocumentVersionDecodeError::OutOfRange {
            sign: Signedness::Positive,
            bounded_class: IntegerRangeClass::AboveI64Max,
        };
        let rendered = format!("{error:?}");
        // Debug output carries only type names and enum variant names; no raw ints.
        assert!(rendered.contains("OutOfRange"));
        assert!(rendered.contains("Positive"));
        assert!(rendered.contains("AboveI64Max"));
        // Must not contain a raw large integer.
        assert!(!rendered.contains("9223372036854775808")); // i64::MAX + 1
    }

    #[test]
    fn null_error_display_distinguishes_from_absent() {
        let error = DocumentVersionDecodeError::Null;
        let rendered = format!("{error}");
        assert!(rendered.contains("null"));
        assert!(rendered.contains("Absent"));
    }

    // ── type distinctness: error cannot be matched as absent ─────────────────

    #[test]
    fn decode_error_is_not_absent() {
        // A caller that receives Err(_) cannot reach Absent without an explicit
        // adapter. This test is a compile-time assertion made runtime: the only
        // way to get Absent is through Ok(DocumentVersionField::Absent).
        let params = json!({"textDocument": {"version": null}});
        let result = decode_document_version(&params);
        // Must not be Ok(Absent) — the error path returns Err.
        assert!(result.is_err());
        assert_ne!(result.ok(), Some(DocumentVersionField::Absent));
    }

    // ── as_i32 accessor ───────────────────────────────────────────────────────

    #[test]
    fn client_document_version_as_i32_round_trips() {
        let params = json!({"textDocument": {"version": 999}});
        let DocumentVersionField::Explicit(v) = decode_document_version(&params).unwrap() else {
            panic!("expected Explicit");
        };
        assert_eq!(v.as_i32(), 999_i32);
    }

    #[test]
    fn client_document_version_as_i32_min() {
        let params = json!({"textDocument": {"version": i32::MIN}});
        let DocumentVersionField::Explicit(v) = decode_document_version(&params).unwrap() else {
            panic!("expected Explicit");
        };
        assert_eq!(v.as_i32(), i32::MIN);
    }

    // ── malformed / missing textDocument envelope ─────────────────────────────

    #[test]
    fn textdocument_is_not_an_object() {
        // If textDocument is present but not an object, version key lookup
        // returns None → Absent.
        let params = json!({"textDocument": "not-an-object"});
        // serde_json's .get() on a non-object returns None.
        assert_eq!(decode_document_version(&params), Ok(DocumentVersionField::Absent));
    }

    #[test]
    fn textdocument_is_null() {
        let params = json!({"textDocument": null});
        assert_eq!(decode_document_version(&params), Ok(DocumentVersionField::Absent));
    }

    #[test]
    fn params_is_not_an_object() {
        // A non-object params value has no textDocument key → Absent.
        let params = json!(null);
        assert_eq!(decode_document_version(&params), Ok(DocumentVersionField::Absent));
    }

    // ── decode_version_value directly ────────────────────────────────────────

    #[test]
    fn decode_version_value_valid() {
        assert_eq!(decode_version_value(&json!(5)), Ok(ClientDocumentVersion(5)));
    }

    #[test]
    fn decode_version_value_null() {
        assert_eq!(decode_version_value(&json!(null)), Err(DocumentVersionDecodeError::Null));
    }

    #[test]
    fn decode_version_value_string() {
        assert_eq!(
            decode_version_value(&json!("5")),
            Err(DocumentVersionDecodeError::NonInteger(JsonValueKind::String))
        );
    }

    // ── Display and Debug impls ───────────────────────────────────────────────

    #[test]
    fn json_value_kind_display() {
        assert_eq!(format!("{}", JsonValueKind::Null), "null");
        assert_eq!(format!("{}", JsonValueKind::Bool), "boolean");
        assert_eq!(format!("{}", JsonValueKind::Float), "float/non-integral-number");
        assert_eq!(format!("{}", JsonValueKind::String), "string");
        assert_eq!(format!("{}", JsonValueKind::Array), "array");
        assert_eq!(format!("{}", JsonValueKind::Object), "object");
    }

    #[test]
    fn integer_range_class_display() {
        let s = format!("{}", IntegerRangeClass::BelowI32Min);
        assert!(s.contains("i32::MIN") || s.contains("-2147483648"));
        let s = format!("{}", IntegerRangeClass::AboveI32Max);
        assert!(s.contains("i32::MAX") || s.contains("2147483647"));
        let s = format!("{}", IntegerRangeClass::AboveI64Max);
        assert!(s.contains("i64::MAX") || s.contains("9223372036854775807"));
    }

    #[test]
    fn client_document_version_display() {
        let v = ClientDocumentVersion(42);
        assert_eq!(format!("{v}"), "42");
    }

    // ── didSave equal-version policy cannot be activated by ordinary didChange ─

    #[test]
    fn same_version_explicit_is_explicit_not_absent() {
        // A didChange notification with version == current_version must go
        // through the version-staleness gate, not the versionless path.
        // This test proves the decoder produces Explicit rather than Absent,
        // so callers cannot silently bypass the monotonicity gate.
        let current_version = 3_i32;
        let params = json!({"textDocument": {"version": current_version}});
        let result = decode_document_version(&params).unwrap();
        assert_eq!(result, DocumentVersionField::Explicit(ClientDocumentVersion(3)));
        assert_ne!(result, DocumentVersionField::Absent);
    }
}
