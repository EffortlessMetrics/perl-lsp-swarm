//! Proof for #5474: `--check` reported `ok` for a file real Perl rejects.
//!
//! A statement followed by another statement with no intervening `;` is a syntax
//! error in Perl — and the most common one there is. The parser treated the
//! semicolon as unconditionally optional, so it recorded nothing and every
//! consumer of the parse (the `--check` CLI and the LSP diagnostics path both
//! read the same `parser.errors()`) reported a clean file.
//!
//! Perl permits omitting the separator in exactly two places: before the `}` that
//! closes the enclosing block, and at end of input. The negative cases below are
//! the ones that must keep parsing clean; without them a fix for the false pass
//! is free to reject valid code instead, which is the worse failure for a tool
//! whose exit code gates a release.

use perl_parser::Parser;
use perl_tdd_support::must;

/// Parse `code` and return the recorded missing-separator diagnostics.
fn missing_separator_errors(code: &str) -> Vec<String> {
    let mut parser = Parser::new(code);
    let _ast = must(parser.parse());
    parser
        .errors()
        .iter()
        .map(|error| error.to_string())
        .filter(|message| message.contains("Missing semicolon"))
        .collect()
}

fn assert_reported(code: &str, context: &str) {
    let errors = missing_separator_errors(code);
    assert_eq!(
        errors.len(),
        1,
        "{context}: expected exactly one missing-semicolon diagnostic for:\n{code}\ngot: {errors:?}"
    );
}

fn assert_clean(code: &str, context: &str) {
    let errors = missing_separator_errors(code);
    assert!(
        errors.is_empty(),
        "{context}: valid Perl must not be reported as missing a semicolon:\n{code}\ngot: {errors:?}"
    );
}

// ── the reported false pass ───────────────────────────────────────────

/// The exact reproduction from #5474. `perl -c` rejects this with
/// `syntax error ... near "print"`; `--check` reported `ok` with exit 0.
#[test]
fn missing_semicolon_between_two_statements_is_reported() {
    assert_reported("my $x = 1\nprint \"hi\";\n", "top-level statement sequence");
}

/// The same defect inside a block. Found while reproducing #5474 — the issue
/// reports the top-level case only, but the block case false-passed identically.
#[test]
fn missing_semicolon_inside_a_block_is_reported() {
    assert_reported("sub f {\n  my $y = 2\n  return $y;\n}\n", "inside a sub body");
}

// ── the two positions Perl genuinely permits ──────────────────────────

/// Perl allows the final statement of a file to omit its semicolon.
#[test]
fn final_statement_without_semicolon_is_permitted() {
    assert_clean("my $x = 1", "last statement at end of input");
}

/// Perl allows the final statement of a block to omit its semicolon.
#[test]
fn final_statement_in_a_block_without_semicolon_is_permitted() {
    assert_clean("sub f {\n  my $y = 2\n}\n", "last statement before a closing brace");
    assert_clean("if ($x) {\n  print 1\n}\n", "last statement in an if body");
    assert_clean("while (1) {\n  last\n}\n", "last statement in a loop body");
}

// ── constructs that are terminated by their own closing brace ─────────

/// Compound statements never need a trailing semicolon, so a following statement
/// on the next line is not a missing separator.
#[test]
fn block_terminated_statements_do_not_need_a_semicolon() {
    assert_clean("if ($x) { print 1; }\nprint 2;\n", "if block followed by a statement");
    assert_clean("sub f { return 1; }\nsub g { return 2; }\n", "consecutive named subs");
    assert_clean("while (1) { last; }\nprint 2;\n", "while block followed by a statement");
    assert_clean("{ print 1; }\nprint 2;\n", "bare block followed by a statement");
}

/// `class` and `method` (Perl 5.38) are block-terminated exactly like `sub`.
///
/// Regression guard: these were missing from the compound-statement set, so
/// consecutive `method` declarations — valid, and present in
/// `examples/perl/modern.pl` — were reported as missing semicolons.
#[test]
fn consecutive_methods_in_a_class_body_are_permitted() {
    assert_clean(
        "use v5.38;\nclass Point {\n    field $x;\n    method y { $x }\n    method z { $x }\n}\n",
        "consecutive methods in a class body",
    );
}

// ── statement modifiers keep their statement intact ───────────────────

/// A postfix modifier is part of the statement it follows, not a new one.
#[test]
fn statement_modifiers_are_not_missing_separators() {
    assert_clean("print \"x\" if $y;\nprint 2;\n", "postfix if");
    assert_clean("print \"x\" for @list;\nprint 2;\n", "postfix for");
    assert_clean("my $z = 1 unless $q;\n", "postfix unless as the final statement");
}

// ── the documented bound ──────────────────────────────────────────────

/// A construct the parser cannot yet absorb ends the statement early at the
/// token it choked on, which is indistinguishable from a missing separator by
/// token kind alone. Requiring the next statement to begin on a later line keeps
/// those gaps from being misreported as missing semicolons.
///
/// Both inputs below are valid Perl that `perl -c` accepts. The parser does not
/// yet handle either construct; when it does, they must still parse clean.
#[test]
fn known_parser_gaps_are_not_misreported_as_missing_semicolons() {
    assert_clean("my $s = \"x\";\n$s x= 2;\nprint 1;\n", "x= repetition-assignment operator");
    assert_clean("no warnings qw(once redefine);\nprint 1;\n", "no MODULE qw(...) import list");
}

/// The cost of the line-break rule, pinned so it is a known bound rather than an
/// unnoticed hole: a separator omitted mid-line is not reported.
///
/// If a later change starts reporting this, that is an improvement — update this
/// test rather than treating it as a regression.
#[test]
fn same_line_omission_is_a_documented_non_goal() {
    assert_clean(
        "my $x = 1 print \"hi\";\n",
        "same-line omission is outside the claim (see #5474 non-goals)",
    );
}
