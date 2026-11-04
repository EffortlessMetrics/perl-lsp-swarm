//! Issue #16296: parser-vs-perl divergence for qualified loop-control labels.
//!
//! Real Perl rejects a package-qualified name as a `next`/`last`/`redo` label
//! (`last FOO::BAR;` -> "Bareword found where operator expected"). The parser
//! used to swallow the qualified name as the label and accept the statement.
//! The fix mirrors the `DoWhileTrailingBlock` hard rejection (#15649): the
//! parse fails outright with `ParseError::QualifiedLoopControlLabel` instead
//! of accepting source `perl -c` refuses to compile.

use perl_parser_core::{ParseError, Parser};

fn parse_err(code: &str) -> ParseError {
    let mut parser = Parser::new(code);
    match parser.parse() {
        Ok(_) => panic!("expected outright parse failure for `{code}`"),
        Err(err) => err,
    }
}

fn parse_clean(code: &str) -> perl_parser_core::Node {
    let mut parser = Parser::new(code);
    let ast = match parser.parse() {
        Ok(ast) => ast,
        Err(err) => panic!("expected clean parse for `{code}`, got: {err:?}"),
    };
    let blocking: Vec<_> =
        parser.get_errors().iter().filter(|error| error.blocks_clean_parse()).collect();
    assert!(
        blocking.is_empty(),
        "expected clean parse for `{code}`, got blocking diagnostics: {blocking:#?}"
    );
    ast
}

fn first_kind(ast: &perl_parser_core::Node, name: &str) -> bool {
    if ast.kind.kind_name() == name {
        return true;
    }
    ast.children().iter().any(|child| first_kind(child, name))
}

/// All three loop-control operators reject a qualified label with the exact
/// `QualifiedLoopControlLabel` variant, bare and loop-nested. Oracle: each
/// form is a `perl -c` syntax error ("Bareword found where operator
/// expected" near the qualified name).
#[test]
fn qualified_loop_control_labels_reject_exact_variant() {
    for code in [
        "last FOO::BAR;",
        "next FOO::BAR;",
        "redo FOO::BAR;",
        "my $x = 0; LOOP: while ($x) { last FOO::BAR; }",
    ] {
        let err = parse_err(code);
        assert!(
            matches!(err, ParseError::QualifiedLoopControlLabel { .. }),
            "expected QualifiedLoopControlLabel for `{code}`, got: {err:?}"
        );
    }
}

/// The hard error anchors at the offending label token so diagnostics point
/// at the qualified name, not the statement start.
#[test]
fn qualified_label_error_anchors_at_label() {
    let code = "my $x = 0; while ($x) { last FOO::BAR; }";
    let err = parse_err(code);
    let anchor = err.location().expect("qualified label error carries a location");
    let label_at = code.find("FOO::BAR").expect("probe present in source");
    assert_eq!(anchor, label_at, "anchor must be the label token start");
}

/// In `my $x = { ... }` the braces are hash-or-block ambiguous: the parser
/// first tries the contents as an expression, then falls back to a block.
/// The qualified-label error must propagate as the dedicated terminal error
/// rather than being swallowed into a recovered block.
#[test]
fn qualified_label_survives_brace_fallback() {
    let code = "my $x = { last FOO::BAR; };";
    let err = parse_err(code);
    assert!(
        matches!(err, ParseError::QualifiedLoopControlLabel { .. }),
        "expected QualifiedLoopControlLabel for `{code}`, got: {err:?}"
    );
    let anchor = err.location().expect("qualified label error carries a location");
    let label_at = code.find("FOO::BAR").expect("probe present in source");
    assert_eq!(anchor, label_at, "anchor must be the label token start");
}

/// Ordinary word labels and phase-keyword labels still parse cleanly and
/// keep their label text — the guard only fires on `::`.
#[test]
fn unqualified_labels_still_parse_clean() {
    let ast = parse_clean("my $x = 0; OUTER: while ($x) { last OUTER; }");
    let sexp = ast.to_sexp();
    assert!(!sexp.contains("ERROR"), "clean parse expected, got: {sexp}");
    assert!(sexp.contains("OUTER"), "label text must be preserved: {sexp}");

    // Phase keywords remain valid loop-control labels (#2389) and must not
    // trip the qualified-name guard.
    let ast = parse_clean("my $x = 0; INIT: while ($x < 3) { $x++; last INIT if $x == 2; }");
    assert!(first_kind(&ast, "LoopControl"), "LoopControl expected in: {}", ast.to_sexp());
}
