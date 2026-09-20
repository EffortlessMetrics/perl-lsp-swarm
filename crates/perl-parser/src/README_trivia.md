# Canonical parsing with trivia retention

`TriviaPreservingParser` is one parser-backed surface over the canonical Perl parser:

```text
source
→ canonical Parser::parse_with_recovery()
→ ParseOutput (AST + diagnostics + recovery)
+ exact source
+ source-ordered trivia inventory
```

It does **not** maintain a second Perl grammar, synthesize placeholder AST nodes, or render `Debug` output as Perl source.

## Usage

```rust
use perl_parser::trivia::Trivia;
use perl_parser::trivia_parser::{TriviaPreservingParser, source_with_trivia};

let source = "# header\nmy $x = 42;\n".to_string();
let output = TriviaPreservingParser::new(source.clone()).parse();

println!("{}", output.parse.ast.to_sexp());
for token in &output.trivia {
    if let Trivia::LineComment(text) = &token.trivia {
        println!("comment: {text}");
    }
}

assert_eq!(source_with_trivia(&output), source);
```

## Current boundary

The current result contains:

- the complete canonical `ParseOutput`;
- exact original source;
- a source-ordered compatibility inventory of whitespace, comments, POD, and newline trivia.

It does **not yet** claim:

- complete per-node leading/trailing ownership;
- exact one-owner partitioning of every source byte;
- opaque-region classification for regexes, heredocs, formats, POD, or DATA;
- safe source transformation or style formatting.

Those contracts are owned by the follow-on source-geometry and formatter issues #7101, #7104, and #7056. The deprecated `NodeWithTrivia` AST-v2 container remains only for migration and is not produced by the canonical parser-backed surface.
