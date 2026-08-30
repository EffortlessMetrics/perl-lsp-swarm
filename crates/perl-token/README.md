# perl-token

Core token type definitions for the Perl parser ecosystem.

## Overview

`perl-token` is a Tier 1 leaf crate that defines the shared token types used
across the lexer, tokenizer, and parser crates. It has zero external
dependencies (only `std::sync::Arc`).

## Stability Contract

`perl-token` is a **tiny stable leaf crate** and should stay dependency-free at runtime.
Public `Token`/`TokenKind` source compatibility is intentionally conservative and should
only change with explicit, reviewed intent.

- TokenKind variants: 132
- Conformance update rule: adding a `TokenKind` variant must also update metadata,
  this crate's docs, and the conformance guard tests.

## Public API

- **`Token`** -- `text` is public; `kind()` is a read accessor; byte geometry is private. Construct with [`Token::new_checked`], [`Token::eof_at`], [`Token::unknown_at`], or the payload-free [`Token::unknown_rest_at`], and read offsets via `start()` / `end()`. Change kind with [`Token::with_kind`]. `text.len()` must equal `end - start`, except for geometry-only `UnknownRest`; empty `Eof` / `Unknown` tokens must have empty text.
- **`TokenRef<'src>`** -- borrowed token view with the same geometry seal; construct with [`TokenRef::new_checked`]. `text` remains public for compatibility, and `to_owned_token()` revalidates externally replaced text. Constructor-created geometry-only `UnknownRest` stays payload-free after mutation, while valid-width payload-bearing `UnknownRest` values round-trip their payload. An empty `UnknownRest` with non-empty geometry is the explicit payload-free representation.
- **`TokenSpan`** -- ordered byte span with private fields; construct with [`TokenSpan::try_new`].
- **`TokenKind`** -- closed/exhaustive enum classifying every Perl token: keywords, operators, delimiters, literals, sigils, and special tokens (#2898)
- **`Token` / `TokenRef` / `TokenSpan` / `TokenSpanError` / `TokenCategory` / `TokenKindMetadata`** -- `#[non_exhaustive]` (#2898). This crate has no `TokenOrigin` or `TokenStatus` types.
- **Spelling tables** -- `KEYWORD_SPELLINGS`, `OPERATOR_SPELLINGS`, `DELIMITER_SPELLINGS`, and `SIGIL_SPELLINGS` define fixed source spellings and power `TokenKind::canonical_spelling()`

## Usage

```rust
use perl_token::{Token, TokenKind, TokenRef};

let tok = Token::new_checked(TokenKind::Identifier, "foo", 0, 3).expect("valid token");
assert_eq!(tok.kind(), TokenKind::Identifier);
assert_eq!(tok.start(), 0);
assert_eq!(tok.end(), 3);

let borrowed = TokenRef::new_checked(TokenKind::Identifier, "foo", 0, 3).expect("valid token");
let owned = borrowed.to_owned_token();
assert_eq!(owned, tok);
```

Fixed-spelling token metadata:

```rust
use perl_token::TokenKind;

assert_eq!(TokenKind::Sub.canonical_spelling(), Some("sub"));
assert_eq!(TokenKind::LeftBrace.canonical_spelling(), Some("{"));
assert_eq!(TokenKind::Identifier.canonical_spelling(), None);
```

Borrowed view from owned tokens:

```rust
use perl_token::{Token, TokenKind};

let tok = Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token");
let borrowed = tok.as_ref_token();
assert_eq!(borrowed.text, "my");
assert_eq!(borrowed.start(), 0);
```

## Benchmark scorecard

Run the token allocation scorecard benchmark:

```bash
cargo bench -p perl-token --bench token_scorecard
```

The benchmark group includes:
- `token/borrowed_construction` (no `Arc` allocation)
- `token/owned_construction` (`Arc<str>` allocation path)
- `token/borrowed_to_owned_conversion` (explicit conversion cost)

## Workspace Role

Foundational crate consumed by `perl-lexer`, `perl-tokenizer`, `perl-parser-core`,
and downstream parser/LSP crates. Part of the
[tree-sitter-perl-rs](https://github.com/EffortlessMetrics/perl-lsp) workspace.

## License

MIT OR Apache-2.0
