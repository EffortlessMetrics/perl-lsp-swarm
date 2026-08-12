# perl-corpus
[![Crates.io](https://img.shields.io/crates/v/perl-corpus.svg)](https://crates.io/crates/perl-corpus)
[![Documentation](https://docs.rs/perl-corpus/badge.svg)](https://docs.rs/perl-corpus)

`perl-corpus` owns the repository's reusable Perl corpus infrastructure: curated fixtures, metadata, deterministic generators, corpus inventory, linting, and the helpers used by parser and language-server tests.

It is part of the [perl-lsp](https://github.com/EffortlessMetrics/perl-lsp) workspace. The corpus is layered; no single count in this README is a completeness claim.

## Root and distribution contract

The published crate contains the Rust APIs, schemas, concept registries, and generators listed by its package manifest. The full repository corpus is **not** bundled into the crate package. Repository-backed validation therefore consumes an external corpus root.

```rust,no_run
use perl_corpus::{CorpusPaths, CorpusRootSource};
use std::path::Path;

let resolved =
    CorpusPaths::resolve_authoritative(Some(Path::new("/absolute/path/to/perl-lsp")))?;
resolved.require_repository_layout()?;
assert_eq!(resolved.root_source(), CorpusRootSource::Explicit);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Authoritative selection is explicit absolute root, then `PERL_CORPUS_ROOT`, then a typed failure. The validated APIs return `ResolvedCorpusPaths`, which dereferences to the existing three-field `CorpusPaths` shape while carrying selection provenance separately.

`CorpusPaths::try_discover` adds validated compile-time workspace discovery for developer convenience. `CorpusPaths::discover` remains the historical non-fallible compatibility surface: it does not validate or retain provenance and is not evidence authority.

Required repository layers are traversed recursively before success. Missing, replaced, linked, non-directory, or unreadable nested populations cannot become an empty or partial green corpus. The existing binary and legacy convenience consumers are migrated separately under #7025 and #7033.

## Corpus layers

| Layer | What it provides | Authority |
| --- | --- | --- |
| Curated corpus | Sectioned `.txt` cases with IDs, tags, flags, and optional expected output | `test_corpus/` and corpus parser APIs |
| Parser accuracy | Manifest-backed fixtures with line, AST, symbol, and boundary expectations | `crates/perl-corpus/fixtures/parser_accuracy/manifest.json` |
| Tree-sitter corpus | Grammar-focused syntax sections | `tree-sitter-perl/test/corpus/` |
| Gap fixtures | Focused real-world and boundary examples | `test_corpus/` and `crates/perl-corpus/fixtures/` |
| Generated inputs | Seeded property-based and fuzzing inputs | `crates/perl-corpus/fuzz/` and generator modules |

The parser-accuracy manifest and generated metric receipts are the sources for coverage numbers. Clean parsing, AST accuracy, symbol accuracy, dynamic boundaries, and runtime behavior remain different claims.

## Library API

The crate exposes root authority, section loading and queries, corpus discovery and inventory, fixture/sidecar models, deterministic generators, focused syntax fixtures, linting, metadata backfill, indexes, and snapshots.

```rust
use perl_corpus::{find_by_tag, parse_dir};
use std::path::Path;

let sections = parse_dir(Path::new("test_corpus"))?;
let regex_cases = find_by_tag(&sections, "regex");
println!("{} regex cases", regex_cases.len());
# Ok::<(), Box<dyn std::error::Error>>(())
```

For deterministic generated input, always retain the seed:

```rust
use perl_corpus::generate_perl_code_with_seed;

let source = generate_perl_code_with_seed(10, 42);
assert!(!source.is_empty());
```

## Working with the corpus

```bash
cargo run -p perl-corpus -- --help
cargo xtask --help
cargo xtask metrics --help
cargo test -p perl-parser --test parser_accuracy_e2e
cargo test -p perl-corpus
```

The current binary still owns a legacy `--corpus` surface; the unified root/command contract remains #7033.

## Documentation and evidence

Start with `src/lib.rs`, `docs/project/status/parser.md`, `docs/project/metrics/parser.md`, `crates/perl-parser/tests/parser_accuracy_e2e.rs`, and repository contribution guidance.

A useful corpus change is small, named, reproducible, and evidence-bearing. Preserve the distinction between clean-parse evidence, measured accuracy, compatibility sweeps, and runtime execution.

## License

MIT OR Apache-2.0
