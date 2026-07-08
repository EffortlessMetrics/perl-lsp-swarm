# NodeKind Classification Divergence — Evidence Table

**Purpose:** Evidence base for issue #914 and the centralized `NodeKindFlags` API in issue #911.

Six crates across `perl-lsp` independently implement overlapping heuristics for classifying
`NodeKind` variants as executable, declaration-introducing, or scope-creating. Each hand-matched
list drifts independently: adding a new `NodeKind` variant requires separately teaching each
consumer whether the variant is executable, creates a declaration, or introduces a scope boundary.

This document pins the **three confirmed divergences** with verified `file:line` references so
that the #911 design can target the correct contracts. All references verified against `main`
HEAD as of 2026-07-08.

---

## Summary table

| Heuristic | DAP breakpoint validator | Semantic analysis | Scope analyzer | Folding provider |
|-----------|--------------------------|-------------------|----------------|------------------|
| **Executable line** | Structural: Program.statements overlap → executable; `_` → executable | No concept; all nodes emit tokens | No concept | No concept |
| **Declaration context** | No concept | `Use` → `Namespace` token (reference) | `Use` → `declarations::handle_use` (may create bindings) | No concept |
| **Scope-introducing** | No concept | No concept | Block, PhaseBlock, For, Foreach, Subroutine, Try, Package only | If, While, Given, Do, Eval, Defer, Try, For, Foreach, Subroutine, Package, PhaseBlock, Class |

---

## Divergence 1 — "Executable line" (HIGH impact)

A line is executable if it contains a breakpointable statement. Consumers disagree on what
constitutes such a statement, and no shared predicate exists.

### DAP breakpoint validator

**File:** `crates/perl-dap/src/breakpoint/validator.rs:235–255`

```
fn has_only_comments_in_range_node(&self, node: &Node, start: usize, end: usize) -> bool {
    match &node.kind {
        NodeKind::Program { statements } => {
            // If no statement nodes overlap the byte range → blank/comment line
            nodes_in_range.is_empty()
        }
        // Any other node type → there is executable code (return false)
        _ => false,
    }
}
```

- **Line 242:** `NodeKind::Program { statements }` — the only structural match; all other
  NodeKind variants return `false` (i.e., "not only comments → executable").
- **Line 265** (separate function `is_inside_heredoc_interior_node`): `NodeKind::Heredoc {
  body_span: Some(span), .. }` — body interior check to suppress breakpoints inside raw heredoc
  content.

The check is purely structural and positional: "any AST node overlapping this byte range in a
`Program.statements` list → executable." There is no per-variant `is_executable` flag. A new
`NodeKind` variant that should be non-executable (e.g., a POD block, a pure comment node) must
either not appear under `Program.statements` or be explicitly excluded.

### Semantic analysis

**File:** `crates/perl-semantic-analyzer/src/analysis/semantic/node_analysis.rs`

No concept of executability. Every node that has a source location produces semantic tokens.
The `node_analysis.rs` visitor does not check whether a node is breakpointable before emitting a
token.

### Divergence

There is no shared `is_executable` predicate or `NodeKind` flag. Adding a new `NodeKind`
variant requires manually deciding in the DAP validator whether the variant is executable. The
semantic analysis layer has no corresponding concept to keep in sync.

**Consequence:** A new construct (e.g., a type annotation, a compile-time const expression)
that the semantic analysis tokens correctly could silently become unbreakable in the DAP client
unless someone remembers to audit the validator's `_` catch-all.

---

## Divergence 2 — "Declaration context" (MEDIUM impact)

Does `use Foo;` introduce a declaration or a reference? The semantic analysis and scope analysis
layers give contradictory answers.

### Semantic analysis (node_analysis.rs)

**File:** `crates/perl-semantic-analyzer/src/analysis/semantic/node_analysis.rs:674–695`

```
NodeKind::Use { module, args, .. } => {
    // Emit "use" keyword token
    self.semantic_tokens.push(SemanticToken {
        token_type: SemanticTokenType::Keyword, ...
    });
    // Emit module name as Namespace token (reference, no declaration modifiers)
    self.semantic_tokens.push(SemanticToken {
        token_type: SemanticTokenType::Namespace,
        modifiers: vec![],
        ...
    });
}
```

- **Line 686–690:** `NodeKind::Use { module }` → module name tagged as `SemanticTokenType::Namespace`
  with **no modifiers** (no `Declaration`, no `Definition`). The semantic analysis layer treats
  `use Foo;` as a reference to a namespace, not as introducing a declaration in the current scope.

### Scope analyzer

**File:** `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs:676–677`

```
NodeKind::Use { module, args, .. } => {
    declarations::handle_use(self, node, module, args, scope, context);
}
```

**File:** `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/declarations.rs:271–320`

```
/// Handle `NodeKind::Use` — register `use vars` variable declarations.
pub(super) fn handle_use(analyzer, node, module, args, scope, context) {
    if module == "vars" {
        // parse qw($x $y) and declare variables in current scope
        analyzer.declare_variable_parts_in_context(scope, sigil, name, ...);
    }
    // ... also handles other forms
}
```

- **Line 676–677:** `NodeKind::Use` is routed to the **declarations** module — it is treated as
  a declaration-level construct, not a pure reference.
- **Lines 271–320:** For `use vars qw($x $y)` it creates variable bindings in the current scope.
  `use Foo;` passes through `handle_use` and may create no bindings, but the **routing** places
  it in the declaration pipeline, not the reference pipeline.

### Divergence on `use Foo;`

| Consumer | What it says about `use Foo;` |
|----------|-------------------------------|
| Semantic analysis | Module name `Foo` is a `Namespace` *reference* (no declaration modifier) |
| Scope analyzer | The entire `Use` node is a *declaration-level construct* (routed to `declarations::handle_use`) |

No shared `declares_symbol` predicate. If a new form of `use` (e.g., `use constant`) needs
special declaration treatment, both layers must be taught independently, with no common contract
to keep them consistent.

---

## Divergence 3 — "Scope-introducing / foldable" (MEDIUM-HIGH impact)

Does `if ($cond) { ... }` introduce a new scope boundary? The folding provider and scope
analyzer give different answers — reflecting a legitimate semantic distinction (Perl `if` does
not create a new lexical scope) but creating a structural divergence in the codebase.

### Folding provider

**File:** `crates/perl-lsp-rs-core/src/providers/folding/mod.rs:248–293`

```
NodeKind::If { condition: _, then_branch, elsif_branches, else_branch, .. } => {
    self.add_range_from_node(node, None);   // ← foldable scope boundary
    ...
}
NodeKind::While { condition: _, body, continue_block, .. } => {
    self.add_range_from_node(node, None);   // ← foldable scope boundary
    ...
}
NodeKind::Given { expr: _, body } => {
    self.add_range_from_node(node, None);   // ← foldable scope boundary
    ...
}
```

- **Line 248:** `NodeKind::If` → `add_range_from_node` (foldable boundary)
- **Line 260:** `NodeKind::While` → `add_range_from_node` (foldable boundary)
- **Line 290:** `NodeKind::Given` → `add_range_from_node` (foldable boundary)

The folding provider treats If, While, Given, Do, Eval, Defer, Try, For, Foreach, Class,
Subroutine, Package, PhaseBlock, and Block as first-class scope boundaries that create foldable
regions.

### Scope analyzer

**File:** `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/scope_constructs.rs:12–328`

Dedicated scope-creating handlers exist for exactly seven NodeKind families:

| Handler | NodeKind | Line |
|---------|----------|------|
| `handle_block` | `NodeKind::Block` | 12 |
| `handle_phase_block` | `NodeKind::PhaseBlock` | 31 |
| `handle_for` | `NodeKind::For` | 49 |
| `handle_foreach` | `NodeKind::Foreach` | 83 |
| `handle_subroutine` | `NodeKind::Subroutine` / `Method` | 115 |
| `handle_try` | `NodeKind::Try` | 238 |
| `handle_package` | `NodeKind::Package` | 296 |

**File:** `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs:886–893`

```
_ => {
    // Recursively analyze children
    ancestors.push(node);
    for child in node.children() {
        self.analyze_node(child, scope, ancestors, issues, context);
    }
    ancestors.pop();
}
```

- **Line 886–893:** `NodeKind::If`, `NodeKind::While`, `NodeKind::Given` have no explicit arm
  and fall through to the `_` catch-all. Their children are analyzed **in the parent scope** —
  no child scope is created.

### Divergence on `if ($x) { ... }`

| Consumer | What it says about `if ($x) { ... }` |
|----------|---------------------------------------|
| Folding provider | `NodeKind::If` → foldable scope boundary (`add_range_from_node`) |
| Scope analyzer | `NodeKind::If` → transparent container (children in parent scope, via `_` catch-all) |

This divergence reflects real Perl semantics: `if`, `while`, and `given` do **not** introduce
a new lexical scope in Perl (variables declared with `my` inside an `if` block are lexically
scoped to the enclosing `{ }` Block node, not the `If` node itself). The scope analyzer is
**semantically correct**. The folding provider is **structurally correct** for UI purposes.

No shared `introduces_scope` or `is_foldable_boundary` predicate exists to make both
judgments explicit and auditable.

---

## Impact on adding new NodeKind variants

When a new `NodeKind` variant is added, the following independent sites must each be manually
updated — there is no central classification to inherit from:

| Classification | Sites to update |
|----------------|-----------------|
| `is_executable` | `validator.rs:235–255` (verify it does/doesn't appear in Program.statements) |
| `declares_symbol` | `scope_analyzer/declarations.rs` (add handler or confirm catch-all is correct); `node_analysis.rs:674–695` (decide token type and modifiers) |
| `introduces_scope` | `scope_analyzer/scope_constructs.rs` (add handler if needed); `folding/mod.rs:248–293` (add foldable arm if needed) |

---

## Flags needed by the #911 centralized classification

| Flag | Divergence it resolves | Current divergent sites |
|------|------------------------|------------------------|
| `executable` / `safe_for_breakpoint` | Divergence 1 | `validator.rs:235–255` |
| `declares_symbol` | Divergence 2 | `node_analysis.rs:674–695` vs `scope_analyzer/mod.rs:676–677` |
| `introduces_lexical_scope` | Divergence 3 | `scope_constructs.rs` seven handlers vs folding `_` catch-all |
| `is_foldable_boundary` | Divergence 3 | `folding/mod.rs:248–293` |
| `recovery_artifact` | (not divergent yet) | All consumers — needed to prevent recovery nodes from being treated as executable/foldable |

---

## Non-goals of this document

- Does **not** propose the `NodeKindFlags` / `NodeKindCategory` API — that is issue #911.
- Does **not** change any `NodeKind` enum variants or consumer logic.
- Does **not** migrate any consumers — those are later sequential builder issues.
- The ~489 match-site count in issue #914 is directional evidence of duplication; the three
  divergences above are the verified, load-bearing part.

---

## References

- Issue #914: evidence base (this document)
- Issue #911: `NodeKindCategory` + `NodeKindFlags` design
- Issue #710: `unless` detected via `If(unary_not(...))` — same fragility pattern
- PR #1558: prior draft of this document (stale line references; rebuilt with verified locations)
