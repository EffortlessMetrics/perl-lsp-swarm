//! Chunk-stable LF source-line geometry for exact UTF-8 source snapshots.
//!
//! This module is the canonical byte-only source-row authority decided by
//! #4973 and specified by #10574. It defines logical source rows exactly once
//! so the parser, LSP mapping, Tree-sitter compatibility, DAP, and
//! source-bound services cannot disagree about where one source row ends:
//!
//! ```text
//! LF       = the only logical source-line terminator
//! CRLF     = one two-byte separator whose LF terminates the row
//! bare CR  = ordinary source content
//! VT/FF/NEL/LS/PS = ordinary source content
//! ```
//!
//! A [`LineRecordTable`] records, per row, three exact byte boundaries plus
//! the separator kind. It contains no UTF-16 columns, Unicode-scalar columns,
//! LSP positions, Tree-sitter `Point`s, DAP bases, document generations, or
//! mapper caches: those belong to protocol consumers built on top of this
//! table (for example the indexed mapper in #7881).
//!
//! # Chunk stability
//!
//! [`LineRecordTable::from_chunks_utf8`] accepts borrowed byte chunks from the
//! exact source owner and produces the same table as
//! [`LineRecordTable::from_str`] for *every* chunk partition, including a
//! partition that splits the CR of a CRLF pair into a different chunk from its
//! LF, and partitions that split inside a multi-byte UTF-8 scalar. The
//! complete source must still be valid UTF-8; validity is verified while
//! streaming without concatenating or copying the source.
//!
//! # Relationship to the rest of this crate
//!
//! [`crate::LineStartsCache`], [`crate::LineIndex`], and [`crate::PositionMapper`]
//! predate this contract and split into two distinct legacy row models, not one:
//!
//! ```text
//! Ropey model (LF, CRLF, CR, VT, FF, NEL, LS, PS)
//!   LineStartsCache::new_rope, PositionMapper   — Rope line APIs
//!
//! CR-aware model (LF, CRLF, CR)
//!   LineStartsCache::new, LineIndex             — local scan
//! ```
//!
//! So bare CR breaks a row on all of them, but VT/FF/NEL/LS/PS break a row only
//! on the Rope-backed queries. They are legacy surfaces; new exact-source
//! consumers should build on this table instead. The exact divergence is pinned
//! in `tests/source_line_policy_authority.rs`. Reconciling these constructors is
//! explicitly out of scope here (ADR-0048 / #4973 follow-up, owned by #8687).

use crate::span::ByteSpan;
use std::fmt;
use thiserror::Error;

/// Identity of the accepted source-line policy a [`LineRecordTable`] encodes.
///
/// The value pins the LF-only ruling (#4973 / #10574): LF terminates a row,
/// CRLF is one two-byte separator, and bare CR, VT, FF, NEL, LS, and PS are
/// ordinary content. If the policy ever changes, this identity changes with it
/// so stale tables can never be silently reinterpreted.
pub const SOURCE_LINE_POLICY_ID: &str = "lf-source-lines/v1";

/// Which bytes terminate a [`LineRecord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SeparatorKind {
    /// The record ends at end-of-source and has no separator bytes.
    None,
    /// The record is terminated by one LF byte (`0x0A`).
    Lf,
    /// The record is terminated by one CRLF pair (`0x0D 0x0A`).
    CrLf,
}

impl SeparatorKind {
    /// Byte length of the separator (`None` → 0, `Lf` → 1, `CrLf` → 2).
    #[must_use]
    pub const fn byte_len(self) -> usize {
        match self {
            Self::None => 0,
            Self::Lf => 1,
            Self::CrLf => 2,
        }
    }
}

/// One source row: exact byte geometry only.
///
/// The record carries no protocol coordinates. The laws it upholds:
///
/// ```text
/// start_byte <= content_end_byte <= separator_end_byte <= source length
/// separator bytes are excluded from content_end_byte
/// only an LF can end a nonterminal record
/// ```
///
/// For `"abc\r\ndef"` the first record is
/// `{start 0, content_end 3, separator_end 5, CrLf}`; for `"abc\rdef"` there
/// is one record whose content includes the CR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LineRecord {
    start_byte: usize,
    content_end_byte: usize,
    separator_end_byte: usize,
    separator_kind: SeparatorKind,
}

impl LineRecord {
    /// Builds a record, validating the ordering law.
    ///
    /// Returns [`SourceLineError::InvalidRecord`] when
    /// `start > content_end`, `content_end > separator_end`, or the declared
    /// separator kind does not match the boundary distances.
    pub fn new(
        start_byte: usize,
        content_end_byte: usize,
        separator_end_byte: usize,
        separator_kind: SeparatorKind,
    ) -> Result<Self, SourceLineError> {
        if !(start_byte <= content_end_byte && content_end_byte <= separator_end_byte) {
            return Err(SourceLineError::InvalidRecord {
                start_byte,
                content_end_byte,
                separator_end_byte,
            });
        }
        // Ordering above guarantees `separator_end >= content_end`, so the
        // subtraction cannot underflow.
        let actual_len = separator_end_byte - content_end_byte;
        let expected_len = separator_kind.byte_len();
        if actual_len != expected_len {
            return Err(SourceLineError::SeparatorKindMismatch {
                start_byte,
                separator_kind,
                separator_byte_len: actual_len,
            });
        }
        Ok(Self { start_byte, content_end_byte, separator_end_byte, separator_kind })
    }

    /// First byte offset of the row's content (and of the row itself).
    #[must_use]
    pub const fn start_byte(&self) -> usize {
        self.start_byte
    }

    /// End offset (exclusive) of the row's content; separator bytes excluded.
    #[must_use]
    pub const fn content_end_byte(&self) -> usize {
        self.content_end_byte
    }

    /// End offset (exclusive) of the whole row including its separator.
    #[must_use]
    pub const fn separator_end_byte(&self) -> usize {
        self.separator_end_byte
    }

    /// How the row is terminated.
    #[must_use]
    pub const fn separator_kind(&self) -> SeparatorKind {
        self.separator_kind
    }

    /// The row's content as a borrowed slice of the exact source bytes.
    ///
    /// Returns `None` when the record's byte range does not lie entirely
    /// inside `source` (for example a different, shorter snapshot). Range
    /// containment cannot prove byte identity; callers must pass the exact
    /// snapshot this table was scanned from.
    #[must_use]
    pub fn content<'a>(&self, source: &'a [u8]) -> Option<&'a [u8]> {
        source.get(self.start_byte..self.content_end_byte)
    }

    /// The row's content as a borrowed string slice of the exact source.
    ///
    /// See [`Self::content`] for the out-of-range behavior.
    #[must_use]
    pub fn content_str<'a>(&self, source: &'a str) -> Option<&'a str> {
        source.get(self.start_byte..self.content_end_byte)
    }
}

impl fmt::Display for LineRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}, {}) sep {:?} ending at {}",
            self.start_byte, self.content_end_byte, self.separator_kind, self.separator_end_byte
        )
    }
}

/// Failure modes of source-line scanning and table construction.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SourceLineError {
    /// The complete source subject is not valid UTF-8.
    ///
    /// Validity is an owning-source precondition; invalid bytes are never
    /// reinterpreted as text.
    #[error("source bytes are not valid UTF-8")]
    InvalidUtf8,
    /// Byte-offset arithmetic exceeded representable bounds while scanning.
    #[error("byte-offset arithmetic overflowed while scanning source lines")]
    ArithmeticOverflow,
    /// A record violates `start <= content_end <= separator_end`.
    #[error(
        "invalid line record: start {start_byte} > content_end {content_end_byte} or content_end {content_end_byte} > separator_end {separator_end_byte}"
    )]
    InvalidRecord {
        /// Declared first byte of the record.
        start_byte: usize,
        /// Declared content end of the record.
        content_end_byte: usize,
        /// Declared full-record end of the record.
        separator_end_byte: usize,
    },
    /// A record's declared separator kind disagrees with its boundary widths.
    #[error(
        "separator kind {separator_kind:?} does not match the {separator_byte_len}-byte gap between content_end and separator_end at record starting at {start_byte}"
    )]
    SeparatorKindMismatch {
        /// First byte of the offending record.
        start_byte: usize,
        /// Declared separator kind.
        separator_kind: SeparatorKind,
        /// Actual gap between content end and separator end.
        separator_byte_len: usize,
    },
    /// Records do not exactly cover the source (gap, overlap, or wrong origin).
    #[error(
        "line records do not exactly cover the source: record {index} starts at {found_start}, expected {expected_start}"
    )]
    NonCoveringRecords {
        /// Zero-based position of the first offending record.
        index: usize,
        /// Start offset the record was required to have.
        expected_start: usize,
        /// Start offset the record actually had.
        found_start: usize,
    },
    /// Only an LF or CRLF may end a nonterminal record.
    #[error("record {index} has no separator but is not the terminal row")]
    NonTerminalRowWithNoSeparator {
        /// Zero-based position of the offending record.
        index: usize,
    },
    /// The last record must be a separator-free terminal row; a final LF
    /// therefore always implies exactly one trailing empty row.
    #[error("records must end with one terminal SeparatorKind::None row at the source length")]
    MissingTerminalRow,
}

/// Immutable byte-only table of logical source rows for one exact source.
///
/// Built once from the exact source bytes — contiguous or chunked — and then
/// only read. Lookups are `O(log n)` via binary search over ordered records;
/// indexed access is `O(1)`. The table never rewrites, normalizes, or copies
/// the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRecordTable {
    source_byte_length: usize,
    records: Vec<LineRecord>,
}

impl std::str::FromStr for LineRecordTable {
    type Err = SourceLineError;

    /// Scans one contiguous valid-UTF-8 source subject.
    ///
    /// ```
    /// use perl_position_tracking::LineRecordTable;
    ///
    /// let table: LineRecordTable = "a\nb".parse()?;
    /// assert_eq!(table.line_count(), 2);
    /// # Ok::<(), perl_position_tracking::SourceLineError>(())
    /// ```
    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let mut scanner = Scanner::default();
        scanner.push_bytes(source.as_bytes())?;
        scanner.finish()
    }
}

impl LineRecordTable {
    /// Scans a sequence of borrowed byte chunks of one valid-UTF-8 source.
    ///
    /// The result equals the contiguous parse ([`std::str::FromStr`]) on the
    /// same complete source for every chunk partition, including empty chunks,
    /// single-byte chunks, splits between the CR and LF of a CRLF pair, and
    /// splits inside multi-byte scalars. The complete source must be valid
    /// UTF-8; validity is checked while streaming, so no concatenation or copy
    /// occurs.
    pub fn from_chunks_utf8<'a, I>(chunks: I) -> Result<Self, SourceLineError>
    where
        I: IntoIterator<Item = &'a [u8]>,
    {
        let mut scanner = Scanner::default();
        for chunk in chunks {
            scanner.push_utf8_chunk(chunk)?;
        }
        if !scanner.utf8.is_complete() {
            return Err(SourceLineError::InvalidUtf8);
        }
        scanner.finish()
    }

    /// Assembles a table from caller-supplied records, enforcing coverage.
    ///
    /// This exists so an independent authority (the raw-byte fixture pack,
    /// #8172) can state expected tables without going through the scanner.
    /// Every coverage law is enforced:
    ///
    /// - the first record starts at byte zero;
    /// - each next record starts exactly at the previous separator end;
    /// - the last record ends exactly at `source_byte_length`;
    /// - only an LF or CRLF ends a nonterminal record;
    /// - the last record is a separator-free terminal row, so an empty
    ///   source carries exactly one `(0, 0, 0, None)` record.
    pub fn try_from_records(
        source_byte_length: usize,
        records: Vec<LineRecord>,
    ) -> Result<Self, SourceLineError> {
        let mut expected_start = 0usize;
        for (index, record) in records.iter().enumerate() {
            if record.start_byte != expected_start {
                return Err(SourceLineError::NonCoveringRecords {
                    index,
                    expected_start,
                    found_start: record.start_byte,
                });
            }
            expected_start = record.separator_end_byte;
        }
        if expected_start != source_byte_length {
            return Err(SourceLineError::NonCoveringRecords {
                index: records.len(),
                expected_start,
                found_start: source_byte_length,
            });
        }
        // Even the empty source carries exactly one terminal row.
        let Some(_first) = records.first() else {
            return Err(SourceLineError::MissingTerminalRow);
        };
        for (index, record) in records.iter().enumerate() {
            let is_last = index + 1 == records.len();
            match (is_last, record.separator_kind) {
                (true, SeparatorKind::None) => {}
                (true, _) => return Err(SourceLineError::MissingTerminalRow),
                (false, SeparatorKind::None) => {
                    return Err(SourceLineError::NonTerminalRowWithNoSeparator { index });
                }
                (false, _) => {}
            }
        }
        Ok(Self { source_byte_length, records })
    }

    /// Identity of the source-line policy this table was built under.
    #[must_use]
    pub const fn policy_id(&self) -> &'static str {
        SOURCE_LINE_POLICY_ID
    }

    /// Exact byte length of the source subject these records cover.
    #[must_use]
    pub const fn source_byte_length(&self) -> usize {
        self.source_byte_length
    }

    /// Number of source rows. Empty source yields exactly one row.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.records.len()
    }

    /// All records in source order.
    #[must_use]
    pub fn records(&self) -> &[LineRecord] {
        &self.records
    }

    /// Borrowed record for one zero-based row, or `None` out of range.
    ///
    /// Out-of-range requests fail honestly; nothing clamps or wraps.
    #[must_use]
    pub fn record(&self, line: usize) -> Option<&LineRecord> {
        self.records.get(line)
    }

    /// Row containing the given byte offset, located by binary search.
    ///
    /// `byte_offset == source_byte_length` addresses the final row (which is
    /// empty after a final LF). Offsets beyond the source return `None`.
    #[must_use]
    pub fn line_index_at_byte(&self, byte_offset: usize) -> Option<usize> {
        if byte_offset > self.source_byte_length {
            return None;
        }
        let line = self.records.partition_point(|r| r.start_byte <= byte_offset);
        Some(line.saturating_sub(1))
    }

    /// Record containing the given byte offset; see [`Self::line_index_at_byte`].
    #[must_use]
    pub fn line_record_at_byte(&self, byte_offset: usize) -> Option<&LineRecord> {
        let line = self.line_index_at_byte(byte_offset)?;
        self.records.get(line)
    }

    /// Content span of one row as a [`ByteSpan`] over the source bytes.
    ///
    /// Returns `None` for an out-of-range row.
    #[must_use]
    pub fn content_span(&self, line: usize) -> Option<ByteSpan> {
        let record = self.record(line)?;
        Some(ByteSpan::new(record.start_byte, record.content_end_byte))
    }
}

/// Streaming scan state: one pending undecided CR plus current record bounds.
#[derive(Default)]
struct Scanner {
    pending_cr_at: Option<usize>,
    record_start: usize,
    offset: usize,
    records: Vec<LineRecord>,
    utf8: Utf8Validator,
}

const LF: u8 = b'\n';
const CR: u8 = b'\r';

impl Scanner {
    /// Consumes one contiguous source subject without re-validating UTF-8.
    fn push_bytes(&mut self, chunk: &[u8]) -> Result<(), SourceLineError> {
        for &byte in chunk {
            self.push_byte(byte)?;
        }
        Ok(())
    }

    /// Consumes one borrowed chunk through the streaming UTF-8 validator.
    fn push_utf8_chunk(&mut self, chunk: &[u8]) -> Result<(), SourceLineError> {
        for &byte in chunk {
            if !self.utf8.advance(byte) {
                return Err(SourceLineError::InvalidUtf8);
            }
            self.push_byte(byte)?;
        }
        Ok(())
    }

    fn push_byte(&mut self, byte: u8) -> Result<(), SourceLineError> {
        if let Some(cr_at) = self.pending_cr_at.take()
            && byte == LF
        {
            // The earlier CR belongs to this LF: one two-byte separator.
            // `cr_at < offset` and `separator_end == offset + 1` hold by
            // construction, so `new` cannot reject here; an impossible
            // rejection maps onto the overflow variant rather than panicking.
            let separator_end =
                self.offset.checked_add(1).ok_or(SourceLineError::ArithmeticOverflow)?;
            // Per-site expectation: if this map_err site is ever removed, this
            // exact expectation becomes unfulfilled and strict Clippy fails,
            // keeping each exception individually ratcheted.
            #[expect(
                clippy::map_err_ignore,
                reason = "CRLF site: cr_at < offset holds by construction (cr_at was recorded \
                          before offset advanced past it), so LineRecord::new cannot reject; \
                          the mapped ArithmeticOverflow class is the complete diagnostic — \
                          LineRecordError carries no payload beyond the violated invariant."
            )]
            let crlf_record =
                LineRecord::new(self.record_start, cr_at, separator_end, SeparatorKind::CrLf)
                    .map_err(|_| SourceLineError::ArithmeticOverflow)?;
            self.records.push(crlf_record);
            self.record_start = separator_end;
            self.offset = separator_end;
            return Ok(());
        }
        // A CR still pending here (followed by a non-LF byte) is bare content
        // inside this row; the current byte is then classified on its own.
        match byte {
            LF => {
                let separator_end =
                    self.offset.checked_add(1).ok_or(SourceLineError::ArithmeticOverflow)?;
                // Per-site expectation: if this map_err site is ever removed,
                // this exact expectation becomes unfulfilled and strict Clippy
                // fails, keeping each exception individually ratcheted.
                #[expect(
                    clippy::map_err_ignore,
                    reason = "LF site: record_start <= offset < separator_end hold by \
                              construction (offset is the pending separator and separator_end \
                              == offset + 1), so LineRecord::new cannot reject; the mapped \
                              ArithmeticOverflow class is the complete diagnostic."
                )]
                let lf_record = LineRecord::new(
                    self.record_start,
                    self.offset,
                    separator_end,
                    SeparatorKind::Lf,
                )
                .map_err(|_| SourceLineError::ArithmeticOverflow)?;
                self.records.push(lf_record);
                self.record_start = separator_end;
                self.offset = separator_end;
            }
            CR => {
                self.pending_cr_at = Some(self.offset);
                self.offset =
                    self.offset.checked_add(1).ok_or(SourceLineError::ArithmeticOverflow)?;
            }
            _ => {
                self.offset =
                    self.offset.checked_add(1).ok_or(SourceLineError::ArithmeticOverflow)?;
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Result<LineRecordTable, SourceLineError> {
        // A CR still pending at EOF is bare content in the terminal row.
        let terminal_content_end = self.offset;
        let record = LineRecord::new(
            self.record_start,
            terminal_content_end,
            terminal_content_end,
            SeparatorKind::None,
        )?;
        self.records.push(record);
        LineRecordTable::try_from_records(terminal_content_end, self.records)
    }
}

/// Incremental UTF-8 validator equivalent to `std::str::from_utf8` semantics.
///
/// Tracks the outstanding continuation count and the admissible range of the
/// *next* continuation byte so overlong forms, UTF-16 surrogates, and values
/// above U+10FFFF are rejected exactly like `std`, while scalars may be split
/// across arbitrary chunk boundaries.
///
/// The validator tracks no accumulated code point: the per-position range
/// constraints on lead and continuation bytes are exactly equivalent to
/// checking the decoded scalar, so no numeric value is ever needed.
#[derive(Default)]
struct Utf8Validator {
    remaining: u8,
    next_min: u8,
    next_max: u8,
}

impl Utf8Validator {
    /// Feeds one byte, returning `false` when the stream stops being UTF-8.
    fn advance(&mut self, byte: u8) -> bool {
        if self.remaining == 0 {
            match byte {
                0x00..=0x7F => true,
                0xC2..=0xDF => self.begin(1, 0x80, 0xBF),
                0xE0 => self.begin(2, 0xA0, 0xBF),
                0xE1..=0xEC | 0xEE..=0xEF => self.begin(2, 0x80, 0xBF),
                0xED => self.begin(2, 0x80, 0x9F),
                0xF0 => self.begin(3, 0x90, 0xBF),
                0xF1..=0xF3 => self.begin(3, 0x80, 0xBF),
                0xF4 => self.begin(3, 0x80, 0x8F),
                _ => false,
            }
        } else if (0x80..=0xBF).contains(&byte) && (self.next_min..=self.next_max).contains(&byte) {
            // Later continuations accept the full 0x80..=0xBF range; only the
            // first one after these leads is constrained.
            self.next_min = 0x80;
            self.next_max = 0xBF;
            self.remaining -= 1;
            true
        } else {
            false
        }
    }

    /// Confirms the stream ended on a scalar boundary.
    fn is_complete(&self) -> bool {
        self.remaining == 0
    }

    fn begin(&mut self, remaining: u8, min: u8, max: u8) -> bool {
        self.remaining = remaining;
        self.next_min = min;
        self.next_max = max;
        true
    }
}
