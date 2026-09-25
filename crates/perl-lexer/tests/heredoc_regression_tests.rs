use perl_lexer::{PerlLexer, TokenType};

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
