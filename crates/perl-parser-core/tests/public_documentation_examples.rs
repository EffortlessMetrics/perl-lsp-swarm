#![allow(clippy::print_stderr)] // Intentional stderr diagnostics for documented example failures.
#![allow(clippy::print_stdout)] // // Intentional stdout/stderr diagnostics for documented example outcomes.
use perl_parser_core::syntax::error::ParseResult;

fn report(result: ParseResult<()>) {
    match result {
        Ok(()) => println!("parse completed"),
        Err(error) => eprintln!("parse failed: {error}"),
    }
}

#[test]
fn parse_result_documentation_example_compiles_against_public_error_api() {
    report(Ok(()));
}
