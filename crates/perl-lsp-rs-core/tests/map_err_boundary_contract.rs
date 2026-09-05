//! #11246 contrast controls for `clippy::map_err_ignore` boundary dispositions.
//!
//! Pins the strongest accepted-boundary behaviors the research classification relies on.
//! Activates no lint and changes no production error model.
//! Toolchain pin: 1.95.0 (`rust-toolchain.toml`) — lint behavior is version-dependent.

use perl_lsp_rs_core::protocol::resolve_envelope::{
    ResolveEnvelopeRejection, ResolveEnvelopeToken,
};
use perl_lsp_rs_core::protocol::{
    DocumentVersionDecodeError, IntegerRangeClass, Signedness, decode_version_value, req_position,
};
use perl_test_must::{must_err, must_some};
use serde_json::json;

/// Stable-protocol control (#11246 `stable_protocol_mapping`): a `u32` overflow in
/// `position.line` maps to the exact public INVALID_PARAMS message; the discarded
/// `TryFromIntError` carries no payload beyond what this message already states,
/// so the mapping loses nothing diagnosable while keeping the wire contract stable.
#[test]
fn position_overflow_keeps_stable_invalid_params_contract() {
    let params = json!({"position": {"line": u64::from(u32::MAX) + 1, "character": 0}});
    let error = must_err(req_position(&params));
    assert_eq!(error.code, -32602);
    assert_eq!(error.message, "position.line exceeds u32::MAX");
    let rendered = format!("{error:?}");
    assert!(
        !rendered.contains("out of range"),
        "internal conversion text must not leak into the protocol error: {rendered}"
    );
}

/// Deliberate-redaction control (#11246 `redact_deliberately`): resolve-envelope token
/// rejections carry only coarse classes. Neither the raw input nor any internal shape
/// is echoed back through the public rejection surface, and the token's `Debug`
/// rendering exposes length, not content.
#[test]
fn malformed_resolve_envelope_rejections_stay_non_leaking() {
    let no_prefix = must_err(ResolveEnvelopeToken::parse("definitely-not-a-token"));
    assert_eq!(no_prefix, ResolveEnvelopeRejection::Malformed);

    let bad_hex = must_err(ResolveEnvelopeToken::parse("perl-lsp.resolve.v1:NOTHEX"));
    assert_eq!(bad_hex, ResolveEnvelopeRejection::Malformed);

    let oversized = must_err(ResolveEnvelopeToken::parse(format!(
        "perl-lsp.resolve.v1:{}",
        "ab".repeat(32 * 1024 + 1)
    )));
    assert_eq!(oversized, ResolveEnvelopeRejection::OversizedOrResourceBound);

    let well_formed = must_some(ResolveEnvelopeToken::parse("perl-lsp.resolve.v1:abcd").ok());
    let rendered = format!("{well_formed:?}");
    assert!(
        !rendered.contains("abcd"),
        "token debug rendering must not echo wire bytes: {rendered}"
    );
}

/// Honest-classification control (#11246 `retain_class_not_details`): an out-of-range
/// document version maps into sign-aware typed variants built from the value itself —
/// strictly more diagnostic information than the discarded `TryFromIntError`.
#[test]
fn document_version_out_of_range_maps_to_richer_typed_class() {
    let below_i32_min = must_err(decode_version_value(&json!(i64::from(i32::MIN) - 1)));
    assert_eq!(
        below_i32_min,
        DocumentVersionDecodeError::OutOfRange {
            sign: Signedness::Negative,
            bounded_class: IntegerRangeClass::BelowI32Min,
        }
    );

    let above_i64_max = must_err(decode_version_value(&json!(18_446_744_073_709_551_615_u64)));
    assert_eq!(
        above_i64_max,
        DocumentVersionDecodeError::OutOfRange {
            sign: Signedness::Positive,
            bounded_class: IntegerRangeClass::AboveI64Max,
        }
    );
}

/// Retain-cause repair-shape control (#11246 `retain_cause`): the honest form binds the
/// source error and preserves its diagnostic content; the lossy form discards it. This
/// contrast is the reference shape for every cohort repair of the 144 retain_cause rows;
/// #12600 records a live production instance of the lossy form.
#[test]
fn retain_cause_contrast_binding_preserves_diagnostic_content() {
    #[derive(Debug)]
    struct SourceError;

    let source: Result<usize, SourceError> = Err(SourceError);

    let lossy: String = must_err(source.map_err(|_| "conversion failed".to_string()));
    assert_eq!(lossy, "conversion failed");

    let source: Result<usize, SourceError> = Err(SourceError);
    let retained: String =
        must_err(source.map_err(|error| format!("conversion failed: {error:?}")));
    assert!(
        retained.contains("SourceError"),
        "the bound cause must survive into the mapped error: {retained}"
    );
}
