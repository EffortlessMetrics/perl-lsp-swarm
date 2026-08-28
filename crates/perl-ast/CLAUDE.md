# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

`perl-ast` is a **Tier 1 leaf crate** providing AST (Abstract Syntax Tree) node definitions for the Perl parser ecosystem.

**Purpose**: Typed representation of Perl syntax constructs used by the parser, semantic analyzer, and LSP server.

**Version**: workspace (currently 0.12.3)

## Commands

```bash
cargo build -p perl-ast              # Build this crate
cargo test -p perl-ast               # Run tests
cargo clippy -p perl-ast             # Lint
cargo doc -p perl-ast --open         # View documentation
```

## Architecture

### Dependencies

- `perl-position-tracking` -- Span/position types (`SourceLocation`, `Range`, `Position`)
- `perl-token` -- Token definitions (`Token`, `TokenKind`)

### Source Modules

| File | Purpose |
|------|---------|
| `lib.rs` | Re-exports `Node`, `NodeKind`, `SourceLocation` |
| `ast.rs` | Primary AST: `Node` struct (kind + location), `NodeKind` enum (50+ variants) |
| `kind_schema/` | Structural `NodeKind` registry: production `FieldId` membership, field-aware child traversal, schema identity, and freshness-gated NodeKind inventory; not rendering or parser behavior |
| `ast/node_clone.rs` | Iterative `Node` clone over canonical child fields |
| `ast/node_debug.rs` | Iterative bounded `Node`/`NodeKind` `Debug` |
| `ast/node_eq.rs` | Iterative `Node` equality over canonical child fields |
| `ast/node_sexp.rs` | Native debug S-expression projection (`to_sexp`, `render_debug_sexp`); not Tree-sitter compatibility |
| `ast/read_cursor.rs` | Iterative exact/bounded whole-tree reads over canonical child fields |
| `v2.rs` | Enhanced AST for incremental parsing: `Node` with `NodeId` + `Range`, `NodeIdGenerator`, `MissingKind`, `DiagnosticId` |

### Key Types

| Type | Module | Purpose |
|------|--------|---------|
| `ast::Node` | `ast` | Primary AST node: `kind: NodeKind` + `location: SourceLocation` |
| `ast::NodeKind` | `ast` | Enum with 50+ variants (Program, Subroutine, If, Variable, FunctionCall, etc.) |
| `v2::Node` | `v2` | Enhanced node with `id: NodeId`, `kind: NodeKind`, `range: Range` |
| `v2::NodeKind` | `v2` | Subset of node kinds for incremental parsing |
| `v2::NodeIdGenerator` | `v2` | Sequential unique ID generator for v2 nodes |
| `v2::MissingKind` | `v2` | Enum for specific kinds of missing syntax (Expression, Block, Semicolon, etc.) |
| `v2::DiagnosticId` | `v2` | Type alias (`u32`) for lightweight error references |

### NodeKind Categories (ast module)

**Declarations**: `VariableDeclaration`, `VariableListDeclaration`, `Subroutine`, `Method`, `Package`, `Class`, `Format`
**Control flow**: `If`, `While`, `For`, `Foreach`, `Given`, `When`, `Default`, `StatementModifier`, `LabeledStatement`
**Expressions**: `Binary`, `Unary`, `Ternary`, `Assignment`, `FunctionCall`, `MethodCall`, `IndirectCall`
**Literals**: `Number`, `String`, `Heredoc`, `ArrayLiteral`, `HashLiteral`, `Regex`
**Variables**: `Variable`, `VariableWithAttributes`, `Typeglob`
**Modules**: `Use`, `No`, `PhaseBlock`, `DataSection`
**Error recovery**: `Error`, `MissingExpression`, `MissingStatement`, `MissingIdentifier`, `MissingBlock`, `UnknownRest`
**Other**: `Program`, `Block`, `ExpressionStatement`, `Return`, `LoopControl`, `Eval`, `Do`, `Try`, `Diamond`, `Ellipsis`, `Undef`, `Readline`, `Glob`, `Identifier`, `Prototype`, `Signature`, `MandatoryParameter`, `OptionalParameter`, `SlurpyParameter`, `NamedParameter`

## Usage

```rust
use perl_ast::{Node, NodeKind, SourceLocation};

// Construct a node
let loc = SourceLocation { start: 0, end: 10 };
let node = Node::new(
    NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() },
    loc,
);

// S-expression output (native debug projection)
assert!(node.to_sexp().starts_with("(variable"));

// Pattern match on kind
match &node.kind {
    NodeKind::Variable { sigil, name } => { /* ... */ }
    _ => {}
}
```

## Important Notes

- `ast::Node` is a concrete struct, not a trait -- work with it via pattern matching on `NodeKind`
- `Node::to_sexp()` is a native debug S-expression projection (one root per node,
  canonical child fields, one escaping policy). Completeness is
  `Node::render_debug_sexp`. The `String` wrapper cannot prove completeness.
  It is not Tree-sitter compatibility (issue 8047), AST equality (issue 7045),
  or typed machine output (issue 8044).
- `NodeKind::kind_name()` returns a static string name; `NodeKind::ALL_KIND_NAMES` lists all names
- `NodeKind::grammar_kind_name_static()` is the allocation-free canonical grammar-kind table; `grammar_kind_name()` handles only runtime-derived names
- Adding a new `NodeKind` variant also requires deliberate classification in `grammar_kind_name_static()`; its exhaustive match is part of the metadata drift guard
- `Node::for_each_child_with_field()` / `try_for_each_child_mut_with_field()` share one visit table owned by `kind_schema`; keep `FieldId` names stable when extending the AST
- Adding a new `NodeKind` variant also requires adding a representative instance to every all-variant test fixture: `classification.rs`'s `all_variants()`/`all_variants_maximal()`, `tests/helpers.rs`'s `all_nodekind_instances()`, `tests/nodekind_coverage_tests.rs`'s `build_cases()`, and `ast.rs`'s `all_node_kinds()`. Each is guarded by a name-set comparison against `ALL_KIND_NAMES`, so an omission fails a test rather than silently narrowing coverage
- Adding new `NodeKind` variants require updating `to_sexp()` payload disposition,
  `kind_name()`, and the structural registry row in `kind_schema/registry.rs`. Child
  fields in the debug projection come from the shared visit table. `ALL_KIND_NAMES`
  is auto-derived and does not need manual updating
- Dependents: `perl-parser-core`, `perl-tokenizer`, `perl-pragma`, `perl-error`
