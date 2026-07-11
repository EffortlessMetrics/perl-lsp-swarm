# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Name**: `perl-semantic-facts`
- **Version**: workspace (inherits)
- **Tier**: 3 (pure vocabulary / data crate)
- **Purpose**: Neutral semantic fact vocabulary for the Perl LSP stack — pure data types and ID newtypes with no parsing or analysis logic. Consumed by `perl-semantic-analyzer` and related crates as a shared language for semantic facts.

## Commands

```bash
cargo build -p perl-semantic-facts           # Build
cargo test -p perl-semantic-facts            # Run tests
cargo clippy -p perl-semantic-facts          # Lint
cargo doc -p perl-semantic-facts --open      # View documentation
```

## Architecture

### ID newtypes

All IDs are `u64` newtypes defined via the `id_newtype!` macro:

| ID Type | Identifies |
|---------|-----------|
| `FileId` | Source file |
| `ScopeId` | Lexical scope |
| `EntityId` | Named entity (sub, variable, package…) |
| `AnchorId` | Declaration site / anchor |
| `OccurrenceId` | Reference occurrence |
| `EdgeId` | Relation edge between entities |
| `DiagnosticId` | Semantic diagnostic |

All newtypes implement `Copy`, `Hash`, `Eq`, `Ord`, `serde::Serialize/Deserialize`.

### Kind enums

| Enum | Variants |
|------|---------|
| `EntityKind` | Sub, Method, Variable, Package, Constant, … |
| `OccurrenceKind` | Definition, Reference, Import, Export, … |
| `EdgeKind` | Calls, Inherits, Uses, Exports, … |

### Evidence metadata

| Type | Purpose |
|------|---------|
| `Provenance` | Where a fact came from (source text, inference, heuristic) |
| `Confidence` | Certainty level (Definite, Probable, Possible) |

### Fact structs

| Struct | Key fields |
|--------|-----------|
| `AnchorFact` | `id`, `file_id`, `range`, `entity_id` |
| `EntityFact` | `id`, `kind`, `name`, `scope_id` |
| `OccurrenceFact` | `id`, `anchor_id`, `kind`, `provenance` |
| `EdgeFact` | `id`, `from`, `to`, `kind` |
| `DiagnosticFact` | `id`, `range`, `message`, `severity` |

### Import/export vocabulary

| Type | Purpose |
|------|---------|
| `ExportSet` | Set of exported symbols from a package |
| `ImportSpec` | One import statement's resolved facts |
| `UseLibFact` | Facts from a `use lib` pragma (`#[non_exhaustive]`) |

### Dependencies

| Crate | Role |
|-------|------|
| `serde` | Serialization for all fact structs and enums |

**No analysis logic, no parsing, no workspace dependencies.** This crate is intentionally a pure vocabulary layer.

## Does NOT own

- Symbol extraction or scope analysis (→ `perl-semantic-analyzer`)
- Cross-file workspace facts (→ `perl-workspace`, `perl-workspace-core`)
- LSP types (→ `lsp-types`)

## Neighbors

| Crate | Relationship |
|-------|-------------|
| `perl-semantic-analyzer` | Primary producer of fact structs |
| `perl-workspace-core` | Consumes facts via `ProjectModel` |
| `perl-lsp-rs-core` | Reads facts for provider responses |

## Important Notes

- `UseLibFact` is `#[non_exhaustive]` — callers must handle unknown fields with `..` patterns
- `doctest = false` — no doc examples by convention; tests use `#[test]` blocks
- `id_newtype!` macro enforces the pattern — do not define new ID types manually
- `serde` is a hard (not optional) dependency — all facts are expected to be serializable
