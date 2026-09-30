//! v0.18 selected text/position envelope (#8129).
//!
//! Current main cannot prove atomic incremental UTF-8/UTF-16 (#1814/#1690/#7409/#7417
//! remain open). The honest supported envelope is full-document transfer with UTF-16
//! as the only wire encoding. Ranged `didChange` members, clamping, and silent skips
//! are protocol violations, not supported incremental synchronization.

use crate::textdoc::strip_utf8_bom;
use lsp_types::TextDocumentContentChangeEvent;
use serde_json::Value;

/// Decision recorded for v0.18 until a later atomic-incremental cutover.
pub(crate) const DECISION: &str = "full_document_utf16";
/// Advertised and stored wire encoding for an accepted session.
pub(crate) const WIRE_ENCODING: &str = "utf-16";
/// Wire/doctor name for `TextDocumentSyncKind::Full`.
pub(crate) const TEXT_SYNC_KIND_NAME: &str = "full";

/// Stable stderr category for Full-sync `contentChanges` protocol violations.
///
/// Exact-process redaction proof (`perllsp` `lsp_text_sync_redaction`) requires this
/// field and a numeric `change_index` without echoing member payloads.
pub(crate) const INVALID_CONTENT_CHANGE: &str = "invalid_content_change";

/// Outcome of admitting one `textDocument/didChange` `contentChanges` array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FullDocumentAdmission {
    /// Every member is an unranged complete replacement. Apply in order; last text wins.
    Accepted { replacements: Vec<String> },
    /// A ranged, missing, empty, or malformed member. Commit nothing.
    Violation {
        reason: &'static str,
        /// Index of the first violating member when the outer array exists.
        /// Empty/missing arrays have no member index.
        change_index: Option<usize>,
    },
}

/// Admit only complete replacements. Any ranged or malformed member is a violation.
pub(crate) fn admit_full_document_changes(changes: &[Value]) -> FullDocumentAdmission {
    if changes.is_empty() {
        return FullDocumentAdmission::Violation {
            reason: "contentChanges must contain at least one full-document replacement",
            change_index: None,
        };
    }

    let mut replacements = Vec::with_capacity(changes.len());
    for (change_index, change) in changes.iter().enumerate() {
        match serde_json::from_value::<TextDocumentContentChangeEvent>(change.clone()) {
            // The LSP full-document event is the text-only variant: the raw
            // member must not carry a `range` key at all, because serde also
            // deserializes an explicit `"range": null` into `None`.
            Ok(event) if change.get("range").is_none() => {
                replacements.push(strip_utf8_bom(&event.text).to_string());
            }
            Ok(_) => {
                return FullDocumentAdmission::Violation {
                    reason: "ranged contentChanges are unsupported under advertised Full text sync",
                    change_index: Some(change_index),
                };
            }
            Err(_) => {
                return FullDocumentAdmission::Violation {
                    reason: "malformed contentChanges member; no partial text was committed",
                    change_index: Some(change_index),
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
    #![expect(
        clippy::expect_used,
        clippy::panic,
        reason = "unit tests of encode/admit classification"
    )]
    use super::*;
    use serde_json::json;

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
            FullDocumentAdmission::Violation { reason, .. } => {
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
        // An explicit `"range": null` is not the text-only full-document
        // variant: the raw member must not carry a `range` key at all.
        assert!(matches!(
            admit_full_document_changes(&[json!({ "range": null, "text": "ok\n" })]),
            FullDocumentAdmission::Violation { change_index: Some(0), .. }
        ));
        let mixed = admit_full_document_changes(&[
            json!({ "text": "committed-if-partial\n" }),
            json!({
                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } },
                "text": "x"
            }),
        ]);
        assert!(
            matches!(mixed, FullDocumentAdmission::Violation { change_index: Some(1), .. }),
            "valid first member plus ranged second must not admit a partial array"
        );
        assert!(matches!(
            admit_full_document_changes(&[]),
            FullDocumentAdmission::Violation { change_index: None, .. }
        ));
        assert!(matches!(
            admit_full_document_changes(&[
                json!({ "range": "INVALID_RANGE_CANARY", "text": "secret" })
            ]),
            FullDocumentAdmission::Violation { change_index: Some(0), .. }
        ));
    }
}
