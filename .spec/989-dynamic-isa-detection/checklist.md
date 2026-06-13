# Implementation Checklist: #989 — dynamic @ISA (push @ISA, $var / @ISA = $computed) silently dropped

## Change order (compiles at each step)

### Step 1: Extend `collect_names_from_node` to handle `NodeKind::Variable`
- **File:** `crates/perl-semantic-analyzer/src/analysis/package_graph_extractor.rs`
- **Change:** Replace the `_ => Vec::new()` arm with explicit match arms for dynamic nodes (Variable, FunctionCall, etc.)
- **Details:** 
  - Add a new match arm before the final `_`: `NodeKind::Variable { .. } => Vec::new()` (placeholder for dynamic variable detection)
  - Add a new match arm: `NodeKind::FunctionCall { .. } => Vec::new()` (for computed expressions)
  - Keep the final `_ => Vec::new()` for truly unhandled cases
  - Function signature stays the same: `fn collect_names_from_node(node: &Node) -> Vec<String>`
- **Rationale:** Explicit arms make the dynamic case discoverable; returning empty vec signals "don't extract names from this" which allows the caller to recognize the dynamic boundary
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 2: Update the function to return an enum indicating extraction status
- **File:** `crates/perl-semantic-analyzer/src/analysis/package_graph_extractor.rs`
- **Change:** Create an internal enum `NameExtractionResult` with variants `Literal(Vec<String>)` and `Dynamic`
- **Details:**
  ```rust
  enum NameExtractionResult {
      Literal(Vec<String>),
      Dynamic,
  }
  ```
- **Rationale:** Allows the caller to distinguish "we found 0 literals" from "this is a dynamic expression"
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 3: Refactor `collect_names_from_node` to return `NameExtractionResult`
- **File:** `crates/perl-semantic-analyzer/src/analysis/package_graph_extractor.rs`
- **Change:** Update function signature and all match arms to return `NameExtractionResult`
- **Details:**
  - Signature becomes: `fn collect_names_from_node(node: &Node) -> NameExtractionResult`
  - String arm: return `NameExtractionResult::Literal(vec![trimmed.to_string()])`
  - Identifier arm: return `NameExtractionResult::Literal(vec![...])`
  - ArrayLiteral arm: recursively collect and merge, return `NameExtractionResult::Literal(...)`
  - Variable arm: return `NameExtractionResult::Dynamic`
  - FunctionCall arm: return `NameExtractionResult::Dynamic`
  - Default arm: return `NameExtractionResult::Literal(Vec::new())` (truly unrecognized — treat as zero literals, not dynamic)
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 4: Update `collect_names_from_args` to propagate dynamic signals
- **File:** `crates/perl-semantic-analyzer/src/analysis/package_graph_extractor.rs`
- **Change:** Update to work with the new `NameExtractionResult` enum
- **Details:**
  - Signature becomes: `fn collect_names_from_args(args: &[Node]) -> NameExtractionResult`
  - If any argument is `Dynamic`, return `Dynamic` (one dynamic argument makes the whole args list dynamic)
  - If all are `Literal`, merge and return `Literal(...)`
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 5: Update all call sites of `collect_names_from_node` and `collect_names_from_args`
- **File:** `crates/perl-semantic-analyzer/src/analysis/package_graph_extractor.rs`
- **Change:** Handle both `Literal` and `Dynamic` variants
- **Details:**
  - Line ~108 (VariableDeclaration with @ISA): 
    ```rust
    match Self::collect_names_from_node(init) {
        NameExtractionResult::Literal(names) => {
            for name in names { self.emit_edge(..., Confidence::High); }
        }
        NameExtractionResult::Dynamic => {
            // Will be handled in Step 6
        }
    }
    ```
  - Line ~124 (Assignment with @ISA): same pattern
  - Line ~161 (push @ISA): same pattern, but iterate over args[1:] handling each result
  - Line ~177 (extends): same pattern
  - Line ~190 (with): same pattern
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 6: Emit low-confidence edge for dynamic @ISA
- **File:** `crates/perl-semantic-analyzer/src/analysis/package_graph_extractor.rs`
- **Change:** Add a new helper method `emit_dynamic_edge` and call it when dynamic @ISA is detected
- **Details:**
  - New method signature: `fn emit_dynamic_edge(&mut self, anchor_id: AnchorId) -> ()`
  - Behavior: Emit an edge with `to_package: "<dynamic>"` (or a sentinel value), `kind: PackageEdgeKind::DependsOn`, `confidence: Confidence::Low`, `provenance: Provenance::DynamicBoundary`
  - Alternative representation: Emit an occurrence fact with `OccurrenceKind::DynamicBoundary` (this requires importing OccurrenceFact and would be a more complex change; **recommend simpler option for MVP**: emit a PackageEdge with sentinel `to_package` and `DynamicBoundary` provenance)
  - Call this in the `Dynamic` branches of Step 5
- **Rationale:** Records the dynamic boundary so the workspace knows "we found dynamic @ISA, confidence is low" instead of "no parent" 
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 7: Add test cases for dynamic @ISA patterns
- **File:** `crates/perl-semantic-analyzer/src/analysis/package_graph_extractor.rs` (test module)
- **Change:** Add new test functions in the `#[cfg(test)]` block
- **Details:**
  - `test_push_isa_with_variable()` — parse `push @ISA, $base_class;` and verify a low-confidence edge is emitted
  - `test_isa_assignment_with_variable()` — parse `@ISA = $computed;` and verify edge is emitted
  - `test_isa_assignment_with_function_call()` — parse `@ISA = get_parents();` and verify edge is emitted
  - `test_push_isa_mixed_literals_and_variables()` — parse `push @ISA, 'Base', $extra;` and verify edges emitted with correct confidence levels (first literal: High, variable part: Low or dynamic marker)
- **Test expectations:** 
  - Edges should be present (not dropped)
  - Confidence should be Low (for dynamic parts)
  - Provenance should be DynamicBoundary or similar
- **Verify:** `cargo test -p perl-semantic-analyzer package_graph_extractor::tests`

### Step 8: Update module documentation
- **File:** `crates/perl-semantic-analyzer/src/analysis/package_graph_extractor.rs`
- **Change:** Update the module-level docstring table to document dynamic @ISA support
- **Details:** 
  - Add rows for the new patterns:
    - `push @ISA, $var` → `Inherits` (Low confidence) or `DependsOn` (sentinel)
    - `@ISA = $computed` → `Inherits` (Low confidence) or `DependsOn` (sentinel)
  - Or alternatively document that these patterns are now detected with a low-confidence dynamic marker
- **Verify:** `cargo check -p perl-semantic-analyzer`

### Step 9: Final verification
- **Verify:** 
  ```bash
  cargo test -p perl-semantic-analyzer
  cargo xtask fmt
  cargo clippy -p perl-semantic-analyzer
  ```

## Callers and consumers

- `collect_names_from_node` is called from:
  - `VariableDeclaration` arm (line ~108)
  - `Assignment` arm (line ~124)
  - `push`/`extends`/`with` expression statement handler (lines ~161, ~177, ~190)
  - `collect_names_from_args` (line ~330)

- `PackageGraphExtractor::extract` is called from:
  - `crates/perl-semantic-analyzer/src/analysis/semantic/mod.rs` (main analyzer initialization)
  - Tests in the same file

## Scope boundary

**Files IN scope:**
- `crates/perl-semantic-analyzer/src/analysis/package_graph_extractor.rs` (primary change)

**Files OUT of scope:**
- `crates/perl-semantic-facts/src/lib.rs` (types already exist; no changes needed)
- `crates/perl-semantic-analyzer/src/analysis/semantic/mod.rs` (consumer of package_edges; no changes needed for MVP)
- LSP providers that consume package edges (not affected; they already handle confidence levels)
- DAP, parser, or workspace indexing (not touched)

## Flags for builder

1. **Sentinel value for dynamic @ISA:** The spec mentions emitting a low-confidence edge. The builder must choose between:
   - Option A (simpler): Emit `PackageEdge` with `to_package: "<dynamic>"`, `kind: DependsOn`, `confidence: Low`, `provenance: DynamicBoundary`
   - Option B (more structured): Create an `OccurrenceFact` with `OccurrenceKind::DynamicBoundary` instead of a PackageEdge (requires more refactoring; recommend deferring to follow-up issue)
   - **Recommendation:** Go with Option A for this PR to minimize scope.

2. **Mixed static + dynamic args:** For `push @ISA, 'Base', $var`:
   - Builder must decide: emit one High-confidence edge for 'Base' + one Low-confidence or Dynamic edge for $var?
   - **Recommendation:** Emit separate edges with appropriate confidence for each.

3. **Upstream consumers:** The semantic analyzer stores `package_edges` but does not currently expose them as OccurrenceFacts. If LSP providers need to distinguish dynamic boundaries, that's a follow-up issue (#894 related).

4. **Testing strategy:** The red TDD builder should write tests before implementing. Each test should verify both that the edge is emitted and that confidence/provenance are correct.
