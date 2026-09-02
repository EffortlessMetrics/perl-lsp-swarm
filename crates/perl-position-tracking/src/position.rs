//! Enhanced position tracking for incremental parsing
//!
//! This module provides position and range types that track byte offsets,
//! lines, and columns for efficient incremental parsing and error reporting.
//!
//! # Wire Types vs Engine Types
//!
//! This module defines **engine types** used internally for parsing and AST tracking.
//! For LSP wire protocol serialization, use `crate::WirePosition` and `crate::WireRange`.
//!
//! # Engine coordinate contract
//!
//! Engine [`Position`] carries three coordinates with fixed bases and units:
//!
//! - `byte`: 0-based byte offset into the source text;
//! - `line`: 1-based line number for human display;
//! - `column`: 1-based column counted in Unicode scalar values (chars), not
//!   bytes and not UTF-16 code units.
//!
//! The engine newline domain is a single `\n` (LF): `advance` starts a new
//! line at `\n` and treats every other scalar value, including `\r`, as an
//! ordinary column character. CRLF/CR-only source handling and UTF-16
//! interpretation belong to the wire adapters
//! ([`crate::WirePosition`], `crate::strict`), not to these types.
//!
//! [`Position::default`] matches [`Position::start`] (origin of the file:
//! byte 0, line 1, column 1) so the zero-derived default cannot smuggle in an
//! off-by-one line/column base.
//!
//! Engine [`Range`] is a half-open byte-ordered interval: [`Range::new`]
//! orders its endpoints so every constructed range is well ordered, while
//! [`Range::try_new`] plus deserialization validation reject reversed
//! intervals. Empty (`start.byte == end.byte`) and reversed
//! (`start.byte > end.byte`) remain distinct states.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A position in a source file with byte offset, line, and column
///
/// This is an **engine type** for internal parsing use. It tracks byte offsets
/// and 1-based line/column for human-friendly display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Position {
    /// Byte offset in the source (0-based)
    pub byte: usize,
    /// Line number (1-based for user display)
    pub line: u32,
    /// Column number (1-based, counted in Unicode scalar values)
    pub column: u32,
}

impl Default for Position {
    fn default() -> Self {
        Position::start()
    }
}

impl Position {
    /// Create a new position
    pub fn new(byte: usize, line: u32, column: u32) -> Self {
        Position { byte, line, column }
    }

    /// Create a position at the start of a file
    pub const fn start() -> Self {
        Position { byte: 0, line: 1, column: 1 }
    }

    /// Advance the position by the given text
    ///
    /// The newline domain is LF only: `\n` starts a new line, and every other
    /// scalar value (including `\r`) advances the column by one.
    pub fn advance(&mut self, text: &str) {
        for ch in text.chars() {
            self.advance_char(ch);
        }
    }

    /// Advance by a single character
    pub fn advance_char(&mut self, ch: char) {
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        self.byte += ch.len_utf8();
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// Error returned when a range would be reversed (`start.byte > end.byte`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReversedRange {
    /// The rejected start position.
    pub start: Position,
    /// The rejected end position.
    pub end: Position,
}

impl fmt::Display for ReversedRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "range start byte {} exceeds end byte {}", self.start.byte, self.end.byte)
    }
}

impl std::error::Error for ReversedRange {}

/// A range in a source file defined by start and end positions
///
/// This is an **engine type**. The interval is half-open and byte-ordered:
/// [`Range::new`] trusts callers (asserting ordering in debug builds),
/// [`Range::try_new`] rejects reversed intervals, and deserialization
/// validates ordering instead of accepting hidden reversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Range {
    /// Start position (inclusive)
    pub start: Position,
    /// End position (exclusive)
    pub end: Position,
}

impl Range {
    /// Create a new range, ordering the endpoints by byte offset.
    ///
    /// This constructor is total and ordering-correcting: the returned range
    /// always satisfies `start.byte <= end.byte`, with the endpoints swapped
    /// when they arrive reversed. Callers that must reject reversed input
    /// instead of normalizing it use [`Range::try_new`]; deserialization is
    /// strict.
    pub fn new(start: Position, end: Position) -> Self {
        if start.byte <= end.byte { Range { start, end } } else { Range { start: end, end: start } }
    }

    /// Create a checked range, failing if the byte offsets are reversed.
    ///
    /// # Errors
    ///
    /// Returns [`ReversedRange`] when `start.byte > end.byte`.
    pub fn try_new(start: Position, end: Position) -> Result<Self, ReversedRange> {
        if start.byte <= end.byte {
            Ok(Range { start, end })
        } else {
            Err(ReversedRange { start, end })
        }
    }

    /// Create an empty range at a position
    pub fn empty(pos: Position) -> Self {
        Range { start: pos, end: pos }
    }

    /// Check if the range contains a byte offset
    pub fn contains_byte(&self, byte: usize) -> bool {
        self.start.byte <= byte && byte < self.end.byte
    }

    /// Check if the range contains a position
    pub fn contains(&self, pos: Position) -> bool {
        self.start.byte <= pos.byte && pos.byte < self.end.byte
    }

    /// Check if this range overlaps with another
    pub fn overlaps(&self, other: &Range) -> bool {
        self.start.byte < other.end.byte && other.start.byte < self.end.byte
    }

    /// Get the length in bytes
    pub fn len(&self) -> usize {
        self.end.byte.saturating_sub(self.start.byte)
    }

    /// Check if the range is empty (zero byte length)
    ///
    /// An empty range has `start.byte == end.byte`. A reversed range
    /// (`start.byte > end.byte`) is not empty; it is reported by
    /// [`Range::is_reversed`] instead.
    pub fn is_empty(&self) -> bool {
        self.start.byte == self.end.byte
    }

    /// Check if the range is reversed (`start.byte > end.byte`)
    ///
    /// Reversed and empty are distinct states: only well-ordered ranges are
    /// empty, and a reversed range never is.
    pub fn is_reversed(&self) -> bool {
        self.start.byte > self.end.byte
    }

    /// Extend this range to include another range
    pub fn extend(&mut self, other: &Range) {
        if other.start.byte < self.start.byte {
            self.start = other.start;
        }
        if other.end.byte > self.end.byte {
            self.end = other.end;
        }
    }

    /// Create a range that spans from this range to another
    pub fn span_to(&self, other: &Range) -> Range {
        Range {
            start: if self.start.byte < other.start.byte { self.start } else { other.start },
            end: if self.end.byte > other.end.byte { self.end } else { other.end },
        }
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.start, self.end)
    }
}

impl<'de> Deserialize<'de> for Range {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Repr {
            start: Position,
            end: Position,
        }
        let repr = Repr::deserialize(deserializer)?;
        Range::try_new(repr.start, repr.end).map_err(|e| serde::de::Error::custom(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_default_matches_start_origin() {
        assert_eq!(Position::default(), Position::start());
        assert_eq!(Position::default(), Position { byte: 0, line: 1, column: 1 });
    }

    #[test]
    fn test_position_advance() {
        let mut pos = Position::start();
        assert_eq!(pos, Position { byte: 0, line: 1, column: 1 });

        pos.advance("hello");
        assert_eq!(pos, Position { byte: 5, line: 1, column: 6 });

        pos.advance("\n");
        assert_eq!(pos, Position { byte: 6, line: 2, column: 1 });

        pos.advance("世界"); // UTF-8 multibyte
        assert_eq!(pos, Position { byte: 12, line: 2, column: 3 });
    }

    #[test]
    fn test_position_advance_newline_domain_is_lf_only() {
        // CRLF: \r is an ordinary column character in the engine domain; only
        // \n starts a new line.
        let mut pos = Position::start();
        pos.advance("\r\n");
        assert_eq!(pos, Position { byte: 2, line: 2, column: 1 });

        // Bare CR never starts a line in the engine domain.
        let mut bare_cr = Position::start();
        bare_cr.advance("a\rb");
        assert_eq!(bare_cr, Position { byte: 3, line: 1, column: 4 });
    }

    #[test]
    fn test_range_operations() {
        let start = Position::new(10, 2, 5);
        let end = Position::new(20, 3, 10);
        let range = Range::new(start, end);

        assert!(range.contains_byte(15));
        assert!(!range.contains_byte(25));
        assert_eq!(range.len(), 10);

        let other = Range::new(Position::new(15, 2, 10), Position::new(25, 4, 5));

        assert!(range.overlaps(&other));

        let span = range.span_to(&other);
        assert_eq!(span.start.byte, 10);
        assert_eq!(span.end.byte, 25);
    }

    #[test]
    fn test_range_new_orders_reversed_positions() {
        // #8740: the trusted constructor is order-correcting, so reversed
        // endpoints can never produce a reversed range.
        let reversed = Range::new(Position::new(9, 1, 10), Position::new(3, 1, 4));
        assert_eq!(reversed.start.byte, 3);
        assert_eq!(reversed.end.byte, 9);
        assert!(!reversed.is_reversed());
    }

    #[test]
    fn test_range_try_new_accepts_ordered_positions() {
        let range = Range::try_new(Position::new(3, 1, 4), Position::new(9, 1, 10));
        assert_eq!(range, Ok(Range::new(Position::new(3, 1, 4), Position::new(9, 1, 10))));
    }

    #[test]
    fn test_range_try_new_rejects_reversed_positions() {
        let start = Position::new(9, 1, 10);
        let end = Position::new(3, 1, 4);
        assert_eq!(Range::try_new(start, end), Err(ReversedRange { start, end }));
    }

    #[test]
    fn test_range_empty_and_reversed_remain_distinct() {
        let pos = Position::new(5, 1, 6);
        assert!(Range::empty(pos).is_empty());
        assert!(!Range::empty(pos).is_reversed());
    }

    #[test]
    fn test_range_serde_round_trip_preserves_range() -> Result<(), serde_json::Error> {
        let range = Range::new(Position::new(2, 1, 3), Position::new(8, 1, 9));
        let json = serde_json::to_string(&range)?;
        let back: Range = serde_json::from_str(&json)?;
        assert_eq!(back, range);
        Ok(())
    }

    #[test]
    fn test_range_serde_rejects_reversed_range() {
        let json =
            r#"{"start":{"byte":9,"line":1,"column":10},"end":{"byte":3,"line":1,"column":4}}"#;
        let result: Result<Range, _> = serde_json::from_str(json);
        assert!(result.is_err(), "deserialization must fail closed on reversed ranges");
    }
}
