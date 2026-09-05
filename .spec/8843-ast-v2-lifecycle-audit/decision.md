# `perl-ast-v2` package lifecycle — decision report (#8843)

**Ruling: `ABSORB`, with a forwarding compatibility window.**

Authored against `main@a5a0f9630bdc0ad0e12d6a115eb0d6f3824c71c4`, 2026-09-05.
Machine-readable authority: [`ast_v2_package_lifecycle.v1.json`](ast_v2_package_lifecycle.v1.json).
Loader and proof: `xtask/src/ast_v2_lifecycle_audit.rs`.

This report summarises the inventory. It is not the authority — the JSON is, and
the loader reconciles it against current source on every run.

## What the inventory found

| denominator | count |
|---|---:|
| public items (incl. every enum variant) | 39 |
| public re-export paths | 6 |
| consumer rows | 36 |
| package/release surfaces | 7 |
| external evidence rows | 5 |

Public-item rows and the production `NodeKind` parity denominator are **derived**
from source with `syn`, not authored. The dispositions, consumer roles, external
evidence and the ruling are authored.

### The package is a real published surface

`perl-ast-v2` is genuinely on crates.io: 13 versions from 0.12.0 to 0.17.0, none
yanked, created 2026-03-30. It is a workspace member, a Tier 1b publish-allowlisted
crate, carries its own docs.rs metadata, and `scripts/verify-docs-rs.sh` checks its
published documentation. Retirement is therefore an API and package migration. It
cannot be justified as deleting dead code.

### It does not earn an independent lifecycle

Against the five `RETAIN` clauses:

| clause | finding |
|---|---|
| separate semver / release cadence | **No.** The version is `version.workspace`; the published series tracks the workspace, and the 0.14.0 dry-run receipt treats it as one of the coordinated crates. |
| external consumers who should not depend on the larger package | **No.** crates.io reports 4 reverse dependencies, all first-party: `perl-ast`, `perl-error`, `perl-parser-core`, `perl-tokenizer`. The latter two pin `^0.12.2` and are absorbed packages, so those are historical. Zero third-party dependents observed. |
| a public proposition owned only by `perl-ast-v2` | **No.** Every public item is already reachable through `perl_ast::v2`, and `DiagnosticId`/`MissingKind` are additionally public API of `perl-parser-core`. |
| clean one-way dependency and maintenance boundary | Present, but a `v2` namespace under the canonical package preserves it. |
| reviewed reason a compatibility shell cannot serve the lifecycle | **None found.** The canonical path already exists and is the documented one. |

Download volume (6412 total / 5007 recent) is recorded and deliberately classified
`not_consumer_evidence`: it is consistent with CI, docs.rs and mirror traffic. The
loader structurally refuses to let a ruling rest on it.

### Unique propositions that must survive absorption

These justify a distinct `v2` namespace and an explicit contract. They do **not**
justify a second package identity, because a namespace preserves all of them.

- `NodeId` / `NodeIdGenerator` — the production AST has no node identity at all.
  Weaker than the name suggests: a process-local `usize` from zero, with no
  document, parse-generation, schema or persistence binding. Arena-local handle,
  not an incremental cross-parse identity contract.
- `DiagnosticId` + `NodeKind::ErrorRef` — lightweight error nodes indexing a
  separately stored diagnostics array. The package owns no `ParseOutput` binding
  node to table, so the index is meaningful only against a caller-supplied array.
- `MissingKind` + `NodeKind::Missing` — nine granular missing-syntax categories.
- `to_sexp` — an independent lossy debug projection over an abbreviated grammar,
  eliding below depth 128. Neither native machine output (#8044) nor Tree-sitter
  compatibility (#8047), and not parity evidence for either.

### Parity, recorded at field level rather than by name

18 v2 `NodeKind` variants against the production AST's 76. 16 share a name with a
production variant; the audit records the exact relation rather than reading the
shared name as parity:

- **8 `equivalent`** — `Variable`, `Number`, `String`, `Identifier`, and the four
  `Missing*` unit variants. Same field names, same field types, and no child
  `Node` through which the differing node contract could leak. Asserted at field
  level only; it never claims the two enclosing `Node` contracts are interchangeable.
- **8 `divergent`** — `Program`, `Block`, `VariableDeclaration`,
  `VariableListDeclaration`, `Binary`, `Unary`, `If`, `Error`. Most read
  field-identical but carry v2 `Node` children, which have a `NodeId` and a
  line/column `Range` instead of the production `SourceLocation`. `If` and `Error`
  differ structurally as well: v2's `If` has no `keyword` field and holds elsif
  branches by value; v2's `Error` loses the token-typed expectation set and the
  `found` token.
- **2 `unique`** — `ErrorRef`, `Missing`.

## Two findings the planning comment did not have

**Four more public paths than recorded.** The 2026-08-21 comment lists two
re-export paths. Current main has six: `perl_ast::v2`,
`perl_parser_core::engine::ast_v2`, `perl_parser_core::ast_v2`,
`perl_parser_core::{DiagnosticId, MissingKind}`, `perl_parser::ast_v2`, and
`perl_parser::compat::ast_v2`. The fourth matters most: a consumer using
`perl_parser_core::MissingKind` has no textual dependency on the `perl-ast-v2`
name at all, so no name-based search would find it.

**`perl-lexer` declares a dependency it does not use.** `crates/perl-lexer/Cargo.toml:35`
declares `perl-ast-v2` under `[dev-dependencies]`, and no `perl_ast_v2` or
`perl_ast::v2` reference exists anywhere in `crates/perl-lexer/src` or
`crates/perl-lexer/tests`. The module doc at
`crates/perl-lexer/src/tokenizer/mod.rs:3-5` states the opposite of the manifest,
describing that module as the slice with *no* `perl-ast-v2` dependency. Recorded
as an inventory row; removing it is outside this issue's claim ceiling.

## A finding about reachability

Only one production site names any `NodeKind` variant: `context_impls.rs` matches
`NodeKind::Error`. The other 17 variants, and all 9 `MissingKind` variants, are
reached only by test consumers. Notably `ErrorRef` — which the crate's own docs
call "the preferred error representation" — has no production consumer at all; the
single production error site constructs the legacy `Error` variant instead.

This is recorded, not acted on. It is evidence about how far the experiment got,
and it belongs to #8844/#8845, not here.

## Compatibility window and successors

`ABSORB` here means: move the implementation under `perl_ast::v2` and leave
`perl-ast-v2` a forwarding package. It does **not** mean merging v1 and v2
semantics, and it does **not** authorize deleting the published package.

| successor | wake |
|---|---|
| #8844 | **May start now.** This ruling is its start condition. |
| #8845 | After #8844 lands. The migration set is this manifest's gating rows: 4 production, 5 re-export, 7 test-fixture. |
| #8847 | After #8845 **and** the window closes on **re-observed** registry evidence. #8847 must re-run the reverse-dependency observation; it may not inherit this snapshot. |

**Reversal condition.** Move to `RETAIN` only on a third-party reverse dependency,
a release cadence diverging from the workspace version, or a public proposition
that exists only under the `perl_ast_v2` path and cannot be served by
`perl_ast::v2`. Package existence, publish allowlisting, docs.rs metadata and
experimental branding are explicitly below this threshold, and the loader enforces
that a `retain` ruling must name independent-lifecycle evidence.

## Limitations

- The consumer scan is textual. A consumer reaching the package only through
  `perl_parser_core`'s unqualified re-export, or a grouped `use perl_ast::{v2, ast}`,
  names none of the four tokens. None exists on the authored basis; the instrument
  cannot promise it.
- `syn` sees declarations, not semantics. The public-item denominator is exact;
  whether a consumer depends on a variant's *meaning* is an authored judgement.
- Per-variant consumer attribution was resolved by alias-aware analysis (v1 and v2
  share 16 variant names, so a bare grep over-attributes). That is authored input,
  not a check the loader re-runs.
- No instrument here enumerates non-registry external use — a vendored copy or a
  private fork. Recorded as an explicit `unavailable` evidence row rather than
  allowed to read as zero. This unknown is precisely why the ruling is
  absorb-with-forwarding rather than delete.
- External evidence is a point-in-time observation from 2026-09-05 and ages.
