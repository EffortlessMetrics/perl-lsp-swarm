# perl-parser-pest

Legacy Pest-based Perl parser (v2) for the [tree-sitter-perl-rs](https://github.com/EffortlessMetrics/perl-lsp) workspace.

## Overview

A pure Rust Perl parser built on the [Pest](https://pest.rs/) PEG parser generator. Parses Perl 5 source into a typed `AstNode` enum and can emit tree-sitter-compatible S-expressions. Maintained as a learning tool, compatibility reference, and benchmark baseline -- not for production use (see `perl-parser` v3 instead).

## Public API

| Type | Description |
|------|-------------|
| `PureRustPerlParser` | Main parser: `parse()` returns `AstNode`, `to_sexp()` formats output |
| `PerlParser` | Pest-derived grammar entry point (exposes `Rule` enum) |
| `AstNode` | Typed AST covering declarations, control flow, expressions, literals, and modern Perl features |
| `PrattParser` | Operator-precedence parser for Perl's expression grammar |
| `SexpFormatter` | Configurable S-expression output (positions, compact mode) |
| `ParseError` | Canonical error union returned by every fallible API: `Rejected` (parser-domain rejection) or `Failed` (operational/instrument failure). The two are never interconvertible by type |
| `StrictParseError` / `ParserFailure` / `SourceRange` | The rejection, instrument-failure, and range types carried by `ParseError`. A `Rejected` range from `parse()` refers to the *normalized* source Pest parsed, not the caller's original text |
| `ParseOutcome` / `ParseAttempt` / `ParseCompleteness` / `ParseDiagnostic` | Typed completeness and diagnostic vocabulary. Not used by `parse()`'s success side, which still returns a bare `AstNode` |

## Usage

```rust
use perl_parser_pest::PureRustPerlParser;

let mut parser = PureRustPerlParser::new();
let ast = parser.parse("my $x = 42;").expect("parse failed");
println!("{}", parser.to_sexp(&ast));
```

## License

MIT OR Apache-2.0
