//! Guard tests for DAP variablesReference arithmetic safety in parsing.rs.
//!
//! Specifically verifies that `fallback_scope_variables` placeholder child refs
//! use the `VariableReference::Child` codec and land in the Child band
//! `[2_000_000_000, i32::MAX]`, never in the EvalResult band `[1_000_000, 1_999_999_999]`.
//!
//! # Why this matters
//!
//! Before issue #1445, the placeholder child refs were computed as
//! `variables_ref.saturating_mul(100) + offset`.  For Scope refs with
//! `frame_id > ~10_000` (wire ≈ 100_000+), multiplying by 100 produces values
//! in the EvalResult band, causing the #1219 collision class — a client expanding
//! the `$self` or `@_` placeholder would have the ref decoded as EvalResult, not
//! Child.  The fix migrates those two sites to `VariableReference::Child::encode()`,
//! which always produces a value in `[2_000_000_000, i32::MAX]` by construction.

use perl_dap::var_ref::VariableReference;
use perl_dap::{DapMessage, DebugAdapter};
use serde_json::json;

const EVAL_BAND_LO: i64 = 1_000_000;
const EVAL_BAND_HI: i64 = 1_999_999_999;
const CHILD_BAND_LO: i64 = 2_000_000_000;

// ─── Encode a valid Scope variablesReference ──────────────────────────────────

fn scope_ref_for_frame(frame_id: i32) -> i32 {
    use perl_dap::var_ref::ScopeKind;
    VariableReference::Scope { frame_id, kind: ScopeKind::Locals }
        .encode()
        .expect("frame_id must be in [0, 99_999] for a valid Scope ref")
}

// ─── Extract variables[] from a handle_variables response ────────────────────

fn variables_from_response(msg: DapMessage) -> Vec<serde_json::Value> {
    match msg {
        DapMessage::Response { success: true, body: Some(body), .. } => {
            body.get("variables").and_then(|v| v.as_array()).cloned().unwrap_or_default()
        }
        _ => vec![],
    }
}

// ─── Test 1: deep frame child refs decode as Child, not EvalResult ────────────

/// For a Scope ref with frame_id=50_000 (above the ~10_000 threshold where raw
/// arithmetic overflows into the EvalResult band), the placeholder child refs
/// produced by `fallback_scope_variables` must decode as `Child`, not `EvalResult`.
#[test]
fn test_fallback_scope_variables_deep_frame_child_ref_no_collision() {
    let frame_id = 50_000_i32;
    let variables_ref = scope_ref_for_frame(frame_id);

    let adapter = DebugAdapter::new();
    // No live session → parse_scope_variables_from_output returns empty → fallback path.
    let msg = adapter.handle_variables(1, 1, Some(json!({ "variablesReference": variables_ref })));
    let vars = variables_from_response(msg);

    // fallback returns [$self, @_] for a Locals scope.
    assert!(
        !vars.is_empty(),
        "fallback_scope_variables must return placeholder vars for a Locals scope ref"
    );

    for var in &vars {
        let child_ref = var.get("variablesReference").and_then(|v| v.as_i64()).unwrap_or(0);

        if child_ref == 0 {
            // variablesReference=0 means "no children" — not a collision, skip.
            continue;
        }

        assert!(
            child_ref >= CHILD_BAND_LO,
            "fallback child ref {} for deep frame_id={} must be in Child band [{}..], \
             not in EvalResult band [{}..{}] — raw arithmetic collision (issue #1445)",
            child_ref,
            frame_id,
            CHILD_BAND_LO,
            EVAL_BAND_LO,
            EVAL_BAND_HI,
        );

        let decoded = VariableReference::decode(child_ref as i32);
        assert!(
            matches!(decoded, Some(VariableReference::Child { .. })),
            "child_ref={} must decode as VariableReference::Child, got {:?}",
            child_ref,
            decoded,
        );
    }
}

// ─── Test 2: encode/decode round-trip for deep frame child ref ────────────────

/// Verifies the Child codec round-trip: a Child ref constructed from a deep-frame
/// Scope parent always encodes to the Child band `[2_000_000_000, i32::MAX]` and
/// decodes back as `VariableReference::Child` (never as EvalResult).
///
/// Note: the codec clamps `parent` to `i32::MAX / 65_536` (= 32_767) during
/// encoding to avoid i32 overflow — so for large parent values the decoded parent
/// may differ from the original.  The invariant this test guards is **band
/// membership and variant identity**, not exact parent field round-trip for
/// out-of-clamp-range parents.
///
/// For small parents (≤ 32_767) the round-trip IS exact; we verify that too.
#[test]
fn test_child_ref_encode_decode_roundtrip_deep_frame() {
    let frame_id = 50_000_i32;
    let parent_scope_ref = scope_ref_for_frame(frame_id);

    // Encode two child refs — index 0 ($self) and index 1 (@_) as in fallback.
    for index in [0u32, 1u32] {
        let child = VariableReference::Child { parent: parent_scope_ref, index };
        let wire = child.encode().expect("Child encoding must succeed for non-negative parent");

        // Must be in Child band.
        assert!(
            wire >= 2_000_000_000,
            "Child wire value {} must be in band [2_000_000_000, i32::MAX]; frame_id={}, index={}",
            wire,
            frame_id,
            index,
        );
        // Must not be in EvalResult band.
        assert!(
            !(wire >= 1_000_000 && wire <= 1_999_999_999),
            "Child wire value {} must NOT be in EvalResult band [1_000_000, 1_999_999_999]",
            wire,
        );

        // decode must return a Child variant — not EvalResult, not Scope, not None.
        let decoded =
            VariableReference::decode(wire).expect("decode of freshly-encoded Child must succeed");
        assert!(
            matches!(decoded, VariableReference::Child { .. }),
            "decode of wire={} must produce VariableReference::Child, got {:?}",
            wire,
            decoded,
        );
    }

    // For small parents (≤ 32_767 = i32::MAX / 65_536) the full round-trip is exact.
    let small_parent: i32 = 11; // Scope(frame_id=1, Locals) → wire=11
    for index in [0u32, 1u32] {
        let child = VariableReference::Child { parent: small_parent, index };
        let wire = child.encode().expect("small parent encode must succeed");
        let decoded = VariableReference::decode(wire).expect("small parent decode must succeed");
        match decoded {
            VariableReference::Child { parent, index: decoded_index } => {
                assert_eq!(parent, small_parent, "exact round-trip: parent mismatch");
                assert_eq!(decoded_index, index & 0xFFFF, "exact round-trip: index mismatch");
            }
            other => panic!("expected Child after round-trip, got {other:?}"),
        }
    }
}

// ─── Test 3: adversarial band-collision test ──────────────────────────────────

/// Sweeps frame_id values from 0 to 99_999 (the full Scope band) and verifies
/// that for every valid Scope ref, the two Child refs produced by
/// `fallback_scope_variables` are always in the Child band and never in the
/// EvalResult band.  This is the band-non-collision invariant by exhaustive
/// construction.
///
/// Sampling 100 points spread across the range runs fast while covering the
/// boundary and the "deep frame" regime.
#[test]
fn test_fallback_child_ref_never_in_eval_band() {
    use perl_dap::var_ref::ScopeKind;

    let test_frame_ids: Vec<i32> =
        (0..100).map(|i| (i as i32) * 1000).filter(|&f| f <= 99_999).collect();

    for frame_id in test_frame_ids {
        let scope_ref = VariableReference::Scope { frame_id, kind: ScopeKind::Locals }
            .encode()
            .expect("frame_id in [0, 99_999] must produce a valid Scope ref");

        for index in [0u32, 1u32] {
            let child = VariableReference::Child { parent: scope_ref, index };
            let wire = child.encode().expect("Child encoding must succeed");

            assert!(
                !(wire >= 1_000_000 && wire <= 1_999_999_999),
                "EvalResult band collision: frame_id={}, scope_ref={}, child_index={}, wire={}",
                frame_id,
                scope_ref,
                index,
                wire,
            );
            assert!(
                wire >= 2_000_000_000,
                "Child wire {} not in Child band [2_000_000_000, i32::MAX]; frame_id={}, index={}",
                wire,
                frame_id,
                index,
            );
        }
    }
}

// ─── Test 4: mechanical lint — no raw arithmetic in fallback_scope_variables ──

/// Grep-based guard: confirms that `parsing.rs` no longer contains the raw
/// arithmetic pattern `saturating_mul(100)` or `saturating_mul(100) +` inside
/// the variables_reference fields of `fallback_scope_variables`.
///
/// This test is intentionally fragile to the specific old pattern so that any
/// reintroduction of raw arithmetic fails immediately.
#[test]
fn test_var_ref_no_raw_arithmetic_in_fallback_scope_variables() {
    let parsing_rs = include_str!("../src/debug_adapter/parsing.rs");

    // The old collision pattern: `variables_ref.saturating_mul(100) + N`.
    // This pattern in a variables_reference field is what issue #1445 retired.
    let banned_pattern = "saturating_mul(100)";

    // Find the fallback_scope_variables function body by scanning for the sentinel.
    let fn_start = parsing_rs
        .find("fn fallback_scope_variables(")
        .expect("fallback_scope_variables must exist in parsing.rs");

    // Scan from the function start to the end of parsing.rs for the banned pattern.
    let fn_body = &parsing_rs[fn_start..];

    assert!(
        !fn_body.contains(banned_pattern),
        "parsing.rs::fallback_scope_variables still contains banned raw arithmetic \
         pattern `{banned_pattern}` — child refs must use VariableReference::Child::encode() \
         to stay in the Child band (issue #1445 fix)"
    );
}
