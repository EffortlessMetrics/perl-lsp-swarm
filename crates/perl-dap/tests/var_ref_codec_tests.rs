//! Red TDD tests for VariableReference codec (var_ref.rs).
//!
//! This test suite stages the H1-H5 hazard-class invariants for the DAP ref-space
//! type-separation refactoring. Tests are written to FAIL until the codec is fully
//! implemented by the builder.
//!
//! Hazard classes tested:
//! - **H1 (ID/ref-space collision):** Scope and EvalResult ranges never overlap.
//! - **H2 (Protocol-safe round-trip):** encode(decode(v)) == v preserves type and value.
//! - **H3 (Bounds/overflow safety):** Extreme inputs handled with saturating arithmetic.
//! - **H5 (Exhaustive decode):** Invalid ranges decode to None, never panic.

use perl_dap::var_ref::{ScopeKind, VariableReference};

// ============================================================================
// H1: No-collision (ID/ref-space collision, #1219 repro)
// ============================================================================

/// H1: Scope and EvalResult must have distinct wire values.
/// This is the #1219 collision test: frame_id=5000, kind=Locals encodes to 50_001,
/// which must be distinct from any EvalResult value.
#[test]
fn test_h1_no_collision_scope_vs_evalresult() -> Result<(), Box<dyn std::error::Error>> {
    let scope = VariableReference::Scope {
        frame_id: 5000,
        kind: ScopeKind::Locals,
    };
    let eval = VariableReference::EvalResult { counter: 1 };

    let scope_wire = scope.encode();
    let eval_wire = eval.encode();

    assert_ne!(
        scope_wire, eval_wire,
        "H1: Scope and EvalResult must have different wire values; got scope={}, eval={}",
        scope_wire, eval_wire
    );
    assert_eq!(
        scope_wire, 50_001,
        "H1: Scope{{frame_id: 5000, kind: Locals}} should encode as 50_001"
    );
    assert_eq!(
        eval_wire, 1_000_001,
        "H1: EvalResult{{counter: 1}} should encode as 1_000_001"
    );

    // Critical test: decode(50_001) must return Scope, NOT EvalResult
    let decoded = VariableReference::decode(50_001)
        .ok_or("H1: decode(50_001) should succeed")?;
    match decoded {
        VariableReference::Scope { frame_id, kind } => {
            assert_eq!(
                frame_id, 5000,
                "H1: decoded Scope frame_id should be 5000, got {}",
                frame_id
            );
            assert_eq!(
                kind, ScopeKind::Locals,
                "H1: decoded Scope kind should be Locals, got {:?}",
                kind
            );
        }
        _ => {
            panic!(
                "H1 VIOLATION: decode(50_001) returned {:?}, expected Scope(5000, Locals)",
                decoded
            );
        }
    }

    Ok(())
}

// ============================================================================
// H2: Wire round-trip (encode/decode preserves value and type)
// ============================================================================

/// H2: Scope{frame_id: 5000, kind: Locals} round-trips correctly.
#[test]
fn test_h2_roundtrip_scope_locals_5000() -> Result<(), Box<dyn std::error::Error>> {
    let original = VariableReference::Scope {
        frame_id: 5000,
        kind: ScopeKind::Locals,
    };
    let wire = original.encode();
    let decoded = VariableReference::decode(wire)
        .ok_or("H2: decode should succeed for valid Scope wire")?;

    assert_eq!(
        original, decoded,
        "H2: round-trip failed for Scope{{frame_id: 5000, kind: Locals}}; decoded={:?}",
        decoded
    );

    // Verify re-encoding matches
    let rewired = decoded.encode();
    assert_eq!(
        wire, rewired,
        "H2: encode(decode(w)) should equal w; wire={}, rewired={}",
        wire, rewired
    );

    Ok(())
}

/// H2: Scope{frame_id: 0, kind: Globals} round-trips.
#[test]
fn test_h2_roundtrip_scope_globals() -> Result<(), Box<dyn std::error::Error>> {
    let original = VariableReference::Scope {
        frame_id: 0,
        kind: ScopeKind::Globals,
    };
    let wire = original.encode();
    let decoded = VariableReference::decode(wire)
        .ok_or("H2: decode should succeed for valid Scope wire")?;

    assert_eq!(
        original, decoded,
        "H2: round-trip failed for Scope{{frame_id: 0, kind: Globals}}"
    );

    let rewired = decoded.encode();
    assert_eq!(
        wire, rewired,
        "H2: encode(decode(w)) == w for Scope Globals"
    );

    Ok(())
}

/// H2: Scope{frame_id: 999_999, kind: Package} round-trips (boundary case).
#[test]
fn test_h2_roundtrip_scope_package_max() -> Result<(), Box<dyn std::error::Error>> {
    let original = VariableReference::Scope {
        frame_id: 999_999,
        kind: ScopeKind::Package,
    };
    let wire = original.encode();
    let decoded = VariableReference::decode(wire)
        .ok_or("H2: decode should succeed for max Scope frame_id")?;

    assert_eq!(
        original, decoded,
        "H2: round-trip failed for Scope{{frame_id: 999_999, kind: Package}}"
    );

    let rewired = decoded.encode();
    assert_eq!(
        wire, rewired,
        "H2: encode(decode(w)) == w for max Scope"
    );

    Ok(())
}

/// H2: EvalResult{counter: 1_000_000} round-trips (large counter).
#[test]
fn test_h2_roundtrip_evalresult_counter_large() -> Result<(), Box<dyn std::error::Error>> {
    let original = VariableReference::EvalResult { counter: 1_000_000 };
    let wire = original.encode();
    let decoded = VariableReference::decode(wire)
        .ok_or("H2: decode should succeed for EvalResult")?;

    assert_eq!(
        original, decoded,
        "H2: round-trip failed for EvalResult{{counter: 1_000_000}}"
    );

    let rewired = decoded.encode();
    assert_eq!(
        wire, rewired,
        "H2: encode(decode(w)) == w for EvalResult large counter"
    );

    Ok(())
}

/// H2: EvalResult{counter: 0} round-trips (base case).
#[test]
fn test_h2_roundtrip_evalresult_counter_zero() -> Result<(), Box<dyn std::error::Error>> {
    let original = VariableReference::EvalResult { counter: 0 };
    let wire = original.encode();
    let decoded = VariableReference::decode(wire)
        .ok_or("H2: decode should succeed for EvalResult counter=0")?;

    assert_eq!(
        original, decoded,
        "H2: round-trip failed for EvalResult{{counter: 0}}"
    );

    let rewired = decoded.encode();
    assert_eq!(
        wire, rewired,
        "H2: encode(decode(w)) == w for EvalResult zero"
    );

    Ok(())
}

/// H2: Child{parent: 1000, index: 50} round-trips.
#[test]
fn test_h2_roundtrip_child_basic() -> Result<(), Box<dyn std::error::Error>> {
    let original = VariableReference::Child {
        parent: 1000,
        index: 50,
    };
    let wire = original.encode();
    let decoded = VariableReference::decode(wire)
        .ok_or("H2: decode should succeed for Child")?;

    assert_eq!(
        original, decoded,
        "H2: round-trip failed for Child{{parent: 1000, index: 50}}"
    );

    let rewired = decoded.encode();
    assert_eq!(
        wire, rewired,
        "H2: encode(decode(w)) == w for Child"
    );

    Ok(())
}

/// H2: Child at base (parent: 0, index: 0) round-trips.
#[test]
fn test_h2_roundtrip_child_base() -> Result<(), Box<dyn std::error::Error>> {
    let original = VariableReference::Child {
        parent: 0,
        index: 0,
    };
    let wire = original.encode();
    assert_eq!(wire, 2_000_000_000, "H2: Child{{parent: 0, index: 0}} should encode to base 2_000_000_000");

    let decoded = VariableReference::decode(wire)
        .ok_or("H2: decode should succeed for Child at base")?;

    assert_eq!(
        original, decoded,
        "H2: round-trip failed for Child{{parent: 0, index: 0}}"
    );

    Ok(())
}

// ============================================================================
// H3: Bounds/overflow safety (no panic on extreme inputs)
// ============================================================================

/// H3: Encode with i32::MAX in frame_id does not panic (uses saturating arithmetic).
#[test]
fn test_h3_encode_frame_id_i32_max_no_panic() -> Result<(), Box<dyn std::error::Error>> {
    let var = VariableReference::Scope {
        frame_id: i32::MAX,
        kind: ScopeKind::Locals,
    };
    // Must not panic; should saturate
    let _wire = var.encode();
    Ok(())
}

/// H3: Encode with i32::MAX in counter does not panic.
#[test]
fn test_h3_encode_counter_i32_max_no_panic() -> Result<(), Box<dyn std::error::Error>> {
    let var = VariableReference::EvalResult { counter: i32::MAX };
    // Must not panic; should saturate
    let _wire = var.encode();
    Ok(())
}

/// H3: Decode(i32::MAX) does not panic; returns Some or None.
#[test]
fn test_h3_decode_i32_max_no_panic() -> Result<(), Box<dyn std::error::Error>> {
    // Must not panic; should return Some(Child variant) or None
    let result = VariableReference::decode(i32::MAX);
    // Result can be Some or None, just must not panic
    let _ = result;
    Ok(())
}

/// H3: Decode(i32::MIN) does not panic.
#[test]
fn test_h3_decode_i32_min_no_panic() -> Result<(), Box<dyn std::error::Error>> {
    // Must not panic
    let result = VariableReference::decode(i32::MIN);
    let _ = result;
    Ok(())
}

/// H3: Child with index: u32::MAX does not panic (bit-packing).
#[test]
fn test_h3_child_index_u32_max_no_panic() -> Result<(), Box<dyn std::error::Error>> {
    let var = VariableReference::Child {
        parent: 1000,
        index: u32::MAX,
    };
    // Must not panic; bit-packing should saturate or overflow safely
    let _wire = var.encode();
    Ok(())
}

// ============================================================================
// H5: Exhaustive decode (invalid ranges return None, not panic)
// ============================================================================

/// H5: Decode(0) returns None (out of range, no variant maps to 0).
#[test]
fn test_h5_decode_zero_returns_none() -> Result<(), Box<dyn std::error::Error>> {
    let result = VariableReference::decode(0);
    assert_eq!(
        result, None,
        "H5: decode(0) should return None (out of Scope [1..], EvalResult [1_000_000..], Child [2_000_000_000..] ranges)"
    );
    Ok(())
}

/// H5: Decode(-1) returns None (negative, out of range).
#[test]
fn test_h5_decode_negative_one_returns_none() -> Result<(), Box<dyn std::error::Error>> {
    let result = VariableReference::decode(-1);
    assert_eq!(
        result, None,
        "H5: decode(-1) should return None"
    );
    Ok(())
}

/// H5: Decode(i32::MIN) returns None.
#[test]
fn test_h5_decode_i32_min_returns_none() -> Result<(), Box<dyn std::error::Error>> {
    let result = VariableReference::decode(i32::MIN);
    // For the most negative i32, result should be None (unless it happens to fall in Child range, which it shouldn't)
    // Since Child base is 2_000_000_000 (positive), i32::MIN definitely returns None
    assert_eq!(
        result, None,
        "H5: decode(i32::MIN) should return None"
    );
    Ok(())
}

/// H5: Decode(999_999) in the gap (Scope max ~9_999_999, EvalResult base 1_000_000).
/// The gap exists between Scope max boundary and EvalResult base 1_000_000.
/// Wire 999_999: frame_id = 99_999, kind_disc = 9 (invalid, kind must be 1-3)
/// So decode(999_999) should return None (invalid kind).
#[test]
fn test_h5_decode_gap_999999_returns_none() -> Result<(), Box<dyn std::error::Error>> {
    let result = VariableReference::decode(999_999);
    assert_eq!(
        result, None,
        "H5: decode(999_999) should return None (frame_id=99_999, kind_disc=9 is invalid; must be 1-3)"
    );
    Ok(())
}

/// H5: Decode at gap boundary (900_001 with invalid kind_disc).
/// Wire 900_001: frame_id = 90_000, kind_disc = 1 (valid). Should decode to Scope.
/// This is NOT a gap case; included to verify boundary.
#[test]
fn test_h5_decode_scope_boundary_valid() -> Result<(), Box<dyn std::error::Error>> {
    let result = VariableReference::decode(900_001);
    assert!(
        result.is_some(),
        "H5: decode(900_001) should be Some (frame_id=90_000, kind_disc=1 is valid)"
    );
    match result.unwrap() {
        VariableReference::Scope { frame_id, kind } => {
            assert_eq!(frame_id, 90_000, "H5: frame_id should be 90_000");
            assert_eq!(kind, ScopeKind::Locals, "H5: kind should be Locals (disc=1)");
        }
        _ => panic!("H5: decode(900_001) should return Scope, not {:?}", result),
    }
    Ok(())
}

/// H5: Decode with invalid scope kind discriminant.
/// Wire 900_004: frame_id = 90_000, kind_disc = 4 (invalid, must be 1-3).
/// Should return None.
#[test]
fn test_h5_decode_invalid_scope_kind_returns_none() -> Result<(), Box<dyn std::error::Error>> {
    let wire = 900_004; // frame_id = 90_000, kind_disc = 4
    let result = VariableReference::decode(wire);
    assert_eq!(
        result, None,
        "H5: decode(900_004) should return None (kind_disc=4 is invalid for Scope)"
    );
    Ok(())
}

/// H5: Decode at EvalResult base edge (1_000_000) should return EvalResult counter=0.
#[test]
fn test_h5_decode_evalresult_base_valid() -> Result<(), Box<dyn std::error::Error>> {
    let result = VariableReference::decode(1_000_000);
    assert!(
        result.is_some(),
        "H5: decode(1_000_000) should be Some (EvalResult base)"
    );
    match result.unwrap() {
        VariableReference::EvalResult { counter } => {
            assert_eq!(counter, 0, "H5: EvalResult at base should have counter=0");
        }
        _ => panic!("H5: decode(1_000_000) should return EvalResult, not {:?}", result),
    }
    Ok(())
}

/// H5: Decode at Child base edge (2_000_000_000) should return Child{parent: 0, index: 0}.
#[test]
fn test_h5_decode_child_base_valid() -> Result<(), Box<dyn std::error::Error>> {
    let result = VariableReference::decode(2_000_000_000);
    assert!(
        result.is_some(),
        "H5: decode(2_000_000_000) should be Some (Child base)"
    );
    match result.unwrap() {
        VariableReference::Child { parent, index } => {
            assert_eq!(parent, 0, "H5: Child at base should have parent=0");
            assert_eq!(index, 0, "H5: Child at base should have index=0");
        }
        _ => panic!("H5: decode(2_000_000_000) should return Child, not {:?}", result),
    }
    Ok(())
}

// ============================================================================
// ScopeKind::TryFrom<i32> tests (bonus: ensures ScopeKind validity)
// ============================================================================

/// Verify ScopeKind::try_from accepts valid discriminants (1, 2, 3).
#[test]
fn test_scope_kind_try_from_valid_locals() -> Result<(), Box<dyn std::error::Error>> {
    let kind = ScopeKind::try_from(1)?;
    assert_eq!(kind, ScopeKind::Locals, "ScopeKind::try_from(1) should be Locals");
    Ok(())
}

#[test]
fn test_scope_kind_try_from_valid_package() -> Result<(), Box<dyn std::error::Error>> {
    let kind = ScopeKind::try_from(2)?;
    assert_eq!(kind, ScopeKind::Package, "ScopeKind::try_from(2) should be Package");
    Ok(())
}

#[test]
fn test_scope_kind_try_from_valid_globals() -> Result<(), Box<dyn std::error::Error>> {
    let kind = ScopeKind::try_from(3)?;
    assert_eq!(kind, ScopeKind::Globals, "ScopeKind::try_from(3) should be Globals");
    Ok(())
}

/// Verify ScopeKind::try_from rejects invalid discriminants (0, 4, -1).
#[test]
fn test_scope_kind_try_from_invalid_zero() -> Result<(), Box<dyn std::error::Error>> {
    let result = ScopeKind::try_from(0);
    assert!(result.is_err(), "ScopeKind::try_from(0) should be Err");
    Ok(())
}

#[test]
fn test_scope_kind_try_from_invalid_four() -> Result<(), Box<dyn std::error::Error>> {
    let result = ScopeKind::try_from(4);
    assert!(result.is_err(), "ScopeKind::try_from(4) should be Err");
    Ok(())
}

#[test]
fn test_scope_kind_try_from_invalid_negative() -> Result<(), Box<dyn std::error::Error>> {
    let result = ScopeKind::try_from(-1);
    assert!(result.is_err(), "ScopeKind::try_from(-1) should be Err");
    Ok(())
}
