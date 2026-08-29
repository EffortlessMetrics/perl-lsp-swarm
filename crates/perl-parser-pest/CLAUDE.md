# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Crate**: `perl-parser-pest`
- **Version**: `0.17.0`, declared literally in this crate's own `Cargo.toml` (not inherited — see "Standalone manifest" below)
- **Tier**: 7 (Legacy/testing)
- **Purpose**: Legacy Pest-based Perl parser (v2) -- maintained as a learning tool, compatibility reference, and benchmark baseline. NOT in the default CI gate.

## Commands

```bash
cargo build -p perl-parser-pest          # Build
cargo test -p perl-parser-pest           # Run tests
cargo clippy -p perl-parser-pest         # Lint
cargo doc -p perl-parser-pest --open     # View docs
```

## Architecture

### Source Modules

| Module | Purpose |
|--------|---------|
| `pure_rust_parser` | `PerlParser` (Pest grammar), `PureRustPerlParser` (high-level API), `AstNode` enum |
| `pratt_parser` | `PrattParser` for Perl operator precedence (Pratt/TDOP algorithm) |
| `sexp_formatter` | `SexpFormatter` and `SexpBuilder` for tree-sitter-compatible S-expression output |
| `error` | `ParseError`, `ParseResult`, `ScannerError`, `UnicodeError` types |
| `outcome` | Typed parse outcome / diagnostic / original-source range vocabulary (`#8427`). Substrate only; does not change `parse()` |

### Key Types (re-exported from `lib.rs`)

- `PureRustPerlParser` -- main entry point: `new()`, `parse()`, `to_sexp()`
- `PerlParser` -- Pest-derived parser struct (generates `Rule` enum via `#[grammar = "grammar.pest"]`)
- `AstNode` -- large enum covering program structure, declarations, control flow, expressions, variables, literals, regex, heredocs, modern Perl (try/catch, class, field, method, role), and error recovery nodes
- `PrattParser` -- operator-precedence parser with `Precedence`, `Associativity`, `OpInfo`
- `SexpFormatter` -- configurable formatter with `.with_positions()` and `.compact()` builder methods
- `ParseError` / `ParseResult<T>` -- serializable error types with `thiserror` derives
- `ParseOutcome` / `ParseAttempt` / `StrictParseError` / `ParserFailure` / `SourceRange` -- typed completeness, rejection, and instrument-failure vocabulary (`#8427`). Not consumed by `parse()` yet

### Dependencies

Every dependency is a published ecosystem crate pinned to an explicit version in
this crate's manifest. There are no path or workspace-alias dependencies.

- `pest`, `pest_derive` -- PEG parser generator (grammar in `src/grammar.pest`)
- `stacker` -- stack overflow protection for deep recursion
- `thiserror` -- error derive macros
- `serde` (with `derive`) -- serialization (always enabled; the `serde` feature flag is a no-op alias)
- `regex` -- pattern matching within parser

Dev-dependencies: `serde_json`, `sha2`, `tempfile`, `toml` -- all published.

### Three-Stage Pipeline

1. **Pest Parsing** -- PEG grammar (`grammar.pest`) produces a parse tree
2. **AST Building** -- `build_ast()` / `build_node()` construct typed `AstNode` tree with Pratt parsing for operator expressions
3. **S-Expression Output** -- `SexpFormatter::format()` generates tree-sitter-compatible strings

## Usage

```rust
use perl_parser_pest::PureRustPerlParser;

let mut parser = PureRustPerlParser::new();
let ast = parser.parse("my $x = 42;")?;
let sexp = parser.to_sexp(&ast);
```

## Fixture manifest (test substrate)

Package-local fixture identity for the pest train lives under `tests/fixtures/`.
The reusable runner is `tests/support/` and is exercised by
`cargo test -p perl-parser-pest --test fixture_manifest`. Rows record current
parse observations only; they do not declare the parser correct or replace
existing inline tests.

```text
tests/fixtures/manifest.toml
tests/fixtures/sources/**
tests/fixture_manifest.rs
tests/support/**
```

Load and select through a caller-supplied package root (`CARGO_MANIFEST_DIR`),
not the workspace root. Duplicate IDs, path escape, missing sources, empty
selection, and parser panics fail closed as instrument errors.

## Standalone manifest (`#8771`)

This package describes and tests itself without borrowing from the workspace.
`Cargo.toml` carries literal identity, MSRV, dependency versions, and a literal
`[lints.clippy]` / `[lints.rust]` policy instead of `*.workspace = true`, and no
dependency or dev-dependency is path-only. `tests/standalone_package.rs` is the
guard: it fails closed on reintroduced workspace inheritance, a path dependency,
a dropped lint denial, a falsely-external `repository`/`homepage`, an unpackaged
load-bearing asset, or a returning swarm test-helper import.

Two consequences for anyone editing this crate:

- **Do not reintroduce `workspace = true` here.** When root `[workspace.lints]`
  changes, mirror the change into this manifest deliberately.
- **Do not add a path dependency**, including test helpers. `perl-tdd-support`
  is replaced by `tests/support/assert.rs` (`must` / `must_err`, same
  `#[track_caller]` and type-name diagnostics), included per test binary via
  `#[path = "support/assert.rs"] mod assert;`. The `src/pure_rust_parser.rs`
  unit tests carry their own file-local copy so the v2 bundle twin stays
  byte-identical.

`repository`/`homepage` deliberately still name the current swarm/public
lineage; the external `perl-parser-pest` repository does not exist yet. The
pending owner is recorded under `[package.metadata.extraction]`, not by
hard-coding a future URL.

Known limitation: the package's `include` set and manifest are proven
structurally. Executing an unpacked copy outside the workspace is the next
train row's claim, not this one's.

## Public example

`examples/parse_basic.rs` is the compiled proof that the documented entry point
still type-checks — `[lib] doctest = false` means the README snippet is not.
Keep them in step.

## Important Notes

- **NOT in default gate** -- excluded from `just ci-gate`; build and test independently
- **v2 bundle sync** -- `grammar.pest`, `pure_rust_parser.rs`, `pratt_parser.rs`, `sexp_formatter.rs`, and `error.rs` are shared with `tree-sitter-perl-rs`; always sync both copies (verify with `just ci-v2-bundle-sync`)
- **No new features** -- this crate is frozen; use `perl-parser` (v3) for active development
- **doctest disabled** -- `[lib] doctest = false` in Cargo.toml
- The `serde` feature flag is a backward-compatible no-op; serde support is always compiled in
