# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Name**: `perl-diagnostics`
- **Version**: workspace (inherits)
- **Tier**: 3 (consolidated diagnostic vocabulary)
- **Purpose**: Unified diagnostic code catalog, types, and metadata for the Perl LSP stack — consolidates three formerly-separate microcrates (`perl-diagnostic-codes`, `perl-diagnostic-types`, `perl-diagnostic-catalog`) into a single stable public API.

## Commands

```bash
cargo build -p perl-diagnostics           # Build
cargo test -p perl-diagnostics            # Run tests
cargo clippy -p perl-diagnostics          # Lint
cargo doc -p perl-diagnostics --open      # View documentation
```

## Architecture

### Key types

| Type | Purpose |
|------|---------|
| `DiagnosticCode` | Typed code: `PL001`–`PL999` (language) + `PC001`–`PC005` (critic) |
| `DiagnosticCategory` | Enum: parse, syntax, semantic, style, critic |
| `DiagnosticSeverity` | Enum: Error, Warning, Information, Hint (mirrors LSP) |
| `DiagnosticTag` | Enum: Unnecessary, Deprecated |
| `Diagnostic` | Full diagnostic: code + message + range + severity + tags + related |
| `RelatedInformation` | Location + message for secondary diagnostic spans |
| `DiagnosticMeta` | Static metadata for a code: description, category, severity, link |

### Builder functions

Convenience constructors in the catalog module:

| Function | Produces |
|----------|---------|
| `diagnostic_meta(code, ...)` | `DiagnosticMeta` for static registration |
| `parse_error(range, msg)` | `Diagnostic` for parse failures |
| `syntax_error(range, msg)` | `Diagnostic` for syntax violations |
| `unused_var(range, name)` | `Diagnostic` for unused variable warnings |
| (and more) | See `src/catalog.rs` for full set |

### Public API contract

`api.rs` uses explicit per-symbol re-exports — all downstream consumers must import from `perl_diagnostics` at the crate root, not from internal submodules directly.

### Dependencies

| Crate | Role |
|-------|------|
| `serde` (optional) | `Diagnostic` serialization for JSON output |
| `serde_json` | JSON encoding support |

## Does NOT own

- LSP `PublishDiagnostics` notification dispatch (→ `perl-lsp-rs-core` providers)
- `Perl::Critic` output parsing (→ `perl-lsp-rs-core::critic_parser`)
- Diagnostic rendering / hover display

## Neighbors

| Crate | Relationship |
|-------|-------------|
| `perl-lsp-rs-core` | Primary consumer — providers produce `Diagnostic` values |
| `perl-workspace-core` | May carry diagnostics in `ParseStatus` |

## Important Notes

- `PL` prefix = language diagnostics (parser, semantic analyzer); `PC` prefix = `Perl::Critic` policy violations
- `serde` support is feature-gated (`optional = true`) — enable when JSON serialization is needed
- This crate was consolidated from three microcrates; no sub-crate granularity survives at publish time
