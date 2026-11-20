//! Regression tests for CPAN source-filter module detection (#2387).

use perl_parser_core::Parser;
use perl_tdd_support::must;

#[test]
fn known_cpan_source_filters_set_has_filter_risk() {
    for module in [
        "Filter",
        "Filter::Util::Call",
        "Filter::Simple",
        "Filter::cpp",
        "Filter::exec",
        "Filter::sh",
        "Filter::tee",
        "Filter::decrypt",
        "Switch",
        "Acme::Bleach",
        "Smart::Comments",
        "Inline::Files",
        "Perl6::Say",
        "Spiffy",
        "B::Hooks::OP::Check",
    ] {
        let code = format!("use {module};");
        let mut parser = Parser::new(&code);
        let result = parser.parse();
        assert!(result.is_ok(), "parse failed for `use {module};`: {result:?}");
        let ast = must(result);
        let sexp = ast.to_sexp();
        assert!(
            sexp.contains("(has_filter_risk true)"),
            "`use {module};` should set has_filter_risk in: {sexp}"
        );
    }
}
