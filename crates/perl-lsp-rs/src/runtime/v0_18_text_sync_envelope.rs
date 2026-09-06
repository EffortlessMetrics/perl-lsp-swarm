//! v0.18 selected text/position envelope (#8129).
//!
//! Current main cannot prove atomic incremental UTF-8/UTF-16 (#1814/#1690/#7409/#7417
//! remain open). The honest supported envelope is full-document transfer with UTF-16
//! as the only wire encoding. Ranged `didChange` members, clamping, and silent skips
//! are protocol violations, not supported incremental synchronization.

use crate::protocol::{JsonRpcError, invalid_params};
use crate::textdoc::strip_utf8_bom;
use lsp_types::TextDocumentContentChangeEvent;
use serde_json::Value;

/// Decision recorded for v0.18 until a later atomic-incremental cutover.
pub(crate) const DECISION: &str = "full_document_utf16";
/// Advertised and stored wire encoding for an accepted session.
pub(crate) const WIRE_ENCODING: &str = "utf-16";
/// LSP `TextDocumentSyncKind::Full`.
pub(crate) const TEXT_SYNC_KIND_FULL: i32 = 1;

/// Why UTF-16 was selected for an accepted initialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Utf16SelectionReason {
    /// `general.positionEncodings` was absent or JSON null.
    Omitted,
    /// Client sent an empty string array.
    Empty,
    /// Client list contained `utf-16`.
    ClientOfferedUtf16,
}

/// Classify `capabilities.general.positionEncodings` for the v0.18 envelope.
pub(crate) fn classify_position_encoding_offer(
    params: &Value,
) -> Result<Utf16SelectionReason, JsonRpcError> {
    match params.pointer("/capabilities/general/positionEncodings") {
        None | Some(Value::Null) => Ok(Utf16SelectionReason::Omitted),
        Some(Value::Array(entries)) if entries.is_empty() => Ok(Utf16SelectionReason::Empty),
        Some(Value::Array(entries)) => {
            let mut saw_utf16 = false;
            for entry in entries {
                let Some(encoding) = entry.as_str() else {
                    return Err(invalid_params(
                        "capabilities.general.positionEncodings must be an array of strings",
                    ));
                };
                if encoding == WIRE_ENCODING {
                    saw_utf16 = true;
                }
            }
            if saw_utf16 {
                Ok(Utf16SelectionReason::ClientOfferedUtf16)
            } else {
                Err(invalid_params(
                    "v0.18 supports only UTF-16 position encoding; \
                     capabilities.general.positionEncodings listed no utf-16 value",
                ))
            }
        }
        Some(_) => Err(invalid_params(
            "capabilities.general.positionEncodings must be an array of strings",
        )),
    }
}

/// Outcome of admitting one `textDocument/didChange` `contentChanges` array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FullDocumentAdmission {
    /// Every member is an unranged complete replacement. Apply in order; last text wins.
    Accepted { replacements: Vec<String> },
    /// A ranged, missing, empty, or malformed member. Commit nothing.
    Violation { reason: &'static str },
}

/// Admit only complete replacements. Any ranged or malformed member is a violation.
pub(crate) fn admit_full_document_changes(changes: &[Value]) -> FullDocumentAdmission {
    if changes.is_empty() {
        return FullDocumentAdmission::Violation {
            reason: "contentChanges must contain at least one full-document replacement",
        };
    }

    let mut replacements = Vec::with_capacity(changes.len());
    for change in changes {
        match serde_json::from_value::<TextDocumentContentChangeEvent>(change.clone()) {
            Ok(event) if event.range.is_none() => {
                replacements.push(strip_utf8_bom(&event.text).to_string());
            }
            Ok(_) => {
                return FullDocumentAdmission::Violation {
                    reason: "ranged contentChanges are unsupported under advertised Full text sync",
                };
            }
            Err(_) => {
                return FullDocumentAdmission::Violation {
                    reason: "malformed contentChanges member; no partial text was committed",
                };
            }
        }
    }
    FullDocumentAdmission::Accepted { replacements }
}

/// Final source after privately applying ordered full replacements.
pub(crate) fn final_full_replacement_text(replacements: &[String]) -> Option<&str> {
    replacements.last().map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn omitted_and_empty_position_encodings_select_utf16() {
        assert_eq!(
            classify_position_encoding_offer(&json!({})).expect("omitted"),
            Utf16SelectionReason::Omitted
        );
        assert_eq!(
            classify_position_encoding_offer(&json!({
                "capabilities": { "general": { "positionEncodings": null } }
            }))
            .expect("null"),
            Utf16SelectionReason::Omitted
        );
        assert_eq!(
            classify_position_encoding_offer(&json!({
                "capabilities": { "general": { "positionEncodings": [] } }
            }))
            .expect("empty"),
            Utf16SelectionReason::Empty
        );
    }

    #[test]
    fn list_containing_utf16_selects_utf16_even_when_utf8_is_preferred() {
        let reason = classify_position_encoding_offer(&json!({
            "capabilities": { "general": { "positionEncodings": ["utf-8", "utf-16"] } }
        }))
        .expect("contains utf-16");
        assert_eq!(reason, Utf16SelectionReason::ClientOfferedUtf16);
    }

    #[test]
    fn list_without_utf16_fails_initialize_instead_of_silent_fallback() {
        let err = classify_position_encoding_offer(&json!({
            "capabilities": { "general": { "positionEncodings": ["utf-8"] } }
        }))
        .expect_err("utf-8-only must fail");
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("UTF-16"), "{}", err.message);

        let err = classify_position_encoding_offer(&json!({
            "capabilities": { "general": { "positionEncodings": ["utf-32"] } }
        }))
        .expect_err("utf-32-only must fail");
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn malformed_position_encodings_shape_fails() {
        let err = classify_position_encoding_offer(&json!({
            "capabilities": { "general": { "positionEncodings": "utf-16" } }
        }))
        .expect_err("non-array");
        assert_eq!(err.code, -32602);

        let err = classify_position_encoding_offer(&json!({
            "capabilities": { "general": { "positionEncodings": [1] } }
        }))
        .expect_err("non-string entry");
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn full_replacement_array_is_admitted_and_last_text_wins() {
        let admission = admit_full_document_changes(&[
            json!({ "text": "first\n" }),
            json!({ "text": "\u{FEFF}second\n" }),
        ]);
        match admission {
            FullDocumentAdmission::Accepted { replacements } => {
                assert_eq!(replacements, vec!["first\n".to_string(), "second\n".to_string()]);
                assert_eq!(final_full_replacement_text(&replacements), Some("second\n"));
            }
            FullDocumentAdmission::Violation { reason } => {
                panic!("expected admission, got {reason}")
            }
        }
    }

    #[test]
    fn ranged_or_malformed_or_empty_array_is_a_violation() {
        assert!(matches!(
            admit_full_document_changes(&[]),
            FullDocumentAdmission::Violation { .. }
        ));
        assert!(matches!(
            admit_full_document_changes(&[json!({
                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } },
                "text": "x"
            })]),
            FullDocumentAdmission::Violation { .. }
        ));
        assert!(matches!(
            admit_full_document_changes(&[json!({ "text": "ok\n" }), json!({ "range": true })]),
            FullDocumentAdmission::Violation { .. }
        ));
        let mixed = admit_full_document_changes(&[
            json!({ "text": "committed-if-partial\n" }),
            json!({
                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } },
                "text": "x"
            }),
        ]);
        assert!(
            matches!(mixed, FullDocumentAdmission::Violation { .. }),
            "valid first member plus ranged second must not admit a partial array"
        );
    }
}
