# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Crate**: `perl-parser-pest`
- **Version**: workspace (currently 0.12.3)
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

- `pest`, `pest_derive` -- PEG parser generator (grammar in `src/grammar.pest`)
- `stacker` -- stack overflow protection for deep recursion
- `thiserror` -- error derive macros
- `serde`, `postcard` -- serialization (always enabled; `serde` feature flag is a no-op alias)
- `regex` -- pattern matching within parser
- `unicode-ident` -- Unicode identifier support
- `once_cell`, `lazy_static` -- lazy initialization

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

## Important Notes

- **NOT in default gate** -- excluded from `just ci-gate`; build and test independently
- **v2 bundle sync** -- `grammar.pest`, `pure_rust_parser.rs`, `pratt_parser.rs`, `sexp_formatter.rs`, and `error.rs` are shared with `tree-sitter-perl-rs`; always sync both copies (verify with `just ci-v2-bundle-sync`)
- **No new features** -- this crate is frozen; use `perl-parser` (v3) for active development
- **doctest disabled** -- `[lib] doctest = false` in Cargo.toml
- The `serde` feature flag is a backward-compatible no-op; serde support is always compiled in
