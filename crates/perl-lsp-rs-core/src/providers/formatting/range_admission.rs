//! One exact admitted-range contract shared by single-range and multi-range
//! formatting requests.
//!
//! Every requested UTF-16 endpoint maps through the same strict source
//! geometry before any formatter runs: LF, CRLF (one separator), and
//! supported bare-CR separators end a line; a terminal separator yields a
//! final empty line; non-BMP characters count as two code units. Invalid
//! lines, characters past a line end, mid-surrogate endpoints, and reversed
//! ranges each produce one typed refusal. Nothing is clamped to convenient
//! source bounds, so a range request can never silently become the nearest
//! convenient lines.

use crate::providers::formatting_types::{FormatPosition, FormatRange};

/// One strict position-mapping failure inside the current source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangePositionError {
    /// The line does not exist in the current source geometry, including the
    /// final empty line after a terminal separator.
    OutsideDocument {
        /// Requested zero-based line.
        line: u32,
    },
    /// The character offset falls between the two halves of a surrogate pair.
    SurrogateSplit {
        /// Requested zero-based line.
        line: u32,
        /// Requested UTF-16 character offset.
        character: u32,
    },
    /// The character offset is past the end of the line body.
    PastLineEnd {
        /// Requested zero-based line.
        line: u32,
        /// Requested UTF-16 character offset.
        character: u32,
        /// UTF-16 length of the line body.
        length: usize,
    },
}

impl std::fmt::Display for RangePositionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for RangePositionError {}

impl RangePositionError {
    /// Stable machine reason shared by every strict mapping failure.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        "invalid_position"
    }

    /// Deterministic human explanation naming the failed endpoint values.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::OutsideDocument { line } => {
                format!("line {line} is outside the current document")
            }
            Self::SurrogateSplit { line, character } => {
                format!("UTF-16 character {character} on line {line} splits a surrogate pair")
            }
            Self::PastLineEnd { line, character, length } => {
                format!("UTF-16 character {character} is outside line {line} (length {length})")
            }
        }
    }
}

/// One typed single-range admission failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeAdmissionError {
    /// The start endpoint did not map onto the current source.
    Start(RangePositionError),
    /// The end endpoint did not map onto the current source.
    End(RangePositionError),
    /// Both endpoints mapped but the end precedes the start.
    Reversed,
}

impl std::fmt::Display for RangeAdmissionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for RangeAdmissionError {}

impl RangeAdmissionError {
    /// Stable machine reason for the admission failure.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Start(_) | Self::End(_) => "invalid_position",
            Self::Reversed => "reversed_range",
        }
    }

    /// Deterministic human explanation naming the failed side.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Start(error) => format!("range start {}", error.message()),
            Self::End(error) => format!("range end {}", error.message()),
            Self::Reversed => "range ends before it starts".to_string(),
        }
    }

    /// Mechanically known next action for the admission failure.
    #[must_use]
    pub const fn next_action(&self) -> &'static str {
        match self {
            Self::Start(_) | Self::End(_) => {
                "request a range whose endpoints are valid positions in the current source"
            }
            Self::Reversed => "request a range whose end follows its start",
        }
    }
}

/// Byte-exact line-start geometry over one current source snapshot.
///
/// The scan recognizes LF, CRLF (one separator), and supported bare-CR
/// separators, and a terminal separator yields a final empty line, matching
/// the true-EOF semantics of `FormatRange::whole_document`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceGeometry {
    line_starts: Vec<usize>,
}

impl SourceGeometry {
    /// Build the geometry for one source snapshot.
    #[must_use]
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        let bytes = source.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'\n' => line_starts.push(index + 1),
                b'\r' => {
                    if bytes.get(index + 1) == Some(&b'\n') {
                        line_starts.push(index + 2);
                        index += 1;
                    } else {
                        line_starts.push(index + 1);
                    }
                }
                _ => {}
            }
            index += 1;
        }
        Self { line_starts }
    }

    /// Number of logical lines including any terminal empty line.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Byte offset where the requested line starts, if it exists.
    #[must_use]
    pub fn line_start(&self, line: u32) -> Option<usize> {
        self.line_starts.get(line as usize).copied()
    }

    /// Byte offset just past the requested line body (before its separator).
    ///
    /// The final line ends at true EOF.
    #[must_use]
    pub fn line_content_end(&self, source: &str, line: u32) -> usize {
        let Some(&next_start) = self.line_starts.get(line as usize + 1) else {
            return source.len();
        };
        let bytes = source.as_bytes();
        if next_start >= 2
            && bytes.get(next_start - 2) == Some(&b'\r')
            && bytes.get(next_start - 1) == Some(&b'\n')
        {
            next_start - 2
        } else if next_start >= 1
            && matches!(bytes.get(next_start - 1), Some(&b'\n') | Some(&b'\r'))
        {
            next_start - 1
        } else {
            next_start
        }
    }

    /// Map one UTF-16 wire position to its exact byte offset.
    pub fn byte_offset(
        &self,
        source: &str,
        line: u32,
        character: u32,
    ) -> Result<usize, RangePositionError> {
        let Some(start) = self.line_start(line) else {
            return Err(RangePositionError::OutsideDocument { line });
        };
        let end = self.line_content_end(source, line);

        let target = character as usize;
        let mut units = 0_usize;
        for (relative, ch) in source[start..end].char_indices() {
            if units == target {
                return Ok(start + relative);
            }
            let next = units.saturating_add(ch.len_utf16());
            if target < next {
                return Err(RangePositionError::SurrogateSplit { line, character });
            }
            units = next;
        }
        if units == target {
            Ok(end)
        } else {
            Err(RangePositionError::PastLineEnd { line, character, length: units })
        }
    }

    /// Body span `(start, content_end)` for one existing line.
    pub fn line_span(&self, source: &str, line: u32) -> Result<(usize, usize), RangePositionError> {
        let start = self.line_start(line).ok_or(RangePositionError::OutsideDocument { line })?;
        Ok((start, self.line_content_end(source, line)))
    }
}

/// One admitted formatting target: the requested wire identity plus its exact
/// byte interval in the admitted source snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedFormatRange {
    /// Requested wire range, retained independently from execution strategy.
    pub requested: FormatRange,
    /// Exact start byte offset in the admitted source.
    pub start_byte: usize,
    /// Exclusive end byte offset in the admitted source.
    pub end_byte: usize,
}

impl AdmittedFormatRange {
    /// Whether the admitted target selects zero bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start_byte == self.end_byte
    }

    /// Canonical containment span engine-emitted edits must stay inside.
    ///
    /// The default range contract is exact: an engine edit may not escape the
    /// requested byte interval. Syntax- or trivia-aware widening, when
    /// introduced, must be represented by a separate explicit admission
    /// contract rather than inferred from line boundaries.
    pub fn allowed_edit_span(
        &self,
        source: &str,
        geometry: &SourceGeometry,
    ) -> Result<(usize, usize), RangePositionError> {
        let _ = (source, geometry);
        Ok((self.start_byte, self.end_byte))
    }
}

/// Admit one requested range against the current source geometry.
///
/// Both endpoints map through the strict position mapper before ordering is
/// checked; the end stays exclusive.
pub fn admit_format_range(
    geometry: &SourceGeometry,
    source: &str,
    range: &FormatRange,
) -> Result<AdmittedFormatRange, RangeAdmissionError> {
    let start_byte = geometry
        .byte_offset(source, range.start.line, range.start.character)
        .map_err(RangeAdmissionError::Start)?;
    let end_byte = geometry
        .byte_offset(source, range.end.line, range.end.character)
        .map_err(RangeAdmissionError::End)?;
    if end_byte < start_byte {
        return Err(RangeAdmissionError::Reversed);
    }
    Ok(AdmittedFormatRange { requested: range.clone(), start_byte, end_byte })
}

/// Admit one requested range given raw wire (line, character) endpoints.
///
/// Identical strict mapping to [`admit_format_range`]; offered so policy-layer
/// `--lib` sources can admit ranges without naming the geometry types
/// (#9618).
pub fn admit_wire_endpoints(
    geometry: &SourceGeometry,
    source: &str,
    start: (u32, u32),
    end: (u32, u32),
) -> Result<AdmittedFormatRange, RangeAdmissionError> {
    admit_format_range(
        geometry,
        source,
        &FormatRange::new(FormatPosition::new(start.0, start.1), FormatPosition::new(end.0, end.1)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::formatting_types::FormatPosition;
    use perl_test_must::{must_err_with, must_with};

    fn position(line: u32, character: u32) -> FormatPosition {
        FormatPosition::new(line, character)
    }

    fn range(sl: u32, sc: u32, el: u32, ec: u32) -> FormatRange {
        FormatRange::new(position(sl, sc), position(el, ec))
    }

    fn admitted(source: &str, requested: FormatRange) -> AdmittedFormatRange {
        let geometry = SourceGeometry::new(source);
        must_with(admit_format_range(&geometry, source, &requested), "test range must admit")
    }

    fn rejection(source: &str, requested: FormatRange) -> RangeAdmissionError {
        let geometry = SourceGeometry::new(source);
        must_err_with(
            admit_format_range(&geometry, source, &requested),
            "test range must refuse deterministically",
        )
    }

    #[test]
    fn geometry_includes_terminal_empty_line_and_true_eof() {
        for (source, expected_lines) in
            [("abc\n", 2), ("abc\r\n", 2), ("abc\r", 2), ("abc", 1), ("", 1), ("\n\n", 3)]
        {
            let geometry = SourceGeometry::new(source);
            assert_eq!(geometry.line_count(), expected_lines, "{source:?}");
        }
    }

    #[test]
    fn byte_offsets_match_true_eof_position_authority() {
        for source in [
            "",
            "x;\n",
            "abc",
            "a\nb",
            "a\n",
            "a\r\n",
            "a\r",
            "a\r\n\r\nb",
            "😀",
            "a\r\nb😀",
            "😀\n",
            "a\rb\nc\r\n",
            "\r\n\r\n",
        ] {
            let eof = FormatRange::whole_document(source).end;
            let geometry = SourceGeometry::new(source);
            let mapped = must_with(
                geometry.byte_offset(source, eof.line, eof.character),
                "whole-document EOF must always map",
            );
            assert_eq!(mapped, source.len(), "EOF of {source:?} must map to true EOF");
        }
    }

    #[test]
    fn crlf_bare_cr_and_astral_positions_map_exactly() {
        let source = "a🦀b\r\nnext\r\n";
        let geometry = SourceGeometry::new(source);
        assert_eq!(must_with(geometry.byte_offset(source, 0, 1), "start of crab"), 1);
        assert_eq!(must_with(geometry.byte_offset(source, 0, 3), "after crab"), 5);
        assert_eq!(must_with(geometry.byte_offset(source, 0, 4), "one past line body"), 6);
        assert_eq!(must_with(geometry.byte_offset(source, 1, 4), "next line body end"), 12);
    }

    #[test]
    fn surrogate_splits_and_past_end_characters_refuse() {
        let source = "a🦀b\n";
        let geometry = SourceGeometry::new(source);
        let split = must_err_with(geometry.byte_offset(source, 0, 2), "mid-surrogate must refuse");
        assert_eq!(split, RangePositionError::SurrogateSplit { line: 0, character: 2 });
        assert!(split.message().contains("splits a surrogate pair"));
        let past = must_err_with(geometry.byte_offset(source, 0, 99), "past end must refuse");
        assert_eq!(past, RangePositionError::PastLineEnd { line: 0, character: 99, length: 4 });
        assert!(past.message().contains("outside line 0"));
    }

    #[test]
    fn out_of_bounds_end_line_refuses_instead_of_clamping() {
        let error = rejection("one\ntwo\n", range(0, 0, 9, 0));
        assert_eq!(
            error,
            RangeAdmissionError::End(RangePositionError::OutsideDocument { line: 9 })
        );
        assert!(error.message().contains("range end"));
        assert!(error.message().contains("line 9 is outside the current document"));
    }

    #[test]
    fn reversed_ranges_refuse_after_both_endpoints_map() {
        let error = rejection("abcdef", range(0, 5, 0, 2));
        assert_eq!(error, RangeAdmissionError::Reversed);
        assert_eq!(error.reason(), "reversed_range");
    }

    #[test]
    fn terminal_eof_line_points_are_admissible() {
        let point = admitted("abc\n", range(1, 0, 1, 0));
        assert_eq!(point.start_byte, 4);
        assert_eq!(point.end_byte, 4);
        assert!(point.is_empty());

        let through_eof = admitted("abc\n", range(0, 0, 1, 0));
        assert_eq!(through_eof.start_byte, 0);
        assert_eq!(through_eof.end_byte, 4, "end exclusivity admits the final separator");
    }

    #[test]
    fn empty_source_admits_the_origin_point() {
        let point = admitted("", range(0, 0, 0, 0));
        assert_eq!(point.start_byte, 0);
        assert_eq!(point.end_byte, 0);
    }

    #[test]
    fn allowed_edit_spans_match_the_requested_bytes() {
        let source = "abc\ndef\nghi";
        let geometry = SourceGeometry::new(source);

        let exact_point = admitted(source, range(0, 3, 0, 3));
        assert_eq!(must_with(exact_point.allowed_edit_span(source, &geometry), "span"), (3, 3));

        let same_line = admitted(source, range(0, 1, 0, 2));
        assert_eq!(must_with(same_line.allowed_edit_span(source, &geometry), "span"), (1, 2));

        let end_at_next_line_zero = admitted(source, range(0, 1, 1, 0));
        assert_eq!(
            must_with(end_at_next_line_zero.allowed_edit_span(source, &geometry), "span"),
            (1, 4),
            "end-at-next-line-character-zero keeps the requested start exact"
        );

        let multiline = admitted(source, range(0, 1, 2, 2));
        assert_eq!(must_with(multiline.allowed_edit_span(source, &geometry), "span"), (1, 10));

        let unterminated_tail = admitted(source, range(1, 0, 2, 3));
        assert_eq!(
            must_with(unterminated_tail.allowed_edit_span(source, &geometry), "span"),
            (4, 11)
        );
    }
}
