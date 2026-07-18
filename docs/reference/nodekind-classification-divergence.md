# NodeKind classification divergence — evidence base

**Issue:** [#914](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/914) ·
**Design companion:** [#911](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/911)
**Verified against:** `main` @ `55f8e8bcb16def0f1ba77088adc322be27649682` (2026-07-18)

## Purpose

AST consumers across the workspace independently classify `NodeKind` variants —
"executable vs. not", "declaration vs. reference", "scope-introducing vs. leaf" —
by hand-matching, with subtly different heuristics that drift apart and produce
user-visible inconsistencies. This document is the **evidence base** for the
centralized classification tracked by #911. It does **not** design the API and it
does **not** migrate any consumer; it records, with verified `file:line` anchors,
the concrete divergences a shared classification must subsume.

The `file:line` references in #914's body were written in May 2026 and have since
drifted; every anchor below was re-derived from and rechecked against the `main`
SHA above.

> **Durability note.** The line numbers here are a snapshot at the SHA above and
> **will drift** as code moves. The **named symbols** — functions, `NodeKind`
> variants, and file paths — are the durable references; the line numbers are only
> a convenience. To re-verify, grep for the named function/variant rather than
> trusting a line number verbatim (e.g. `rg 'fn handle_use' crates/perl-semantic-analyzer`).
> A migration builder acting on this evidence should re-anchor against the current
> `main` before editing.

## Headline finding: the classification primitive exists, but its divergence-relevant flags are unwired

`crates/perl-ast/src/classification.rs` already defines the exact shape #911
proposed:

- `NodeKindCategory` (`classification.rs:73`)
- `NodeKindFlags` (`classification.rs:102`)
- `NodeKindFlags::is_executable()` (`classification.rs:947`)
- `NodeKindFlags::introduces_scope()` (`classification.rs:955`)
- `NodeKindFlags::declares_symbol()` (`classification.rs:964`)

The layer is only **partially** wired, and — critically — the three flag
predicates that would subsume the divergences below are **not** consumed:

- `NodeKindCategory` / `classification::category()` **has** a production consumer:
  `crates/perl-lsp-rs/src/runtime/symbol_extraction.rs:55` imports it and
  `:864` documents `category()` as the compile-time-enforcement point.
- `NodeKindFlags::is_executable()` / `introduces_scope()` / `declares_symbol()`
  have **zero** production consumers: a workspace search for `.is_executable()` /
  `.introduces_scope()` / `.declares_symbol()` outside `perl-ast` returns nothing
  (tests aside). (The only other `*classification*` hits — `perl-lexer`'s
  `operator_classification` / `word_classification` — are unrelated lexer-level
  modules, not `perl-ast` classification.)

So the shared *category* vocabulary is starting to be adopted, but the semantic
**flags** that encode "executable / introduces-scope / declares-symbol" — exactly
the axes the three divergences disagree on — are declared and unused. Every
consumer below still rolls its own hand-matched heuristic. **The outstanding work
in this area is consumer migration onto those flags, not primitive design** — the
divergences persist precisely because nothing consumes them.

## Divergence 1 — "executable line" (HIGH impact)

**Side A — DAP breakpoint validator** — `crates/perl-dap/src/breakpoint/validator.rs`

- "any AST node present ⇒ executable" — `has_only_comments_in_range_node`,
  `validator.rs:235-255`: matches `NodeKind::Program { statements }`, treats a
  range with no in-range statement nodes as blank/comment, and every other node
  kind (`_ => false`) as executable.
- Heredoc-interior suppression inspecting `Heredoc { body_span }` —
  `is_inside_heredoc_interior_node`, `validator.rs:265`.
- Blank / comment / POD / heredoc ordering — `validate_with_column`,
  `validator.rs:287-320` (POD regions come from a separate regex scan,
  `find_pod_regions`, `validator.rs:144-173`, not from the AST).
- Public entry `is_executable_line` — trait `validator.rs:90`, impl
  `validator.rs:323`.

**Side B — the centralized flag / semantic tokens**

- The only centralized `is_executable` concept is `classification.rs:947`, which
  the validator does **not** consume.
- The semantic-tokens provider has **no** executability concept at all — it paints
  token types/modifiers and never asks whether a line is executable.

**Divergence:** executability is decided in exactly one place (the DAP validator)
with a positional "any non-`Program` node ⇒ executable" rule, while the
variant-level `executable` flag that could centralize it goes unused. Adding a new
`NodeKind` variant requires manually teaching the DAP validator whether it is
executable.

## Divergence 2 — "declaration context", the `Use` treatment (MEDIUM impact)

Provider moved since #914 was filed: the canonical implementation is now
`crates/perl-lsp-rs-core/src/providers/semantic_tokens/semantic_tokens.rs`
(the old `crates/perl-lsp-rs/src/features/semantic_tokens_provider.rs` no longer
exists). The `is_declaration_context` flag #914 described in the provider is also
gone — the provider no longer threads a declaration-context boolean at all, and no
symbol by that name exists in the codebase today. (The only similarly-named symbol,
`is_field_declaration_context` at
`crates/perl-parser-core/src/engine/parser/helpers.rs:73`, is a `field`-keyword
parse-time helper, unrelated to `use` or to token painting.)

**Side A — semantic tokens** — `semantic_tokens.rs`

- Declaration/definition modifiers are assigned in `walk_ast_full`,
  `semantic_tokens.rs:694-795`: `Package` (`:697-708`), `Subroutine`
  (`:712-738`), `Method` (`:742-760`), `Class` (`:764-769`),
  `LabeledStatement` (`:782-793`), and `my/our/local/state` variable
  declarations (`:893-894`).
- `Use` is inspected **only** for a read-only modifier on specific modules
  (`Const::Fast` at `semantic_tokens.rs:1057`, `Readonly` at
  `semantic_tokens.rs:1069`) — never as a declaration.

**Side B — scope analyzer** — `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/`

- Dispatch `mod.rs:677` routes `NodeKind::Use { .. }` to
  `declarations::handle_use`.
- `handle_use` (`declarations.rs:271-323`) treats `use vars` as a **binding
  introducer** — it declares the named variables into the current scope
  (`declarations.rs:291-299`, `declare_variable_parts_in_context(...)`). Note the
  binding path is gated on `module == "vars"` (`declarations.rs:281`), so an
  ordinary `use Foo;` no-ops here — only the `use vars qw(...)` form exercises the
  divergence.
- Sibling binding/scope introducers for comparison: `VariableListDeclaration`
  (`mod.rs:663`), `Subroutine` (`mod.rs:830`), `Package` (`mod.rs:857`).

**Divergence:** semantic tokens treats `Use` as **non-declaration** (only a
read-only-module probe); the scope analyzer treats the `use vars` form as a
**binding introducer** that declares symbols. Same `NodeKind::Use`, two answers —
the fixture `use vars qw($x);` declares `$x` in the scope analyzer but is painted
as a non-declaration by semantic tokens. (An ordinary `use Foo;` is a weaker
example: both consumers agree it declares nothing, because `handle_use` only acts
on `module == "vars"`.)

## Divergence 3 — "scope-introducing / foldable", If/While/Given/Try (MEDIUM-HIGH impact)

**Side A — folding** — `crates/perl-lsp-rs-core/src/providers/folding/mod.rs`

`visit_node` makes `If` / `While` / `Given` / `Try` / `Do` first-class foldable
ranges — `folding/mod.rs:248-293` (`If` `:248`, `While` `:260`, `Do`/`Eval`/`Defer`
`:274`, `Try` `:279`, `Given` `:290`), each calling `add_range_from_node(node, None)`.

**Side B — scope analyzer** — `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/`

Dedicated scope handlers exist only for `Block` (`scope_constructs.rs:12`),
`PhaseBlock` (`:31`), `For` (`:49`), `Foreach` (`:83`), `Subroutine` (`:115`),
`Try` (`:238`, partial — child scopes for catch blocks only), and `Package`
(`:296`); each opens a child `Scope::with_parent(...)`. `If` / `While` / `Given` /
`Do` have **no** match arm and fall through to the transparent `_` catch-all
(`mod.rs:886-893`), which recurses into children in the **current** scope with no
new boundary.

**Divergence:** folding treats `If`/`While`/`Given`/`Try` as scope/fold
boundaries; the scope analyzer treats `If`/`While`/`Given`/`Do` as transparent
containers. There is no single authoritative "introduces_scope" list driving both.

## `NodeKind` match-site counts (current, directional)

`src/` only, `.rs` files. `NodeKind::` is a raw grep line count (construction,
imports, match arms, and tests all included); `matches!(…NodeKind` counts
`matches!` sites touching `NodeKind`. These counts are **directional evidence of
duplication**, not a curated hand-match census — do not treat any single number as
load-bearing. (#914's "~489" was a narrower May estimate and is superseded.)

| Crate | `NodeKind::` lines | `matches!(…NodeKind` sites |
|---|---:|---:|
| perl-parser | 332 | 10 |
| perl-semantic-analyzer | 808 | 32 |
| perl-lsp-rs | 294 | 2 |
| perl-lsp-rs-core | 826 | 20 |
| perl-workspace | 149 | 3 |
| perl-dap | 4 | 0 |
| **Total (these 6)** | **2413** | **67** |

`perl-lsp-rs-core` and `perl-semantic-analyzer` are the heaviest consumers.
`perl-dap` touches `NodeKind` directly only 4 times because its breakpoint logic
lives under `perl-dap/src/breakpoint/` and reaches the AST through `perl-parser`
re-exports — its real classification logic is the validator functions in
Divergence 1.

## What the migration must subsume (for #911, not decided here)

Each divergence maps to a flag the shared classification already declares in
`classification.rs`:

| Divergence | Shared flag | Migration target |
|---|---|---|
| 1 — executable line | `is_executable()` | DAP `validator.rs` "any node ⇒ executable" |
| 2 — `Use` declaration | `declares_symbol()` | semantic-tokens vs. `scope_analyzer` `handle_use` |
| 3 — If/While/Given/Try scope | `introduces_scope()` | folding vs. `scope_analyzer` catch-all |

The primitive is in the tree; the divergences are the acceptance targets for
wiring consumers to it. Behavior-pinned migration (DAP → semantic tokens →
folding/document-symbols → scope analyzer) is later sequential work under #911 and
its child builders, not this evidence issue.