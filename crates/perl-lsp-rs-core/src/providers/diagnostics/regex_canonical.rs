//! Canonical regex diagnostics: projection of the parser-retained analysis (#7024).
//!
//! # Why this module exists
//!
//! The parser retains exactly one canonical regex analysis per source generation
//! (#7018). Before this module the language server could not use it. Regex findings
//! reached clients by two lossy routes instead:
//!
//! 1. as parser errors, whose client-facing code was recovered by matching the
//!    parser's *message text* (`DiagnosticCode::from_message`) and whose range was
//!    always the error offset plus one byte; and
//! 2. as a security lint reading the AST's `has_embedded_code` boolean, whose range
//!    was the whole regex node.
//!
//! Neither route could carry the analysis's typed code, its diagnostic class, its
//! exact span, or its completeness. This module replaces both for callers that hold
//! a retained table: it *projects*, and never re-analyzes. There is no regex parser,
//! scanner, or message inference here, and adding one would defeat the point.
//!
//! # Range spaces
//!
//! The retained record carries findings in three different coordinate spaces, and
//! mixing them up silently misplaces diagnostics. They are:
//!
//! | Source                                | Space                  | Conversion |
//! |---------------------------------------|------------------------|------------|
//! | `pattern.structural.diagnostics`      | pattern-body relative  | [`RegexAnalysisRecord::map_pattern_range`] |
//! | `pattern.controls.captures.diagnostics` | original source      | used as-is |
//! | `modifiers.diagnostics`               | original source        | used as-is |
//!
//! `regex_canonical_range_spaces_are_pinned_to_source_text` pins all three against
//! the actual bytes they name, so a future change to any of them fails loudly.
//!
//! # Publication policy
//!
//! - Severity is never decided here. It comes from the catalog
//!   ([`DiagnosticCode::severity`]) so one code cannot mean two urgencies.
//! - Embedded executable code keeps its established identity `PL609`
//!   ([`DiagnosticCode::SecurityEmbeddedRegexCode`]) rather than being renumbered
//!   into the new block. The projection improves its *range* — the `(?{ ... })`
//!   block rather than the whole pattern — not its meaning.
//! - Interpolation alone publishes nothing. A dynamic pattern is extremely common
//!   (`/$re/`), and a diagnostic on every one of them would be noise, not evidence.
//!   Dynamic *execution* is what gets published, as `PL609`.
//! - Analysis that stopped early publishes `PL1007`, so a missing finding is never
//!   read as a clean pattern.
//! - Records with no analysis (transliteration bodies, unavailable geometry) publish
//!   nothing. Absence of a record is not a claim that a pattern is clean.

use perl_diagnostics::codes::DiagnosticCode;
use perl_parser_core::{RegexAnalysisRecord, RegexAnalysisTable, RetainedRegexPatternAnalysis};
use perl_regex::analyzer::CaptureDiagnosticCode;
use perl_regex::validator::{RegexDiagnostic, RegexDiagnosticCode};

use super::internal_types::Diagnostic;

/// Project every canonical finding in `table` into provider diagnostics.
///
/// Records are visited in retained (source) order and each record's findings are
/// emitted in their own analysis order, so the result is deterministic.
///
/// This publishes each canonical finding exactly once. Callers that also run the
/// compatibility `has_embedded_code` lint must suppress it when a table is present —
/// see [`super::lints::security::check_security_with_canonical_regex`].
pub fn project_canonical_regex_diagnostics(table: &RegexAnalysisTable) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for record in &table.records {
        project_record(record, &mut diagnostics);
    }
    diagnostics
}

/// Project one record's findings, in source order within the record.
///
/// Modifier findings are emitted even when the body was not analyzed as a regex:
/// `tr/a-z/A-Z/r` has no regex body, but `r` is still wrong on it.
fn project_record(record: &RegexAnalysisRecord, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(pattern) = &record.pattern {
        project_structural(record, pattern, diagnostics);
        project_captures(pattern, diagnostics);
        project_incompleteness(record, pattern, diagnostics);
    }

    // Modifier findings survive even when the body itself was not analyzed as a
    // regex: `tr/a-z/A-Z/r` has no regex body, but `r` is still wrong there.
    if let Some(modifiers) = &record.modifiers {
        for diagnostic in &modifiers.diagnostics {
            let Some(code) = modifier_code(diagnostic.code) else {
                continue;
            };
            diagnostics.push(build(
                code,
                (diagnostic.range.start, diagnostic.range.end),
                diagnostic.message(),
            ));
        }
    }
}

/// Project the pattern-body findings, the only ones whose spans are body-relative.
///
/// Each is mapped back to original source through the record's geometry; a span
/// that cannot be mapped is dropped rather than anchored at a guessed offset.
fn project_structural(
    record: &RegexAnalysisRecord,
    pattern: &RetainedRegexPatternAnalysis,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for diagnostic in &pattern.structural.diagnostics {
        let Some(code) = structural_code(diagnostic.code) else {
            continue;
        };
        // Body-relative. A record whose geometry cannot map the span back to
        // original source is dropped rather than anchored at a guessed offset:
        // a diagnostic on the wrong bytes is worse than one the client never sees.
        let Some(span) = record.map_pattern_range(diagnostic.range) else {
            continue;
        };
        diagnostics.push(build(code, (span.start, span.end), diagnostic.message()));
    }
}

/// Project capture findings, which already carry original-source spans.
///
/// `analyze_pattern_controls` is given the body's start offset and resolves its
/// ranges against it, so these must not be mapped a second time.
fn project_captures(pattern: &RetainedRegexPatternAnalysis, diagnostics: &mut Vec<Diagnostic>) {
    for diagnostic in &pattern.controls.captures.diagnostics {
        let (code, message) = match diagnostic.code {
            CaptureDiagnosticCode::InvalidName => (
                DiagnosticCode::RegexCaptureInvalid,
                "invalid capture name in this pattern".to_string(),
            ),
            CaptureDiagnosticCode::RequiresPerlVersion => (
                DiagnosticCode::RegexCaptureUnavailable,
                match diagnostic.required_perl_version {
                    Some((major, minor)) => {
                        format!("this capture form requires Perl {major}.{minor} or newer")
                    }
                    None => "this capture form requires a newer Perl version".to_string(),
                },
            ),
            CaptureDiagnosticCode::RequiresSourceUtf8 => (
                DiagnosticCode::RegexCaptureUnavailable,
                "this capture name requires source UTF-8 semantics (`use utf8;`)".to_string(),
            ),
            // `CaptureDiagnosticCode` is `#[non_exhaustive]`. A capture code added
            // upstream is not published until it is classified here on purpose:
            // guessing a class for an unknown finding is how a wrong severity ships.
            _ => continue,
        };
        // Already in original-source coordinates: `analyze_pattern_controls` is
        // given the body's start offset and resolves its ranges against it.
        diagnostics.push(build(code, (diagnostic.range.start, diagnostic.range.end), message));
    }
}

/// Publish the limitation when static analysis stopped before the end of the
/// evidence, so that a missing finding cannot be read as a proven-clean pattern.
///
/// Deliberately narrow: only an analyzer that actually stopped early (a budget) or
/// that could not map its own ranges back to source qualifies. Dynamic or
/// unsupported constructs are ordinary, are already described by their own
/// findings, and would make this a diagnostic on most real patterns.
fn project_incompleteness(
    record: &RegexAnalysisRecord,
    pattern: &RetainedRegexPatternAnalysis,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let control_status = pattern.controls.status;
    let stopped_early = pattern.structural.exhausted.is_some()
        || control_status.exhausted.is_some()
        || !control_status.source_mapping_complete;
    if !stopped_early {
        return;
    }

    let Some(span) = record.pattern_range() else {
        return;
    };
    diagnostics.push(build(
        DiagnosticCode::RegexAnalysisIncomplete,
        (span.start, span.end),
        "static analysis of this pattern stopped before the end; findings are partial".to_string(),
    ));
}

/// Map a canonical structural finding onto its stable client-facing identity.
///
/// Modifier-domain codes never reach here — they arrive through
/// [`modifier_code`] — but the match stays total so a new canonical code has to be
/// classified deliberately rather than defaulting into a wrong class.
fn structural_code(code: RegexDiagnosticCode) -> Option<DiagnosticCode> {
    match code {
        // Executable pattern code keeps its established security identity.
        RegexDiagnosticCode::EmbeddedCodeImmediate | RegexDiagnosticCode::EmbeddedCodeDeferred => {
            Some(DiagnosticCode::SecurityEmbeddedRegexCode)
        }
        RegexDiagnosticCode::NestedQuantifierRisk => Some(DiagnosticCode::RegexBacktrackingRisk),
        RegexDiagnosticCode::UnicodePropertyLimit
        | RegexDiagnosticCode::LookbehindNestingLimit
        | RegexDiagnosticCode::BranchResetNestingLimit
        | RegexDiagnosticCode::BranchResetBranchLimit => Some(DiagnosticCode::RegexAnalysisLimit),
        RegexDiagnosticCode::UnknownModifier
        | RegexDiagnosticCode::ModifierNotAllowedForOperator
        | RegexDiagnosticCode::ConflictingCharacterSetModifiers
        | RegexDiagnosticCode::RepeatedCharacterSetModifier
        | RegexDiagnosticCode::ModifierHasNoEffect
        | RegexDiagnosticCode::ModifierRequiresPerlVersion
        | RegexDiagnosticCode::ModifierRequiresFeature => modifier_code(code),
        _ => None,
    }
}

/// Map a canonical modifier finding onto its stable client-facing identity.
fn modifier_code(code: RegexDiagnosticCode) -> Option<DiagnosticCode> {
    match code {
        RegexDiagnosticCode::UnknownModifier
        | RegexDiagnosticCode::ModifierNotAllowedForOperator
        | RegexDiagnosticCode::ConflictingCharacterSetModifiers
        | RegexDiagnosticCode::RepeatedCharacterSetModifier => {
            Some(DiagnosticCode::RegexModifierInvalid)
        }
        RegexDiagnosticCode::ModifierHasNoEffect => Some(DiagnosticCode::RegexModifierNoEffect),
        RegexDiagnosticCode::ModifierRequiresPerlVersion
        | RegexDiagnosticCode::ModifierRequiresFeature => {
            Some(DiagnosticCode::RegexModifierUnavailable)
        }
        // A structural finding carried on the modifier analysis would be a model
        // change, not a modifier problem; classify it deliberately when that happens.
        _ => None,
    }
}

/// Build one provider diagnostic, taking severity from the catalog.
fn build(code: DiagnosticCode, range: (usize, usize), message: String) -> Diagnostic {
    Diagnostic {
        range,
        severity: code.severity(),
        code: Some(code.as_str().to_string()),
        message,
        related_information: Vec::new(),
        tags: Vec::new(),
        suggestion: code.context_hint().map(str::to_string),
        fixable: false,
        critic_observation: None,
    }
}

/// Original-source spans of every embedded-code finding this projection publishes.
///
/// The compatibility `has_embedded_code` lint uses these to suppress *only* the
/// findings the canonical projection actually covered. Blanket suppression would be
/// wrong: a record whose geometry is unavailable produces no canonical finding, and
/// silently dropping the AST-flag finding there would lose a security diagnostic
/// rather than improve it.
pub(crate) fn embedded_code_spans(table: &RegexAnalysisTable) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for record in &table.records {
        let Some(pattern) = &record.pattern else {
            continue;
        };
        for diagnostic in &pattern.structural.diagnostics {
            if !is_embedded_code(diagnostic) {
                continue;
            }
            if let Some(span) = record.map_pattern_range(diagnostic.range) {
                spans.push((span.start, span.end));
            }
        }
    }
    spans
}

/// Whether `diagnostic` names executable pattern code.
fn is_embedded_code(diagnostic: &RegexDiagnostic) -> bool {
    matches!(
        diagnostic.code,
        RegexDiagnosticCode::EmbeddedCodeImmediate | RegexDiagnosticCode::EmbeddedCodeDeferred
    )
}
