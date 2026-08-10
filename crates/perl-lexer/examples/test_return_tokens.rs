//! Prints the token stream for a few `return`-statement shapes.
//!
//! This is a developer-facing demo binary whose entire output *is* stdout, so
//! the workspace-wide `print_stdout = "deny"` lint is opted out of here rather
//! than worked around.
#![allow(clippy::print_stdout)]

use perl_lexer::{PerlLexer, TokenType};

fn main() {
    let test_cases =
        vec!["return if 1;", "return;", "return $x if $cond;", "return $x or die if $error;"];

    for input in test_cases {
        println!("\nTokenizing: {}", input);
        let mut lexer = PerlLexer::new(input);

        loop {
            match lexer.next_token() {
                Some(token) => {
                    println!("  {:?} => '{}'", token.token_type, &input[token.start..token.end]);
                    if matches!(token.token_type, TokenType::EOF) {
                        break;
                    }
                }
                None => {
                    println!("  End of tokens");
                    break;
                }
            }
        }
    }
}
