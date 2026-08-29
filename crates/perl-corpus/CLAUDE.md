# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Tier**: 7 (testing/legacy crate)
- **Version**: workspace
- **Purpose**: Test corpus management, property-based generators, and edge case fixtures for Perl parser and LSP/DAP testing.

## Commands

```bash
cargo build -p perl-corpus
cargo test -p perl-corpus
cargo test -p perl-corpus --features ci-fast
cargo test -p perl-corpus --test root_path_authority
cargo test -p perl-corpus --test distribution_contract
cargo run -p perl-corpus -- --help

# Generation is self-contained; always retain the seed.
cargo run -p perl-corpus -- gen program --count 10 --seed 42

cargo package -p perl-corpus --allow-dirty --list
cargo clippy -p perl-corpus --all-targets -- -D warnings -A missing_docs
cargo doc -p perl-corpus --open
```

## Root authority

`CorpusRoot` and `CorpusPaths` serve different contracts.

- `CorpusRoot::resolve_authoritative(explicit)` selects explicit input, then `PERL_CORPUS_ROOT`, then returns `AuthoritativeRootRequired`.
- Invalid explicit input fails immediately. It never falls through to a valid environment value or workspace discovery.
- Strict roots must be absolute, directories, and free of symbolic-link or Windows reparse-point components.
- A strict root retains a shared open `same_file::Handle`. The canonical path is diagnostic context; clones share the retained directory identity and do not reopen the path.
- `CorpusRoot::require_repository_layout()` proves only the `test_corpus/` and `crates/perl-corpus/fuzz/` directory chains. It does not recurse, select extensions, inspect leaves, or redefine `CorpusTopology`.
- `CorpusPaths::discover()` and `CorpusPaths::from_root()` remain unchecked compatibility APIs. Their raw mutable paths are never authority.
- `CorpusPaths::try_from_root`, `try_discover`, and `resolve_authoritative` return immutable `ResolvedCorpusPaths`; `into_paths()` is an explicit authority downgrade.
- `ResolvedCorpusPaths` must not implement `Deref`, `AsRef<CorpusPaths>`, `Borrow<CorpusPaths>`, or any other implicit conversion into `CorpusPaths`. The downgrade is written down at the call site as `as_paths()` (borrowed view) or `into_paths()` (consuming). `tests/root_path_authority.rs` holds this boundary with `assert_does_not_implement!`, which breaks the build of that test target if such an impl reappears; a `compile_fail` example in `files.rs` documents the resulting call-site error. Put the enforcement in the integration test, not only the doctest: the gates run `cargo test --locked --tests` and never `cargo test --doc`.
- Component-by-component selected-member opening must consume the retained root capability. Do not add another root-opening path.
- The published package ships APIs and deliberately included crate assets. Repository corpus data remains an external root.

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
- `same-file` - Cross-platform retained directory and opened-file identity
- `toml` - Structured topology and package-contract parsing
- `clap` - CLI argument parsing
- `chrono` - Timestamps in coverage reports
- `anyhow` - Error handling

### Key Types and Modules

| Type/Module | Location | Purpose |
|-------------|----------|---------|
| `CorpusRoot` / `CorpusRootError` / `CorpusRootSource` | `api/root.rs` | Strict external root selection, retained directory identity, provenance, and typed failures |
| `CorpusPaths` / `ResolvedCorpusPaths` | `files.rs` | Unchecked compatibility paths versus immutable paths bound to strict root authority |
| `CorpusFile` / `CorpusLayer` | `files.rs` | Legacy layer classification |
| `CorpusTopology` / `CorpusAsset` | `api/topology.rs` | Versioned root-relative topology identity for migrated asset populations |
| `PlainPerlSource` / `SectionedCorpusDocument` / `CorpusLoadError` | `loading/typed.rs` | Explicit plain-versus-sectioned loading, opened-handle source authority, structured case identity, and typed failures |
| `Section` | `meta.rs` | Parsed corpus section with id, title, tags, flags, body, line number |
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

- `CorpusRoot::resolve_authoritative` / `CorpusRoot::explicit` / `CorpusRoot::try_discover` - strict typed root authority
- `CorpusPaths::try_from_root` / `CorpusPaths::resolve_authoritative` - strict paths retaining their root authority
- `CorpusPaths::discover` / `CorpusPaths::from_root` - unchecked compatibility discovery
- `load_plain_perl_source` - strict UTF-8 ordinary source loading without delimiter interpretation
- `load_sectioned_corpus_document` - strict section expansion with structured parent-plus-section IDs
- `parse_file(path)` / `parse_dir(dir)` - legacy sectioned corpus compatibility APIs
- `find_by_tag(sections, tag)` / `find_by_flag(sections, flag)` - filter sections
- `generate_perl_code_with_seed(n, seed)` - deterministic code generation
- `edge_cases()` / `complex_data_structure_cases()` - static fixture accessors
- `get_corpus_files()` / `get_all_test_files()` - legacy convenience discovery pending topology migration

## Important Notes

- The `gen` module is accessed as `r#gen` in Rust source.
- Do not pass `CorpusPaths::discover()` where evidence authority is required.
- Do not add current-working-directory fallback to load-bearing paths.
- Do not reopen a root pathname when a retained `CorpusRoot` capability is available.
- Do not recursively turn root validation into topology, population, member, or leaf policy.
- Do not infer loader type from `.txt` alone.
- Do not accept a partial section population because at least one section parsed.
- Do not validate one path and reopen it for the load-bearing read; authority stays with one opened handle.
- Do not treat legacy `Section.id` as global asset authority.
- Do not package the complete repository corpus implicitly. A self-contained asset distribution requires a separate reviewed contract.
- Required selected assets and directories must fail closed on absence, symbolic link/reparse point, non-regular type, unreadable state, or escape under their owning layer.
- Generated inputs used as evidence require an explicit seed and eventual registry/profile identity under #6708.
