//! The package's public usage example, compiled against the actual API.
//!
//! `[lib] doctest = false` means the README snippet is never compiled, so this
//! example is the packaged proof that the documented entry point still exists
//! and still type-checks (`#8771`). It is built by
//! `cargo check -p perl-parser-pest --all-targets`.
//!
//! ```console
//! cargo run -p perl-parser-pest --example parse_basic
//! ```

// Narrow, deliberate disposition: a usage example demonstrates output, and the
// package lint policy denies `print_stdout` for library and service code.
#![allow(clippy::print_stdout)]

use std::error::Error;

use perl_parser_pest::PureRustPerlParser;

fn main() -> Result<(), Box<dyn Error>> {
    let mut parser = PureRustPerlParser::new();
    let ast = parser.parse("my $x = 42;")?;
    let sexp = parser.to_sexp(&ast);

    println!("{sexp}");
    Ok(())
}
