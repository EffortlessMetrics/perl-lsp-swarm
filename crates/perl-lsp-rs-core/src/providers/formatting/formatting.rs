//! Code formatting support for Perl parsing workflow pipeline.
//!
//! This module is the request-independent formatting policy layer. It keeps the
//! existing [`FormattedDocument`] API as a projection while exposing
//! [`FormattingDecision`] for callers that must distinguish applied, no-change,
//! refused, disabled, and failed/not-proven outcomes.

#[path = "legacy.rs"]
mod legacy;

use crate::hashing::fnv1a64_hex;
pub use crate::providers::formatting_types::{
    FormatPosition, FormatRange, FormatTextEdit, FormattedDocument, FormattingOptions,
};
use crate::tooling::perltidy::native::{
    FormatChangeSummary, FormatContext, FormatDisposition, FormatEngine, FormatEvidenceState,
    FormatIdentity, FormatLineEndingDisposition, FormatOutcome, FormatReasonCode,
    FormatRequestTarget, FormatSafetyEvidence, TypedFormatResult,
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
        match self.mode {
            FormatterMode::Native | FormatterMode::Compat => {
                self.native_document_decision(content, options, context)
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
    pub fn format_range_decision(
        &self,
        content: &str,
        range: &FormatRange,
        options: &FormattingOptions,
        context: &FormatContext,
    ) -> Result<FormattingDecision, FormattingError> {
        let target = FormatRequestTarget::Range { range: to_native_range(range) };
        if !range_is_admissible(content, range) {
            return Ok(refused_decision(
                content,
                self.mode,
                FormatEngine::Unknown,
                target,
                context,
                FormatReasonCode::UnsafeRange,
                "request a valid complete source range",
            ));
        }

        match self.mode {
            FormatterMode::Native | FormatterMode::Compat => {
                self.native_range_decision(content, range, options, context)
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
    ) -> Result<FormattingDecision, FormattingError> {
        let mut config = native_format_config(options, self.perltidy_config.as_ref(), true);
        config.mode = self.mode;
        let mut typed = NativeFormatter::new().format_document_typed(content, &config, context);
        bind_lsp_options(&mut typed.outcome.identity.config_fingerprint, options);
        project_native_document(content, options, typed)
    }

    fn native_range_decision(
        &self,
        content: &str,
        range: &FormatRange,
        options: &FormattingOptions,
        context: &FormatContext,
    ) -> Result<FormattingDecision, FormattingError> {
        let native_range = to_native_range(range);
        let mut config = native_format_config(options, self.perltidy_config.as_ref(), false);
        config.mode = self.mode;
        let mut typed =
            NativeFormatter::new().format_range_typed(content, native_range, &config, context);
        bind_lsp_options(&mut typed.outcome.identity.config_fingerprint, options);
        project_native_range(content, range, options, typed)
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

    let formatted = apply_lsp_whitespace_options(&result.formatted, options);
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
    range: &FormatRange,
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
            let edits: Vec<_> = result.edits.into_iter().map(native_edit_to_format_edit).collect();
            finalize_outcome(&mut outcome, content, &result.formatted, &edits);
            return Ok(FormattingDecision {
                document: FormattedDocument { text: result.formatted, edits },
                outcome,
            });
        }
        FormatDisposition::NoChange => {}
    }

    let document = whitespace_range_fallback(content, range, options);
    finalize_outcome(&mut outcome, content, &document.text, &document.edits);
    Ok(FormattingDecision { document, outcome })
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

fn whitespace_range_fallback(
    content: &str,
    range: &FormatRange,
    options: &FormattingOptions,
) -> FormattedDocument {
    let lines: Vec<&str> = content.lines().collect();
    let start_line = range.start.line as usize;
    let end_line = (range.end.line as usize).min(lines.len().saturating_sub(1));

    if start_line >= lines.len() || end_line < start_line {
        return unchanged_document(content);
    }

    let line_ending = if content.contains("\r\n") { "\r\n" } else { "\n" };
    let text_to_format = lines[start_line..=end_line].join(line_ending);
    let raw = apply_lsp_whitespace_options(&text_to_format, options);
    let formatted = raw.trim_end_matches(['\r', '\n']).to_string();
    if formatted == text_to_format {
        return unchanged_document(content);
    }

    let line_segments: Vec<&str> = content.split_inclusive('\n').collect();
    let start_offset = line_segments.iter().take(start_line).map(|line| line.len()).sum::<usize>();
    let end_line_body = line_segments[end_line].trim_end_matches(['\r', '\n']);
    let end_offset = line_segments.iter().take(end_line).map(|line| line.len()).sum::<usize>()
        + end_line_body.len();
    let mut updated =
        String::with_capacity(content.len() - (end_offset - start_offset) + formatted.len());
    updated.push_str(&content[..start_offset]);
    updated.push_str(&formatted);
    updated.push_str(&content[end_offset..]);

    FormattedDocument {
        text: updated,
        edits: vec![FormatTextEdit {
            range: FormatRange::new(
                FormatPosition::new(start_line as u32, 0),
                FormatPosition::new(end_line as u32, utf16_len(lines[end_line]) as u32),
            ),
            new_text: formatted,
        }],
    }
}

fn apply_lsp_whitespace_options(content: &str, options: &FormattingOptions) -> String {
    let mut output = content.to_string();

    if options.trim_trailing_whitespace.unwrap_or(false) {
        output = trim_trailing_whitespace(&output);
    }
    if options.trim_final_newlines.unwrap_or(false) {
        while output.ends_with('\n') {
            output.pop();
        }
    }
    if options.insert_final_newline.unwrap_or(false) && !output.ends_with('\n') {
        output.push('\n');
    }

    output
}

fn trim_trailing_whitespace(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    for line in content.split_inclusive('\n') {
        if let Some(without_nl) = line.strip_suffix('\n') {
            result.push_str(without_nl.trim_end_matches([' ', '\t']));
            result.push('\n');
        } else {
            result.push_str(line.trim_end_matches([' ', '\t']));
        }
    }
    result
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

fn range_is_admissible(content: &str, range: &FormatRange) -> bool {
    let line_count = content.lines().count();
    let start_line = range.start.line as usize;
    start_line < line_count
        && range.end.line >= range.start.line
        && (range.end.line > range.start.line || range.end.character >= range.start.character)
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

#[cfg(test)]
mod decision_projection_tests {
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
}

fn utf16_len(s: &str) -> usize {
    s.chars().map(|ch| if ch as u32 >= 0x10000 { 2 } else { 1 }).sum()
}

const fn formatter_mode_name(mode: FormatterMode) -> &'static str {
    match mode {
        FormatterMode::Native => "native",
        FormatterMode::Compat => "compat",
        FormatterMode::ExternalLegacy => "external-legacy",
        FormatterMode::Off => "off",
    }
}
