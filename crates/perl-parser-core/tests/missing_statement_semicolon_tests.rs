//! #5474 — a statement terminator omitted between two statements must be
//! reported, and the places Perl permits omitting it must stay clean.
//!
//! Before this, the `;` was unconditionally optional at the end of every
//! statement, so `my $x = 1` followed by `print "hi";` parsed clean and
//! `perllsp --check` answered `ok` with exit 0 for source real `perl` rejects
//! with a syntax error — a false-exact result on the most common Perl syntax
//! error there is.

use perl_parser_core::Parser;
use perl_parser_core::error::{ParseError, RecoveryKind, RecoverySite};

fn diagnostics(source: &str) -> Vec<ParseError> {
    let mut parser = Parser::new(source);
    let _ = parser.parse();
    parser.errors().to_vec()
}

fn missing_semicolons(source: &str) -> usize {
    diagnostics(source)
        .iter()
        .filter(|error| {
            matches!(
                error,
                ParseError::Recovered {
                    site: RecoverySite::Statement,
                    kind: RecoveryKind::InferredSemicolon,
                    ..
                }
            )
        })
        .count()
}

/// The issue's reproduction: `perl -c` reports `syntax error … near "print"`.
#[test]
fn missing_semicolon_between_two_statements_is_reported() {
    assert_eq!(missing_semicolons("my $x = 1\nprint \"hi\";\n"), 1);
}

/// Same shape one level down. A block body is not a licence to omit the
/// terminator anywhere except immediately before its `}`.
#[test]
fn missing_semicolon_inside_a_block_is_reported() {
    assert_eq!(missing_semicolons("sub f {\n    my $y = 2\n    return $y;\n}\n1;\n"), 1);
}

/// The diagnostic must block a clean receipt — an advisory would leave
/// `--check` answering `ok`, which is the defect.
#[test]
fn the_diagnostic_blocks_a_clean_parse() {
    let reported = diagnostics("my $x = 1\nprint \"hi\";\n");
    assert!(
        reported.iter().any(ParseError::blocks_clean_parse),
        "missing-semicolon diagnostic must be blocking, got: {reported:?}"
    );
}

/// Perl permits omitting the final `;` of a file.
#[test]
fn final_statement_without_semicolon_is_clean() {
    assert_eq!(missing_semicolons("my $a = 1;\nmy $b = 2\n"), 0);
    assert_eq!(missing_semicolons("my $only = 1"), 0);
}

/// …and the final `;` before a closing brace.
#[test]
fn last_statement_in_a_block_without_semicolon_is_clean() {
    assert_eq!(missing_semicolons("sub f {\n    my $y = 2\n}\n1;\n"), 0);
    assert_eq!(missing_semicolons("if ($c) {\n    print \"x\"\n}\n1;\n"), 0);
    assert_eq!(missing_semicolons("{\n    my $bare = 1\n}\n1;\n"), 0);
}

/// `__END__`/`__DATA__` end the program text exactly like EOF, so the statement
/// before the marker may omit its terminator. This is the idiomatic module
/// ending — a bare `1` truth value followed by `__END__` and POD — and real
/// `perl -c` accepts it.
///
/// Missed by the first corpus sweep: the shape needs an unterminated statement
/// *immediately* before the marker, and every `__END__`/`__DATA__` file in
/// `test_corpus` has a `;` on the preceding line. Found in review (#5503).
#[test]
fn statement_before_a_data_marker_needs_no_terminator() {
    assert_eq!(missing_semicolons("my $x = 1\n__END__\ndocs\n"), 0);
    assert_eq!(missing_semicolons("my $x = 1\n__DATA__\nrow1\n"), 0);
    assert_eq!(
        missing_semicolons("package Foo;\nsub f { 1 }\n1\n__END__\n\n=head1 NAME\n\n=cut\n"),
        0
    );
    // The control: the marker is not a blanket amnesty for the file.
    assert_eq!(missing_semicolons("my $x = 1\nprint \"hi\";\n__END__\ndocs\n"), 1);
}

/// A brace-terminated compound statement never needs one, and the statement
/// after it does not inherit a missing terminator from it.
#[test]
fn compound_statements_need_no_terminator() {
    let sources = [
        "if ($c) { print \"a\"; }\nprint \"b\";\n",
        "while ($c) { $i++; }\nprint \"b\";\n",
        "for my $i (1 .. 3) { print $i; }\nprint \"b\";\n",
        "sub f { return 1; }\nprint \"b\";\n",
        "{ my $x = 1; }\nprint \"b\";\n",
        "BEGIN { $x = 1; }\nprint \"b\";\n",
    ];
    for source in sources {
        assert_eq!(missing_semicolons(source), 0, "compound statement flagged in:\n{source}");
    }
}

/// Perl 5.38 `class`/`method` bodies are brace-terminated declarations too.
/// `examples/perl/modern.pl` is accepted by real `perl -c`; before `Class` and
/// `Method` were added to the compound set, every `method` in it was reported
/// as a missing terminator on the statement before it.
#[test]
fn class_and_method_declarations_need_no_terminator() {
    let source = "class Point {\n    field $x :param;\n\n    method x { $x }\n\n    method to_string {\n        \"($x)\"\n    }\n}\n\nmy $p = Point->new(x => 1);\n";
    assert_eq!(missing_semicolons(source), 0);
}

/// `package NAME;` is an ordinary statement and needs its terminator; only
/// `package NAME { … }` ends itself. Both forms are barred from postfix
/// modifiers, which is a different question — sharing one predicate for the two
/// left `package Foo` followed by another statement reported as `ok` while
/// `perl -c` rejects it (found in review, #5503).
#[test]
fn only_the_block_form_of_package_ends_itself() {
    assert_eq!(missing_semicolons("package Foo\nprint \"hi\";\n"), 1);
    assert_eq!(missing_semicolons("package Foo 1.0\nprint \"hi\";\n"), 1);
    assert_eq!(missing_semicolons("package Foo;\nprint \"hi\";\n"), 0);
    assert_eq!(missing_semicolons("package Foo {\n    sub f { 1 }\n}\nprint \"hi\";\n"), 0);
}

/// Statement modifiers bind to the statement they follow; the terminator check
/// runs after them, not instead of them.
#[test]
fn statement_modifiers_are_not_mistaken_for_a_new_statement() {
    assert_eq!(missing_semicolons("print \"x\" if $y;\nprint \"z\";\n"), 0);
    assert_eq!(missing_semicolons("print \"x\" for 1 .. 3;\n"), 0);
}

/// A statement that stops mid-line means the parser gave up early on a
/// construct it does not support — not that the user forgot a `;`. Reporting
/// those would reject valid Perl to describe our own gap, so the check stays
/// inside the shape `perl` itself reports: two statements on separate lines.
///
/// Both inputs are accepted by real `perl -c` and both stop the statement
/// short of the end of its line today.
#[test]
fn parser_gaps_that_stop_mid_line_are_not_reported_as_missing_semicolons() {
    // `no MODULE qw(...)` — the pragma's qw list is not consumed.
    assert_eq!(missing_semicolons("use strict;\nno warnings qw(once redefine);\n1;\n"), 0);
    // `x=` repetition assignment.
    assert_eq!(missing_semicolons("my $s = \"x\";\n$s x= 2;\n1;\n"), 0);
}

/// Trailing content after the last statement must not be blamed on it.
#[test]
fn empty_and_comment_only_sources_are_clean() {
    assert_eq!(missing_semicolons(""), 0);
    assert_eq!(missing_semicolons("# just a comment\n"), 0);
    assert_eq!(missing_semicolons("my $x = 1;\n# trailing comment\n"), 0);
    assert_eq!(missing_semicolons("my $x = 1;\n\n=pod\n\ndocs\n\n=cut\n"), 0);
}
