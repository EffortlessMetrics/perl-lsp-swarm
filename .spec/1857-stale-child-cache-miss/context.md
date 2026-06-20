# Context: Issue #1857 — Stale Child variablesReference on cache miss

## Problem Statement

After `variable_cache.clear()` on resume (continue/next/step), a stale `Child` variablesReference allocated during the previous stopped state causes a silent empty-response bug. The code path for stale `Child` refs differs from stale `EvalResult` refs:

- **EvalResult refs** (lines 127-139 in `variables.rs`): explicit short-circuit check returns honest-empty (`success=true, variables=[]`) immediately.
- **Child refs**: no early check; fall through to the `None` branch (line 245) and implicitly produce empty response via output-parsing machinery, which is semantically silent and confusing.

This violates the principle established by fix #1338: stale refs after resume should return honest-empty with an explicit short-circuit check, not silent fall-through.

## Root Cause

The fix for #1338 (merged) added an explicit short-circuit for stale `EvalResult` refs in the `cache_miss` branch of `handle_variables()`. However, it did not add a parallel short-circuit for stale `Child` refs. The `Child` variant is not checked in the early return block (lines 127-139); instead, it falls through to the `None` branch (line 245-252) and implicitly produces an empty list via normal output-parsing machinery.

## Key Decisions and Alternatives

### Option 1 (Chosen): Parallel short-circuit check for Child
Add a second check immediately after the existing `EvalResult` short-circuit (or combine both into a single match):
```rust
if matches!(
    VariableReference::decode(variables_ref),
    Some(VariableReference::Child { .. })
) {
    return DapMessage::Response {
        seq,
        request_seq,
        success: true,
        command: "variables".to_string(),
        body: Some(json!({ "variables": [] })),
        message: None,
    };
}
```
**Rationale**: Mirrors the exact pattern of the `EvalResult` short-circuit. Consistent, explicit, easy to verify. Both variant checks are now early returns that prevent any downstream scope routing or debugger querying.

### Option 2: Combine into single match
```rust
if matches!(
    VariableReference::decode(variables_ref),
    Some(VariableReference::EvalResult { .. }) | Some(VariableReference::Child { .. })
) {
    // shared short-circuit
}
```
**Rationale**: Slightly more compact. However, less consistent with the existing code structure (one check per variant), and marginally harder to read the dual-case pattern.

**Chosen**: Option 1 (parallel check) for consistency with existing `EvalResult` structure and to preserve the existing read pattern. Option 1 is the more maintainable approach.

## Acceptance Test Design

The test (named `test_stale_child_ref_after_resume`) verifies:
1. A session in `Stopped` state.
2. A `Child` variablesReference encoded and stored (simulating a cached child from the previous stop).
3. `variable_cache.clear()` is called to simulate resume (continue/next/step).
4. A variables request for the stale `Child` ref is made.
5. **Assertion**: Response is `success=true` with `variables=[]` (honest-empty), not a crash or bogus data.

This mirrors the test pattern in `eval_ref_cache_miss_resume_tests.rs` (the #1338 test suite).

## Related Issues and Prior Art

- **#1338** (merged): Introduced the stale-ref short-circuit pattern for `EvalResult` refs. The comment at line 250-251 in `variables.rs` references issue #1445, but #1445 was a wire-band collision bug (different issue). The gap for `Child` refs is tracked in this issue (#1857).
- **#1445** (closed): Wire-band collision between `Child` and `EvalResult` refs. Fixed by the `var_ref.rs` codec that provides pure-range band classification.
- **#1219**: Original identification of the collision hazard; led to the creation of `var_ref.rs` codec.

## Confidence Assessment

**High confidence**. The issue premise is ratified (verified by code review in issue comment), the fix sketch is sound, and the test design mirrors an established pattern from #1338. The fix is isolated to a single short-circuit check in a single function. No compilation-order or dependency issues.

## Spec-Planner Findings

- ✓ File exists: `crates/perl-dap/src/debug_adapter/variables.rs`
- ✓ EvalResult short-circuit exists at lines 127-139 as described
- ✓ Non-Scope branch (None case) at line 245-252; comment mentions the gap at lines 250-251
- ✓ `VariableReference::Child` variant is properly defined and decoded in `var_ref.rs`
- ✓ Test patterns established in `eval_ref_cache_miss_resume_tests.rs` and `dap_variable_reference_hardening_tests.rs`
- ✓ No API-surface changes; no new types, only a control-flow check
- ✓ Scope is size/S as stated: ~10 lines of code + ~20 lines of test + comment update

All premises verified against current main. Ready for builder.
