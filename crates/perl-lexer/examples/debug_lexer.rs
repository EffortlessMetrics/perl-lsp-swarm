//! Developer-facing demo binary whose entire output *is* stdout, so the
//! workspace-wide `print_stdout = "deny"` lint is opted out of here rather
//! than worked around.
#![allow(clippy::print_stdout)]

//! Debug lexer output
use perl_lexer::PerlLexer;

fn main() {
    let code = "package Test::Module;";
    println!("Code: {}", code);
    println!("\nLexer tokens:");

    let mut lexer = PerlLexer::new(code);
    while let Some(token) = lexer.next_token() {
        println!("  Token: {:?} '{}'", token.token_type, token.text);
    }
}
