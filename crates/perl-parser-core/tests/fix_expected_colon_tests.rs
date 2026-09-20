#![allow(clippy::print_stderr)] // //! Intentional stderr diagnostics: this suite prints sexp context on failure.
mod cpan_test_helpers;
use cpan_test_helpers::*;

#[test]
fn test_recovery_check() {
    let ast = parse("my $x = { { { my $y = ; } } }");
    let sexp = ast.to_sexp();
    eprintln!("sexp: {}", sexp);
    // This should have errors
}
