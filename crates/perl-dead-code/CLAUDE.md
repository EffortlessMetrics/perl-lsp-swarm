# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Name**: `perl-dead-code`
- **Version**: workspace (inherits)
- **Tier**: 3 (stub detector; `publish = false`)
- **Purpose**: Dead code detection for Perl workspaces — identifies unused subroutines, variables, and packages by cross-referencing declaration sites against occurrence sets from the workspace model. Currently a working stub; block-depth tracking uses a character-counting heuristic rather than a full parse.

## Commands

```bash
cargo build -p perl-dead-code           # Build
cargo test -p perl-dead-code            # Run tests
cargo clippy -p perl-dead-code          # Lint
cargo doc -p perl-dead-code --open      # View documentation
```

## Architecture

### Key types

| Type | Purpose |
|------|---------|
| `DeadCodeType` | Enum: `Sub`, `Variable`, `Package` |
| `DeadCode` | Single finding: `kind`, `name`, `file`, `line` |
| `DeadCodeAnalysis` | Full analysis result: `Vec<DeadCode>` + `DeadCodeStats` |
| `DeadCodeStats` | Counts per `DeadCodeType`; implements `Default` |
| `DeadCodeDetector` | Entry point: constructed with workspace reference, runs detection |

### Key functions

| Function | Signature | Purpose |
|----------|-----------|---------|
| `generate_report` | `fn(analysis: &DeadCodeAnalysis) -> String` | Format findings as a human-readable report |

### Detection approach

The detector cross-references declaration sites (from the workspace `ProjectModel`) against the occurrence set. Anything declared but never referenced outside its own declaration site is flagged as dead.

**Block-depth tracking is a character-counting heuristic** (`{` / `}` counting), not a full parse. This means:
- False positives are possible inside strings or heredocs containing braces
- Not suitable as a correctness gate — treat findings as hints, not proof
- A future version should replace this with proper AST traversal from `perl-ast-v2`

### Dependencies

| Crate | Role |
|-------|------|
| `perl-workspace` | Workspace index and `ProjectModel` for declaration/occurrence facts |
| `serde` | `DeadCodeAnalysis` serialization for JSON output |

## Does NOT own

- Reachability analysis across call graphs (stub only)
- Integration with LSP code-action providers (→ `perl-lsp-rs-core`)
- Cross-workspace dead code detection across multiple projects

## Neighbors

| Crate | Relationship |
|-------|-------------|
| `perl-workspace` | Input: workspace facts used for cross-reference |
| `perl-lsp-rs-core` | Intended consumer when the stub matures to a full implementation |

## Important Notes

- `publish = false` — internal tool; not published to crates.io
- `DeadCodeStats` implements `Default` — use `DeadCodeStats::default()` when constructing an empty analysis
- The character-counting block-depth heuristic is a known limitation; do not rely on it for exact scope boundaries
- When replacing the heuristic with AST traversal, target `perl-ast-v2` `Node` ranges — those carry reliable byte-range spans
