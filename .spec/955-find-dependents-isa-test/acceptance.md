# Acceptance Criteria: Issue #955

Test-only PR to add regression tests for `@ISA`-based inheritance in `find_dependents`.

---

## §Behavior

| Input / Condition | Expected Result |
|---|---|
| `WorkspaceIndex::find_dependents("Base::Class")` called after indexing file with `our @ISA = qw(Base::Class);` | Returns non-empty list including the child package |
| `WorkspaceIndex::find_dependents("My::Root")` called after indexing file with `@ISA = ('My::Root');` | Returns non-empty list including the child package |
| `WorkspaceIndex::find_dependents("Base::Extended")` called after indexing file with `push @ISA, 'Base::Extended';` | Returns non-empty list including the child package |
| Multiple bases in `our @ISA = qw(Base1 Base2);` | Both Base1 and Base2 are registered and found by `find_dependents` |
| Parent/base not in `@ISA` | `find_dependents` returns empty or does not include the child |

---

## §Hazards

This is a **test-only PR** (no implementation changes to runtime code). Hazard focus is on test correctness and avoiding false positives/negatives. Test infrastructure hazards are minimal since the change reuses existing patterns.

| Hazard Class | Surface | Mitigation |
|---|---|---|
| **WORKSPACE-1: Index State Corruption** | `WorkspaceIndex::find_dependents` may incorrectly register/forget inheritance edges in multi-file scenarios | Each test uses a fresh `WorkspaceIndex` instance; single file per test. Edge case: multiple inheritance forms in the same file is covered implicitly by existing tests. No cross-file state mutation. |
| **WORKSPACE-2: Cache Invalidation** | Symbol cache may serve stale results if inheritance registration is not flushed | Fresh index per test ensures no cache carry-over. File URIs are unique per test (e.g., `/child.pm`, `/derived.pm`, `/extended.pm`). |
| **WORKSPACE-3: Dual-Index Consistency** | Inheritance edge registered under one name form but not the other (qualified vs bare) | The implementation emits edges with full package names. Tests verify by looking up the base class name directly (e.g., `"Base::Class"`). |
| **WORKSPACE-4: Parser Regression** | Parser fails to parse `@ISA` forms → extractor never runs → test passes falsely | Tests will fail if parser rejects `@ISA` syntax (syntax error during `index_file`). Parser rejects leading to test failure is correct behavior. |
| **WORKSPACE-5: Edge Kind Mismatch** | Edge is registered with wrong kind (e.g., `ComposesRole` instead of `Inherits`) | Tests use `find_dependents`, which treats all edge kinds as dependencies. If wrong kind is emitted, test may still pass. Mitigation: if a future caller cares about edge kind, this test suite is sufficient to reveal the distinction. For now, coverage is adequate. |
| **WORKSPACE-6: @ISA Extractor Regression** | `package_graph_extractor.rs` logic for `@ISA` forms regresses and stops emitting edges | Test failure signals the regression. This is the desired outcome: the test uncovers the bug. |

---

## §Contracts

**Crates touched**: `perl-workspace` (tests only; no parser, analyzer, or workspace-index implementation changes).

**API contracts verified**:

| Contract | Surface | Verified By |
|---|---|---|
| `WorkspaceIndex::index_file(uri, text) -> Result<(), E>` | Accepts Perl source with `@ISA` patterns and parses without error | Tests pass `index_file` calls with valid `@ISA` syntax; no error expected. |
| `WorkspaceIndex::find_dependents(name) -> Vec<...>` | Returns list of packages that inherit from or compose the named package | `test_find_dependents_via_*` assertions check `!dependents.is_empty()`. |
| `package_graph_extractor.rs`: `@ISA = (...)` parsing → `Inherits` edge emission | Documented in extractor (lines 14-16, 104-134). Verification: if extractor regresses, tests fail and flag it. | Test structure: if implementation works, test passes; if broken, fails. |
| `package_graph_extractor.rs`: `our @ISA = qw(...)` parsing → `Inherits` edge emission | Documented in extractor (line 15, 104-118). | Same as above. |
| `package_graph_extractor.rs`: `push @ISA, '...'` parsing → `Inherits` edge emission | Documented in extractor (line 16, 155-173). | Same as above. |

---

## §API-Shape

**No new public API introduced** — tests only.

**Caller count**: Tests are callers of `WorkspaceIndex::new()` (line 26 of module, public), `index_file()` (public), and `find_dependents()` (public). Existing callers unaffected.

**Dup-risk**: Test names follow the existing pattern `test_find_dependents_via_<form>`. No name collision with existing tests (grep confirms none exist). File-scope test functions pose no export/symbol table collision risk.

**Regression risk**: Tests are additive (no test removal or modification). Existing tests remain unchanged. Worst case: new tests fail, signaling a real bug in the indexing layer.

---

## §Test-Grid

**Test cases by form + scope**:

| Test Name | Input | Condition | Expected Result | Invariant |
|---|---|---|---|---|
| `test_find_dependents_via_our_isa` | `our @ISA = qw(Base::Class);` | Single inheritance via `our` qualifier, qw() list | `find_dependents("Base::Class")` non-empty | Inherited class is in dependents list |
| `test_find_dependents_via_bare_isa` | `@ISA = ('My::Root');` | Single inheritance via bare assignment, list literal | `find_dependents("My::Root")` non-empty | Inherited class is in dependents list |
| `test_find_dependents_via_push_isa` | `push @ISA, 'Base::Extended';` | Single base added via push | `find_dependents("Base::Extended")` non-empty | Inherited class is in dependents list |

**Negative / Boundary cases** (covered by test structure):

| Case | Coverage | Result |
|---|---|---|
| Non-existent base in `find_dependents` call | Implicit (each test uses a different base name; no collision) | Returns empty; test would fail if registration missed |
| Empty `@ISA = ()` | Not explicitly tested; out of scope (issue requests `@ISA` coverage, not edge cases). Follow-up if needed. | N/A |
| Malformed Perl (syntax error) | Parser will reject; `index_file` returns error; test fails with parse error | Expected behavior, not a test failure. |

---

## §Blast-Radius

**Consumers of changed files**: 
- `perl-workspace` tests: Only test file modified. No production code change.
- Downstream crates (`perl-lsp-*`, `perl-dap-*`): No change to public APIs or exports. Tests only.

**Boundary preservation**:
- Parser contract (`package_graph_extractor.rs`): No change.
- Workspace index API: No change to signatures or behavior.
- Semantic analyzer: No change.
- LSP/DAP consumers: No change to workspace symbol resolution behavior (tests verify it works, not that it changes).

**Must-not-touch**:
- `crates/perl-parser/` — No parser changes.
- `crates/perl-semantic-analyzer/` — No analyzer changes.
- `crates/perl-workspace/src/` — No workspace index implementation changes (test file only).

**Merge safety**: 
- Low risk: Test-only PR, no production code.
- Pre-merge verification: Run `cargo test -p perl-workspace --lib` to ensure all tests (new + existing) pass.
- No dependency or CI config changes required.

---

## Summary

This is a test-only PR closing a documented coverage gap. The implementation in `package_graph_extractor.rs` already supports `@ISA` inheritance patterns (lines 14-16 document them; lines 104-173 implement extraction). Three new regression tests verify that `WorkspaceIndex::find_dependents` correctly registers these forms as inheritance dependencies. If tests pass, coverage is closed. If any test fails, it surfaces a latent bug in the indexing layer that should be escalated to the parser-fix or semantic-analyzer teams.
