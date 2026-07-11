# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Name**: `perl-workspace-core`
- **Version**: 0.17.0
- **Tier**: 3 (LSP-free workspace model layer; `publish = false`)
- **Purpose**: Deterministic, LSP-free project-facts substrate — builds and owns the typed `ProjectModel` for an entire Perl workspace. Consumed by the LSP server, DAP server, critic/tidy, RIPR exporter, and Kwalitee scoring.

## Commands

```bash
cargo build -p perl-workspace-core             # Build
cargo test -p perl-workspace-core              # Run tests
cargo clippy -p perl-workspace-core            # Lint
cargo doc -p perl-workspace-core --open        # View documentation
```

## Architecture

### Key types

| Type | Module | Purpose |
|------|--------|---------|
| `ProjectModel` | `model` | The assembled fact set — flat ordered vecs, serializable via serde |
| `ProjectModelRequest` | root | Entry-point configuration |
| `build_project_model` | root | Main builder function |
| `FileRecord`, `FileId`, `FileRole`, `ParseStatus` | `model` | Per-file facts |
| `PackageRecord`, `PackageId` | `model` | Per-package facts |
| `SymbolRecord`, `SymbolId`, `SymbolFactKind`, `Visibility` | `model` | Per-symbol facts |
| `ImportFact`, `ImportKind` | `model` | Import edge facts |
| `ExportFact`, `ExportKind` | `model` | Export edge facts |
| `CompileEffectFacts` | `model` | Compile-time effect classification |
| `DistMetadataFacts`, `Prereq` | `model` | Distribution metadata (`META.json`) |
| `PodFact`, `PodSection`, `PodSectionKind` | `model` | Structured POD facts |
| `RelationFact`, `RelationKind` | `model` | Cross-entity relations |
| `DynamicBoundary`, `DynamicBoundaryKind` | `model` | Dynamic dispatch boundaries |
| `SourceRange`, `Utf8LineIndex`, `Digest`, `fnv1a` | `id` | Primitives |
| `Provenance`, `Confidence`, `EvidenceSource`, `Producer` | `model` | Evidence metadata |
| `FactClasses` | root | Bitflag selector — pay only for requested fact classes |
| `SCHEMA_VERSION: u32 = 1` | root | Serialization schema version |

### Key files

| File | Purpose |
|------|---------|
| `src/model.rs` | `ProjectModel` struct; query helpers (`file_by_path`, `packages_in_file`…); `sort_for_determinism()` |
| `src/builder.rs` | `build_project_model`; directory walker (skips `.git`, `target`, `node_modules`, `blib`, `_build`, `.svn`, `vendor`) |
| `src/id.rs` | FNV-1a 64-bit hashing; `Digest` (`fnv64:<16 hex>`), typed ID newtypes |

### Dependencies

| Crate | Role |
|-------|------|
| `perl-parser-core` | Workspace walk and Perl parsing |
| `perl-symbol` | `extract_symbol_decls` for declaration projection |
| `perl-pragma` | Perl version/feature tables (5.10→5.42) for compile-effect facts |
| `perl-pod` | Structured POD documentation facts |
| `serde` + `serde_json` | Serialization; `META.json` parsing |

## Hard dependency contract

`tests/dependency_contract.rs` enforces that this crate must **NOT** depend (transitively) on:
- `perl-lsp-rs`, `perl-lsp-rs-core`, `perllsp`
- `perl-dap`
- `lsp-types`, `tokio`, `tower-lsp`
- `perl-workspace`

This keeps the project model LSP-free and reusable across all consumers.

## Position model

- Stored as **byte offsets + UTF-8 line/col**
- UTF-16 LSP positions are produced **only at the LSP boundary**, never stored here

## Does NOT own

- LSP protocol types or providers (→ `perl-lsp-rs-core`)
- DAP protocol types (→ `perl-dap`)
- Cross-file symbol resolution used by LSP navigation (→ `perl-workspace`)
- Module resolution (→ `perl-module`)

## Neighbors

| Crate | Relationship |
|-------|-------------|
| `perl-pod` | Provides `PodDoc` consumed as `PodFact` |
| `perl-symbol` | Provides declaration projection |
| `perl-pragma` | Provides version/feature table |
| `perl-lsp-rs-core` | Major consumer of `ProjectModel` |
| `perl-dap` | Consumer for debug session workspace facts |

## Important Notes

- Governed by PLSP-ADR-0006 and `docs/reference/NATIVE_STACK_POLICY.md`
- `sort_for_determinism()` on `ProjectModel` guarantees stable ordering for diffing and caching
- `FactClasses` bitflag: callers request only what they need to avoid expensive fact extraction
- `SCHEMA_VERSION` must be bumped on any breaking serialization change
