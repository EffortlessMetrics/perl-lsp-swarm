# Context & Decision Log: #1372 — Parser Panic on User Input

## Problem Statement

**Issue**: `crates/perl-parser/tests/nodekind_combination_error_handling_edge_cases.rs::test_incomplete_ambiguous_syntax()` is commented out with `#[ignore]` and marked "Temporarily ignored due to compiler panic".

**Root cause**: The test contains Perl code that exercises ambiguous and edge-case syntax (e.g., regex with alternative delimiters `m#...#`, quote-like operators `q{...}`, unterminated heredocs, deeply nested structures). When uncommented and run, the parser panics instead of gracefully handling the malformed input.

**Severity**: P0 (STABILITY blocker)
- If parser panics on arbitrary user input, LSP server crashes
- Debugger (DAP) crashes
- Workspace symbol indexing fails
- Per CLAUDE.md: "parser must NEVER panic on user input"

**Scope**: 
- Parser behavioral fix (reduce panics to graceful error nodes)
- Test harness fix (uncomment test, fix reference to nonexistent function)
- Downstream consumer verification (LSP/DAP/semantic analyzer handle Error nodes)

---

## Investigation & Discovery

### Test File History

- **Created**: v0.9.0 (commit 6104e2890)
- **File**: `crates/perl-parser/tests/nodekind_combination_error_handling_edge_cases.rs`
- **Test function**: `test_incomplete_ambiguous_syntax()` (lines 342-510)
- **Status**: Commented out (all lines prefixed with `//`), marked `#[ignore]`

### Test Code Issues Discovered

The commented test uses:
- `find_nodes_of_kind(&ast, |k| ...)` — function does not exist in module
- Correct alternative exists: `has_node_kind(&ast, "NodeKindName")` (available in `nodekind_helpers`)
- This suggests the test was never actually valid or executable

### Parser Panic Hypothesis

When the test is uncommented and run, the Perl code will be parsed by `Parser::new().parse()`. The likely panic sites:

1. **Quote-like operators with alternative delimiters** (`q{...}`, `q[...]`, `q|...|`)
   - Likely site: Delimiter matching / quote-like parser in `crates/perl-parser-core/src/`
   - Panic mechanism: Index out of bounds, unwrap() on None, array indexing without bounds check

2. **Regex with alternative delimiters** (`m#...#`, `m!...!`, `m|...|`)
   - Likely site: Regex parser in `crates/perl-parser-core/src/`
   - Panic mechanism: Similar to quote-like

3. **Heredocs with variants** (`<<'EOF'`, `<<"EOF"`, `<<\`EOF\``, `<<~EOF`)
   - Likely site: Heredoc parser
   - Panic mechanism: Delimiter boundary detection, UTF-8 slicing

4. **Deep nesting or byte-slicing on UTF-8 boundaries**
   - Panic mechanism: Stack overflow, panic on string slice at non-UTF8 boundary

### Current Parser Status

**No `unwrap()` or `expect()` in production parser code**:
```bash
$ grep -r "\.unwrap()\|\.expect(" crates/perl-parser/src/ --include="*.rs"
# Result: 0 matches
```

This is excellent — the parser already practices defensive programming at the top level. Panics are likely from:
- Direct array indexing (`arr[i]` without bounds check)
- String slicing on byte boundaries without validation
- Recursive depth leading to stack overflow

---

## Alternatives Rejected

### Alternative 1: Delete the Test
**Rejected**: Issue explicitly asks to uncomment, fix, and verify. The test is valid and belongs in the suite.

### Alternative 2: Implement `find_nodes_of_kind()` Helper
**Rejected**: Helper doesn't exist and isn't necessary. `has_node_kind()` is already available and sufficient for the test assertions.

### Alternative 3: Fix Only the Test, Leave Parser Panic
**Rejected**: The panic is in the *parser*, not the test. Red-TDD builder will uncover it when running the uncommented test. The fix must be in the parser itself.

### Alternative 4: Introduce `panic_safe!()` Wrapper
**Rejected**: Wrappers don't fix the root cause. Parser must handle all inputs safely without runtime panic.

---

## Key Decisions

### Decision 1: Scope of Fix

**Chosen**: Fix the parser to return graceful Error nodes instead of panicking.
- This aligns with the project's STABILITY goal ("robust on imperfect code")
- Downstream LSP/DAP code is built to handle Error nodes
- No changes to public API or parser signature needed

### Decision 2: Error Node Placement

**Chosen**: Construct Error nodes at the point of parse failure, with source location span preserved.
- Error nodes must have valid location (0..source.len()), not panic on invalid spans
- Recovery must continue parsing subsequent valid statements
- Documented in `docs/reference/PARSER_CONTRACTS.md`

### Decision 3: Test Harness Fixes

**Chosen**: Fix nonexistent `find_nodes_of_kind()` calls to use `has_node_kind()`.
- Minimal change; same assertion semantics
- Aligns with existing test helpers
- Red-TDD builder will implement

### Decision 4: Snapshot Updates

**Chosen**: Update snapshot tests if parser behavior changes (graceful error vs panic).
- This is expected and acceptable
- Snapshots document the new graceful-degradation behavior
- No semantic change to valid Perl parsing

---

## Prior Art & Reference

### Perl Parsing Edge Cases

**Quote-like operators** (from Perl docs):
- `q{...}`, `q[...]`, `q|...|`, `q#...#`, etc. (single-quoted)
- `qq{...}` (double-quoted with interpolation)
- `qx{...}` (backtick execution)
- `qw(...)` (word list)
- Alternative delimiters must be matched: opening/closing must be balanced or paired

**Regex delimiters**:
- `m#pattern#`, `m!pattern!`, `m|pattern|`, `m{...}`, `m[...]`, etc.
- Substitution: `s/.../.../`, `s#...#...#`, etc.
- Transliteration: `tr/.../.../`, `tr[...][...]`, etc.

**Heredocs**:
- `<<EOF` (double-quoted by default)
- `<<'EOF'` (single-quoted, no interpolation)
- `<<"EOF"` (explicit double-quoted)
- `<<\`EOF\`` (backtick, command execution)
- `<<~EOF` (indented heredoc, Perl 5.26+)

**Nesting & Complexity**:
- Perl allows arbitrary nesting depth
- Parser must gracefully degrade at practical limits (e.g., 1000+ levels)

### Related Issues

- **#964** (fix/964-clear-frames-on-resume): DAP frame handling; related to parser stability indirectly
- **#1232** (fix/ci): Coverage measurement decoupling; test infrastructure improvement
- **#777** (v0.9.0 release): Semantic-Ready milestone; introduced nodekind edge-case tests

### Learned Patterns

- **Shift-left-ladder** (docs/concepts): Catch panics early in parser (red-TDD), not in downstream LSP/DAP
- **Recovery-node-correctness** (docs/reference/PARSER_CONTRACTS.md): Error nodes must have valid AST shape
- **Hazard-class-invariants**: PARSER-2 (Panic-on-Input) is a cross-subsystem hazard; must be eliminated

---

## Rationale for Spec-Planner Approach

### Why Spec-Planner Writes the Checklist

This issue touches the parser (1 crate, heavily used):
- 50+ callers across LSP, DAP, workspace, semantic analyzer
- Panic is P0 stability risk
- High blast radius if fix introduces regressions

**Non-trivial criteria met**:
1. Issue introduces no new public API (internal fix only)
2. Multiple subsystems affected (LSP, DAP, workspace)
3. Panic-on-input is a recurring hazard class

**Workflow invocation**: This issue warrants the full `spec-builder` workflow to populate §Hazards and §Blast-Radius from multiple angles before writing the final acceptance.md. All six sections are required.

### Why Red-TDD Builder Writes Failing Tests First

The test already exists (though commented). Red-TDD builder's job:
1. Uncomment the test
2. Fix the test harness (replace `find_nodes_of_kind` with `has_node_kind`)
3. Run the test → watch it panic
4. Commit the panicking test with assertion `assert!(!panic, "Parser must not panic")`

This documents the exact input and backtrace for the builder.

### Why Builder Fixes the Parser

Once red-TDD captures the panic, the builder:
1. Identifies the panic site (file:line)
2. Replaces panicking operation with graceful error handling
3. Constructs Error node instead
4. Verifies test passes

### Why Green-TDD Hardens the Fix

After builder implements, green-TDD:
1. Adds adversarial tests around the panic site
2. Adds boundary tests (UTF-8, empty input, etc.)
3. Verifies parser state after error recovery
4. Checks downstream code (semantic analyzer, LSP) doesn't crash on Error nodes

---

## Related Documentation

- **PARSER_CONTRACTS.md** (docs/reference/): Recovery node invariants, quote-like/regex/heredoc specs
- **SUBSYSTEM_HAZARD_DEFAULTS.md** (docs/reference/): PARSER-1 through PARSER-4 hazard rows (seed into §Hazards)
- **STABILITY.md** (docs/reference/): Project stability goals; parser must NEVER panic
- **FAILURE_MODES.md** (docs/reference/): Historical parser failure patterns
- **Learnings** (docs/learnings/): Related incidents: panic recovery, error node correctness, boundary bugs

---

## Risk Assessment

### High-Risk Aspects

1. **Downstream Pattern Matching**: If Error node schema changes, LSP/DAP code may need updates
   - **Mitigation**: Document Error node structure in PARSER_CONTRACTS; test semantic analyzer with Error nodes
   
2. **Snapshot Test Cascades**: Parser behavior change (graceful error) may require snapshot updates
   - **Mitigation**: Re-run `insta` snapshots with `--test-threads=1 --nocapture`; review diffs before approving

3. **Performance Regression**: New bounds checks or error handling may slow parser
   - **Mitigation**: Benchmark before/after; if >10% regression, optimize or defer to follow-up

### Low-Risk Aspects

1. **Public API**: No signature changes expected; fix is internal
2. **LSP Protocol**: No LSP spec changes; robustness is transparent
3. **Existing Valid Perl**: Parser already handles valid Perl correctly; fix only impacts error paths

---

## Success Criteria

**Issue closes when ALL of the following are true**:

1. ✅ Test `test_incomplete_ambiguous_syntax()` is uncommented
2. ✅ Test fixes `find_nodes_of_kind` calls to use `has_node_kind`
3. ✅ Test runs WITHOUT panicking (parser returns Error nodes or valid AST)
4. ✅ Panic site identified and fixed in parser code
5. ✅ No new `unwrap()` / `expect()` / `panic!()` introduced in parser
6. ✅ Green-TDD hardening tests pass (adversarial, boundary, recovery)
7. ✅ All parser tests pass (no regressions)
8. ✅ Semantic analyzer + LSP providers handle Error nodes without crashes
9. ✅ Snapshot tests updated if behavior changed
10. ✅ PR reviewer confirms no panic paths remain

**Verification command** (final):
```bash
cargo test -p perl-parser 2>&1 | grep -E "^test result:|FAILED|panicked"
# Expected: "test result: ok. X passed; 0 failed; 0 ignored"
```

---

## Open Questions (For Red-TDD & Builder)

1. **Exact panic site**: Which file:line panics? (Will be determined when test is uncommented and run)
2. **Input class**: Is panic triggered by all ambiguous syntax, or specific delimiters?
3. **Downstream impact**: Do Error nodes require schema changes?
4. **Performance impact**: Any measurable slowdown from new error handling?

These will be answered during the red-TDD and build phases.
