//! Integration fixtures for `=~` bound-expression traversal (#9821).
//!
//! `NodeKind::Match` and `NodeKind::Substitution` carry the expression bound
//! via `=~`, but the security walker filed both variants as inert terminals,
//! so any security-relevant child escaped detection purely through placement:
//! `` `ls` =~ /x/ `` lost the PL601 its assignment-position twin publishes.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::DiagnosticsProvider;
use perl_parser::Parser;

fn codes(source: &str) -> Vec<String> {
    let output = Parser::new(source).parse_with_recovery();
    let ast = Arc::new(output.ast);
    DiagnosticsProvider::new()
        .get_diagnostics(&ast, &output.diagnostics, source, None)
        .into_iter()
        .filter_map(|diag| diag.code)
        .collect()
}

fn has_code(codes: &[String], expected: &str) -> bool {
    codes.iter().any(|code| code == expected)
}

#[test]
fn backtick_bound_to_match_is_still_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let got = codes("`ls` =~ /x/;");
    assert!(
        has_code(&got, "PL601"),
        "backtick under Match.expr must publish the same PL601 as elsewhere: {got:?}"
    );
    Ok(())
}

#[test]
fn backtick_bound_to_substitution_is_still_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let got = codes("`ls` =~ s/a/b/;");
    assert!(
        has_code(&got, "PL601"),
        "backtick under Substitution.expr must publish the same PL601: {got:?}"
    );
    Ok(())
}

#[test]
fn readpipe_bound_to_match_is_still_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let got = codes(r#"readpipe("ls") =~ /x/;"#);
    assert!(
        has_code(&got, "PL606"),
        "readpipe() under Match.expr must keep its own stable code PL606: {got:?}"
    );
    Ok(())
}

// --- controls ---

#[test]
fn backtick_in_assignment_control_is_flagged() -> Result<(), Box<dyn std::error::Error>> {
    // The detection exists in ordinary positions; only placement must differ.
    let got = codes("my $x = `ls`;");
    assert!(
        has_code(&got, "PL601"),
        "control: backtick in assignment should publish PL601: {got:?}"
    );
    Ok(())
}

#[test]
fn variable_bound_to_match_is_not_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let got = codes("$s =~ /x/;");
    assert!(
        !got.iter().any(|code| code.starts_with("PL6")),
        "ordinary variable binding under =~ must stay silent: {got:?}"
    );
    Ok(())
}
