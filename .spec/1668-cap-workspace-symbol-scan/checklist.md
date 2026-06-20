# Implementation Checklist: #1668 — cap O(n) workspace/symbol scan

**Quick summary:** Add `cap` parameter to `search_source_symbols()` and `search_generated_workspace_symbols()` with early exit when cap is reached. Apply the cap at the search boundary, not after collecting all results.

## Change order (compiles at each step)

### Step 1: Add cap parameter to search_source_symbols
- **File:** `crates/perl-workspace/src/workspace/workspace_index.rs`
- **Change:** Modify function signature and implementation to accept and apply cap
- **Details:**
  - Line 2917: Change `pub fn search_source_symbols(&self, query: &str) -> Vec<WorkspaceSymbol>`
  - To: `pub fn search_source_symbols(&self, query: &str, cap: Option<usize>) -> Vec<WorkspaceSymbol>`
  - Inside function (lines 2917-2936): Add early-exit logic after symbol is pushed to `results`:
    - Check `if cap.is_some() && results.len() >= cap.unwrap() { break; }` before continuing to next symbol
    - Requires converting the nested loops to a labeled break pattern or early return
  - Update tests at lines 4710, 4715, 4779 to pass `None` as cap (backward-compatible default)
- **Verify:** `cargo check -p perl-workspace`

### Step 2: Add cap parameter to search_generated_workspace_symbols
- **File:** `crates/perl-workspace/src/workspace/workspace_index.rs`
- **Change:** Modify function signature and implementation to accept and apply cap
- **Details:**
  - Line 2943: Change `pub fn search_generated_workspace_symbols(&self, query: &str) -> Vec<WorkspaceSymbol>`
  - To: `pub fn search_generated_workspace_symbols(&self, query: &str, cap: Option<usize>) -> Vec<WorkspaceSymbol>`
  - Inside function (lines 2954-2997): Add early-exit logic after symbol is pushed to `results`:
    - Check `if cap.is_some() && results.len() >= cap.unwrap() { break; }` in the outer loop before continuing
  - Update test at line 4721 to pass `None` as cap (backward-compatible default)
- **Depends on:** Step 1 (to establish pattern)
- **Verify:** `cargo check -p perl-workspace`

### Step 3: Update call site 1 in workspace.rs (full index path)
- **File:** `crates/perl-lsp-rs/src/runtime/workspace.rs`
- **Change:** Pass cap to both search functions
- **Details:**
  - Line 290: Change `let mut symbols = coordinator.index().search_source_symbols(query);`
  - To: `let mut symbols = coordinator.index().search_source_symbols(query, Some(cap));`
  - Line 291: Change `symbols.extend(coordinator.index().search_generated_workspace_symbols(query));`
  - To: `symbols.extend(coordinator.index().search_generated_workspace_symbols(query, Some(cap)));`
  - Remove the `.take(cap)` on line 296 since capping now happens at search boundary
  - Note: Update comment on line 293-294 to reflect early-exit strategy instead of post-collection capping
- **Depends on:** Steps 1-2 (function signatures updated)
- **Verify:** `cargo check -p perl-lsp-rs`

### Step 4: Update call site 2 in workspace.rs (partial index path)
- **File:** `crates/perl-lsp-rs/src/runtime/workspace.rs`
- **Change:** Pass cap to search function
- **Details:**
  - Line 337: Change `let symbols = coordinator.index().search_source_symbols(query);`
  - To: `let symbols = coordinator.index().search_source_symbols(query, Some(cap));`
  - Remove the `.take(cap)` on line 340 since capping now happens at search boundary
  - Note: No generated symbols search in this path (check line 337-338 context)
- **Depends on:** Steps 1-2
- **Verify:** `cargo check -p perl-lsp-rs`

### Step 5: Update call site 3 in workspace.rs (tracing/counting path)
- **File:** `crates/perl-lsp-rs/src/runtime/workspace.rs`
- **Change:** Pass cap to search function for consistency
- **Details:**
  - Line 2290: Change `coordinator.index().search_source_symbols(query).iter().take(workspace_symbol_cap()).count()`
  - To: `coordinator.index().search_source_symbols(query, Some(workspace_symbol_cap())).len()`
  - This eliminates the unnecessary `.iter().take().count()` pattern when early-exit is now in place
- **Depends on:** Steps 1-2
- **Verify:** `cargo check -p perl-lsp-rs`

### Step 6: Update call site in signature_help.rs (method lookup path)
- **File:** `crates/perl-lsp-rs/src/runtime/language/hover/signature_help.rs`
- **Change:** Pass None to search function (no cap needed for method lookup)
- **Details:**
  - Line 890: Change `let candidates = workspace_index.search_source_symbols(method_name);`
  - To: `let candidates = workspace_index.search_source_symbols(method_name, None);`
  - Rationale: Signature help needs the full candidate list to find exact matches by name, so no cap is appropriate
- **Depends on:** Steps 1-2
- **Verify:** `cargo check -p perl-lsp-rs`

### Step 7: Final verification
- **Verify:** `cargo test -p perl-workspace --lib && cargo test -p perl-lsp-rs --lib && cargo xtask fmt && cargo clippy -p perl-workspace -p perl-lsp-rs`

## Callers and consumers

| Function | Called from | Call sites |
|---|---|---|
| `search_source_symbols` | 5 locations | workspace.rs (lines 290, 337, 2290), signature_help.rs (line 890), workspace_index.rs (line 2909) |
| `search_generated_workspace_symbols` | 2 locations | workspace.rs (line 291), workspace_index.rs (line 2909) |

## Scope boundary

**Files IN scope:**
- `crates/perl-workspace/src/workspace/workspace_index.rs` — function signatures and implementations
- `crates/perl-lsp-rs/src/runtime/workspace.rs` — call sites in workspace/symbol handler
- `crates/perl-lsp-rs/src/runtime/language/hover/signature_help.rs` — method signature lookup call site

**Files OUT of scope:**
- `crates/perl-workspace/src/workspace/document_store.rs` — unrelated document management
- `crates/perl-lsp-rs-core/src/runtime/limits/mod.rs` — workspace_symbol_cap() function definition (read-only, no changes)
- Test files in other crates
- Protocol/LSP type definitions

## Flags for builder

1. **Loop structure:** The current nested-loop in `search_source_symbols` and `search_generated_workspace_symbols` may need refactoring to support early exit. Consider:
   - Using a labeled loop with `break 'outer` when cap is reached, or
   - Extracting early-exit logic into a helper function, or
   - Accepting that outer loop completes even if cap is reached (acceptable since we check on each push)
   
2. **Option vs direct usize:** Using `Option<usize>` for cap allows callers to pass `None` when no cap is desired (signature_help). Alternative: use `usize::MAX` as sentinel. Current design is more explicit.

3. **Backward compatibility:** The signature change is not breaking at crate level since `search_source_symbols` and `search_generated_workspace_symbols` are public but not part of a stable public API (they're workspace internals). However, any external consumers will need to update their call sites.

4. **Performance assumption:** Builder should verify in tests that:
   - With cap=200 and 1000 matching symbols, at most 200 are cloned
   - Early exit prevents full scan even for broad queries
   - Benchmark shows latency improvement on large workspaces
