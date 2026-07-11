# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Name**: `perl-module`
- **Version**: 0.17.0
- **Tier**: 3 (consolidated facade over 13 formerly-separate microcrates)
- **Purpose**: Unified Perl module resolution, import analysis, name normalization, and rename refactoring. The stable public API for module-level facts across the LSP ecosystem.

## Commands

```bash
cargo build -p perl-module           # Build
cargo test -p perl-module            # Run all test suites
cargo clippy -p perl-module          # Lint
cargo doc -p perl-module --open      # View documentation
```

## Architecture

### Public modules (re-exported via `api.rs`)

| Module | Key types / functions |
|--------|-----------------------|
| `boundary` | `ModuleTokenRange`, `ModuleTokenRangeIter`, `find_standalone_module_token_ranges`, `contains_standalone_module_token` |
| `import` | `ModuleImportHead`, `ModuleImportKind`, `ImportBehavior`, `LoadTiming`, `DispatchSemantics`, `RequireImportEntry`, `parse_module_import_head`, `extract_require_import_symbols`, `resolve_known_export_tag` |
| `import_match` | `line_references_module_import` |
| `name` | `normalize_package_separator`, `legacy_package_separator`, `module_variant_pairs` |
| `path` | `module_name_to_path`, `file_path_to_module_name`, `module_path_to_name` |
| `reference` | `ModuleReference`, `ModuleReferenceKind`, `extract_module_reference`, `find_module_reference`, `_extended` variants |
| `rename` | `ModuleLineEdit`, `plan_module_rename_edits`, `apply_module_rename_edits`, `replace_module_name_prefix`, `line_references_package_declaration`, `line_references_isa_assignment`, `line_references_qualified_call` |
| `resolution` | `IncRoot`, `IncRootKind`, `ModuleUriResolution`, `resolve_module_path`, `resolve_module_uri`, `resolve_module_uri_with_effective_inc` |
| `token` / `token_core` / `token_parser` | `ModuleTokenSpan`, `is_module_token_char`, `parse_module_token`, `replace_module_token` |

### Dependencies

| Crate | Role |
|-------|------|
| `url` | URI-based module resolution |
| `perl-parser-core` | Token-level parsing for import and boundary detection |
| `perl-workspace` | Workspace context for resolution |

## Contract

All downstream consumers **must import from `perl_module` at the crate root**, not from internal submodules directly. `api.rs` is the contract boundary — it re-exports the stable surface from all internal modules.

## Does NOT own

- Cross-file workspace symbol lookup (→ `perl-workspace`, `perl-workspace-core`)
- LSP-level rename coordination (→ `perl-lsp-rs-core` providers)
- Module documentation (→ `perl-pod`)

## Neighbors

| Crate | Relationship |
|-------|-------------|
| `perl-workspace` | Consumer of module resolution facts |
| `perl-lsp-rs-core` | Drives goto-definition, rename, and import-management providers |
| `perl-workspace-core` | Uses import/export facts from this crate |

## Important Notes

- This crate was consolidated from 13 formerly-separate `perl-module-*` microcrates in Wave 1 (#4420). No sub-crate granularity survives at publish time.
- Test suites are split by area (`name`, `path`, `import`, `token`, `boundary`, `reference`, `rename`, `resolution`, `import_match`, `token_parser`) — see `[[test]]` entries in `Cargo.toml`
- `#![deny(unsafe_code)]`
