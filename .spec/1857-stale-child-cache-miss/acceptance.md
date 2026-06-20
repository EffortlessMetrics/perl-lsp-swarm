# Acceptance Criteria: Issue #1857

## §Behavior

**Goal:** Stale Child variablesReferences (allocated during previous stop, invalid after cache clear on resume) return honest-empty response instead of silent empty.

| Input | Condition | Expected Result | Test Name |
|-------|-----------|-----------------|-----------|
| Child variablesReference wire (e.g., `2_000_000_100`) | Session Stopped, cache miss (ref not in cache) | Response: `success=true, variables=[], message=null` (honest-empty) | `test_stale_child_ref_after_resume` |
| Child variablesReference wire | Session Running (implies cache cleared) | Response: `success=true, variables=[]` (stale-ref guard) | `test_stale_child_ref_with_running_session` |
| Child variablesReference wire | Cache hit (ref exists in cache) | Response: cached children (normal path, unchanged) | (covered by existing cache-hit tests) |
| EvalResult variablesReference wire (e.g., `1_000_000`) | Session Stopped, cache miss | Response: `success=true, variables=[]` (existing behavior, unchanged) | `eval_ref_wire_decodes_as_eval_result_not_scope` |
| Scope variablesReference wire (e.g., `11`) | Session Stopped, cache miss | Response: Query debugger for scope variables (normal routing, unchanged) | (covered by existing scope tests) |
| Invalid ref (0, negative, gap) | Any state | Response: `success=true, variables=[]` (out-of-range guard) | (covered by existing boundary tests) |

---

## §Hazards

**Subsystem:** DAP (Debug Adapter Protocol)  
**Hazard Class Reference:** [SUBSYSTEM_HAZARD_DEFAULTS.md](../../docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md) §DAP-1 through §DAP-4

| ID | Hazard Class | Surface | Risk | Mitigation | Test |
|----|--------------|---------|------|-----------|------|
| DAP-1 | **Silent Failure on Stale Ref** | `handle_variables()` at line 140-155 (new Child short-circuit check) | MEDIUM | Short-circuit explicit check prevents fall-through to wrong code path (no implicit empty response). Return honest-empty immediately. | `test_stale_child_ref_after_resume` — verifies response is `success=true, variables=[]` when cache miss occurs |
| DAP-2 | **Collision Between Ref Bands** | `VariableReference::decode(variables_ref)` at line 140 | LOW | Child band [2_000_000_000, i32::MAX] is disjoint from Scope [1, 999_999] and EvalResult [1_000_000, 1_999_999_999]. Decode is pure-range; no risk of misclassification. | `child_ref_wire_decodes_as_child_not_scope_or_eval` — verifies Child wires stay in correct band and decode correctly |
| DAP-3 | **Cache Invalidation Race** | `variable_cache.clear()` call on resume (not in this change, external) vs. `handle_variables()` check at line 140 | LOW | Child refs are checked AFTER cache lookup (line 102). If cache hit, early return before reaching line 140. If cache miss, short-circuit at line 140. No window for concurrent mutation. | Existing `dap_variable_reference_hardening_tests.rs` covers cache behavior; new test confirms short-circuit is only taken on miss |
| DAP-4 | **Scope Routing Bypass** | New short-circuit at line 140-155 | MEDIUM | Risk: Scope refs should NOT short-circuit; they must continue to scope routing (line 145+). Mitigation: Check is only for `VariableReference::Child { .. }`, never matches Scope refs (band-disjoint). | `test_scope_ref_continues_to_routing` (implied in existing tests) — verifies Scope refs continue through to scope routing, not short-circuited |
| DAP-5 | **Semantic Confusion: "No children" vs "Stale ref"** | Response shape at line 131-138 (honest-empty) | HIGH | Before fix: silent empty (client can't distinguish). After fix: explicit short-circuit + honest-empty (same response shape, but reached via correct code path). Client still receives `success=true, variables=[]` and interprets as "no children" (correct for stale ref). Mitigation: Mirrors #1338 pattern; semantic contract is established. | `test_stale_child_ref_after_resume` — verifies response shape is honest-empty |

**Cross-Subsystem Hazards:**

| ID | Hazard Class | Surface | Risk | Mitigation | Test |
|----|--------------|---------|------|-----------|------|
| CROSS-1 | **LSP Depends on DAP Correctness** | DAP protocol contract: `success=true, variables=[]` for stale refs | LOW | LSP does not depend on HOW the response is generated (code path), only the response shape. Short-circuit doesn't change response shape, only the path to it. | Existing LSP integration tests verify DAP protocol compliance; new test confirms shape is correct |
| CROSS-2 | **Variable Cache Lifecycle** | `variable_cache.clear()` in `session.rs` (external) | LOW | This change does not modify cache behavior. Cache is still cleared on resume; stale refs are still invalid. Fix only adds an explicit check for what was previously implicit. | Existing tests cover cache lifecycle; new test confirms check is triggered on cache miss |

---

## §Contracts

**Protocol Spec:** DAP (Debug Adapter Protocol) v1.70 — [https://microsoft.github.io/debug-adapter-protocol/specification](https://microsoft.github.io/debug-adapter-protocol/specification)

**Relevant sections:**
- **Variables Request** (§VariablesRequest): Specifies `variablesReference` as an i32 identifying a scope or variable container. A reference that is no longer valid (e.g., stale after resume) should return an empty variables list.
- **Variables Response** (§VariablesResponse): Success=true with empty variables array is a valid response for refs with no children.

**Internal Contracts:**

| Contract | Reference | Relevance | Verification |
|----------|-----------|-----------|--------------|
| **Stale-Ref Short-Circuit Pattern** | Issue #1338, PR merged | Establishes that stale refs after resume should short-circuit explicitly. Child refs were the gap. | `test_stale_child_ref_after_resume` confirms Child refs follow the #1338 pattern |
| **VariableReference Codec Bands** | `crates/perl-dap/src/debug_adapter/var_ref.rs` (lines 154-164) | Child band [2_000_000_000, i32::MAX] is disjoint from Scope and EvalResult. Pure-range decode is unambiguous. | `child_ref_wire_decodes_as_child_not_scope_or_eval` confirms band disjointness |
| **Variable Cache Lifecycle** | `crates/perl-dap/src/debug_adapter/session.rs` | Cache is cleared on every resume (continue/next/step). Any variablesReference from the previous stop becomes invalid. | Existing tests verify cache behavior; new test confirms short-circuit is triggered on cache miss |
| **Honest-Empty Response** | DAP spec + issue #1338 comment | A ref with no children (or stale ref) returns `success=true, variables=[], message=null`. "Honest" means no error or misleading output. | `test_stale_child_ref_after_resume` verifies response is honest-empty (success=true, empty array, no message) |

---

## §API-Shape

**API Surface:** Internal to `perl-dap` crate. No public API changes.

**Change Summary:**
- **New code**: 15 lines in `handle_variables()` (short-circuit check + comment)
- **Changed code**: 1 block comment (line 246-252) to note the gap is fixed
- **New types**: None
- **Removed types**: None
- **Modified types**: None
- **New functions**: None
- **Modified functions**: `DebugAdapter::handle_variables()` (internal refactor, no signature change)

**ID-Space Impact:** None. No new variablesReference IDs or bands. Child band [2_000_000_000, i32::MAX] already exists and is unchanged.

**Dup-Risk Grep:**
```bash
grep -n "stale.*Child\|Child.*cache" crates/perl-dap/src/debug_adapter/variables.rs
```
Expected: Only the new short-circuit block (line 140-155) and updated comment (line 246-252). No existing refs to "stale Child cache".

**Caller Count:** None. The change is a guard within a single handler function. No callers of `handle_variables()` are affected; they continue to receive the same response shape.

---

## §Test-Grid

**Test Table:** Maps each behavior row to a named test, invariant, and test type.

| Behavior Row | Test Name | Test Type | Invariant | Assertion |
|--------------|-----------|-----------|-----------|-----------|
| Stale Child ref, cache miss → honest-empty | `test_stale_child_ref_after_resume` | Positive / Acceptance | Child refs with cache miss return `success=true, variables=[]` immediately (short-circuit taken) | `is_honest_empty(&response)` returns true; no debugger query sent |
| Child ref band is disjoint | `child_ref_wire_decodes_as_child_not_scope_or_eval` | Positive / Codec Invariant | Child wires in [2_000_000_000, i32::MAX] decode as Child, never as Scope or EvalResult | `VariableReference::decode(wire)` returns `Some(Child { .. })` for all sample wires in Child band |
| Scope refs continue to routing | (implied, existing tests) | Positive / Regression | Scope refs are NOT short-circuited; they continue to scope routing and debugger query. New short-circuit only matches Child. | Existing scope-routing tests pass without modification |
| Running state prevents any query | (existing test in hardening suite) | Positive / Guard | Session in Running state returns honest-empty before reaching short-circuit (earlier guard at line 78-92). | `dap_variable_reference_hardening_tests.rs` covers this |
| Cache hit returns cached data | (existing cache-hit tests) | Positive / Regression | Child refs with cache hit return cached data, not honest-empty. Short-circuit is only for cache miss. | Existing cache-hit tests pass without modification |
| Invalid ref (0, negative, gap) | (existing boundary tests) | Negative / Guard | Out-of-range refs return honest-empty via earlier guard (line 57-67), before reaching line 140. | `dap_variable_reference_hardening_tests.rs` covers this |

---

## §Blast-Radius

**Scope:** Change is isolated to the DAP crate and does not affect other subsystems.

**Consumers:**
- **Direct consumer:** DAP protocol clients (VSCode Debugger, CLI tools) consuming `variables` responses. The response shape is unchanged; only the code path changes.
- **Indirect consumer:** LSP (perl-lsp crate) has integration tests with DAP; responses remain unchanged.

**Downstream crates:** None directly affected. The change is internal to `perl-dap`.

**Must-Not-Touch Boundary:**
- ✓ **Parser** (perl-parser, perl-lexer): Not affected. DAP does not trigger parser changes.
- ✓ **Lexer** (perl-lexer): Not affected.
- ✓ **Workspace** (perl-workspace): Not affected.
- ✓ **LSP** (perl-lsp-rs): Not affected. LSP integration tests continue to work with unchanged response shapes.
- ✓ **Protocol** (lsp-types, DAP spec): Not affected. No protocol changes.

**Integration Points:**
- `variable_cache.clear()` call on resume: External to this change; not modified. Stale refs are still invalid; this change only adds an explicit guard.
- `VariableReference::decode()`: Already imported and used correctly at line 140. No changes to codec.

**Risk Level:** **Very Low**
- Single short-circuit check in a single function.
- Only taken for a specific ref type (Child) with cache miss (narrow condition).
- Response shape unchanged; only code path changes.
- Mirrors established pattern from #1338 (low novelty risk).
- No API-surface changes; no type changes; no dependency changes.

**Regression Prevention:**
- Existing tests continue to pass (cache-hit tests, scope-routing tests, running-state guard tests).
- New tests verify the short-circuit is taken (positive assertion).
- No existing code paths modified (only added); pure addition.

---

## Summary

All six required sections completed:

1. **§Behavior** — Table of input/condition → expected result for stale Child refs
2. **§Hazards** — Five hazard rows (DAP-1 through DAP-5) plus cross-subsystem, all with mitigations and tests
3. **§Contracts** — DAP protocol spec compliance, VariableReference codec bands, cache lifecycle
4. **§API-Shape** — No public API changes; internal refactor only
5. **§Test-Grid** — Six test rows mapping behaviors to test names and invariants
6. **§Blast-Radius** — Very low: isolated to DAP crate, response shape unchanged, mirrors #1338 pattern

**Acceptance:** Issue #1857 is accepted when:
- [ ] Test `test_stale_child_ref_after_resume` passes (stale Child refs return honest-empty)
- [ ] Test `child_ref_wire_decodes_as_child_not_scope_or_eval` passes (codec band verification)
- [ ] All existing perl-dap tests pass (no regressions)
- [ ] Code comment updated to note gap is fixed
- [ ] `cargo xtask fmt` and `cargo clippy -p perl-dap` pass
