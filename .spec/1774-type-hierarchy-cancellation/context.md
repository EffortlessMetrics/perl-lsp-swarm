# Context: Type Hierarchy Cancellation Support

## Problem Statement

Type hierarchy operations (`textDocument/typeHierarchy/prepare`, `typeHierarchy/supertypes`, `typeHierarchy/subtypes`) can be expensive in large workspaces because they traverse package inheritance hierarchies using BTreeMap-based lookups across the AST. However, these operations are not registered for LSP cancellation support, which means:

1. Users cannot abort slow type hierarchy operations via the `$/cancelRequest` LSP protocol mechanism
2. This creates perceived UI hangs in VSCode and other LSP clients
3. Inconsistency with similar operations: `callHierarchy/incomingCalls` and `callHierarchy/outgoingCalls` ARE cancellable (same UX pattern, both hierarchical traversals)

The fix is straightforward: add the three type hierarchy methods to the cancellation whitelist (request_cancellation.rs) and wrap their dispatch handlers with the existing `route_cancellable()` mechanism (routing.rs).

## Decisions Made

### Decision 1: Follow the callHierarchy Pattern (Chosen)
Type hierarchy operations mirror call hierarchy operations:
- Both are hierarchical traversals of a namespace (packages vs. functions)
- Both can be expensive in large codebases
- CallHierarchy is already cancellable (lines 57-58, 154-162)
- Solution: Apply the same cancellation pattern to type hierarchy

**Alternatives rejected:**
- **Alt 1 (generalize all operations):** Would require validating all 50+ LSP methods for cancellation-safety; out of scope for this issue
- **Alt 2 (focus on workspace/executeCommand only):** workspace/executeCommand has unbounded complexity but is a separate concern; type hierarchy should be added first (proven pattern)

### Decision 2: Phase Approach (Chosen)
1. Phase 1: Add methods to cancellation whitelist (request_cancellation.rs) — minimal, low risk
2. Phase 2: Wrap handlers with route_cancellable (routing.rs) — mirrors callHierarchy exactly
3. Phase 3: Write tests — validates both phases work together

This ordering ensures each phase compiles independently and tests exercise the full stack.

**Alternatives rejected:**
- Combine all into one phase — makes debugging harder if tests fail
- Implement tests first — tests would fail for missing handler wraps, not capturing registration check

### Decision 3: Don't Modify Provider Methods (Chosen)
The type hierarchy provider methods (`prepare()`, `find_supertypes()`, `find_subtypes()`) do NOT need cancellation token polling internally. The routing layer's `route_cancellable()` checks for cancellation BEFORE calling the handler, so:
- If request is cancelled before routing, no handler call happens (RequestCancelled response)
- If request is not cancelled before routing, handler runs to completion normally

This is different from completion, which needs internal polling during long traversals. Type hierarchy operations are shorter and complete quickly enough for the routing-level check to be sufficient.

**Alternatives rejected:**
- Add cancellation token polling inside provider methods — unnecessary complexity; routing-level check is sufficient for type hierarchy's scope
- Make provider methods async-cancellable — would require major refactoring; routing pattern is proven sufficient

### Decision 4: Test Strategy (Chosen)
Write three cancellation tests (one per method) in lsp_cancellation_protocol_tests.rs using the existing CancellationTestFixture pattern:
1. Send a type hierarchy request with a specific request ID
2. Send `$/cancelRequest` with that ID before the handler completes
3. Verify response is -32800 RequestCancelled error

This validates both the routing wrapper and the cancellation registry registration.

**Alternatives rejected:**
- Unit test only the match arms — wouldn't verify end-to-end protocol behavior
- Integration test without fixtures — existing test utilities (CancellationTestFixture) should be reused

## Prior Art and References

**Cancellation support in perl-lsp:**
- `crates/perl-lsp-rs/src/runtime/dispatch/request_cancellation.rs` — cancellation token registry and registration logic (added ~2024)
- `crates/perl-lsp-rs/src/runtime/dispatch/routing.rs` — route_cancellable wrapper for dispatch routing (lines 211-232)
- `crates/perl-lsp-rs/tests/lsp_cancellation_protocol_tests.rs` — existing test patterns for cancellation validation

**callHierarchy pattern (proven model):**
- routing.rs lines 154-162: `callHierarchy/incomingCalls` and `callHierarchy/outgoingCalls` wrapped with `route_cancellable()`
- request_cancellation.rs lines 57-58: both methods in needs_cancellation match
- Similar use case: both are hierarchical traversals that can be expensive

**LSP Specification:**
- [LSP 3.17 Type Hierarchy](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#typeHierarchy)
- [LSP 3.17 Cancellation](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#requestCancellation)
- Error code -32800 (RequestCancelled) is LSP standard

**Related issues:**
- #1663 — LSP 3.17/3.18 spec-compliance roadmap (type hierarchy was added in 3.17, already routed in perl-lsp)
- #799 — typeHierarchyItem/resolve (related feature, separate from cancellation)

## Issue Pipeline Summary

1. **Scout (swarm-discovered):** Found inconsistency — type hierarchy routed but not cancellable, while callHierarchy is both
2. **Accuracy-scout (accuracy-reviewed):** Verified code facts:
   - Type hierarchy methods routed in routing.rs (lines 75-87) ✓
   - Type hierarchy NOT in request_cancellation.rs match (lines 46-60) ✓
   - CallHierarchy IS in both places (proven pattern) ✓
3. **Research-verifier (research-reviewed):** Confirmed LSP spec:
   - typeHierarchy operations are LSP 3.17 standard ✓
   - All LSP requests support cancellation via $/cancelRequest ✓
   - RequestCancelled error code is -32800 ✓
4. **Plan-reviewer:** Spec ready for building:
   - Clear, minimal scope: 3 methods, 2 files, ~50 LOC
   - Conservative approach (mirrors callHierarchy)
   - Low blast radius (internal dispatch only, no API changes)

## Test Coverage Map

The acceptance.md §Test-Grid maps each behavior row and hazard mitigation to a specific test:
- **Positive tests** (normal operation): test_type_hierarchy_prepare/supertypes/subtypes_normal
- **Negative tests** (cancellation works): test_type_hierarchy_prepare/supertypes/subtypes_cancellation
- **Adversarial tests** (concurrent isolation): test_type_hierarchy_concurrent_cancellation_isolation
- **Invariant tests** (error code, registry state): test_type_hierarchy_cancellation_error_code_invariant, test_type_hierarchy_cancellation_registry_registration
- **Regression tests** (non-type-hierarchy routes unchanged): test_regression_non_type_hierarchy_routes_unchanged

## Builder Handoff Notes

1. **Red-TDD builder:** Write the 7-8 cancellation tests first in lsp_cancellation_protocol_tests.rs
   - Use CancellationTestFixture as the pattern
   - Focus on positive case (cancellation works) and concurrent isolation
   - Let tests fail initially (red phase)

2. **Builder:** Implement in order:
   - Phase 1: Add 3 method names to request_cancellation.rs match (3 LOC)
   - Phase 2: Wrap 4 handlers in routing.rs with route_cancellable (12 LOC)
   - Run tests after each phase
   - Verify clippy/fmt pass

3. **Verification at each step:**
   - Compilation succeeds
   - No new clippy warnings
   - Existing tests still pass
   - New cancellation tests turn green

## Unresolved / Out of Scope

1. **workspace/executeCommand cancellation:** Mentioned in the issue as having "unbounded complexity." This should be a separate issue with performance analysis first.
2. **Generalize all operations to cancellable by default:** Would be a broader refactoring; this issue stays conservative (whitelist only known-expensive operations).
3. **Type hierarchy provider optimizations:** Not required for cancellation support; the routing-level cancellation check is sufficient.
