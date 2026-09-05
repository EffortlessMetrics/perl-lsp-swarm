# AST Compatibility Contract

This document defines compatibility expectations for the AST crates while
`perl-ast` moves toward a more explicit public stability contract.

## Surfaces and stability tier

- `perl_ast::ast` (`Node`, `NodeKind`, `SourceLocation`) is the primary parser AST
  surface and is treated as the stable public contract.
- `perl_ast::v2` (re-export of `perl-ast-v2`) is an experimental compatibility tier
  intended for incremental parsing consumers.
- `perl-ast-v2` is published for focused integration work, but its API is
  considered pre-stability until explicitly promoted.

## NodeKind change gate

No new `NodeKind` variant should land without all of the following:

- Child traversal coverage (`children`, `first_child`, immutable traversal,
  mutable traversal).
- Kind-name coverage (`kind_name`, `ALL_KIND_NAMES`).
- S-expression coverage, or an explicit "not renderable" decision documented in
  tests.
- Parser fixture coverage proving when the variant is emitted.
- Semantic analyzer decision: handled, intentionally ignored, or explicitly
  deferred.

## Depth safety

`perl_ast::Node` stays recursively owned. That is the public geometry; it is
not an arena or index tree.

- **Drop** is iterative and depth-independent. New child fields must be
  visited by the canonical mutable child walk so they inherit destruction
  safety. Overflow is proven on a 50,000-node chain with a 256 KiB worker;
  construct/destroy equality is proven at 10,000-node cycle depth.
- **Clone** is iterative over the same canonical child fields. Overflow is
  proven on a 50,000-node chain with a 256 KiB worker. Cloning is a full
  owned duplication, not a shared projection.
- **PartialEq** is iterative exact structural equality over the same
  canonical child fields. Overflow is proven on a 50,000-node chain with a
  256 KiB worker. This preserves current `==` semantics; it is not
  S-expression, fingerprint, or source-text equality.
- **Debug** is an iterative bounded human projection over the same
  canonical child fields. Overflow is proven on a 50,000-node chain with a
  256 KiB worker. Output stays at or under the documented byte bound, and
  truncation is visible. Rust `Debug` is not machine identity, equality, or
  a durable metric.
- Exact whole-tree reads (`count_nodes`, `find_deepest_containing_offset`)
  are iterative over the canonical child visit table and cannot silently
  truncate. Bounded variants expose complete/truncated/instrument-failed
  state. Native debug `render_debug_sexp` is iterative and returns
  `Complete` / `Truncated` / `InstrumentFailure`. `to_sexp` is a `String`
  convenience over that engine and cannot prove completeness.

## Contributor checklist (AST behavior changes)

Before opening a PR that adds or changes an AST node shape, verify:

1. Parser emission is tested with a fixture.
2. Traversal behavior is covered.
3. S-expression output expectation is covered.
4. Semantic analyzer handling decision is encoded in tests/docs.
5. `perl-ast` and `perl-ast-v2` test suites pass.
