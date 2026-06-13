# Issue #811: Workspace-Symbol Dual Indexing Gaps in Re-Export Chains

## Problem Statement

PR #122 implemented dual-name indexing (qualified `Package::name` and bare `name` forms) for comprehensive symbol resolution. However, this indexing strategy conflates re-export edges with definition edges, causing LSP consumers (goto-definition, references, workspace-symbol queries) to diverge when:

1. **Re-export chains**: Module A re-exports Module B's subroutine via `Exporter`/`EXPORT_OK`. Goto-definition on the bare call should jump to B's definition, not A's import site.

2. **Package-qualified references in cross-file contexts**: `Foo::Bar::baz()` called from a different file should resolve to the definition in `Foo/Bar.pm`, not a bare-name hit from a different package.

3. **Bare-name collisions**: Two packages define the same subroutine name. Bare-name references should resolve to the lexically appropriate one (per scope/imports), not an arbitrary workspace match.

## Current Behavior (Buggy)

- `find_definition()` returns the first indexed candidate without considering re-export boundaries.
- `find_references()` and `find_symbols()` do not distinguish between original definitions and re-export sites.
- `symbol_uri_reachable()` in `EffectiveIncContext` filters by @INC membership but ignores Exporter re-export boundaries.
- Workspace-symbol queries return re-export sites and original definitions with equal ranking.

### Test Evidence

Test `test_reference_kinds_import_parent_and_export_ok_are_currently_import_only()` at line 5061 of `workspace_index.rs` explicitly asserts:
```
"EXPORT_OK entries are currently not represented as reference edges"
```

This is the root cause: export relationships are not tracked as edges in the reference index, so re-export chains cannot be followed.

## Solution Approach

1. **Track export relationships in the reference index**: When a file declares `@EXPORT_OK` or `@EXPORT`, record edges from the exporting module to each exported symbol.

2. **Enhance `find_definition()` to follow re-export chains**: When a symbol is found in an importing package, check if it was re-exported from another module, and return the original definition instead.

3. **Rank workspace-symbol results by origin**: Original definitions should rank higher than re-export sites.

4. **Extend `symbol_uri_reachable()` to account for re-exports**: Allow symbols from re-exporting modules to be reachable even if their original definitions are outside the current @INC roots.

## Dependencies

- `perl-semantic-facts`: `ExportSet`, `ImportSpec`, `EdgeFact` types already exist
- `perl-workspace/semantic/imports.rs`: `ImportExportIndex` already tracks imports and exports
- `perl-workspace/semantic/references.rs`: `ReferenceIndex` needs extension to support export edges

## Prior Art & Decisions

### Decision: Edge-Based Re-Export Tracking (Not Symbol-Level Annotation)

**Rejected**: Annotating each symbol with "is_re_export" field.
- Reason: Symbols should be canonical; re-export is an edge property. Multiple re-exports would create duplicates.

**Chosen**: Store explicit edges in `ReferenceIndex` from exporting module → each exported symbol.
- Allows tracing re-export chains through the index.
- Matches the existing reference-edge pattern (parent, with, has, etc.).
- Enables `find_definition()` to follow chains: `A::foo` → (re-export from B) → `B::foo` definition.

### Decision: Scope-Aware Resolution (Deferred)

The issue mentions lexical scope resolution ("appropriate one per scope/imports"). Full scope-aware resolution requires:
- Position-scoped import tracking in `EffectiveIncContext`
- Cross-file scope graph in the workspace index

This is deferred to a follow-up; the current fix focuses on eliminating re-export chain confusion via definition tracing.

## Related Issues / PRs

- **PR #122**: Original dual-indexing implementation (`find_definition` + `find_references` + `find_symbols`)
- **#8537**: PL701 module reachability (related but separate — handles @INC filtering)
- **test_reference_kinds_import_parent_and_export_ok_are_currently_import_only**: Test at line 5061 documenting the gap

## Test Fixtures

Existing fixture structure in `crates/perl-workspace/tests/fixtures/semantic_scorecard/` can be extended. For now, inline multi-file tests in `dual_indexing_tests.rs` are sufficient to verify the fix.

## Success Criteria

1. Multi-file test: Module A re-exports Module B's `optional()`. Goto-definition from A's namespace jumps to B's definition.
2. Consumer consistency: `goto-definition()`, `find_references()`, and `find_symbols()` all agree on re-export targets.
3. Ranking: Original definition scores higher than re-export sites in workspace-symbol queries.
