//! Thin compatibility entry points; `perl-tidy` owns all native computation.
use super::{
    FormatChangeSummary, FormatConfig, FormatContext, FormatDiagnostic, FormatDiagnosticSeverity,
    FormatDisposition, FormatEngine, FormatEvidenceState, FormatExecutionStrategy, FormatIdentity,
    FormatLineEndingDisposition, FormatOutcome, FormatReasonCode, FormatRequestTarget,
    FormatResult, FormatSafetyEvidence, FormatterMode, NativePipelineCounters, TextRange,
    TypedFormatResult, projection,
};
use perl_tidy::{ExactSource, FormatRequest};

/// Former native formatter interface using mode-bearing policy and UTF-16 ranges.
#[deprecated(
    since = "0.17.0",
    note = "migrate by 0.18.0 (#9292): use perl_tidy::NativeFormatter with exact byte requests"
)]
pub trait PerlFormatter {
    /// Format a complete source document.
    fn format_document(&self, source: &str, config: &FormatConfig) -> FormatResult;

    /// Format complete line bodies selected by the former UTF-16 range policy.
    fn format_range(&self, source: &str, range: TextRange, config: &FormatConfig) -> FormatResult;
}

/// Deprecated adapter to the canonical native formatter.
///
/// This type contains no renderer, parser, syntax scanner or external executor.
/// Its range adapter makes the historical complete-line widening explicit in
/// the canonical request before projecting the returned edits to UTF-16.
#[deprecated(since = "0.17.0", note = "migrate by 0.18.0 (#9292): use perl_tidy::NativeFormatter")]
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeFormatter;

impl NativeFormatter {
    /// Create the compatibility adapter without acquiring process state.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Format a complete document and project its canonical typed outcome.
    #[must_use]
    pub fn format_document_typed(
        &self,
        source: &str,
        config: &FormatConfig,
        context: &FormatContext,
    ) -> TypedFormatResult {
        self.format_compat(source, FormatRequestTarget::Document, config, context, None)
    }

    /// Format the same canonical document request with native work counters.
    #[must_use]
    pub fn format_document_typed_with_counters(
        &self,
        source: &str,
        config: &FormatConfig,
        context: &FormatContext,
        counters: &mut NativePipelineCounters,
    ) -> TypedFormatResult {
        self.format_compat(source, FormatRequestTarget::Document, config, context, Some(counters))
    }

    /// Format a legacy UTF-16 range through explicit complete-line admission.
    #[must_use]
    pub fn format_range_typed(
        &self,
        source: &str,
        range: TextRange,
        config: &FormatConfig,
        context: &FormatContext,
    ) -> TypedFormatResult {
        self.format_compat(source, FormatRequestTarget::Range { range }, config, context, None)
    }

    /// Format the same canonical range request with native work counters.
    #[must_use]
    pub fn format_range_typed_with_counters(
        &self,
        source: &str,
        range: TextRange,
        config: &FormatConfig,
        context: &FormatContext,
        counters: &mut NativePipelineCounters,
    ) -> TypedFormatResult {
        self.format_compat(
            source,
            FormatRequestTarget::Range { range },
            config,
            context,
            Some(counters),
        )
    }

    fn format_compat(
        &self,
        source: &str,
        target: FormatRequestTarget,
        config: &FormatConfig,
        context: &FormatContext,
        counters: Option<&mut NativePipelineCounters>,
    ) -> TypedFormatResult {
        // Translate legacy selection at the boundary. No external selection is
        // passed to the native engine, and no fallback choice is made here.
        if config.mode != FormatterMode::Native {
            return selection_not_started(source, target, config, context);
        }
        let exact = ExactSource::new(source);
        let request = match target {
            FormatRequestTarget::Document => FormatRequest::document(&exact),
            FormatRequestTarget::Range { range } => projection::admit_legacy_range(&exact, range)
                .unwrap_or_else(|| FormatRequest {
                    requested: perl_tidy::FormatRequestTarget::UnadmittedRange,
                    admitted: exact.whole_document(),
                    widening: None,
                }),
        };
        let policy = config.native_policy();
        let native = perl_tidy::NativeFormatter::new();
        let typed = match counters {
            Some(counters) => {
                native.format_with_counters(&exact, &request, &policy, context, counters)
            }
            None => native.format(&exact, &request, &policy, context),
        };
        projection::project_typed(source, target, typed)
    }
}

impl PerlFormatter for NativeFormatter {
    fn format_document(&self, source: &str, config: &FormatConfig) -> FormatResult {
        self.format_document_typed(source, config, &FormatContext::default()).result
    }

    fn format_range(&self, source: &str, range: TextRange, config: &FormatConfig) -> FormatResult {
        self.format_range_typed(source, range, config, &FormatContext::default()).result
    }
}

fn selection_not_started(
    source: &str,
    target: FormatRequestTarget,
    config: &FormatConfig,
    context: &FormatContext,
) -> TypedFormatResult {
    let disabled = config.mode == FormatterMode::Off;
    let mut result = FormatResult::unchanged(source);
    if !disabled {
        result.diagnostics.push(FormatDiagnostic::new(
            "native.format.external_selection",
            FormatDiagnosticSeverity::Error,
            None,
            "the native compatibility adapter cannot execute external Perl::Tidy",
        ));
    }
    TypedFormatResult {
        result,
        outcome: FormatOutcome {
            disposition: FormatDisposition::Refused,
            reason: if disabled {
                FormatReasonCode::FormatterDisabled
            } else {
                FormatReasonCode::InvalidConfiguration
            },
            identity: FormatIdentity {
                source_id: context.source_id.clone(),
                content_digest: ExactSource::new(source).identity().content.to_string(),
                source_generation: context.source_generation,
                actual_engine: if disabled {
                    FormatEngine::Disabled
                } else {
                    FormatEngine::Unknown
                },
                requested_mode: config.mode,
                config_fingerprint: perl_tidy::native_config_fingerprint(&config.native_policy()),
            },
            target,
            execution: if disabled {
                FormatExecutionStrategy::Disabled
            } else {
                FormatExecutionStrategy::NotStarted
            },
            change: FormatChangeSummary {
                edit_count: 0,
                source_bytes_changed: 0,
                rendered_bytes_changed: 0,
                changed_lines: 0,
            },
            safety: FormatSafetyEvidence {
                parse_before: FormatEvidenceState::NotRun,
                parse_after: FormatEvidenceState::NotRun,
                literal_preservation: FormatEvidenceState::NotRun,
                utf8: FormatEvidenceState::Proven,
                line_endings: FormatLineEndingDisposition::Preserved,
            },
            next_action: Some(if disabled {
                "enable native formatting or select a supported formatter mode".to_string()
            } else {
                "select the explicit external Perl::Tidy adapter".to_string()
            }),
        },
    }
}
