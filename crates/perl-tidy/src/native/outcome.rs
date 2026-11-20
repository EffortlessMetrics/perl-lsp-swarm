use super::implementation::counters::{self, NativePipelineCounters, PipelineCollectorScope};
use super::implementation::{
    BracePlacement, ElsePlacement, FinalNewline, FormatConfig, FormatDiagnosticSeverity,
    FormatResult, FormatterMode, KeywordSpacing, NativeFormatter, TrailingComma,
    format_simple_line, indent_unit, is_perl_line_directive, is_rendered_block_boundary_line,
    literal_preserve_region, preserve_region_lines, range_includes_line, rendered_boundary_role,
    split_trailing_comment,
};
use super::{
    EditPlanError, ExactSource, FormatEdit, FormatEditPlan, FormatRequest, FormatSourceIdentity,
    SourceByteRange, TargetWidening,
};
use perl_source_identity::ContentDigest;
use serde::{Deserialize, Serialize};

const PARSE_ERROR_CODE: &str = "native.format.parse_error";
const PARSE_INCOMPLETE_CODE: &str = "native.format.parse_incomplete";
const UNSAFE_RANGE_CODE: &str = "native.format.unsafe_range";
const PARSE_PRESERVATION_CODE: &str = "native.format.parse_preservation";
const LITERAL_PRESERVE_CODE: &str = "native.format.literal_preserve_region";

/// Exhaustive terminal disposition for one formatting computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatDisposition {
    /// The computation produced one or more safe edits.
    Applied,
    /// The current eligible source already satisfies the formatter.
    NoChange,
    /// The formatter deliberately withheld edits at a named safety or support boundary.
    Refused,
    /// The computation or its instrument did not establish an eligible result.
    FailedOrNotProven,
}

/// Stable machine-readable reason for a formatter disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatReasonCode {
    /// Formatting produced safe changes.
    Applied,
    /// Formatting produced safe changes while literal-preserve regions were
    /// emitted byte-exact instead of refusing the document (#17708).
    AppliedPreservingLiterals,
    /// The eligible source was already formatted.
    AlreadyFormatted,
    /// The eligible source was already formatted and contains
    /// literal-preserve regions that were emitted byte-exact (#17708).
    AlreadyFormattedPreservingLiterals,
    /// Formatting is disabled by the effective configuration.
    FormatterDisabled,
    /// The current native formatter does not support the admitted syntax safely.
    UnsupportedSyntax,
    /// A literal or opaque region requires preservation support not yet available.
    LiteralPreservationUnsupported,
    /// The source does not parse cleanly enough to authorize formatting.
    SourceParseError,
    /// The rendered output failed the post-format parse-preservation check.
    FormattedOutputParseError,
    /// The requested range cannot be formatted safely.
    UnsafeRange,
    /// The supplied source identity is stale or superseded.
    StaleSource,
    /// The effective formatter configuration is invalid or incomplete.
    InvalidConfiguration,
    /// The formatter instrument failed independently of source eligibility.
    InstrumentFailure,
}

/// Formatter implementation that actually handled the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatEngine {
    /// Rust-native formatter implementation.
    Native,
    /// Explicit external Perl::Tidy compatibility adapter.
    ExternalLegacy,
    /// Formatting was disabled before an engine ran.
    Disabled,
    /// The implementation identity was not established.
    Unknown,
}

/// Evidence state for one formatter safety check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatEvidenceState {
    /// The check completed and established the required property.
    Proven,
    /// Literal regions were preserved byte-exact while the rest of the
    /// document was formatted normally (#17708). Distinct from `Proven`,
    /// which describes a document with no preserve regions in play.
    PreservedPartially,
    /// The check was not required for this computation.
    NotApplicable,
    /// The check did not run.
    NotRun,
    /// The check deliberately refused authorization.
    Refused,
    /// The check ran but failed or did not prove the property.
    Failed,
}

/// Disposition of source line-ending conventions after formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatLineEndingDisposition {
    /// Source and rendered output use the same line-ending convention set.
    Preserved,
    /// Formatting changed the observed line-ending convention set.
    ChangedByFormatter,
    /// Line-ending behavior was not checked.
    NotChecked,
}

/// Optional adapter provenance retained for receipt compatibility.
///
/// These strings and integers do not establish canonical logical identity or
/// currentness. Supply those through [`ExactSource::with_identity`] instead.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatContext {
    /// Optional adapter source label, such as a document URI.
    pub source_id: Option<String>,
    /// Optional adapter generation label; not a canonical source generation.
    pub source_generation: Option<u64>,
}

impl FormatContext {
    /// Retain adapter source and generation labels without promoting their authority.
    #[must_use]
    pub fn new(source_id: Option<String>, source_generation: Option<u64>) -> Self {
        Self { source_id, source_generation }
    }
}

/// Requested formatting target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FormatRequestTarget {
    /// Format the complete document.
    Document,
    /// Format one requested source byte range.
    Range {
        /// Requested exact byte interval; native entry revalidates admission.
        range: SourceByteRange,
    },
    /// An application range request failed before any exact byte target existed.
    /// The application retains the original transport request; this state never executes.
    UnadmittedRange,
}

/// Engine path actually executed for one formatting computation.
///
/// This is independent from [`FormatRequestTarget`]: a range request may still
/// execute whole-document external logic, and a document request must not be
/// inferred from returned edit shape (#7581).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatExecutionStrategy {
    /// Native formatter rendered the complete document.
    NativeDocument,
    /// Native formatter rendered one admitted range or syntax island.
    NativeRangeOrIsland,
    /// Native formatter executed a canonical multi-range plan.
    NativeMultiRangePlan,
    /// Bounded on-type indentation adjustment.
    BoundedOnTypeIndentation,
    /// External Perl::Tidy compatibility adapter ran whole-document logic.
    ExternalWholeDocument,
    /// External adapter cannot honor a partial range safely.
    ExternalPartialUnsupported,
    /// Formatting was disabled before an engine ran.
    Disabled,
    /// No engine started for this terminal outcome.
    NotStarted,
}

/// Derive the native execution strategy from the immutable requested target.
#[must_use]
pub const fn execution_for_requested_native_target(
    target: &FormatRequestTarget,
) -> FormatExecutionStrategy {
    match target {
        FormatRequestTarget::Document => FormatExecutionStrategy::NativeDocument,
        FormatRequestTarget::Range { .. } => FormatExecutionStrategy::NativeRangeOrIsland,
        FormatRequestTarget::UnadmittedRange => FormatExecutionStrategy::NotStarted,
    }
}

fn execution_for_classified_native_outcome(
    target: &FormatRequestTarget,
    reason: FormatReasonCode,
    range_admission_passed: Option<bool>,
) -> FormatExecutionStrategy {
    if matches!(reason, FormatReasonCode::InvalidConfiguration) {
        return FormatExecutionStrategy::NotStarted;
    }
    if matches!(reason, FormatReasonCode::UnsafeRange) && range_admission_passed == Some(false) {
        return FormatExecutionStrategy::NotStarted;
    }
    execution_for_requested_native_target(target)
}

/// Compatibility/report projection of caller provenance and computation identity.
///
/// `FormatOutcome::source` owns the checked source identity and generation. These
/// optional caller labels are retained for adapters and legacy result compatibility;
/// they do not establish a canonical logical source or current document revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatIdentity {
    /// Optional adapter provenance label, such as a URI or report label.
    pub source_id: Option<String>,
    /// Deterministic digest of the exact source bytes.
    pub content_digest: String,
    /// Optional adapter generation label, distinct from the checked source generation.
    pub source_generation: Option<u64>,
    /// Formatter implementation selected by the actual call path.
    pub actual_engine: FormatEngine,
    /// Formatter mode requested by configuration.
    pub requested_mode: FormatterMode,
    /// Deterministic fingerprint of every current native configuration field.
    pub config_fingerprint: String,
}

/// Bounded summary of the source transformation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatChangeSummary {
    /// Number of edits returned to the caller.
    pub edit_count: usize,
    /// Source bytes inside the changed middle region.
    pub source_bytes_changed: usize,
    /// Rendered bytes inside the changed middle region.
    pub rendered_bytes_changed: usize,
    /// Number of physical line slots whose text differs.
    pub changed_lines: usize,
}

/// Safety evidence attached to one terminal formatter outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatSafetyEvidence {
    /// Parse disposition for the input source.
    pub parse_before: FormatEvidenceState,
    /// Parse disposition for the rendered output.
    pub parse_after: FormatEvidenceState,
    /// Literal or opaque-region preservation disposition.
    pub literal_preservation: FormatEvidenceState,
    /// UTF-8 validity disposition.
    pub utf8: FormatEvidenceState,
    /// Line-ending convention disposition.
    pub line_endings: FormatLineEndingDisposition,
}

/// Typed terminal formatter outcome and its load-bearing evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatOutcome {
    /// Canonical exact source identity for every terminal outcome, including refusals.
    pub source: FormatSourceIdentity,
    /// Exhaustive terminal disposition.
    pub disposition: FormatDisposition,
    /// Stable machine-readable reason.
    pub reason: FormatReasonCode,
    /// Source, implementation, mode, and configuration identity.
    pub identity: FormatIdentity,
    /// Immutable caller request target.
    pub target: FormatRequestTarget,
    /// Exact admitted source bytes, absent when admission failed.
    pub admitted: Option<SourceByteRange>,
    /// Explicit policy provenance for any widening beyond the original request.
    pub widening: Option<TargetWidening>,
    /// Final edit coverage, independent from requested/admitted geometry.
    pub coverage: Vec<SourceByteRange>,
    /// Engine path actually executed, recorded independently from `target`.
    pub execution: FormatExecutionStrategy,
    /// Bounded edit and changed-source summary.
    pub change: FormatChangeSummary,
    /// Safety evidence used to authorize or refuse edits.
    pub safety: FormatSafetyEvidence,
    /// Optional bounded next action for callers and presentation layers.
    pub next_action: Option<String>,
}

/// Compatibility result plus its explicit terminal outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedFormatResult {
    /// Source-bound applied plan; absent on no-change, refusal or failure.
    pub plan: Option<FormatEditPlan>,
    /// Existing formatted text, edits, and diagnostics contract.
    pub result: FormatResult,
    /// Explicit disposition, reason, identity, and safety evidence.
    pub outcome: FormatOutcome,
}

impl TypedFormatResult {
    /// Consume the typed wrapper and return its source-byte result.
    #[must_use]
    pub fn into_result(self) -> FormatResult {
        self.result
    }

    /// Verify the full result against caller-owned exact source/currentness.
    pub fn apply_exact(&self, source: &ExactSource<'_>) -> Result<String, EditPlanError> {
        if self.outcome.source != *source.identity()
            || self.outcome.identity.content_digest != source.identity().content.to_string()
        {
            return Err(EditPlanError::StaleSource);
        }
        self.validate_terminal_envelope(source)?;
        if let Some(admitted) = &self.outcome.admitted {
            admitted.validate(source)?;
        }
        if self.outcome.disposition == FormatDisposition::Applied {
            let plan = self.plan.as_ref().ok_or(EditPlanError::MissingPlan)?;
            if plan.request().requested != self.outcome.target
                || Some(&plan.request().admitted) != self.outcome.admitted.as_ref()
                || plan.request().widening != self.outcome.widening
                || plan.execution() != self.outcome.execution
                || plan.coverage() != self.outcome.coverage
                || plan.edits() != self.result.edits
                || !self.result.changed
            {
                return Err(EditPlanError::InvalidOutcome);
            }
            let applied = plan.apply(source)?;
            if applied != self.result.formatted || applied == source.text() {
                return Err(EditPlanError::RenderedMismatch);
            }
            Ok(applied)
        } else {
            if self.plan.is_some()
                || self.result.changed
                || !self.result.edits.is_empty()
                || !self.outcome.coverage.is_empty()
                || self.result.formatted != source.text()
            {
                return Err(EditPlanError::InvalidOutcome);
            }
            Ok(source.text().to_string())
        }
    }

    fn validate_terminal_envelope(&self, source: &ExactSource<'_>) -> Result<(), EditPlanError> {
        let outcome = &self.outcome;
        let reason_matches = match outcome.disposition {
            FormatDisposition::Applied => matches!(
                outcome.reason,
                FormatReasonCode::Applied | FormatReasonCode::AppliedPreservingLiterals
            ),
            FormatDisposition::NoChange => matches!(
                outcome.reason,
                FormatReasonCode::AlreadyFormatted
                    | FormatReasonCode::AlreadyFormattedPreservingLiterals
            ),
            FormatDisposition::Refused => matches!(
                outcome.reason,
                FormatReasonCode::UnsupportedSyntax
                    | FormatReasonCode::LiteralPreservationUnsupported
                    | FormatReasonCode::SourceParseError
                    | FormatReasonCode::UnsafeRange
                    | FormatReasonCode::StaleSource
                    | FormatReasonCode::InvalidConfiguration
            ),
            FormatDisposition::FailedOrNotProven => matches!(
                outcome.reason,
                FormatReasonCode::FormattedOutputParseError | FormatReasonCode::InstrumentFailure
            ),
        };
        // This wrapper is the native engine's checked result. Composition-owned
        // disabled/external outcomes use FormatOutcome without claiming a native plan.
        if !reason_matches
            || outcome.identity.actual_engine != FormatEngine::Native
            || outcome.identity.requested_mode != FormatterMode::Native
            || outcome.change
                != change_summary(source.text(), &self.result.formatted, &self.result.edits)
            || outcome.safety != safety_evidence(source.text(), &self.result, outcome.reason)
        {
            return Err(EditPlanError::InvalidOutcome);
        }
        for diagnostic in &self.result.diagnostics {
            if let Some(range) = &diagnostic.range {
                range.validate(source)?;
            }
        }
        if matches!(outcome.disposition, FormatDisposition::Applied | FormatDisposition::NoChange) {
            let request = FormatRequest {
                requested: outcome.target.clone(),
                admitted: outcome.admitted.clone().ok_or(EditPlanError::InvalidOutcome)?,
                widening: outcome.widening.clone(),
            };
            request.validate(source)?;
            if !matches!(
                outcome.execution,
                FormatExecutionStrategy::NativeDocument
                    | FormatExecutionStrategy::NativeRangeOrIsland
                    | FormatExecutionStrategy::NativeMultiRangePlan
            ) || !self.result.diagnostics.is_empty()
            {
                return Err(EditPlanError::InvalidOutcome);
            }
            if matches!(outcome.target, FormatRequestTarget::Document)
                && outcome.execution != FormatExecutionStrategy::NativeDocument
            {
                return Err(EditPlanError::InvalidOutcome);
            }
            if outcome.execution == FormatExecutionStrategy::NativeDocument
                && request.admitted != source.whole_document()
            {
                return Err(EditPlanError::InvalidOutcome);
            }
        } else if !matches!(
            outcome.execution,
            FormatExecutionStrategy::NativeDocument
                | FormatExecutionStrategy::NativeRangeOrIsland
                | FormatExecutionStrategy::NativeMultiRangePlan
                | FormatExecutionStrategy::NotStarted
        ) {
            return Err(EditPlanError::InvalidOutcome);
        }
        Ok(())
    }

    /// Verify an anonymous-source result against exact predecessor bytes.
    ///
    /// Callers with logical identity or generation must use `apply_exact`; this
    /// convenience deliberately refuses to replace those identities with unknown.
    pub fn apply_to(&self, source: &str) -> Result<String, EditPlanError> {
        self.apply_exact(&ExactSource::new(source))
    }
}

impl NativeFormatter {
    /// Execute one validated source-byte request through the canonical native engine.
    #[must_use]
    pub fn format(
        &self,
        source: &ExactSource<'_>,
        request: &FormatRequest,
        config: &FormatConfig,
        context: &FormatContext,
    ) -> TypedFormatResult {
        let admission_error = request.validate(source).err();
        let admitted = admission_error.is_none();
        let execution_target = match &request.requested {
            FormatRequestTarget::Document => FormatRequestTarget::Document,
            FormatRequestTarget::Range { .. } => {
                FormatRequestTarget::Range { range: request.admitted.clone() }
            }
            FormatRequestTarget::UnadmittedRange => FormatRequestTarget::UnadmittedRange,
        };
        let result = if !admitted {
            FormatResult::unsafe_to_format(
                source.text(),
                UNSAFE_RANGE_CODE,
                "source-byte request admission failed",
            )
        } else {
            match &execution_target {
                FormatRequestTarget::Document => self.render_document(source.text(), config),
                FormatRequestTarget::Range { range } => {
                    self.render_range(source.text(), range.clone(), config)
                }
                FormatRequestTarget::UnadmittedRange => FormatResult::unsafe_to_format(
                    source.text(),
                    UNSAFE_RANGE_CODE,
                    "range request was not admitted",
                ),
            }
        };
        let mut typed = classify_native_result(
            source.text(),
            config,
            context,
            execution_target,
            result,
            Some(admitted),
        );
        typed.outcome.source = source.identity().clone();
        typed.outcome.target = request.requested.clone();
        if matches!(admission_error, Some(EditPlanError::MixedSource | EditPlanError::StaleSource))
        {
            typed.outcome.reason = FormatReasonCode::StaleSource;
            typed.outcome.execution = FormatExecutionStrategy::NotStarted;
            typed.outcome.next_action =
                next_action(FormatReasonCode::StaleSource).map(str::to_string);
            typed.outcome.safety =
                safety_evidence(source.text(), &typed.result, typed.outcome.reason);
        }
        typed.outcome.admitted = admitted.then(|| request.admitted.clone());
        typed.outcome.widening = request.widening.clone();
        // Renderer geometry comes from the exact same text. Bind its byte locations
        // to the caller's admitted logical subject/generation before authorizing a plan.
        for edit in &mut typed.result.edits {
            match SourceByteRange::new(source, edit.range.start(), edit.range.end()) {
                Ok(range) => edit.range = range,
                Err(_) => return failed_plan_result(source.text(), typed),
            }
        }
        for diagnostic in &mut typed.result.diagnostics {
            diagnostic.range = diagnostic
                .range
                .as_ref()
                .and_then(|range| SourceByteRange::new(source, range.start(), range.end()).ok());
        }
        if typed.outcome.disposition == FormatDisposition::Applied {
            match FormatEditPlan::new(
                source,
                request.clone(),
                typed.outcome.execution,
                typed.result.edits.clone(),
                &typed.result.formatted,
            ) {
                Ok(plan) => {
                    typed.outcome.coverage = plan.coverage().to_vec();
                    typed.plan = Some(plan);
                }
                Err(_) => return failed_plan_result(source.text(), typed),
            }
        }
        typed
    }

    /// Execute the same canonical request while recording opt-in work counters.
    #[must_use]
    pub fn format_with_counters(
        &self,
        source: &ExactSource<'_>,
        request: &FormatRequest,
        config: &FormatConfig,
        context: &FormatContext,
        counters: &mut NativePipelineCounters,
    ) -> TypedFormatResult {
        let scope = PipelineCollectorScope::install();
        let started = std::time::Instant::now();
        let typed = self.format(source, request, config, context);
        scope.merge_into(counters);
        observe_edits_derived(counters, &typed.result);
        observe_elapsed(counters, started.elapsed());
        typed
    }

    /// Format an anonymous exact document with the canonical byte request.
    #[must_use]
    pub fn format_document_typed(
        &self,
        source: &str,
        config: &FormatConfig,
        context: &FormatContext,
    ) -> TypedFormatResult {
        let exact = ExactSource::new(source);
        self.format(&exact, &FormatRequest::document(&exact), config, context)
    }

    /// Format an anonymous document and collect the same native pipeline counters.
    #[must_use]
    pub fn format_document_typed_with_counters(
        &self,
        source: &str,
        config: &FormatConfig,
        context: &FormatContext,
        counters: &mut NativePipelineCounters,
    ) -> TypedFormatResult {
        let exact = ExactSource::new(source);
        self.format_with_counters(
            &exact,
            &FormatRequest::document(&exact),
            config,
            context,
            counters,
        )
    }

    /// Format an exact byte range. Call `format` when independently supplied
    /// logical identity/generation must be compared with the target's identity.
    #[must_use]
    pub fn format_range_typed(
        &self,
        source: &str,
        range: SourceByteRange,
        config: &FormatConfig,
        context: &FormatContext,
    ) -> TypedFormatResult {
        let exact = ExactSource::new(source);
        let request = FormatRequest {
            requested: FormatRequestTarget::Range { range: range.clone() },
            admitted: range,
            widening: None,
        };
        self.format(&exact, &request, config, context)
    }

    /// Format an anonymous byte range while collecting pipeline work counters.
    #[must_use]
    pub fn format_range_typed_with_counters(
        &self,
        source: &str,
        range: SourceByteRange,
        config: &FormatConfig,
        context: &FormatContext,
        counters: &mut NativePipelineCounters,
    ) -> TypedFormatResult {
        let exact = ExactSource::new(source);
        let request = FormatRequest {
            requested: FormatRequestTarget::Range { range: range.clone() },
            admitted: range,
            widening: None,
        };
        self.format_with_counters(&exact, &request, config, context, counters)
    }
}

fn failed_plan_result(source: &str, mut typed: TypedFormatResult) -> TypedFormatResult {
    typed.result = FormatResult::unchanged(source);
    typed.plan = None;
    typed.outcome.disposition = FormatDisposition::FailedOrNotProven;
    typed.outcome.reason = FormatReasonCode::InstrumentFailure;
    typed.outcome.coverage.clear();
    typed.outcome.change = change_summary(source, source, &[]);
    typed.outcome.safety = safety_evidence(source, &typed.result, typed.outcome.reason);
    typed.outcome.next_action =
        Some("retain unchanged source; native plan did not reproduce its render".to_string());
    typed
}

fn observe_edits_derived(counters: &mut NativePipelineCounters, result: &FormatResult) {
    let edits = u64::try_from(result.edits.len()).unwrap_or(u64::MAX);
    let replacement_bytes =
        result.edits.iter().map(|edit| edit.new_text.len() as u64).fold(0_u64, u64::saturating_add);
    counters.observe_edits_derived(edits, replacement_bytes);
    // A typed entry may be nested inside a caller-owned collector. The
    // supplied snapshot and the outer scope must observe the same derived
    // output, just as they already do for pipeline-stage counters.
    counters::record_with(|outer| outer.observe_edits_derived(edits, replacement_bytes));
}

fn observe_elapsed(counters: &mut NativePipelineCounters, elapsed: std::time::Duration) {
    counters.observe_elapsed(elapsed);
    counters::record_with(|outer| outer.observe_elapsed(elapsed));
}

fn classify_native_result(
    source: &str,
    config: &FormatConfig,
    context: &FormatContext,
    target: FormatRequestTarget,
    result: FormatResult,
    range_admission_passed: Option<bool>,
) -> TypedFormatResult {
    let mut result = result;
    // The rendered bytes are the formatting authority. A result whose render
    // reproduces the source carries no real change, so its edits are normalized
    // away before classification; otherwise the typed outcome could report
    // `Applied` beside the zero change summary that identical bytes produce
    // (#7585). Refusals and failures already render the source unchanged with
    // no edits, so this leaves their diagnostics-driven classification intact.
    if result.formatted == source && !result.edits.is_empty() {
        result.edits.clear();
        result.changed = false;
    }
    // Typed range admission is aligned with the enclosing block context
    // (#17728): the engine's range walk reindents selected lines relative to
    // their enclosing hand-written block (#17664), but a selection that would
    // move a line at the wrong depth is not admitted — selection format must
    // not reindent a line relative to hand-written surroundings. The same
    // walk that adjudicates no-change targets
    // (`target_has_only_supported_lines`) decides: a changed range result
    // whose target it refuses is normalized back to unchanged so the
    // classifier below reports `Refused`/`UnsupportedSyntax` instead of
    // `Applied`. Document targets keep the full-document reindent contract.
    if result.changed
        && matches!(target, FormatRequestTarget::Range { .. })
        && !target_has_only_supported_lines(source, config, &target)
    {
        result.formatted = source.to_string();
        result.edits.clear();
        result.changed = false;
    }
    let classification = classify(source, config, &target, &result);
    let actual_engine = FormatEngine::Native;
    let safety = safety_evidence(source, &result, classification.reason);
    let outcome = FormatOutcome {
        source: FormatSourceIdentity::content_only(source),
        disposition: classification.disposition,
        reason: classification.reason,
        identity: FormatIdentity {
            source_id: context.source_id.clone(),
            content_digest: ContentDigest::of_bytes(source.as_bytes()).to_string(),
            source_generation: context.source_generation,
            actual_engine,
            requested_mode: FormatterMode::Native,
            config_fingerprint: native_config_fingerprint(config),
        },
        execution: execution_for_classified_native_outcome(
            &target,
            classification.reason,
            range_admission_passed,
        ),
        target,
        admitted: None,
        widening: None,
        coverage: Vec::new(),
        change: change_summary(source, &result.formatted, &result.edits),
        safety,
        next_action: next_action(classification.reason).map(str::to_string),
    };

    TypedFormatResult { result, outcome, plan: None }
}

#[derive(Debug, Clone, Copy)]
struct Classification {
    disposition: FormatDisposition,
    reason: FormatReasonCode,
}

fn classify(
    source: &str,
    config: &FormatConfig,
    target: &FormatRequestTarget,
    result: &FormatResult,
) -> Classification {
    if config.validate().is_err() {
        return Classification {
            disposition: FormatDisposition::Refused,
            reason: FormatReasonCode::InvalidConfiguration,
        };
    }

    // Document targets with preserve regions report the preserving reason
    // codes so callers can distinguish partial preservation from a plain
    // full-document format (#17708). Range targets keep the scoped refusal.
    let preserving = matches!(target, FormatRequestTarget::Document)
        && literal_preserve_region(source).is_some();

    if result.changed {
        return Classification {
            disposition: FormatDisposition::Applied,
            reason: if preserving {
                FormatReasonCode::AppliedPreservingLiterals
            } else {
                FormatReasonCode::Applied
            },
        };
    }

    let Some(diagnostic) = result.diagnostics.first() else {
        // A document target that spans a preserved-literal region (#17708) is
        // admitted on the preservation contract itself: the formatter emits
        // literal lines byte-exact, so an unchanged result over a preserve
        // region is already formatted preserving literals — even when an
        // opener line (`my $m = <<'EOF';`, bare `write;`) is not a
        // `format_simple_line` candidate and fails the rendered-layout walk
        // (#18465).
        return if preserving || target_has_only_supported_lines(source, config, target) {
            Classification {
                disposition: FormatDisposition::NoChange,
                reason: if preserving {
                    FormatReasonCode::AlreadyFormattedPreservingLiterals
                } else {
                    FormatReasonCode::AlreadyFormatted
                },
            }
        } else {
            Classification {
                disposition: FormatDisposition::Refused,
                reason: FormatReasonCode::UnsupportedSyntax,
            }
        };
    };

    match diagnostic.code.as_str() {
        LITERAL_PRESERVE_CODE => Classification {
            disposition: FormatDisposition::Refused,
            reason: FormatReasonCode::LiteralPreservationUnsupported,
        },
        PARSE_INCOMPLETE_CODE => Classification {
            disposition: FormatDisposition::FailedOrNotProven,
            reason: FormatReasonCode::InstrumentFailure,
        },
        UNSAFE_RANGE_CODE => Classification {
            disposition: FormatDisposition::Refused,
            reason: FormatReasonCode::UnsafeRange,
        },
        PARSE_ERROR_CODE => Classification {
            disposition: FormatDisposition::Refused,
            reason: FormatReasonCode::SourceParseError,
        },
        PARSE_PRESERVATION_CODE => Classification {
            disposition: FormatDisposition::FailedOrNotProven,
            reason: FormatReasonCode::FormattedOutputParseError,
        },
        _ if matches!(diagnostic.severity, FormatDiagnosticSeverity::Error) => Classification {
            disposition: FormatDisposition::FailedOrNotProven,
            reason: FormatReasonCode::InstrumentFailure,
        },
        _ => Classification {
            disposition: FormatDisposition::Refused,
            reason: FormatReasonCode::UnsupportedSyntax,
        },
    }
}

/// Existing classifier line segmentation, independent from protocol positions.
fn source_lines(source: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    let bytes = source.as_bytes();
    while offset < bytes.len() {
        match bytes[offset] {
            b'\r' | b'\n' => {
                lines.push((start, &source[start..offset]));
                offset += if bytes[offset] == b'\r' && bytes.get(offset + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = offset;
            }
            _ => offset += 1,
        }
    }
    lines.push((start, &source[start..]));
    lines.into_iter()
}

#[cfg(test)]
mod supported_lines_comment_tests {
    use super::{FormatConfig, FormatRequestTarget, target_has_only_supported_lines};

    fn supported(source: &str) -> bool {
        target_has_only_supported_lines(
            source,
            &FormatConfig::default(),
            &FormatRequestTarget::Document,
        )
    }

    #[test]
    fn comment_layout_inside_blocks_is_part_of_the_support_check() {
        // A comment left at the enclosing statement indent inside a block is
        // not in rendered form: the formatter would reindent it, so the
        // support check must not certify the document as already formatted.
        assert!(!supported("sub f {\n# explain\n    my $x = 1;\n}\n"));
        assert!(supported("sub f {\n    # explain\n    my $x = 1;\n}\n"));
        // Comments outside any block impose no layout constraint.
        assert!(supported("# top level\nmy $x = 1;\n"));
    }

    #[test]
    fn preserved_lines_are_exempt_from_comment_layout() {
        // Column-zero Perl line directives are preserved byte-for-byte by
        // the formatter wherever they appear, so they certify as-is.
        assert!(supported("sub f {\n#line 100 \"generated.pl\"\n    my $x = 1;\n}\n"));
        assert!(supported("sub f {\n# line 42 \"other.pl\"\n    my $x = 1;\n}\n"));
    }
}

#[cfg(test)]
mod line_geometry_tests {
    use super::source_lines;

    #[test]
    fn split_lines_strips_terminators_and_keeps_trailing_empty() -> Result<(), String> {
        let cases = [
            ("", vec![""]),
            ("a\nb", vec!["a", "b"]),
            ("a\rb", vec!["a", "b"]),
            ("a\r\nb", vec!["a", "b"]),
            ("a\nb\n", vec!["a", "b", ""]),
            ("a\r\nb\r\n", vec!["a", "b", ""]),
        ];
        for (source, expected) in cases {
            let actual: Vec<_> = source_lines(source).map(|(_, text)| text).collect();
            if actual != expected {
                return Err(format!("unexpected line geometry for {source:?}: {actual:?}"));
            }
        }
        Ok(())
    }
}

/// Whether every included line stays inside the formatter's supported
/// envelope, with the document-level evidence that the block boundaries the
/// renderers own are in rendered form.
///
/// Boundary *shape* is necessary but not sufficient: a hand-written block
/// reuses exactly the header and tail shapes the renderers emit, so per-line
/// recognition alone would classify a never-rendered document as
/// `AlreadyFormatted` and silence its pinned unsupported-syntax warning. The
/// evidence that separates rendered output from hand-written input is the
/// layout `push_simple_block_body_docs` produces, and only a walk over the
/// target's lines can check it:
///
/// - every `}`-prefixed tail closes the block opened at the same indent;
/// - every non-inert line inside an open block sits at least one
///   [`indent_unit`] past that block's indent — a body at or above its
///   header's indent (`sub f {` / `return 1;` / `}`) is hand-written, not
///   rendered;
/// - the walked lines end with every opened block closed. Document targets
///   always balance (this walk runs only after a clean parse).
///
/// Blank lines impose no indent constraint. Comment lines follow the layout
/// the formatter emits: inside a tracked block a comment must sit at least
/// one [`indent_unit`] past that block's indent, matching the body-line
/// evidence below. Comments outside any tracked block, Perl line directives,
/// and lines inside multiline literals are preserved byte-for-byte by the
/// formatter, so they impose no constraint.
///
/// A range target is judged with its enclosing block context (#17713): the
/// walk covers the whole document with an opener stack like the document
/// formatter's `format_safe_subset` walk, so blocks opened before the
/// range seed the stack and blocks closed after it balance it. The engine's
/// range walk may reindent selected lines relative to that stack (#17664),
/// but the typed admission refuses a changed range result when this walk
/// refuses its target (#17728): selection format must not reindent a line
/// relative to a hand-written enclosing block. Only lines
/// inside the range are adjudicated: an out-of-range line can neither
/// refuse the range nor be excused by it, it only moves the stack. Before
/// this, a range was judged on its selected lines alone, so a no-change
/// range inside an already-formatted block (a body plus its tail, a header
/// plus its body, a lone tail) still reported `UnsupportedSyntax` on the
/// second pass.
///
/// Blank, comment, and multiline-literal continuation lines impose no
/// indent constraint: they pass the formatter through unchanged wherever
/// they appear. Context lines that are not rendered boundaries still move
/// the stack with the same comment-stripped close/open shape
/// `format_safe_subset` uses, so an inline `} else { ... }` tail closes its
/// block instead of leaving a stale opener for a later range.
fn target_has_only_supported_lines(
    source: &str,
    config: &FormatConfig,
    target: &FormatRequestTarget,
) -> bool {
    let body_indent = indent_unit(config);
    let mut open_blocks: Vec<&str> = Vec::new();
    // Physical lines inside a multiline literal are literal bytes, not
    // code: exactly as in `format_safe_subset`, they never move the block
    // stack and are never adjudicated, so string content shaped like a
    // block header cannot invent an opener for a later range.
    let literal_lines = preserve_region_lines(source);
    for (line, (byte_start, text)) in source_lines(source).enumerate() {
        if literal_lines.contains(&line) {
            continue;
        }
        let included = match target {
            FormatRequestTarget::Document => true,
            FormatRequestTarget::Range { range } => {
                range_includes_line(range, byte_start, byte_start + text.len())
            }
            FormatRequestTarget::UnadmittedRange => return false,
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent_len = text.len() - text.trim_start_matches([' ', '\t']).len();
        let indent = &text[..indent_len];
        if trimmed.starts_with('#') {
            // Comment layout inside a tracked block is part of the rendered
            // form (PR #17704 review): the formatter reindents comment-only
            // lines to the block body indent, so a comment at or above its
            // header's indent is hand-written, not rendered. Preserved
            // lines (directives, literal content) and comments outside any
            // tracked block certify at whatever indent they carry.
            let preserved = is_perl_line_directive(text) || literal_lines.contains(&line);
            if !preserved
                && open_blocks
                    .last()
                    .is_some_and(|open| !indent.starts_with(&format!("{open}{body_indent}")))
            {
                return false;
            }
            continue;
        }
        // Block boundaries the renderers own. Without this a block the
        // formatter itself rendered refused on the second pass, because no
        // header or tail line is a `format_simple_line` candidate.
        if is_rendered_block_boundary_line(text, config) {
            let role = rendered_boundary_role(text);
            if !included {
                // Context only: track the enclosing stack without judging
                // lines the caller did not ask about.
                if role.closes_block {
                    open_blocks.pop();
                }
                if role.opens_block {
                    open_blocks.push(indent);
                }
                continue;
            }
            if role.closes_block {
                let closes_matched_block = open_blocks.last().is_some_and(|open| *open == indent);
                if !closes_matched_block {
                    return false;
                }
                open_blocks.pop();
            }
            let outside_renderer_layout = open_blocks
                .last()
                .is_some_and(|open| !indent.starts_with(&format!("{open}{body_indent}")));
            if outside_renderer_layout {
                return false;
            }
            if role.opens_block {
                open_blocks.push(indent);
            }
        } else if !included {
            // Context only: a non-boundary line can still close or open a
            // block (an inline `} else { ... }` tail, an anonymous-sub
            // opener). Track that with the same comment-stripped shape
            // `format_safe_subset` uses, without judging the line's support
            // or layout — lines the caller did not ask about must not leave
            // a stale opener behind for the selected range.
            let (code, _comment) = split_trailing_comment(text);
            let code = code.trim();
            if code.starts_with('}') {
                open_blocks.pop();
            }
            if code.ends_with('{') && !code.starts_with('{') {
                open_blocks.push(indent);
            }
        } else if format_simple_line(text, config).is_none()
            || open_blocks
                .last()
                .is_some_and(|open| !indent.starts_with(&format!("{open}{body_indent}")))
        {
            return false;
        }
    }
    open_blocks.is_empty()
}

fn safety_evidence(
    source: &str,
    result: &FormatResult,
    reason: FormatReasonCode,
) -> FormatSafetyEvidence {
    let (parse_before, parse_after, literal_preservation) = match reason {
        FormatReasonCode::Applied | FormatReasonCode::AlreadyFormatted => {
            (FormatEvidenceState::Proven, FormatEvidenceState::Proven, FormatEvidenceState::Proven)
        }
        FormatReasonCode::AppliedPreservingLiterals
        | FormatReasonCode::AlreadyFormattedPreservingLiterals => (
            FormatEvidenceState::Proven,
            FormatEvidenceState::Proven,
            FormatEvidenceState::PreservedPartially,
        ),
        FormatReasonCode::FormatterDisabled => {
            (FormatEvidenceState::NotRun, FormatEvidenceState::NotRun, FormatEvidenceState::NotRun)
        }
        FormatReasonCode::LiteralPreservationUnsupported => {
            (FormatEvidenceState::NotRun, FormatEvidenceState::NotRun, FormatEvidenceState::Refused)
        }
        FormatReasonCode::SourceParseError => {
            (FormatEvidenceState::Failed, FormatEvidenceState::NotRun, FormatEvidenceState::Proven)
        }
        FormatReasonCode::FormattedOutputParseError => {
            (FormatEvidenceState::Proven, FormatEvidenceState::Failed, FormatEvidenceState::Proven)
        }
        FormatReasonCode::UnsafeRange | FormatReasonCode::UnsupportedSyntax => {
            (FormatEvidenceState::NotRun, FormatEvidenceState::NotRun, FormatEvidenceState::Refused)
        }
        FormatReasonCode::StaleSource
        | FormatReasonCode::InvalidConfiguration
        | FormatReasonCode::InstrumentFailure => {
            (FormatEvidenceState::NotRun, FormatEvidenceState::NotRun, FormatEvidenceState::NotRun)
        }
    };

    FormatSafetyEvidence {
        parse_before,
        parse_after,
        literal_preservation,
        utf8: FormatEvidenceState::Proven,
        line_endings: line_ending_disposition(source, &result.formatted),
    }
}

fn next_action(reason: FormatReasonCode) -> Option<&'static str> {
    match reason {
        FormatReasonCode::FormatterDisabled => {
            Some("enable native formatting or select a supported formatter mode")
        }
        FormatReasonCode::LiteralPreservationUnsupported => Some(
            "format a range that excludes the protected construct or select explicit external compatibility",
        ),
        FormatReasonCode::SourceParseError => {
            Some("repair the Perl parse error before requesting formatting")
        }
        FormatReasonCode::FormattedOutputParseError | FormatReasonCode::InstrumentFailure => {
            Some("retain the unchanged source and report the formatter evidence")
        }
        FormatReasonCode::UnsafeRange => {
            Some("request a complete safe syntactic range or format the document")
        }
        FormatReasonCode::StaleSource => Some("retry against the current source generation"),
        FormatReasonCode::InvalidConfiguration => {
            Some("repair the formatter configuration and retry")
        }
        FormatReasonCode::Applied
        | FormatReasonCode::AlreadyFormatted
        | FormatReasonCode::AppliedPreservingLiterals
        | FormatReasonCode::AlreadyFormattedPreservingLiterals
        | FormatReasonCode::UnsupportedSyntax => None,
    }
}

/// Summarize the transformation between the source and the rendered output.
///
/// Contract: callers normalize before calling. A rendered no-op reaches this
/// function with an already-empty edit set (`classify_native_result` clears it),
/// so the `source == formatted` branch reports `edit_count: 0`. The branch does
/// not itself enforce that — passing a non-empty edit set alongside identical
/// bytes would report a non-zero `edit_count` beside zero byte deltas, which is
/// exactly the envelope inconsistency #7585 forbids. The sibling implementation
/// in `perl-lsp-rs-core` guards the same condition with `edits.is_empty() ||
/// source == formatted`, so the two answer an unnormalized call differently;
/// unifying them belongs to #7138, which owns this duplication.
fn change_summary(source: &str, formatted: &str, edits: &[FormatEdit]) -> FormatChangeSummary {
    if source == formatted {
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

    FormatChangeSummary {
        edit_count: edits.len(),
        source_bytes_changed: source_bytes.len().saturating_sub(prefix + suffix),
        rendered_bytes_changed: formatted_bytes.len().saturating_sub(prefix + suffix),
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

/// Stable identity of pure native policy, excluding engine selection and external state.
#[must_use]
pub fn native_config_fingerprint(config: &FormatConfig) -> String {
    let canonical = format!(
        "native-format-config-v2|line_width={}|indent_width={}|use_tabs={}|final_newline={}|trailing_comma={}|brace_placement={}|else_placement={}|keyword_spacing={}",
        config.line_width,
        config.indent_width,
        config.use_tabs,
        final_newline_name(config.final_newline),
        trailing_comma_name(config.trailing_comma),
        brace_placement_name(config.brace_placement),
        else_placement_name(config.else_placement),
        keyword_spacing_name(config.keyword_spacing),
    );
    stable_digest("native-format-config-v2", canonical.as_bytes())
}

fn stable_digest(prefix: &str, bytes: &[u8]) -> String {
    // Domain tags are part of the exact material; the shared identity owner owns hashing.
    format!("{prefix}:{}", ContentDigest::of_bytes(bytes))
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

const fn final_newline_name(value: FinalNewline) -> &'static str {
    match value {
        FinalNewline::Preserve => "preserve",
        FinalNewline::Insert => "insert",
        FinalNewline::Trim => "trim",
    }
}

const fn trailing_comma_name(value: TrailingComma) -> &'static str {
    match value {
        TrailingComma::Preserve => "preserve",
        TrailingComma::AddWhenWrapped => "add-when-wrapped",
    }
}

const fn brace_placement_name(value: BracePlacement) -> &'static str {
    match value {
        BracePlacement::SameLine => "same-line",
        BracePlacement::NextLine => "next-line",
    }
}

const fn else_placement_name(value: ElsePlacement) -> &'static str {
    match value {
        ElsePlacement::Cuddled => "cuddled",
        ElsePlacement::SeparateLine => "separate-line",
    }
}

const fn keyword_spacing_name(value: KeywordSpacing) -> &'static str {
    match value {
        KeywordSpacing::Space => "space",
        KeywordSpacing::Compact => "compact",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Negative control for #7585: a rendered no-op can never classify as
    /// `Applied`.
    ///
    /// The engine currently pushes a range edit only when the line text really
    /// differs, so the inconsistency is unreachable through the public
    /// formatter today. That makes the guard coincidental rather than enforced,
    /// so this test drives `classify_native_result` directly with the one
    /// result shape that would break the envelope. Deleting the normalization
    /// in `classify_native_result` fails this test: `classify` reads
    /// `result.changed` and would report `Applied` beside the zero change
    /// summary that identical bytes always produce.
    #[test]
    fn a_rendered_no_op_edit_set_is_normalized_before_classification() {
        let source = "my $value = 1;\n";
        let result = FormatResult {
            formatted: source.to_string(),
            edits: vec![FormatEdit::new(
                perl_tdd_support::must(SourceByteRange::new(&ExactSource::new(source), 0, 14)),
                "my $value = 1;".to_string(),
            )],
            changed: true,
            diagnostics: Vec::new(),
        };

        let typed = classify_native_result(
            source,
            &FormatConfig::default(),
            &FormatContext::default(),
            FormatRequestTarget::Document,
            result,
            None,
        );

        assert_eq!(typed.outcome.disposition, FormatDisposition::NoChange);
        assert_eq!(typed.outcome.reason, FormatReasonCode::AlreadyFormatted);
        assert!(
            typed.result.edits.is_empty(),
            "a no-op edit set must be normalized away, not returned to the caller"
        );
        assert_eq!(typed.outcome.change.edit_count, 0);
        assert_eq!(typed.outcome.change.source_bytes_changed, 0);
        assert_eq!(typed.outcome.change.rendered_bytes_changed, 0);
        assert_eq!(typed.outcome.change.changed_lines, 0);
        assert_eq!(typed.result.formatted, source, "normalization must not alter rendered bytes");
    }

    /// Normalization must not rescue a refusal into a legitimate no-change:
    /// refusals already render the source unchanged with no edits, so their
    /// diagnostic still decides the terminal outcome.
    #[test]
    fn normalization_leaves_diagnostic_driven_refusals_intact() {
        let source = "my $value = 1;\n";
        let result = FormatResult::unsafe_to_format(
            source,
            UNSAFE_RANGE_CODE,
            "native range formatting refused because the requested byte range is invalid",
        );

        let typed = classify_native_result(
            source,
            &FormatConfig::default(),
            &FormatContext::default(),
            FormatRequestTarget::Document,
            result,
            None,
        );

        assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
        assert_eq!(typed.outcome.reason, FormatReasonCode::UnsafeRange);
        assert!(typed.result.edits.is_empty());
        assert_eq!(typed.outcome.change.edit_count, 0);
    }

    /// A real change still classifies as `Applied` and still carries non-zero
    /// change evidence, so the normalization cannot be satisfied by reporting
    /// everything as a no-change.
    #[test]
    fn a_real_render_difference_still_applies_with_non_zero_evidence() {
        let typed = NativeFormatter::new().format_document_typed(
            "my$x=1;\n",
            &FormatConfig::default(),
            &FormatContext::default(),
        );

        assert_eq!(typed.outcome.disposition, FormatDisposition::Applied);
        assert_eq!(typed.outcome.reason, FormatReasonCode::Applied);
        assert!(!typed.result.edits.is_empty());
        assert_eq!(typed.outcome.change.edit_count, typed.result.edits.len());
        assert!(typed.outcome.change.source_bytes_changed > 0);
        assert!(typed.outcome.change.rendered_bytes_changed > 0);
    }

    /// Regression control for #17300: an admitted bare-print document needs
    /// no edits or unsupported-syntax warning. Unsupported layouts retain
    /// their separate refusal boundary.
    #[test]
    fn parse_clean_unchanged_source_is_already_formatted() {
        let source = "print $x + $y;\n";
        let result = NativeFormatter::new().format_document_typed(
            source,
            &FormatConfig::default(),
            &FormatContext::default(),
        );
        assert_eq!(result.outcome.disposition, FormatDisposition::NoChange);
        assert_eq!(result.outcome.reason, FormatReasonCode::AlreadyFormatted);
        assert!(result.result.edits.is_empty());
        assert!(!result.result.changed);
    }

    /// An admitted bare-print range that needs no edits is a successful no-op.
    #[test]
    fn parse_clean_unchanged_range_is_already_formatted() {
        let source = "my $value = 1;\nprint $x + $y;\n";
        let range = perl_tdd_support::must(SourceByteRange::new(
            &ExactSource::new(source),
            source.find("print").unwrap_or(0),
            source.len(),
        ));
        let result = NativeFormatter::new().format_range_typed(
            source,
            range,
            &FormatConfig::default(),
            &FormatContext::default(),
        );
        assert_eq!(result.outcome.disposition, FormatDisposition::NoChange);
        assert_eq!(result.outcome.reason, FormatReasonCode::AlreadyFormatted);
        assert!(result.result.edits.is_empty());
    }

    #[test]
    fn unsupported_unchanged_source_is_refused() {
        let result = NativeFormatter::new().format_document_typed(
            "print 1;;;\n",
            &FormatConfig::default(),
            &FormatContext::default(),
        );
        assert_eq!(result.outcome.disposition, FormatDisposition::Refused);
        assert_eq!(result.outcome.reason, FormatReasonCode::UnsupportedSyntax);
    }

    #[test]
    fn malformed_deserialized_ranges_are_refused_before_formatting() {
        let source = "my $value = 1;\n";
        for (start, end) in [(8, 2), (0, 99), (99, 99)] {
            let mut value = perl_tdd_support::must(serde_json::to_value(
                ExactSource::new(source).whole_document(),
            ));
            value["start"] = serde_json::json!(start);
            value["end"] = serde_json::json!(end);
            let range = perl_tdd_support::must(serde_json::from_value(value));
            let result = NativeFormatter::new().format_range_typed(
                source,
                range,
                &FormatConfig::default(),
                &FormatContext::default(),
            );
            assert_eq!(result.outcome.disposition, FormatDisposition::Refused);
            assert_eq!(result.outcome.reason, FormatReasonCode::UnsafeRange);
            assert_eq!(result.outcome.execution, FormatExecutionStrategy::NotStarted);
            assert!(!result.result.changed);
            assert!(result.plan.is_none());
        }
    }

    #[test]
    fn invalid_policy_records_no_execution() {
        let config = FormatConfig { indent_width: u32::MAX, ..FormatConfig::default() };
        let document = NativeFormatter::new().format_document_typed(
            "my$x=1;\n",
            &config,
            &FormatContext::default(),
        );
        assert_eq!(document.outcome.reason, FormatReasonCode::InvalidConfiguration);
        assert_eq!(document.outcome.execution, FormatExecutionStrategy::NotStarted);
        assert_eq!(document.outcome.identity.actual_engine, FormatEngine::Native);

        let range = ExactSource::new("my$x=1;\n").whole_document();
        let range_result = NativeFormatter::new().format_range_typed(
            "my$x=1;\n",
            range,
            &config,
            &FormatContext::default(),
        );
        assert_eq!(range_result.outcome.reason, FormatReasonCode::InvalidConfiguration);
        assert_eq!(range_result.outcome.execution, FormatExecutionStrategy::NotStarted);
    }

    #[test]
    fn successful_native_calls_record_target_matched_execution() {
        let document = NativeFormatter::new().format_document_typed(
            "my$x=1;\n",
            &FormatConfig::default(),
            &FormatContext::default(),
        );
        assert_eq!(document.outcome.execution, FormatExecutionStrategy::NativeDocument);

        let range = ExactSource::new("my$x=1;\n").whole_document();
        let range_result = NativeFormatter::new().format_range_typed(
            "my$x=1;\n",
            range,
            &FormatConfig::default(),
            &FormatContext::default(),
        );
        assert_eq!(range_result.outcome.execution, FormatExecutionStrategy::NativeRangeOrIsland);
    }

    #[test]
    fn parser_budget_exhaustion_is_not_a_source_parse_verdict() {
        let result = FormatResult::unsafe_to_format(
            "fixture",
            PARSE_INCOMPLETE_CODE,
            "parsing terminated early",
        );
        let classification =
            classify("fixture", &FormatConfig::default(), &FormatRequestTarget::Document, &result);
        assert_eq!(classification.disposition, FormatDisposition::FailedOrNotProven);
        assert_eq!(classification.reason, FormatReasonCode::InstrumentFailure);
    }

    #[test]
    fn changed_line_count_normalizes_line_endings_without_allocating() {
        assert_eq!(count_changed_lines("first\r\nsecond\r\n", "first\nsecond\n"), 0);
        assert_eq!(count_changed_lines("first\nsecond\n", "first\nchanged\n"), 1);
        assert_eq!(count_changed_lines("first\n", "first\nsecond\nthird\n"), 2);
        assert_eq!(count_changed_lines("first\nsecond\nthird\n", "first\n"), 2);
    }
}
