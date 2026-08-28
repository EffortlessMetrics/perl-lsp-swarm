//! Safety facade for the Rust-native formatter.
//!
//! The layout engine remains in `implementation`; this facade adds two lexical
//! boundaries before delegating to it:
//!
//! - marker-shaped `<<LABEL` text proven to be inside an ordinary string or line
//!   comment is temporarily shielded from the legacy line heuristic;
//! - completed heredoc bodies drained by the lexer are protected during range
//!   formatting even when their opener lies outside the requested lines.
//!
//! Every existing parse, literal-preservation, render, and post-parse gate still
//! runs in the underlying engine.

use super::implementation::{
    self, FormatConfig, FormatResult, FormatterMode, PerlFormatter, TextRange,
    range_includes_line,
};
use super::outcome::{
    FormatChangeSummary, FormatContext, FormatRequestTarget, TypedFormatResult,
};
use perl_parser_core::{SourceRegionIndex, SourceRegionKind, TokenKind, TokenStream};

const LITERAL_PRESERVE_CODE: &str = "native.format.literal_preserve_region";
const CLASSIFIER_HEREDOC_SOURCE: &str = "print <<'EOF';\nbody\nEOF\n";
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Parse-gated Rust-native Perl formatter with lexical preserve-region guards.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeFormatter;

impl NativeFormatter {
    /// Create a parse-gated native formatter.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Format a complete document and return an explicit typed terminal outcome.
    #[must_use]
    pub fn format_document_typed(
        &self,
        source: &str,
        config: &FormatConfig,
        context: &FormatContext,
    ) -> TypedFormatResult {
        let engine = implementation::NativeFormatter::new();
        let Some(sanitized) = sanitize_non_code_heredoc_markers(source) else {
            return engine.format_document_typed(source, config, context);
        };

        restore_typed_result(
            source,
            &sanitized,
            engine.format_document_typed(&sanitized.text, config, context),
        )
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
        let engine = implementation::NativeFormatter::new();
        if matches!(config.mode, FormatterMode::Off) || !valid_range(source, range) {
            return engine.format_range_typed(source, range, config, context);
        }

        if range_overlaps_completed_heredoc(source, range) {
            return typed_heredoc_range_refusal(source, range, config, context);
        }

        let Some(sanitized) = sanitize_non_code_heredoc_markers(source) else {
            return engine.format_range_typed(source, range, config, context);
        };

        restore_typed_result(
            source,
            &sanitized,
            engine.format_range_typed(&sanitized.text, range, config, context),
        )
    }
}

impl PerlFormatter for NativeFormatter {
    fn format_document(&self, source: &str, config: &FormatConfig) -> FormatResult {
        let engine = implementation::NativeFormatter::new();
        let Some(sanitized) = sanitize_non_code_heredoc_markers(source) else {
            return <implementation::NativeFormatter as PerlFormatter>::format_document(
                &engine, source, config,
            );
        };

        restore_format_result(
            source,
            &sanitized,
            <implementation::NativeFormatter as PerlFormatter>::format_document(
                &engine,
                &sanitized.text,
                config,
            ),
        )
    }

    fn format_range(&self, source: &str, range: TextRange, config: &FormatConfig) -> FormatResult {
        let engine = implementation::NativeFormatter::new();
        if !matches!(config.mode, FormatterMode::Off)
            && valid_range(source, range)
            && range_overlaps_completed_heredoc(source, range)
        {
            return heredoc_range_refusal(source);
        }

        let Some(sanitized) = sanitize_non_code_heredoc_markers(source) else {
            return <implementation::NativeFormatter as PerlFormatter>::format_range(
                &engine, source, range, config,
            );
        };

        restore_format_result(
            source,
            &sanitized,
            <implementation::NativeFormatter as PerlFormatter>::format_range(
                &engine,
                &sanitized.text,
                range,
                config,
            ),
        )
    }
}

struct SanitizedSource {
    text: String,
    sentinel: String,
}

fn sanitize_non_code_heredoc_markers(source: &str) -> Option<SanitizedSource> {
    if !source.contains("<<") {
        return None;
    }

    let regions = SourceRegionIndex::build(source);
    let offsets = source
        .match_indices("<<")
        .filter_map(|(offset, _)| {
            matches!(
                regions.kind_at_offset(offset),
                SourceRegionKind::StringLiteral | SourceRegionKind::LineComment
            )
            .then_some(offset)
        })
        .collect::<Vec<_>>();
    if offsets.is_empty() {
        return None;
    }

    let sentinel = unused_two_letter_sentinel(source)?;
    let mut text = source.to_string();
    for offset in offsets.into_iter().rev() {
        text.replace_range(offset..offset + 2, &sentinel);
    }

    Some(SanitizedSource { text, sentinel })
}

fn unused_two_letter_sentinel(source: &str) -> Option<String> {
    for first in b'A'..=b'Z' {
        for second in b'A'..=b'Z' {
            let mut candidate = String::with_capacity(2);
            candidate.push(char::from(first));
            candidate.push(char::from(second));
            if !source.contains(candidate.as_str()) {
                return Some(candidate);
            }
        }
    }
    None
}

fn restore_typed_result(
    source: &str,
    sanitized: &SanitizedSource,
    mut typed: TypedFormatResult,
) -> TypedFormatResult {
    typed.result = restore_format_result(source, sanitized, typed.result);
    typed.outcome.identity.content_digest = source_digest(source);
    typed.outcome.change = change_summary(source, &typed.result.formatted, typed.result.edits.len());
    typed
}

fn restore_format_result(
    source: &str,
    sanitized: &SanitizedSource,
    mut result: FormatResult,
) -> FormatResult {
    result.formatted = result.formatted.replace(&sanitized.sentinel, "<<");
    for edit in &mut result.edits {
        edit.new_text = edit.new_text.replace(&sanitized.sentinel, "<<");
    }
    for diagnostic in &mut result.diagnostics {
        diagnostic.message = diagnostic.message.replace(&sanitized.sentinel, "<<");
    }

    result.changed = result.formatted != source;
    if !result.changed {
        result.edits.clear();
    }
    result
}

fn typed_heredoc_range_refusal(
    source: &str,
    range: TextRange,
    config: &FormatConfig,
    context: &FormatContext,
) -> TypedFormatResult {
    // Reuse the existing outcome classifier instead of creating a second
    // disposition/config-fingerprint authority. The synthetic source reaches
    // the same literal-preservation refusal, after which the subject, target,
    // and compatibility result are rebound to the caller's request.
    let mut typed = implementation::NativeFormatter::new().format_document_typed(
        CLASSIFIER_HEREDOC_SOURCE,
        config,
        context,
    );
    typed.result = heredoc_range_refusal(source);
    typed.outcome.identity.content_digest = source_digest(source);
    typed.outcome.target = FormatRequestTarget::Range { range };
    typed.outcome.change = FormatChangeSummary {
        edit_count: 0,
        source_bytes_changed: 0,
        rendered_bytes_changed: 0,
        changed_lines: 0,
    };
    typed
}

fn heredoc_range_refusal(source: &str) -> FormatResult {
    FormatResult::unsafe_to_format(
        source,
        LITERAL_PRESERVE_CODE,
        "native range formatting skipped because heredoc preservation is not enabled yet",
    )
}

fn range_overlaps_completed_heredoc(source: &str, range: TextRange) -> bool {
    if !source.contains("<<") {
        return false;
    }

    let (range_start, range_end) = byte_span_for_line_range(source, range);
    if range_start == range_end {
        return false;
    }

    let mut stream = TokenStream::new(source);
    let mut pending_body_starts = Vec::new();

    loop {
        let Ok(token) = stream.next() else {
            return false;
        };
        let kind = token.kind();

        let unknown_heredoc_tail = kind == TokenKind::UnknownRest
            && pending_body_starts
                .first()
                .is_some_and(|body_start| token.start() == *body_start);
        if unknown_heredoc_tail {
            // An unclosed or over-budget heredoc is not a proven completed
            // literal region. Leave it to the full-document parse gate.
            pending_body_starts.clear();
        } else if let Some(first_body_start) = pending_body_starts.first().copied()
            && token.start() >= first_body_start
        {
            let body_end = if kind == TokenKind::Eof {
                source.len()
            } else {
                line_start_at_or_before(source, token.start())
            };
            if pending_body_starts
                .iter()
                .any(|body_start| *body_start < range_end && body_end > range_start)
            {
                return true;
            }
            pending_body_starts.clear();
        }

        match kind {
            TokenKind::HeredocStart => {
                pending_body_starts.push(byte_offset_after_line(source, token.end()));
            }
            TokenKind::Eof => return false,
            _ => {}
        }
    }
}

fn byte_offset_after_line(source: &str, offset: usize) -> usize {
    source
        .get(offset..)
        .and_then(|suffix| suffix.find('\n').map(|relative| offset + relative + 1))
        .unwrap_or(source.len())
}

fn line_start_at_or_before(source: &str, offset: usize) -> usize {
    let offset = offset.min(source.len());
    source
        .get(..offset)
        .and_then(|prefix| prefix.rfind('\n').map(|index| index + 1))
        .unwrap_or(0)
}

fn byte_span_for_line_range(source: &str, range: TextRange) -> (usize, usize) {
    let mut byte_start = 0_usize;
    let mut byte_end = source.len();
    let mut found_start = false;
    let mut byte_offset = 0_usize;

    for (line_index, line) in source.split_inclusive('\n').enumerate() {
        let line_index = line_index as u32;
        if line_index == range.start.line {
            byte_start = byte_offset;
            found_start = true;
        }
        let next_offset = byte_offset + line.len();
        if range_includes_line(range, line_index) {
            byte_end = next_offset;
        }
        byte_offset = next_offset;
    }

    if !found_start {
        return (source.len(), source.len());
    }
    (byte_start, byte_end)
}

fn valid_range(source: &str, range: TextRange) -> bool {
    if (range.start.line, range.start.character) > (range.end.line, range.end.character) {
        return false;
    }
    let lines = source.split('\n').collect::<Vec<_>>();
    let position_is_valid = |position: implementation::TextPosition| {
        lines
            .get(position.line as usize)
            .is_some_and(|line| line.encode_utf16().count() >= position.character as usize)
    };
    position_is_valid(range.start) && position_is_valid(range.end)
}

fn source_digest(source: &str) -> String {
    let hash = source.as_bytes().iter().fold(FNV_OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    });
    format!("source-v1:{hash:016x}")
}

fn change_summary(source: &str, formatted: &str, edit_count: usize) -> FormatChangeSummary {
    if source == formatted {
        return FormatChangeSummary {
            edit_count,
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

    FormatChangeSummary {
        edit_count,
        source_bytes_changed: source_bytes.len().saturating_sub(prefix + suffix),
        rendered_bytes_changed: formatted_bytes.len().saturating_sub(prefix + suffix),
        changed_lines: count_changed_lines(source, formatted),
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

#[cfg(test)]
mod tests {
    use super::super::outcome::FormatReasonCode;
    use super::*;

    #[test]
    fn completed_heredoc_span_excludes_following_code_line() {
        let source = "print <<'EOF';\nbody\nEOF\nmy$x=1;\n";
        let body = TextRange::new(
            implementation::TextPosition::new(1, 0),
            implementation::TextPosition::new(2, 0),
        );
        let following = TextRange::new(
            implementation::TextPosition::new(3, 0),
            implementation::TextPosition::new(4, 0),
        );
        assert!(range_overlaps_completed_heredoc(source, body));
        assert!(!range_overlaps_completed_heredoc(source, following));
    }

    #[test]
    fn unclosed_heredoc_is_not_claimed_as_completed() {
        let source = "print <<'EOF';\nbody\n";
        let body = TextRange::new(
            implementation::TextPosition::new(1, 0),
            implementation::TextPosition::new(2, 0),
        );
        assert!(!range_overlaps_completed_heredoc(source, body));
    }

    #[test]
    fn typed_refusal_uses_existing_literal_classifier() {
        let source = "print <<'EOF';\nbody\nEOF\n";
        let range = TextRange::new(
            implementation::TextPosition::new(1, 0),
            implementation::TextPosition::new(2, 0),
        );
        let typed = typed_heredoc_range_refusal(
            source,
            range,
            &FormatConfig::default(),
            &FormatContext::default(),
        );
        assert_eq!(typed.outcome.reason, FormatReasonCode::LiteralPreservationUnsupported);
        assert_eq!(typed.outcome.target, FormatRequestTarget::Range { range });
        assert_eq!(typed.outcome.identity.content_digest, source_digest(source));
    }
}
