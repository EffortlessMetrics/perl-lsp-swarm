# perl-ast

AST (Abstract Syntax Tree) node definitions for the Perl parser ecosystem.

## Overview

`perl-ast` provides the typed node structures used to represent parsed Perl source code. It contains two AST modules:

- **`ast`** -- The primary AST used by `perl-parser`. Defines `Node` (kind + `SourceLocation`) and the `NodeKind` enum with 50+ variants covering declarations, expressions, control flow, regex, OO constructs, and error recovery nodes. Includes S-expression serialization via `to_sexp()`.
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
- **Debug** remains the derived recursive implementation. It is a supported
  operation on ordinary parser-produced trees (nesting within `MAX_AST_DEPTH`
  and the parser recursion limit). It is **not** stack-safe for adversarial
  or hand-built chains of destruction-test depth, and that precondition is
  not enforced at runtime. The stack-safe replacement is tracked as
  [#8840](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/8840).
- Recursive whole-tree reads such as `to_sexp`, `count_nodes`, and
  `find_deepest_containing_offset` stay separately guarded by `MAX_AST_DEPTH`
  and may truncate. That guard does not apply to Drop, Clone, or PartialEq.

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
