# perl-lexer
[![Crates.io](https://img.shields.io/crates/v/perl-lexer.svg)](https://crates.io/crates/perl-lexer)
[![Documentation](https://docs.rs/perl-lexer/badge.svg)](https://docs.rs/perl-lexer)

Context-aware Perl lexer with mode-based tokenization for the
[perl-lsp](https://github.com/EffortlessMetrics/perl-lsp) workspace.

## Overview

Handles Perl's inherently context-sensitive grammar by tracking lexer mode
(`ExpectTerm` vs `ExpectOperator`) to disambiguate `/` (division vs regex),
`%` (modulo vs hash sigil), heredocs, quote-like operators, and more. Provides
checkpointing for incremental parsing and deterministic size/depth limits for
pathological input.

## Usage

```rust
use perl_lexer::{PerlLexer, TokenType};

let mut lexer = PerlLexer::new("my $x = 42;");
while let Some(token) = lexer.next_token() {
    if matches!(token.token_type, TokenType::EOF) { break; }
    println!("{:?}: {}", token.token_type, token.text);
}
```

## Configuration contract

`LexerConfig` remains a public struct, but its fields do not all represent
independent implementation modes. Two legacy no-op surfaces are explicitly
deprecated as of 0.17.0 and are planned for removal at a future semver boundary
under open issue #8749:

| Field / feature | Current behavior |
| --- | --- |
| `parse_interpolation` | Controls interpolation recognition in ordinary double-quoted strings. Disabled non-empty strings retain one `Literal` part in the legacy `InterpolatedString` token shape. Quote-like `qq` bodies remain opaque and do not consume this switch. |
| `track_positions` | **Deprecated** (since 0.17.0, planned for future semver-boundary removal by open issue #8749). Compatibility field with no runtime effect. Token byte spans are always produced because parser and editor consumers require them. Migration: remove the field from struct literals (or route the rest of the literal through `..LexerConfig::default()`); token output is identical. |
| `max_lookahead` | Maximum zero-based offset admitted by shared character, byte, and fixed-pattern cursor probes. `0` permits only the current offset/one-byte pattern; larger values can change identifier, operator, delimiter, numeric, Unicode, and BOM decisions. |
| `symbol_table` | Optionally supplies file-local subroutine names for the declared bareword/regex ambiguity. |
| Cargo feature `simd` | **Deprecated** (since 0.17.0, planned for future semver-boundary removal by open issue #8749). Compatibility no-op: it selects no distinct implementation, no code depends on it, and `--features simd` builds are equivalent to the default build. No SIMD performance claim is made. |

Use `LexerConfig::DEFAULT_MAX_LOOKAHEAD`,
`LexerConfig::POSITIONS_ARE_ALWAYS_TRACKED`, and the query methods on
`LexerConfig` instead of inferring behavior from historical field names.
Checkpoints do not embed lexer configuration, so legacy no-op variation never
invalidates a captured checkpoint. This deprecation PR does not remove either
surface; the open removal issue owns that later compatibility boundary.

## License

Licensed under either of [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
or [MIT license](http://opensource.org/licenses/MIT) at your option.
