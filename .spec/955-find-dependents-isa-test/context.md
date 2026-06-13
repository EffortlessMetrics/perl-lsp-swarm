# Context: Issue #955 — find_dependents Test Coverage for @ISA Inheritance

## Problem Statement

`WorkspaceIndex::find_dependents` has regression tests for `use parent` and `use base` inheritance forms, but **none** for the `@ISA` equivalents, despite the extractor explicitly supporting them.

- **Gap**: No tests for `our @ISA = qw(...)`, `@ISA = (...)`, or `push @ISA, ...`.
- **Risk**: A regression in `@ISA` indexing would be silent — `find_dependents("Base")` would silently miss `@ISA`-derived children.
- **Confidence**: Medium. Implementation is present; writing the tests will either close the gap (test pass) or surface a real bug (test fail).

## Evidence

### Implementation Support (Verified)

`crates/perl-semantic-analyzer/src/analysis/package_graph_extractor.rs`:
- **Lines 14-16**: Documentation table lists `@ISA = ('Base')`, `our @ISA = qw(...)`, `push @ISA, 'Base'` as supported patterns, all mapping to `Inherits` edge kind.
- **Lines 104-118**: Handler for `NodeKind::VariableDeclaration { variable, initializer: Some(init), .. }` — matches `our @ISA` and emits inheritance edge.
- **Lines 120-134**: Handler for `NodeKind::Assignment { lhs, rhs, .. }` — matches bare `@ISA = (...)` and emits inheritance edge.
- **Lines 155-173**: Handler for `push` function calls — matches `push @ISA, '...'` and emits inheritance edge.

All three forms call `emit_edge(...)` with `PackageEdgeKind::Inherits` and `Confidence::High`, indicating production-ready implementation.

### Existing Tests (Verified)

`crates/perl-workspace/tests/comprehensive_unit_tests.rs`:
- **Lines 2108-2119**: `test_find_dependents_via_use_parent` — regression test for `use parent` (filed for issue #2747).
- **Lines 2124-2131**: `test_find_dependents_via_use_base` — regression test for `use base`.
- **Lines 2136-2149**: `test_find_dependents_via_use_parent_qw` — regression test for `use parent qw(...)` with multiple bases.
- **Zero matches** for `@ISA` in the test file (grep -n "@ISA" returned 0).

### Test Infrastructure Available

- `WorkspaceIndex::new()` — public constructor.
- `file_url(path: &str)` — test helper (line 28) to construct file URIs.
- `WorkspaceIndex::index_file(uri, text)` — indexes Perl source.
- `WorkspaceIndex::find_dependents(name)` — returns list of packages that inherit from the named package.
- `Result<(), Box<dyn std::error::Error>>` — standard test return type.

All infrastructure is present and tested; new tests can reuse the existing pattern.

---

## Decisions

### 1. Test-Only Fix vs. Implementation Debug
**Decision**: Test-only. The extractor implementation is present and documented. Adding tests will reveal whether the gap is test-only (all pass) or a latent bug (one or more fail). Either outcome is valuable; the test provides the proof.

**Rationale**: The issue description suggests confidence that the implementation works; the risk is that tests never verified it. Writing tests is lower-cost than debugging the implementation blind.

### 2. Three Separate Tests vs. Single Parameterized Test
**Decision**: Three separate tests (`test_find_dependents_via_our_isa`, `test_find_dependents_via_bare_isa`, `test_find_dependents_via_push_isa`).

**Rationale**: 
- Matches the existing test pattern (separate functions for `use parent` and `use base`).
- Simpler to debug if one form fails (test name immediately shows which form is broken).
- Faster feedback loop than parameterized tests in a test framework with minimal infrastructure.
- Mirrors the three forms documented in the extractor's comment table (lines 14-16).

### 3. Single-File Index vs. Multi-File Workspace
**Decision**: Single-file per test (fresh `WorkspaceIndex` per test, one file indexed).

**Rationale**: 
- Simplicity: each test is independent and reproducible.
- Matches existing test pattern.
- Sufficient to verify the indexing logic. Multi-file scenarios are covered by other workspace tests if needed.

### 4. Scope: Only `find_dependents` or Also Edge Metadata?
**Decision**: Only `find_dependents` (return value is non-empty).

**Rationale**: 
- The issue specifically requests regression tests for `find_dependents`.
- The function accepts all inheritance edge kinds equally (doesn't distinguish `Inherits` from `ComposesRole` in its result).
- If future work cares about edge metadata (e.g., distinguishing inheritance from role composition), new tests can verify that.
- Current scope is tightly scoped to the issue statement.

---

## Alternatives Rejected

### Alt 1: Manually Test Against Live LSP Server
**Rejected**: Not hermetic, not reproducible, not part of the CI gate.

### Alt 2: Add Tests to `package_graph_extractor` Tests
**Rejected**: The issue explicitly asks for tests in `WorkspaceIndex` to verify the end-to-end behavior (indexing + lookup). Testing the extractor in isolation would not verify that the index consumes the edges correctly.

### Alt 3: Use Snapshot Testing
**Rejected**: Unnecessary for boolean assertions (`!is_empty()`). Snapshot tests are valuable for complex outputs (e.g., AST dumps); here we're just checking a list is non-empty.

---

## Prior Art & References

### Related Issues
- **Issue #2747** (referenced in test comments): Regression that motivated `test_find_dependents_via_use_parent` and `test_find_dependents_via_use_base`. Context: a file with only `use parent` (no direct `use Module`) was not registering the parent as a dependency.
  - **How this applies**: `@ISA` is the Perl 4 / legacy equivalent of `use parent`. If #2747's regression existed, an `@ISA`-based regression would have been silent (no test to catch it).

### Perl Documentation
- **`@ISA` pattern**: Standard Perl inheritance mechanism, older than `use parent` (which was introduced in Perl 5.10.1, 2009).
  - Perl 5 Perlvar: "If a package defines an array named `@ISA`, the inheritance mechanism in Perl will use that array as a list of base classes."
  - Still widely used in legacy codebases, CPAN modules, and L<Moose>/L<Moo> codebases (Moose::Object uses it internally).

### PARSER_CONTRACTS.md
- **Reference**: `docs/reference/PARSER_CONTRACTS.md` — not directly cited in this issue, but the extractor's contract is documented there if a parser-level test exists.
  - To verify: if `@ISA` patterns are in the parser test corpus, they should already be tested at the parse level. This PR tests the semantic/indexing layer.

---

## Test Sequence

**Step 1**: Index file with `our @ISA = qw(Base::Class);` → call `find_dependents("Base::Class")` → expect non-empty.

**Step 2**: Index file with `@ISA = ('My::Root');` → call `find_dependents("My::Root")` → expect non-empty.

**Step 3**: Index file with `push @ISA, 'Base::Extended';` → call `find_dependents("Base::Extended")` → expect non-empty.

**Step 4**: Run full test suite; all tests should pass.

If any test fails, the failure message will indicate which `@ISA` form broke the indexing, providing a clear signal for the builder or parser-fix team to investigate.

---

## Success Criteria

1. All three new tests compile without error.
2. All three new tests pass when run against the current main branch.
3. No existing tests regress.
4. The PR can merge cleanly without conflict.

## Follow-Ups (Not In Scope)

- If a test fails: escalate to parser-fix or semantic-analyzer teams to debug the extractor.
- If all tests pass but edge metadata (inheritance vs. role composition) becomes important later: add separate tests for `find_dependents_for_inheritance()` vs. `find_dependents_for_roles()` (hypothetical API).
- If `@ISA` multi-inheritance is a common pattern and needs adversarial testing: add a test with `our @ISA = qw(Base1 Base2 Base3);` and verify all three are found.
