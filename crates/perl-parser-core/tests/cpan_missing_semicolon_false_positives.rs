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
