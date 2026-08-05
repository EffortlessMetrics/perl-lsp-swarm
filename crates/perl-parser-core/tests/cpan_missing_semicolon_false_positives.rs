//! Shapes from Perl's own core modules that the terminator check must not
//! report (#5474, found in review on #5503).
//!
//! Each case below is a real file from an installed Perl 5.38.2 tree that
//! `perl -c` accepts and an earlier revision of the terminator check rejected.
//! They are the second demonstration that the in-repo corpus is not sufficient
//! evidence for this claim — the first was `__END__` — so they are pinned here
//! rather than left to a corpus sweep that happens not to contain the shape.
//!
//! Assertions are on `blocks_clean_parse()` rather than on a variant or a
//! message, so they survive any later change to how the diagnostic is
//! represented or worded. What matters is whether `--check` fails, not what it
//! is called internally.

mod cpan_test_helpers;

use cpan_test_helpers::assert_no_blocking_diagnostics;
use perl_parser_core::Parser;
use perl_parser_core::error::ParseError;

#[track_caller]
fn assert_blocking_diagnostic(source: &str) {
    let mut parser = Parser::new(source);
    let _ = parser.parse();
    assert!(
        parser.errors().iter().any(ParseError::blocks_clean_parse),
        "expected a blocking diagnostic for:\n{source}"
    );
}

/// `File/Copy.pm:175,179` — a continuation line beginning with a low-precedence
/// word operator. `copy(...)` and `or goto fail_inner;` are one statement
/// wrapped across a newline.
///
/// This is the case that falsifies the line-boundary rule on its own: crossing
/// a line boundary is necessary but not sufficient, because no Perl statement
/// begins with `or`.
#[test]
fn cpan_word_operator_continuation_line_stays_clean() {
    assert_no_blocking_diagnostics("copy($from, $to)\n    or goto fail_inner;\nprint \"ok\";\n");
    assert_no_blocking_diagnostics("open(my $fh, '<', $f)\n    || die \"no\";\nprint \"ok\";\n");
    assert_no_blocking_diagnostics("my $x = f()\n    and g();\nprint \"ok\";\n");
}

/// Arithmetic continuation across a line break. `perl` reads `1\n- 2;` as one
/// subtraction, not two statements, so a leading operator is a continuation
/// even where — as with unary minus — it could in principle begin a statement.
///
/// Expression parsing already consumes these today, so this passed before
/// `Minus` joined the continuation set (raised in review on #5503, from the
/// enumeration rather than from a measurement). Pinned so it stays true if
/// expression parsing ever stops short of the operator.
#[test]
fn arithmetic_continuation_line_stays_clean() {
    assert_no_blocking_diagnostics("my $x = 1\n- 2;\nprint \"x\";\n");
    assert_no_blocking_diagnostics("my $y = f()\n- 2;\nprint \"x\";\n");
    assert_no_blocking_diagnostics("my $z = $a\n- $b;\nprint \"x\";\n");
    assert_no_blocking_diagnostics("my $w = $a\n. $b;\nprint \"x\";\n");
}

/// `autodie/exception.pm:17` — a multi-line `use overload` import list. Import
/// lists are not fully modelled, so the statement stops early through no fault
/// of the source.
#[test]
fn cpan_multiline_use_overload_import_list_stays_clean() {
    assert_no_blocking_diagnostics(concat!(
        "use overload\n",
        "    q{\"\"} => \"stringify\",\n",
        "    # Overload smart-match only if we're using 5.10 or up\n",
        "    ($] >= 5.010 ? ('~~'  => \"matches\") : ()),\n",
        "    fallback => 1\n",
        ";\n",
        "print \"after\";\n",
    ));
    assert_no_blocking_diagnostics("no warnings qw(uninitialized);\nprint \"x\";\n");
}

/// `ExtUtils/MM_Any.pm:1779` — a heredoc introducer after an unknown bareword.
/// The lexer emits a left shift rather than a heredoc, so the body lines and
/// the `CODE` terminator are parsed as code and the terminator lands as a lone
/// bareword statement.
#[test]
fn cpan_heredoc_introducer_after_unknown_bareword_stays_clean() {
    assert_no_blocking_diagnostics(concat!(
        "my $subrclean .= $self->oneliner(_sprintf562 <<'CODE', $dir, $makefile);\n",
        "chdir '%1$s';\n",
        "CODE\n",
        "push @m, \"\\t- $subrclean\\n\";\n",
    ));
}

/// `Sys/Syslog.pm:930` — `__END__` after a final statement with no `;`.
#[test]
fn cpan_end_marker_after_unterminated_final_statement_stays_clean() {
    assert_no_blocking_diagnostics("\"Eighth Rule: read the documentation.\"\n\n__END__\n");
    assert_no_blocking_diagnostics("my $x = 1\n__DATA__\nsome data\n");
}

/// The narrowings above must not have removed the claim. Ordinary shift
/// expressions are still policed — the heredoc-introducer scan deliberately
/// does not match `<<` followed by whitespace or a digit — and the issue's own
/// reproduction still fails.
#[test]
fn the_narrowings_do_not_disarm_the_check() {
    assert_blocking_diagnostic("my $x = 1\nprint \"hi\";\n");
    assert_blocking_diagnostic("my $bits = 1 << 2\nprint \"hi\";\n");
}

/// A package declaration without a block is an ordinary statement and still
/// needs its terminator. Only `package NAME { ... }` is self-terminated by the
/// closing brace; keeping this distinction prevents the brace-terminated
/// predicate from masking the original missing-semicolon claim.
#[test]
fn package_declaration_without_block_still_requires_a_semicolon() {
    assert_blocking_diagnostic("package MissingTerminator\nprint \"after\";\n");
    assert_blocking_diagnostic("package Versioned 1.0\nprint \"after\";\n");
    assert_no_blocking_diagnostics("package Blocked { our $x = 1 }\nprint \"after\";\n");
}

/// `<<` inside a string literal, a comment, or a quote-like body is not a
/// heredoc introducer. Treating it as one suppressed the diagnostic for the
/// statement after it, so `--check` answered `ok` for source `perl -c` rejects
/// (found in review, #5503).
///
/// The counterpart to `cpan_heredoc_introducer_after_unknown_bareword_stays_clean`:
/// that one pins what the guard must suppress, this one pins what it must not.
#[test]
fn angle_brackets_inside_a_literal_are_not_a_heredoc_introducer() {
    assert_blocking_diagnostic("my $s = \"<<EOF\"\nprint $s;\n");
    assert_blocking_diagnostic("my $s = '<<EOF'\nprint $s;\n");
    assert_blocking_diagnostic("my $re = qr/<<EOF/\nprint $re;\n");
    assert_blocking_diagnostic("my $x = 1;   # <<EOF in a comment\nmy $y = 2\nprint $y;\n");
    // An escaped quote inside the literal must not end it early and re-expose
    // the `<<` to the scan.
    assert_blocking_diagnostic("my $s = \"a\\\"b <<EOF\"\nprint $s;\n");
}

/// Quote-like operators can contain `<<WORD` as ordinary payload. The source
/// still needs a semicolon before the following statement, so the scanner must
/// skip each quote body without using it to suppress the diagnostic.
#[test]
fn quote_like_bodies_do_not_hide_missing_terminators() {
    for source in [
        "my $re = qr /<<EOF/\nprint $re;\n",
        "my $re = qr{<<EOF}\nprint $re;\n",
        "my $re = m/<<EOF/\nprint $re;\n",
        "my $s = s/<<EOF/<<DONE/\nprint $s;\n",
        "my $s = q(<<EOF)\nprint $s;\n",
        "my @words = qw(<<EOF item)\nprint @words;\n",
    ] {
        assert_blocking_diagnostic(source);
    }
}

/// A real heredoc declaration requires its semicolon before the body. The
/// pending-heredoc queue must not make a following statement look valid.
#[test]
fn heredoc_declaration_without_terminator_is_reported() {
    assert_blocking_diagnostic("my $text = <<'EOT'\nline one\nEOT\nprint $text;\n");
}

/// The recovery exception for a leaked unknown-heredoc terminator must not
/// silence an ordinary bareword call followed by a keyword statement.
#[test]
fn bareword_call_followed_by_keyword_is_reported() {
    assert_blocking_diagnostic("foo\nprint \"after\";\n");
}

#[test]
fn quoted_shift_operand_is_not_a_heredoc_recovery_boundary() {
    assert_blocking_diagnostic("my $n = 1 << 'TAG'\nTAG\nprint \"after\";\n");
    assert_blocking_diagnostic("my $n = 1 <<'TAG'\nTAG\nprint \"after\";\n");
    assert_blocking_diagnostic("my $n = $value << 'TAG'\nTAG\nprint \"after\";\n");
    assert_blocking_diagnostic("my $n = foo << 'TAG'\nTAG\nprint \"after\";\n");
}
