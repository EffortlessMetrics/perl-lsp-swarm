//! Residue contract for #7291 beyond scanned bytes (#13786).
//!
//! Pins attachment bounds/identity, cooperative cancellation at heredoc stages,
//! same-parse post-exhaustion blast radius, and parser-level boundary variants
//! for the single enforced heredoc dimension (`max_heredoc_scan_bytes`).

use perl_ast::NodeKind;
use perl_parser_core::error::ParseBudget;
use perl_parser_core::{ParseError, ParseOutput, ParseStopCause, Parser, ParserConfigIdentity};
use perl_tdd_support::must;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

mod cpan_test_helpers;

const ALPHA_STMT: &str = "my $a = <<ALPHA;\nalpha-only\nALPHA\n";
const BETA_STMT: &str = "my $b = <<BETA;\nbeta-only\nBETA\n";
const GAMMA_STMT: &str = "my $c = <<GAMMA;\ngamma-only\nGAMMA\n";

fn parse_with_budget(source: &str, budget: ParseBudget) -> ParseOutput {
    let config = ParserConfigIdentity::production_default().with_budget(budget);
    let mut parser = Parser::with_production_config(source, config);
    parser.parse_with_recovery()
}

fn heredoc_scan_budget(max_heredoc_scan_bytes: usize) -> ParseBudget {
    let mut budget = ParseBudget::unlimited();
    budget.max_heredoc_scan_bytes = max_heredoc_scan_bytes;
    budget
}

fn heredoc_contents(output: &ParseOutput) -> Vec<String> {
    fn walk(node: &perl_ast::Node, found: &mut Vec<String>) {
        if let NodeKind::Heredoc { content, .. } = &node.kind {
            found.push(content.clone());
        }
        node.for_each_child(|child| walk(child, found));
    }
    let mut found = Vec::new();
    walk(&output.ast, &mut found);
    found
}

fn budget_diagnostic_count(output: &ParseOutput) -> usize {
    output
        .diagnostics
        .iter()
        .filter(|error| matches!(error, ParseError::HeredocBudgetExhausted { .. }))
        .count()
}

// ---------------------------------------------------------------------------
// Attachment identity across exhaustion (#7289)
// ---------------------------------------------------------------------------

#[test]
fn exhaustion_keeps_each_body_on_its_own_declaration() {
    let source = format!("{ALPHA_STMT}{BETA_STMT}");
    let admitted_charge =
        parse_with_budget(ALPHA_STMT, ParseBudget::unlimited()).budget_usage.heredoc_scan_bytes;

    let output = parse_with_budget(&source, heredoc_scan_budget(admitted_charge));

    assert!(
        matches!(output.stop_cause(), Some(ParseStopCause::HeredocBudgetExhausted { .. })),
        "second collection must be refused"
    );
    assert_eq!(
        heredoc_contents(&output),
        vec!["alpha-only".to_string(), String::new()],
        "only the refused placeholder stays unresolved"
    );
    let joined = heredoc_contents(&output).join("|");
    assert!(
        !joined.contains("beta-only"),
        "refused body bytes must not attach to the first declaration"
    );
    assert!(
        !joined.contains("alpha-only\nbeta-only"),
        "bodies must not be merged across declarations"
    );
}

// ---------------------------------------------------------------------------
// Same-parse post-exhaustion blast radius
// ---------------------------------------------------------------------------

#[test]
fn post_exhaustion_refuses_every_later_heredoc_with_one_diagnostic() {
    let source = format!("{ALPHA_STMT}{BETA_STMT}{GAMMA_STMT}");
    let admitted_charge =
        parse_with_budget(ALPHA_STMT, ParseBudget::unlimited()).budget_usage.heredoc_scan_bytes;

    let output = parse_with_budget(&source, heredoc_scan_budget(admitted_charge));

    assert_eq!(
        heredoc_contents(&output),
        vec!["alpha-only".to_string(), String::new(), String::new()],
        "every later heredoc in the same parse stays unresolved after exhaustion"
    );
    assert_eq!(
        budget_diagnostic_count(&output),
        1,
        "exhaustion is one event per parse — the terminal, not the count, is the signal"
    );
}

// ---------------------------------------------------------------------------
// Cancellation at heredoc stages
// ---------------------------------------------------------------------------

#[test]
fn cancellation_at_heredoc_queue_is_distinct_from_budget_exhaustion() {
    let flag = Arc::new(AtomicBool::new(true));
    let mut parser = Parser::new_with_cancellation(ALPHA_STMT, flag);
    let output = parser.parse_with_recovery();

    assert_eq!(output.stop_cause(), Some(ParseStopCause::Cancelled));
    assert!(
        output.diagnostics.iter().any(|error| matches!(error, ParseError::Cancelled)),
        "cancellation must be observable in diagnostics"
    );
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|error| matches!(error, ParseError::HeredocBudgetExhausted { .. })),
        "cancellation must not be reported as heredoc budget exhaustion"
    );
}

#[test]
fn cancellation_during_heredoc_scan_stops_before_attachment() {
    let body = "x\n".repeat(400_000);
    let source = format!("my $x = <<EOF;\n{body}EOF\n");
    let flag = Arc::new(AtomicBool::new(false));
    let flag_for_parse = Arc::clone(&flag);
    let parse_handle = thread::spawn(move || {
        let mut parser = Parser::new_with_cancellation(&source, flag_for_parse);
        parser.parse_with_recovery()
    });
    thread::sleep(Duration::from_millis(1));
    flag.store(true, Ordering::Relaxed);
    let output = must(parse_handle.join());

    assert_eq!(output.stop_cause(), Some(ParseStopCause::Cancelled));
    assert!(
        heredoc_contents(&output).iter().all(String::is_empty),
        "cancelled scan must not attach partial bodies as complete content"
    );
}

#[test]
fn cancellation_at_attachment_stage_is_observable() {
    let padding = "line\n".repeat(200_000);
    let source = format!("my $a = <<ALPHA;\n{padding}ALPHA\n{BETA_STMT}");
    let flag = Arc::new(AtomicBool::new(false));
    let flag_for_parse = Arc::clone(&flag);
    let parse_handle = thread::spawn(move || {
        let mut parser = Parser::new_with_cancellation(&source, flag_for_parse);
        parser.parse_with_recovery()
    });
    thread::sleep(Duration::from_millis(1));
    flag.store(true, Ordering::Relaxed);
    let output = must(parse_handle.join());

    assert_eq!(output.stop_cause(), Some(ParseStopCause::Cancelled));
    assert!(
        output.diagnostics.iter().any(|error| matches!(error, ParseError::Cancelled)),
        "attachment-stage cancellation must surface the typed terminal"
    );
}

// ---------------------------------------------------------------------------
// Boundary matrix variants on the enforced scan dimension
// ---------------------------------------------------------------------------

#[test]
fn scan_budget_threshold_is_invariant_under_crlf_tilde_and_unicode_fixtures() {
    let fixtures = [
        ("crlf", "my $x = <<EOF;\r\nline\r\nEOF\r\n".to_string(), "line".to_string()),
        ("tilde", "my $x = <<~EOF;\n  body\n  EOF\n".to_string(), "body".to_string()),
        ("unicode_body", "my $x = <<EOF;\n日本語本文\nEOF\n".to_string(), "日本語本文".to_string()),
    ];

    for (name, source, expected_body) in fixtures {
        let measured = parse_with_budget(&source, ParseBudget::unlimited());
        assert_eq!(
            heredoc_contents(&measured),
            vec![expected_body.clone()],
            "{name}: unlimited parse must attach the fixture body"
        );
        let charge = measured.budget_usage.heredoc_scan_bytes;
        assert!(charge > 0, "{name}: fixture must charge scan bytes");

        let at = parse_with_budget(&source, heredoc_scan_budget(charge));
        assert_eq!(at.stop_cause(), None, "{name}: landing on the limit must not terminate early");
        assert_eq!(
            heredoc_contents(&at),
            vec![expected_body],
            "{name}: at-threshold parse must keep the body"
        );

        let refused = parse_with_budget(&source, heredoc_scan_budget(0));
        assert!(
            matches!(refused.stop_cause(), Some(ParseStopCause::HeredocBudgetExhausted { .. })),
            "{name}: zero budget must refuse collection"
        );
        assert_eq!(
            heredoc_contents(&refused),
            vec![String::new()],
            "{name}: refused collection keeps an unresolved placeholder"
        );
    }
}

#[test]
fn budget_usage_exposes_only_the_scan_dimension_for_heredoc_work() {
    let output = parse_with_budget(&format!("{ALPHA_STMT}{BETA_STMT}"), ParseBudget::unlimited());
    assert!(
        output.budget_usage.heredoc_scan_bytes > 0,
        "heredoc work must be charged through heredoc_scan_bytes for #7085 / #7112 consumers"
    );
}

#[test]
fn fixtures_are_valid_perl_under_an_ordinary_parse() {
    let body = "x\n".repeat(200);
    for source in [
        format!("{ALPHA_STMT}{BETA_STMT}{GAMMA_STMT}"),
        format!("my $x = <<EOF;\n{body}EOF\n"),
        "my $x = <<EOF;\r\nline\r\nEOF\r\n".to_string(),
        "my $x = <<~EOF;\n  body\n  EOF\n".to_string(),
        "my $x = <<EOF;\n日本語本文\nEOF\n".to_string(),
    ] {
        cpan_test_helpers::assert_clean_parse(&source);
    }
}
