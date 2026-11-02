//! Unterminated quote-like operator after a term (#16608 class 2).
//!
//! The lexer already classifies these as `unclosed ...` error tokens. The
//! parser must name the unterminated operator (mirroring perl 5.42), anchored
//! at the operator start, instead of reporting a misleading missing closer
//! for the enclosing bracket.

use perl_parser_core::error::ParseError;
use perl_parser_core::{NodeKind, Parser, TokenKind, TokenStream};

fn collect_var_names(node: &perl_parser_core::Node, out: &mut Vec<String>) {
    if let NodeKind::VariableDeclaration { variable, .. } = &node.kind
        && let NodeKind::Variable { name, .. } = &variable.kind
    {
        out.push(name.clone());
    }
    for child in node.children() {
        collect_var_names(child, out);
    }
}

fn diagnostics(source: &str) -> Vec<ParseError> {
    Parser::new(source).parse_with_recovery().diagnostics
}

fn assert_terminator(source: &str, expected_message: &str, operator_start: usize) {
    let diagnostics = diagnostics(source);
    let matching: Vec<&ParseError> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.to_string().contains(expected_message))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one {expected_message:?} diagnostic for {source:?}, got {diagnostics:?}"
    );
    let diagnostic = matching[0];
    assert!(diagnostic.blocks_clean_parse(), "terminator diagnostic must block: {diagnostic:?}");
    assert_eq!(
        diagnostic.location(),
        Some(operator_start),
        "terminator diagnostic must anchor at the operator start for {source:?}"
    );
    // Total blocking set: beyond `Recovered` recovery notes, the terminator
    // diagnostic must be the ONLY diagnostic. Any extra blocking diagnostic
    // (an `UnexpectedToken` cascade such as expected-'}' or expected-statement
    // at EOF) fails here even when the terminator count itself is right.
    let blocking: Vec<&ParseError> = diagnostics
        .iter()
        .filter(|diagnostic| !matches!(diagnostic, ParseError::Recovered { .. }))
        .collect();
    assert_eq!(
        blocking.len(),
        1,
        "expected exactly one non-recovered diagnostic for {source:?}, got {diagnostics:?}"
    );
    assert_eq!(
        blocking[0].to_string(),
        diagnostic.to_string(),
        "the sole non-recovered diagnostic must be the terminator for {source:?}"
    );
    for diagnostic in &diagnostics {
        assert!(
            !matches!(diagnostic, ParseError::UnexpectedToken { .. }),
            "no UnexpectedToken cascade may be emitted for {source:?}: {diagnostic:?}"
        );
        let text = diagnostic.to_string();
        assert!(
            !text.contains("expected ']'")
                && !text.contains("expected ')'")
                && !text.contains("expected '}'"),
            "misleading expected-closer diagnostic must not be emitted for {source:?}: {text}"
        );
        assert!(
            !text.contains("unknown token"),
            "unknown-token diagnostic must not be emitted for {source:?}: {text}"
        );
    }
}

fn assert_no_blocking(source: &str) {
    let diagnostics = diagnostics(source);
    let blocking: Vec<&ParseError> =
        diagnostics.iter().filter(|diagnostic| diagnostic.blocks_clean_parse()).collect();
    assert!(
        blocking.is_empty(),
        "expected no blocking diagnostics for {source:?}, got {blocking:?}"
    );
}

#[test]
fn unterminated_m_after_term_in_array_ref() {
    let source = "my @x = @{['a'm]};";
    assert_terminator(source, "Search pattern not terminated", source.find("m]}").unwrap_or(0));
}

#[test]
fn unterminated_q_after_term_in_array_ref() {
    let source = "my @x = @{['a'q]};";
    assert_terminator(
        source,
        "Can't find string terminator \"]\" anywhere before EOF",
        source.find("q]}").unwrap_or(0),
    );
}

#[test]
fn unterminated_qr_after_term_in_array_ref() {
    let source = "my @x = @{['a'qr]};";
    assert_terminator(source, "Search pattern not terminated", source.find("qr").unwrap_or(0));
}

#[test]
fn unterminated_s_after_term_in_array_ref() {
    let source = "my @x = @{['a's]};";
    assert_terminator(
        source,
        "Substitution pattern not terminated",
        source.find("s]}").unwrap_or(0),
    );
}

#[test]
fn unterminated_tr_after_term_in_array_ref() {
    let source = "my @x = @{['a'tr]};";
    assert_terminator(
        source,
        "Transliteration pattern not terminated",
        source.find("tr]}").unwrap_or(0),
    );
}

#[test]
fn unterminated_m_in_term_position() {
    let source = "my $r = m]};";
    assert_terminator(source, "Search pattern not terminated", source.find("m]}").unwrap_or(0));
}

#[test]
fn unterminated_qr_in_term_position() {
    let source = "my $r = qr]};";
    assert_terminator(source, "Search pattern not terminated", source.find("qr").unwrap_or(0));
}

#[test]
fn unterminated_qx_in_term_position() {
    let source = "my $r = qx]};";
    assert_terminator(
        source,
        "Can't find string terminator \"]\" anywhere before EOF",
        source.find("qx").unwrap_or(0),
    );
}

#[test]
fn unterminated_m_after_term_in_paren_list() {
    let source = "my @x = ('a', m]});";
    assert_terminator(source, "Search pattern not terminated", source.find("m]}").unwrap_or(0));
}

#[test]
fn unterminated_m_after_term_with_whitespace() {
    let source = "my @x = ['a' m]};";
    assert_terminator(source, "Search pattern not terminated", source.find("m]}").unwrap_or(0));
}

#[test]
fn unclosed_m_token_maps_to_regex_and_qx_to_quote_command() -> Result<(), Box<dyn std::error::Error>>
{
    let mut stream = TokenStream::new("my $r = m]};");
    let mut kinds = Vec::new();
    loop {
        let token = stream.next()?;
        if token.kind() == TokenKind::Eof {
            break;
        }
        kinds.push(token.kind());
    }
    assert!(kinds.contains(&TokenKind::Regex), "m token must map to Regex, got {kinds:?}");
    assert!(!kinds.contains(&TokenKind::Unknown), "no Unknown token expected, got {kinds:?}");

    let mut stream = TokenStream::new("my $r = qx]};");
    let mut kinds = Vec::new();
    loop {
        let token = stream.next()?;
        if token.kind() == TokenKind::Eof {
            break;
        }
        kinds.push(token.kind());
    }
    assert!(
        kinds.contains(&TokenKind::QuoteCommand),
        "qx token must map to QuoteCommand, got {kinds:?}"
    );
    Ok(())
}

#[test]
fn closed_m_after_equals_sign_stays_clean() {
    assert_no_blocking("my $s = 'a'; $s =~ m/foo/;");
}

#[test]
fn closed_regex_as_list_element_stays_clean() {
    assert_no_blocking("my @x = ['a', m/foo/];");
}

#[test]
fn method_call_named_m_stays_clean() {
    assert_no_blocking("'a'->m;");
    assert_no_blocking("'a'->m();");
    assert_no_blocking("'a'->m(1);");
}

#[test]
fn hash_keys_named_m_and_q_stay_clean() {
    assert_no_blocking("my $z = $h{m};");
    assert_no_blocking("my $z = $h{q};");
}

#[test]
fn fat_arrow_keys_stay_clean() {
    assert_no_blocking("my %h = (m => 1, q => 2);");
}

#[test]
fn closed_qw_stays_clean() {
    assert_no_blocking("my @w = qw/a b/;");
}

#[test]
fn arrow_hash_key_named_m_stays_clean() {
    assert_no_blocking("my $z = $h->{m};");
}

#[test]
fn bounded_qw_after_term_recovers_to_real_bracket() {
    // `qw(` stops at the newline, so the array's real `]` is still ahead.
    // Recovery must reach that `]` instead of closing the array early and
    // stranding the `]` as a trailing expected-statement cascade.
    let source = "my @x = ['a' qw(foo bar\nmy $y = 1;];";
    assert_terminator(
        source,
        "Can't find string terminator \")\" anywhere before EOF",
        source.find("qw").unwrap_or(0),
    );
}

#[test]
fn bounded_qw_after_term_preserves_later_declaration() {
    // The `qw(` token stops before `my`; the later declaration is a real
    // statement, not qw content, and must survive in the AST.
    let source = "my @x = ['a' qw(foo bar\nmy $y = 1;];";
    assert_terminator(
        source,
        "Can't find string terminator \")\" anywhere before EOF",
        source.find("qw").unwrap_or(0),
    );
    let output = Parser::new(source).parse_with_recovery();
    let mut vars = Vec::new();
    collect_var_names(&output.ast, &mut vars);
    assert!(vars.contains(&"y".to_string()), "$y must survive recovery, got vars {vars:?}");
    assert!(vars.contains(&"x".to_string()), "$x/@x must survive recovery, got vars {vars:?}");
}

#[test]
fn missing_s_replacement_after_term_names_replacement() {
    let source = "my @x = ['a' s{foo}];";
    assert_terminator(
        source,
        "Substitution replacement not terminated",
        source.find("s{foo}").unwrap_or(0),
    );
}

#[test]
fn missing_tr_replacement_after_term_names_replacement() {
    let source = "my @x = ['a' tr{foo}];";
    assert_terminator(
        source,
        "Transliteration replacement not terminated",
        source.find("tr{foo}").unwrap_or(0),
    );
}

#[test]
fn missing_s_replacement_at_eof_names_replacement() {
    let source = "my @x = ['a' s{foo}";
    assert_terminator(
        source,
        "Substitution replacement not terminated",
        source.find("s{foo}").unwrap_or(0),
    );
}

#[test]
fn missing_tr_replacement_at_eof_names_replacement() {
    let source = "my @x = ['a' tr{foo}";
    assert_terminator(
        source,
        "Transliteration replacement not terminated",
        source.find("tr{foo}").unwrap_or(0),
    );
}

#[test]
fn unclosed_s_replacement_names_replacement_not_pattern() {
    let source = "my $x = s{foo}{bar;";
    assert_terminator(
        source,
        "Substitution replacement not terminated",
        source.find("s{foo}").unwrap_or(0),
    );
}

#[test]
fn unclosed_tr_replacement_names_replacement_not_pattern() {
    let source = "my $x = tr{foo}{bar;";
    assert_terminator(
        source,
        "Transliteration replacement not terminated",
        source.find("tr{foo}").unwrap_or(0),
    );
}

#[test]
fn unclosed_s_replacement_after_term_names_replacement() {
    let source = "my @x = ['a' s{foo}{bar];";
    assert_terminator(
        source,
        "Substitution replacement not terminated",
        source.find("s{foo}").unwrap_or(0),
    );
}

#[test]
fn closed_s_and_tr_after_term_stay_clean() {
    assert_no_blocking("my @x = ['a', s{foo}{bar}];");
    assert_no_blocking("my @x = ['a', tr{a}{b}];");
    assert_no_blocking("my $x = s/foo/bar/;");
    assert_no_blocking("my $x = tr/a/b/;");
}

#[test]
fn bounded_qw_after_term_bare_array_recovers_to_real_bracket() {
    let source = "['a' qw(foo bar\nmy $y = 1;];";
    assert_terminator(
        source,
        "Can't find string terminator \")\" anywhere before EOF",
        source.find("qw").unwrap_or(0),
    );
}
