# perl-lexer
[![Crates.io](https://img.shields.io/crates/v/perl-lexer.svg)](https://crates.io/crates/perl-lexer)
[![Documentation](https://docs.rs/perl-lexer/badge.svg)](https://docs.rs/perl-lexer)

Context-aware Perl lexer with mode-based tokenization for the
[perl-lsp](https://github.com/EffortlessMetrics/perl-lsp) workspace.

## Overview

Handles Perl's inherently context-sensitive grammar by tracking lexer mode
(`ExpectTerm` vs `ExpectOperator`) to disambiguate `/` (division vs regex),
`%` (modulo vs hash sigil), heredocs, quote-like operators, and more. Provides
checkpointing for incremental parsing and budget limits to guard against
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
independent implementation modes:

| Field / feature | Current behavior |
| --- | --- |
| `parse_interpolation` | Enables structured `StringPart` segmentation for supported interpolating strings. Disabling it keeps interpolation-looking text inside the literal token. |
| `track_positions` | Compatibility field. Token byte spans are always produced because parser and editor consumers require them. |
| `max_lookahead` | `0` disables package-qualified `::segment` continuation; any non-zero value enables the current one-boundary lookahead path. It is not a general scan budget. |
| `symbol_table` | Optionally supplies file-local subroutine names for bareword/regex disambiguation. |
| Cargo feature `simd` | Compatibility no-op. It currently selects no distinct implementation; issue #6715 owns implementation or removal. |

Use `LexerConfig::DEFAULT_MAX_LOOKAHEAD` and
`LexerConfig::POSITIONS_ARE_ALWAYS_TRACKED` instead of inferring these contracts
from historical field names.

## License

Licensed under either of [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
or [MIT license](http://opensource.org/licenses/MIT) at your option.
