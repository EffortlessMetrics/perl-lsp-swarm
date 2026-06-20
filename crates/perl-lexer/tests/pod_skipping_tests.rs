//! Tests for POD block skipping in the lexer's whitespace/comment skipper.
//!
//! POD (Plain Old Documentation) blocks start with `=head1`, `=pod`, `=over`, etc.
//! at the beginning of a line and end with `=cut` at the beginning of a line.
//! The lexer should skip these blocks entirely, producing no tokens for them.

use perl_lexer::{PerlLexer, Token, TokenType};

type R = Result<(), Box<dyn std::error::Error>>;

/// Collect all tokens from input.
fn tokens(input: &str) -> Vec<Token> {
    PerlLexer::new(input).collect_tokens()
}

/// Collect only significant (non-whitespace, non-newline, non-EOF) tokens.
fn significant(input: &str) -> Vec<Token> {
    tokens(input)
        .into_iter()
        .filter(|t| {
            !matches!(t.token_type, TokenType::Whitespace | TokenType::Newline | TokenType::EOF)
        })
        .collect()
}

// ===========================================================================
// 1. POD between statements
// ===========================================================================

#[test]
fn pod_between_statements_is_skipped() -> R {
    let code = "my $x = 1;\n=head1 NAME\nFoo\n=cut\nmy $y = 2;";
    let toks = significant(code);
    // Should have tokens for both `my $x = 1;` and `my $y = 2;`
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();
    assert!(texts.contains(&"my"), "Should contain 'my' keyword: {texts:?}");
    // Should have two 'my' keywords (one for each statement)
    let my_count = texts.iter().filter(|&&t| t == "my").count();
    assert_eq!(my_count, 2, "Should have two 'my' keywords, got: {texts:?}");
    // Should NOT contain any POD-related tokens
    assert!(
        !texts.iter().any(|t| t.starts_with("=head") || t.starts_with("=cut")),
        "Should not contain POD tokens: {texts:?}"
    );
    Ok(())
}

// ===========================================================================
// 2. POD at start of file
// ===========================================================================

#[test]
fn pod_at_start_of_file_is_skipped() -> R {
    let code = "=head1 NAME\nFoo\n=cut\nmy $x = 1;";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();
    assert!(texts.contains(&"my"), "Should contain 'my': {texts:?}");
    assert!(
        !texts.iter().any(|t| t.starts_with("=head") || t.starts_with("=cut")),
        "Should not contain POD tokens: {texts:?}"
    );
    Ok(())
}

// ===========================================================================
// 3. POD at EOF without =cut
// ===========================================================================

#[test]
fn pod_at_eof_without_cut_is_skipped() -> R {
    let code = "my $x = 1;\n=head1 NAME\nFoo";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();
    assert!(texts.contains(&"my"), "Should contain 'my': {texts:?}");
    assert!(
        !texts.iter().any(|t| t.starts_with("=head")),
        "Should not contain POD tokens: {texts:?}"
    );
    Ok(())
}

// ===========================================================================
// 4. Multiple POD sections in one file
// ===========================================================================

#[test]
fn multiple_pod_sections_are_skipped() -> R {
    let code =
        "my $a = 1;\n=head1 FIRST\nstuff\n=cut\nmy $b = 2;\n=pod\nmore stuff\n=cut\nmy $c = 3;";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();
    let my_count = texts.iter().filter(|&&t| t == "my").count();
    assert_eq!(my_count, 3, "Should have three 'my' keywords, got: {texts:?}");
    assert!(
        !texts
            .iter()
            .any(|t| t.starts_with("=head") || t.starts_with("=pod") || t.starts_with("=cut")),
        "Should not contain POD tokens: {texts:?}"
    );
    Ok(())
}

// ===========================================================================
// 5. Non-POD '=' at start of line should NOT be skipped
// ===========================================================================

#[test]
fn non_pod_equals_at_line_start_not_skipped() -> R {
    // $x\n= 1 — the '=' here is an assignment operator at column 0, not POD
    let code = "$x\n= 1;";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();
    assert!(texts.contains(&"="), "Assignment '=' should be preserved: {texts:?}");
    assert!(texts.contains(&"1"), "Number '1' should be present: {texts:?}");
    Ok(())
}

#[test]
fn non_pod_equals_equals_at_line_start_not_skipped() -> R {
    // == at line start is not a POD directive
    let code = "$x\n== $y;";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();
    assert!(texts.contains(&"=="), "Equality '==' should be preserved: {texts:?}");
    Ok(())
}

// ===========================================================================
// 6. Various POD directive types
// ===========================================================================

#[test]
fn pod_directive_types_are_all_skipped() -> R {
    let directives = [
        "=pod",
        "=head1",
        "=head2",
        "=over",
        "=item",
        "=back",
        "=begin",
        "=end",
        "=for",
        "=encoding",
    ];
    for directive in &directives {
        let code = format!("my $x = 1;\n{directive} stuff\ncontent\n=cut\nmy $y = 2;");
        let toks = significant(&code);
        let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();
        let my_count = texts.iter().filter(|&&t| t == "my").count();
        assert_eq!(my_count, 2, "Directive '{directive}' should be skipped; tokens: {texts:?}");
    }
    Ok(())
}

// ===========================================================================
// 7. POD with only =cut on last line (no trailing newline)
// ===========================================================================

#[test]
fn pod_with_cut_at_eof_no_trailing_newline() -> R {
    let code = "my $x = 1;\n=head1 NAME\nFoo\n=cut";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();
    assert!(texts.contains(&"my"), "Should contain 'my': {texts:?}");
    assert!(
        !texts.iter().any(|t| t.starts_with("=head") || t.starts_with("=cut")),
        "Should not contain POD tokens: {texts:?}"
    );
    Ok(())
}

// ===========================================================================
// 8. POD followed by code on same line as after =cut line
// ===========================================================================

#[test]
fn code_immediately_after_cut_line() -> R {
    let code = "=head1 NAME\nFoo\n=cut\nprint 42;";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();
    assert!(texts.contains(&"print"), "Should contain 'print': {texts:?}");
    assert!(texts.contains(&"42"), "Should contain '42': {texts:?}");
    Ok(())
}

// ===========================================================================
// 9. POD with multi-byte UTF-8 content (byte-safety regression test)
// ===========================================================================

#[test]
fn pod_with_multibyte_utf8_content() -> R {
    // Ensure the byte-oriented POD scanner doesn't panic on multi-byte chars
    let code = "my $x = 1;\n=head1 NAÏVE ÜBERSICHT\n日本語テスト\n=cut\nmy $y = 2;";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();
    let my_count = texts.iter().filter(|&&t| t == "my").count();
    assert_eq!(my_count, 2, "Should have two 'my' keywords: {texts:?}");
    Ok(())
}

// ===========================================================================
// 10. =begin...=end POD blocks (Issue #1860 — termination by matching =end FORMAT)
// ===========================================================================

#[test]
fn test_begin_end_pod_blocks_terminate_correctly_html() -> R {
    // Per perlpod, =begin html...=end html should terminate at =end html, not =cut.
    // Code after =end html should be lexed as Perl, not consumed as POD.
    let code = "my $before = 1;\n=begin html\n<b>bold</b>\n=end html\nmy $x = 1;";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();

    // Should have two 'my' tokens (one for $before, one for $x)
    let my_count = texts.iter().filter(|&&t| t == "my").count();
    assert_eq!(my_count, 2, "Should have two 'my' keywords (before and after =end html): {texts:?}");

    // Should contain both $before and $x variables
    assert!(texts.contains(&"$before"), "Should contain '$before': {texts:?}");
    assert!(texts.contains(&"$x"), "Should contain '$x' (code after =end html): {texts:?}");

    // Should contain the '1' after $x
    let ones: Vec<&&str> = texts.iter().filter(|&&t| t == "1").collect();
    assert!(ones.len() >= 2, "Should have at least two '1' literals: {texts:?}");

    Ok(())
}

#[test]
fn test_begin_end_pod_blocks_with_different_format_token() -> R {
    // =begin xml...=end xml should work similarly
    let code = "my $a = 1;\n=begin xml\n<root/>\n=end xml\nmy $b = 2;";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();

    let my_count = texts.iter().filter(|&&t| t == "my").count();
    assert_eq!(my_count, 2, "Should have two 'my' keywords: {texts:?}");

    assert!(texts.contains(&"$a"), "Should contain '$a': {texts:?}");
    assert!(texts.contains(&"$b"), "Should contain '$b' (code after =end xml): {texts:?}");

    Ok(())
}

#[test]
fn test_for_pod_block_terminates_at_blank_line() -> R {
    // Per perlpod, =for FORMAT blocks terminate at the next blank line.
    // The code after the blank line should be lexed as Perl.
    let code = "=for html <i>italic</i>\n\nmy $y = 2;";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();

    // Should contain 'my' and '$y' (code after blank line)
    assert!(texts.contains(&"my"), "Should contain 'my': {texts:?}");
    assert!(texts.contains(&"$y"), "Should contain '$y' (code after blank line): {texts:?}");
    assert!(texts.contains(&"2"), "Should contain '2': {texts:?}");

    // Should NOT contain the HTML content from =for block
    assert!(!texts.iter().any(|t| t.contains("italic")),
        "Should not contain HTML content from =for block: {texts:?}");

    Ok(())
}

#[test]
fn test_for_without_blank_line_until_eof() -> R {
    // If =for block never reaches a blank line, it should consume to EOF
    let code = "=for text\nThis is documentation\nWith multiple lines\nNo blank line after";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();

    // Everything should be skipped; no significant tokens
    assert!(texts.is_empty(), "Should have no significant tokens (all POD): {texts:?}");

    Ok(())
}

#[test]
fn test_pod_cut_unchanged_regression() -> R {
    // =pod...=cut behavior should remain unchanged (still terminated by =cut, not =end)
    let code = "my $x = 1;\n=pod\nPOD content\n=cut\nmy $y = 2;";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();

    let my_count = texts.iter().filter(|&&t| t == "my").count();
    assert_eq!(my_count, 2, "Should have two 'my' keywords: {texts:?}");
    assert!(texts.contains(&"$x"), "Should contain '$x': {texts:?}");
    assert!(texts.contains(&"$y"), "Should contain '$y': {texts:?}");

    Ok(())
}

// ===========================================================================
// 11. Adversarial tests for literal/comment blindness (PARSER-1 hazard)
// ===========================================================================

#[test]
fn test_begin_end_inside_string_literal_should_not_terminate_pod() -> R {
    // The =begin/=end markers should not be recognized inside string literals.
    // This is a scanner context-blindness hazard.
    let code = "=begin html\nmy $html = \"<div>=end html</div>\";\n=end html\nmy $y = 2;";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();

    // The =begin block should consume the entire code including the string literal,
    // terminating only at the real =end html on its own line.
    // So we should see: my, $y, =, 2 (the code after =end html)
    assert!(texts.contains(&"my"), "Should contain 'my' from code after =end html: {texts:?}");
    assert!(texts.contains(&"$y"), "Should contain '$y' from code after =end html: {texts:?}");

    // The string literal contents should be hidden inside POD, not tokenized separately
    assert!(!texts.contains(&"div"), "Should not tokenize 'div' from inside POD block: {texts:?}");

    Ok(())
}

#[test]
fn test_begin_end_in_comment_should_not_terminate_pod() -> R {
    // =begin/=end inside a comment should not affect POD block termination
    let code = "=begin html\n# This is a comment with =end html in it\nReal content\n=end html\nmy $x = 1;";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();

    // The =begin block should still be terminated by the real =end html,
    // so we should see 'my' and '$x' from the code after.
    assert!(texts.contains(&"my"), "Should contain 'my': {texts:?}");
    assert!(texts.contains(&"$x"), "Should contain '$x': {texts:?}");

    Ok(())
}

#[test]
fn test_nested_begin_blocks_first_end_wins() -> R {
    // If there are nested =begin blocks (invalid POD but defensive test),
    // the first matching =end should win for the outermost =begin.
    // This tests that FORMAT token matching is correct.
    let code = "=begin html\n=begin text\ninner\n=end text\n=end html\nmy $x = 1;";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();

    // The outer =begin html should terminate at =end html (not at =end text).
    // So we should see 'my' and '$x'.
    assert!(texts.contains(&"my"), "Should contain 'my' after outer =end html: {texts:?}");
    assert!(texts.contains(&"$x"), "Should contain '$x' after outer =end html: {texts:?}");

    Ok(())
}

#[test]
fn test_begin_format_with_multiple_words() -> R {
    // Some POD formats might have additional words after the format token.
    // The =end FORMAT should match only the format token, not the entire line.
    let code = "=begin html This is extra stuff\nContent\n=end html\nmy $x = 1;";
    let toks = significant(code);
    let texts: Vec<&str> = toks.iter().map(|t| t.text.as_ref()).collect();

    // Should terminate at =end html despite the extra words on =begin line.
    assert!(texts.contains(&"my"), "Should contain 'my': {texts:?}");
    assert!(texts.contains(&"$x"), "Should contain '$x': {texts:?}");

    Ok(())
}
