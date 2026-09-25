use perl_lexer::{LocalSymbolTable, PerlLexer, TokenType};

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
    let mut lx = PerlLexer::new(&with_slash);
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
    let mut lx = PerlLexer::new(&with_slash);
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
