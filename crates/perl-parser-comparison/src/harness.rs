//! Parser harness for legacy verdicts and generic subject execution evidence.
//!
//! New comparison work must use [`SubjectExecution`] and an independent
//! [`ScoredComparison`](crate::evidence::ScoredComparison). The legacy
//! [`ParseResult`] path remains temporarily for existing corpus and report
//! consumers and is deliberately documented as lossy.

use std::collections::BTreeMap;
use std::panic;

use crate::evidence::{
    BoundedText, ComparisonModelError, DiagnosticSummary, HarnessFailure, InstrumentState,
    ObservationDisposition, ObservationPlane, SubjectDisposition, SubjectExecution, SubjectRole,
};
use crate::outcomes::Verdict;

const MAX_DEBUG_PROJECTION_BYTES: usize = 4_096;
const MAX_ERROR_BYTES: usize = 1_024;

/// Legacy output of running one parser on one input.
///
/// `verdict` is a lossy compatibility projection. In particular,
/// `Verdict::Correct` may mean only that the subject executed without its
/// designated error signal. New comparison code must use [`execute_v1`] or
/// [`execute_v3`] and score an explicit reviewed expectation separately.
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
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::V1TreeSitterC => write!(formatter, "v1(tree-sitter-c)"),
            Self::V2Pest => write!(formatter, "v2(pest)"),
            Self::V3RecursiveDescent => write!(formatter, "v3(recursive-descent)"),
        }
    }
}

#[derive(Debug)]
enum RawTerminal {
    Completed(SubjectDisposition),
    Failed(HarnessFailure),
}

#[derive(Debug)]
struct RawExecution {
    terminal: RawTerminal,
    projection: String,
    error: Option<String>,
    diagnostics: DiagnosticSummary,
    instrument_state: InstrumentState,
}

/// Execute the currently embedded historical C Tree-sitter subject.
///
/// A tree without an `ERROR` node is a completed
/// [`SubjectDisposition::AcceptedClean`] execution, not a correctness verdict.
/// Structural correctness must be scored by an independent observer.
pub fn execute_v1(source: &str) -> Result<SubjectExecution, ComparisonModelError> {
    subject_execution(
        SubjectRole::HistoricalTreeSitterC,
        run_v1(source),
        historical_tree_sitter_observations(),
    )
}

/// Execute the native recursive-descent subject.
///
/// Diagnostic-bearing output is [`SubjectDisposition::AcceptedRecovered`]
/// rather than rejection or correctness. Structural correctness must be scored
/// by an independent observer.
pub fn execute_v3(source: &str) -> Result<SubjectExecution, ComparisonModelError> {
    subject_execution(
        SubjectRole::NativeRecursiveDescent,
        run_v3(source),
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
        verdict: lossy_legacy_verdict(&raw.terminal),
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
            Err(error) => {
                let message = format!("{error}");
                (None, Some(message), None)
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
        Ok((_, Some(error), _)) => ParseResult {
            parser: ParserLabel::V2Pest,
            source: source.to_owned(),
            verdict: Verdict::Errors,
            sexp: String::new(),
            error: Some(error),
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
        verdict: lossy_legacy_verdict(&raw.terminal),
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
            terminal: RawTerminal::Failed(HarnessFailure::CrashedOrSignalled),
            projection: String::new(),
            error: Some("v1 panicked".to_owned()),
            diagnostics: DiagnosticSummary::default(),
            instrument_state: InstrumentState::Failed,
        },
        Ok(Err(error)) => RawExecution {
            terminal: RawTerminal::Failed(HarnessFailure::SetupFailed),
            projection: String::new(),
            error: Some(error),
            diagnostics: DiagnosticSummary::default(),
            instrument_state: InstrumentState::Unavailable,
        },
        Ok(Ok((projection, has_error))) => RawExecution {
            terminal: RawTerminal::Completed(if has_error {
                SubjectDisposition::AcceptedRecovered
            } else {
                SubjectDisposition::AcceptedClean
            }),
            projection,
            error: None,
            diagnostics: DiagnosticSummary::new(
                usize::from(has_error),
                has_error,
                has_error,
            ),
            instrument_state: InstrumentState::Complete,
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
            terminal: RawTerminal::Failed(HarnessFailure::CrashedOrSignalled),
            projection: String::new(),
            error: Some("v3 panicked".to_owned()),
            diagnostics: DiagnosticSummary::default(),
            instrument_state: InstrumentState::Failed,
        },
        Ok((projection, diagnostic_count)) => RawExecution {
            terminal: RawTerminal::Completed(if diagnostic_count == 0 {
                SubjectDisposition::AcceptedClean
            } else {
                SubjectDisposition::AcceptedRecovered
            }),
            projection,
            error: None,
            diagnostics: DiagnosticSummary::new(
                diagnostic_count,
                diagnostic_count > 0,
                false,
            ),
            instrument_state: InstrumentState::Complete,
        },
    }
}

fn subject_execution(
    subject: SubjectRole,
    raw: RawExecution,
    successful_observations: BTreeMap<ObservationPlane, ObservationDisposition>,
) -> Result<SubjectExecution, ComparisonModelError> {
    let RawExecution {
        terminal,
        projection,
        error,
        diagnostics,
        instrument_state,
    } = raw;

    let debug_projection = bounded_optional_text(projection, MAX_DEBUG_PROJECTION_BYTES)?;
    let error = match error {
        Some(error) => Some(BoundedText::new(error, MAX_ERROR_BYTES)?),
        None => None,
    };

    match terminal {
        RawTerminal::Completed(disposition) => SubjectExecution::completed(
            subject,
            disposition,
            diagnostics,
            successful_observations,
            debug_projection,
            instrument_state,
        ),
        RawTerminal::Failed(failure) => SubjectExecution::failed(
            subject,
            failure,
            diagnostics,
            failed_observations(successful_observations),
            debug_projection,
            instrument_state,
            error,
        ),
    }
}

fn historical_tree_sitter_observations(
) -> BTreeMap<ObservationPlane, ObservationDisposition> {
    BTreeMap::from([
        (ObservationPlane::Structure, ObservationDisposition::Observed),
        (ObservationPlane::Recovery, ObservationDisposition::Observed),
        (
            ObservationPlane::SourceGeometry,
            ObservationDisposition::NotProven,
        ),
        (
            ObservationPlane::BodyOwnership,
            ObservationDisposition::NotProven,
        ),
        (
            ObservationPlane::IncrementalFinalState,
            ObservationDisposition::NotProven,
        ),
        (
            ObservationPlane::QueryOrHighlight,
            ObservationDisposition::NotProven,
        ),
    ])
}

fn native_recursive_descent_observations(
) -> BTreeMap<ObservationPlane, ObservationDisposition> {
    BTreeMap::from([
        (ObservationPlane::Structure, ObservationDisposition::Observed),
        (ObservationPlane::Recovery, ObservationDisposition::Observed),
        (
            ObservationPlane::SourceGeometry,
            ObservationDisposition::NotProven,
        ),
        (
            ObservationPlane::BodyOwnership,
            ObservationDisposition::NotProven,
        ),
        (
            ObservationPlane::IncrementalFinalState,
            ObservationDisposition::NotProven,
        ),
        (
            ObservationPlane::QueryOrHighlight,
            ObservationDisposition::Unsupported,
        ),
    ])
}

fn failed_observations(
    observations: BTreeMap<ObservationPlane, ObservationDisposition>,
) -> BTreeMap<ObservationPlane, ObservationDisposition> {
    observations
        .into_keys()
        .map(|plane| (plane, ObservationDisposition::NotProven))
        .collect()
}

fn bounded_optional_text(
    value: String,
    maximum: usize,
) -> Result<Option<BoundedText>, ComparisonModelError> {
    if value.is_empty() {
        Ok(None)
    } else {
        BoundedText::new(value, maximum)
            .map(Some)
            .map_err(ComparisonModelError::from)
    }
}

fn lossy_legacy_verdict(terminal: &RawTerminal) -> Verdict {
    match terminal {
        RawTerminal::Completed(SubjectDisposition::AcceptedClean) => Verdict::Correct,
        RawTerminal::Failed(HarnessFailure::CrashedOrSignalled) => Verdict::Crashes,
        RawTerminal::Completed(_) | RawTerminal::Failed(_) => Verdict::Errors,
    }
}
