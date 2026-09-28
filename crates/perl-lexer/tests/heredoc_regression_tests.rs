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
fn lexer_term_slot_heredoc_bodies_stay_out_of_the_public_slash_path() {
    // `return <<END` is a definite heredoc (local Perl oracle), so the body
    // prose must not become a known sub and must not take the known-sub regex
    // path, while the declaration after the terminator keeps it.
    let source = "sub f { return <<END unless $cond; }\nsub fake { }\nEND\nsub real { }\n";
    let table = LocalSymbolTable::scan_subs(source);
    assert!(!table.is_known_sub("fake"), "body prose leaked: {source:?}");
    assert!(table.is_known_sub("real"), "suffix declaration lost: {source:?}");

    let with_slash = format!("{source}fake /x/;\n");
    let config = LexerConfig { symbol_table: Some(table), ..LexerConfig::default() };
    let mut lx = PerlLexer::with_config_and_body_tokens(&with_slash, config);
    let mut saw_division = false;
    let mut took_regex_path = false;
    while let Some(token) = lx.next_token() {
        match token.token_type {
            TokenType::Division => saw_division = true,
            TokenType::RegexMatch => took_regex_path = true,
            TokenType::EOF => break,
            _ => {}
        }
    }
    assert!(!took_regex_path, "body prose took the known-sub regex path");
    assert!(saw_division, "fake /x/ did not lex as division");
}

#[test]
fn lexer_dereferenced_return_method_keeps_shift_lines_live() {
    // `$object->return <<END` is a method invocation, not the `return`
    // keyword (local Perl oracle): the arrow supplies the left operand, so
    // the `<<` is a left shift and the lines after it stay live code, with
    // the declaration between the shift and the bare `END` line keeping the
    // known-sub regex path.
    let source = "my $x = $object->return <<END;\nsub fake { }\nEND\nsub real { }\n";
    let table = LocalSymbolTable::scan_subs(source);
    assert!(table.is_known_sub("fake"), "shift operand swallowed live code: {source:?}");
    assert!(table.is_known_sub("real"), "suffix declaration lost: {source:?}");

    let with_slash = format!("{source}fake /x/;\n");
    let config = LexerConfig { symbol_table: Some(table), ..LexerConfig::default() };
    let mut lx = PerlLexer::with_config_and_body_tokens(&with_slash, config);
    let mut took_regex_path = false;
    while let Some(token) = lx.next_token() {
        match token.token_type {
            TokenType::RegexMatch => took_regex_path = true,
            TokenType::EOF => break,
            _ => {}
        }
    }
    assert!(took_regex_path, "fake /x/ lost the known-sub regex path");
}

#[test]
fn lexer_dereferenced_return_method_with_space_keeps_shift_lines_live() {
    // The spaced form exercises the guard's trailing-space trim: the slice
    // before the word is `"$object-> "` and must still read as a method
    // invocation, not the `return` keyword (#16433-review finding on the
    // tight-form-only guard).
    let source = "my $x = $object-> return <<END;
sub fake { }
END
sub real { }
";
    let table = LocalSymbolTable::scan_subs(source);
    assert!(
        table.is_known_sub("fake"),
        "spaced-form shift operand swallowed live code: {source:?}"
    );
    assert!(table.is_known_sub("real"), "spaced-form suffix declaration lost: {source:?}");
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

#[test]
fn lexer_sigiled_print_is_a_shift_operand_not_a_heredoc_introducer() {
    use perl_lexer::LocalSymbolTable;

    // `$print` is a completed scalar term, so `<<` is a left shift and the
    // following lines stay live code. Bare `print <<END` is the
    // matching-marker opposite control: it stays a heredoc introducer, so
    // its body prose must stay out of the known-sub scan.
    let shift =
        "my $print = shift;\nmy $width = $print <<'END';\nsub fake { }\nEND\nsub real { }\n";
    let table = LocalSymbolTable::scan_subs(shift);
    assert!(table.is_known_sub("fake"), "shift operand swallowed live code: {shift:?}");
    assert!(table.is_known_sub("real"), "suffix declaration lost: {shift:?}");

    let heredoc = "print <<END;\nsub fake { }\nEND\nsub real { }\n";
    let table = LocalSymbolTable::scan_subs(heredoc);
    assert!(!table.is_known_sub("fake"), "heredoc body leaked: {heredoc:?}");
    assert!(table.is_known_sub("real"), "suffix declaration lost: {heredoc:?}");

    // Mandatory negative control: `foo` is unprototyped, so Perl itself
    // reads `foo <<END` as the call consuming the heredoc; that callable
    // authority must survive this slice.
    let unprototyped = "sub foo { 4 }\nprint foo <<END;\nsub fake { }\nEND\nsub real { }\n";
    let table = LocalSymbolTable::scan_subs(unprototyped);
    assert!(
        !table.is_known_sub("fake"),
        "unprototyped foo lost callable authority: {unprototyped:?}"
    );
    assert!(table.is_known_sub("real"), "suffix declaration lost: {unprototyped:?}");
}

#[test]
fn lexer_typeglob_and_last_index_terms_are_shift_operands_not_heredoc_introducers() {
    use perl_lexer::LocalSymbolTable;

    // `*print` and `$#print` are completed terms too, so their `<<` is a
    // left shift and the following lines stay live code, exactly like the
    // `$print` scalar form. Bare `print <<END` is the matching-marker
    // opposite control: it stays a heredoc introducer, so its body prose
    // must stay out of the known-sub scan.
    let glob = "my $width = *print <<'END';\nsub fake { }\nEND\nsub real { }\n";
    let table = LocalSymbolTable::scan_subs(glob);
    assert!(table.is_known_sub("fake"), "shift operand swallowed live code: {glob:?}");
    assert!(table.is_known_sub("real"), "suffix declaration lost: {glob:?}");

    let last_index = "my $width = $#print <<'END';\nsub fake { }\nEND\nsub real { }\n";
    let table = LocalSymbolTable::scan_subs(last_index);
    assert!(table.is_known_sub("fake"), "shift operand swallowed live code: {last_index:?}");
    assert!(table.is_known_sub("real"), "suffix declaration lost: {last_index:?}");

    let heredoc = "print <<END;\nsub fake { }\nEND\nsub real { }\n";
    let table = LocalSymbolTable::scan_subs(heredoc);
    assert!(!table.is_known_sub("fake"), "heredoc body leaked: {heredoc:?}");
    assert!(table.is_known_sub("real"), "suffix declaration lost: {heredoc:?}");
}
