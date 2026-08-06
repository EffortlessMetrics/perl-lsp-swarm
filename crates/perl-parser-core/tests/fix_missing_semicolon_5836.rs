//! Regression tests for missing semicolon recovery and quote-like operator edge cases.
//! Issue #5836: Narrow missing-semicolon recovery false positives
//! PR #5838: New helpers for quote-like body scanning and statement termination
//!
//! These tests exercise the new `quote_like_body_end`, `quote_like_part_end`,
//! `quote_like_unpaired_end`, and `statement_span_heredoc_tag` helpers added in this PR.
//! They guard against RIPR mutation gaps by testing critical branch paths:
//! - One-part vs two-part delimiters (s/tr/y vs m/q/qq/qx/qr/qw)
//! - Paired vs unpaired delimiters (angle brackets / parens vs pipes / slashes)
//! - Nested and escaped delimiters
//! - Heredoc tag extraction and recovery
//! - Statement termination with and without pending heredocs

mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::Parser;

// --- Quote-like operators: one-part delimiters ---

#[test]
fn quote_like_m_slash() {
    // m/pattern/ — one-part, unpaired slash delimiter
    assert_clean_parse(r#"if ($x =~ m/pattern/) { }"#);
}

#[test]
fn quote_like_m_slash_modifiers() {
    // m/pattern/modifiers
    assert_clean_parse(r#"if ($x =~ m/pattern/ix) { }"#);
}

#[test]
fn quote_like_q_slash() {
    // q/string/ — one-part, unpaired slash delimiter
    assert_clean_parse(r#"my $s = q/hello world/;"#);
}

#[test]
fn quote_like_qq_slash() {
    // qq/string/ — one-part, unpaired slash delimiter with interpolation
    assert_clean_parse(r#"my $s = qq/hello $name/;"#);
}

#[test]
fn quote_like_qx_slash() {
    // qx/command/ — one-part, unpaired slash delimiter (backtick replacement)
    assert_clean_parse(r#"my $out = qx/echo hello/;"#);
}

#[test]
fn quote_like_qr_slash() {
    // qr/pattern/ — one-part, unpaired slash delimiter
    assert_clean_parse(r#"my $re = qr/pattern/;"#);
}

#[test]
fn quote_like_qr_slash_modifiers() {
    // qr/pattern/modifiers
    assert_clean_parse(r#"my $re = qr/pattern/xi;"#);
}

#[test]
fn quote_like_qw_slash() {
    // qw/words/ — one-part, unpaired slash delimiter
    assert_clean_parse(r#"my @a = qw/one two three/;"#);
}

// --- Quote-like operators: one-part, unpaired non-slash delimiters ---

#[test]
fn quote_like_m_pipe() {
    // m|pattern| — unpaired pipe delimiter
    assert_clean_parse(r#"if ($x =~ m|pattern|) { }"#);
}

#[test]
fn quote_like_m_bang() {
    // m!pattern! — unpaired bang delimiter
    assert_clean_parse(r#"if ($x =~ m!pattern!) { }"#);
}

#[test]
fn quote_like_q_pipe() {
    // q|string|
    assert_clean_parse(r#"my $s = q|hello|;"#);
}

#[test]
fn quote_like_qq_bang() {
    // qq!string!
    assert_clean_parse(r#"my $s = qq!hello $name!;"#);
}

#[test]
fn quote_like_qx_pipe() {
    // qx|command|
    assert_clean_parse(r#"my $out = qx|echo hello|;"#);
}

#[test]
fn quote_like_qr_bang() {
    // qr!pattern!
    assert_clean_parse(r#"my $re = qr!pattern!;"#);
}

#[test]
fn quote_like_qw_pipe() {
    // qw|words|
    assert_clean_parse(r#"my @a = qw|one two three|;"#);
}

// --- Quote-like operators: one-part, paired delimiters ---

#[test]
fn quote_like_m_parens() {
    // m(pattern) — paired parens delimiter
    assert_clean_parse(r#"if ($x =~ m(pattern)) { }"#);
}

#[test]
fn quote_like_m_brackets() {
    // m[pattern] — paired square bracket delimiter
    assert_clean_parse(r#"if ($x =~ m[pattern]) { }"#);
}

#[test]
fn quote_like_m_braces() {
    // m{pattern} — paired curly brace delimiter
    assert_clean_parse(r#"if ($x =~ m{pattern}) { }"#);
}

#[test]
fn quote_like_m_angles() {
    // m<pattern> — paired angle bracket delimiter
    assert_clean_parse(r#"if ($x =~ m<pattern>) { }"#);
}

#[test]
fn quote_like_q_parens() {
    // q(string)
    assert_clean_parse(r#"my $s = q(hello);"#);
}

#[test]
fn quote_like_qq_brackets() {
    // qq[string]
    assert_clean_parse(r#"my $s = qq[hello $name];"#);
}

#[test]
fn quote_like_qx_braces() {
    // qx{command}
    assert_clean_parse(r#"my $out = qx{echo hello};"#);
}

#[test]
fn quote_like_qr_parens() {
    // qr(pattern)
    assert_clean_parse(r#"my $re = qr(pattern);"#);
}

#[test]
fn quote_like_qr_brackets() {
    // qr[pattern]
    assert_clean_parse(r#"my $re = qr[pattern];"#);
}

#[test]
fn quote_like_qr_braces() {
    // qr{pattern}
    assert_clean_parse(r#"my $re = qr{pattern};"#);
}

#[test]
fn quote_like_qw_parens() {
    // qw(words)
    assert_clean_parse(r#"my @a = qw(one two three);"#);
}

#[test]
fn quote_like_qw_brackets() {
    // qw[words]
    assert_clean_parse(r#"my @a = qw[one two three];"#);
}

#[test]
fn quote_like_qw_braces() {
    // qw{words}
    assert_clean_parse(r#"my @a = qw{one two three};"#);
}

#[test]
fn quote_like_qw_angles() {
    // qw<words>
    assert_clean_parse(r#"my @a = qw<one two three>;"#);
}

// --- Quote-like two-part operators: substitution (s/tr/y) ---

#[test]
fn quote_like_s_slash_slash() {
    // s/pattern/replacement/ — two-part, unpaired slash
    assert_clean_parse(r#"$x =~ s/foo/bar/;"#);
}

#[test]
fn quote_like_s_slash_slash_modifiers() {
    // s/pattern/replacement/modifiers
    assert_clean_parse(r#"$x =~ s/foo/bar/g;"#);
}

#[test]
fn quote_like_s_pipe_pipe() {
    // s|pattern|replacement| — two-part, unpaired pipe
    assert_clean_parse(r#"$x =~ s|foo|bar|;"#);
}

#[test]
fn quote_like_s_bang_bang() {
    // s!pattern!replacement! — two-part, unpaired bang
    assert_clean_parse(r#"$x =~ s!foo!bar!;"#);
}

#[test]
fn quote_like_s_parens_parens() {
    // s(pattern)(replacement) — two-part, paired parens
    assert_clean_parse(r#"$x =~ s(foo)(bar);"#);
}

#[test]
fn quote_like_s_brackets_brackets() {
    // s[pattern][replacement] — two-part, paired brackets
    assert_clean_parse(r#"$x =~ s[foo][bar];"#);
}

#[test]
fn quote_like_s_braces_braces() {
    // s{pattern}{replacement} — two-part, paired braces
    assert_clean_parse(r#"$x =~ s{foo}{bar};"#);
}

#[test]
fn quote_like_s_angles_angles() {
    // s<pattern><replacement> — two-part, paired angles
    assert_clean_parse(r#"$x =~ s<foo><bar>;"#);
}

#[test]
fn quote_like_tr_slash_slash() {
    // tr/a-z/A-Z/ — two-part, unpaired slash
    assert_clean_parse(r#"$x =~ tr/a-z/A-Z/;"#);
}

#[test]
fn quote_like_tr_pipe_pipe() {
    // tr|a-z|A-Z| — two-part, unpaired pipe
    assert_clean_parse(r#"$x =~ tr|a-z|A-Z|;"#);
}

#[test]
fn quote_like_tr_parens_parens() {
    // tr(a-z)(A-Z) — two-part, paired parens
    assert_clean_parse(r#"$x =~ tr(a-z)(A-Z);"#);
}

#[test]
fn quote_like_tr_brackets_brackets() {
    // tr[a-z][A-Z] — two-part, paired brackets
    assert_clean_parse(r#"$x =~ tr[a-z][A-Z];"#);
}

#[test]
fn quote_like_y_slash_slash() {
    // y/a-z/A-Z/ — two-part, unpaired slash (y is alias for tr)
    assert_clean_parse(r#"$x =~ y/a-z/A-Z/;"#);
}

#[test]
fn quote_like_y_braces_braces() {
    // y{a-z}{A-Z} — two-part, paired braces
    assert_clean_parse(r#"$x =~ y{a-z}{A-Z};"#);
}

// --- Nested delimiters in paired quote-like operators ---

#[test]
fn quote_like_nested_same_pair() {
    // qr{a{b}c} — nested same-type delimiters (braces inside braces)
    assert_clean_parse(r#"my $re = qr{a{b}c};"#);
}

#[test]
fn quote_like_nested_different_pair() {
    // qr{a[b]c} — nested different-type delimiters
    assert_clean_parse(r#"my $re = qr{a[b]c};"#);
}

#[test]
fn quote_like_nested_in_pattern() {
    // Complex nested angle brackets in pattern
    assert_clean_parse(r#"my $re = qr<(?:foo|bar)<baz>>xi;"#);
}

#[test]
fn quote_like_s_nested_pattern() {
    // s{pattern{nested}}{replacement}
    assert_clean_parse(r#"$x =~ s{a{b}}{c};"#);
}

#[test]
fn quote_like_tr_nested_list() {
    // tr(a-z(inner))(A-Z)
    assert_clean_parse(r#"$x =~ tr(a(z))(A(Z));"#);
}

// --- Escaped characters in quote-like operators ---

#[test]
fn quote_like_escaped_delimiter() {
    // s/\//bar/ — escaped delimiter
    assert_clean_parse(r#"$x =~ s/\//bar/;"#);
}

#[test]
fn quote_like_backslash_escape() {
    // qr/a\\b/ — escaped backslash
    assert_clean_parse(r#"my $re = qr/a\\b/;"#);
}

#[test]
fn quote_like_multiple_escapes() {
    // s/a\nb\tc/x\ny\tz/
    assert_clean_parse(r#"$x =~ s/a\nb\tc/x\ny\tz/;"#);
}

#[test]
fn quote_like_escaped_in_paired() {
    // qr{a\{b\}c} — escaped braces
    assert_clean_parse(r#"my $re = qr{a\{b\}c};"#);
}

// --- Empty quote-like operators ---

#[test]
fn quote_like_empty_m() {
    // m// — empty pattern (repeat last regex)
    assert_clean_parse(r#"if ($x =~ m//) { }"#);
}

#[test]
fn quote_like_empty_s() {
    // s/// — empty pattern and replacement
    assert_clean_parse(r#"$x =~ s///;"#);
}

#[test]
fn quote_like_empty_tr() {
    // tr/// — empty transliteration
    assert_clean_parse(r#"$x =~ tr///;"#);
}

#[test]
fn quote_like_empty_q() {
    // q() — empty string
    assert_clean_parse(r#"my $s = q();"#);
}

#[test]
fn quote_like_empty_qr() {
    // qr{} — empty pattern
    assert_clean_parse(r#"my $re = qr{};"#);
}

// --- Whitespace before delimiter ---

#[test]
fn quote_like_whitespace_qr_space() {
    // qr /pattern/ — space before delimiter
    assert_clean_parse(r#"my $re = qr /pattern/;"#);
}

#[test]
fn quote_like_whitespace_s_space() {
    // s /pattern/ /replacement/ — space before delimiters
    assert_clean_parse(r#"$x =~ s /foo/ /bar/;"#);
}

#[test]
fn quote_like_whitespace_tr_space() {
    // tr /a-z/ /A-Z/ — space before delimiters
    assert_clean_parse(r#"$x =~ tr /a-z/ /A-Z/;"#);
}

#[test]
fn quote_like_whitespace_m_angle() {
    // m <pattern> — space before angle bracket
    assert_clean_parse(r#"if ($x =~ m <pattern>) { }"#);
}

// --- Heredoc interaction with quote-like operators ---

#[test]
fn heredoc_before_quote_like() {
    // Heredoc marker followed by quote-like operator should not confuse
    assert_clean_parse(
        r#"my $text = <<'END';
some text
END
my $re = qr/pattern/;
"#,
    );
}

#[test]
fn heredoc_tag_in_quote_like_body() {
    // <<TAG inside a regex body should not trigger heredoc recovery
    assert_clean_parse(r#"my $re = qr/a << b c/;"#);
}

#[test]
fn multiple_quote_like_operators() {
    // Multiple quote-like operators in sequence
    assert_clean_parse(
        r#"
my $s1 = q/string/;
my $s2 = qq/string/;
my $re = qr/pattern/;
$x =~ s/foo/bar/;
"#,
    );
}

// --- Statement termination with quote-like operators ---

#[test]
fn quote_like_statement_with_semicolon() {
    // Quote-like statement followed by semicolon
    assert_clean_parse(r#"my $re = qr/pattern/;"#);
}

#[test]
fn quote_like_at_block_end() {
    // Quote-like statement at the end of a block (no semicolon needed)
    assert_clean_parse(
        r#"{
    my $re = qr/pattern/
}"#,
    );
}

#[test]
fn quote_like_followed_by_statement() {
    // Quote-like statement followed by another statement on next line
    assert_clean_parse(
        r#"my $re = qr/pattern/
my $x = 1;"#,
    );
}

#[test]
fn substitution_statement_with_semicolon() {
    // s/// statement with semicolon
    assert_clean_parse(r#"$x =~ s/foo/bar/;"#);
}

#[test]
fn substitution_at_block_end() {
    // s/// at end of block
    assert_clean_parse(
        r#"{
    $x =~ s/foo/bar/
}"#,
    );
}

// --- Edge cases and real-world patterns ---

#[test]
fn quote_like_in_ternary() {
    // Quote-like in ternary expression
    assert_clean_parse(r#"my $re = $mode ? qr/a/ : qr/b/;"#);
}

#[test]
fn quote_like_chained() {
    // Chained quote-like operators (return value used as argument)
    assert_clean_parse(r#"my @words = split /\s+/, qw/one two three/;"#);
}

#[test]
fn quote_like_as_function_argument() {
    // Quote-like operator as function argument
    assert_clean_parse(r#"print join ",", qw/a b c/;"#);
}

#[test]
fn s_with_modifiers_and_flags() {
    // s/// with multiple modifiers
    assert_clean_parse(r#"$x =~ s/foo/bar/gei;"#);
}

#[test]
fn complex_regex_with_nesting() {
    // Real-world complex regex pattern
    assert_clean_parse(r#"if ($text =~ /(?:foo|bar)(?:{[^}]*})?/) { }"#);
}

#[test]
fn qw_with_special_chars_in_words() {
    // qw with words containing allowed punctuation (shouldn't end early)
    assert_clean_parse(r#"my @words = qw(foo-bar baz_qux hello.world);"#);
}

#[test]
fn heredoc_tag_special_forms() {
    // Various heredoc tag forms
    assert_clean_parse(
        r#"my $text1 = <<END;
text
END
my $text2 = <<"END";
text
END
my $text3 = <<'END';
text
END
my $text4 = <<~END;
text
END
"#,
    );
}

#[test]
fn statement_missing_semicolon_between_statements() {
    // Missing semicolon between two statements (should not clean parse after fix)
    // But we test that the parser at least recovers sensibly
    let source = r#"my $x = qr/pattern/
my $y = 1;"#;
    let mut parser = Parser::new(source);
    let _ast = parser.parse();
    // Should have an inferred semicolon error, not crash
    assert!(!parser.get_errors().is_empty());
}

#[test]
fn quote_like_with_escaped_quotes_in_qq() {
    // qq with escaped quotes
    assert_clean_parse(r#"my $s = qq/hello \"world\"/"#);
}

#[test]
fn quote_like_regex_with_lookahead() {
    // Regex with lookahead assertion
    assert_clean_parse(r#"my $re = qr/foo(?=bar)/;"#);
}

#[test]
fn substitution_with_code_modifier() {
    // s/// with /e modifier (code execution)
    assert_clean_parse(r#"$x =~ s/(\d+)/$1 * 2/e;"#);
}

#[test]
fn qw_multiline() {
    // qw spanning multiple lines
    assert_clean_parse(
        r#"my @words = qw(
    one
    two
    three
);"#,
    );
}

#[test]
fn multiple_substitutions_chained() {
    // Multiple chained substitutions
    assert_clean_parse(r#"$x =~ s/a/b/ =~ s/c/d/;"#);
}
