# perl-corpus
[![Crates.io](https://img.shields.io/crates/v/perl-corpus.svg)](https://crates.io/crates/perl-corpus)
[![Documentation](https://docs.rs/perl-corpus/badge.svg)](https://docs.rs/perl-corpus)

`perl-corpus` owns the repository's reusable Perl corpus infrastructure: curated fixtures, metadata, deterministic generators, corpus inventory, linting, and the helpers used by parser and language-server tests.

It is part of the [perl-lsp](https://github.com/EffortlessMetrics/perl-lsp) workspace. The corpus is layered; no single count in this README is a completeness claim.

## Corpus layers

| Layer | What it provides | Authority |
| --- | --- | --- |
| Curated corpus | Sectioned `.txt` cases with IDs, tags, flags, and optional expected output | `test_corpus/` and corpus parser APIs |
| Parser accuracy | Manifest-backed fixtures with line, AST, symbol, and boundary expectations | `crates/perl-corpus/fixtures/parser_accuracy/manifest.json` |
| Tree-sitter corpus | Grammar-focused syntax sections | `tree-sitter-perl/test/corpus/` |
| Gap fixtures | Focused real-world and boundary examples | `test_corpus/` and `crates/perl-corpus/fixtures/` |
| Generated inputs | Seeded property-based and fuzzing inputs | `crates/perl-corpus/fuzz/` and generator modules |

The parser-accuracy manifest and generated metric receipts are the sources for coverage numbers. They distinguish clean parsing from AST accuracy, symbol accuracy, dynamic-boundary handling, and unsupported constructs. A fixture passing the parser does not by itself establish semantic or runtime support.

## Library API

The crate exposes:

- section loading and queries: `parse_file`, `parse_dir`, `find_by_tag`;
- corpus discovery and inventory helpers;
- fixture and sidecar expectation models;
- deterministic Perl generators with explicit seeds;
- focused helpers for heredocs, regexes, globs, tie interfaces, formats, and loop-control cases;
- linting, metadata backfill, indexing, and snapshot support.

Example:

```rust
use perl_corpus::{find_by_tag, parse_dir};
use std::path::Path;

let sections = parse_dir(Path::new("test_corpus"))?;
let regex_cases = find_by_tag(&sections, "regex");
println!("{} regex cases", regex_cases.len());
# Ok::<(), Box<dyn std::error::Error>>(())
```

For deterministic generated input, always record the seed with the resulting test or receipt:

```rust
use perl_corpus::generate_perl_code_with_seed;

let source = generate_perl_code_with_seed(10, 42);
assert!(!source.is_empty());
```

## Working with the corpus

From the repository root:

```bash
# Inspect corpus commands and their current options
cargo xtask --help

# Inspect parser-accuracy commands
cargo xtask metrics --help

# Run the parser's manifest-backed E2E surface
cargo test -p perl-parser --test parser_accuracy_e2e

# Run perl-corpus unit tests
cargo test -p perl-corpus
```

The exact command surface is owned by `xtask` and the workspace test targets; this README does not maintain a shadow CLI contract.

For a new parser-accuracy fixture:

1. Add a focused source file under `crates/perl-corpus/fixtures/parser_accuracy/`.
2. Add one manifest entry with explicit line and AST expectations where parser output is stable; record dynamic or unsupported boundaries instead of guessing.
3. Add the fixture to the public parser E2E selector.
4. Run the focused test and applicable corpus/format checks.
5. Keep the PR claim limited to the measured fixture slice. Do not turn one fixture into a claim about all Perl syntax or runtime semantics.

## Documentation and evidence

Start with:

- `crates/perl-corpus/src/lib.rs` for library-level API and organization;
- `docs/project/status/parser.md` for generated parser and corpus status;
- `docs/project/metrics/parser.md` for scorecard definitions and ratchets;
- `crates/perl-parser/tests/parser_accuracy_e2e.rs` for the public accuracy test surface;
- `AGENTS.md` and the contribution guidance for change and review requirements.

The repository distinguishes:

- **clean-parse evidence**: input was ingested without an error;
- **accuracy evidence**: expected constructs, AST nodes, spans, symbols, or boundaries were measured;
- **compatibility evidence**: a broader system or CPAN corpus was swept;
- **runtime evidence**: behavior was observed by executing Perl.

Those are separate claims and should remain separate in fixture metadata, tests, documentation, and PR descriptions.

## Contributing a gap case

A useful corpus change is small, named, reproducible, and evidence-bearing. Prefer one construct family or one real failure mode per PR. Include the source, metadata, expected result, and a short claim boundary. Use a fixed seed for generated cases and preserve the original input when a case came from a parser failure.

The goal is not to make the corpus look complete. The goal is to make each missing behavior visible, testable, and safe to improve.

## License

MIT OR Apache-2.0
