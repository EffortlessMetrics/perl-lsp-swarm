//! #16610: `--check` and LSP must agree on hints for unbalanced delimiter / regex cases.

use perl_parser_core::Parser;
use perl_parser_core::error::get_error_contexts;
use perl_parser_core::syntax::error::ParseError;
use perl_tdd_support::must;

// Matches issue #16610 evidence (`13a_regex_unbalanced.pl` line 5).
const REGEX_FIXTURE: &str = "use strict;\nuse warnings;\nmy $s = \"abc\";\nmy $m = $s =~ m/abc;\n";

fn parse_errors(source: &str) -> Vec<ParseError> {
    let mut parser = Parser::new(source);
    must(parser.parse());
    parser.errors().to_vec()
}

#[test]
fn unbalanced_regex_surfaces_unknown_token_parse_error() {
    let errors = parse_errors(REGEX_FIXTURE);
    assert!(
        errors.iter().any(|error| matches!(
            error,
            ParseError::UnexpectedToken { found, .. } if found == "unknown token"
        )),
        "expected an unknown-token diagnostic for unterminated m//, got: {errors:?}"
    );
}

#[test]
fn check_context_and_lsp_share_unknown_token_hint() {
    let errors = parse_errors(REGEX_FIXTURE);
    let unknown_token_errors: Vec<_> = errors
        .iter()
        .filter(|error| {
            matches!(
                error,
                ParseError::UnexpectedToken { found, .. } if found == "unknown token"
            )
        })
        .collect();
    assert!(
        !unknown_token_errors.is_empty(),
        "fixture must include at least one unknown-token error"
    );

    for error in unknown_token_errors {
        let canonical_hint = perl_parser_core::syntax::error::build_parse_error_hint_with_source(
            error,
            "",
            Some(REGEX_FIXTURE),
        )
        .unwrap_or_else(|| panic!("hint table must cover unknown-token errors: {error:?}"));
        assert!(
            canonical_hint.contains("Search pattern not terminated"),
            "unexpected hint: {canonical_hint}"
        );

        let contexts = get_error_contexts(std::slice::from_ref(error), REGEX_FIXTURE);
        let check_hint = contexts[0]
            .suggestion
            .as_deref()
            .unwrap_or_else(|| panic!("--check must surface the same hint via error context"));
        assert_eq!(
            check_hint, canonical_hint,
            "--check and shared hint table must agree for unknown-token errors"
        );

        let lsp_hint = perl_parser_core::syntax::error::build_parse_error_hint_with_source(
            error,
            "",
            Some(REGEX_FIXTURE),
        )
        .expect("LSP uses the same source-aware table");
        assert_eq!(lsp_hint, canonical_hint);
    }
}

#[test]
fn unbalanced_regex_hint_names_search_pattern_termination() {
    let errors = parse_errors(REGEX_FIXTURE);
    let error = errors
        .iter()
        .find(|error| {
            matches!(
                error,
                ParseError::UnexpectedToken { found, .. } if found == "unknown token"
            )
        })
        .expect("unknown-token error");
    let hint = perl_parser_core::syntax::error::build_parse_error_hint_with_source(
        error,
        "",
        Some(REGEX_FIXTURE),
    )
    .expect("hint for unbalanced regex");
    assert!(
        hint.contains("Search pattern not terminated"),
        "hint should name the unterminated regex delimiter like perl -c: {hint}"
    );
}
