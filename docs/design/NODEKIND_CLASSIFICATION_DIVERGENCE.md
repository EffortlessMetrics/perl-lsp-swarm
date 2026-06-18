# NodeKind Classification Divergence — Evidence Base

**Issue**: #914 (evidence base for centralized classification in #911)  
**Filed**: 2026-06-18  
**Status**: Active — feeds `NodeKindFlags` design in #911

---

## Problem

Six crates across perl-lsp independently implement overlapping heuristics to classify
`NodeKind` variants as executable, declaration-introducing, or scope-creating. These
hand-matched lists drift independently and produce user-visible inconsistencies (e.g.,
DAP treats a line as breakpoint-able while semantic tokens classify it differently).

This document is the **evidence base** for the centralized classification proposed
in issue #911 (`NodeKindCategory` + `NodeKindFlags`). It does not propose the API.

---

## Divergence 1 — "Executable Line" Heuristic

**Impact: HIGH** — affects debugger UX (breakpoint placement).

### Side A — DAP breakpoint validator

**File**: `crates/perl-dap/src/breakpoint/validator.rs`

| Lines | Function | Classification rule |
|-------|----------|---------------------|
| 209–224 | `is_comment_or_blank_line` | Text-based fast path (blank or `#`) then delegates to AST check |
| 231–255 | `has_only_comments_in_range_node` | Matches only `NodeKind::Program { statements }` — if no child statements overlap the byte range, line is **non-executable** (blank/comment) |
| 252–253 | `_` catch-all | Any non-Program node means **executable code present** (returns `false`) |
| 265 | `is_inside_heredoc_interior_node` | `NodeKind::Heredoc { body_span: Some(span), .. }` — **heredoc interior is non-executable** |

```
// crates/perl-dap/src/breakpoint/validator.rs:241-254
match &node.kind {
    NodeKind::Program { statements } => {
        let nodes_in_range: Vec<_> = statements
            .iter()
            .filter(|s| s.location.start < end && s.location.end > start)
            .collect();
        nodes_in_range.is_empty()   // true ⇒ non-executable
    }
    _ => false,   // any other node ⇒ executable
}
```

### Side B — Semantic tokens provider

**File**: `crates/perl-lsp-rs/src/features/semantic_tokens_provider.rs`

There is **no centralized `is_executable` concept**. Executability is inferred
implicitly by token presence: nodes that emit semantic tokens are assumed to represent
executable code; nodes that emit no tokens are ignored. Adding a new `NodeKind` variant
requires manually deciding whether it should emit tokens — the DAP validator is not consulted.

### Divergence

| Question | DAP (`validator.rs:241`) | Semantic tokens |
|----------|--------------------------|-----------------|
| Is `use Foo;` executable? | Yes — `Use` falls to `_ => false` inside `Program`, so the Program node returns `!is_empty()` = true | Treated as a namespace reference; `Namespace/Reference` token emitted |
| Is a heredoc body line executable? | No — `is_inside_heredoc_interior_node` checks `body_span` | Not modeled; heredoc lines would silently emit no tokens |
| New `NodeKind` variant | Must teach DAP checker explicitly | Must teach token emitter explicitly — independently |

---

## Divergence 2 — "Declaration Context" Heuristic

**Impact: MEDIUM** — affects semantic highlighting and "go to declaration".

### Side A — Semantic tokens provider

**File**: `crates/perl-lsp-rs/src/features/semantic_tokens_provider.rs:207-384`

The provider threads `is_declaration_context: bool` through the visitor (line 211).

| NodeKind variant | Lines | Classification |
|-----------------|-------|----------------|
| `Package { name, .. }` | 214–227 | Module name → `Namespace/Declaration` |
| `Subroutine { name, .. }` | 230–254 | Sub name → `Function/Declaration+Definition` |
| `VariableDeclaration { variable, .. }` | 267–284 | Variable → `Variable/Declaration` |
| `Variable { .. }` | 257–264 | `Modification` if in declaration context, else `Reference` |
| `Use { module, .. }` | 340–348 | Module name → **`Namespace/Reference`** (not Declaration) |

```
// crates/perl-lsp-rs/src/features/semantic_tokens_provider.rs:340-348
NodeKind::Use { module, .. } => {
    self.add_token_from_string(
        module,
        SemanticTokenType::Namespace,
        vec![SemanticTokenModifier::Reference],   // ← Reference, not Declaration
        tokens,
        node,
    );
}
```

### Side B — Scope analyzer

**File**: `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs:660-661`  
**File**: `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/declarations.rs:159-211`

The scope analyzer routes `NodeKind::Use` through the **declarations** module, treating
it as a declaration-level construct:

```
// crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs:660-661
NodeKind::Use { module, args, .. } => {
    declarations::handle_use(self, node, module, args, scope, context);
}
```

For `use vars qw($x $y)`, `handle_use` declares the listed variables in the current
scope (declarations.rs:168–210). For other `use Foo;` forms the handler is a no-op,
but the **routing decision** — placing `Use` in the declarations dispatch path — makes
it semantically a binding-level statement from the analyzer's perspective.

| NodeKind variant | Semantic tokens (`semantic_tokens_provider.rs`) | Scope analyzer (`scope_analyzer/mod.rs`) |
|-----------------|------------------------------------------------|------------------------------------------|
| `Use { module }` | `Reference` — module is a lookup, not a binding | Routed to `declarations` module — may introduce bindings (`use vars`) |

### Divergence — `use Foo;`

```
use Foo;
```

- **Semantic tokens** (line 340–348): module name `"Foo"` tagged as `Namespace/Reference`. Not a declaration.
- **Scope analyzer** (line 660–661): routed to `declarations::handle_use`. For non-`vars` imports, currently a no-op — but the **architectural classification** is "declaration-level," not "reference."

This diverges on the conceptual question: is `use Foo;` a declaration (binding introducer)
or a reference (lookup)? The two systems give opposite answers.

---

## Divergence 3 — "Scope-Introducing / Foldable" Heuristic

**Impact: MEDIUM-HIGH** — affects code folding, document symbols, and scope analysis accuracy.

### Side A — Folding provider

**File**: `crates/perl-lsp-rs-core/src/providers/folding/mod.rs:106-314`

The folding provider's `visit_node` function (line 106) treats the following as
**foldable scope boundaries** (each calls `self.add_range_from_node`):

| NodeKind | Lines | Foldable? |
|----------|-------|-----------|
| `Package` | 158–168 | Yes (always) |
| `Subroutine \| Method` | 170–175 | Yes |
| `Block` | 177–185 | Yes (if non-empty) |
| `If` | 187–197 | **Yes** |
| `While` | 199–205 | **Yes** |
| `For \| Foreach` | 207–211 | Yes |
| `Do \| Eval \| Defer` | 213–216 | Yes |
| `Try` | 218–227 | Yes |
| `Given` | 229–232 | **Yes** |
| `PhaseBlock` | 234–238 | Yes |
| `Class` | 240–243 | Yes |
| `Heredoc` | 246–249 | Yes (as `FoldingRangeKind::Region`) |
| `ArrayLiteral \| HashLiteral` | 256–276 | Yes (if non-empty) |
| `Use \| No` (grouped) | 115–155 | Yes (consecutive imports → `FoldingRangeKind::Imports`) |

`If`, `While`, and `Given` are treated as first-class scope boundaries for folding.

### Side B — Scope analyzer's explicit scope constructors

**File**: `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/scope_constructs.rs`

The scope analyzer has dedicated handler functions that **create child scopes** via
`Rc::new(Scope::with_parent(scope.clone()))`:

| NodeKind | Handler function | Lines | Creates child scope? |
|----------|-----------------|-------|----------------------|
| `Block` | `handle_block` | 12–28 | **Yes** |
| `PhaseBlock` | `handle_phase_block` | 31–45 | **Yes** |
| `For` | `handle_for` | 49–79 | **Yes** |
| `Foreach` | `handle_foreach` | 83–112 | **Yes** |
| `Subroutine` | `handle_subroutine` | 115–234 | **Yes** |
| `Try` | `handle_try` | 238–293 | **Yes** (per catch block) |
| `Package` | `handle_package` | 296–328 | **Yes** (block form only) |

`If`, `While`, and `Given` are **not handled** by any `scope_constructs` function.
They fall through to the catch-all in `scope_analyzer/mod.rs:870-877`:

```
// crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs:870-877
_ => {
    // Recursively analyze children
    ancestors.push(node);
    for child in node.children() {
        self.analyze_node(child, scope, ancestors, issues, context);
    }
    ancestors.pop();
}
```

This means `If`, `While`, and `Given` bodies are analyzed in the **parent scope** —
variables declared inside them are visible outside (reflecting Perl's actual scoping
semantics), but the folding provider considers these as scope-introducing constructs.

### Divergence

| NodeKind | Folding provider | Scope analyzer |
|----------|-----------------|----------------|
| `If { then_branch }` | Foldable (scope boundary) — lines 187–197 | Transparent — children analyzed in parent scope (mod.rs:870) |
| `While { body }` | Foldable (scope boundary) — lines 199–205 | Transparent — catch-all (mod.rs:870) |
| `Given { body }` | Foldable (scope boundary) — lines 229–232 | Transparent — catch-all (mod.rs:870) |
| `Try { body }` | Foldable — lines 218–227 | **Creates scope per catch block** — scope_constructs.rs:238–293 |
| `Block` | Foldable (if non-empty) — lines 177–185 | **Creates child scope** — scope_constructs.rs:12–28 |

`Try` is interesting: both systems agree it's a scope boundary, but differ on what's
inside. The folding provider folds `body` + `catch_blocks` + `finally_block` all as
one region; the scope analyzer creates a separate child scope per catch block but
analyzes `body` and `finally_block` in the parent scope.

---

## Summary Table for #911 (`NodeKindFlags` design)

The following flags are needed to subsume these three divergent heuristics:

| Flag | What it replaces | Primary consumers |
|------|-----------------|-------------------|
| `executable` | DAP's `has_only_comments_in_range_node` (validator.rs:241) | `perl-dap` |
| `safe_for_breakpoint` | Heredoc body filter (validator.rs:265) | `perl-dap` |
| `declares_symbol` | Semantic tokens' declaration context threading (semantic_tokens_provider.rs:211) + scope analyzer's declaration routing (scope_analyzer/mod.rs:633–661) | `perl-lsp-rs`, `perl-semantic-analyzer` |
| `references_symbol` | Semantic tokens' Reference modifier on `Use` (semantic_tokens_provider.rs:345) | `perl-lsp-rs` |
| `introduces_scope` | Scope constructs' child-scope creation (scope_constructs.rs:12–328) | `perl-semantic-analyzer` |
| `foldable_region` | Folding provider's 17-variant foldable list (folding/mod.rs:107–314) | `perl-lsp-rs-core` |
| `recovery_artifact` | N/A (no current consumer — gap to fill) | All consumers |

---

## Non-goals

This document does **not** propose:
- The `NodeKindFlags` or `NodeKindCategory` API — that is issue #911
- Changes to `NodeKind` enum variants
- Consumer migration — those are later sequential builder issues

It establishes the **verified file:line evidence** for the three divergence types
that the centralization in #911 must subsume.

---

## References

- Issue #914 (this evidence base)
- Issue #911 (centralized classification design: `NodeKindCategory` + `NodeKindFlags`)
- Issue #710 (related: `unless` detected via `If(unary_not(...))` — same fragility)
- `crates/perl-dap/src/breakpoint/validator.rs`
- `crates/perl-lsp-rs/src/features/semantic_tokens_provider.rs`
- `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs`
- `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/scope_constructs.rs`
- `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/declarations.rs`
- `crates/perl-lsp-rs-core/src/providers/folding/mod.rs`
