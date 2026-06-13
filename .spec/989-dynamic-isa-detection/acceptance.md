# Acceptance Criteria: #989 — Dynamic @ISA Detection

## §Behavior

| Input | Condition | Expected Result |
|-------|-----------|-----------------|
| `push @ISA, 'Base'` | Static string literal | Emits `PackageEdge(Child → Base, Inherits, High confidence)` |
| `push @ISA, $base` | Variable (dynamic) | Emits marker/edge indicating dynamic inheritance (Low confidence or sentinel `<dynamic>`) |
| `@ISA = ('Base')` | Static array literal | Emits `PackageEdge(Child → Base, Inherits, High confidence)` |
| `@ISA = $computed` | Variable (dynamic) | Emits marker/edge indicating dynamic inheritance (Low confidence) |
| `@ISA = get_parents()` | Function call (dynamic) | Emits marker/edge indicating dynamic inheritance (Low confidence) |
| `push @ISA, 'Base', $var` | Mixed literal and variable | Emits High-confidence edge for 'Base' + Low-confidence edge or marker for dynamic part |
| `@ISA = ()` | Empty array | Emits no edges (package has no parents) |
| `push @ISA, undef` | Undef value | Emits no edge or Low-confidence marker (depends on implementation choice) |

## §Hazards

| Hazard Class | Surface | Risk | Mitigation |
|--------------|---------|------|------------|
| **Missing dynamic boundary** (ARCH-1) | `collect_names_from_node` line 324 (catch-all arm) | Dynamic @ISA silently drops, workspace pretends static; goto/hover fails on inherited members | Emit Low-confidence edge or dynamic marker instead of Vec::new(); test that marker is present |
| **Incorrect confidence propagation** (LOGIC-1) | `collect_names_from_args`, all call sites | Mixed static+dynamic args lose individual confidence; all treated as High or Low | Emit separate edges per argument with appropriate confidence; test each separately |
| **Sentinel collision** (SEMANTICS-1) | `to_package: "<dynamic>"` if chosen | Literal package named `<dynamic>` or similar could collide | Use an internal marker that cannot be a valid Perl package name (e.g., `__DYNAMIC__`); document as internal; consider enum variant instead (follow-up) |
| **Provenance vs Confidence conflation** (SEMANTICS-2) | `Provenance::DynamicBoundary` + `Confidence::Low` | Unclear which field signals the dynamic aspect | Provenance signals "how we inferred it" (DynamicBoundary = detected a dynamic expression); Confidence signals "how sure we are" (Low = dynamic = uncertain); both present is correct |
| **Regression: static case still works** (REGRESSION-1) | All static @ISA patterns (use parent, use base, @ISA = (...), push @ISA, 'string') | Builder's changes break existing high-confidence extraction | Test suite already covers static cases; verify all existing tests still pass |
| **Unhandled AST nodes** (DESIGN-1) | `NodeKind::Variable`, `NodeKind::FunctionCall`, and any future nodes | New dynamic cases are handled but future patterns may still be silently dropped | Document the dynamic-detection strategy in comments; consider a follow-up issue for expanding covered patterns |

## §Contracts

| Contract | File:line | Obligation |
|----------|-----------|-----------|
| **PARSER_CONTRACTS: NodeKind exhaustiveness** | `crates/perl-parser/src/ast.rs` | The parser defines NodeKind variants; extractor must match new variants as they're added |
| **Semantic Facts: PackageEdgeKind** | `crates/perl-semantic-facts/src/lib.rs:line(PackageEdgeKind enum)` | Extractor may emit Inherits, ComposesRole, or DependsOn; DynamicBoundary is a Provenance, not EdgeKind |
| **Semantic Facts: Confidence enum** | `crates/perl-semantic-facts/src/lib.rs:line(Confidence enum)` | Extractor may emit High, Medium, or Low confidence; Low is correct for dynamic detection |
| **Semantic Facts: Provenance enum** | `crates/perl-semantic-facts/src/lib.rs:line(Provenance enum)` | DynamicBoundary is a valid Provenance variant; signals "this fact was inferred from a dynamic boundary" |
| **Semantic Analyzer: package_edges storage** | `crates/perl-semantic-analyzer/src/analysis/semantic/mod.rs` | Semantic analyzer receives package_edges from extractor and stores them; no changes to storage needed, only source (extractor) |

## §API-Shape

### New Internal Enum
- **Name:** `NameExtractionResult` (internal to `package_graph_extractor.rs`)
- **Variants:** `Literal(Vec<String>)`, `Dynamic`
- **Scope:** Private to module, not exported
- **Callers:** `collect_names_from_node`, `collect_names_from_args`, all arms of those functions

### Updated Functions
- **`collect_names_from_node`** — signature change (returns `NameExtractionResult` instead of `Vec<String>`)
  - Callers: 4 sites within the extractor (VariableDeclaration, Assignment, push/extends/with handlers)
  - Impact: All callers must be updated to match-on the new return type (see checklist Step 5)
  - No external API change (private function)

- **`collect_names_from_args`** — signature change (returns `NameExtractionResult` instead of `Vec<String>`)
  - Callers: 1 site (extends/with handlers)
  - Impact: Caller must be updated
  - No external API change (private function)

### New Internal Method
- **`emit_dynamic_edge`** (if choosing sentinel representation)
  - Signature: `fn emit_dynamic_edge(&mut self, anchor_id: AnchorId) -> ()`
  - Behavior: Emits a single edge with `to_package = "<dynamic>"` or similar, `kind = DependsOn`, `confidence = Low`, `provenance = DynamicBoundary`
  - Callers: Dynamic branches in Step 5 call sites
  - No external API change (private method)

### Dup-Risk Grep
- Search for: `collect_names` (low risk; only used internally in package_graph_extractor)
- Search for: `emit_edge` (low risk; method is private)
- Search for: `@ISA` (already tested extensively; new tests are additive)

## §Test-Grid

| Category | Input | Expected | Test Name | Invariant |
|----------|-------|----------|-----------|-----------|
| **Positive: Static** | `push @ISA, 'Base'` | High-confidence `Inherits` edge emitted | `test_push_isa_single` (existing) | Edge present, confidence High |
| **Positive: Static (multi)** | `push @ISA, 'Base1', 'Base2'` | Two High-confidence edges | `test_push_isa_multiple` (existing) | Both edges present, both High |
| **Positive: Dynamic var** | `my $base = 'Base'; push @ISA, $base;` | Low-confidence edge or dynamic marker emitted | `test_push_isa_with_variable` (NEW) | Edge present, confidence Low or provenance DynamicBoundary |
| **Positive: Dynamic computed** | `@ISA = get_parents();` | Low-confidence edge or dynamic marker | `test_isa_assignment_with_function_call` (NEW) | Edge present, confidence Low |
| **Positive: Dynamic array var** | `my @bases = ('Base'); @ISA = @bases;` | Low-confidence edge or dynamic marker | `test_isa_assignment_with_array_variable` (NEW) | Edge present, confidence Low |
| **Positive: Mixed** | `push @ISA, 'Base', $var` | Base: High-confidence; $var: Low-confidence or marker | `test_push_isa_mixed_static_dynamic` (NEW) | Two edges with correct confidence levels |
| **Negative: Empty @ISA** | `@ISA = ()` | No edges emitted | `test_isa_empty_array` (existing or new) | Zero edges |
| **Negative: Unrelated code** | `use strict; use warnings;` | No inheritance edges | `test_no_edges_for_plain_use` (existing) | No edges |
| **Adversarial: undef** | `push @ISA, undef` | No edge or Low-confidence marker (consistent behavior) | `test_push_isa_with_undef` (NEW) | No edge OR consistent low-confidence marker |
| **Adversarial: interpolated string** | `push @ISA, "Base$ext"` | No High-confidence edge (string interpolation is dynamic) | `test_push_isa_interpolated_string` (NEW) | No High-confidence edge or marked dynamic |
| **State transition** | Static → add dynamic push | Both static and dynamic edges present | `test_isa_static_then_dynamic` (NEW) | Both edges in extracted result |

## Acceptance Checklist

- [ ] All existing tests pass (static cases unaffected)
- [ ] `test_push_isa_with_variable()` passes and emits Low-confidence or dynamic marker
- [ ] `test_isa_assignment_with_function_call()` passes
- [ ] `test_isa_assignment_with_array_variable()` passes
- [ ] `test_push_isa_mixed_static_dynamic()` passes with separate confidence levels
- [ ] Dynamic @ISA edges have `provenance = DynamicBoundary` or similar sentinel
- [ ] Sentinel package name (if used) is documented as internal and cannot collide with real packages
- [ ] No clippy warnings: `cargo clippy -p perl-semantic-analyzer`
- [ ] Formatted: `cargo xtask fmt`
- [ ] All tests pass: `cargo test -p perl-semantic-analyzer`
- [ ] Builder notes any ambiguities resolved in PR description

## §Blast-Radius

### Consumers

| Consumer | Location | Impact | Mitigation |
|----------|----------|--------|-----------|
| **Semantic Analyzer** | `crates/perl-semantic-analyzer/src/analysis/semantic/mod.rs` | Receives package_edges; already handles confidence levels; no change needed | Verify that Low-confidence edges do not break downstream LSP providers |
| **LSP providers** (e.g., definition, hover, rename) | `crates/perl-lsp-*/src/` | Consume package_edges via semantic analyzer; already filter by confidence or provenance | Verify that Low-confidence edges are handled gracefully (e.g., used as fallback, not primary) |
| **Workspace indexing** | `crates/perl-workspace/` | May consume semantic facts; low risk if confidence levels are already used | Verify that workspace symbol resolution still works with mixed-confidence edges |

### Downstream Crates

- `perl-semantic-analyzer` (this change): Core change, contained
- `perl-lsp-rs`, `perl-lsp-definition`, `perl-lsp-hover` (downstream consumers): Low risk; already handle confidence/provenance filtering
- `perl-workspace` (workspace symbol indexing): Low risk; can ignore low-confidence edges if needed

### Must-Not-Touch Boundary

- Parser (`crates/perl-parser/`, `crates/perl-lexer/`): No changes; extractor just consumes AST
- Semantic facts vocabulary (`crates/perl-semantic-facts/`): No changes; reuse existing Confidence/Provenance variants
- DAP, LSP server binary (`crates/perl-dap/`, `crates/perl-lsp-rs/`): No changes
- Module resolution (`crates/perl-module-*/`): No changes
- Test corpus, fixtures: No changes

## Coverage Notes

- **New test cases:** 5-6 new test functions in the package_graph_extractor test module
- **Existing test coverage:** All existing static @ISA tests must continue to pass
- **Integration coverage:** Optional follow-up: verify that LSP goto-definition gracefully handles low-confidence inherited-method candidates (not required for this PR)
