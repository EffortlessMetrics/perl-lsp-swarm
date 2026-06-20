# Context: Fix rename dedup() only removing consecutive duplicates

## Problem Statement

The `rename()` and `scoped_rename()` methods in `crates/perl-lsp-rs-core/src/providers/rename/mod.rs` attempt to deduplicate TextEdit results using `Vec::dedup()` after a partial sort by `location.start` only. However, `Vec::dedup()` only removes **consecutive** equal elements.

When a symbol appears in both the symbol table's `symbols` table (declaration) and `references` table (usage including self-reference), two TextEdit objects with identical `location` and `new_text` fields are pushed to the vec. After sorting by start offset alone, these identical edits may not be adjacent (e.g., interspersed with other edits at different start offsets, or appearing in a different order due to unstable sort stability). As a result, `dedup()` fails to remove them, leaving duplicates in the final result.

**Example**:
```perl
my $x = $x + 1;
```
Here, `$x` appears as:
1. A declaration (in `symbol_table.symbols`) at some offset
2. A reference (in `symbol_table.references`) at the same or nearly-same offset

If both are added to the edits vec with identical start/end/new_text, sorting by start alone may not order them consecutively. `dedup()` then leaves both, and the LSP client applies the rename twice at the same location.

## Root Cause Analysis

1. **Source of duplicate edits**: Lines 168-177 (symbols table loop) and 179-188 (references table loop) independently push TextEdit objects. The same symbol may satisfy both conditions, resulting in two identical TextEdits.

2. **Why dedup() fails**: 
   - `Vec::dedup()` uses equality (PartialEq) to compare consecutive elements
   - Sort by `|edit| edit.location.start` is partial — it only orders by the start offset
   - Two edits with identical (start, end, new_text) but sorted in any order are not necessarily adjacent
   - Example: if edit A and B have the same start but are added in either order, and C has a different start, the sorted order could be [A, C, B] or [B, C, A], leaving duplicates

3. **Why this happens now**: The symbol table extraction process correctly identifies symbols in both tables (a symbol declaration and its uses including self-references). The rename logic simply doesn't account for this overlap.

## Design Decisions

### Option A: Full sort + dedup (Chosen)
- Add `Ord` and `PartialOrd` derives to `TextEdit`
- Replace `sort_by_key(|edit| edit.location.start)` with `sort()`
- Keep `dedup()` unchanged
- **Pros**: Minimal change, idiomatic Rust (use auto-derives), deterministic output, handles all cases
- **Cons**: None significant; full sort is negligible overhead for typical rename edits (<100 items)

### Option B: HashSet dedup
- Collect unique edits into a HashSet during processing or post-collection
- **Pros**: Explicit deduplication by (location, new_text) key
- **Cons**: Requires Hash impl on TextEdit, hash overhead, loses sort order (would need re-sort), more verbose code

### Option C: Location-only dedup
- Deduplicate by location alone, discard duplicates at the same byte span
- **Pros**: Slightly faster (less comparison)
- **Cons**: Loses semantic meaning if the same location could have different new_text values (unlikely but violates principle of least surprise)

### Decision Rationale
**Option A** is chosen because:
1. TextEdit already has PartialEq/Eq; adding PartialOrd/Ord is automatic and zero-cost
2. Full sort ensures correctness: all identical edits are adjacent, dedup is complete
3. Minimal code change: 2 lines per method
4. Idiomatic Rust: Ord is the standard way to make a type fully orderable
5. No performance concerns: typical rename has <100 edits, sort overhead is microseconds
6. Deterministic: full sort produces stable, predictable output

## Alternatives Rejected

1. **Manual deduplication loop**: 
   - Would require explicit HashMap or HashSet with (start, end, new_text) key
   - More lines of code, less idiomatic
   - Rejected in favor of leveraging Ord + sort + dedup

2. **Use BTreeSet**:
   - Could collect edits into BTreeSet<TextEdit> to auto-deduplicate during insertion
   - Would require TextEdit: Ord
   - Loses insertion order; would need re-sort at the end
   - More complex than sort + dedup
   - Rejected in favor of simpler Vec approach

3. **Filter duplicates after sort**:
   - Use `edits.iter().collect::<HashSet<_>>()` or similar
   - Similar complexity to Option B above
   - Rejected in favor of native dedup

## Prior Art and Contracts

**Perl Semantics**:
- A lexical variable declaration in Perl (e.g., `my $x`) can be referenced in the same statement or expression (e.g., `my $x = $x + 1` is valid, though unusual).
- The semantics are: declaration first, then reference in the RHS. However, the parser may index both in the same table entry or in separate entries depending on implementation.
- No contract change: the symbol table generation is correct; rename just needs to handle overlaps.

**LSP Protocol (textDocument/rename)**:
- RFC 3040: https://microsoft.github.io/language-server-protocol/specifications/lsp/3.x/specification/#textDocument_rename
- Output: `WorkspaceEdit` contains `changes: Map<DocumentUri, TextEdit[]>`
- Expectation: TextEdit array should not contain duplicates at the same location
- This fix ensures compliance

**Rust std library**:
- `Vec::dedup()` is documented to remove only consecutive duplicates
- To remove all duplicates, either sort fully (all equals adjacent) or use HashSet
- This fix aligns with documented best practices

## Testing Strategy

1. **Unit test (red-TDD)**:
   - Add test `test_rename_no_duplicate_edits_for_shared_locations` that constructs code with self-referential symbol (e.g., `my $x = $x + 1`)
   - Call `rename()` and count occurrences per location
   - Assert all counts are 1 (no duplicates)
   - This test should fail before the fix, pass after

2. **Regression tests**:
   - Run all existing rename tests to ensure no breaking changes
   - Verify scoped_rename tests still pass (same fix applied to both methods)

3. **Edge cases**:
   - Empty edits vec (no symbol found) — no change needed
   - Single edit — dedup is no-op (correct)
   - Many identical edits from overlapping tables — full test coverage

## Links and References

- **Issue**: #1863 (this issue)
- **Issue URL**: https://github.com/perl-lsp/perl-lsp/issues/1863
- **Code location**: 
  - `crates/perl-lsp-rs-core/src/providers/rename/mod.rs` (lines 196-197, 284-285)
  - `crates/perl-lsp-rs-core/src/providers/rename/types.rs` (line 8)
- **Related crates**:
  - `perl-semantic-analyzer` (symbol table generation)
  - `perl-parser-core` (location tracking)
  - `perl-lsp-rs` (LSP handler using rename)

## Implementation Handoff Notes

- **TDD order**: Red tests first (test_rename_no_duplicate_edits_for_shared_locations should fail), then green (apply fix)
- **Verification after fix**: `cargo test -p perl-lsp-rs-core` should pass all tests
- **Size**: Trivial (3 lines + 1 test, ~20 lines total)
- **Risk**: Very low — internal logic only, no API or protocol changes
- **Reviewer notes**: Verify that full sort is stable Rust behavior (yes, sort is guaranteed to be stable in Rust 1.0+) and that TextEdit comparison is sensible (location first, then new_text — correct ordering)
