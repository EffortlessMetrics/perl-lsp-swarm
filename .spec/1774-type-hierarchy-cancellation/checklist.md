# Implementation Checklist: Type Hierarchy Cancellation Support

## Overview
Add cancellation support to `typeHierarchy/prepare`, `typeHierarchy/supertypes`, and `typeHierarchy/subtypes` LSP operations. These operations can be expensive when traversing package hierarchies in large workspaces, but users cannot abort them if they become unresponsive.

## Change Order

### Phase 1: Register cancellation support (3 LOC)

**1. Add three type hierarchy methods to cancellation match pattern**
- **File:** `crates/perl-lsp-rs/src/runtime/dispatch/request_cancellation.rs` (lines 46-60)
- **What changes:** Add three match arms to the `matches!()` expression in `register_request_cancellation()`
- **Current state:** 12 methods matched (completion, hover, definition, references, documentSymbol, codeAction, formatting, rename, workspace/symbol, callHierarchy/incomingCalls, callHierarchy/outgoingCalls, textDocument/inlayHint)
- **New state:** Add after line 59 (after inlayHint, before closing paren):
  - `"typeHierarchy/prepare"`
  - `"typeHierarchy/supertypes"`
  - `"typeHierarchy/subtypes"`
- **Compilation check:** `cargo build -p perl-lsp-rs --lib 2>&1 | grep -E "error|warning"`
- **Verify command:** `cargo clippy -p perl-lsp-rs --lib -- -D warnings`

### Phase 2: Wrap handlers with route_cancellable (3 LOC)

**2. Replace direct dispatch with route_cancellable wrapper for type hierarchy handlers**
- **File:** `crates/perl-lsp-rs/src/runtime/dispatch/routing.rs` (lines 75-87)
- **What changes:** Replace each handler invocation with `route_cancellable()` wrapper (same pattern as callHierarchy at lines 154-162)
- **Current state (3 direct calls):**
  ```rust
  "textDocument/prepareTypeHierarchy" => {
      self.handle_prepare_type_hierarchy_dispatch(request.params)
  }
  "typeHierarchy/prepare" => {
      // Alias for deprecated/alternate method string
      self.handle_prepare_type_hierarchy_dispatch(request.params)
  }
  "typeHierarchy/supertypes" => {
      self.handle_type_hierarchy_supertypes_dispatch(request.params)
  }
  "typeHierarchy/subtypes" => {
      self.handle_type_hierarchy_subtypes_dispatch(request.params)
  }
  ```
- **New state:** Wrap each in `route_cancellable()` (4 lines per method):
  ```rust
  "textDocument/prepareTypeHierarchy" => {
      return self.route_cancellable(id, method, should_respond, |_| {
          self.handle_prepare_type_hierarchy_dispatch(request.params)
      });
  }
  "typeHierarchy/prepare" => {
      return self.route_cancellable(id, method, should_respond, |_| {
          self.handle_prepare_type_hierarchy_dispatch(request.params)
      });
  }
  "typeHierarchy/supertypes" => {
      return self.route_cancellable(id, method, should_respond, |_| {
          self.handle_type_hierarchy_supertypes_dispatch(request.params)
      });
  }
  "typeHierarchy/subtypes" => {
      return self.route_cancellable(id, method, should_respond, |_| {
          self.handle_type_hierarchy_subtypes_dispatch(request.params)
      });
  }
  ```
- **Compilation check:** `cargo build -p perl-lsp-rs --lib 2>&1 | grep -E "error|warning"`
- **Verify command:** `cargo clippy -p perl-lsp-rs --lib -- -D warnings`

### Phase 3: Write cancellation tests

**3. Add test cases for type hierarchy cancellation**
- **File:** `crates/perl-lsp-rs/tests/lsp_cancellation_protocol_tests.rs`
- **What changes:** Add three test cases (one per method) validating cancellation behavior
- **Test location:** Add after existing test functions (end of file before closing brace)
- **Test structure:** Each test should:
  1. Send a type hierarchy request (prepare, supertypes, or subtypes)
  2. Send a `$/cancelRequest` notification with the request ID before the handler completes
  3. Verify the response is a `-32800 RequestCancelled` error
- **Test names:**
  - `test_type_hierarchy_prepare_cancellation()`
  - `test_type_hierarchy_supertypes_cancellation()`
  - `test_type_hierarchy_subtypes_cancellation()`
- **Verify command:** `RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs test_type_hierarchy_ -- --test-threads=2 2>&1`

## Dependency Graph

1. Phase 1 must complete before Phase 2 (cannot use route_cancellable without registering the methods)
2. Phase 2 must complete before Phase 3 (tests verify cancellation works)
3. Phases 1 & 2 compile independently

## Expected Test Results

After all phases complete:
- `cargo test -p perl-lsp-rs test_type_hierarchy_prepare_cancellation` → PASS
- `cargo test -p perl-lsp-rs test_type_hierarchy_supertypes_cancellation` → PASS
- `cargo test -p perl-lsp-rs test_type_hierarchy_subtypes_cancellation` → PASS
- `cargo clippy -p perl-lsp-rs --lib -- -D warnings` → PASS
- `cargo clippy -p perl-lsp-rs --tests -- -D warnings` → PASS

## Scope Boundaries

**In scope:**
- Add cancellation support to the three type hierarchy operations
- Follow the existing callHierarchy pattern exactly
- Write tests validating cancellation response

**Out of scope:**
- workspace/executeCommand cancellation (separate issue, mentioned as future work)
- Generalizing all operations to be cancellable by default (separate policy decision)
- Type hierarchy provider internal optimizations (not required for cancellation support)

## Builder Notes

1. Red-TDD builder: Write failing tests first in lsp_cancellation_protocol_tests.rs
   - Tests should verify -32800 error code when cancellation is sent
   - Use the existing CancellationTestFixture pattern
   - Focus on positive case: cancellation works as expected

2. Builder: Implement the three phases in order
   - Start with Phase 1 (register methods) - minimal, low risk
   - Add Phase 2 (wrap handlers) - mirrors callHierarchy
   - Run existing tests to ensure no regression

3. Verify at each phase:
   - Compilation succeeds
   - Clippy passes (no warnings)
   - Workspace tests still pass

## Key Files

| File | Lines | Change | Risk |
|------|-------|--------|------|
| request_cancellation.rs | 46-60 | Add 3 match arms | LOW |
| routing.rs | 75-87 | Wrap 4 handlers | LOW |
| lsp_cancellation_protocol_tests.rs | EOL | Add 3 tests | LOW |

Total new code: ~50 LOC (pattern additions + test cases)
Total modified: ~15 LOC (three match additions + wrap changes)
