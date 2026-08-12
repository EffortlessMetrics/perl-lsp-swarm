# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Tier**: 7 (testing/legacy crate)
- **Version**: workspace
- **Purpose**: Test corpus management, property-based generators, and edge case fixtures for Perl parser and LSP/DAP testing.
- **Distribution boundary**: the crate package ships APIs, concepts, and generators; repository corpus assets are selected through an external root.

## Commands

```bash
cargo build -p perl-corpus
cargo test -p perl-corpus
cargo test -p perl-corpus --features ci-fast
cargo run -p perl-corpus -- --help

# The current binary still uses its legacy --corpus argument. Root and
# command migration remains #7033; do not treat the binary as root authority.

# Generation is self-contained; always retain the seed.
cargo run -p perl-corpus -- gen program --count 10 --seed 42

cargo clippy -p perl-corpus
cargo doc -p perl-corpus --open
```

## Root authority

`CorpusRoot` is the validated root-selection authority. `ResolvedCorpusPaths` carries that selection provenance without changing the published `CorpusPaths` field layout.

- `CorpusRoot::resolve_authoritative` and `CorpusPaths::resolve_authoritative` accept an explicit absolute root first, then `PERL_CORPUS_ROOT`, and otherwise fail.
- The validated `CorpusPaths` constructors return `ResolvedCorpusPaths`, which dereferences to `CorpusPaths` and records the source separately.
- `CorpusPaths::try_discover` adds validated compile-time workspace discovery for developer convenience only.
- `CorpusPaths::discover` is a non-fallible compatibility surface. It returns the original three-field shape without validation or provenance and is not evidence authority.
- Do not add fields to `CorpusPaths`; downstream struct literals and destructuring are part of the existing published surface.
- `CorpusPaths::require_repository_layout` recursively traverses `test_corpus/` and `crates/perl-corpus/fuzz/`, propagates nested enumeration and metadata failures, and rejects nested symbolic links.
- A bound root and its top-level layer are revalidated around recursive traversal so a persistent rename/replacement through a symbolic link cannot inherit the earlier verdict.
- Workspace discovery parses candidate `Cargo.toml` files as TOML and requires a real top-level `[workspace]` table.
- Relative roots are rejected because their identity changes with the current working directory.
- The package manifest deliberately excludes the repository asset trees and declares `package.metadata.perl-corpus.repository-assets = "external-root"`.

## Typed loading authority

`load_plain_perl_source` and `load_sectioned_corpus_document` are deliberately different contracts.

- Loader selection comes from topology or the consumer. Never infer sectioned format from `.txt` alone.
- The selected leaf is opened with a platform-reviewed no-follow contract, metadata is read from that opened handle, and bytes are read from the same handle.
- Symbolic-link/reparse leaves, non-regular files, invalid UTF-8, and platforms without a reviewed no-follow contract fail explicitly.
- Plain loading preserves the exact UTF-8 source, including BOM and newline representation. It does not interpret delimiter-looking Perl text.
- Sectioned loading preserves exact source separately from its newline-normalized parser view.
- Every section delimiter candidate must have a non-empty title and closing delimiter. The declared header count and parsed section count must agree exactly.
- Duplicate effective IDs fail the document.
- `SectionCaseId { asset_id, section_id }` is the stable identity. The legacy `Section.id` fallback remains leaf-derived compatibility data and may collide across parent assets.
- Intermediate-component containment remains topology/path-authority work under #6985, #6989, and #6994; do not overstate direct loader containment.

## Architecture

### Dependencies

- `proptest` - Property-based testing strategies and test runners
- `rand` - Seeded random generation for deterministic codegen
- `serde`, `serde_json` - Corpus index serialization
- `regex` - Section delimiter and metadata parsing
- `glob` - Legacy corpus file discovery pending topology migration
- `clap` - CLI argument parsing
- `chrono` - Timestamps in coverage reports
- `anyhow` - Error handling

### Key Types and Modules

| Type/Module | Location | Purpose |
|-------------|----------|---------|
| `CorpusRoot` / `CorpusRootSource` / `CorpusRootError` | `api/root.rs` | Explicit external-root selection, path validation, rebinding checks, and top-level layer validation |
| `CorpusPaths` / `ResolvedCorpusPaths` | `files.rs` | Preserved public path shape plus separate validated provenance wrapper and recursive layer proof |
| `CorpusTopology` / `CorpusAsset` | `api/topology.rs` | Versioned root-relative topology identity for migrated asset populations |
| `PlainPerlSource` / `SectionedCorpusDocument` / `CorpusLoadError` | `loading/typed.rs` | Explicit plain-versus-sectioned loading, opened-handle source authority, structured case identity, and typed failures |
| `Section` | `meta.rs` | Parsed corpus section with id, title, tags, flags, body, line number |
| `CorpusFile` / `CorpusLayer` | `files.rs` | Current compatibility discovery classifications |
| `EdgeCase` / `EdgeCaseGenerator` | `cases.rs` | Static edge case fixtures with tag filtering and deterministic sampling |
| `ComplexDataStructureCase` | `cases.rs` | Static complex data structure samples for DAP variable inspection |
| `ContinueRedoCase` | `continue_redo.rs` | Continue/redo loop control fixtures with parse expectation flags |
| `FormatStatementCase` / `FormatStatementGenerator` | `format_statements.rs` | Format/formline statement fixtures |
| `GlobExpressionCase` / `GlobExpressionGenerator` | `glob_expressions.rs` | Glob and diamond operator fixtures |
| `TieInterfaceCase` | `tie_interface.rs` | Tie/untie/tied mechanism fixtures |
| `CodegenOptions` / `StatementKind` | `codegen.rs` | Randomized Perl code generation |
| `LintConfig` / `LintResult` | `lint.rs` | Corpus validation |
| `gen::*` | `gen/` | Proptest strategy modules pending the versioned generator registry |

### Public API

- `CorpusRoot::resolve_authoritative` / `CorpusPaths::resolve_authoritative` - load-bearing root selection returning `ResolvedCorpusPaths`
- `CorpusPaths::try_discover` - validated developer convenience discovery returning `ResolvedCorpusPaths`
- `CorpusPaths::discover` / `CorpusPaths::from_root` - original unchecked compatibility shape
- `load_plain_perl_source` - strict UTF-8 ordinary source loading without delimiter interpretation
- `load_sectioned_corpus_document` - strict section expansion with structured parent-plus-section IDs
- `parse_file(path)` / `parse_dir(dir)` - legacy sectioned corpus compatibility APIs
- `find_by_tag(sections, tag)` / `find_by_flag(sections, flag)` - filter sections
- `generate_perl_code_with_seed(n, seed)` - deterministic code generation
- `edge_cases()` / `complex_data_structure_cases()` - static fixture accessors
- `get_corpus_files()` / `get_all_test_files()` - legacy convenience discovery pending topology migration

## Important Notes

- The `gen` module is accessed as `r#gen` in Rust source.
- `PERL_CORPUS_ROOT` is the only supported root environment variable; `CORPUS_ROOT` is not authoritative.
- Do not add current-working-directory fallback to load-bearing paths.
- Do not infer loader type from `.txt` alone.
- Do not accept a partial section population because at least one section parsed.
- Do not validate one path and reopen it for the load-bearing read; authority stays with one opened handle.
- Do not treat legacy `Section.id` as global asset authority.
- Do not package the complete repository corpus implicitly. A self-contained asset distribution would require a separate reviewed contract.
- Required selected assets and directories must fail closed on absence, symbolic link, non-regular type, unreadable state, or escape.
- Generated inputs used as evidence require an explicit seed and eventual registry/profile identity under #6708.
