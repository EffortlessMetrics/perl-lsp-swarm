# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Name**: `perl-tree-sitter-compat`
- **Version**: 0.17.0
- **Tier**: 3 (adapter / compatibility shim; `publish = false`)
- **Purpose**: Tree-sitter compatibility adapter — bridges the native Perl parser (`perl-parser-core`) to the tree-sitter node shape expected by consumers (syntax highlighting, S-expression output, tooling that speaks tree-sitter's API). Governed by PLSP-ADR-0006.

## Commands

```bash
cargo build -p perl-tree-sitter-compat           # Build
cargo test -p perl-tree-sitter-compat            # Run tests
cargo clippy -p perl-tree-sitter-compat          # Lint
cargo doc -p perl-tree-sitter-compat --open      # View documentation
```

## Architecture

### Key types

| Type | Purpose |
|------|---------|
| `TsNode` | Tree-sitter-shaped node: `kind: &'static str`, `range: TsRange`, `named: bool`, children |
| `TsPoint` | Tree-sitter point: `row: u32`, `column: u32` |
| `TreeError` | Error from parse-to-tree conversion |
| `Highlight` | Syntax highlight capture: `range`, `capture_name` |

**Invariant**: `TsNode.named` is always `true` — this adapter exposes only named nodes (anonymous punctuation nodes are elided to match tree-sitter's named-node traversal pattern).

### Key functions

| Function | Signature | Purpose |
|----------|-----------|---------|
| `parse_to_tree` | `fn(source: &str) -> Result<TsNode, TreeError>` | Parse Perl source and return a `TsNode` tree |
| `to_ts_node` | `fn(node: &Node, line_index: &LineIndex) -> TsNode` | Convert a native `Node` to `TsNode` shape |
| `highlights` | `fn(tree: &TsNode) -> Vec<Highlight>` | Walk tree and collect syntax highlight captures |
| `capture_for` | `fn(kind: &NodeKind) -> Option<&'static str>` | Map `NodeKind` to a tree-sitter capture name |
| `to_sexp` | `fn(node: &TsNode) -> String` | Produce compact S-expression string |
| `to_sexp_pretty` | `fn(node: &TsNode) -> String` | Produce indented S-expression string |
| `pascal_to_snake` | `fn(s: &str) -> String` | Convert `PascalCase` node kind names to `snake_case` |

### Dependencies

| Crate | Role |
|-------|------|
| `perl-parser-core` | Source of the native AST to be adapted |
| `perl-workspace-core` | `LineIndex` for position conversion |

## Does NOT own

- The native parser itself (→ `perl-parser-core`)
- Semantic analysis over the adapted tree (→ `perl-semantic-analyzer`)
- Actual tree-sitter C library (this is a pure-Rust adapter matching the API shape)

## Neighbors

| Crate | Relationship |
|-------|-------------|
| `perl-parser-core` | Primary input — provides the `Node`/`NodeKind` to adapt |
| `perl-workspace-core` | Provides `LineIndex` for row/column conversion |
| `perl-lsp-rs-core` | Consumer for semantic-tokens and syntax-highlighting providers |

## Important Notes

- `publish = false` — internal adapter; not published to crates.io
- Governed by PLSP-ADR-0006 (native stack policy — tree-sitter C binding is explicitly prohibited)
- `TsNode.named = true` always — only named nodes are exposed (anonymous tokens are structural noise)
- `pascal_to_snake` exists because `NodeKind` variants are PascalCase while tree-sitter capture names are snake_case; keep this mapping in sync when adding `NodeKind` variants
- `to_sexp` / `to_sexp_pretty` are primarily for debugging and test assertions; they match the tree-sitter CLI format for easy diffing
