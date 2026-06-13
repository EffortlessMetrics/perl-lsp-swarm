# Implementation Checklist: #1372 — Parser Panic on User Input

**Status**: Ready for red-TDD → Build → Review

**Issue**: Parser panics on ambiguous/incomplete Perl syntax in `crates/perl-parser/tests/nodekind_combination_error_handling_edge_cases.rs::test_incomplete_ambiguous_syntax()`.

**Scope**: Uncomment the ignored test, reproduce the panic, fix the root cause in the parser to return graceful error nodes instead of panicking.

---

## Step 1: Uncomment the Test (No Implementation Yet)

**What changes**: `crates/perl-parser/tests/nodekind_combination_error_handling_edge_cases.rs`

**Changes**:
- Line 342-345: Remove `//` comment markers from `#[test]` and `#[ignore]` decorator lines
- Line 346: Remove `//` from function signature line
- Lines 347-510: Uncomment the Perl code string and test assertions
- Line 492-509: Replace calls to nonexistent `find_nodes_of_kind(&ast, |k| ...)` with existing `has_node_kind(&ast, "NodeKindName")` calls (same assertions, compatible API)

**Why this order**: Red-TDD builder writes failing tests first. This uncommented test WILL panic when run, documenting the exact input and panic site in the backtrace.

**Verify command**:
```bash
cargo test -p perl-parser --test nodekind_combination_error_handling_edge_cases test_incomplete_ambiguous_syntax 2>&1 | head -200
```

Expected: Test runs and panics with backtrace showing file:line of panic.

---

## Step 2: Identify Panic Site

**What to do**: The red-TDD builder runs the test and captures the panic backtrace.

**Expected outcome**: Backtrace will show:
- File and line in `crates/perl-parser-core/src/` or `crates/perl-parser/src/` where `unwrap()` / `expect()` / index-out-of-bounds / slice panic occurs
- Likely sites: regex delimiter handling (`m#pattern#`), quote-like operators (`q{...}`, `<<EOF`), or byte-slicing operations on UTF-8 boundaries

**Key invariant**: The panic MUST be in parser code, not test code. If the panic is in test scaffolding, red-TDD will fix the test harness instead.

---

## Step 3: Fix the Parser Panic Site (Builder Implements)

**What changes**: The parser module containing the panic site

**How to fix**:
1. **Locate the `unwrap()`/`expect()`/panicking slice/array access**
   - Use `.get()` or `.get_mut()` instead of direct indexing
   - Use `checked_*` operations for arithmetic (e.g., `checked_add`, `checked_sub`)
   - Wrap in `?` error propagation or `.ok_or_else()` Result handling
2. **Return graceful error or recovery node**
   - On bounds violation or invalid state, construct an `Error` recovery node instead of panicking
   - Preserve parser state so recovery can continue past the error
3. **Do NOT introduce `unwrap()` / `expect()` / `panic!()` / `todo!()` elsewhere**
   - Use pattern matching, `.map()`, `.and_then()`, `Result` returns
4. **Test locally with bounded fuzz**
   - The green-TDD builder will add adversarial test cases; the fix must handle them

**Verify command**:
```bash
cargo test -p perl-parser --test nodekind_combination_error_handling_edge_cases test_incomplete_ambiguous_syntax 2>&1 | tail -20
```

Expected after fix: Test passes (no panic).

---

## Step 4: Verify No New Panics (Post-Build)

**What to do**: Red-TDD + Green-TDD add edge-case tests around the panic site to harden recovery.

**Test additions** (green-TDD will add):
- Minimal repro: Single Perl line that triggered the panic
- Boundary cases: Empty input, truncated input, only delimiters, nested structures
- Fuzz-adjacent: Adversarial UTF-8, mixed delimiters, unterminated quotes/heredocs
- State invariants: After error recovery, parser can continue parsing valid statements

**Verify command**:
```bash
cargo test -p perl-parser 2>&1 | grep -E "^test result:|FAILED|panicked"
```

Expected: All tests pass, no panicked output.

---

## Step 5: Integration Verification

**What to do**: Verify parser doesn't regress on corpus or snapshot tests.

**Verify commands**:
```bash
cargo test -p perl-parser --lib 2>&1 | tail -5
cargo test -p perl-parser --test '*' 2>&1 | grep -E "^test result:|FAILED"
```

Expected: All tests pass.

---

## Compilation Order

**Dependency chain**:
1. Step 1 (red-TDD): Uncomment test → test panics in Step 1 verify
2. Step 2 (discovery): Backtrace shows panic site
3. Step 3 (builder): Fix panic site → Step 3 verify passes
4. Step 4 (green-TDD): Add adversarial tests → all tests pass
5. Step 5 (integration): Workspace-wide tests pass

Each step must compile. No unresolved dependencies until Step 3 completes the fix.

---

## Known Constraints

- **Parser must NEVER panic on user input** — this is non-negotiable (§STABILITY goal)
- **No new `unwrap()` / `expect()` / `panic!()` in production code**
- **Recovery nodes must be valid AST** — downstream LSP/DAP consumers depend on AST structure
- **Test file is in crates/perl-parser/tests/** — changes do not affect core parser library exports
- **Snapshot tests may need refresh** — if parser recovery changes AST shape, insta snapshots may need `--test-threads=1 --nocapture` updates

---

## Rollback/Safety

- If Step 3 fix breaks >5 existing tests, revert and bump back to red-TDD for clarification
- If panic is unfixable without refactoring, mark as `follow-up-recommended` and file separate issue
- The `#[ignore]` on the test is temporary; after fix, the test runs as normal

---

## Related Artifacts

- **Issue**: #1372 (GitHub)
- **Test file**: `crates/perl-parser/tests/nodekind_combination_error_handling_edge_cases.rs`
- **Spec**: `.spec/1372-parser-panic-on-user-input/` (this directory)
- **PARSER_CONTRACTS**: `docs/reference/PARSER_CONTRACTS.md` (recovery node invariants)
- **Hazard class**: PARSER-2 (Panic-on-Input), PARSER-3 (Bounds Violation)
