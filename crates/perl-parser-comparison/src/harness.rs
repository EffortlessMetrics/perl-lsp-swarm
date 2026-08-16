//! Parser harness for legacy verdicts and generic subject execution evidence.
//!
//! New comparison work must use [`SubjectExecution`] and an independent
//! [`ScoredComparison`](crate::evidence::ScoredComparison). The legacy
//! [`ParseResult`] path remains temporarily for existing corpus and report
//! consumers and is deliberately documented as lossy.

use std::collections::BTreeMap;
use std::panic;

use crate::evidence::{
    DiagnosticSummary, ExecutionDisposition, InstrumentState, ObservationAvailability,
    ObservationPlane, SubjectExecution, SubjectRole,
};
use crate::outcomes::Verdict;

const MAX_DEBUG_PROJECTION_BYTES: usize = 4_096;

/// Legacy output of running one parser on one input.
///
/// `verdict` is a lossy compatibility projection. In particular,
/// `Verdict::Correct` may mean only that the subject executed without its
/// designated error signal. New comparison code must use [`execute_v1`] or
/// [`execute_v3`] and score an explicit observer expectation separately.
#[derive(Debug)]
#[non_exhaustive]
pub struct ParseResult {
    /// Which parser produced this result.
    pub parser: ParserLabel,
    /// The source string that was parsed.
    pub source: String,
    /// Legacy outcome category.
    pub verdict: Verdict,
    /// S-expression or description of the parse output for diagnostics.
    pub sexp: String,
    /// Error message when execution or parser setup did not return a usable result.
    pub error: Option<String>,
}

impl ParseResult {
    /// Returns `true` if the legacy debug projection contains the substring.
    pub fn sexp_contains(&self, needle: &str) -> bool {
        self.sexp.contains(needle)
    }
}

/// Identifies which parser produced a legacy result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParserLabel {
    /// v1: historical C Tree-sitter grammar via FFI.
    V1TreeSitterC,
    /// v2: Pest/PEG legacy parser.
    V2Pest,
    /// v3: recursive-descent native parser.
    V3RecursiveDescent,
}

impl std::fmt::Display for ParserLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::V1TreeSitterC => write!(f, "v1(tree-sitter-c)"),
            Self::V2Pest => write!(f, "v2(pest)"),
            Self::V3RecursiveDescent => write!(f, "v3(recursive-descent)"),
        }
    }
}

#[derive(Debug)]
struct RawExecution {
    disposition: ExecutionDisposition,
    projection: String,
    error: Option<String>,
    diagnostics: DiagnosticSummary,
}

/// Execute the currently embedded historical C Tree-sitter subject.
///
/// A tree without an `ERROR` node is [`ExecutionDisposition::AcceptedClean`],
/// not a correctness verdict. Structural correctness must be scored by an
/// independent observer.
pub fn execute_v1(source: &str) -> SubjectExecution {
    let raw = run_v1(source);
    subject_execution(
        SubjectRole::HistoricalTreeSitterC,
        raw,
        historical_tree_sitter_observations(),
    )
}

/// Execute the native recursive-descent subject.
///
/// Diagnostic-bearing output is [`ExecutionDisposition::AcceptedRecovered`]
/// rather than rejection or correctness. Structural correctness must be scored
/// by an independent observer.
pub fn execute_v3(source: &str) -> SubjectExecution {
    let raw = run_v3(source);
    subject_execution(
        SubjectRole::NativeRecursiveDescent,
        raw,
        native_recursive_descent_observations(),
    )
}

/// Parse with the historical C Tree-sitter subject through the legacy verdict bridge.
///
/// This function preserves current corpus/report behavior until the consumer
/// migration train lands. New comparison code must use [`execute_v1`].
pub fn parse_v1(source: &str) -> ParseResult {
    let raw = run_v1(source);
    ParseResult {
        parser: ParserLabel::V1TreeSitterC,
        source: source.to_owned(),
        verdict: lossy_legacy_verdict(raw.disposition),
        sexp: raw.projection,
        error: raw.error,
    }
}

/// Parse with the v2 Pest legacy parser through the unchanged legacy path.
///
/// Pest execution-to-observation migration is owned by the dedicated Pest
/// subject train. This PR intentionally preserves `Ok => Verdict::Correct`.
pub fn parse_v2(source: &str) -> ParseResult {
    let source_owned = source.to_owned();
    let result = panic::catch_unwind(move || {
        use perl_parser_pest::PureRustPerlParser;
        let mut parser = PureRustPerlParser::new();
        match parser.parse(&source_owned) {
            Ok(ast) => {
                let sexp = parser.to_sexp(&ast);
                (Some(sexp), None::<String>, Some(ast))
            }
            Err(e) => {
                let msg = format!("{e}");
                (None, Some(msg), None)
            }
        }
    });

    match result {
        Err(_panic) => ParseResult {
            parser: ParserLabel::V2Pest,
            source: source.to_owned(),
            verdict: Verdict::Crashes,
            sexp: String::new(),
            error: Some("v2 panicked".to_owned()),
        },
        Ok((Some(sexp), None, _ast)) => ParseResult {
            parser: ParserLabel::V2Pest,
            source: source.to_owned(),
            verdict: Verdict::Correct,
            sexp,
            error: None,
        },
        Ok((_, Some(err), _)) => ParseResult {
            parser: ParserLabel::V2Pest,
            source: source.to_owned(),
            verdict: Verdict::Errors,
            sexp: String::new(),
            error: Some(err),
        },
        Ok((None, None, _)) => ParseResult {
            parser: ParserLabel::V2Pest,
            source: source.to_owned(),
            verdict: Verdict::Errors,
            sexp: String::new(),
            error: Some("parse returned neither Ok nor Err".to_owned()),
        },
    }
}

/// Parse with the native recursive-descent subject through the legacy verdict bridge.
///
/// This function preserves current corpus/report behavior until the consumer
/// migration train lands. New comparison code must use [`execute_v3`].
pub fn parse_v3(source: &str) -> ParseResult {
    let raw = run_v3(source);
    ParseResult {
        parser: ParserLabel::V3RecursiveDescent,
        source: source.to_owned(),
        verdict: lossy_legacy_verdict(raw.disposition),
        sexp: raw.projection,
        error: raw.error,
    }
}

fn run_v1(source: &str) -> RawExecution {
    let source_owned = source.to_owned();
    let result = panic::catch_unwind(move || {
        use tree_sitter_perl_c::try_parse_perl_code;
        match try_parse_perl_code(&source_owned) {
            Ok(tree) => {
                let root = tree.root_node();
                let has_error = root.has_error();
                Ok((root.to_sexp(), has_error))
            }
            Err(error) => Err(format!("{error}")),
        }
    });

    match result {
        Err(_panic) => RawExecution {
            disposition: ExecutionDisposition::Crashed,
            projection: String::new(),
            error: Some("v1 panicked".to_owned()),
            diagnostics: DiagnosticSummary::default(),
        },
        Ok(Err(error)) => RawExecution {
            disposition: ExecutionDisposition::SetupFailed,
            projection: String::new(),
            error: Some(error),
            diagnostics: DiagnosticSummary::default(),
        },
        Ok(Ok((projection, has_error))) => RawExecution {
            disposition: if has_error {
                ExecutionDisposition::AcceptedRecovered
            } else {
                ExecutionDisposition::AcceptedClean
            },
            projection,
            error: None,
            diagnostics: DiagnosticSummary::new(
                usize::from(has_error),
                has_error,
                has_error,
            ),
        },
    }
}

fn run_v3(source: &str) -> RawExecution {
    let source_owned = source.to_owned();
    let result = panic::catch_unwind(move || {
        use perl_parser_core::Parser;
        let mut parser = Parser::new(&source_owned);
        let output = parser.parse_with_recovery();
        let diagnostic_count = output.diagnostics.len();
        (output.ast.to_sexp(), diagnostic_count)
    });

    match result {
        Err(_panic) => RawExecution {
            disposition: ExecutionDisposition::Crashed,
            projection: String::new(),
            error: Some("v3 panicked".to_owned()),
            diagnostics: DiagnosticSummary::default(),
        },
        Ok((projection, diagnostic_count)) => RawExecution {
            disposition: if diagnostic_count == 0 {
                ExecutionDisposition::AcceptedClean
            } else {
                ExecutionDisposition::AcceptedRecovered
            },
            projection,
            error: None,
            diagnostics: DiagnosticSummary::new(
                diagnostic_count,
                diagnostic_count > 0,
                false,
            ),
        },
    }
}

fn subject_execution(
    subject: SubjectRole,
    raw: RawExecution,
    observations: BTreeMap<ObservationPlane, ObservationAvailability>,
) -> SubjectExecution {
    let instrument_state = match raw.disposition {
        ExecutionDisposition::InstrumentUnavailable => InstrumentState::Unavailable,
        ExecutionDisposition::InstrumentFailed => InstrumentState::Failed,
        _ => InstrumentState::Available,
    };
    SubjectExecution::new(
        subject,
        raw.disposition,
        raw.diagnostics,
        observations,
        bounded_debug_projection(raw.projection),
        instrument_state,
        raw.error,
    )
}

fn historical_tree_sitter_observations() -> BTreeMap<ObservationPlane, ObservationAvailability> {
    BTreeMap::from([
        (ObservationPlane::Structure, ObservationAvailability::Observable),
        (ObservationPlane::Recovery, ObservationAvailability::Observable),
        (ObservationPlane::SourceGeometry, ObservationAvailability::NotProven),
        (ObservationPlane::BodyOwnership, ObservationAvailability::NotProven),
        (
            ObservationPlane::IncrementalFinalState,
            ObservationAvailability::NotProven,
        ),
        (
            ObservationPlane::QueryOrHighlight,
            ObservationAvailability::NotProven,
        ),
    ])
}

fn native_recursive_descent_observations() -> BTreeMap<ObservationPlane, ObservationAvailability> {
    BTreeMap::from([
        (ObservationPlane::Structure, ObservationAvailability::Observable),
        (ObservationPlane::Recovery, ObservationAvailability::Observable),
        (ObservationPlane::SourceGeometry, ObservationAvailability::NotProven),
        (ObservationPlane::BodyOwnership, ObservationAvailability::NotProven),
        (
            ObservationPlane::IncrementalFinalState,
            ObservationAvailability::NotProven,
        ),
        (
            ObservationPlane::QueryOrHighlight,
            ObservationAvailability::Unsupported,
        ),
    ])
}

fn bounded_debug_projection(projection: String) -> Option<String> {
    if projection.is_empty() {
        return None;
    }
    if projection.len() <= MAX_DEBUG_PROJECTION_BYTES {
        return Some(projection);
    }

    let mut end = MAX_DEBUG_PROJECTION_BYTES;
    while !projection.is_char_boundary(end) {
        end -= 1;
    }
    let mut bounded = projection[..end].to_owned();
    bounded.push('…');
    Some(bounded)
}

fn lossy_legacy_verdict(disposition: ExecutionDisposition) -> Verdict {
    match disposition {
        ExecutionDisposition::AcceptedClean => Verdict::Correct,
        ExecutionDisposition::Crashed => Verdict::Crashes,
        ExecutionDisposition::AcceptedRecovered
        | ExecutionDisposition::Rejected
        | ExecutionDisposition::Unsupported
        | ExecutionDisposition::TimedOut
        | ExecutionDisposition::SetupFailed
        | ExecutionDisposition::InstrumentUnavailable
        | ExecutionDisposition::InstrumentFailed
        | ExecutionDisposition::NotRun => Verdict::Errors,
    }
}
