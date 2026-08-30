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
//! The claim covered here is the native provider API. Public LSP
//! `rangeFormatting` reaches this facade through
//! `perl_lsp_rs::runtime::language::formatting_policy::handle_range_formatting_policy`
//! → `perl_lsp_rs_core::providers::formatting::FormattingProvider::format_range_decision`
//! → `native_range_decision` → this facade, because `native.rs` re-exports it
//! as the crate's public `NativeFormatter`. The guard applies when the
//! formatter mode selects the native engine. Incremental snapshot replay
//! remains out of scope.
//!
//! Every existing parse, literal-preservation, render, and post-parse gate still
//! runs in the underlying engine.

use super::implementation::{
    self, FormatConfig, FormatResult, FormatterMode, PerlFormatter, TextRange, range_includes_line,
};
use super::outcome::{
    FormatContext, FormatIdentity, FormatReasonCode, FormatRequestTarget, TypedFormatResult,
    classify_format_result, valid_range,
};
use perl_parser_core::{SourceRegionIndex, SourceRegionKind};

const LITERAL_PRESERVE_CODE: &str = "native.format.literal_preserve_region";

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
        if matches!(config.mode, FormatterMode::Off) {
            return engine.format_document_typed(source, config, context);
        }

        let Some(sanitized) = sanitize_non_code_heredoc_markers(source) else {
            return engine.format_document_typed(source, config, context);
        };
        let source_identity = FormatIdentity::for_request(source, config, context);
        let typed = engine.format_document_typed(&sanitized.text, config, context);
        match restore_typed_result(source, &sanitized, source_identity, typed) {
            Some(result) => result,
            None => engine.format_document_typed(source, config, context),
        }
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
        let baseline = engine.format_range_typed(source, range, config, context);
        if matches!(config.mode, FormatterMode::Off)
            || baseline.outcome.reason == FormatReasonCode::UnsafeRange
        {
            return baseline;
        }

        if range_overlaps_completed_heredoc(source, range, &source_line_ranges(source)) {
            return typed_heredoc_range_refusal(
                source,
                range,
                config,
                context,
                baseline.outcome.identity,
            );
        }

        let Some(sanitized) = sanitize_non_code_heredoc_markers(source) else {
            return baseline;
        };

        let typed = engine.format_range_typed(&sanitized.text, range, config, context);
        let source_identity = baseline.outcome.identity.clone();
        match restore_typed_result(source, &sanitized, source_identity, typed) {
            Some(result) => result,
            None => baseline,
        }
    }
}

impl PerlFormatter for NativeFormatter {
    fn format_document(&self, source: &str, config: &FormatConfig) -> FormatResult {
        let engine = implementation::NativeFormatter::new();
        if matches!(config.mode, FormatterMode::Off) {
            return <implementation::NativeFormatter as PerlFormatter>::format_document(
                &engine, source, config,
            );
        }

        let Some(sanitized) = sanitize_non_code_heredoc_markers(source) else {
            return <implementation::NativeFormatter as PerlFormatter>::format_document(
                &engine, source, config,
            );
        };

        let result = <implementation::NativeFormatter as PerlFormatter>::format_document(
            &engine,
            &sanitized.text,
            config,
        );
        match restore_format_result(source, &sanitized, result) {
            Some(result) => result,
            None => <implementation::NativeFormatter as PerlFormatter>::format_document(
                &engine, source, config,
            ),
        }
    }

    fn format_range(&self, source: &str, range: TextRange, config: &FormatConfig) -> FormatResult {
        let engine = implementation::NativeFormatter::new();
        if matches!(config.mode, FormatterMode::Off) {
            return <implementation::NativeFormatter as PerlFormatter>::format_range(
                &engine, source, range, config,
            );
        }
        let lines = source_line_ranges(source);
        if valid_range(source, range) && range_overlaps_completed_heredoc(source, range, &lines) {
            return heredoc_range_refusal(source);
        }

        let Some(sanitized) = sanitize_non_code_heredoc_markers(source) else {
            return <implementation::NativeFormatter as PerlFormatter>::format_range(
                &engine, source, range, config,
            );
        };

        let result = <implementation::NativeFormatter as PerlFormatter>::format_range(
            &engine,
            &sanitized.text,
            range,
            config,
        );
        match restore_format_result(source, &sanitized, result) {
            Some(result) => result,
            None => <implementation::NativeFormatter as PerlFormatter>::format_range(
                &engine, source, range, config,
            ),
        }
    }
}

struct SanitizedSource {
    text: String,
    sentinel: String,
    substitutions: usize,
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

    let substitutions = offsets.len();
    let sentinel = unused_two_letter_sentinel(source)?;
    let mut text = source.to_string();
    for offset in offsets.into_iter().rev() {
        text.replace_range(offset..offset + 2, &sentinel);
    }

    Some(SanitizedSource { text, sentinel, substitutions })
}

fn unused_two_letter_sentinel(source: &str) -> Option<String> {
    let mut used = [0_u64; 11];
    for pair in source.as_bytes().windows(2) {
        if pair[0].is_ascii_uppercase() && pair[1].is_ascii_uppercase() {
            let index = usize::from(pair[0] - b'A') * 26 + usize::from(pair[1] - b'A');
            used[index / 64] |= 1_u64 << (index % 64);
        }
    }

    for first in b'A'..=b'Z' {
        for second in b'A'..=b'Z' {
            let index = usize::from(first - b'A') * 26 + usize::from(second - b'A');
            if used[index / 64] & (1_u64 << (index % 64)) == 0 {
                let mut candidate = String::with_capacity(2);
                candidate.push(char::from(first));
                candidate.push(char::from(second));
                return Some(candidate);
            }
        }
    }
    None
}

fn restore_typed_result(
    source: &str,
    sanitized: &SanitizedSource,
    source_identity: FormatIdentity,
    mut typed: TypedFormatResult,
) -> Option<TypedFormatResult> {
    typed.result = restore_format_result(source, sanitized, typed.result)?;
    typed.outcome.identity = source_identity;
    Some(typed)
}

fn restore_format_result(
    source: &str,
    sanitized: &SanitizedSource,
    mut result: FormatResult,
) -> Option<FormatResult> {
    if result.formatted.match_indices(&sanitized.sentinel).count() != sanitized.substitutions {
        return None;
    }
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
    Some(result)
}

fn typed_heredoc_range_refusal(
    source: &str,
    range: TextRange,
    config: &FormatConfig,
    context: &FormatContext,
    source_identity: FormatIdentity,
) -> TypedFormatResult {
    let mut typed = classify_format_result(
        source,
        config,
        context,
        FormatRequestTarget::Range { range },
        heredoc_range_refusal(source),
    );
    typed.outcome.identity = source_identity;
    typed
}

fn heredoc_range_refusal(source: &str) -> FormatResult {
    FormatResult::unsafe_to_format(
        source,
        LITERAL_PRESERVE_CODE,
        "native range formatting skipped because heredoc preservation is not enabled yet",
    )
}

fn range_overlaps_completed_heredoc(
    source: &str,
    range: TextRange,
    lines: &[(usize, usize)],
) -> bool {
    if !source.contains("<<") {
        return false;
    }

    let (range_start, range_end) = byte_span_for_line_range(source, range, lines);
    if range_start == range_end {
        return false;
    }

    let regions = SourceRegionIndex::build(source);
    // The production lexer returns completed heredoc body events in FIFO order.
    // Do not join them back to opener offsets: queued declarations such as
    // `print <<A, <<B;` share one physical body start even though the second
    // body begins after the first terminator. A region ending at EOF is an
    // unclosed heredoc, so it remains owned by the document parse gate.
    regions.completed_heredoc_spans().into_iter().any(|region| {
        region.kind == SourceRegionKind::Heredoc
            && region.end < source.len()
            && region.start < range_end
            && byte_offset_after_line(source, region.end, lines) > range_start
    })
}

fn byte_offset_after_line(source: &str, offset: usize, lines: &[(usize, usize)]) -> usize {
    lines
        .iter()
        .find(|(start, end)| *start <= offset && offset < *end)
        .map_or(source.len(), |(_, end)| *end)
}

fn byte_span_for_line_range(
    source: &str,
    range: TextRange,
    lines: &[(usize, usize)],
) -> (usize, usize) {
    let mut byte_start = 0_usize;
    let mut byte_end = source.len();
    let mut found_start = false;
    for (line_index, (line_start, line_end)) in lines.iter().copied().enumerate() {
        let line_index = line_index as u32;
        if line_index == range.start.line {
            byte_start = line_start;
            found_start = true;
        }
        if range_includes_line(range, line_index) {
            byte_end = line_end;
        }
    }

    if !found_start {
        return (source.len(), source.len());
    }
    (byte_start, byte_end)
}

fn source_line_ranges(source: &str) -> Vec<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0_usize;
    let mut offset = 0_usize;

    while offset < bytes.len() {
        let terminator_len = match bytes[offset] {
            b'\n' => 1,
            b'\r' if offset + 1 < bytes.len() && bytes[offset + 1] == b'\n' => 2,
            b'\r' => 1,
            _ => {
                offset += 1;
                continue;
            }
        };
        offset += terminator_len;
        ranges.push((start, offset));
        start = offset;
    }

    if start < source.len() || ranges.is_empty() || start == source.len() {
        ranges.push((start, source.len()));
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_selection_scans_all_existing_ascii_pairs() {
        assert_eq!(unused_two_letter_sentinel("AA AB AC <<LABEL"), Some("AD".to_string()));
    }

    #[test]
    fn restoration_rejects_missing_sentinel_text() {
        let sanitized = SanitizedSource {
            text: "AA".to_string(),
            sentinel: "AA".to_string(),
            substitutions: 1,
        };
        let result = FormatResult::replace_document("<<", "");

        assert!(restore_format_result("<<", &sanitized, result).is_none());
    }
}
