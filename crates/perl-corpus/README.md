# perl-corpus
[![Crates.io](https://img.shields.io/crates/v/perl-corpus.svg)](https://crates.io/crates/perl-corpus)
[![Documentation](https://docs.rs/perl-corpus/badge.svg)](https://docs.rs/perl-corpus)

`perl-corpus` owns the repository's reusable Perl corpus infrastructure: curated fixtures, metadata, deterministic generators, corpus inventory, linting, and the helpers used by parser and language-server tests.

It is part of the [perl-lsp](https://github.com/EffortlessMetrics/perl-lsp) workspace. The corpus is layered; no single count in this README is a completeness claim.

## Root and distribution contract

The published crate contains the Rust APIs, schemas, concept registries, and generators listed by its package manifest. The full repository corpus is **not** bundled into the crate package. Repository-backed validation therefore consumes an external corpus root.

Load-bearing callers must select an absolute root explicitly or set `PERL_CORPUS_ROOT`:

```rust,no_run
use perl_corpus::{CorpusPaths, CorpusRootSource};
use std::path::Path;

let resolved =
    CorpusPaths::resolve_authoritative(Some(Path::new("/absolute/path/to/perl-lsp")))?;
resolved.require_repository_layout()?;
assert_eq!(resolved.root_source(), CorpusRootSource::Explicit);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Selection order for authoritative operations is:

1. an explicit absolute root;
2. `PERL_CORPUS_ROOT`;
3. otherwise, a typed error.

The validated APIs return `ResolvedCorpusPaths`, which dereferences to the existing `CorpusPaths` fields while carrying selection provenance separately. This preserves the published three-field `CorpusPaths` struct-literal and destructuring surface.

`CorpusPaths::try_discover` additionally supports bounded compile-time workspace discovery for developer convenience. `CorpusPaths::discover` is a non-fallible compatibility surface: it returns the historical `CorpusPaths` shape without validation or provenance and must not be used as evidence authority.

Validated roots reject symbolic-link components and are revalidated when required layers are resolved. Required layer trees are traversed recursively before success, so a missing, replaced, linked, non-directory, or unreadable nested population cannot become a successful empty or partial repository corpus. The existing binary and legacy convenience consumers are migrated separately under #7025 and #7033.

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

- explicit corpus-root authority through `CorpusRoot`, `ResolvedCorpusPaths`, and `CorpusPaths`;
- distinct plain-source and sectioned-document loading contracts;
- section loading and queries: `parse_file`, `parse_dir`, `find_by_tag`;
- corpus discovery and inventory helpers;
- fixture and sidecar expectation models;
- deterministic Perl generators with explicit seeds;
- focused helpers for heredocs, regexes, globs, tie interfaces, formats, and loop-control cases;
- linting, metadata backfill, indexing, and snapshot support.

### Typed source loading

Ordinary Perl sources and sectioned corpus documents use different APIs:

```rust,no_run
use perl_corpus::{load_plain_perl_source, load_sectioned_corpus_document};

let plain = load_plain_perl_source(
    "test_corpus/example.pl",
    "/absolute/root/test_corpus/example.pl",
)?;
let sectioned = load_sectioned_corpus_document(
    "tree_sitter/corpus/expressions.txt",
    "/absolute/root/tree-sitter-perl/test/corpus/expressions.txt",
)?;
assert!(!plain.source.is_empty());
assert!(!sectioned.cases.is_empty());
# Ok::<(), Box<dyn std::error::Error>>(())
```

A `.txt` extension does not make an asset sectioned. The topology or consumer chooses the loader.

Plain loading opens the selected leaf without following a symbolic link or Windows reparse point, verifies the opened handle is a regular file, and reads bytes from that same handle. It preserves exact UTF-8 text, BOM presence, and newline representation; delimiter-looking Perl content is never reinterpreted. Platforms without a reviewed no-follow open contract fail explicitly.

Sectioned loading retains the same exact source but normalizes newlines only for its parser view. Every delimiter candidate must have a non-empty title and closing delimiter, the structurally declared and parsed populations must match exactly, and duplicate effective IDs fail the document.

`SectionCaseId { asset_id, section_id }` is the stable case authority. The legacy `Section.id` fallback remains leaf-derived compatibility data and may collide across parent assets; it is not promoted as global corpus identity.

Legacy `parse_file` and `parse_dir` remain compatibility APIs pending the topology migrations in #6985 and #6989. Intermediate-component containment also remains topology/path-authority work; the direct loader protects the selected leaf and opened bytes.

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
# Inspect the current binary. Its root/command migration remains #7033.
cargo run -p perl-corpus -- --help
cargo xtask --help

# Inspect parser-accuracy commands
cargo xtask metrics --help

# Run the parser's manifest-backed E2E surface
cargo test -p perl-parser --test parser_accuracy_e2e

# Run perl-corpus unit and integration tests
cargo test -p perl-corpus
```

The exact command surface is owned by the binary, `xtask`, and workspace test targets; this README does not maintain an independent shadow of every subcommand.

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
