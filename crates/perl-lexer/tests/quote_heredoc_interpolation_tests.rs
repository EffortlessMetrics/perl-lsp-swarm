//! #8779 — the `parse_interpolation` setting governs every supported
//! interpolating quote-like and heredoc form with one explicit per-surface
//! disposition, while non-interpolating forms stay invariant controls and
//! `qx`/backtick bodies stay an intentional opaque boundary.

use perl_lexer::{Checkpointable, LexerConfig, PerlLexer, StringPart, Token, TokenType};

fn config(interpolation: bool) -> LexerConfig {
    LexerConfig { parse_interpolation: interpolation, ..LexerConfig::default() }
}

fn collect(lexer: &mut PerlLexer) -> Vec<Token> {
    let mut out = Vec::new();
    while let Some(token) = lexer.next_token() {
        if matches!(token.token_type, TokenType::EOF) {
            break;
        }
        out.push(token);
    }
    out
}

fn tokens_with(input: &str, interpolation: bool) -> Vec<Token> {
    collect(&mut PerlLexer::with_config(input, config(interpolation)))
}

fn heredoc_tokens_with(input: &str, interpolation: bool) -> Vec<Token> {
    collect(&mut PerlLexer::with_config_and_body_tokens(input, config(interpolation)))
}

fn quote_double_parts(input: &str, interpolation: bool) -> Vec<StringPart> {
    tokens_with(input, interpolation)
        .iter()
        .find_map(|token| match &token.token_type {
            TokenType::QuoteDouble(parts) => Some(parts.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no QuoteDouble token for {input:?}"))
}

fn interpolated_string_parts(input: &str) -> Vec<StringPart> {
    tokens_with(input, true)
        .iter()
        .find_map(|token| match &token.token_type {
            TokenType::InterpolatedString(parts) => Some(parts.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no InterpolatedString token for {input:?}"))
}

// --- Surface dispositions -------------------------------------------------

#[test]
fn qq_enabled_segments_the_body_during_the_scan() {
    let parts = quote_double_parts("qq($name and @list!)", true);
    assert_eq!(
        parts,
        vec![
            StringPart::Variable("$name".into()),
            StringPart::Literal(" and ".into()),
            StringPart::Variable("@list".into()),
            StringPart::Literal("!".into()),
        ]
    );
}

#[test]
fn qq_disabled_is_uniformly_opaque() {
    for input in ["qq($name and @list!)", "qq/$name/", "qq {$x}"] {
        let parts = quote_double_parts(input, false);
        assert_eq!(
            parts,
            vec![StringPart::Literal(expected_opaque_body(input).into())],
            "disabled qq must keep one opaque Literal part for {input:?}"
        );
    }
}

fn expected_opaque_body(input: &str) -> &'static str {
    match input {
        "qq($name and @list!)" => "$name and @list!",
        "qq/$name/" => "$name",
        "qq {$x}" => "$x",
        other => unreachable!("unmapped body {other:?}"),
    }
}

#[test]
fn qq_delimiter_variants_all_segment_when_enabled() {
    for input in ["qq($v)", "qq{$v}", "qq[$v]", "qq<$v>", "qq/$v/", "qq!$v!", "qq,$v,", "qq|$v|"] {
        let parts = quote_double_parts(input, true);
        assert_eq!(
            parts,
            vec![StringPart::Variable("$v".into())],
            "delimiter variant {input:?} must segment the island"
        );
    }
}

#[test]
fn q_stays_an_invariant_control_under_both_settings() {
    for interpolation in [true, false] {
        let toks = tokens_with("q($name)", interpolation);
        let first = &toks[0];
        assert!(
            matches!(first.token_type, TokenType::QuoteSingle),
            "q must stay QuoteSingle: {:?}",
            first.token_type
        );
    }
}

#[test]
fn qw_stays_an_invariant_control_under_both_settings() {
    for interpolation in [true, false] {
        let toks = tokens_with("qw($a $b)", interpolation);
        assert!(matches!(toks[0].token_type, TokenType::QuoteWords), "qw must stay QuoteWords");
    }
}

#[test]
fn qx_and_backticks_are_the_intentional_opaque_boundary() {
    for interpolation in [true, false] {
        for input in ["qx/$name/", "`$name`"] {
            let toks = tokens_with(input, interpolation);
            assert!(
                matches!(toks[0].token_type, TokenType::QuoteCommand),
                "{input:?} must stay QuoteCommand"
            );
        }
    }
}

// --- One policy: the qq mirror matches the ordinary-string scanner --------

#[test]
fn qq_parts_match_the_ordinary_string_scanner_over_the_island_matrix() {
    let corpus = [
        "$name",
        "@list",
        "${expr}",
        "@{[1, 2]}",
        "$$ref",
        "@$ref",
        "$#array",
        "$1",
        "$^W",
        "$!",
        "$main::x",
        "@main::list",
        "$h->{k}",
        "$a->[0]",
        "$foo->bar",
        r"\$literal",
        "$x$y",
        "plain text",
        "a $b c",
    ];
    for body in corpus {
        let ordinary = interpolated_string_parts(&format!("\"{body}\""));
        let qq = quote_double_parts(&format!("qq({body})"), true);
        assert_eq!(
            ordinary, qq,
            "island policy diverged between \"...\" and qq(...) for body {body:?}"
        );
    }
}

#[test]
fn qq_body_geometry_and_identity_are_stable_across_settings() {
    let input = "qq(before $v after)";
    let enabled = tokens_with(input, true);
    let disabled = tokens_with(input, false);
    let (a, b) = (&enabled[0], &disabled[0]);
    assert_eq!(a.start, b.start);
    assert_eq!(a.end, b.end);
    assert_eq!(a.text, b.text);
    assert!(matches!(a.token_type, TokenType::QuoteDouble(_)));
    assert!(matches!(b.token_type, TokenType::QuoteDouble(_)));
}

#[test]
fn unclosed_qq_still_recovers_into_the_error_token() {
    for interpolation in [true, false] {
        let toks = tokens_with("qq(before $v", interpolation);
        assert!(
            toks.iter().any(|t| matches!(t.token_type, TokenType::Error(_))),
            "unclosed qq must recover through the error token in both settings"
        );
    }
}

#[test]
fn paired_qq_delimiters_are_not_consumed_as_interpolation_tail_boundaries() {
    assert_eq!(
        quote_double_parts("qq{${foo}}", true),
        vec![StringPart::Expression("${foo}".into())],
    );
    assert_eq!(
        quote_double_parts("qq[$a[0]]", true),
        vec![StringPart::Variable("$a".into()), StringPart::ArraySlice("[0]".into())],
    );
    assert_eq!(
        quote_double_parts("qq($obj->())", true),
        vec![StringPart::Variable("$obj".into()), StringPart::MethodCall("->()".into())],
    );
}

// --- Heredoc dispositions --------------------------------------------------

fn heredoc_source(body: &str) -> String {
    format!("my $text = <<\"END\";\n{body}\nEND\n")
}

fn single_quoted_heredoc_source(body: &str) -> String {
    format!("my $text = <<'END';\n{body}\nEND\n")
}

fn heredoc_body_kind(input: &str, interpolation: bool) -> TokenType {
    heredoc_tokens_with(input, interpolation)
        .into_iter()
        .find_map(|token| match token.token_type {
            TokenType::HeredocBody(_) | TokenType::InterpolatedHeredocBody(_) => {
                Some(token.token_type)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no heredoc body token for {input:?}"))
}

#[test]
fn interpolating_heredoc_enabled_segments_the_body() {
    let source = heredoc_source("value $name!");
    let kind = heredoc_body_kind(&source, true);
    match kind {
        TokenType::InterpolatedHeredocBody(parts) => assert_eq!(
            parts,
            vec![
                StringPart::Literal("value ".into()),
                StringPart::Variable("$name".into()),
                StringPart::Literal("!\n".into()),
            ]
        ),
        other => panic!("expected interpolated heredoc body, got {other:?}"),
    }
}

#[test]
fn interpolating_heredoc_disabled_is_uniformly_opaque() {
    let source = heredoc_source("value $name!");
    match heredoc_body_kind(&source, false) {
        TokenType::InterpolatedHeredocBody(parts) => assert_eq!(
            parts,
            vec![StringPart::Literal("value $name!\n".into())],
            "disabled interpolating heredocs keep one opaque Literal part"
        ),
        other => panic!("expected interpolated heredoc body, got {other:?}"),
    }
}

#[test]
fn disabled_interpolating_heredocs_keep_one_complete_literal_for_empty_and_escaped_bodies() {
    for (body, expected) in [("", "\n"), (r"a\$name", "a\\$name\n")] {
        let source = heredoc_source(body);
        match heredoc_body_kind(&source, false) {
            TokenType::InterpolatedHeredocBody(parts) => {
                assert_eq!(parts, vec![StringPart::Literal(expected.into())], "{body:?}");
            }
            other => panic!("expected interpolated heredoc body, got {other:?}"),
        }
    }
}

#[test]
fn interpolating_heredoc_body_matches_the_ordinary_scanner_per_line() {
    let body = "head $v tail";
    let source = heredoc_source(body);
    let ordinary = interpolated_string_parts(&format!("\"{body}\n\""));
    match heredoc_body_kind(&source, true) {
        TokenType::InterpolatedHeredocBody(parts) => {
            assert_eq!(parts, ordinary, "island policy must be one policy");
        }
        other => panic!("expected interpolated heredoc body, got {other:?}"),
    }
}

#[test]
fn interpolating_heredoc_segments_across_multiple_body_lines() {
    let source = heredoc_source("line1 $v\nline2");
    match heredoc_body_kind(&source, true) {
        TokenType::InterpolatedHeredocBody(parts) => assert_eq!(
            parts,
            vec![
                StringPart::Literal("line1 ".into()),
                StringPart::Variable("$v".into()),
                StringPart::Literal("\nline2\n".into()),
            ]
        ),
        other => panic!("expected interpolated heredoc body, got {other:?}"),
    }
}

#[test]
fn interpolating_heredoc_allows_multiline_interpolation_islands() {
    let source = heredoc_source("before ${\n  $name\n} after");
    match heredoc_body_kind(&source, true) {
        TokenType::InterpolatedHeredocBody(parts) => assert_eq!(
            parts,
            vec![
                StringPart::Literal("before ".into()),
                StringPart::Expression("${\n  $name\n}".into()),
                StringPart::Literal(" after\n".into()),
            ]
        ),
        other => panic!("expected interpolated heredoc body, got {other:?}"),
    }
}

#[test]
fn unclosed_heredoc_interpolation_stays_within_body_boundary() {
    let source = "my $text = <<\"END\";\nbefore ${\n$name\nEND\nprint $text;\n";
    let tokens = heredoc_tokens_with(source, true);
    let body = tokens
        .iter()
        .find_map(|token| match &token.token_type {
            TokenType::InterpolatedHeredocBody(parts) => Some(parts),
            _ => None,
        })
        .expect("interpolated heredoc body");
    assert_eq!(
        body,
        &vec![StringPart::Literal("before ".into()), StringPart::Expression("${\n$name\n".into()),]
    );
    assert!(body.iter().all(|part| !format!("{part:?}").contains("END")));
    assert!(tokens.iter().any(|token| token.text == "print".into()));
}

#[test]
fn indented_interpolating_heredoc_segments_too() {
    let source = String::from("my $text = <<~\"END\";\n    value $v\n    END\n");
    match heredoc_body_kind(&source, true) {
        TokenType::InterpolatedHeredocBody(parts) => assert_eq!(
            parts,
            vec![
                StringPart::Literal("    value ".into()),
                StringPart::Variable("$v".into()),
                StringPart::Literal("\n".into())
            ]
        ),
        other => panic!("expected interpolated heredoc body, got {other:?}"),
    }
}

#[test]
fn non_interpolating_heredocs_are_invariant_controls() {
    let bodies = [
        (single_quoted_heredoc_source("value $name!"), "single-quoted"),
        (format!("my $text = <<\\END;\nvalue $name!\nEND\n"), "backslashed"),
        (format!("my $text = <<`END`;\nvalue $name!\nEND\n"), "backtick boundary"),
    ];
    for (source, label) in bodies {
        for interpolation in [true, false] {
            assert!(
                matches!(heredoc_body_kind(&source, interpolation), TokenType::HeredocBody(_)),
                "{label} heredoc must stay HeredocBody under interpolation={interpolation}"
            );
        }
    }
}

#[test]
fn heredoc_identity_and_geometry_are_stable_across_settings() {
    let source = heredoc_source("value $name");
    let enabled = heredoc_tokens_with(&source, true);
    let disabled = heredoc_tokens_with(&source, false);
    let pick = |tokens: &[Token]| {
        tokens
            .iter()
            .find(|t| {
                matches!(
                    t.token_type,
                    TokenType::HeredocBody(_) | TokenType::InterpolatedHeredocBody(_)
                )
            })
            .expect("body token")
            .clone()
    };
    let (a, b) = (pick(&enabled), pick(&disabled));
    assert_eq!(a.start, b.start, "body geometry must not move");
    assert_eq!(a.end, b.end, "body geometry must not move");
    assert_eq!(a.text, b.text, "body token text stays geometry-only");
}

#[test]
fn checkpoint_rejects_a_different_interpolation_policy_for_pending_heredocs() {
    let source = heredoc_source("value $name");
    let mut enabled = PerlLexer::with_config_and_body_tokens(&source, config(true));
    while enabled.checkpoint().pending_heredocs.is_empty() {
        assert!(enabled.next_token().is_some(), "heredoc opener should be reached");
    }
    let checkpoint = enabled.checkpoint();

    let disabled = PerlLexer::with_config_and_body_tokens(&source, config(false));
    assert!(!disabled.can_restore(&checkpoint));

    let same_policy = PerlLexer::with_config_and_body_tokens(&source, config(true));
    assert!(same_policy.can_restore(&checkpoint));
}
