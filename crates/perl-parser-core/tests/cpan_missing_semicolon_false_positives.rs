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

use perl_parser_core::Parser;
use perl_parser_core::error::ParseError;

#[track_caller]
fn assert_no_blocking_diagnostics(source: &str) {
    let mut parser = Parser::new(source);
    let _ = parser.parse();
    let blocking: Vec<_> =
        parser.errors().iter().filter(|error| error.blocks_clean_parse()).collect();
    assert!(blocking.is_empty(), "expected no blocking diagnostics, got: {blocking:#?}\n{source}");
}

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
    assert_blocking_diagnostic("my $x = 1;   # <<EOF in a comment\nmy $y = 2\nprint $y;\n");
    // An escaped quote inside the literal must not end it early and re-expose
    // the `<<` to the scan.
    assert_blocking_diagnostic("my $s = \"a\\\"b <<EOF\"\nprint $s;\n");
}

/// A lone bareword is a real statement in Perl — `use constant foo => 1;` makes
/// `foo` a call — so exempting the shape wholesale hid a genuine missing
/// terminator (found in review, #5503). The exemption now requires the
/// identifier to name a heredoc delimiter introduced above it.
#[test]
fn a_bare_identifier_is_only_exempt_when_it_terminates_a_heredoc() {
    // No `<<foo` above it, so this is a real statement missing its `;`.
    assert_blocking_diagnostic("use constant foo => 1;\nfoo\nprint \"hi\";\n");
    assert_blocking_diagnostic("use constant foo => 1;\nfoo\n$x = 1;\n");

    // With the introducer above it, the same bareword is a leaked terminator.
    assert_no_blocking_diagnostics(concat!(
        "my $t = $self->oneliner(_sprintf562 <<'CODE', $dir);\n",
        "chdir '%1$s';\n",
        "CODE\n",
        "push @m, \"x\";\n",
    ));
}

/// Known remaining false negative, pinned so it is not mistaken for coverage:
/// `foo\nmy $x = 1;` still reports `ok`. The bareword swallows the following
/// `my` declaration as a list-operator argument, so no leftover token ever
/// reaches the terminator seam. That is expression parsing, not this check —
/// removing the exemption entirely does not fix it.
#[test]
fn bareword_followed_by_my_is_a_known_expression_parsing_gap() {
    let source = "use constant foo => 1;\nfoo\nmy $x = 1;\n";
    let mut parser = Parser::new(source);
    let _ = parser.parse();
    assert!(
        !parser.errors().iter().any(ParseError::blocks_clean_parse),
        "if this starts failing the expression-parsing gap was fixed; delete this test"
    );
}

/// The literal-context bug, on the *other* scanner. `is_leaked_heredoc_terminator`
/// repeated the naive byte scan two commits after it was fixed in its sibling,
/// so a `<<FOO` inside a string literal exempted a later bare `FOO` (#5503).
///
/// Both callers now share one scanner, so this and
/// `angle_brackets_inside_a_literal_are_not_a_heredoc_introducer` pin the same
/// property from the two directions that reach it.
#[test]
fn a_heredoc_name_inside_a_literal_does_not_exempt_a_later_bareword() {
    assert_blocking_diagnostic("my $s = \"<<FOO\";\nprint $s;\nFOO\nprint \"hi\";\n");
    assert_blocking_diagnostic("my $s = '<<FOO';\nprint $s;\nFOO\nprint \"hi\";\n");
    assert_blocking_diagnostic("# <<FOO in a comment\nmy $x = 1;\nFOO\nprint \"hi\";\n");
}

/// The delimiter must match in full. `<<LONGNAME` above must not exempt a bare
/// `LONG` below — a prefix match is a false negative in the direction that
/// matters for a check gating a release (#5503).
#[test]
fn a_prefix_of_a_heredoc_delimiter_is_not_a_terminator() {
    assert_blocking_diagnostic("my $t = f(<<LONGNAME);\nbody\nLONGNAME\nLONG\nprint \"hi\";\n");
    // The full delimiter still is one, in both bare and quoted forms.
    assert_no_blocking_diagnostics("my $t = f(<<LONGNAME);\nbody\nLONGNAME\nprint \"hi\";\n");
    assert_no_blocking_diagnostics("my $t = f(<<'LONGNAME');\nbody\nLONGNAME\nprint \"hi\";\n");
}
