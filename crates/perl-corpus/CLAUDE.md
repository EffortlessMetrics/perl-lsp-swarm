# CLAUDE.md

This file provides guidance to Claude Code when working with this crate.

## Crate overview

- **Tier**: 7 (testing/legacy crate)
- **Version**: workspace
- **Purpose**: Corpus management, deterministic generators, and parser/LSP/DAP fixtures.
- **Distribution boundary**: APIs, concepts, and generators ship in the crate; repository corpus assets are selected through an external root.

## Commands

```bash
cargo build -p perl-corpus
cargo test -p perl-corpus
cargo test -p perl-corpus --features ci-fast
cargo run -p perl-corpus -- --help
cargo run -p perl-corpus -- gen program --count 10 --seed 42
cargo clippy -p perl-corpus
cargo doc -p perl-corpus --open
```

The binary's root/command migration remains #7033. Do not treat its legacy `--corpus` argument as the new authority.

## Root authority

`CorpusRoot` validates root selection. `ResolvedCorpusPaths` carries provenance without changing the published three-field `CorpusPaths` layout.

- `CorpusPaths::resolve_authoritative`: explicit absolute root, then `PERL_CORPUS_ROOT`, otherwise failure.
- `CorpusPaths::try_discover`: validated developer workspace discovery.
- `CorpusPaths::discover` and `from_root`: original unchecked compatibility shape; not evidence authority.
- Do not add fields to `CorpusPaths`; downstream struct literals/destructuring are part of the public surface.
- `require_repository_layout` recursively traverses `test_corpus/` and `crates/perl-corpus/fuzz/`, propagates nested enumeration/metadata failures, and rejects nested symbolic links.
- The root and top-level layer are revalidated around traversal. This is still path-based authority, not a capability-safe directory handle.
- Workspace discovery parses `Cargo.toml` as TOML and requires a real top-level `[workspace]` table.
- Relative roots are rejected.
- Package metadata declares `repository-assets = "external-root"`.

## Key types

| Type | Location | Purpose |
|---|---|---|
| `CorpusRoot`, `CorpusRootError`, `CorpusRootSource` | `api/root.rs` | Root selection, provenance, validation, rebinding checks |
| `CorpusPaths`, `ResolvedCorpusPaths` | `files.rs` | Preserved compatibility shape plus validated provenance wrapper and recursive layout proof |
| `CorpusTopology`, `CorpusAsset` | `api/topology.rs` | Versioned root-relative asset identity |
| `Section` | `metadata/section.rs` | Parsed section metadata and source body |
| `EdgeCaseGenerator`, specialized case modules | crate modules | Static and generated Perl fixtures |

## Important rules

- `PERL_CORPUS_ROOT` is the only supported root environment variable.
- Do not add current-working-directory fallback to load-bearing operations.
- Do not treat top-level existence or one successful `read_dir` as recursive completeness.
- Do not bundle the complete repository corpus implicitly.
- Required assets must fail closed on absence, symbolic link, non-regular type, unreadable state, or escape.
- Generated evidence requires explicit deterministic seed/profile identity under #6708.
