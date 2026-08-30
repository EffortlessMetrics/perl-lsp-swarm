//! Code formatting support for Perl parsing workflow pipeline.
//!
//! This module is the request-independent formatting policy layer. It keeps the
//! existing [`FormattedDocument`] API as a projection while exposing
//! [`FormattingDecision`] for callers that must distinguish applied, no-change,
//! refused, disabled, and failed/not-proven outcomes.

#[path = "legacy.rs"]
mod legacy;

use crate::hashing::fnv1a64_hex;
use crate::providers::formatting::range_admission::{
    AdmittedFormatRange, RangePositionError, SourceGeometry, admit_format_range,
};
pub use crate::providers::formatting_types::{
    FormatPosition, FormatRange, FormatTextEdit, FormattedDocument, FormattingOptions,
};
use crate::tooling::perltidy::native::{
    FormatChangeSummary, FormatContext, FormatDisposition, FormatEngine, FormatEvidenceState,
    FormatIdentity, FormatLineEndingDisposition, FormatOutcome, FormatReasonCode,
    FormatRequestTarget, FormatSafetyEvidence, NativePipelineCounters, TypedFormatResult,
};
use crate::tooling::perltidy::{
    BracePlacement, ElsePlacement, FinalNewline, FormatConfig, FormatterMode, KeywordSpacing,
    NativeFormatter, TextPosition, TextRange, TrailingComma,
};
use perl_subprocess_runtime::SubprocessRuntime;
use serde::{Deserialize, Serialize};

/// Re-export PerlTidyConfig from perl-lsp-perltidy for convenience.
pub use perl_lsp_perltidy::PerlTidyConfig;

/// Formatting error.
#[derive(Debug, thiserror::Error)]
pub enum FormattingError {
    #[error(
        "perltidy not found: {0}\n\nTo install perltidy:\n  - Recommended: cpanm Perl::Tidy\n  - CPAN: cpan Perl::Tidy\n  - Debian/Ubuntu: apt-get install perltidy\n  - RedHat/Fedora: yum install perltidy\n  - macOS: brew install perltidy\n  - Windows: cpanm Perl::Tidy"
    )]
    /// Perltidy executable was not found or could not be started.
    PerltidyNotFound(String),

    /// Perltidy returned a non-success status.
    #[error("perltidy error (check Perl syntax): {0}")]
    PerltidyError(String),

    /// Perltidy returned bytes that are not valid UTF-8 source text.
    #[error("perltidy returned invalid UTF-8 output")]
    InvalidOutputEncoding,

    /// I/O error during formatting operations.
    #[error("IO error: {0}")]
    IoError(String),

    /// Native formatting reached a failed/not-proven terminal outcome.
    #[error("native formatting did not prove a safe result: {0:?}")]
    NativeNotProven(FormatReasonCode),
}

impl FormattingError {
    /// Return a stable machine-readable error kind string.
    #[must_use]
    pub fn error_kind(&self) -> &'static str {
        match self {
            Self::PerltidyNotFound(_) => "perltidy_not_found",
            Self::PerltidyError(_) => "perltidy_error",
            Self::InvalidOutputEncoding => "invalid_output_encoding",
            Self::IoError(_) => "io_error",
            Self::NativeNotProven(_) => "native_formatting_not_proven",
        }
    }
}

impl From<legacy::FormattingError> for FormattingError {
    fn from(error: legacy::FormattingError) -> Self {
        match error {
            legacy::FormattingError::PerltidyNotFound(message) => Self::PerltidyNotFound(message),
            legacy::FormattingError::PerltidyError(message) => Self::PerltidyError(message),
            legacy::FormattingError::InvalidOutputEncoding => Self::InvalidOutputEncoding,
            legacy::FormattingError::IoError(message) => Self::IoError(message),
        }
    }
}

impl perl_parser_core::ErrorClass for FormattingError {
    fn error_class(&self) -> perl_parser_core::ErrorCategory {
        match self {
            Self::PerltidyNotFound(_) | Self::IoError(_) => perl_parser_core::ErrorCategory::Infra,
            Self::PerltidyError(_) => perl_parser_core::ErrorCategory::UserError,
            Self::InvalidOutputEncoding | Self::NativeNotProven(_) => {
                perl_parser_core::ErrorCategory::Bug
            }
        }
    }
}

/// A formatted document paired with the explicit decision that authorized or
/// withheld its edits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormattingDecision {
    /// Formatted text and LSP-ready edits.
    #[serde(skip)]
    pub document: FormattedDocument,
    /// Terminal outcome, reason, identity, and safety evidence.
    pub outcome: FormatOutcome,
}

impl FormattingDecision {
    /// Consume the decision and return the compatibility document projection.
    #[must_use]
    pub fn into_document(self) -> FormattedDocument {
        self.document
    }
}

/// Code formatter using native formatting with an explicit external perltidy adapter.
pub struct FormattingProvider<R> {
    inner: legacy::FormattingProvider<R>,
    perltidy_config: Option<PerlTidyConfig>,
    mode: FormatterMode,
}

impl<R> FormattingProvider<R> {
    /// Create a provider whose default engine is the Rust-native formatter.
    pub fn new(runtime: R) -> Self {
        Self {
            inner: legacy::FormattingProvider::new(runtime),
            perltidy_config: None,
            mode: FormatterMode::Native,
        }
    }

    /// Set a custom external perltidy executable path.
    pub fn with_perltidy_path(mut self, path: String) -> Self {
        self.inner = self.inner.with_perltidy_path(path);
        self
    }

    /// Set native style and external perltidy compatibility configuration.
    pub fn with_perltidy_config(mut self, config: PerlTidyConfig) -> Self {
        self.perltidy_config = Some(config.clone());
        self.inner = self.inner.with_perltidy_config(config);
        self
    }

    /// Select the requested formatter mode.
    pub fn with_formatter_mode(mut self, mode: FormatterMode) -> Self {
        self.mode = mode;
        self
    }
}

impl<R: SubprocessRuntime> FormattingProvider<R> {
    /// Format the entire document through the compatibility projection.
    pub fn format_document(
        &self,
        content: &str,
        options: &FormattingOptions,
    ) -> Result<FormattedDocument, FormattingError> {
        self.format_document_decision(content, options, &FormatContext::default())
            .map(FormattingDecision::into_document)
    }

    /// Format the entire document and retain the explicit terminal decision.
    pub fn format_document_decision(
        &self,
        content: &str,
        options: &FormattingOptions,
        context: &FormatContext,
    ) -> Result<FormattingDecision, FormattingError> {
        self.document_decision_with_counters(content, options, context, None)
    }

    /// Format the entire document and record deterministic native-pipeline
    /// work counters through the exact same decision path. The counters ride
    /// the production typed entry, so one LSP request must always observe
    /// exactly one pipeline invocation (#10302 NPC-003).
    pub fn format_document_decision_with_counters(
        &self,
        content: &str,
        options: &FormattingOptions,
        context: &FormatContext,
        counters: &mut NativePipelineCounters,
    ) -> Result<FormattingDecision, FormattingError> {
        self.document_decision_with_counters(content, options, context, Some(counters))
    }

    fn document_decision_with_counters(
        &self,
        content: &str,
        options: &FormattingOptions,
        context: &FormatContext,
        counters: Option<&mut NativePipelineCounters>,
    ) -> Result<FormattingDecision, FormattingError> {
        match self.mode {
            FormatterMode::Native | FormatterMode::Compat => {
                self.native_document_decision(content, options, context, counters)
            }
            FormatterMode::ExternalLegacy => self.external_document_decision(
                content,
                options,
                context,
                FormatRequestTarget::Document,
            ),
            FormatterMode::Off => Ok(refused_decision(
                content,
                self.mode,
                FormatEngine::Disabled,
                FormatRequestTarget::Document,
                context,
                FormatReasonCode::FormatterDisabled,
                "enable formatting or select a supported formatter mode",
            )),
        }
    }

    /// Format a range through the compatibility projection.
    pub fn format_range(
        &self,
        content: &str,
        range: &FormatRange,
        options: &FormattingOptions,
    ) -> Result<FormattedDocument, FormattingError> {
        self.format_range_decision(content, range, options, &FormatContext::default())
            .map(FormattingDecision::into_document)
    }

    /// Format a range and retain the explicit terminal decision.
    ///
    /// One strict admission maps both requested UTF-16 endpoints onto the
    /// current source before any engine runs. Invalid lines, characters past a
    /// line end, surrogate splits, and reversed ranges refuse as one typed
    /// result; nothing is clamped and no fallback may run for them.
    pub fn format_range_decision(
        &self,
        content: &str,
        range: &FormatRange,
        options: &FormattingOptions,
        context: &FormatContext,
    ) -> Result<FormattingDecision, FormattingError> {
        self.range_decision_with_counters(content, range, options, context, None)
    }

    /// Format a range and record deterministic native-pipeline work counters
    /// through the exact same decision path (#10302 NPC-003).
    pub fn format_range_decision_with_counters(
        &self,
        content: &str,
        range: &FormatRange,
        options: &FormattingOptions,
        context: &FormatContext,
        counters: &mut NativePipelineCounters,
    ) -> Result<FormattingDecision, FormattingError> {
        self.range_decision_with_counters(content, range, options, context, Some(counters))
    }

    fn range_decision_with_counters(
        &self,
        content: &str,
        range: &FormatRange,
        options: &FormattingOptions,
        context: &FormatContext,
        counters: Option<&mut NativePipelineCounters>,
    ) -> Result<FormattingDecision, FormattingError> {
        let target = FormatRequestTarget::Range { range: to_native_range(range) };
        let geometry = SourceGeometry::new(content);
        let admitted = match admit_format_range(&geometry, content, range) {
            Ok(admitted) => admitted,
            Err(error) => {
                return Ok(refused_decision(
                    content,
                    self.mode,
                    FormatEngine::Unknown,
                    target,
                    context,
                    FormatReasonCode::UnsafeRange,
                    error.next_action(),
                ));
            }
        };

        match self.mode {
            FormatterMode::Native | FormatterMode::Compat => {
                self.native_range_decision(content, &geometry, admitted, options, context, counters)
            }
            FormatterMode::ExternalLegacy if is_whole_document_range(content, range) => {
                self.external_document_decision(content, options, context, target)
            }
            FormatterMode::ExternalLegacy => Ok(refused_decision(
                content,
                self.mode,
                FormatEngine::Unknown,
                target,
                context,
                FormatReasonCode::UnsafeRange,
                "external Perl::Tidy compatibility currently supports whole-document formatting only",
            )),
            FormatterMode::Off => Ok(refused_decision(
                content,
                self.mode,
                FormatEngine::Disabled,
                target,
                context,
                FormatReasonCode::FormatterDisabled,
                "enable formatting or select a supported formatter mode",
            )),
        }
    }

    fn native_document_decision(
        &self,
        content: &str,
        options: &FormattingOptions,
        context: &FormatContext,
        counters: Option<&mut NativePipelineCounters>,
    ) -> Result<FormattingDecision, FormattingError> {
        let mut config = native_format_config(options, self.perltidy_config.as_ref(), true);
        config.mode = self.mode;
        let mut typed = match counters {
            Some(counters) => NativeFormatter::new()
                .format_document_typed_with_counters(content, &config, context, counters),
            None => NativeFormatter::new().format_document_typed(content, &config, context),
        };
        bind_lsp_options(&mut typed.outcome.identity.config_fingerprint, options);
        project_native_document(content, options, typed)
    }

    fn native_range_decision(
        &self,
        content: &str,
        geometry: &SourceGeometry,
        admitted: AdmittedFormatRange,
        options: &FormattingOptions,
        context: &FormatContext,
        counters: Option<&mut NativePipelineCounters>,
    ) -> Result<FormattingDecision, FormattingError> {
        let native_range = to_native_range(&admitted.requested);
        let mut config = native_format_config(options, self.perltidy_config.as_ref(), false);
        config.mode = self.mode;
        let mut typed = match counters {
            Some(counters) => NativeFormatter::new().format_range_typed_with_counters(
                content,
                native_range,
                &config,
                context,
                counters,
            ),
            None => {
                NativeFormatter::new().format_range_typed(content, native_range, &config, context)
            }
        };
        bind_lsp_options(&mut typed.outcome.identity.config_fingerprint, options);
        project_native_range(content, geometry, &admitted, options, typed)
    }

    fn external_document_decision(
        &self,
        content: &str,
        options: &FormattingOptions,
        context: &FormatContext,
        target: FormatRequestTarget,
    ) -> Result<FormattingDecision, FormattingError> {
        let document =
            self.inner.format_document(content, options).map_err(FormattingError::from)?;
        let disposition = if document.edits.is_empty() {
            FormatDisposition::NoChange
        } else {
            FormatDisposition::Applied
        };
        let reason = if document.edits.is_empty() {
            FormatReasonCode::AlreadyFormatted
        } else {
            FormatReasonCode::Applied
        };
        let outcome = provider_outcome(ProviderOutcomeInput {
            source: content,
            formatted: &document.text,
            edits: &document.edits,
            requested_mode: self.mode,
            actual_engine: FormatEngine::ExternalLegacy,
            target,
            context,
            disposition,
            reason,
            config_fingerprint: external_config_fingerprint(self.perltidy_config.as_ref(), options),
            safety: FormatSafetyEvidence {
                parse_before: FormatEvidenceState::NotRun,
                parse_after: FormatEvidenceState::NotRun,
                literal_preservation: FormatEvidenceState::NotRun,
                utf8: FormatEvidenceState::Proven,
                line_endings: line_ending_disposition(content, &document.text),
            },
            next_action: None,
        });

        Ok(FormattingDecision { document, outcome })
    }
}

fn project_native_document(
    content: &str,
    options: &FormattingOptions,
    typed: TypedFormatResult,
) -> Result<FormattingDecision, FormattingError> {
    let TypedFormatResult { result, mut outcome } = typed;
    match outcome.disposition {
        FormatDisposition::Refused => {
            return Ok(FormattingDecision { document: unchanged_document(content), outcome });
        }
        FormatDisposition::FailedOrNotProven => {
            return Ok(FormattingDecision { document: unchanged_document(content), outcome });
        }
        FormatDisposition::Applied | FormatDisposition::NoChange => {}
    }

    let formatted = apply_lsp_whitespace_options_from_source(&result.formatted, options, content);
    let edits = if formatted == content {
        Vec::new()
    } else if formatted == result.formatted {
        result.edits.into_iter().map(native_edit_to_format_edit).collect()
    } else {
        vec![FormatTextEdit {
            range: FormatRange::whole_document(content),
            new_text: formatted.clone(),
        }]
    };
    finalize_outcome(&mut outcome, content, &formatted, &edits);

    Ok(FormattingDecision { document: FormattedDocument { text: formatted, edits }, outcome })
}

fn project_native_range(
    content: &str,
    geometry: &SourceGeometry,
    admitted: &AdmittedFormatRange,
    options: &FormattingOptions,
    typed: TypedFormatResult,
) -> Result<FormattingDecision, FormattingError> {
    let TypedFormatResult { result, mut outcome } = typed;
    match outcome.disposition {
        FormatDisposition::Refused => {
            return Ok(FormattingDecision { document: unchanged_document(content), outcome });
        }
        FormatDisposition::FailedOrNotProven => {
            return Ok(FormattingDecision { document: unchanged_document(content), outcome });
        }
        FormatDisposition::Applied => {
            let span = match admitted.allowed_edit_span(content, geometry) {
                Ok(span) => span,
                Err(_) => {
                    return Ok(unproven_range_projection(content, outcome));
                }
            };
            return match contained_native_edits(content, geometry, span, result.edits) {
                Ok((_, native_updated)) if native_updated == result.formatted => {
                    let Some((replacement, updated)) =
                        projected_native_range(content, admitted, &result.formatted, options)
                    else {
                        return Ok(unproven_range_projection(content, outcome));
                    };
                    if content
                        .get(admitted.start_byte..admitted.end_byte)
                        .is_some_and(|slice| replacement == slice)
                    {
                        finalize_outcome(&mut outcome, content, content, &[]);
                        return Ok(FormattingDecision {
                            document: unchanged_document(content),
                            outcome,
                        });
                    }
                    let edits = vec![FormatTextEdit {
                        range: admitted.requested.clone(),
                        new_text: replacement,
                    }];
                    finalize_outcome(&mut outcome, content, &updated, &edits);
                    Ok(FormattingDecision {
                        document: FormattedDocument { text: updated, edits },
                        outcome,
                    })
                }
                Ok(_) | Err(_) => Ok(unproven_range_projection(content, outcome)),
            };
        }
        FormatDisposition::NoChange => {}
    }

    // One admitted plan governs projection: after a legitimate native
    // no-change, LSP whitespace options apply strictly inside the admitted
    // bytes. Refusals, failures, stale snapshots, and ambiguous geometry never
    // reach this line.
    if let Some((replacement, updated)) = whitespace_within_admitted(content, admitted, options) {
        let edits =
            vec![FormatTextEdit { range: admitted.requested.clone(), new_text: replacement }];
        finalize_outcome(&mut outcome, content, &updated, &edits);
        return Ok(FormattingDecision {
            document: FormattedDocument { text: updated, edits },
            outcome,
        });
    }
    finalize_outcome(&mut outcome, content, content, &[]);
    Ok(FormattingDecision { document: unchanged_document(content), outcome })
}

/// Why an applied native projection was downgraded to one typed not-proven
/// outcome with no edits.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EditContainmentRejection {
    /// An edit endpoint did not resolve against the admitted source.
    UnmappableEditPosition(RangePositionError),
    /// An edit replaced a reversed byte interval.
    ReversedEdit,
    /// An edit escaped the canonical admitted span.
    EscapedAdmittedSpan { start_byte: usize, end_byte: usize, span: (usize, usize) },
}

/// Downgrade an applied projection to one typed not-proven outcome with no
/// edits so a containment failure can never escape as a successful edit set.
fn unproven_range_projection(content: &str, mut outcome: FormatOutcome) -> FormattingDecision {
    outcome.disposition = FormatDisposition::FailedOrNotProven;
    outcome.reason = FormatReasonCode::InstrumentFailure;
    outcome.next_action =
        Some("retain the unchanged source and report the formatter evidence".to_string());
    FormattingDecision { document: unchanged_document(content), outcome }
}

/// Map engine-emitted range edits onto source bytes and verify each stays
/// inside the exact admitted span.
fn contained_native_edits(
    content: &str,
    geometry: &SourceGeometry,
    span: (usize, usize),
    native_edits: Vec<crate::tooling::perltidy::TextEdit>,
) -> Result<(Vec<FormatTextEdit>, String), EditContainmentRejection> {
    let mut mapped = Vec::with_capacity(native_edits.len());
    let mut byte_spans = Vec::with_capacity(native_edits.len());
    for edit in native_edits {
        let start_byte = geometry
            .byte_offset(content, edit.range.start.line, edit.range.start.character)
            .map_err(EditContainmentRejection::UnmappableEditPosition)?;
        let end_byte = geometry
            .byte_offset(content, edit.range.end.line, edit.range.end.character)
            .map_err(EditContainmentRejection::UnmappableEditPosition)?;
        if end_byte < start_byte {
            return Err(EditContainmentRejection::ReversedEdit);
        }
        if start_byte < span.0 || end_byte > span.1 {
            return Err(EditContainmentRejection::EscapedAdmittedSpan {
                start_byte,
                end_byte,
                span,
            });
        }
        byte_spans.push((start_byte, end_byte, edit.new_text.clone()));
        mapped.push(FormatTextEdit {
            range: FormatRange::new(
                FormatPosition::new(edit.range.start.line, edit.range.start.character),
                FormatPosition::new(edit.range.end.line, edit.range.end.character),
            ),
            new_text: edit.new_text,
        });
    }
    byte_spans.sort_by_key(|(start_byte, _, _)| *start_byte);
    let mut updated = String::with_capacity(content.len());
    let mut cursor = 0;
    for (start_byte, end_byte, new_text) in byte_spans {
        if start_byte < cursor {
            return Err(EditContainmentRejection::ReversedEdit);
        }
        updated.push_str(&content[cursor..start_byte]);
        updated.push_str(&new_text);
        cursor = end_byte;
    }
    updated.push_str(&content[cursor..]);
    Ok((mapped, updated))
}

/// Apply LSP whitespace options to the native result without allowing them to
/// escape the exact admitted source interval.
fn projected_native_range(
    content: &str,
    admitted: &AdmittedFormatRange,
    formatted: &str,
    options: &FormattingOptions,
) -> Option<(String, String)> {
    let suffix = content.get(admitted.end_byte..)?;
    if !formatted.ends_with(suffix) {
        return None;
    }
    let formatted_end = formatted.len().checked_sub(suffix.len())?;
    let native_slice = formatted.get(admitted.start_byte..formatted_end)?;
    let projected = apply_lsp_whitespace_options_with_eof(
        native_slice,
        options,
        admitted.end_byte == content.len(),
        admitted_end_is_line_end(formatted, formatted_end),
        content.get(..admitted.start_byte).is_some_and(|prefix| prefix.ends_with(['\r', '\n'])),
        content,
    );
    let mut updated = String::with_capacity(formatted.len() - native_slice.len() + projected.len());
    updated.push_str(&formatted[..admitted.start_byte]);
    updated.push_str(&projected);
    updated.push_str(suffix);
    Some((projected, updated))
}

/// Apply LSP whitespace options strictly inside the admitted bytes.
///
/// The replacement covers exactly the admitted interval — endpoints and end
/// exclusivity are honored, no line is widened into the edit, and final-newline
/// options act only when the admitted target reaches true EOF. Returns the
/// replacement text and the fully spliced document when anything changed.
fn whitespace_within_admitted(
    content: &str,
    admitted: &AdmittedFormatRange,
    options: &FormattingOptions,
) -> Option<(String, String)> {
    // An empty admitted interval stays a no-op for trimming (nothing is
    // selectable), yet a zero-width target at true EOF still honors the
    // final-newline option exactly like the applied-native projection of the
    // same request.
    let slice = content.get(admitted.start_byte..admitted.end_byte)?;
    let mut projected = String::with_capacity(slice.len());
    if options.trim_trailing_whitespace.unwrap_or(false) {
        projected.push_str(&trim_trailing_whitespace_in_slice(
            slice,
            admitted_end_is_line_end(content, admitted.end_byte),
        ));
    } else {
        projected.push_str(slice);
    }
    if admitted.end_byte == content.len() {
        if options.trim_final_newlines.unwrap_or(false) {
            // A bare CR ends a line under the shared geometry, so strip the
            // complete trailing terminator â€” never leave a dangling CR after
            // popping an LF from a CRLF pair.
            while projected.ends_with(['\r', '\n']) {
                projected.pop();
            }
        }
        if options.insert_final_newline.unwrap_or(false)
            && !projected_tail_is_terminated(
                &projected,
                content
                    .get(..admitted.start_byte)
                    .is_some_and(|prefix| prefix.ends_with(['\r', '\n'])),
            )
        {
            projected.push_str(inferred_line_ending(content));
        }
    }
    if projected == slice {
        return None;
    }

    let mut updated = String::with_capacity(content.len() - slice.len() + projected.len());
    updated.push_str(&content[..admitted.start_byte]);
    updated.push_str(&projected);
    updated.push_str(&content[admitted.end_byte..]);
    Some((projected, updated))
}

fn finalize_outcome(
    outcome: &mut FormatOutcome,
    source: &str,
    formatted: &str,
    edits: &[FormatTextEdit],
) {
    if edits.is_empty() {
        outcome.disposition = FormatDisposition::NoChange;
        outcome.reason = FormatReasonCode::AlreadyFormatted;
    } else {
        outcome.disposition = FormatDisposition::Applied;
        outcome.reason = FormatReasonCode::Applied;
    }
    outcome.change = change_summary(source, formatted, edits);
    outcome.safety.line_endings = line_ending_disposition(source, formatted);
}

struct ProviderOutcomeInput<'a> {
    source: &'a str,
    formatted: &'a str,
    edits: &'a [FormatTextEdit],
    requested_mode: FormatterMode,
    actual_engine: FormatEngine,
    target: FormatRequestTarget,
    context: &'a FormatContext,
    disposition: FormatDisposition,
    reason: FormatReasonCode,
    config_fingerprint: String,
    safety: FormatSafetyEvidence,
    next_action: Option<String>,
}

fn provider_outcome(input: ProviderOutcomeInput<'_>) -> FormatOutcome {
    FormatOutcome {
        disposition: input.disposition,
        reason: input.reason,
        identity: FormatIdentity {
            source_id: input.context.source_id.clone(),
            content_digest: stable_digest("source-v1", input.source.as_bytes()),
            source_generation: input.context.source_generation,
            actual_engine: input.actual_engine,
            requested_mode: input.requested_mode,
            config_fingerprint: input.config_fingerprint,
        },
        target: input.target,
        change: change_summary(input.source, input.formatted, input.edits),
        safety: input.safety,
        next_action: input.next_action,
    }
}

fn refused_decision(
    content: &str,
    requested_mode: FormatterMode,
    actual_engine: FormatEngine,
    target: FormatRequestTarget,
    context: &FormatContext,
    reason: FormatReasonCode,
    next_action: &str,
) -> FormattingDecision {
    let outcome = provider_outcome(ProviderOutcomeInput {
        source: content,
        formatted: content,
        edits: &[],
        requested_mode,
        actual_engine,
        target,
        context,
        disposition: FormatDisposition::Refused,
        reason,
        config_fingerprint: stable_digest(
            "format-config-v1",
            formatter_mode_name(requested_mode).as_bytes(),
        ),
        safety: FormatSafetyEvidence {
            parse_before: FormatEvidenceState::NotRun,
            parse_after: FormatEvidenceState::NotRun,
            literal_preservation: FormatEvidenceState::NotRun,
            utf8: FormatEvidenceState::Proven,
            line_endings: FormatLineEndingDisposition::Preserved,
        },
        next_action: Some(next_action.to_string()),
    });

    FormattingDecision { document: unchanged_document(content), outcome }
}

fn native_format_config(
    options: &FormattingOptions,
    perltidy_config: Option<&PerlTidyConfig>,
    allow_final_newline: bool,
) -> FormatConfig {
    let mut config = FormatConfig {
        indent_width: options.tab_size,
        use_tabs: !options.insert_spaces,
        final_newline: if allow_final_newline {
            if options.trim_final_newlines.unwrap_or(false) {
                FinalNewline::Trim
            } else if options.insert_final_newline.unwrap_or(false) {
                FinalNewline::Insert
            } else {
                FinalNewline::Preserve
            }
        } else {
            FinalNewline::Preserve
        },
        ..FormatConfig::default()
    };

    if let Some(perltidy_config) = perltidy_config {
        if let Some(width) = perltidy_config.maximum_line_length {
            config.line_width = width;
        }
        if let Some(indent_columns) = perltidy_config.indent_columns {
            config.indent_width = indent_columns;
        }
        if let Some(tabs) = perltidy_config.tabs {
            config.use_tabs = tabs;
        }
        if let Some(opening_brace_on_new_line) = perltidy_config.opening_brace_on_new_line {
            config.brace_placement = if opening_brace_on_new_line {
                BracePlacement::NextLine
            } else {
                BracePlacement::SameLine
            };
        }
        if let Some(cuddled_else) = perltidy_config.cuddled_else {
            config.else_placement =
                if cuddled_else { ElsePlacement::Cuddled } else { ElsePlacement::SeparateLine };
        }
        if let Some(space_after_keyword) = perltidy_config.space_after_keyword {
            config.keyword_spacing =
                if space_after_keyword { KeywordSpacing::Space } else { KeywordSpacing::Compact };
        }
        if let Some(add_trailing_commas) = perltidy_config.add_trailing_commas {
            config.trailing_comma = if add_trailing_commas {
                TrailingComma::AddWhenWrapped
            } else {
                TrailingComma::Preserve
            };
        }
    }

    config
}

fn native_edit_to_format_edit(edit: crate::tooling::perltidy::TextEdit) -> FormatTextEdit {
    FormatTextEdit {
        range: FormatRange::new(
            FormatPosition::new(edit.range.start.line, edit.range.start.character),
            FormatPosition::new(edit.range.end.line, edit.range.end.character),
        ),
        new_text: edit.new_text,
    }
}

fn apply_lsp_whitespace_options(content: &str, options: &FormattingOptions) -> String {
    apply_lsp_whitespace_options_with_eof(content, options, true, true, false, content)
}

fn apply_lsp_whitespace_options_from_source(
    content: &str,
    options: &FormattingOptions,
    line_ending_source: &str,
) -> String {
    apply_lsp_whitespace_options_with_eof(content, options, true, true, false, line_ending_source)
}

fn apply_lsp_whitespace_options_with_eof(
    content: &str,
    options: &FormattingOptions,
    allow_final_newline: bool,
    trim_tail: bool,
    prefix_terminated: bool,
    line_ending_source: &str,
) -> String {
    let mut output = content.to_string();
    let document_line_ending = inferred_line_ending(line_ending_source);

    if options.trim_trailing_whitespace.unwrap_or(false) {
        output = trim_trailing_whitespace_in_slice(&output, trim_tail);
    }
    if allow_final_newline && options.trim_final_newlines.unwrap_or(false) {
        output.truncate(output.trim_end_matches(['\r', '\n']).len());
    }
    if allow_final_newline
        && options.insert_final_newline.unwrap_or(false)
        && !projected_tail_is_terminated(&output, prefix_terminated)
    {
        output.push_str(document_line_ending);
    }

    output
}

fn inferred_line_ending(content: &str) -> &'static str {
    let bytes = content.as_bytes();
    let Some(last_lf) = bytes.iter().rposition(|byte| *byte == b'\n') else {
        return "\n";
    };

    if last_lf > 0 && bytes[last_lf - 1] == b'\r' { "\r\n" } else { "\n" }
}

/// Whether the document tail is already line-terminated after projection.
///
/// A non-empty replacement decides from its own final byte; an empty
/// replacement inherits the termination state of the untouched prefix before
/// the admitted interval, so an already-terminated document never grows a
/// blank line and an unterminated one still receives its final newline.
fn projected_tail_is_terminated(projected: &str, prefix_terminated: bool) -> bool {
    match projected.as_bytes().last() {
        Some(byte) => matches!(byte, b'\r' | b'\n'),
        None => prefix_terminated,
    }
}

/// Trim trailing spaces and tabs before every line separator fully contained
/// in the slice.
///
/// The residual tail after the last contained separator counts as trailing
/// only when `trim_tail` holds — that is, when the admitted interval ends at
/// a true document line boundary (EOF or directly before a separator).
/// Otherwise the tail continues mid-line in the surrounding document and its
/// whitespace is interior content that must survive.
fn trim_trailing_whitespace_in_slice(content: &str, trim_tail: bool) -> String {
    let mut result = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let mut line_start = 0;
    while let Some(offset) =
        bytes[line_start..].iter().position(|byte| matches!(byte, b'\r' | b'\n'))
    {
        let newline = line_start + offset;
        let ending_len =
            usize::from(bytes[newline] == b'\r' && bytes.get(newline + 1) == Some(&b'\n')) + 1;
        result.push_str(content[line_start..newline].trim_end_matches([' ', '\t']));
        result.push_str(&content[newline..newline + ending_len]);
        line_start = newline + ending_len;
    }
    if trim_tail {
        result.push_str(content[line_start..].trim_end_matches([' ', '\t']));
    } else {
        result.push_str(&content[line_start..]);
    }
    result
}

/// True when `end_byte` ends the last line's content of `content`: either the
/// offset is EOF or the next byte begins a line separator.
fn admitted_end_is_line_end(content: &str, end_byte: usize) -> bool {
    match content.as_bytes().get(end_byte) {
        None => true,
        Some(byte) => matches!(byte, b'\r' | b'\n'),
    }
}

fn unchanged_document(content: &str) -> FormattedDocument {
    FormattedDocument { text: content.to_string(), edits: Vec::new() }
}

fn to_native_range(range: &FormatRange) -> TextRange {
    TextRange::new(
        TextPosition::new(range.start.line, range.start.character),
        TextPosition::new(range.end.line, range.end.character),
    )
}

fn is_whole_document_range(content: &str, range: &FormatRange) -> bool {
    let whole = FormatRange::whole_document(content);
    range.start.line == whole.start.line
        && range.start.character == whole.start.character
        && (range.end.line > whole.end.line
            || (range.end.line == whole.end.line && range.end.character >= whole.end.character))
}

fn change_summary(source: &str, formatted: &str, edits: &[FormatTextEdit]) -> FormatChangeSummary {
    if edits.is_empty() || source == formatted {
        return FormatChangeSummary {
            edit_count: edits.len(),
            source_bytes_changed: 0,
            rendered_bytes_changed: 0,
            changed_lines: 0,
        };
    }

    let source_bytes = source.as_bytes();
    let formatted_bytes = formatted.as_bytes();
    let prefix =
        source_bytes.iter().zip(formatted_bytes).take_while(|(left, right)| left == right).count();
    let suffix_limit = source_bytes.len().min(formatted_bytes.len()).saturating_sub(prefix);
    let suffix = source_bytes
        .iter()
        .rev()
        .zip(formatted_bytes.iter().rev())
        .take(suffix_limit)
        .take_while(|(left, right)| left == right)
        .count();
    let changed_lines = count_changed_lines(source, formatted);
    // Prefix/suffix accounting reports 0 on the shorter side for pure
    // insertions or deletions. Mirror the non-zero span so callers that require
    // both source and rendered evidence still see the edit magnitude.
    let mut source_bytes_changed = source_bytes.len().saturating_sub(prefix + suffix);
    let mut rendered_bytes_changed = formatted_bytes.len().saturating_sub(prefix + suffix);
    if source_bytes_changed == 0 {
        source_bytes_changed = rendered_bytes_changed;
    }
    if rendered_bytes_changed == 0 {
        rendered_bytes_changed = source_bytes_changed;
    }

    FormatChangeSummary {
        edit_count: edits.len(),
        source_bytes_changed,
        rendered_bytes_changed,
        changed_lines,
    }
}

fn count_changed_lines(source: &str, formatted: &str) -> usize {
    let mut source_lines = source.lines();
    let mut formatted_lines = formatted.lines();
    let mut changed_lines = 0;
    loop {
        match (source_lines.next(), formatted_lines.next()) {
            (Some(source_line), Some(formatted_line)) => {
                changed_lines += usize::from(source_line != formatted_line);
            }
            (Some(_), None) => return changed_lines + 1 + source_lines.count(),
            (None, Some(_)) => return changed_lines + 1 + formatted_lines.count(),
            (None, None) => return changed_lines,
        }
    }
}

fn bind_lsp_options(fingerprint: &mut String, options: &FormattingOptions) {
    let canonical = format!(
        "native-lsp-config-v1|native={fingerprint}|tab_size={}|insert_spaces={}|trim_trailing_whitespace={:?}|insert_final_newline={:?}|trim_final_newlines={:?}",
        options.tab_size,
        options.insert_spaces,
        options.trim_trailing_whitespace,
        options.insert_final_newline,
        options.trim_final_newlines,
    );
    *fingerprint = stable_digest("native-lsp-config-v1", canonical.as_bytes());
}

fn external_config_fingerprint(
    config: Option<&PerlTidyConfig>,
    options: &FormattingOptions,
) -> String {
    let canonical = match config {
        Some(config) => format!(
            "external-format-config-v1|maximum_line_length={:?}|indent_columns={:?}|tabs={:?}|opening_brace_on_new_line={:?}|cuddled_else={:?}|space_after_keyword={:?}|add_trailing_commas={:?}|vertical_alignment={:?}|block_comment_indentation={:?}|profile={:?}|extra_args={:?}|timeout_secs={}|tab_size={}|insert_spaces={}|trim_trailing_whitespace={:?}|insert_final_newline={:?}|trim_final_newlines={:?}",
            config.maximum_line_length,
            config.indent_columns,
            config.tabs,
            config.opening_brace_on_new_line,
            config.cuddled_else,
            config.space_after_keyword,
            config.add_trailing_commas,
            config.vertical_alignment,
            config.block_comment_indentation,
            config.profile,
            config.extra_args,
            config.timeout_secs,
            options.tab_size,
            options.insert_spaces,
            options.trim_trailing_whitespace,
            options.insert_final_newline,
            options.trim_final_newlines,
        ),
        None => format!(
            "external-format-config-v1|none|tab_size={}|insert_spaces={}|trim_trailing_whitespace={:?}|insert_final_newline={:?}|trim_final_newlines={:?}",
            options.tab_size,
            options.insert_spaces,
            options.trim_trailing_whitespace,
            options.insert_final_newline,
            options.trim_final_newlines,
        ),
    };
    stable_digest("external-format-config-v1", canonical.as_bytes())
}

fn stable_digest(prefix: &str, bytes: &[u8]) -> String {
    format!("{prefix}:{}", fnv1a64_hex(bytes).trim_start_matches("fnv1a64:"))
}

fn line_ending_disposition(source: &str, formatted: &str) -> FormatLineEndingDisposition {
    if line_ending_kind(source) == line_ending_kind(formatted) {
        FormatLineEndingDisposition::Preserved
    } else {
        FormatLineEndingDisposition::ChangedByFormatter
    }
}

fn line_ending_kind(source: &str) -> (bool, bool, bool) {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut has_lf = false;
    let mut has_crlf = false;
    let mut has_cr = false;

    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                has_crlf = true;
                index += 2;
            }
            b'\r' => {
                has_cr = true;
                index += 1;
            }
            b'\n' => {
                has_lf = true;
                index += 1;
            }
            _ => index += 1,
        }
    }

    (has_lf, has_crlf, has_cr)
}

const fn formatter_mode_name(mode: FormatterMode) -> &'static str {
    match mode {
        FormatterMode::Native => "native",
        FormatterMode::Compat => "compat",
        FormatterMode::ExternalLegacy => "external-legacy",
        FormatterMode::Off => "off",
    }
}

#[cfg(test)]
mod decision_projection_tests {
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn failed_native_outcome_retains_complete_evidence() -> Result<(), FormattingError> {
        let source = "my $x = 1;\n";
        let config = FormatConfig::default();
        let mut typed = NativeFormatter::new().format_document_typed(
            source,
            &config,
            &FormatContext::new(Some("fixture.pl".to_string()), Some(9)),
        );
        typed.outcome.disposition = FormatDisposition::FailedOrNotProven;
        typed.outcome.reason = FormatReasonCode::InstrumentFailure;

        let decision = project_native_document(
            source,
            &FormattingOptions {
                tab_size: 4,
                insert_spaces: true,
                trim_trailing_whitespace: None,
                insert_final_newline: None,
                trim_final_newlines: None,
            },
            typed,
        )?;

        assert_eq!(decision.outcome.disposition, FormatDisposition::FailedOrNotProven);
        assert_eq!(decision.outcome.identity.source_generation, Some(9));
        assert_eq!(decision.outcome.identity.source_id.as_deref(), Some("fixture.pl"));
        assert!(
            decision.document.edits.is_empty(),
            "a failed/not-proven outcome must not emit edits"
        );
        Ok(())
    }

    fn range_options() -> FormattingOptions {
        FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        }
    }

    /// A syntactically valid fixture supplies a fully-formed outcome envelope;
    /// tests then pin only the disposition under projection.
    fn base_outcome(disposition: FormatDisposition, reason: FormatReasonCode) -> FormatOutcome {
        let mut typed = NativeFormatter::new().format_document_typed(
            "my $x = 1;\n",
            &FormatConfig::default(),
            &FormatContext::default(),
        );
        typed.outcome.disposition = disposition;
        typed.outcome.reason = reason;
        typed.outcome
    }

    fn applied_typed(source: &str, edit: crate::tooling::perltidy::TextEdit) -> TypedFormatResult {
        TypedFormatResult {
            result: crate::tooling::perltidy::FormatResult {
                formatted: source.to_string(),
                changed: true,
                edits: vec![edit],
                diagnostics: Vec::new(),
            },
            outcome: base_outcome(FormatDisposition::Applied, FormatReasonCode::Applied),
        }
    }

    #[test]
    fn escaping_native_edit_downgrades_to_not_proven_with_no_edits() {
        let source = "abc\ndef\n";
        // Admitted target covers line zero only; the fabricated engine edit
        // reaches into line one, so projection must fail closed.
        let admitted = admitted_fixture(source, 0, 0, 0, 3);
        let geometry = SourceGeometry::new(source);
        let escaping_edit = crate::tooling::perltidy::TextEdit {
            range: crate::tooling::perltidy::TextRange::new(
                crate::tooling::perltidy::TextPosition::new(0, 0),
                crate::tooling::perltidy::TextPosition::new(1, 3),
            ),
            new_text: "XYZ".to_string(),
        };
        let typed = applied_typed(source, escaping_edit);

        let decision = project_native_range(source, &geometry, &admitted, &range_options(), typed)
            .expect("projection must not error on containment failure");

        assert_eq!(decision.outcome.disposition, FormatDisposition::FailedOrNotProven);
        assert_eq!(decision.outcome.reason, FormatReasonCode::InstrumentFailure);
        assert!(decision.document.edits.is_empty(), "an escaping edit must not be projected");
        assert_eq!(decision.document.text, source, "failed containment keeps the source");
    }

    #[test]
    fn whitespace_projection_cannot_rewrite_bytes_outside_the_admitted_interval() {
        let source = "# trailing   \nsecond\n";
        let geometry = SourceGeometry::new(source);
        let mut options = range_options();
        options.trim_trailing_whitespace = Some(true);

        // The trailing spaces sit outside the requested interval, so the
        // projection must stay a legitimate no-change.
        let partial = admitted_fixture(source, 0, 0, 0, 10);
        let decision =
            project_native_range(source, &geometry, &partial, &options, no_change_typed(source))
                .expect("projection must not error");
        assert_eq!(decision.outcome.disposition, FormatDisposition::NoChange);
        assert!(decision.document.edits.is_empty());
        assert_eq!(decision.document.text, source);

        // Covering the whole line body admits exactly those bytes.
        let full_body = admitted_fixture(source, 0, 0, 0, 13);
        let decision =
            project_native_range(source, &geometry, &full_body, &options, no_change_typed(source))
                .expect("projection must not error");
        assert_eq!(decision.outcome.disposition, FormatDisposition::Applied);
        assert_eq!(decision.document.edits.len(), 1);
        assert_eq!(decision.document.edits[0].range.start.character, 0);
        assert_eq!(decision.document.edits[0].range.end.character, 13);
        assert_eq!(decision.document.edits[0].new_text, "# trailing");
        assert_eq!(decision.document.text, "# trailing\nsecond\n");
    }

    #[test]
    fn trim_trailing_whitespace_treats_only_true_line_boundaries_as_trailing() {
        // A mid-line admitted boundary continues the surrounding line, so
        // the spaces it selects are interior document whitespace, not
        // trailing; a boundary at the line-content end still trims exactly
        // like the whole-document option contract.
        let source = "ab  \ncd\n";
        let geometry = SourceGeometry::new(source);
        let mut options = range_options();
        options.trim_trailing_whitespace = Some(true);

        let mid_line = admitted_fixture(source, 0, 0, 0, 3);
        let decision =
            project_native_range(source, &geometry, &mid_line, &options, no_change_typed(source))
                .expect("projection must not error");
        assert_eq!(decision.outcome.disposition, FormatDisposition::NoChange);
        assert!(
            decision.document.edits.is_empty(),
            "interior whitespace must survive a mid-line boundary"
        );
        assert_eq!(decision.document.text, source);

        let line_end = admitted_fixture(source, 0, 0, 0, 4);
        let decision =
            project_native_range(source, &geometry, &line_end, &options, no_change_typed(source))
                .expect("projection must not error");
        assert_eq!(decision.outcome.disposition, FormatDisposition::Applied);
        assert_eq!(decision.document.text, "ab\ncd\n");
        assert_eq!(decision.document.edits.len(), 1);
        assert_eq!(decision.document.edits[0].new_text, "ab");
    }

    #[test]
    fn applied_native_projection_respects_the_same_interior_boundary() {
        let source = "abc   def\nzzz\n";
        let geometry = SourceGeometry::new(source);
        let mut options = range_options();
        options.trim_trailing_whitespace = Some(true);
        let edit = |new_text: &str, ec: u32| crate::tooling::perltidy::TextEdit {
            range: crate::tooling::perltidy::TextRange::new(
                crate::tooling::perltidy::TextPosition::new(0, 0),
                crate::tooling::perltidy::TextPosition::new(0, ec),
            ),
            new_text: new_text.to_string(),
        };

        // The fabricated applied edit keeps trailing spaces before the
        // admitted mid-line end; trimming them would delete live line
        // content beyond the boundary.
        let mid_line = admitted_fixture(source, 0, 0, 0, 6);
        let typed = applied_typed("abZ   def\nzzz\n", edit("abZ   ", 6));
        let decision = project_native_range(source, &geometry, &mid_line, &options, typed)
            .expect("projection must not error");
        assert_eq!(decision.outcome.disposition, FormatDisposition::Applied);
        assert_eq!(decision.document.text, "abZ   def\nzzz\n");
        assert_eq!(decision.document.edits.len(), 1);
        assert_eq!(decision.document.edits[0].new_text, "abZ   ");

        // At a true line-content boundary the same option still trims the tail.
        let line_end = admitted_fixture(source, 0, 0, 0, 9);
        let typed = applied_typed("abc   X  \nzzz\n", edit("abc   X  ", 9));
        let decision = project_native_range(source, &geometry, &line_end, &options, typed)
            .expect("projection must not error");
        assert_eq!(decision.outcome.disposition, FormatDisposition::Applied);
        assert_eq!(decision.document.text, "abc   X\nzzz\n");
        assert_eq!(decision.document.edits[0].new_text, "abc   X");
    }

    #[test]
    fn final_newline_options_act_only_when_the_target_reaches_true_eof() {
        let geometry_source = "# t   \nmore";
        let geometry = SourceGeometry::new(geometry_source);
        let mut options = range_options();
        options.insert_final_newline = Some(true);

        // A mid-document slice never grows a newline.
        let mid_document = admitted_fixture(geometry_source, 0, 0, 0, 5);
        let decision = project_native_range(
            geometry_source,
            &geometry,
            &mid_document,
            &options,
            no_change_typed(geometry_source),
        )
        .expect("projection must not error");
        assert_eq!(decision.outcome.disposition, FormatDisposition::NoChange);

        // A target reaching true EOF of an unterminated document inserts the
        // final newline exactly at the admitted boundary.
        let to_eof = admitted_fixture(geometry_source, 1, 0, 1, 4);
        let decision = project_native_range(
            geometry_source,
            &geometry,
            &to_eof,
            &options,
            no_change_typed(geometry_source),
        )
        .expect("projection must not error");
        assert_eq!(decision.outcome.disposition, FormatDisposition::Applied);
        assert_eq!(decision.document.text, "# t   \nmore\n");
        assert_eq!(decision.document.edits.len(), 1);
        assert_eq!(decision.document.edits[0].range.start.line, 1);
        assert_eq!(decision.document.edits[0].range.start.character, 0);
        assert_eq!(decision.document.edits[0].range.end.character, 4);
        assert_eq!(decision.document.edits[0].new_text, "more\n");
    }

    #[test]
    fn trim_final_newlines_at_true_eof_strips_the_complete_terminator() {
        // Under the shared geometry a bare CR ends a line, so trimming the
        // final newline at true EOF must remove the complete terminator â€”
        // including any directly preceding carriage return â€” exactly like the
        // replaced legacy fallback. A dangling CR would keep a separator in
        // the user document while the outcome claims Applied.
        for (label, source, expected) in
            [("LF", "x\n", "x"), ("CRLF", "x\r\n", "x"), ("bare CR", "x\r", "x")]
        {
            let geometry = SourceGeometry::new(source);
            let mut options = range_options();
            options.trim_final_newlines = Some(true);

            // The end line is the terminal empty line, so the admitted target
            // reaches true EOF and its exclusive end covers the separator.
            let admitted = admitted_fixture(source, 0, 0, 1, 0);
            let decision = project_native_range(
                source,
                &geometry,
                &admitted,
                &options,
                no_change_typed(source),
            )
            .expect("projection must not error");

            assert_eq!(decision.outcome.disposition, FormatDisposition::Applied, "{label}");
            assert_eq!(
                decision.document.text, expected,
                "{label} trim must leave no dangling terminator"
            );
            assert_eq!(decision.document.edits.len(), 1, "{label}");
            assert_eq!(decision.document.edits[0].new_text, expected, "{label}");
        }
    }

    fn admitted_fixture(source: &str, sl: u32, sc: u32, el: u32, ec: u32) -> AdmittedFormatRange {
        let geometry = SourceGeometry::new(source);
        admit_format_range(
            &geometry,
            source,
            &FormatRange::new(FormatPosition::new(sl, sc), FormatPosition::new(el, ec)),
        )
        .expect("test fixture range must admit")
    }

    #[test]
    fn empty_true_eof_range_inserts_one_final_newline_into_an_unterminated_document() {
        // A zero-width target at true EOF is admissible, so the no-change
        // projection must honor the final-newline option exactly once there;
        // an already-terminated document must never grow a blank line.
        let mut options = range_options();
        options.insert_final_newline = Some(true);

        let unterminated = "my $x = 1;";
        let geometry = SourceGeometry::new(unterminated);
        let eof_zero_width = admitted_fixture(unterminated, 0, 10, 0, 10);
        let decision = project_native_range(
            unterminated,
            &geometry,
            &eof_zero_width,
            &options,
            no_change_typed(unterminated),
        )
        .expect("projection must not error");
        assert_eq!(decision.outcome.disposition, FormatDisposition::Applied);
        assert_eq!(decision.document.text, "my $x = 1;\n");
        assert_eq!(decision.document.edits.len(), 1);
        assert_eq!(decision.document.edits[0].range, eof_zero_width.requested);
        assert_eq!(decision.document.edits[0].new_text, "\n");

        let terminated = "my $x = 1;\n";
        let geometry = SourceGeometry::new(terminated);
        let eof_zero_width = admitted_fixture(terminated, 1, 0, 1, 0);
        let decision = project_native_range(
            terminated,
            &geometry,
            &eof_zero_width,
            &options,
            no_change_typed(terminated),
        )
        .expect("projection must not error");
        assert_eq!(decision.outcome.disposition, FormatDisposition::NoChange);
        assert!(decision.document.edits.is_empty(), "no extra blank line");
        assert_eq!(decision.document.text, terminated);

        // An interior zero-width interval stays a pure no-op.
        let interior_source = "ab\ncd\n";
        let geometry = SourceGeometry::new(interior_source);
        let interior = admitted_fixture(interior_source, 0, 1, 0, 1);
        let decision = project_native_range(
            interior_source,
            &geometry,
            &interior,
            &options,
            no_change_typed(interior_source),
        )
        .expect("projection must not error");
        assert_eq!(decision.outcome.disposition, FormatDisposition::NoChange);
        assert_eq!(decision.document.text, interior_source);
    }

    #[test]
    fn no_change_true_eof_range_reinserts_the_source_crlf_terminator() {
        let mut options = range_options();
        options.trim_trailing_whitespace = Some(true);
        options.insert_final_newline = Some(true);
        options.trim_final_newlines = Some(true);
        let source = "my $x = 1;  \r\n";
        let geometry = SourceGeometry::new(source);
        let admitted = admitted_fixture(source, 0, 0, 1, 0);

        let decision =
            project_native_range(source, &geometry, &admitted, &options, no_change_typed(source))
                .expect("projection must not error");

        assert_eq!(decision.outcome.disposition, FormatDisposition::Applied);
        assert_eq!(decision.document.text, "my $x = 1;\r\n");
        assert_eq!(decision.document.edits.len(), 1);
        assert_eq!(decision.document.edits[0].new_text, "my $x = 1;\r\n");
        assert!(!decision.document.edits[0].new_text.ends_with("\n\n"));
    }

    #[test]
    fn empty_eof_range_produces_one_outcome_regardless_of_native_disposition() {
        // One-outcome policy: the identical zero-width EOF request must yield
        // the identical document whether the native engine reported Applied
        // or NoChange; only the engine evidence differs, never the edit set.
        let mut options = range_options();
        options.insert_final_newline = Some(true);

        let unterminated = "my $x = 1;";
        let geometry = SourceGeometry::new(unterminated);
        let admitted = admitted_fixture(unterminated, 0, 10, 0, 10);
        let zero_width_noop_edit = crate::tooling::perltidy::TextEdit {
            range: crate::tooling::perltidy::TextRange::new(
                crate::tooling::perltidy::TextPosition::new(0, 10),
                crate::tooling::perltidy::TextPosition::new(0, 10),
            ),
            new_text: String::new(),
        };

        let via_no_change = project_native_range(
            unterminated,
            &geometry,
            &admitted,
            &options,
            no_change_typed(unterminated),
        )
        .expect("projection must not error");
        let via_applied = project_native_range(
            unterminated,
            &geometry,
            &admitted,
            &options,
            applied_typed(unterminated, zero_width_noop_edit.clone()),
        )
        .expect("projection must not error");
        assert_eq!(via_no_change.document.text, via_applied.document.text);
        assert_eq!(via_no_change.document.edits.len(), via_applied.document.edits.len());
        for (no_change_edit, applied_edit) in
            via_no_change.document.edits.iter().zip(via_applied.document.edits.iter())
        {
            assert_eq!(no_change_edit.range, applied_edit.range);
            assert_eq!(no_change_edit.new_text, applied_edit.new_text);
        }
        assert_eq!(via_no_change.outcome.disposition, FormatDisposition::Applied);
        assert_eq!(via_applied.outcome.disposition, FormatDisposition::Applied);
        assert_eq!(via_applied.outcome.reason, FormatReasonCode::Applied);

        let terminated = "my $x = 1;\n";
        let geometry = SourceGeometry::new(terminated);
        let admitted = admitted_fixture(terminated, 1, 0, 1, 0);
        let zero_width_noop_edit = crate::tooling::perltidy::TextEdit {
            range: crate::tooling::perltidy::TextRange::new(
                crate::tooling::perltidy::TextPosition::new(1, 0),
                crate::tooling::perltidy::TextPosition::new(1, 0),
            ),
            new_text: String::new(),
        };
        let via_no_change = project_native_range(
            terminated,
            &geometry,
            &admitted,
            &options,
            no_change_typed(terminated),
        )
        .expect("projection must not error");
        let via_applied = project_native_range(
            terminated,
            &geometry,
            &admitted,
            &options,
            applied_typed(terminated, zero_width_noop_edit),
        )
        .expect("projection must not error");
        assert_eq!(via_no_change.document.text, via_applied.document.text);
        assert_eq!(via_no_change.document.edits.len(), via_applied.document.edits.len());
        assert!(
            via_no_change.document.edits.is_empty(),
            "a terminated document must not grow a blank line on either path"
        );
        assert_eq!(via_no_change.document.text, terminated);
    }

    fn no_change_typed(source: &str) -> TypedFormatResult {
        TypedFormatResult {
            result: crate::tooling::perltidy::FormatResult {
                formatted: source.to_string(),
                changed: false,
                edits: Vec::new(),
                diagnostics: Vec::new(),
            },
            outcome: base_outcome(FormatDisposition::NoChange, FormatReasonCode::AlreadyFormatted),
        }
    }
}
