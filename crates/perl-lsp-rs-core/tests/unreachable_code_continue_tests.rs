//! Parser-backed integration controls for PL406 loop and `continue` semantics.
//!
//! These tests exercise the public diagnostic entry point with source text.
//! They deliberately distinguish exact language transfers from call spellings
//! such as `croak`, `confess`, or `throw`, which require semantic authority
//! before they may be treated as non-returning.

use perl_lsp_rs_core::providers::diagnostics::Diagnostic;
use perl_lsp_rs_core::providers::diagnostics::unreachable_code::check_unreachable_code;
use perl_parser::Parser;

fn diagnostics(source: &str) -> Result<Vec<Diagnostic>, Box<dyn std::error::Error>> {
    let ast = Parser::new(source).parse()?;
    let mut diagnostics = Vec::new();
    check_unreachable_code(&ast, &mut diagnostics);
    Ok(diagnostics)
}

fn count_pl406(diagnostics: &[Diagnostic]) -> usize {
    diagnostics.iter().filter(|diagnostic| diagnostic.code.as_deref() == Some("PL406")).count()
}

fn assert_pl406_count(
    source: &str,
    expected: usize,
    context: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let diagnostics = diagnostics(source)?;
    assert_eq!(count_pl406(&diagnostics), expected, "{context}: {diagnostics:?}");
    Ok(())
}

#[test]
fn exact_transfers_in_continue_blocks_close_sibling_fallthrough()
-> Result<(), Box<dyn std::error::Error>> {
    for (source, context) in [
        (r#"while (1) { work(); } continue { die "err"; print "dead"; }"#, "die in continue"),
        (r#"while (1) { work(); } continue { exit 0; print "dead"; }"#, "exit in continue"),
        (
            r#"sub f { while (1) { work(); } continue { return; print "dead"; } }"#,
            "return in continue",
        ),
        (r#"while (1) { work(); } continue { last; print "dead"; }"#, "last in continue"),
        (r#"while (1) { work(); } continue { next; print "dead"; }"#, "next in continue"),
        (r#"while (1) { work(); } continue { redo; print "dead"; }"#, "redo in continue"),
    ] {
        assert_pl406_count(source, 1, context)?;
    }
    Ok(())
}

#[test]
fn for_and_foreach_continue_blocks_use_the_same_local_flow_contract()
-> Result<(), Box<dyn std::error::Error>> {
    for (source, context) in [
        (
            r#"for (my $i = 0; $i < 3; $i++) { work(); } continue { die "err"; print "dead"; }"#,
            "for continue",
        ),
        (
            r#"foreach my $x (1, 2) { work(); } continue { die "err"; print "dead"; }"#,
            "foreach continue",
        ),
    ] {
        assert_pl406_count(source, 1, context)?;
    }
    Ok(())
}

#[test]
fn call_spelling_alone_does_not_close_continue_fallthrough()
-> Result<(), Box<dyn std::error::Error>> {
    for (source, context) in [
        (r#"while (1) { work(); } continue { croak "err"; print "reachable"; }"#, "croak spelling"),
        (
            r#"while (1) { work(); } continue { Carp::confess "err"; print "reachable"; }"#,
            "qualified confess spelling",
        ),
        (
            r#"while (1) { work(); } continue { $object->throw(); print "reachable"; }"#,
            "method spelling",
        ),
    ] {
        assert_pl406_count(source, 0, context)?;
    }
    Ok(())
}

#[test]
fn all_later_continue_siblings_are_reported() -> Result<(), Box<dyn std::error::Error>> {
    assert_pl406_count(
        r#"while (1) { work(); } continue { die "err"; my $x = 1; my $y = 2; print "dead"; }"#,
        3,
        "multiple unreachable continue siblings",
    )
}

#[test]
fn ordinary_loop_body_detection_remains_local() -> Result<(), Box<dyn std::error::Error>> {
    assert_pl406_count(
        r#"while (1) { die "err"; print "dead"; } print "after loop";"#,
        1,
        "ordinary loop body",
    )
}

#[test]
fn conditional_loop_controls_keep_a_fallthrough_path() -> Result<(), Box<dyn std::error::Error>> {
    for (source, context) in [
        (
            r#"while (1) { work(); } continue { next if $skip; print "reachable"; }"#,
            "conditional next",
        ),
        (
            r#"while (1) { work(); } continue { last unless $ready; print "reachable"; }"#,
            "conditional last",
        ),
        (
            r#"while (1) { work(); } continue { redo while $retry; print "reachable"; }"#,
            "conditional redo",
        ),
    ] {
        assert_pl406_count(source, 0, context)?;
    }
    Ok(())
}

#[test]
fn loop_transfer_does_not_poison_code_after_the_loop() -> Result<(), Box<dyn std::error::Error>> {
    for (source, context) in [
        (r#"while ($ready) { next; } print "after";"#, "next loop exit"),
        (r#"while ($ready) { redo; } print "after";"#, "redo loop exit"),
        (r#"while ($ready) { last; } print "after";"#, "last loop exit"),
    ] {
        assert_pl406_count(source, 0, context)?;
    }
    Ok(())
}

#[test]
fn goto_forms_transfer_without_falling_through() -> Result<(), Box<dyn std::error::Error>> {
    for (source, expected, context) in [
        (r#"goto DONE; print "dead";"#, 1, "unresolved forward label"),
        (r#"goto DONE; print "dead"; DONE: print "alive";"#, 1, "resolved forward label"),
        (r#"goto &handler; print "dead";"#, 1, "goto sub"),
        (r#"while (1) { } continue { goto &handler; print "dead"; }"#, 1, "goto sub in continue"),
        (
            r#"while (1) { } continue { goto DONE; print "dead"; DONE: print "alive"; }"#,
            1,
            "forward label in continue",
        ),
    ] {
        assert_pl406_count(source, expected, context)?;
    }
    Ok(())
}
