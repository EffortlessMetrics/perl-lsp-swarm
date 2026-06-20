# Implementation Checklist: #1654 — Fix scope-analyzer: state variables not distinguished from my

## Change order (compiles at each step)

### Step 1: Add `is_state` field to Variable struct
- **File:** `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs`
- **Change:** Add `is_state: bool` field to `Variable` struct (lines 107-113)
- **Details:** The struct currently has `is_our: bool` (line 111). Add `is_state: bool` as a new field:
  ```rust
  #[derive(Debug)]
  struct Variable {
      declaration_offset: usize,
      is_used: RefCell<bool>,
      is_our: bool,
      is_state: bool,           // NEW FIELD
      is_initialized: RefCell<bool>,
  }
  ```
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 2: Update `declare_variable_parts` method signature to accept and track `is_state`
- **File:** `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs`
- **Change:** Modify `declare_variable_parts` method (lines 182-224) to accept `is_state: bool` parameter and pass it to `Variable` construction
- **Details:** Update the signature and pass is_state to Variable construction
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 3: Update redeclaration logic in `declare_variable_parts` for state
- **File:** `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs`
- **Change:** Modify `declare_variable_parts` method to forbid state redeclaration
- **Details:** Update the redeclaration check logic to handle `state` differently from `our`
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 4: Update `declare_variable_parts_in_context` to pass `is_state` parameter
- **File:** `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs`
- **Change:** Modify `declare_variable_parts_in_context` (lines 507-528) signature and implementation
- **Details:** Add `is_state: bool` parameter and pass it through to `declare_variable_parts`
- **Depends on:** Step 2
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 5: Update callers of `declare_variable_parts_in_context` in `handle_variable_declaration`
- **File:** `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/declarations.rs`
- **Change:** Modify `handle_variable_declaration` function (lines 14-96) to pass `is_state` flag
- **Details:** Extract `is_state` from declarator and update the call
- **Depends on:** Step 4
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 6: Update callers of `declare_variable_parts_in_context` in `handle_variable_list_declaration`
- **File:** `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/declarations.rs`
- **Change:** Modify `handle_variable_list_declaration` function (lines 99-157) to pass `is_state` flag
- **Details:** Similar to Step 5, extract `is_state` from declarator
- **Depends on:** Step 4
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 7: Update callers of `declare_variable_parts_in_context` in `handle_use`
- **File:** `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/declarations.rs`
- **Change:** Modify `handle_use` function (lines 159-211) calls to `declare_variable_parts_in_context`
- **Details:** Pass `is_state: false` for use vars (which declares package globals, never state)
- **Depends on:** Step 4
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 8: Add regression tests for state redeclaration
- **File:** `crates/perl-semantic-analyzer/tests/scope_and_symbol_tests.rs`
- **Change:** Add new test functions
- **Details:** Add three new test functions for state redeclaration behavior
- **Verify:** `cargo test -p perl-semantic-analyzer --lib scope_and_symbol_tests`

### Step 9: Final verification and formatting
- **Verify:** 
  - `cargo test -p perl-semantic-analyzer` (all tests pass)
  - `cargo xtask fmt` (format code)
  - `cargo clippy -p perl-semantic-analyzer` (no warnings)

## Callers and consumers

- `declare_variable_parts_in_context`: Called from handle_variable_declaration, handle_variable_list_declaration, handle_use
- `declare_variable_parts`: Called from declare_variable_parts_in_context
- `Variable` struct: Used in Scope operations

## Scope boundary

Files IN scope:
- `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs`
- `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/declarations.rs`
- `crates/perl-semantic-analyzer/tests/scope_and_symbol_tests.rs`

Files OUT of scope:
- Parser, AST, DAP, LSP, other semantic analyzer modules

## Flags for builder

1. State redeclaration must always error (unlike our which accepts); my also errors (no change)
2. State scope is block-scoped like my (research verified this; no scope-handling changes needed)
3. No AST changes required; declarator string already distinguishes state
