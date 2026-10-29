# perl-parser-core

Core parsing engine for Perl source.

Use this crate when you need the parser, AST, position mapping, or trivia
preservation layers without the higher-level workspace or LSP facades.

## Where it fits

`perl-parser-core` is the low-level parsing layer used by `perl-parser`,
`perl-semantic-analyzer`, `perl-workspace`, and `perl-refactoring`. It
turns source text into AST nodes and exposes the building blocks that higher
layers compose.

## Key entry points

- `Parser` - recursive-descent parser
- `ParseError`, `ParseResult`, `ParseOutput`
- `Node`, `NodeKind`, `SourceLocation`
- `TokenStream`, `Token`, `TokenKind`
- `incremental::IncrementalState` and `incremental::IncrementalMetrics`
- `PositionMapper` and `LineEnding`
- `Trivia`, `TriviaPreservingParser`, `format_with_trivia`

## Example

```rust
use perl_parser_core::Parser;

let mut parser = Parser::new("my $x = 42; sub hello { print $x; }");
let ast = parser.parse()?;
assert!(!ast.to_sexp().is_empty());
```

## Typical use

Use `perl-parser-core` when you are building parser-adjacent tooling or adding
new syntax behavior. If you want the higher-level analysis and refactoring
re-exports in one crate, use `perl-parser`.

The `incremental` module provides checkpoint-bounded token replay with explicit
fallback metrics. It reuses parser tokens outside the edit window and rebuilds the
AST from the assembled stream; it does not claim AST subtree reuse.

### Parenthesized loop headers

`for` and `foreach` share delimiter-based parsing: a complete parenthesized
expression is an implicit-`$_` `Foreach` list, including declarations such as
`for (my @filename = @_)` and expressions such as `for (my $x ** 2)`,
`for (my $x && $ready)` or `for (my $x ? 1 : 2)`. The declaration stays in `Foreach.list`; it is not
an explicit iterator declaration. Semicolon-separated headers remain C-style
`For` nodes, and missing separators still produce parser diagnostics. This is
a syntax/AST contract, not a claim of runtime loop execution support.
The shared expression parser's ternary-comma limitation (#16210) and implicit-list
continue attachment (#17091) remain open.
