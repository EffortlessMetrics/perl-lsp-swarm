use perl_lexer::{LexerConfig, LocalSymbolTable, PerlLexer, TokenType};

#[test]
fn lexer_terminates_on_backtick_heredoc_with_cr() {
    let mut lx = PerlLexer::new("``<<a\r");

    // Try to consume up to 16 tokens - should not spin forever
    for i in 0..16 {
        if let Some(token) = lx.next_token() {
            // Just consume tokens, we're checking for termination
            if matches!(token.token_type, perl_lexer::TokenType::EOF) {
                // Found EOF, lexer terminated properly
                break;
            }
        } else {
            // No more tokens
            break;
        }

        // Safety check - if we're still going after 15 iterations, something's wrong
        assert!(i < 15, "Lexer appears to be in infinite loop");
    }

    // If we got here, the lexer terminated properly
    // Test passed - lexer terminated without infinite loop
}

#[test]
fn lexer_handles_heredoc_with_various_line_endings() {
    // Test with LF
    let mut lx = PerlLexer::new("<<EOF\nHello\nEOF\n");
    let mut token_count = 0;
    while let Some(token) = lx.next_token() {
        token_count += 1;
        if matches!(token.token_type, perl_lexer::TokenType::EOF) {
            break;
        }
        assert!(token_count < 20, "Too many tokens, possible infinite loop");
    }

    // Test with CRLF
    let mut lx = PerlLexer::new("<<EOF\r\nHello\r\nEOF\r\n");
    let mut token_count = 0;
    while let Some(token) = lx.next_token() {
        token_count += 1;
        if matches!(token.token_type, perl_lexer::TokenType::EOF) {
            break;
        }
        assert!(token_count < 20, "Too many tokens, possible infinite loop");
    }

    // Test with just CR (old Mac style)
    let mut lx = PerlLexer::new("<<EOF\rHello\rEOF\r");
    let mut token_count = 0;
    while let Some(token) = lx.next_token() {
        token_count += 1;
        if matches!(token.token_type, perl_lexer::TokenType::EOF) {
            break;
        }
        assert!(token_count < 20, "Too many tokens, possible infinite loop");
    }
}

#[test]
fn lexer_handles_malformed_heredoc_gracefully() {
    // Heredoc without terminator
    let mut lx = PerlLexer::new("<<EOF\nThis heredoc never ends");
    let mut token_count = 0;
    while let Some(token) = lx.next_token() {
        token_count += 1;
        if matches!(token.token_type, perl_lexer::TokenType::EOF) {
            break;
        }
        assert!(token_count < 30, "Too many tokens, possible infinite loop");
    }

    // Empty heredoc delimiter
    let mut lx = PerlLexer::new("<<\nContent\n");
    let mut token_count = 0;
    while let Some(token) = lx.next_token() {
        token_count += 1;
        if matches!(token.token_type, perl_lexer::TokenType::EOF) {
            break;
        }
        assert!(token_count < 20, "Too many tokens, possible infinite loop");
    }
}

#[test]
fn lexer_rejects_unterminated_backtick_heredoc_label() {
    let input = "<<`EOF\rprint 1;\r";
    let mut lx = PerlLexer::new(input);
    let tokens = lx.collect_tokens();

    let has_heredoc = tokens.iter().any(|t| matches!(t.token_type, TokenType::HeredocStart));
    assert!(!has_heredoc, "unterminated backtick label should not become heredoc");
    assert!(tokens.iter().any(|t| matches!(t.token_type, TokenType::EOF)));
}

#[test]
fn lexer_handles_data_markers_with_cr_line_endings() {
    let input = "my $x = 1;\r__DATA__\rline one\rline two\r";
    let mut lx = PerlLexer::new(input);
    let tokens = lx.collect_tokens();

    assert!(
        tokens
            .iter()
            .any(|t| matches!(&t.token_type, TokenType::DataMarker(marker) if marker.as_ref() == "__DATA__")),
        "expected __DATA__ marker in CR-delimited source"
    );
    assert!(
        tokens.iter().any(|t| matches!(t.token_type, TokenType::DataBody(_))),
        "expected data body after __DATA__ marker"
    );
}

#[test]
fn heredoc_body_dispatch_precedes_pod_and_comment_skipping() -> Result<(), String> {
    for body in ["", "ordinary\n", "=head1 NAME\nsub fake {}\n", "# comment\n", "\n  \n"] {
        let source = format!("my $x = <<'END';\n{body}END\nsub real {{}}\n");
        let tokens = PerlLexer::with_body_tokens(&source).collect_tokens();
        if !tokens.iter().any(|token| {
            matches!(token.token_type, TokenType::HeredocBody(_))
                && source.get(token.start..token.end) == Some(body)
        }) {
            return Err(format!("body lost or changed: {body:?}; {tokens:?}"));
        }
        if !tokens.iter().any(|token| token.text.as_ref() == "real") {
            return Err(format!("post-heredoc code lost: {body:?}; {tokens:?}"));
        }
    }
    Ok(())
}

#[test]
fn nullary_prototype_sub_completes_term_so_marker_is_left_shift() {
    // Local Perl oracle (#16165): `sub foo () { 4 }` followed by
    // `print foo <<'END'` completes `foo()` as a term, so `<<` is left
    // shift and the `<<'END'` opener is NOT a heredoc. The lines after the
    // opener therefore stay code: the would-be body declaration is visible,
    // and so is the suffix declaration after the marker.
    let source = "sub foo () { 4 }\nprint foo <<'END';\nsub phantom { }\nEND\nsub real { }\n";
    let table = LocalSymbolTable::scan_subs(source);
    assert!(table.is_known_sub("foo"), "declaration lost: {source:?}");
    assert!(table.is_nullary_sub("foo"), "nullary prototype not captured: {source:?}");
    assert!(table.is_known_sub("phantom"), "body scanned as heredoc: {source:?}");
    assert!(table.is_known_sub("real"), "suffix declaration lost: {source:?}");

    let config = LexerConfig { symbol_table: Some(table), ..Default::default() };
    let tokens = PerlLexer::with_config(source, config).collect_tokens();
    assert!(
        !tokens.iter().any(|t| matches!(t.token_type, TokenType::HeredocStart)),
        "nullary call must not consume <<'END' as a heredoc: {source:?}"
    );
}

#[test]
fn nullary_prototype_declared_after_the_call_still_blocks_heredoc() {
    // Prototypes are file-scoped in Perl, so the two-pass hint must carry the
    // empty prototype back to the earlier opener (#16165).
    let source = "print foo <<'END';\nsub phantom { }\nEND\nsub foo () { 4 }\n";
    let table = LocalSymbolTable::scan_subs(source);
    assert!(table.is_nullary_sub("foo"), "hint pass lost the prototype: {source:?}");
    assert!(table.is_known_sub("phantom"), "body scanned as heredoc: {source:?}");

    let config = LexerConfig { symbol_table: Some(table), ..Default::default() };
    let tokens = PerlLexer::with_config(source, config).collect_tokens();
    assert!(
        !tokens.iter().any(|t| matches!(t.token_type, TokenType::HeredocStart)),
        "later nullary declaration must still block the heredoc: {source:?}"
    );
}

#[test]
fn unprototyped_sub_keeps_heredoc_reading_negative_control() {
    // Mandatory negative control (local Perl oracle): an unprototyped `foo`
    // can still take arguments, so `print foo <<'END'` IS a heredoc — the
    // body is consumed and the suffix declaration is the only one visible.
    let source = "sub foo { 4 }\nprint foo <<'END';\nsub phantom { }\nEND\nsub real { }\n";
    let table = LocalSymbolTable::scan_subs(source);
    assert!(table.is_known_sub("foo") && !table.is_nullary_sub("foo"));
    assert!(!table.is_known_sub("phantom"), "body not consumed: {source:?}");
    assert!(table.is_known_sub("real"), "suffix declaration lost: {source:?}");

    let config = LexerConfig { symbol_table: Some(table), ..Default::default() };
    let tokens = PerlLexer::with_config(source, config).collect_tokens();
    assert!(
        tokens.iter().any(|t| matches!(t.token_type, TokenType::HeredocStart)),
        "unprototyped sub must keep the callable-heredoc reading: {source:?}"
    );
}

#[test]
fn nullary_builtin_time_shifts_instead_of_consuming_heredoc() {
    // Local Perl oracle (#16165): `print time <<'END'` treats `time()` as a
    // complete term, so `<<` is left shift. The bounded nullary-builtin list
    // is what keeps this off the heredoc path even though `time` is a bare
    // term builtin.
    let source = "print time <<'END';\nsub phantom { }\nEND\nsub real { }\n";
    let table = LocalSymbolTable::scan_subs(source);
    assert!(table.is_known_sub("phantom"), "body scanned as heredoc: {source:?}");
    assert!(table.is_known_sub("real"), "suffix declaration lost: {source:?}");

    let config = LexerConfig { symbol_table: Some(table), ..Default::default() };
    let tokens = PerlLexer::with_config(source, config).collect_tokens();
    assert!(
        !tokens.iter().any(|t| matches!(t.token_type, TokenType::HeredocStart)),
        "nullary builtin must not consume <<'END' as a heredoc: {source:?}"
    );
}

#[test]
fn spaced_nullary_prototype_completes_term_so_marker_is_left_shift() {
    // Local Perl oracle (#16165, perl 5.42.2): whitespace inside a prototype
    // is ignored, so `sub foo (  ) { 4 }` is still a zero-argument declaration
    // (`prototype \&foo` keeps the literal spaces, but `foo(1)` is rejected
    // with "Too many arguments" and `foo <<'END'` shifts). The spaces-only
    // parens must therefore record as nullary, not plain callable.
    let source = "sub foo (  ) { 4 }\nprint foo <<'END';\nsub phantom { }\nEND\nsub real { }\n";
    let table = LocalSymbolTable::scan_subs(source);
    assert!(table.is_known_sub("foo"), "declaration lost: {source:?}");
    assert!(table.is_nullary_sub("foo"), "spaced empty prototype not captured: {source:?}");
    assert!(table.is_known_sub("phantom"), "body scanned as heredoc: {source:?}");
    assert!(table.is_known_sub("real"), "suffix declaration lost: {source:?}");

    let config = LexerConfig { symbol_table: Some(table), ..Default::default() };
    let tokens = PerlLexer::with_config(source, config).collect_tokens();
    assert!(
        !tokens.iter().any(|t| matches!(t.token_type, TokenType::HeredocStart)),
        "spaced nullary call must not consume <<'END' as a heredoc: {source:?}"
    );
}

#[test]
fn single_space_and_tab_nullary_prototypes_still_complete_term() {
    // Local Perl oracle (#16165, perl 5.42.2): both `( )` and a tab inside the
    // parens leave `foo <<'END'` a left shift, same mechanism as `(  )`.
    for prototype in ["( )", "(\t)"] {
        let source = format!(
            "sub foo {prototype} {{ 4 }}\nprint foo <<'END';\nsub phantom {{ }}\nEND\nsub real {{ }}\n"
        );
        let table = LocalSymbolTable::scan_subs(&source);
        assert!(table.is_nullary_sub("foo"), "prototype {prototype:?} not captured: {source:?}");
        assert!(table.is_known_sub("phantom"), "body scanned as heredoc: {source:?}");
        assert!(table.is_known_sub("real"), "suffix declaration lost: {source:?}");

        let config = LexerConfig { symbol_table: Some(table), ..Default::default() };
        let tokens = PerlLexer::with_config(&source, config).collect_tokens();
        assert!(
            !tokens.iter().any(|t| matches!(t.token_type, TokenType::HeredocStart)),
            "prototype {prototype:?} must not consume <<'END' as a heredoc: {source:?}"
        );
    }
}

#[test]
fn whitespace_prototype_with_parameter_keeps_heredoc_reading_negative_control() {
    // Mandatory negative control (local Perl oracle): `( $ )` declares one
    // scalar parameter, so `print foo <<'END'` IS a heredoc — perl consumes
    // the body as the argument. Inner whitespace must not blur a non-empty
    // prototype into the nullary class.
    let source = "sub foo ( $ ) { 4 }\nprint foo <<'END';\nsub phantom { }\nEND\nsub real { }\n";
    let table = LocalSymbolTable::scan_subs(source);
    assert!(table.is_known_sub("foo") && !table.is_nullary_sub("foo"));
    assert!(!table.is_known_sub("phantom"), "body not consumed: {source:?}");
    assert!(table.is_known_sub("real"), "suffix declaration lost: {source:?}");

    let config = LexerConfig { symbol_table: Some(table), ..Default::default() };
    let tokens = PerlLexer::with_config(source, config).collect_tokens();
    assert!(
        tokens.iter().any(|t| matches!(t.token_type, TokenType::HeredocStart)),
        "one-parameter prototype must keep the callable-heredoc reading: {source:?}"
    );
}
