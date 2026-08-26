//! Half-open original-source byte ranges and derived UTF-8 line/column projection.

use super::failure::OutcomeError;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

/// Half-open byte range `[start, end)` over the caller's original source.
///
/// Line and column are not stored. Project them with
/// [`SourceRange::line_column`] when a display form is needed.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct SourceRange {
    start: usize,
    end: usize,
}

/// 0-based UTF-8 line and Unicode-scalar column span derived from a [`SourceRange`].
///
/// This is a projection of original-source bytes, not a second range authority.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceLineColumn {
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
}

impl SourceRange {
    /// Construct a half-open range that does not yet know its source length.
    ///
    /// Fails when `start > end`. Binding to concrete source bytes still requires
    /// [`SourceRange::try_over_source`] or [`SourceRange::check_over_source`].
    pub fn try_new(start: usize, end: usize) -> Result<Self, OutcomeError> {
        if start > end {
            return Err(OutcomeError::InvertedRange { start, end });
        }
        Ok(Self { start, end })
    }

    /// Construct a range and require it to lie on UTF-8 boundaries of `source`.
    pub fn try_over_source(start: usize, end: usize, source: &str) -> Result<Self, OutcomeError> {
        let range = Self::try_new(start, end)?;
        range.check_over_source(source)?;
        Ok(range)
    }

    /// Inclusive start byte offset.
    #[must_use]
    pub const fn start(self) -> usize {
        self.start
    }

    /// Exclusive end byte offset.
    #[must_use]
    pub const fn end(self) -> usize {
        self.end
    }

    /// `end - start` in bytes.
    #[must_use]
    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether the range is an insertion point (`start == end`).
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Fail if this range is inverted, out of bounds, or not on a UTF-8 character boundary.
    pub fn check_over_source(self, source: &str) -> Result<(), OutcomeError> {
        if self.start > self.end {
            return Err(OutcomeError::InvertedRange { start: self.start, end: self.end });
        }
        let source_len = source.len();
        if self.end > source_len {
            return Err(OutcomeError::OutOfBounds { start: self.start, end: self.end, source_len });
        }
        if !source.is_char_boundary(self.start) || !source.is_char_boundary(self.end) {
            return Err(OutcomeError::InvalidUtf8Boundary { start: self.start, end: self.end });
        }
        Ok(())
    }

    /// Derived 0-based UTF-8 line/column span. Newlines are `\n`, `\r\n`, or bare `\r`.
    pub fn line_column(self, source: &str) -> Result<SourceLineColumn, OutcomeError> {
        self.check_over_source(source)?;
        let (start_line, start_column) = offset_to_line_column(source, self.start);
        let (end_line, end_column) = offset_to_line_column(source, self.end);
        Ok(SourceLineColumn { start_line, start_column, end_line, end_column })
    }

    /// Sort ranges by start then end and reject overlapping or duplicate ranges.
    ///
    /// Adjacent half-open ranges (`[0, 4)` and `[4, 8)`) are allowed. Identical
    /// ranges, including duplicate empty insertion points, are overlapping.
    pub fn sort_and_check_disjoint(
        ranges: impl Into<Vec<Self>>,
    ) -> Result<Vec<Self>, OutcomeError> {
        let mut ranges = ranges.into();
        ranges.sort_by(|left, right| left.start.cmp(&right.start).then(left.end.cmp(&right.end)));
        let mut previous: Option<Self> = None;
        for range in &ranges {
            match previous {
                Some(left) if left == *range || left.end > range.start => {
                    return Err(OutcomeError::overlapping(left, *range));
                }
                _ => previous = Some(*range),
            }
        }
        Ok(ranges)
    }
}

impl SourceLineColumn {
    /// 0-based start line.
    #[must_use]
    pub const fn start_line(self) -> usize {
        self.start_line
    }

    /// 0-based Unicode-scalar column at the start offset.
    #[must_use]
    pub const fn start_column(self) -> usize {
        self.start_column
    }

    /// 0-based end line.
    #[must_use]
    pub const fn end_line(self) -> usize {
        self.end_line
    }

    /// 0-based Unicode-scalar column at the exclusive end offset.
    #[must_use]
    pub const fn end_column(self) -> usize {
        self.end_column
    }
}

impl fmt::Display for SourceRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}, {})", self.start, self.end)
    }
}

impl<'de> Deserialize<'de> for SourceRange {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct SourceRangeWire {
            start: usize,
            end: usize,
        }
        let wire = SourceRangeWire::deserialize(deserializer)?;
        Self::try_new(wire.start, wire.end).map_err(D::Error::custom)
    }
}

fn offset_to_line_column(source: &str, offset: usize) -> (usize, usize) {
    let prefix = match source.get(..offset) {
        Some(prefix) => prefix,
        None => return (0, 0),
    };
    let bytes = prefix.as_bytes();
    let mut line = 0usize;
    let mut last_line_start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'\n' {
            line = line.saturating_add(1);
            last_line_start = index.saturating_add(1);
            index = index.saturating_add(1);
        } else if bytes[index] == b'\r' {
            line = line.saturating_add(1);
            if bytes.get(index + 1) == Some(&b'\n') {
                last_line_start = index.saturating_add(2);
                index = index.saturating_add(2);
            } else {
                last_line_start = index.saturating_add(1);
                index = index.saturating_add(1);
            }
        } else {
            index = index.saturating_add(1);
        }
    }
    let column = match prefix.get(last_line_start..) {
        Some(rest) => rest.chars().count(),
        None => 0,
    };
    (line, column)
}
