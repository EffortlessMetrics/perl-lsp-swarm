# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Name**: `perl-ast-v2`
- **Version**: workspace (inherits)
- **Tier**: 3 (incremental AST layer)
- **Purpose**: Second-generation Perl AST — uses `Range` (byte-range spans) instead of raw byte offsets, and a two-tier error model (`Error` inline vs. `ErrorRef` indexed) enabling incremental parsing and memory-efficient error storage.

## Commands

```bash
cargo build -p perl-ast-v2           # Build
cargo test -p perl-ast-v2            # Run tests
cargo clippy -p perl-ast-v2          # Lint
cargo doc -p perl-ast-v2 --open      # View documentation
```

## Architecture

### Key types

| Type | Module | Purpose |
|------|--------|---------|
| `NodeId` | root | `usize` alias — stable index into the node arena |
| `DiagnosticId` | root | `u32` alias — stable index into the diagnostic vec |
| `Node` | root | `id: NodeId`, `kind: NodeKind`, `range: Range` |
| `NodeKind` | root | Enum of all AST node variants (mirrors `perl-ast` `NodeKind` with range-based spans) |
| `MissingKind` | root | Enum for structurally absent but syntactically expected nodes |
| `Error` | root | Inline rich error (legacy path; carries full context inline) |
| `ErrorRef` | root | Indexed error reference (preferred; index into diagnostics vec) |

### Two-tier error model

`Error` (inline, legacy): embeds the full error message and context directly on the node. Used by older parser paths.

`ErrorRef` (indexed, preferred): stores only a `DiagnosticId` pointing into a side-channel diagnostics vector. Enables:
- Memory-efficient storage for files with many errors
- Stable referencing across incremental re-parses
- Cheaper cloning of error-heavy trees

New code should produce `ErrorRef` nodes; `Error` remains for backward compatibility.

### `Range` vs byte offsets

`perl-ast-v2` uses `Range` (start byte, end byte as a struct) throughout, unlike `perl-ast` which stores flat byte offsets. This makes span arithmetic explicit and avoids off-by-one errors when slicing source text.

### Dependencies

| Crate | Role |
|-------|------|
| `perl-position-tracking` | `Range` type and position primitives |

## Does NOT own

- Parsing logic (→ `perl-parser-core`, `perl-ast`)
- Tree-sitter compatibility shim (→ `perl-tree-sitter-compat`)
- Semantic analysis over the AST (→ `perl-semantic-analyzer`)

## Neighbors

| Crate | Relationship |
|-------|-------------|
| `perl-ast` | First-generation sibling; `perl-ast-v2` is the incremental successor |
| `perl-parser-core` | Produces `perl-ast-v2` nodes during incremental parse |
| `perl-tree-sitter-compat` | Adapts `perl-ast-v2` nodes to tree-sitter `TsNode` shape |
| `perl-semantic-analyzer` | Consumes `perl-ast-v2` for symbol extraction |

## Important Notes

- `NodeId` is a `usize` alias, not a newtype — arithmetic is intentional; do not wrap it without a use case
- `MissingKind` variants represent structurally expected but absent nodes — they carry source attribution for error recovery, not panic
- Prefer `ErrorRef` over `Error` for all new parser error sites
- `doctest = false` — no doc examples; tests live in `tests/` or `#[test]` blocks
