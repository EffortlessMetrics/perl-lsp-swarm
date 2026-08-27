//! Integration fixtures for the embedded-code regex security lints (#9818).
//!
//! Perl evaluates `s/pat/repl/e` replacements as Perl code and runs `(?{ ... })`
//! blocks whenever a pattern matches, but `walk_security_node` filed
//! `Regex`/`Match`/`Substitution` as inert terminals and published nothing.
//! These are the red-first public-API fixtures per construct class.

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

fn has_security_family(codes: &[String]) -> bool {
    codes.iter().any(|code| code.starts_with("PL6"))
}

// --- s///e construct class (replacement evaluated as code): PL608 ---

#[test]
fn e_modifier_substitution_is_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let got = codes(r#"$s =~ s/(\w+)/uc($1)/e;"#);
    assert!(
        has_code(&got, "PL608"),
        "s///e should publish the stable substitution-eval code PL608: {got:?}"
    );
    Ok(())
}

#[test]
fn ee_modifier_substitution_is_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let got = codes(r#"$t =~ s/\$(\w+)/$$1/ee;"#);
    assert!(
        has_code(&got, "PL608"),
        "s///ee should publish the stable substitution-eval code PL608: {got:?}"
    );
    Ok(())
}

#[test]
fn standalone_e_modifier_substitution_is_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let got = codes(r#"s/version (\d+)/$1 + 1/e;"#);
    assert!(
        has_code(&got, "PL608"),
        "bare s///e should publish PL608 even without a =~ binding: {got:?}"
    );
    Ok(())
}

// --- (?{ ... }) construct class (executable pattern code): PL609 ---

#[test]
fn embedded_code_block_in_qr_is_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let got = codes(r#"my $r = qr/(?{ print "hi" })/;"#);
    assert!(
        has_code(&got, "PL609"),
        "qr/(?{{...}})/ should publish the stable embedded-code class PL609: {got:?}"
    );
    Ok(())
}

#[test]
fn deferred_embedded_code_block_in_qr_is_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let got = codes(r#"my $r = qr/(??{ build_pattern() })/;"#);
    assert!(
        has_code(&got, "PL609"),
        "qr/(??{{...}})/ should publish PL609 for deferred embedded code: {got:?}"
    );
    Ok(())
}

#[test]
fn embedded_code_block_in_explicit_match_is_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let got = codes(r#"$x =~ m/(?{ print "hi" })/;"#);
    assert!(
        has_code(&got, "PL609"),
        "m/(?{{...}})/ should publish the stable embedded-code class PL609: {got:?}"
    );
    Ok(())
}

#[test]
fn embedded_code_block_in_bare_match_is_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let got = codes(r#"$x =~ /(?{ print "hi" })/;"#);
    assert!(
        has_code(&got, "PL609"),
        "bare /(?{{...}})/ should publish the same embedded-code class PL609: {got:?}"
    );
    Ok(())
}

#[test]
fn embedded_code_block_in_substitution_pattern_is_flagged() -> Result<(), Box<dyn std::error::Error>>
{
    let got = codes(r#"$x =~ s/(?{ print "hi" })/ok/;"#);
    assert!(
        has_code(&got, "PL609"),
        "(?{{...}}) inside a substitution pattern should publish PL609: {got:?}"
    );
    Ok(())
}

// --- negative controls: plain constructs stay silent ---

#[test]
fn plain_substitution_is_not_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let got = codes(r#"$s =~ s/a/b/;"#);
    assert!(
        !has_security_family(&got),
        "plain s/// must not publish a security diagnostic: {got:?}"
    );
    Ok(())
}

#[test]
fn plain_match_is_not_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let got = codes(r#"$s =~ m/hello/;"#);
    assert!(
        !has_security_family(&got),
        "plain m// must not publish a security diagnostic: {got:?}"
    );
    Ok(())
}

#[test]
fn qr_without_embedded_code_is_not_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let got = codes(r#"my $re = qr/hello/;"#);
    assert!(
        !has_security_family(&got),
        "plain qr// must not publish a security diagnostic: {got:?}"
    );
    Ok(())
}
