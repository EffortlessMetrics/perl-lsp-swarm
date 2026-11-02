#![cfg(feature = "incremental")]
//! Fresh vs incremental `ParseSnapshot` parity across edit transitions (#7008).
//!
//! Expected terminal class is fixture-named and checked against independent
//! structural evidence (`recovered_count`, `terminated_early`, blocking
//! diagnostics, `ParseError::Cancelled`). Production `ParseSnapshot::disposition`
//! is then compared to that named class; the test does not ask
//! `ParseSnapshot::from_output` what the expected class should be.
//!
//! Cancellation, budget exhaustion, and recovered→catastrophic remain named
//! unimplemented boundaries until their production injection owners land.

use perl_ast::{Node, NodeKind};
use perl_parser::incremental::{
    Edit, IncrementalState, MAX_EDIT_SIZE, ParseGeneration, ParseSnapshot, ParseSnapshotStrategy,
    ParseTerminalDisposition, apply_edits,
};
use perl_parser::{ParseError, ParseOutput, Parser};

type TestResult = Result<(), String>;

#[derive(Debug, Clone, Copy)]
enum Needle {
    Replace { old: &'static str, new: &'static str },
    InsertBefore { needle: &'static str, text: &'static str },
    InsertBeforeNth { needle: &'static str, index: usize, text: &'static str },
    Delete { text: &'static str },
    Prefix { text: &'static str },
}

#[derive(Debug, Clone, Copy)]
enum Step {
    Apply(&'static [Needle]),
    InvalidOverlap { start: usize, old_end: usize },
}

#[derive(Debug, Clone, Copy)]
struct Transition {
    name: &'static str,
    initial: &'static str,
    expected_initial: ParseTerminalDisposition,
    steps: &'static [Step],
    expected_final: ParseTerminalDisposition,
}

fn splice(source: &str, start: usize, old_end: usize, new_text: &str) -> Result<Edit, String> {
    if start > old_end || old_end > source.len() {
        return Err(format!(
            "edit range {start}..{old_end} is invalid for source length {}",
            source.len()
        ));
    }
    if !source.is_char_boundary(start) || !source.is_char_boundary(old_end) {
        return Err(format!("edit range {start}..{old_end} is not on a UTF-8 boundary"));
    }
    Ok(Edit {
        start_byte: start,
        old_end_byte: old_end,
        new_end_byte: start + new_text.len(),
        new_text: new_text.to_string(),
    })
}

fn needle_to_edit(source: &str, needle: Needle) -> Result<Edit, String> {
    match needle {
        Needle::Replace { old, new } => {
            let start =
                source.find(old).ok_or_else(|| format!("missing replace needle {old:?}"))?;
            splice(source, start, start + old.len(), new)
        }
        Needle::InsertBefore { needle, text } => {
            let start =
                source.find(needle).ok_or_else(|| format!("missing insert needle {needle:?}"))?;
            splice(source, start, start, text)
        }
        Needle::InsertBeforeNth { needle, index, text } => {
            let start = source
                .match_indices(needle)
                .nth(index)
                .map(|(start, _)| start)
                .ok_or_else(|| format!("missing insert needle {needle:?} at index {index}"))?;
            splice(source, start, start, text)
        }
        Needle::Delete { text } => {
            let start =
                source.find(text).ok_or_else(|| format!("missing delete needle {text:?}"))?;
            splice(source, start, start + text.len(), "")
        }
        Needle::Prefix { text } => splice(source, 0, 0, text),
    }
}

fn apply_reference_edits(source: &str, edits: &[Edit]) -> Result<String, String> {
    let mut sorted = edits.to_vec();
    sorted.sort_by_key(|edit| edit.start_byte);
    sorted.reverse();
    let mut result = source.to_string();
    for edit in sorted {
        if edit.start_byte > edit.old_end_byte || edit.old_end_byte > result.len() {
            return Err(format!(
                "reference edit {}..{} is invalid for length {}",
                edit.start_byte,
                edit.old_end_byte,
                result.len()
            ));
        }
        result.replace_range(edit.start_byte..edit.old_end_byte, &edit.new_text);
    }
    Ok(result)
}

fn fresh_output(source: &str) -> ParseOutput {
    Parser::new(source).parse_with_recovery()
}

fn fresh_snapshot(source: &str) -> ParseSnapshot {
    ParseSnapshot::from_output(
        source,
        ParseGeneration::INITIAL,
        ParseSnapshotStrategy::Fresh,
        fresh_output(source),
    )
}

/// Independent structural class. This is not a copy of production
/// `classify_output`; it only names the evidence the fixture already claimed.
fn structural_matches_named(output: &ParseOutput, named: ParseTerminalDisposition) -> bool {
    let cancelled = output.diagnostics.iter().any(|error| matches!(error, ParseError::Cancelled));
    let blocking = output.diagnostics.iter().any(ParseError::blocks_clean_parse);
    match named {
        ParseTerminalDisposition::Clean => {
            output.recovered_count == 0 && !output.terminated_early() && !cancelled && !blocking
        }
        ParseTerminalDisposition::Recovered => {
            output.recovered_count > 0 && !output.terminated_early() && !cancelled
        }
        ParseTerminalDisposition::Catastrophic => {
            output.recovered_count == 0 && blocking && !output.terminated_early() && !cancelled
        }
        ParseTerminalDisposition::Cancelled => cancelled && !output.terminated_early(),
        ParseTerminalDisposition::BudgetExhausted => output.terminated_early() && !cancelled,
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecoverySite {
    kind: &'static str,
    start: usize,
    end: usize,
}

fn recovery_topology(root: &Node) -> Vec<RecoverySite> {
    let mut sites = Vec::new();
    collect_recovery_sites(root, &mut sites);
    sites
}

fn collect_recovery_sites(node: &Node, sites: &mut Vec<RecoverySite>) {
    let kind = match &node.kind {
        NodeKind::Error { .. } => Some("Error"),
        NodeKind::MissingExpression => Some("MissingExpression"),
        NodeKind::MissingStatement => Some("MissingStatement"),
        NodeKind::MissingIdentifier => Some("MissingIdentifier"),
        NodeKind::MissingBlock => Some("MissingBlock"),
        _ => None,
    };
    if let Some(kind) = kind {
        sites.push(RecoverySite { kind, start: node.location.start, end: node.location.end });
    }
    node.for_each_child(|child| collect_recovery_sites(child, sites));
}

fn diagnostic_plane(
    output: &ParseOutput,
    source: &str,
) -> Vec<(String, Option<usize>, Option<usize>)> {
    output
        .diagnostics
        .iter()
        .map(|error| {
            let identity = match error {
                ParseError::Recovered { site, kind, location } => {
                    format!("Recovered:{site:?}:{kind:?}:{location}")
                }
                ParseError::UnexpectedToken { expected, found, location } => {
                    format!("UnexpectedToken:{expected}:{found}:{location}")
                }
                ParseError::SyntaxError { message: _, location } => {
                    format!("SyntaxError:{location}")
                }
                ParseError::Advisory { location, .. } => format!("Advisory:{location}"),
                other => format!("{other:?}"),
            };
            (identity, error.location(), error.diagnostic_anchor().resolve(source).exact_offset())
        })
        .collect()
}

trait ExactOffset {
    fn exact_offset(self) -> Option<usize>;
}

impl ExactOffset for perl_parser::ResolvedParseDiagnosticAnchor {
    fn exact_offset(self) -> Option<usize> {
        match self {
            perl_parser::ResolvedParseDiagnosticAnchor::Exact(offset)
            | perl_parser::ResolvedParseDiagnosticAnchor::EndOfInput(offset) => Some(offset),
            _ => None,
        }
    }
}

fn first_divergence(
    case: &str,
    source: &str,
    named: Option<ParseTerminalDisposition>,
    incremental: &ParseSnapshot,
    returned: &ParseSnapshot,
) -> Result<(), String> {
    let fresh = fresh_snapshot(source);
    let fresh_out = fresh.parse_output();
    let incr_out = incremental.parse_output();
    let returned_out = returned.parse_output();
    let named_for_receipt = named.unwrap_or(incremental.disposition());

    incremental.validate_against(source).map_err(|error| {
        receipt(case, source, named_for_receipt, "snapshot-validate", format!("{error}"))
    })?;
    returned.validate_against(source).map_err(|error| {
        receipt(case, source, named_for_receipt, "returned-validate", format!("{error}"))
    })?;

    if let Some(named) = named {
        if !structural_matches_named(incr_out, named) {
            return Err(receipt(
                case,
                source,
                named,
                "named-disposition-vs-structure",
                format!(
                    "incremental recovered_count={}, terminated_early={}, diagnostics={:?}",
                    incr_out.recovered_count,
                    incr_out.terminated_early(),
                    incr_out.diagnostics
                ),
            ));
        }
        if incremental.disposition() != named {
            return Err(receipt(
                case,
                source,
                named,
                "named-disposition-vs-production",
                format!("{:?}", incremental.disposition()),
            ));
        }
        if fresh.disposition() != named {
            return Err(receipt(
                case,
                source,
                named,
                "fresh-disposition",
                format!("{:?}", fresh.disposition()),
            ));
        }
    }
    if incremental.disposition() != fresh.disposition() {
        return Err(receipt(
            case,
            source,
            named_for_receipt,
            "disposition-parity",
            format!("{:?} vs {:?}", incremental.disposition(), fresh.disposition()),
        ));
    }
    if incr_out.stop_cause() != fresh_out.stop_cause() {
        return Err(receipt(
            case,
            source,
            named_for_receipt,
            "stop-cause",
            format!("{:?} vs {:?}", incr_out.stop_cause(), fresh_out.stop_cause()),
        ));
    }
    if incr_out.recovered_count != fresh_out.recovered_count {
        return Err(receipt(
            case,
            source,
            named_for_receipt,
            "recovered-count",
            format!("{} vs {}", incr_out.recovered_count, fresh_out.recovered_count),
        ));
    }
    if diagnostic_plane(incr_out, source) != diagnostic_plane(fresh_out, source) {
        return Err(receipt(
            case,
            source,
            named_for_receipt,
            "diagnostic-order-and-ranges",
            format!(
                "{:?} vs {:?}",
                diagnostic_plane(incr_out, source),
                diagnostic_plane(fresh_out, source)
            ),
        ));
    }
    if incr_out.diagnostics != fresh_out.diagnostics {
        return Err(receipt(
            case,
            source,
            named_for_receipt,
            "typed-diagnostics",
            format!("{:?} vs {:?}", incr_out.diagnostics, fresh_out.diagnostics),
        ));
    }
    if recovery_topology(&incr_out.ast) != recovery_topology(&fresh_out.ast) {
        return Err(receipt(
            case,
            source,
            named_for_receipt,
            "recovery-topology",
            format!(
                "{:?} vs {:?}",
                recovery_topology(&incr_out.ast),
                recovery_topology(&fresh_out.ast)
            ),
        ));
    }
    if incr_out.ast != fresh_out.ast {
        return Err(receipt(
            case,
            source,
            named_for_receipt,
            "ast-payload-and-spans",
            "AST kind/payload/span mismatch".to_string(),
        ));
    }
    let budget_planes = [
        (
            "errors_emitted",
            incr_out.budget_usage.errors_emitted,
            fresh_out.budget_usage.errors_emitted,
        ),
        (
            "current_depth",
            incr_out.budget_usage.current_depth,
            fresh_out.budget_usage.current_depth,
        ),
        (
            "max_depth_reached",
            incr_out.budget_usage.max_depth_reached,
            fresh_out.budget_usage.max_depth_reached,
        ),
        (
            "tokens_skipped",
            incr_out.budget_usage.tokens_skipped,
            fresh_out.budget_usage.tokens_skipped,
        ),
        (
            "recoveries_attempted",
            incr_out.budget_usage.recoveries_attempted,
            fresh_out.budget_usage.recoveries_attempted,
        ),
        (
            "heredoc_scan_bytes",
            incr_out.budget_usage.heredoc_scan_bytes,
            fresh_out.budget_usage.heredoc_scan_bytes,
        ),
        (
            "tokens_consumed",
            incr_out.budget_usage.tokens_consumed,
            fresh_out.budget_usage.tokens_consumed,
        ),
        (
            "nodes_constructed",
            incr_out.budget_usage.nodes_constructed,
            fresh_out.budget_usage.nodes_constructed,
        ),
    ];
    for (plane, actual, expected) in budget_planes {
        if actual != expected {
            return Err(receipt(
                case,
                source,
                named_for_receipt,
                plane,
                format!("{actual} vs {expected}"),
            ));
        }
    }
    if incremental.content_digest() != fresh.content_digest()
        || incremental.source_len() != fresh.source_len()
        || incremental.source_len() != source.len()
    {
        return Err(receipt(
            case,
            source,
            named_for_receipt,
            "source-identity",
            format!(
                "incremental len={} digest={:?}; fresh len={} digest={:?}",
                incremental.source_len(),
                incremental.content_digest(),
                fresh.source_len(),
                fresh.content_digest()
            ),
        ));
    }
    if returned.generation() != incremental.generation()
        || returned.disposition() != incremental.disposition()
        || returned.content_digest() != incremental.content_digest()
        || returned_out.diagnostics != incr_out.diagnostics
        || returned_out.ast != incr_out.ast
        || returned_out.recovered_count != incr_out.recovered_count
        || returned_out.stop_cause() != incr_out.stop_cause()
    {
        return Err(receipt(
            case,
            source,
            named_for_receipt,
            "returned-vs-committed",
            "ReparseResult snapshot disagrees with IncrementalState".to_string(),
        ));
    }
    Ok(())
}

fn receipt(
    case: &str,
    source: &str,
    named: ParseTerminalDisposition,
    plane: &str,
    detail: String,
) -> String {
    format!(
        "case={case}; named={named:?}; plane={plane}; source_len={}; detail={detail}; reproduce=cargo test -p perl-parser --features incremental --test incremental_recovery_transitions -- --exact {case}",
        source.len()
    )
}

fn run_transition(case: Transition) -> TestResult {
    let mut state = IncrementalState::new(case.initial.to_string());
    let initial = state.snapshot();
    if !structural_matches_named(initial.parse_output(), case.expected_initial) {
        return Err(receipt(
            case.name,
            case.initial,
            case.expected_initial,
            "initial-named-disposition-vs-structure",
            format!(
                "recovered_count={}, diagnostics={:?}",
                initial.parse_output().recovered_count,
                initial.parse_output().diagnostics
            ),
        ));
    }
    if initial.disposition() != case.expected_initial {
        return Err(receipt(
            case.name,
            case.initial,
            case.expected_initial,
            "initial-named-disposition-vs-production",
            format!("{:?}", initial.disposition()),
        ));
    }
    initial.validate_against(state.source()).map_err(|error| {
        receipt(
            case.name,
            case.initial,
            case.expected_initial,
            "initial-validate",
            format!("{error}"),
        )
    })?;

    let mut expected_generation = state.generation();
    for (index, step) in case.steps.iter().enumerate() {
        match *step {
            Step::Apply(needles) => {
                let edits = needles
                    .iter()
                    .map(|needle| needle_to_edit(state.source(), *needle))
                    .collect::<Result<Vec<_>, _>>()?;
                let expected_source = apply_reference_edits(state.source(), &edits)?;
                let result = apply_edits(&mut state, &edits)
                    .map_err(|error| format!("{} step {index}: {error}", case.name))?;
                if state.source() != expected_source {
                    return Err(receipt(
                        case.name,
                        &expected_source,
                        case.expected_final,
                        "edit-application",
                        format!("state source diverged at step {index}"),
                    ));
                }
                expected_generation = expected_generation
                    .checked_next()
                    .ok_or_else(|| format!("{} generation exhausted", case.name))?;
                if state.generation() != expected_generation {
                    return Err(receipt(
                        case.name,
                        state.source(),
                        case.expected_final,
                        "generation",
                        format!("{:?} vs {:?}", state.generation(), expected_generation),
                    ));
                }
                if result.snapshot.strategy() == ParseSnapshotStrategy::Fresh {
                    return Err(receipt(
                        case.name,
                        state.source(),
                        case.expected_final,
                        "strategy",
                        "mutating edit published Fresh".to_string(),
                    ));
                }
                let named = if case
                    .steps
                    .iter()
                    .skip(index + 1)
                    .all(|step| !matches!(step, Step::Apply(_)))
                {
                    Some(case.expected_final)
                } else {
                    None
                };
                first_divergence(
                    case.name,
                    state.source(),
                    named,
                    state.snapshot(),
                    &result.snapshot,
                )?;
            }
            Step::InvalidOverlap { start, old_end } => {
                let before_source = state.source().to_string();
                let before_generation = state.generation();
                let before_digest = state.snapshot().content_digest().clone();
                let before_disposition = state.snapshot().disposition();
                let before_output = state.parse_output().clone();
                let edits = [
                    splice(&before_source, start, old_end, "x")?,
                    splice(&before_source, start.saturating_add(1).min(old_end), old_end, "y")?,
                ];
                if apply_edits(&mut state, &edits).is_ok() {
                    return Err(receipt(
                        case.name,
                        &before_source,
                        before_disposition,
                        "invalid-transaction",
                        "overlapping batch was published".to_string(),
                    ));
                }
                if state.source() != before_source
                    || state.generation() != before_generation
                    || state.snapshot().content_digest() != &before_digest
                    || state.snapshot().disposition() != before_disposition
                    || state.parse_output().diagnostics != before_output.diagnostics
                    || state.parse_output().ast != before_output.ast
                {
                    return Err(receipt(
                        case.name,
                        &before_source,
                        before_disposition,
                        "invalid-transaction-preserve",
                        "previous snapshot was mutated".to_string(),
                    ));
                }
            }
        }
    }

    if case.steps.iter().any(|step| matches!(step, Step::Apply(_)))
        && state.snapshot().disposition() != case.expected_final
    {
        return Err(receipt(
            case.name,
            state.source(),
            case.expected_final,
            "final-disposition",
            format!("{:?}", state.snapshot().disposition()),
        ));
    }
    Ok(())
}

const TRANSITIONS: &[Transition] = &[
    Transition {
        name: "clean_to_clean_literal_edit",
        initial: "my $x = 1; print 2;",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Replace { old: "1", new: "9" }])],
        expected_final: ParseTerminalDisposition::Clean,
    },
    Transition {
        name: "clean_to_recovered_missing_operand",
        initial: "my $x = 1; print 2;",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Delete { text: "1" }])],
        expected_final: ParseTerminalDisposition::Recovered,
    },
    Transition {
        name: "recovered_to_clean_insert_operand",
        initial: "my $x = ; print 2;",
        expected_initial: ParseTerminalDisposition::Recovered,
        steps: &[Step::Apply(&[Needle::InsertBefore { needle: ";", text: "1" }])],
        expected_final: ParseTerminalDisposition::Clean,
    },
    Transition {
        name: "recovered_to_recovered_prefix_range_shift",
        initial: "my $x = ; print 2;",
        expected_initial: ParseTerminalDisposition::Recovered,
        steps: &[Step::Apply(&[Needle::Prefix { text: "  " }])],
        expected_final: ParseTerminalDisposition::Recovered,
    },
    Transition {
        name: "recovered_to_recovered_different_family",
        initial: "my $x = ; print 2;",
        expected_initial: ParseTerminalDisposition::Recovered,
        steps: &[Step::Apply(&[Needle::Replace { old: "print 2;", new: "foo(1;" }])],
        expected_final: ParseTerminalDisposition::Recovered,
    },
    Transition {
        name: "recovered_to_catastrophic_unclosed_quote",
        initial: "my $x = ; print 2;",
        expected_initial: ParseTerminalDisposition::Recovered,
        steps: &[Step::Apply(&[Needle::Replace { old: " = ;", new: " = \";" }])],
        expected_final: ParseTerminalDisposition::Catastrophic,
    },
    Transition {
        name: "edit_inside_recovery_site",
        initial: "my $x = ; print 2;",
        expected_initial: ParseTerminalDisposition::Recovered,
        steps: &[Step::Apply(&[Needle::InsertBefore { needle: ";", text: " " }])],
        expected_final: ParseTerminalDisposition::Recovered,
    },
    Transition {
        name: "clean_to_recovered_to_clean_sequence",
        initial: "my $x = 1; print 2;",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[
            Step::Apply(&[Needle::Delete { text: "1" }]),
            Step::Apply(&[Needle::InsertBefore { needle: ";", text: "1" }]),
        ],
        expected_final: ParseTerminalDisposition::Clean,
    },
    Transition {
        name: "edit_undo_returns_to_original_bytes_new_generation",
        initial: "my $x = 1; print 2;",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[
            Step::Apply(&[Needle::Replace { old: "1", new: "42" }]),
            Step::Apply(&[Needle::Replace { old: "42", new: "1" }]),
        ],
        expected_final: ParseTerminalDisposition::Clean,
    },
    Transition {
        name: "atomic_batch_repairs_two_recoveries",
        initial: "my $x = ; my $y = ;",
        expected_initial: ParseTerminalDisposition::Recovered,
        steps: &[Step::Apply(&[
            Needle::InsertBeforeNth { needle: ";", index: 0, text: "1" },
            Needle::InsertBeforeNth { needle: ";", index: 1, text: "2" },
        ])],
        expected_final: ParseTerminalDisposition::Clean,
    },
    Transition {
        name: "atomic_batch_introduces_two_recoveries",
        initial: "my $x = 1; my $y = 2;",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Delete { text: "1" }, Needle::Delete { text: "2" }])],
        expected_final: ParseTerminalDisposition::Recovered,
    },
    Transition {
        name: "nested_block_clean_to_recovered",
        initial: "sub f { if (1) { my $x = 1; } }",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Replace { old: "my $x = 1", new: "my $x = " }])],
        expected_final: ParseTerminalDisposition::Recovered,
    },
    Transition {
        name: "quote_like_unclosed_q",
        initial: "my $x = q{ok};",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Delete { text: "}" }])],
        expected_final: ParseTerminalDisposition::Catastrophic,
    },
    Transition {
        name: "regex_unclosed_match",
        initial: "my $x = m/foo/;",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Delete { text: "/" }])],
        expected_final: ParseTerminalDisposition::Recovered,
    },
    Transition {
        name: "qw_missing_closer",
        initial: "my @x = qw(a b);",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Delete { text: ")" }])],
        expected_final: ParseTerminalDisposition::Recovered,
    },
    Transition {
        name: "signature_missing_parameter",
        initial: "sub f ($x) { 1 }",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Delete { text: ")" }])],
        expected_final: ParseTerminalDisposition::Catastrophic,
    },
    Transition {
        name: "unicode_identifier_clean_to_recovered",
        initial: "my $café = 1;",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Delete { text: "1" }])],
        expected_final: ParseTerminalDisposition::Recovered,
    },
    Transition {
        name: "crlf_clean_to_recovered",
        initial: "my $x = 1;\r\nprint 2;\r\n",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Delete { text: "1" }])],
        expected_final: ParseTerminalDisposition::Recovered,
    },
    Transition {
        name: "bare_cr_clean_to_clean",
        initial: "my $x = 1;\rprint 2;",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Replace { old: "2", new: "3" }])],
        expected_final: ParseTerminalDisposition::Clean,
    },
    Transition {
        name: "bom_prefix_range_shift",
        initial: "my $x = ;",
        expected_initial: ParseTerminalDisposition::Recovered,
        steps: &[Step::Apply(&[Needle::Prefix { text: "\u{FEFF}" }])],
        expected_final: ParseTerminalDisposition::Recovered,
    },
    Transition {
        name: "empty_to_clean",
        initial: "",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Prefix { text: "my $x = 1;" }])],
        expected_final: ParseTerminalDisposition::Clean,
    },
    Transition {
        name: "whitespace_only_stays_clean",
        initial: " \n\t ",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Prefix { text: "\n" }])],
        expected_final: ParseTerminalDisposition::Clean,
    },
    Transition {
        name: "pod_then_statement_recovery",
        initial: "=pod\nignored\n=cut\nmy $x = 1;\n",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Delete { text: "1" }])],
        expected_final: ParseTerminalDisposition::Recovered,
    },
    Transition {
        name: "data_section_clean_to_clean",
        initial: "my $x = 1;\n__DATA__\npayload\n",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Replace { old: "1", new: "2" }])],
        expected_final: ParseTerminalDisposition::Clean,
    },
    Transition {
        name: "heredoc_terminator_edit",
        initial: "print <<EOF;\nhello\nEOF\n",
        expected_initial: ParseTerminalDisposition::Clean,
        steps: &[Step::Apply(&[Needle::Replace { old: "hello", new: "world" }])],
        expected_final: ParseTerminalDisposition::Clean,
    },
    Transition {
        name: "invalid_overlap_preserves_recovered_snapshot",
        initial: "my $x = ; print 2;",
        expected_initial: ParseTerminalDisposition::Recovered,
        steps: &[Step::InvalidOverlap { start: 0, old_end: 4 }],
        expected_final: ParseTerminalDisposition::Recovered,
    },
];

#[test]
fn recovery_transition_matrix_matches_fresh_parse() -> TestResult {
    let mut failures = Vec::new();
    for case in TRANSITIONS {
        if let Err(error) = run_transition(*case) {
            failures.push(error);
        }
    }
    if failures.is_empty() { Ok(()) } else { Err(failures.join("\n")) }
}

#[test]
fn atomic_batch_uses_old_generation_coordinates() -> TestResult {
    let source = "my $left = 1; my $right = 2;";
    let first = source.find('1').ok_or("missing 1")?;
    let second = source.find('2').ok_or("missing 2")?;
    let edits = [splice(source, first, first + 1, "")?, splice(source, second, second + 1, "")?];
    let expected = apply_reference_edits(source, &edits)?;
    let mut state = IncrementalState::new(source.to_string());
    let result = apply_edits(&mut state, &edits).map_err(|error| error.to_string())?;
    assert_eq!(state.source(), expected);
    first_divergence(
        "atomic_batch_uses_old_generation_coordinates",
        &expected,
        Some(ParseTerminalDisposition::Recovered),
        state.snapshot(),
        &result.snapshot,
    )
}

#[test]
fn oversized_multi_edit_batch_still_matches_fresh() -> TestResult {
    let source = "my $left = 1;\nmy $right = 2;\n";
    let second_start = source.find("my $right").ok_or("second statement missing")?;
    let padding = " ".repeat(MAX_EDIT_SIZE / 2 + 1);
    let edits = vec![
        splice(source, 0, 0, &padding)?,
        splice(source, second_start, second_start, &padding)?,
    ];
    let expected = apply_reference_edits(source, &edits)?;
    let mut state = IncrementalState::new(source.to_string());
    let result = apply_edits(&mut state, &edits).map_err(|error| error.to_string())?;
    assert_eq!(state.source(), expected);
    assert_eq!(result.snapshot.strategy(), ParseSnapshotStrategy::IncrementalFullFallback);
    first_divergence(
        "oversized_multi_edit_batch_still_matches_fresh",
        &expected,
        Some(ParseTerminalDisposition::Clean),
        state.snapshot(),
        &result.snapshot,
    )
}

#[test]
fn prefix_edit_moves_recovery_diagnostic_range() -> TestResult {
    let source = "my $x = ;";
    let mut state = IncrementalState::new(source.to_string());
    let before = state
        .parse_output()
        .diagnostics
        .iter()
        .filter_map(ParseError::location)
        .collect::<Vec<_>>();
    if before.is_empty() {
        return Err("fixture must start with an anchored recovery diagnostic".to_string());
    }
    let prefix = "    ";
    let result = apply_edits(&mut state, &[splice(source, 0, 0, prefix)?])
        .map_err(|error| error.to_string())?;
    let after = state
        .parse_output()
        .diagnostics
        .iter()
        .filter_map(ParseError::location)
        .collect::<Vec<_>>();
    if after == before {
        return Err("prefix edit kept the old diagnostic byte offset".to_string());
    }
    first_divergence(
        "prefix_edit_moves_recovery_diagnostic_range",
        state.source(),
        Some(ParseTerminalDisposition::Recovered),
        state.snapshot(),
        &result.snapshot,
    )
}

#[test]
fn comparison_helper_rejects_cleared_diagnostics() -> TestResult {
    let source = "my $x = ;";
    let mut incremental = fresh_snapshot(source);
    assert_eq!(incremental.disposition(), ParseTerminalDisposition::Recovered);
    let mut output = incremental.parse_output().clone();
    output.diagnostics.clear();
    output.recovered_count = 0;
    incremental = ParseSnapshot::from_output(
        source,
        ParseGeneration::INITIAL,
        ParseSnapshotStrategy::IncrementalFullFallback,
        output,
    );
    match first_divergence(
        "comparison_helper_rejects_cleared_diagnostics",
        source,
        Some(ParseTerminalDisposition::Recovered),
        &incremental,
        &incremental,
    ) {
        Err(err)
            if err.contains("named-disposition-vs-structure")
                || err.contains("typed-diagnostics") =>
        {
            Ok(())
        }
        Err(err) => Err(format!("cleared diagnostics failed on the wrong plane: {err}")),
        Ok(()) => Err("cleared diagnostics must fail the named Recovered oracle".to_string()),
    }
}

#[test]
fn comparison_helper_rejects_reordered_diagnostics() -> TestResult {
    let source = "my $x = ; my $y = ;";
    let mut incremental = fresh_snapshot(source);
    let mut output = incremental.parse_output().clone();
    if output.diagnostics.len() < 2 {
        return Err("fixture must carry at least two diagnostics".to_string());
    }
    let last = output.diagnostics.len() - 1;
    output.diagnostics.swap(0, last);
    incremental = ParseSnapshot::from_output(
        source,
        ParseGeneration::INITIAL,
        ParseSnapshotStrategy::IncrementalFullFallback,
        output,
    );
    match first_divergence(
        "comparison_helper_rejects_reordered_diagnostics",
        source,
        Some(ParseTerminalDisposition::Recovered),
        &incremental,
        &incremental,
    ) {
        Err(err)
            if err.contains("diagnostic-order-and-ranges") || err.contains("typed-diagnostics") =>
        {
            Ok(())
        }
        Err(err) => Err(format!("reordered diagnostics failed on the wrong plane: {err}")),
        Ok(()) => Err("reordered diagnostics must fail".to_string()),
    }
}

#[test]
#[ignore = "#7008/#7112: deterministic cancellation injection is not production-reachable on this snapshot surface"]
fn any_state_to_cancelled_is_an_owned_unimplemented_boundary() {}

#[test]
#[ignore = "#7008/#7074: independent budget-exhaustion injection is not production-reachable on this snapshot surface"]
fn any_state_to_budget_exhausted_is_an_owned_unimplemented_boundary() {}
