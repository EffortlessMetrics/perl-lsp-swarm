//! Issue #16606: a literal value where a group's closing delimiter is
//! expected must suggest the missing comma/operator, not a closing
//! parenthesis that usually already exists further along.
//!
//! `09b_missing_comma_list.pl` line 4, `my @nums = (1 2 3);`, previously
//! produced `help: add a closing parenthesis ')' to end the group` while the
//! `)` sits three columns later on the same line — advice that cannot be
//! followed. Real `perl` names the actual problem: `Missing operator before
//! "2"?`.

use perl_parser_core::Parser;
use perl_parser_core::error::{ParseError, found_is_value_like, get_error_contexts};

const MISSING_COMMA_LIST: &str = "my @nums = (1 2 3);\n";

#[test]
fn missing_comma_error_names_the_comma_not_the_closer() -> Result<(), String> {
    let mut parser = Parser::new(MISSING_COMMA_LIST);
    let _ast = parser.parse().map_err(|e| format!("recovery failed: {e:?}"))?;
    let errors = parser.errors();

    let paren_error = errors
        .iter()
        .find(|e| e.to_string().starts_with("expected ')'"))
        .ok_or_else(|| format!("expected the `expected ')'` list error: {errors:?}"))?;

    let suggestion = paren_error
        .suggestion()
        .ok_or_else(|| "missing-comma error must carry a hint".to_string())?;
    assert!(suggestion.contains("','"), "help must name the missing comma, got: {suggestion}");
    assert!(
        !suggestion.contains("closing parenthesis"),
        "help must not ask for a closer that already exists, got: {suggestion}"
    );
    Ok(())
}

#[test]
fn missing_comma_error_is_recorded_once_with_context() -> Result<(), String> {
    let mut parser = Parser::new(MISSING_COMMA_LIST);
    let _ast = parser.parse().map_err(|e| format!("recovery failed: {e:?}"))?;

    let errors = parser.errors();
    assert_eq!(
        errors.iter().filter(|e| e.blocks_clean_parse()).count(),
        1,
        "one failed statement, one error (see #16605): {errors:?}"
    );

    let contexts = get_error_contexts(errors, MISSING_COMMA_LIST);
    assert_eq!(contexts.len(), 1);
    assert_eq!(contexts[0].line, 0, "anchors on line 1");
    let suggestion = contexts[0].suggestion.as_deref().unwrap_or("");
    assert!(
        suggestion.contains("','") && !suggestion.contains("closing parenthesis"),
        "rendered help names the comma, got: {suggestion}"
    );
    Ok(())
}

#[test]
fn non_literal_found_keeps_the_closer_advice() {
    // `my @nums = (1, 2;` — the parser expects `)` but finds `;`: here the
    // group really is missing its closer, so the original advice stands.
    let error = ParseError::unexpected("')'", "';'", 17);
    assert_eq!(
        error.suggestion().as_deref(),
        Some("add a closing parenthesis ')' to end the group"),
        "a non-literal `found` must keep the closer suggestion"
    );
}

#[test]
fn semicolon_rule_still_wins_for_statement_errors() {
    // The comma rule is scoped to the `)` expectation; the earlier `;` rule
    // must keep firing for statement-level errors with a literal `found`.
    let error = ParseError::unexpected("';'", "number", 10);
    assert_eq!(
        error.suggestion().as_deref(),
        Some("add a semicolon ';' at the end of the statement"),
    );
}

#[test]
fn bracket_rule_is_out_of_scope_for_the_comma_rule() {
    // `]` groups keep the closer suggestion even for a literal `found` —
    // the rule was scoped to `)` lists only (#16606).
    let error = ParseError::unexpected("']'", "number", 10);
    assert_eq!(error.suggestion().as_deref(), Some("add a closing bracket ']' to end the array"),);
}

#[test]
fn value_like_discriminator_table() {
    // Token display names that can appear as a list item.
    for found in [
        "number",
        "string",
        "identifier",
        "q// string",
        "qq// string",
        "qw() word list",
        "qx// command",
        "version string",
    ] {
        assert!(found_is_value_like(found), "{found} must be value-like");
    }

    // Delimiters, operators, keywords, and stream shapes are not list items.
    for found in
        ["';'", "')'", "'}'", "'my'", "'='", "end of input", "unknown token", "heredoc body", ""]
    {
        assert!(!found_is_value_like(found), "{found} must not be value-like");
    }
}
