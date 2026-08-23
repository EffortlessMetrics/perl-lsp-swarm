//! Demonstration of canonical parsing with exact source and trivia retention.
//!
//! The AST and recovery output come from the normal `perl_parser::Parser` path.
//! Trivia is currently exposed as a source-ordered inventory; #7101 owns the
//! later per-node source-geometry contract.

use perl_parser::trivia::Trivia;
use perl_parser::trivia_parser::{TriviaPreservingParser, source_with_trivia};

fn main() {
    let source = r#"#!/usr/bin/perl
# File header

use strict;
use warnings;

=head1 NAME

Example - canonical trivia demo

=cut

my $value = 42; # trailing comment
"#;

    let output = TriviaPreservingParser::new(source.to_string()).parse();

    println!("=== Canonical AST ===");
    println!("{}", output.parse.ast.to_sexp());
    println!("diagnostics: {}", output.parse.diagnostics.len());
    println!("recoveries: {}", output.parse.recovered_count);

    println!("\n=== Source-ordered trivia ===");
    for (index, token) in output.trivia.iter().enumerate() {
        let summary = match &token.trivia {
            Trivia::Whitespace(text) => format!("{:?}", text),
            Trivia::LineComment(text) => text.clone(),
            Trivia::PodComment(text) => {
                format!("{}...", text.lines().next().unwrap_or_default())
            }
            Trivia::Newline => "\\n".to_string(),
        };
        println!("{}. {} — {}", index + 1, token.trivia.kind_name(), summary);
    }

    println!("\n=== Exact source projection ===");
    println!("{}", source_with_trivia(&output));

    assert_eq!(source_with_trivia(&output), source);
}
