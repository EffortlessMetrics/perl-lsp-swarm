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
