use super::implementation::{
    BracePlacement, ElsePlacement, FinalNewline, FormatConfig, FormatDiagnosticSeverity,
    format_simple_line, range_includes_line,
    FormatResult, FormatterMode, KeywordSpacing, NativeFormatter, PerlFormatter, TextEdit,
    TextRange, TrailingComma,
};
use serde::{Deserialize, Serialize};

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
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
    /// The eligible source was already formatted.
    AlreadyFormatted,
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

/// Caller-supplied source identity independent from editor and LSP types.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatContext {
    /// Optional logical source identifier supplied by the caller.
    pub source_id: Option<String>,
    /// Optional caller-owned source generation.
    pub source_generation: Option<u64>,
}

impl FormatContext {
    /// Create a source context from an optional logical identifier and generation.
    #[must_use]
    pub fn new(source_id: Option<String>, source_generation: Option<u64>) -> Self {
        Self { source_id, source_generation }
    }
}

/// Requested formatting target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FormatRequestTarget {
    /// Format the complete document.
    Document,
    /// Format one requested source range.
    Range {
        /// Requested UTF-16 range.
        range: TextRange,
    },
}

/// Stable subject, implementation, and configuration identity for one computation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatIdentity {
    /// Optional caller-owned logical source identity.
    pub source_id: Option<String>,
    /// Deterministic digest of the exact source bytes.
    pub content_digest: String,
    /// Optional caller-owned source generation.
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
    /// Exhaustive terminal disposition.
    pub disposition: FormatDisposition,
    /// Stable machine-readable reason.
    pub reason: FormatReasonCode,
    /// Source, implementation, mode, and configuration identity.
    pub identity: FormatIdentity,
    /// Requested document or range target.
    pub target: FormatRequestTarget,
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
    /// Existing formatted text, edits, and diagnostics contract.
    pub result: FormatResult,
    /// Explicit disposition, reason, identity, and safety evidence.
    pub outcome: FormatOutcome,
}

impl TypedFormatResult {
    /// Consume the typed wrapper and return the compatibility result.
    #[must_use]
    pub fn into_result(self) -> FormatResult {
        self.result
    }
}

impl NativeFormatter {
    /// Format a complete document and return an explicit typed terminal outcome.
    #[must_use]
    pub fn format_document_typed(
        &self,
        source: &str,
        config: &FormatConfig,
        context: &FormatContext,
    ) -> TypedFormatResult {
        let result = <Self as PerlFormatter>::format_document(self, source, config);
        classify_native_result(source, config, context, FormatRequestTarget::Document, result)
    }

    /// Format one range and return an explicit typed terminal outcome.
    #[must_use]
    pub fn format_range_typed(
        &self,
        source: &str,
        range: TextRange,
        config: &FormatConfig,
        context: &FormatContext,
    ) -> TypedFormatResult {
        let result = if valid_range(source, range) {
            <Self as PerlFormatter>::format_range(self, source, range, config)
        } else {
            FormatResult::unsafe_to_format(
                source,
                UNSAFE_RANGE_CODE,
                "native range formatting refused because the requested UTF-16 range is invalid",
            )
        };
        classify_native_result(
            source,
            config,
            context,
            FormatRequestTarget::Range { range },
            result,
        )
    }
}

fn classify_native_result(
    source: &str,
    config: &FormatConfig,
    context: &FormatContext,
    target: FormatRequestTarget,
    result: FormatResult,
) -> TypedFormatResult {
    let classification = classify(source, config, target, &result);
    let actual_engine = if matches!(config.mode, FormatterMode::Off) {
        FormatEngine::Disabled
    } else {
        FormatEngine::Native
    };
    let safety = safety_evidence(source, &result, classification.reason);
    let outcome = FormatOutcome {
        disposition: classification.disposition,
        reason: classification.reason,
        identity: FormatIdentity {
            source_id: context.source_id.clone(),
            content_digest: stable_digest("source-v1", source.as_bytes()),
            source_generation: context.source_generation,
            actual_engine,
            requested_mode: config.mode,
            config_fingerprint: config_fingerprint(config),
        },
        target,
        change: change_summary(source, &result.formatted, &result.edits),
        safety,
        next_action: next_action(classification.reason).map(str::to_string),
    };

    TypedFormatResult { result, outcome }
}

#[derive(Debug, Clone, Copy)]
struct Classification {
    disposition: FormatDisposition,
    reason: FormatReasonCode,
}

fn classify(
    source: &str,
    config: &FormatConfig,
    target: FormatRequestTarget,
    result: &FormatResult,
) -> Classification {
    if matches!(config.mode, FormatterMode::Off) {
        return Classification {
            disposition: FormatDisposition::Refused,
            reason: FormatReasonCode::FormatterDisabled,
        };
    }

    if result.changed {
        return Classification {
            disposition: FormatDisposition::Applied,
            reason: FormatReasonCode::Applied,
        };
    }

    let Some(diagnostic) = result.diagnostics.first() else {
        return if target_has_only_supported_lines(source, config, target) {
            Classification {
                disposition: FormatDisposition::NoChange,
                reason: FormatReasonCode::AlreadyFormatted,
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


fn valid_range(source: &str, range: TextRange) -> bool {
    if (range.start.line, range.start.character) > (range.end.line, range.end.character) {
        return false;
    }
    let lines: Vec<&str> = source.split('\n').collect();
    let position_is_valid = |position: super::implementation::TextPosition| {
        lines
            .get(position.line as usize)
            .is_some_and(|line| utf16_len(line) >= position.character)
    };
    position_is_valid(range.start) && position_is_valid(range.end)
}

fn utf16_len(source: &str) -> u32 {
    source.encode_utf16().count() as u32
}

fn target_has_only_supported_lines(
    source: &str,
    config: &FormatConfig,
    target: FormatRequestTarget,
) -> bool {
    source.split('\n').enumerate().all(|(line, text)| {
        let included = match target {
            FormatRequestTarget::Document => true,
            FormatRequestTarget::Range { range } => range_includes_line(range, line as u32),
        };
        !included
            || text.trim().is_empty()
            || text.trim_start().starts_with('#')
            || format_simple_line(text, config).is_some()
    })
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
        | FormatReasonCode::UnsupportedSyntax => None,
    }
}

fn change_summary(source: &str, formatted: &str, edits: &[TextEdit]) -> FormatChangeSummary {
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

fn config_fingerprint(config: &FormatConfig) -> String {
    let canonical = format!(
        "format-config-v1|mode={}|line_width={}|indent_width={}|use_tabs={}|final_newline={}|trailing_comma={}|brace_placement={}|else_placement={}|keyword_spacing={}",
        formatter_mode_name(config.mode),
        config.line_width,
        config.indent_width,
        config.use_tabs,
        final_newline_name(config.final_newline),
        trailing_comma_name(config.trailing_comma),
        brace_placement_name(config.brace_placement),
        else_placement_name(config.else_placement),
        keyword_spacing_name(config.keyword_spacing),
    );
    stable_digest("format-config-v1", canonical.as_bytes())
}

fn stable_digest(prefix: &str, bytes: &[u8]) -> String {
    let hash = bytes
        .iter()
        .fold(FNV_OFFSET_BASIS, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME));
    format!("{prefix}:{hash:016x}")
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
    use super::count_changed_lines;

    #[test]
    fn changed_line_count_normalizes_line_endings_without_allocating() {
        assert_eq!(count_changed_lines("first\r\nsecond\r\n", "first\nsecond\n"), 0);
        assert_eq!(count_changed_lines("first\nsecond\n", "first\nchanged\n"), 1);
        assert_eq!(count_changed_lines("first\n", "first\nsecond\nthird\n"), 2);
        assert_eq!(count_changed_lines("first\nsecond\nthird\n", "first\n"), 2);
    }
}
