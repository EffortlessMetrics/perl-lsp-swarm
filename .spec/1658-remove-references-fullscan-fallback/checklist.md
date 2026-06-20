# Implementation Checklist: #1658 — perf(lsp): references request spawns full-workspace text scan at request time

## Change order (compiles at each step)

### Step 1: Remove enhanced fallback scan in `IndexAccessMode::Full` (Scan #1, lines 279–340)
- **File:** `crates/perl-lsp-rs/src/runtime/language/references.rs`
- **Change:** Delete the enhanced fallback text-search loop that creates `docs_snapshot` (lines 282–340) and searches all documents for both symbol and package-qualified references.
- **Details:** 
  - Delete lines 279–340 (comment through end of pattern loop)
  - Specifically remove:
    - `let docs_snapshot: Vec<(String, String)> = documents.iter().map(|(k, v)| (k.clone(), v.text.clone())).collect();` (lines 282–285)
    - The `enhanced_locations` vector and associated pattern loop (lines 287–340)
    - The `.extend(enhanced_locations)` on line 343
  - Keep the index-backed results (lines 245–268) and the `find_references()` fallback (lines 358–386)
  - The deadline check (lines 271–277) becomes redundant after this step and should also be removed
- **Verify:** `cargo check -p perl-lsp-rs`

### Step 2: Remove second full-workspace text scan (Scan #2, lines 453–523)
- **File:** `crates/perl-lsp-rs/src/runtime/language/references.rs`
- **Change:** Delete the full-document text scan for qualified name references that occurs after the qualified name regex match (lines 453–523).
- **Details:**
  - Delete lines 453–523 (the entire fallback text-scan block for qualified names)
  - Specifically remove:
    - `let docs_snapshot: Vec<(String, String)> = documents.iter().map(|(k, v)| (k.clone(), v.text.clone())).collect();` (lines 455–461)
    - The `'doc_scan` loop and regex search (lines 475–517)
    - The result return (lines 519–523)
  - Keep the index-backed lookups for the qualified symbol (lines 419–451)
  - The deadline check (line 479) is also part of this scan and should be removed
- **Verify:** `cargo check -p perl-lsp-rs`

### Step 3: Simplify the qualified-name path flow
- **File:** `crates/perl-lsp-rs/src/runtime/language/references.rs`
- **Change:** After deleting the fallback text scan, the code after the qualified-name index lookups (lines 451+) should cleanly flow to the next pattern iteration or outer block closure. Ensure the control flow is unambiguous.
- **Details:**
  - After deleting Scan #2, lines 419–451 perform index lookups for the qualified symbol
  - Lines 452+ currently include the text-scan fallback; after deletion, ensure the block properly closes
  - The qualified-name regex loop should continue to the next iteration or properly exit
  - No new code is added; this is structural cleanup
- **Verify:** `cargo check -p perl-lsp-rs`

### Step 4: Verify same-file fallback remains intact
- **File:** `crates/perl-lsp-rs/src/runtime/language/references.rs`
- **Change:** No changes needed. Verify that the same-file semantic analyzer fallback (lines 590–627) is still reachable and functional.
- **Details:**
  - Lines 590–627: `SemanticAnalyzer::analyze(ast)` and `find_all_references()` fallback
  - This path is used when:
    - `IndexAccessMode::Partial` (line 531) fails to find results
    - `IndexAccessMode::None` (line 584) has no index
  - Confirm this path is not affected by steps 1–3
- **Verify:** `cargo check -p perl-lsp-rs`

### Step 5: Write failing tests
- **File:** `crates/perl-lsp-rs/tests/integration_references.rs` (CREATE if not present)
- **Change:** Create tests that verify `handle_references_inner` with `IndexAccessMode::Full` does NOT perform full-workspace document iteration for text scanning.
- **Details:**
  - Test 1: `test_references_full_index_mode_no_document_iteration`
    - Mock workspace index with known reference results
    - Call `handle_references_inner` with a symbol that matches
    - Assert that `documents.iter()` is never called for the enhanced fallback scan
    - Verify index results are returned
  - Test 2: `test_references_full_index_mode_respects_deadline`
    - Verify that the deadline check (if any remains) only applies to index operations, not fallback scans
  - Test 3: `test_references_partial_mode_uses_same_file_fallback`
    - Verify `IndexAccessMode::Partial` still uses the open-document fallback
  - Tests should use `perl_tdd_support::must` for assertions
- **Verify:** `cargo test -p perl-lsp-rs -- --test-threads=2`

### Step 6: Run full verification suite
- **File:** N/A (verification only)
- **Verify:** 
  - `cargo test -p perl-lsp-rs -- --test-threads=2` — all tests pass
  - `cargo xtask fmt` — formatting clean
  - `cargo clippy -p perl-lsp-rs` — no clippy warnings

## Callers and consumers

- `handle_references_inner()` is called from `handle_references()` (line 176)
- `handle_references()` is called from `on_references_document_highlight()` (line 706)
- Both functions are LSP protocol handlers (no external callers outside the LSP server binary)
- `search_document_texts_for_references()` is called from:
  - Deprecated fallback in references.rs (to be removed in steps 1–2)
  - `on_references()` legacy handler (line 914, which does perform a full scan as documented behavior for fallback mode)

## Scope boundary

**Files IN scope:**
- `crates/perl-lsp-rs/src/runtime/language/references.rs` — the only file modified

**Files OUT of scope:**
- All other LSP handler files
- `crates/perl-workspace/` — index remains unchanged
- `crates/perl-semantic-analyzer/` — same-file analysis unchanged
- Test files (created, not modified existing code)

## Flags for builder

1. **Deletion is the primary action.** After step 1, the builder should verify that no reference-finding test in the suite starts failing (which would indicate the enhanced fallback was filling gaps in the index). If tests fail, report to the issue — this indicates index gaps and should be filed as a follow-up.

2. **Deadline checks are removed as part of the fallback.** The deadline in lines 271–277 and 479 exist only to cap the fallback text-search. After step 1–2, verify no deadline checks remain in the critical path.

3. **The `on_references()` legacy handler is separate.** It intentionally performs full-document text scans (line 912: `self.iter_open_buffers()`). This is NOT part of the scope — it's a deprecated handler with known performance characteristics. Do NOT modify it.

4. **Test coverage must demonstrate index-backed flow.** After steps 1–2, the only paths through `IndexAccessMode::Full` are:
   - `live_source_backed_reference_locations()` (lines 224–240) — live compiler facts
   - Index `find_refs()` (line 245) → convert to LSP locations (lines 261–267)
   - Index `find_references()` (line 367) → convert to LSP locations (lines 379–384)
   - Qualified-name regex path (lines 397–529) with index lookup (lines 419–451) only, no fallback scan
   - Fallthrough to same-file analyzer (lines 590–627)

5. **Verify the IndexAccessMode::Full branch closes correctly.** After deletion of lines 453–523 (Scan #2), ensure the match arm for `IndexAccessMode::Full` properly exits. The block should end at a clear closing brace around line 530 (original line 530 will shift after steps 1–2).
