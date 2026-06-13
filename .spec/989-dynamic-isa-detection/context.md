# Context: #989 — Dynamic @ISA Detection

## Problem Statement

The package-graph extractor silently drops dynamic `@ISA` mutations (`push @ISA, $var` and `@ISA = $computed`) with no edge AND no low-confidence / `DynamicBoundary` marker. The workspace then believes the package has no parents — pretending dynamic Perl is statically known instead of recording "unknown."

**Root cause:** `collect_names_from_node` (line 305-326) has a catch-all arm `_ => Vec::new()` that returns an empty vec for any unhandled node kind, including `Variable` and `FunctionCall`. The caller cannot distinguish between "we found zero literal parent names" and "this is a dynamic expression."

**Impact on users:**
- Editor: with dynamic `@ISA`, inherited-method goto/hover candidates are silently missing, with no hint that inheritance is dynamic
- Refactoring: rename / safe-delete plans may treat a symbol as safe when its inheritance chain is actually dynamic
- Workspace facts principle violation: "unknown is acceptable; pretending dynamic Perl is statically known is not"

## Decision Log

### Decision 1: Detect all dynamic @ISA mutations, emit low-confidence edge

**Chosen approach:** Extend `collect_names_from_node` to recognize `Variable` and `FunctionCall` nodes and signal that the parent names are dynamic. Emit a low-confidence edge with a sentinel target package (e.g., `<dynamic>`) or provenance marker.

**Rationale:**
- Aligns with the principle "unknown is acceptable" — recording "dynamic inheritance detected" is better than silence
- Minimal scope: single-file change in package_graph_extractor
- Reuses existing `Confidence::Low` and `Provenance::DynamicBoundary` enums (no type changes)
- Allows downstream (LSP providers, rename safety checks) to recognize dynamic boundaries and behave accordingly

**Alternative rejected:** Create a separate `OccurrenceFact` with `OccurrenceKind::DynamicBoundary` instead of PackageEdge
- Pro: More semantically correct (dynamic boundary is an occurrence, not an edge)
- Con: Requires changes to semantic analyzer storage, serialization, and all downstream consumers
- Decision: Defer to follow-up issue #894 (missing first-class Inheritance/UseLib facts vocabulary); this PR focuses on detecting and recording the boundary at the extractor level

### Decision 2: Use internal enum to distinguish Literal vs Dynamic extraction result

**Chosen approach:** Create an internal `NameExtractionResult` enum with `Literal(Vec<String>)` and `Dynamic` variants. Refactor `collect_names_from_node` and `collect_names_from_args` to return this enum instead of `Vec<String>`.

**Rationale:**
- Makes the dynamic case explicit and discoverable in code (no silent empty-vec fallback)
- Allows call sites to respond differently to dynamic vs. literal cases
- Type-safe: compiler forces all call sites to handle both variants

**Alternative rejected:** Keep returning `Vec<String>` and use an external signal (e.g., a separate method `is_dynamic_node`)
- Pro: Minimal signature changes
- Con: Call sites must manually check both, error-prone, less discoverable
- Decision: Go with enum for clarity

### Decision 3: Mixed static + dynamic arguments

**Chosen approach:** When `push @ISA, 'Base', $var` is parsed, emit two edges:
1. `Child → Base` with `Inherits` and `High` confidence (static literal)
2. `Child → <dynamic>` with `DependsOn` and `Low` confidence (or `Confidence::Low` + `Provenance::DynamicBoundary`)

**Rationale:**
- Preserves high-confidence knowledge about 'Base' while signaling the dynamic part
- Matches real-world Perl code where some parents are static, others computed at runtime
- Allows downstream to use high-confidence edges while being aware of dynamic boundaries

**Alternative rejected:** Mark the entire inheritance as Low confidence if any argument is dynamic
- Pro: Simpler logic (all-or-nothing)
- Con: Loses useful static information when mixed with dynamic
- Decision: Emit separate edges per argument

### Decision 4: Sentinel representation for dynamic @ISA

**Two options:**
1. **Sentinel package name** (simpler, MVP): Use `to_package: "__DYNAMIC__"` or similar, `kind: DependsOn`, `confidence: Low`, `provenance: DynamicBoundary`
2. **Enum variant** (more structured, future-proof): Add `PackageEdgeKind::DynamicInherits` variant (requires type change, not minimal scope)

**Chosen:** Option 1 for this PR (minimal scope).

**Rationale:**
- Requires no type changes to `PackageEdgeKind`
- Reuses existing enums (Confidence, Provenance)
- Downstream can filter edges by `to_package != "__DYNAMIC__"` or by `confidence == Low`
- Clear sentinel cannot collide with real Perl package names

**Note:** A follow-up issue can upgrade this to a proper enum variant.

## Objections Addressed

### Objection: "Silent drop is fine; LSP providers should filter by confidence"
**Response:**
- The issue is not LSP filtering; it's that the extractor *never records* the dynamic boundary at all
- There's no low-confidence edge to filter — the workspace has no record that inheritance is dynamic
- The fix ensures the boundary is recorded, giving downstream the *choice* to filter or warn
- Aligns with the workspace-facts principle (see context.md §Research Findings)

### Objection: "This is a parser problem; we shouldn't fix it in the extractor"
**Response:**
- The parser correctly parses `push @ISA, $var` into an AST with a Variable node
- The extractor is *supposed* to extract parent names from the AST — that includes recognizing when the source is dynamic
- This is a semantic-analyzer responsibility, not parser responsibility
- Parser's job: produce correct AST; Semantic analyzer's job: interpret it and signal uncertainty

### Objection: "Shouldn't we distinguish between require $var and push @ISA, $var?"
**Response:**
- Good point. `require $var` is a different boundary (module name is dynamic) vs. `@ISA = $var` (parent list is dynamic)
- Both should be marked as dynamic boundaries, but with different edge kinds or provenance details
- For this PR: use `DependsOn` for dynamic @ISA (conservative); follow-up can use `Inherits` if preferred
- This PR handles @ISA specifically; require dynamic is already handled elsewhere

## Research Findings

### Finding 1: Workspace-facts principle
**Source:** `.claude/worktrees/*/docs/reference/ORCHESTRATION_DOCTRINE.md` and issue #894 (Missing first-class Inheritance/UseLib facts vocabulary)

**Claim:** "unknown is acceptable; pretending dynamic Perl is statically known is not"

**Verification:** Confirmed in perl-lsp's design philosophy. Confidence and Provenance enums exist specifically to record degrees of certainty. Silent drops violate this principle.

### Finding 2: OccurrenceKind::DynamicBoundary exists
**Source:** `crates/perl-semantic-facts/src/lib.rs:62`

**Claim:** DynamicBoundary is already defined as an OccurrenceKind enum variant.

**Verification:** Confirmed. It's used by the import extractor for `require $var` (per issue finding).

### Finding 3: No tests for dynamic @ISA
**Source:** Grep of `crates/perl-semantic-analyzer/src/analysis/package_graph_extractor.rs` tests

**Claim:** All existing tests cover static @ISA patterns only.

**Verification:** Confirmed. Tests include `use parent 'Base'`, `use base`, `@ISA = (...)`, `our @ISA = qw(...)`, `push @ISA, 'Base'`, but none with variables or computed expressions.

### Finding 4: Static string case works
**Source:** `crates/perl-semantic-analyzer/src/analysis/package_graph_extractor.rs:440-459` (test_push_isa_single, test_push_isa_multiple)

**Claim:** `push @ISA, 'Base'` produces high-confidence Inherits edge.

**Verification:** Confirmed. The issue notes "The static-string case works, so only the variable/computed case is affected."

## Related Issues

- **#894** — "Missing first-class Inheritance/UseLib facts vocabulary": Proposes a more comprehensive inheritance fact model. This PR is a stepping-stone; it records dynamic boundaries in the current model. Once #894 is implemented, the representation can be upgraded from sentinel edges to dedicated fact types.

- **#812** — "Static multi-file @ISA method resolution": Separate issue about resolving inherited methods across files. This PR supports that work by ensuring dynamic @ISA is marked as such (so #812 knows when to give up or use lower-confidence hints).

- **#964** — "DAP: clear frames on resume" (unrelated; mentions in recent commits, not blocking this work)

## Technical Notes

### NodeKind Exhaustiveness
The `collect_names_from_node` match statement currently has a catch-all `_ => Vec::new()` arm. After this PR, it should explicitly match:
- `NodeKind::String { value, .. }` — extract quoted string
- `NodeKind::Identifier { name }` — extract identifier or qw(...) string
- `NodeKind::ArrayLiteral { elements }` — recursively extract from array elements
- `NodeKind::Variable { .. }` — signal Dynamic
- `NodeKind::FunctionCall { .. }` — signal Dynamic
- `_` — return Literal(Vec::new()) (truly unrecognized, treat as zero literals)

This makes the code more maintainable (future parser changes will show up as unhandled arms).

### Confidence vs. Provenance
- **Confidence:** How sure are we in this fact? High / Medium / Low
- **Provenance:** How was this fact inferred? ExactAst / DynamicBoundary / etc.
- A dynamic @ISA should be: `confidence: Low` (we don't know the exact parent names) + `provenance: DynamicBoundary` (we detected a dynamic expression)
- Both fields are complementary, not contradictory.

### Future Work
1. **#894**: Upgrade sentinel `__DYNAMIC__` edge to proper `PackageEdgeKind::DynamicInherits` enum variant
2. Follow-up issue to handle other dynamic inheritance patterns (e.g., `@ISA = @{$config{parents}}`)
3. Extend OccurrenceFact model to emit DynamicBoundary occurrences (not just PackageEdges)
4. LSP provider enhancement: when presenting goto-definition for inherited method, include low-confidence candidates from dynamic inheritance with a caveat ("may be inherited; inheritance is dynamic")
