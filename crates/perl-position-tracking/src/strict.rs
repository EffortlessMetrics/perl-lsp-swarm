//! Strict, encoding-aware conversion between LSP wire coordinates and source bytes.
//!
//! Authoritative mutation paths must distinguish protocol-approved line-end
//! normalization from malformed code-unit boundaries. This module therefore
//! returns a typed disposition instead of a bare clamped offset.
//!
//! # Cost boundary
//!
//! These free functions are the **semantic reference**, not the editor hot
//! path. Each conversion walks the document from byte zero: once to locate the
//! line, and once more to count the character prefix. One conversion is
//! therefore `O(document length)`, and mapping many coordinates in one document
//! repeats that walk per coordinate.
//!
//! That is deliberate here — the reference implementation is kept obvious so it
//! can arbitrate disputes about encoding semantics — but this crate is called on
//! every keystroke, so:
//!
//! - use these functions for isolated conversions, correctness proofs, and as
//!   the oracle for other mappers;
//! - do **not** adopt them for repeated per-keystroke mapping. The indexed,
//!   generation-bound mapper tracked by #7881 owns that shape, with `O(1)` line
//!   lookup and `O(log lines)` byte lookup over a reusable line table.
//!
//! The provider migrations in #1690 and #7409 must consume the indexed mapper,
//! not this module.

use crate::{WirePosition, WireRange};
use serde::{Deserialize, Serialize};

/// Position encoding selected for one LSP session.
///
/// This is the canonical encoding type for the workspace. `perl-lsp-rs` still
/// carries an older `textdoc::PosEnc` with the same two variants for its
/// existing negotiation and text-sync call sites; that type is legacy, and
/// collapsing it into this one belongs to the provider migrations in #1690 and
/// #7409, which retire those call sites rather than bridging them. No
/// conversion is offered here on purpose: an unused shim would be a second
/// place to keep the two spellings in agreement, which is the duplication this
/// type exists to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub enum PositionEncoding {
    /// Count UTF-8 code units (bytes) within each line.
    #[serde(rename = "utf-8")]
    Utf8,
    /// Count UTF-16 code units within each line.
    #[default]
    #[serde(rename = "utf-16")]
    Utf16,
}

impl PositionEncoding {
    /// Return the LSP protocol spelling for this encoding.
    #[must_use]
    pub const fn as_lsp_name(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf16 => "utf-16",
        }
    }

    /// Parse one LSP protocol encoding name supported by this crate.
    #[must_use]
    pub fn from_lsp_name(name: &str) -> Option<Self> {
        match name {
            "utf-8" => Some(Self::Utf8),
            "utf-16" => Some(Self::Utf16),
            _ => None,
        }
    }

    fn code_units(self, character: char) -> u32 {
        match self {
            Self::Utf8 => character.len_utf8() as u32,
            Self::Utf16 => character.len_utf16() as u32,
        }
    }
}

/// Outcome of one strict position or range conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PositionMappingDisposition {
    /// The requested coordinate was already an exact source boundary.
    Exact,
    /// A character value beyond a valid line was normalized to its content end.
    LineEndNormalized,
    /// The requested line does not exist in the source.
    InvalidLine,
    /// The coordinate lands inside a UTF-8 or UTF-16 code point.
    InvalidCodeUnitBoundary,
    /// The range start is ordered after its end.
    InvalidRangeOrder,
    /// The byte coordinate lands inside a multi-byte newline separator.
    InvalidNewlineBoundary,
    /// Arithmetic or representational bounds prevented exact conversion.
    OverflowOrResourceBoundary,
    /// Required conversion evidence could not be produced consistently.
    InstrumentFailure,
}

impl PositionMappingDisposition {
    /// Whether the disposition authorizes an incoming source mutation endpoint.
    #[must_use]
    pub const fn admits_incoming_mutation(self) -> bool {
        matches!(self, Self::Exact | Self::LineEndNormalized)
    }

    /// Whether the conversion was exact without protocol normalization.
    #[must_use]
    pub const fn is_exact(self) -> bool {
        matches!(self, Self::Exact)
    }
}

/// Typed result of converting one wire position to source offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionMapping {
    requested: WirePosition,
    normalized: Option<WirePosition>,
    byte_offset: Option<usize>,
    char_offset: Option<usize>,
    encoding: PositionEncoding,
    disposition: PositionMappingDisposition,
}

impl PositionMapping {
    /// Original wire position supplied by the caller.
    #[must_use]
    pub const fn requested(self) -> WirePosition {
        self.requested
    }

    /// Exact or normalized wire position represented by the mapped offsets.
    #[must_use]
    pub const fn normalized(self) -> Option<WirePosition> {
        self.normalized
    }

    /// Exact source byte offset when the disposition admits mapping.
    #[must_use]
    pub const fn byte_offset(self) -> Option<usize> {
        self.byte_offset
    }

    /// Exact source character offset when the disposition admits mapping.
    #[must_use]
    pub const fn char_offset(self) -> Option<usize> {
        self.char_offset
    }

    /// Position encoding used for this conversion.
    #[must_use]
    pub const fn encoding(self) -> PositionEncoding {
        self.encoding
    }

    /// Typed conversion disposition.
    #[must_use]
    pub const fn disposition(self) -> PositionMappingDisposition {
        self.disposition
    }

    /// Whether this mapping may authorize an incoming mutation endpoint.
    #[must_use]
    pub const fn admits_incoming_mutation(self) -> bool {
        self.disposition.admits_incoming_mutation()
    }
}

/// Typed result of converting one wire range to source offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeMapping {
    start: PositionMapping,
    end: PositionMapping,
    disposition: PositionMappingDisposition,
}

impl RangeMapping {
    /// Mapping result for the inclusive range start.
    #[must_use]
    pub const fn start(self) -> PositionMapping {
        self.start
    }

    /// Mapping result for the exclusive range end.
    #[must_use]
    pub const fn end(self) -> PositionMapping {
        self.end
    }

    /// Overall range conversion disposition.
    #[must_use]
    pub const fn disposition(self) -> PositionMappingDisposition {
        self.disposition
    }

    /// Exact byte range when the complete mapping is admitted.
    #[must_use]
    pub fn byte_range(self) -> Option<std::ops::Range<usize>> {
        Some(self.start.byte_offset()?..self.end.byte_offset()?)
            .filter(|_| self.disposition.admits_incoming_mutation())
    }

    /// Exact character range when the complete mapping is admitted.
    #[must_use]
    pub fn char_range(self) -> Option<std::ops::Range<usize>> {
        Some(self.start.char_offset()?..self.end.char_offset()?)
            .filter(|_| self.disposition.admits_incoming_mutation())
    }

    /// Whether this complete range may authorize an incoming mutation.
    #[must_use]
    pub const fn admits_incoming_mutation(self) -> bool {
        self.disposition.admits_incoming_mutation()
    }
}

/// Typed result of converting one source byte offset to a wire position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BytePositionMapping {
    byte_offset: usize,
    char_offset: Option<usize>,
    wire_position: Option<WirePosition>,
    encoding: PositionEncoding,
    disposition: PositionMappingDisposition,
}

impl BytePositionMapping {
    /// Source byte offset supplied by the caller.
    #[must_use]
    pub const fn byte_offset(self) -> usize {
        self.byte_offset
    }

    /// Source character offset when the byte position was exact.
    #[must_use]
    pub const fn char_offset(self) -> Option<usize> {
        self.char_offset
    }

    /// Exact wire position when conversion succeeded.
    #[must_use]
    pub const fn wire_position(self) -> Option<WirePosition> {
        self.wire_position
    }

    /// Position encoding used for this conversion.
    #[must_use]
    pub const fn encoding(self) -> PositionEncoding {
        self.encoding
    }

    /// Typed conversion disposition.
    #[must_use]
    pub const fn disposition(self) -> PositionMappingDisposition {
        self.disposition
    }

    /// Whether the byte position converted exactly.
    #[must_use]
    pub const fn is_exact(self) -> bool {
        self.disposition.is_exact()
    }
}

/// Typed result of converting one source byte range to a wire range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRangeMapping {
    start: BytePositionMapping,
    end: BytePositionMapping,
    wire_range: Option<WireRange>,
    disposition: PositionMappingDisposition,
}

impl ByteRangeMapping {
    /// Mapping result for the inclusive byte-range start.
    #[must_use]
    pub const fn start(self) -> BytePositionMapping {
        self.start
    }

    /// Mapping result for the exclusive byte-range end.
    #[must_use]
    pub const fn end(self) -> BytePositionMapping {
        self.end
    }

    /// Exact wire range when conversion succeeded.
    #[must_use]
    pub const fn wire_range(self) -> Option<WireRange> {
        self.wire_range
    }

    /// Overall byte-range conversion disposition.
    #[must_use]
    pub const fn disposition(self) -> PositionMappingDisposition {
        self.disposition
    }

    /// Whether the complete byte range converted exactly.
    #[must_use]
    pub const fn is_exact(self) -> bool {
        self.disposition.is_exact()
    }
}

#[derive(Debug, Clone, Copy)]
struct LineBounds {
    line: u32,
    start: usize,
    content_end: usize,
}

enum ByteLineLookup {
    Content(LineBounds),
    NewlineInterior,
    OutOfBounds,
    Overflow,
}

/// One logical LSP line together with the end of its separator.
#[derive(Debug, Clone, Copy)]
struct ScannedLine {
    bounds: LineBounds,
    /// First byte after this line's separator; equals `content_end` on the
    /// final line, which has no separator.
    separator_end: usize,
}

/// Single walk over logical LSP lines, shared by every lookup below.
///
/// Both lookups need the same CR / LF / CRLF separator rule and the same
/// `u32` line-overflow guard. Keeping one scan means a change to line-end
/// handling cannot be applied to one lookup and forgotten in the other.
struct LineScan<'a> {
    bytes: &'a [u8],
    line: u32,
    start: usize,
    index: usize,
    exhausted: bool,
    overflowed: bool,
}

impl<'a> LineScan<'a> {
    const fn new(source: &'a str) -> Self {
        Self {
            bytes: source.as_bytes(),
            line: 0,
            start: 0,
            index: 0,
            exhausted: false,
            overflowed: false,
        }
    }

    /// Yield the next line, or `None` once the final line has been produced or
    /// the line counter would exceed `u32`.
    fn next_line(&mut self) -> Option<ScannedLine> {
        if self.exhausted {
            return None;
        }

        let mut index = self.index;
        while index < self.bytes.len() && !matches!(self.bytes[index], b'\r' | b'\n') {
            index += 1;
        }

        let content_end = index;
        let is_final = index == self.bytes.len();
        let separator_end = if is_final {
            index
        } else if self.bytes[index] == b'\r'
            && index + 1 < self.bytes.len()
            && self.bytes[index + 1] == b'\n'
        {
            index + 2
        } else {
            index + 1
        };

        let bounds = LineBounds { line: self.line, start: self.start, content_end };

        if is_final {
            self.exhausted = true;
        } else if let Some(next_line) = self.line.checked_add(1) {
            self.line = next_line;
            self.start = separator_end;
            self.index = separator_end;
        } else {
            self.exhausted = true;
            self.overflowed = true;
        }

        Some(ScannedLine { bounds, separator_end })
    }

    /// Whether the scan stopped because the line counter exceeded `u32`
    /// rather than because the document ended.
    const fn overflowed(&self) -> bool {
        self.overflowed
    }
}

fn line_bounds(source: &str, target_line: u32) -> Option<LineBounds> {
    let mut scan = LineScan::new(source);
    while let Some(line) = scan.next_line() {
        if line.bounds.line == target_line {
            return Some(line.bounds);
        }
    }
    None
}

fn lookup_byte_line(source: &str, target_byte: usize) -> ByteLineLookup {
    if target_byte > source.len() {
        return ByteLineLookup::OutOfBounds;
    }

    let mut scan = LineScan::new(source);
    while let Some(line) = scan.next_line() {
        if (line.bounds.start..=line.bounds.content_end).contains(&target_byte) {
            return ByteLineLookup::Content(line.bounds);
        }
        if target_byte > line.bounds.content_end && target_byte < line.separator_end {
            return ByteLineLookup::NewlineInterior;
        }
    }

    if scan.overflowed() { ByteLineLookup::Overflow } else { ByteLineLookup::OutOfBounds }
}

fn invalid_wire_mapping(
    requested: WirePosition,
    encoding: PositionEncoding,
    disposition: PositionMappingDisposition,
) -> PositionMapping {
    PositionMapping {
        requested,
        normalized: None,
        byte_offset: None,
        char_offset: None,
        encoding,
        disposition,
    }
}

fn mapped_wire_position(
    source: &str,
    requested: WirePosition,
    normalized: WirePosition,
    byte_offset: usize,
    encoding: PositionEncoding,
    disposition: PositionMappingDisposition,
) -> PositionMapping {
    let char_offset = source.get(..byte_offset).map(|prefix| prefix.chars().count());
    let disposition = if char_offset.is_some() {
        disposition
    } else {
        PositionMappingDisposition::InstrumentFailure
    };

    PositionMapping {
        requested,
        normalized: char_offset.map(|_| normalized),
        byte_offset: char_offset.map(|_| byte_offset),
        char_offset,
        encoding,
        disposition,
    }
}

fn invalid_byte_mapping(
    byte_offset: usize,
    encoding: PositionEncoding,
    disposition: PositionMappingDisposition,
) -> BytePositionMapping {
    BytePositionMapping {
        byte_offset,
        char_offset: None,
        wire_position: None,
        encoding,
        disposition,
    }
}

/// Convert one LSP wire position to source byte and character offsets.
///
/// Positions beyond an existing line's content are normalized to that line end.
/// Positions inside a UTF-8 or UTF-16 code point are rejected.
#[must_use]
pub fn wire_position_to_byte(
    source: &str,
    position: WirePosition,
    encoding: PositionEncoding,
) -> PositionMapping {
    let Some(bounds) = line_bounds(source, position.line) else {
        return invalid_wire_mapping(position, encoding, PositionMappingDisposition::InvalidLine);
    };
    let Some(line_text) = source.get(bounds.start..bounds.content_end) else {
        return invalid_wire_mapping(
            position,
            encoding,
            PositionMappingDisposition::InstrumentFailure,
        );
    };

    let mut code_units = 0u32;
    let mut line_bytes = 0usize;

    for character in line_text.chars() {
        if code_units == position.character {
            return mapped_wire_position(
                source,
                position,
                position,
                bounds.start + line_bytes,
                encoding,
                PositionMappingDisposition::Exact,
            );
        }

        let Some(next_units) = code_units.checked_add(encoding.code_units(character)) else {
            return invalid_wire_mapping(
                position,
                encoding,
                PositionMappingDisposition::OverflowOrResourceBoundary,
            );
        };
        if position.character < next_units {
            return invalid_wire_mapping(
                position,
                encoding,
                PositionMappingDisposition::InvalidCodeUnitBoundary,
            );
        }

        code_units = next_units;
        line_bytes += character.len_utf8();
    }

    let disposition = if position.character == code_units {
        PositionMappingDisposition::Exact
    } else {
        PositionMappingDisposition::LineEndNormalized
    };
    let normalized = WirePosition { line: position.line, character: code_units };
    mapped_wire_position(source, position, normalized, bounds.content_end, encoding, disposition)
}

/// Convert one LSP wire range to exact source byte and character ranges.
#[must_use]
pub fn wire_range_to_bytes(
    source: &str,
    range: WireRange,
    encoding: PositionEncoding,
) -> RangeMapping {
    let start = wire_position_to_byte(source, range.start, encoding);
    let end = wire_position_to_byte(source, range.end, encoding);

    let requested_reversed =
        (range.start.line, range.start.character) > (range.end.line, range.end.character);
    let disposition = if requested_reversed {
        PositionMappingDisposition::InvalidRangeOrder
    } else if !start.admits_incoming_mutation() {
        start.disposition()
    } else if !end.admits_incoming_mutation() {
        end.disposition()
    } else if start.byte_offset() > end.byte_offset() {
        PositionMappingDisposition::InvalidRangeOrder
    } else if matches!(
        (start.disposition(), end.disposition()),
        (PositionMappingDisposition::LineEndNormalized, _)
            | (_, PositionMappingDisposition::LineEndNormalized)
    ) {
        PositionMappingDisposition::LineEndNormalized
    } else {
        PositionMappingDisposition::Exact
    };

    RangeMapping { start, end, disposition }
}

/// Convert one exact source byte boundary to a wire position.
///
/// Byte offsets inside a UTF-8 code point or inside the second byte of CRLF are
/// rejected rather than clamped.
#[must_use]
pub fn byte_to_wire_position(
    source: &str,
    byte_offset: usize,
    encoding: PositionEncoding,
) -> BytePositionMapping {
    if byte_offset > source.len() {
        return invalid_byte_mapping(
            byte_offset,
            encoding,
            PositionMappingDisposition::OverflowOrResourceBoundary,
        );
    }
    if !source.is_char_boundary(byte_offset) {
        return invalid_byte_mapping(
            byte_offset,
            encoding,
            PositionMappingDisposition::InvalidCodeUnitBoundary,
        );
    }

    let bounds = match lookup_byte_line(source, byte_offset) {
        ByteLineLookup::Content(bounds) => bounds,
        ByteLineLookup::NewlineInterior => {
            return invalid_byte_mapping(
                byte_offset,
                encoding,
                PositionMappingDisposition::InvalidNewlineBoundary,
            );
        }
        ByteLineLookup::OutOfBounds | ByteLineLookup::Overflow => {
            return invalid_byte_mapping(
                byte_offset,
                encoding,
                PositionMappingDisposition::OverflowOrResourceBoundary,
            );
        }
    };

    let Some(line_prefix) = source.get(bounds.start..byte_offset) else {
        return invalid_byte_mapping(
            byte_offset,
            encoding,
            PositionMappingDisposition::InstrumentFailure,
        );
    };
    let mut character = 0u32;
    for source_character in line_prefix.chars() {
        let Some(next) = character.checked_add(encoding.code_units(source_character)) else {
            return invalid_byte_mapping(
                byte_offset,
                encoding,
                PositionMappingDisposition::OverflowOrResourceBoundary,
            );
        };
        character = next;
    }
    let Some(char_offset) = source.get(..byte_offset).map(|prefix| prefix.chars().count()) else {
        return invalid_byte_mapping(
            byte_offset,
            encoding,
            PositionMappingDisposition::InstrumentFailure,
        );
    };

    BytePositionMapping {
        byte_offset,
        char_offset: Some(char_offset),
        wire_position: Some(WirePosition { line: bounds.line, character }),
        encoding,
        disposition: PositionMappingDisposition::Exact,
    }
}

/// Convert one exact source byte range to an LSP wire range.
#[must_use]
pub fn byte_range_to_wire_range(
    source: &str,
    byte_range: std::ops::Range<usize>,
    encoding: PositionEncoding,
) -> ByteRangeMapping {
    let start = byte_to_wire_position(source, byte_range.start, encoding);
    let end = byte_to_wire_position(source, byte_range.end, encoding);
    let disposition = if byte_range.start > byte_range.end {
        PositionMappingDisposition::InvalidRangeOrder
    } else if !start.is_exact() {
        start.disposition()
    } else if !end.is_exact() {
        end.disposition()
    } else {
        PositionMappingDisposition::Exact
    };
    let wire_range = if disposition.is_exact() {
        match (start.wire_position(), end.wire_position()) {
            (Some(start), Some(end)) => Some(WireRange { start, end }),
            _ => None,
        }
    } else {
        None
    };
    let disposition = if disposition.is_exact() && wire_range.is_none() {
        PositionMappingDisposition::InstrumentFailure
    } else {
        disposition
    };

    ByteRangeMapping { start, end, wire_range, disposition }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_names_are_protocol_exact() {
        assert_eq!(PositionEncoding::Utf8.as_lsp_name(), "utf-8");
        assert_eq!(PositionEncoding::Utf16.as_lsp_name(), "utf-16");
        assert_eq!(PositionEncoding::from_lsp_name("utf-8"), Some(PositionEncoding::Utf8));
        assert_eq!(PositionEncoding::from_lsp_name("utf-32"), None);
    }

    #[test]
    fn utf16_mid_surrogate_is_invalid_for_strict_mapping() {
        let mapping = wire_position_to_byte(
            "x😀y",
            WirePosition { line: 0, character: 2 },
            PositionEncoding::Utf16,
        );

        assert_eq!(mapping.disposition(), PositionMappingDisposition::InvalidCodeUnitBoundary);
        assert_eq!(mapping.byte_offset(), None);
    }

    #[test]
    fn utf8_mid_code_point_is_invalid_for_strict_mapping() {
        let mapping = wire_position_to_byte(
            "x😀y",
            WirePosition { line: 0, character: 2 },
            PositionEncoding::Utf8,
        );

        assert_eq!(mapping.disposition(), PositionMappingDisposition::InvalidCodeUnitBoundary);
        assert_eq!(mapping.byte_offset(), None);
    }

    #[test]
    fn character_beyond_line_content_is_normalized_to_line_end() {
        let mapping = wire_position_to_byte(
            "abc\r\ndef",
            WirePosition { line: 0, character: 99 },
            PositionEncoding::Utf16,
        );

        assert_eq!(mapping.disposition(), PositionMappingDisposition::LineEndNormalized);
        assert_eq!(mapping.normalized(), Some(WirePosition { line: 0, character: 3 }));
        assert_eq!(mapping.byte_offset(), Some(3));
    }

    #[test]
    fn line_outside_document_is_invalid() {
        let mapping = wire_position_to_byte(
            "abc",
            WirePosition { line: 1, character: 0 },
            PositionEncoding::Utf16,
        );

        assert_eq!(mapping.disposition(), PositionMappingDisposition::InvalidLine);
    }

    #[test]
    fn trailing_newline_creates_an_empty_final_line() {
        let mapping = wire_position_to_byte(
            "abc\n",
            WirePosition { line: 1, character: 0 },
            PositionEncoding::Utf16,
        );

        assert_eq!(mapping.disposition(), PositionMappingDisposition::Exact);
        assert_eq!(mapping.byte_offset(), Some(4));
    }

    #[test]
    fn bare_cr_and_mixed_endings_use_logical_lsp_lines() {
        let source = "a\rb\r\nc\nd";
        for (line, byte) in [(0, 0), (1, 2), (2, 5), (3, 7)] {
            let mapping = wire_position_to_byte(
                source,
                WirePosition { line, character: 0 },
                PositionEncoding::Utf16,
            );
            assert_eq!(mapping.byte_offset(), Some(byte));
        }
    }

    #[test]
    fn outgoing_position_inside_crlf_is_invalid() {
        let mapping = byte_to_wire_position("abc\r\ndef", 4, PositionEncoding::Utf16);

        assert_eq!(mapping.disposition(), PositionMappingDisposition::InvalidNewlineBoundary);
        assert_eq!(mapping.wire_position(), None);
    }

    #[test]
    fn valid_boundaries_round_trip_under_both_encodings() {
        let source = "x😀y\r\nz";
        for encoding in [PositionEncoding::Utf8, PositionEncoding::Utf16] {
            for byte in [0, 1, 5, 6, 8, 9] {
                let outgoing = byte_to_wire_position(source, byte, encoding);
                assert_eq!(outgoing.disposition(), PositionMappingDisposition::Exact);
                assert!(
                    outgoing.wire_position().is_some(),
                    "exact outgoing mapping must contain a wire position"
                );
                if let Some(position) = outgoing.wire_position() {
                    let incoming = wire_position_to_byte(source, position, encoding);
                    assert_eq!(incoming.byte_offset(), Some(byte));
                }
            }
        }
    }

    #[test]
    fn reversed_wire_range_is_rejected() {
        let mapping = wire_range_to_bytes(
            "abc",
            WireRange {
                start: WirePosition { line: 0, character: 2 },
                end: WirePosition { line: 0, character: 1 },
            },
            PositionEncoding::Utf16,
        );

        assert_eq!(mapping.disposition(), PositionMappingDisposition::InvalidRangeOrder);
        assert_eq!(mapping.byte_range(), None);
    }

    #[test]
    fn exact_byte_range_converts_without_clamping() {
        let mapping = byte_range_to_wire_range("x😀y", 1..5, PositionEncoding::Utf16);

        assert_eq!(mapping.disposition(), PositionMappingDisposition::Exact);
        assert_eq!(
            mapping.wire_range(),
            Some(WireRange {
                start: WirePosition { line: 0, character: 1 },
                end: WirePosition { line: 0, character: 3 },
            })
        );
    }

    #[test]
    fn admitted_wire_range_reports_byte_and_char_ranges() {
        let mapping = wire_range_to_bytes(
            "x😀y",
            WireRange {
                start: WirePosition { line: 0, character: 1 },
                end: WirePosition { line: 0, character: 3 },
            },
            PositionEncoding::Utf16,
        );

        assert_eq!(mapping.disposition(), PositionMappingDisposition::Exact);
        assert_eq!(mapping.byte_range(), Some(1..5), "byte range must not clamp");
        assert_eq!(
            mapping.char_range(),
            Some(1..2),
            "char range counts code points, not UTF-16 units"
        );
    }

    #[test]
    fn admitted_byte_position_reports_char_offset() {
        let mapping = byte_to_wire_position("x😀y", 5, PositionEncoding::Utf16);

        assert_eq!(mapping.disposition(), PositionMappingDisposition::Exact);
        assert_eq!(
            mapping.char_offset(),
            Some(2),
            "char offset counts code points before the byte offset"
        );
        assert_eq!(
            mapping.wire_position(),
            Some(WirePosition { line: 0, character: 3 }),
            "the same boundary is 3 UTF-16 units in"
        );
    }

    #[test]
    fn reversed_byte_range_is_rejected() {
        // Built field-wise: a literal `2..1` is a lint-flagged empty range, but
        // a reversed range is exactly the malformed input under test.
        let reversed = std::ops::Range { start: 2, end: 1 };
        let mapping = byte_range_to_wire_range("abc", reversed, PositionEncoding::Utf16);

        assert_eq!(mapping.disposition(), PositionMappingDisposition::InvalidRangeOrder);
        assert_eq!(mapping.wire_range(), None);
    }

    #[test]
    fn line_scan_lookups_agree_on_mixed_line_endings() {
        // One scan now serves both lookups; this pins them to the same notion
        // of where each logical line starts and ends.
        let source = "a\r\nbb\rccc\ndddd";

        for (line, expected_start) in [(0u32, 0usize), (1, 3), (2, 6), (3, 10)] {
            let scanned = line_bounds(source, line);
            assert!(scanned.is_some(), "line {line} must exist in the fixture");
            let Some(bounds) = scanned else { continue };
            assert_eq!(bounds.start, expected_start, "line {line} start");

            let found = lookup_byte_line(source, bounds.start);
            assert!(
                matches!(found, ByteLineLookup::Content(_)),
                "line {line} start must resolve to line content"
            );
            let ByteLineLookup::Content(found) = found else { continue };
            assert_eq!(found.line, line, "byte {} maps back to its line", bounds.start);
            assert_eq!(found.start, bounds.start);
            assert_eq!(found.content_end, bounds.content_end);
        }

        assert!(line_bounds(source, 4).is_none(), "there is no fifth line");
        assert!(
            matches!(lookup_byte_line(source, 2), ByteLineLookup::NewlineInterior),
            "the byte between CR and LF is interior to the separator"
        );
        assert!(
            matches!(lookup_byte_line(source, source.len() + 1), ByteLineLookup::OutOfBounds),
            "a byte past the document is out of bounds"
        );
    }
}
