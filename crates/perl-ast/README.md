# perl-ast

AST (Abstract Syntax Tree) node definitions for the Perl parser ecosystem.

## Overview

`perl-ast` provides the typed node structures used to represent parsed Perl source code. It contains two AST modules:

- **`ast`** -- The primary AST used by `perl-parser`. Defines `Node` (kind + `SourceLocation`) and the `NodeKind` enum with 50+ variants covering declarations, expressions, control flow, regex, OO constructs, and error recovery nodes. Includes a native debug S-expression projection via `to_sexp()` (not Tree-sitter compatibility; see issue 8047).
- **`v2`** -- Re-exported from the extracted `perl-ast-v2` microcrate. This incremental-parsing surface is currently experimental/pre-stability; nodes carry a unique `NodeId` and use `Range` (line/column) positions instead of byte offsets. Adds `NodeIdGenerator`, `MissingKind`, `DiagnosticId`, and lightweight `ErrorRef` nodes.

## Public API

Re-exports from `lib.rs`: `Node`, `NodeKind`, `SourceLocation`.

## Ownership and depth safety

`Node` is a recursively owned tree (`Box<Node>`, `Vec<Node>`, optional
children, pair/clause records). That public geometry is unchanged.

- **Drop** is iterative. Children are detached through the canonical mutable
  child walk into a heap work stack before each node's remaining fields are
  released. A 50,000-node chain on a 256 KiB worker does not overflow the
  thread stack. Construct/destroy equality is proven at 10,000-node cycle
  depth, not on the overflow fixture. `std::mem::forget` is not a production
  or test strategy for this crate.
- **Clone** is iterative. It walks the same canonical child fields, rebuilds
  each parent after cloned children exist, and is a full owned duplication
  (not a cheap share). A 50,000-node chain on a 256 KiB worker does not
  overflow the thread stack.
- **PartialEq** is iterative exact structural equality over the same canonical
  child fields (location, variant, every non-child payload, optional/repeated
  cardinality, child order). A 50,000-node chain on a 256 KiB worker does not
  overflow the thread stack. S-expression, fingerprint, and source-text
  projections are not this proposition.
- **Debug** is an iterative bounded human projection over the same canonical
  child fields. It shows kind, range, a selected payload summary, and a
  bounded child projection. Truncation is visible (`#truncated`). A
  50,000-node chain on a 256 KiB worker does not overflow the thread stack,
  and the rendering stays at or under the documented byte bound. Rust `Debug`
  is not machine identity, equality, or a durable metric oracle.
- Recursive whole-tree reads: `count_nodes` and
  `find_deepest_containing_offset` are iterative over the canonical child
  visit table and return exact results. Bounded variants expose
  `Complete` / `Truncated` / `InstrumentFailure` instead of an ordinary
  `usize` / `Some` after a caller-selected bound. Native debug
  `render_debug_sexp` is iterative over the same visit table and returns
  `Complete` / `Truncated` / `InstrumentFailure`. `to_sexp()` is a `String`
  convenience over that engine and cannot prove completeness.

See the rustdoc on `Node` and the [AST compatibility contract](../../docs/reference/ast-contract.md).

## Workspace Role

Tier 1 leaf crate. Depended on by `perl-parser-core`, `perl-tokenizer`, `perl-pragma`, and `perl-error`.

## Dependencies

- `perl-position-tracking` -- span and position types (`SourceLocation`, `Range`, `Position`)
- `perl-token` -- token definitions (`Token`, `TokenKind`) used in error recovery nodes

## License

MIT OR Apache-2.0
## AST compatibility contract

See the [AST compatibility contract](../../docs/reference/ast-contract.md) for
the stability tiers and required coverage when adding or changing `NodeKind`
variants.
