//! Independent old-generation edit transaction model for source-equivalence proof.
//!
//! This module answers one question for differential incremental-parsing proof:
//! *given an immutable predecessor source and an ordered set of edits expressed
//! in that predecessor's coordinates, what are the exact final bytes?*
//!
//! It is deliberately **not** built on the production incremental edit path.
//! Reusing production edit application as the oracle would make source
//! equivalence self-referential: a defect in the applicator would be reproduced
//! identically on both sides of the comparison and cancel out. The reference
//! model therefore re-derives the answer from the predecessor bytes alone, and
//! `tests/reference_edit_independence.rs` gates that independence so a later
//! change cannot quietly delegate to the production applicator.
//!
//! # Coordinate model
//!
//! One explicit model is supported, named
//! [`REFERENCE_EDIT_COORDINATE_MODEL_ID`]:
//!
//! - every edit range addresses the same immutable predecessor source;
//! - ranges are half-open `[start_byte, old_end_byte)`;
//! - both endpoints must lie on UTF-8 scalar boundaries;
//! - an accepted transaction is already in ascending predecessor order;
//! - overlapping ranges are rejected;
//! - two edits sharing a start byte are rejected as ambiguous;
//! - a malformed transaction is **never** silently sorted into a valid one.
//!
//! Rejection is atomic. [`ReferenceSourceState::apply`] borrows the predecessor
//! immutably and returns a new state on success, so a rejected transaction
//! cannot leave partially applied bytes, a moved generation, or a stale digest
//! behind.
//!
//! # Line geometry
//!
//! Final line geometry is reported as a
//! [`LineRecordTable`], the canonical
//! LF-delimited source-line authority accepted in ADR-0048
//! (`lf-source-lines/v1`): LF terminates a row, CRLF is one separator whose LF
//! terminates the row, and bare CR, VT, FF, NEL, LS, PS, and a non-leading BOM
//! are ordinary content. The table is always built from the **final** bytes;
//! deriving it from the predecessor is one of the mutations the proof matrix
//! rejects.
//!
//! # Example
//!
//! ```
//! use perl_tdd_support::reference_edit::{
//!     ReferenceEdit, ReferenceEditTransaction, ReferenceSourceState,
//! };
//!
//! let state = ReferenceSourceState::new("aaa bbb ccc")?;
//! let result = state.apply(&ReferenceEditTransaction::new(vec![
//!     ReferenceEdit::replace(0, 3, "AAAA"),
//!     ReferenceEdit::replace(8, 11, "C"),
//! ]))?;
//!
//! assert_eq!(result.source(), "AAAA bbb C");
//! assert_eq!(result.generation(), 1);
//! # Ok::<(), perl_tdd_support::reference_edit::ReferenceEditError>(())
//! ```

use perl_position_tracking::{ByteSpan, LineRecordTable, SourceLineError};
use perl_source_identity::ContentDigest;
use thiserror::Error;

/// The one coordinate model this reference implementation accepts.
///
/// A transaction declaring any other model is rejected with
/// [`ReferenceEditError::UnsupportedCoordinateModel`] rather than being
/// reinterpreted under these rules.
pub const REFERENCE_EDIT_COORDINATE_MODEL_ID: &str = "old-generation-utf8-bytes/v1";

/// One edit addressed in immutable predecessor-source coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceEdit {
    start_byte: usize,
    old_end_byte: usize,
    replacement: String,
    expected_new_end_byte: Option<usize>,
}

impl ReferenceEdit {
    /// Replaces the predecessor bytes in `[start_byte, old_end_byte)`.
    #[must_use]
    pub fn replace(start_byte: usize, old_end_byte: usize, replacement: &str) -> Self {
        Self {
            start_byte,
            old_end_byte,
            replacement: replacement.to_owned(),
            expected_new_end_byte: None,
        }
    }

    /// Inserts `text` before predecessor byte `at_byte`, replacing nothing.
    #[must_use]
    pub fn insert(at_byte: usize, text: &str) -> Self {
        Self::replace(at_byte, at_byte, text)
    }

    /// Deletes the predecessor bytes in `[start_byte, old_end_byte)`.
    #[must_use]
    pub fn delete(start_byte: usize, old_end_byte: usize) -> Self {
        Self::replace(start_byte, old_end_byte, "")
    }

    /// Records the successor-coordinate end this replacement must produce.
    ///
    /// The declared value is checked against the independently computed end and
    /// a disagreement is reported as
    /// [`ReferenceEditError::NewEndMismatch`]. This lets a caller state the
    /// resulting local end as an expectation instead of trusting the model's
    /// own arithmetic.
    #[must_use]
    pub fn with_expected_new_end(mut self, expected_new_end_byte: usize) -> Self {
        self.expected_new_end_byte = Some(expected_new_end_byte);
        self
    }

    /// First predecessor byte this edit addresses.
    #[must_use]
    pub const fn start_byte(&self) -> usize {
        self.start_byte
    }

    /// Predecessor byte one past the last this edit addresses.
    #[must_use]
    pub const fn old_end_byte(&self) -> usize {
        self.old_end_byte
    }

    /// Text substituted for the addressed predecessor bytes.
    #[must_use]
    pub fn replacement(&self) -> &str {
        &self.replacement
    }

    /// Declared successor-coordinate end, when the caller recorded one.
    #[must_use]
    pub const fn expected_new_end_byte(&self) -> Option<usize> {
        self.expected_new_end_byte
    }

    // Deliberately no `old_span()` accessor. The constructors admit a reversed
    // range so `apply` can reject it as `ReversedRange`, but `ByteSpan::new`
    // debug-asserts `start <= end`, so handing an unvalidated edit to it would
    // panic while merely inspecting malformed input. `start_byte` and
    // `old_end_byte` describe the range without that hazard, and a validated
    // span is available from `ReferenceEditResult::changed_old` once the
    // transaction is accepted.
}

/// An ordered set of edits applied as one atomic old-generation transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceEditTransaction {
    edits: Vec<ReferenceEdit>,
    coordinate_model: String,
}

impl ReferenceEditTransaction {
    /// Builds a transaction in the supported old-generation byte model.
    ///
    /// The edits are stored exactly as supplied. Canonical ordering is a
    /// *validation rule*, not a normalization step: a caller-supplied order
    /// that is not already ascending is rejected, never repaired.
    #[must_use]
    pub fn new(edits: Vec<ReferenceEdit>) -> Self {
        Self { edits, coordinate_model: REFERENCE_EDIT_COORDINATE_MODEL_ID.to_owned() }
    }

    /// Declares a coordinate model explicitly.
    ///
    /// Any value other than [`REFERENCE_EDIT_COORDINATE_MODEL_ID`] is rejected
    /// on application. This keeps a future successor-coordinate or
    /// character-indexed transaction from being silently evaluated under the
    /// old-generation byte rules.
    #[must_use]
    pub fn with_coordinate_model(mut self, coordinate_model: &str) -> Self {
        self.coordinate_model = coordinate_model.to_owned();
        self
    }

    /// The edits in the order the caller supplied them.
    #[must_use]
    pub fn edits(&self) -> &[ReferenceEdit] {
        &self.edits
    }

    /// The declared coordinate model.
    #[must_use]
    pub fn coordinate_model(&self) -> &str {
        &self.coordinate_model
    }
}

/// One piece of the total old-to-new byte mapping.
///
/// The segments of a [`ReferenceEditResult`] partition the predecessor and the
/// successor completely and in ascending order, and each carries both spans, so
/// a consumer can derive a translation in either direction without re-deriving
/// the edits.
///
/// Only the old-to-new direction has a provided helper
/// ([`ReferenceEditResult::map_old_to_new`]); that is the direction #7344
/// requires. A new-to-old translation is a scan of these same segments, and is
/// left to the consumer that needs it rather than added speculatively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceByteMapSegment {
    /// Bytes carried through unchanged, possibly at a shifted offset.
    Unchanged {
        /// Predecessor span.
        old: ByteSpan,
        /// Successor span; the same length as `old`.
        new: ByteSpan,
    },
    /// Bytes an edit replaced. The two spans need not have the same length.
    Replaced {
        /// Predecessor span the edit addressed.
        old: ByteSpan,
        /// Successor span the replacement text occupies.
        new: ByteSpan,
    },
}

impl ReferenceByteMapSegment {
    /// The predecessor span of this segment.
    #[must_use]
    pub const fn old(&self) -> ByteSpan {
        match *self {
            Self::Unchanged { old, .. } | Self::Replaced { old, .. } => old,
        }
    }

    /// The successor span of this segment.
    #[must_use]
    pub const fn new_span(&self) -> ByteSpan {
        match *self {
            Self::Unchanged { new, .. } | Self::Replaced { new, .. } => new,
        }
    }

    /// Whether an edit rewrote this segment.
    #[must_use]
    pub const fn is_replaced(&self) -> bool {
        matches!(*self, Self::Replaced { .. })
    }
}

/// Why a transaction was rejected.
///
/// Every variant reports a stable snake_case [`ReferenceEditError::reason`]
/// so a proof can assert the exact rejection class without matching on
/// variant shape.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ReferenceEditError {
    /// An edit range ends beyond the predecessor source.
    #[error(
        "edit {index} addresses [{start_byte}, {old_end_byte}) beyond the {source_len}-byte predecessor"
    )]
    OutOfBounds {
        /// Position of the offending edit in the supplied order.
        index: usize,
        /// Declared range start.
        start_byte: usize,
        /// Declared range end.
        old_end_byte: usize,
        /// Length of the predecessor source in bytes.
        source_len: usize,
    },
    /// An edit endpoint falls inside a multi-byte UTF-8 scalar.
    #[error("edit {index} endpoint {byte_offset} splits a UTF-8 scalar in the predecessor")]
    SplitUtf8Scalar {
        /// Position of the offending edit in the supplied order.
        index: usize,
        /// The endpoint that is not on a scalar boundary.
        byte_offset: usize,
    },
    /// An edit range ends before it starts.
    #[error("edit {index} range [{start_byte}, {old_end_byte}) is reversed")]
    ReversedRange {
        /// Position of the offending edit in the supplied order.
        index: usize,
        /// Declared range start.
        start_byte: usize,
        /// Declared range end.
        old_end_byte: usize,
    },
    /// The supplied edits are not in ascending predecessor order.
    ///
    /// The transaction is rejected rather than sorted, so a caller cannot get a
    /// silently repaired result from a transaction it described incorrectly.
    #[error(
        "edit {later_index} starts at {later_start} before edit {earlier_index} at {earlier_start}; \
         a noncanonical transaction is rejected, not sorted"
    )]
    NoncanonicalOrder {
        /// Index of the earlier-supplied edit.
        earlier_index: usize,
        /// Index of the later-supplied edit.
        later_index: usize,
        /// Start byte of the earlier-supplied edit.
        earlier_start: usize,
        /// Start byte of the later-supplied edit.
        later_start: usize,
    },
    /// Two edits address overlapping predecessor ranges.
    #[error(
        "edit {earlier_index} ends at {earlier_old_end} after edit {later_index} starts at {later_start}"
    )]
    Overlap {
        /// Index of the earlier-supplied edit.
        earlier_index: usize,
        /// Index of the later-supplied edit.
        later_index: usize,
        /// End byte of the earlier-supplied edit.
        earlier_old_end: usize,
        /// Start byte of the later-supplied edit.
        later_start: usize,
    },
    /// Two edits share a start byte, so their relative effect is ambiguous.
    #[error("edits {earlier_index} and {later_index} share start byte {start_byte}")]
    DuplicateStart {
        /// Index of the earlier-supplied edit.
        earlier_index: usize,
        /// Index of the later-supplied edit.
        later_index: usize,
        /// The shared start byte.
        start_byte: usize,
    },
    /// A recorded successor end disagrees with the computed one.
    #[error(
        "edit {index} declared successor end {expected_new_end_byte} but produced {actual_new_end_byte}"
    )]
    NewEndMismatch {
        /// Position of the offending edit in the supplied order.
        index: usize,
        /// The end the caller recorded.
        expected_new_end_byte: usize,
        /// The end the model computed.
        actual_new_end_byte: usize,
    },
    /// The transaction declared a coordinate model this model does not own.
    #[error("coordinate model {requested:?} is not {REFERENCE_EDIT_COORDINATE_MODEL_ID:?}")]
    UnsupportedCoordinateModel {
        /// The model the transaction declared.
        requested: String,
    },
    /// Source line geometry could not be derived.
    ///
    /// Reachable only through [`ReferenceSourceState::new`], whose input is
    /// arbitrary caller-supplied text. Successor bytes are assembled from
    /// boundary-checked `&str` slices and replacement `&str`s, so they are
    /// valid UTF-8 by construction; this variant keeps that step fail-closed
    /// instead of asserting the invariant with a panic.
    #[error("source line geometry is invalid: {0}")]
    InvalidSourceGeometry(#[from] SourceLineError),
    /// A generation counter exceeded its representable range.
    #[error("source generation {generation} cannot be advanced")]
    GenerationOverflow {
        /// The generation that could not be advanced.
        generation: u64,
    },
}

impl ReferenceEditError {
    /// Stable snake_case identifier for this rejection class.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match *self {
            Self::OutOfBounds { .. } => "out_of_bounds",
            Self::SplitUtf8Scalar { .. } => "split_utf8_scalar",
            Self::ReversedRange { .. } => "reversed_range",
            Self::NoncanonicalOrder { .. } => "noncanonical_order",
            Self::Overlap { .. } => "overlap",
            Self::DuplicateStart { .. } => "duplicate_start",
            Self::NewEndMismatch { .. } => "new_end_mismatch",
            Self::UnsupportedCoordinateModel { .. } => "unsupported_coordinate_model",
            Self::InvalidSourceGeometry(_) => "invalid_source_geometry",
            Self::GenerationOverflow { .. } => "generation_overflow",
        }
    }
}

/// An immutable source subject: exact bytes plus derived identity and geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceSourceState {
    source: String,
    digest: ContentDigest,
    generation: u64,
    lines: LineRecordTable,
}

impl ReferenceSourceState {
    /// Builds generation `0` from exact source bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ReferenceEditError::InvalidSourceGeometry`] if line geometry
    /// cannot be derived from `source`.
    pub fn new(source: &str) -> Result<Self, ReferenceEditError> {
        Self::with_generation(source, 0)
    }

    /// Builds a state at an explicit generation.
    ///
    /// # Errors
    ///
    /// Returns [`ReferenceEditError::InvalidSourceGeometry`] if line geometry
    /// cannot be derived from `source`.
    pub fn with_generation(source: &str, generation: u64) -> Result<Self, ReferenceEditError> {
        Ok(Self {
            digest: ContentDigest::of_bytes(source.as_bytes()),
            lines: source.parse::<LineRecordTable>()?,
            source: source.to_owned(),
            generation,
        })
    }

    /// The exact source bytes of this subject.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Content digest of the exact bytes, under the canonical identity rules.
    #[must_use]
    pub const fn digest(&self) -> &ContentDigest {
        &self.digest
    }

    /// This subject's generation. Each accepted transaction advances it by one.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Line geometry of the exact bytes under `lf-source-lines/v1`.
    #[must_use]
    pub const fn lines(&self) -> &LineRecordTable {
        &self.lines
    }

    /// Applies one ordered old-generation transaction.
    ///
    /// On success the predecessor is left untouched and a successor state is
    /// returned alongside the mapping and changed ranges. On rejection nothing
    /// is produced, so predecessor bytes, digest, generation, and line geometry
    /// are unchanged by construction.
    ///
    /// # Errors
    ///
    /// Returns the [`ReferenceEditError`] naming the first violated rule. Rules
    /// are checked in a fixed order: per-edit shape (reversed, out of bounds,
    /// scalar boundaries), then pairwise ordering (duplicate start,
    /// noncanonical order, overlap), then recorded successor ends.
    pub fn apply(
        &self,
        transaction: &ReferenceEditTransaction,
    ) -> Result<ReferenceEditResult, ReferenceEditError> {
        if transaction.coordinate_model != REFERENCE_EDIT_COORDINATE_MODEL_ID {
            return Err(ReferenceEditError::UnsupportedCoordinateModel {
                requested: transaction.coordinate_model.clone(),
            });
        }

        let edits = transaction.edits();
        let source_len = self.source.len();

        for (index, edit) in edits.iter().enumerate() {
            if edit.start_byte > edit.old_end_byte {
                return Err(ReferenceEditError::ReversedRange {
                    index,
                    start_byte: edit.start_byte,
                    old_end_byte: edit.old_end_byte,
                });
            }
            if edit.old_end_byte > source_len {
                return Err(ReferenceEditError::OutOfBounds {
                    index,
                    start_byte: edit.start_byte,
                    old_end_byte: edit.old_end_byte,
                    source_len,
                });
            }
            for byte_offset in [edit.start_byte, edit.old_end_byte] {
                if !self.source.is_char_boundary(byte_offset) {
                    return Err(ReferenceEditError::SplitUtf8Scalar { index, byte_offset });
                }
            }
        }

        // Adjacent pairs are sufficient. Rejecting equal and descending starts
        // makes an accepted transaction strictly ascending, so for any i < j,
        // `edits[i].old_end <= edits[i + 1].start <= edits[j].start`. A
        // non-adjacent overlap or duplicate would therefore have to violate an
        // adjacent pair first.
        for later_index in 1..edits.len() {
            let earlier_index = later_index - 1;
            let earlier = &edits[earlier_index];
            let later = &edits[later_index];

            if later.start_byte == earlier.start_byte {
                return Err(ReferenceEditError::DuplicateStart {
                    earlier_index,
                    later_index,
                    start_byte: later.start_byte,
                });
            }
            if later.start_byte < earlier.start_byte {
                return Err(ReferenceEditError::NoncanonicalOrder {
                    earlier_index,
                    later_index,
                    earlier_start: earlier.start_byte,
                    later_start: later.start_byte,
                });
            }
            if earlier.old_end_byte > later.start_byte {
                return Err(ReferenceEditError::Overlap {
                    earlier_index,
                    later_index,
                    earlier_old_end: earlier.old_end_byte,
                    later_start: later.start_byte,
                });
            }
        }

        let generation = self
            .generation
            .checked_add(1)
            .ok_or(ReferenceEditError::GenerationOverflow { generation: self.generation })?;

        let mut successor = String::new();
        let mut mapping = Vec::new();
        let mut changed_old = Vec::new();
        let mut changed_new = Vec::new();
        let mut cursor = 0usize;

        for (index, edit) in edits.iter().enumerate() {
            if cursor < edit.start_byte {
                let carried = &self.source[cursor..edit.start_byte];
                let new_start = successor.len();
                successor.push_str(carried);
                mapping.push(ReferenceByteMapSegment::Unchanged {
                    old: ByteSpan::new(cursor, edit.start_byte),
                    new: ByteSpan::new(new_start, successor.len()),
                });
            }

            let new_start = successor.len();
            successor.push_str(&edit.replacement);
            let new_end = successor.len();

            if let Some(expected_new_end_byte) = edit.expected_new_end_byte
                && expected_new_end_byte != new_end
            {
                return Err(ReferenceEditError::NewEndMismatch {
                    index,
                    expected_new_end_byte,
                    actual_new_end_byte: new_end,
                });
            }

            let old = ByteSpan::new(edit.start_byte, edit.old_end_byte);
            let new = ByteSpan::new(new_start, new_end);
            mapping.push(ReferenceByteMapSegment::Replaced { old, new });
            changed_old.push(old);
            changed_new.push(new);
            cursor = edit.old_end_byte;
        }

        if cursor < source_len {
            let carried = &self.source[cursor..];
            let new_start = successor.len();
            successor.push_str(carried);
            mapping.push(ReferenceByteMapSegment::Unchanged {
                old: ByteSpan::new(cursor, source_len),
                new: ByteSpan::new(new_start, successor.len()),
            });
        }

        Ok(ReferenceEditResult {
            predecessor_digest: self.digest.clone(),
            predecessor_generation: self.generation,
            transaction: transaction.clone(),
            successor: ReferenceSourceState::with_generation(&successor, generation)?,
            mapping,
            changed_old,
            changed_new,
        })
    }
}

/// The accepted outcome of one old-generation transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceEditResult {
    predecessor_digest: ContentDigest,
    predecessor_generation: u64,
    transaction: ReferenceEditTransaction,
    successor: ReferenceSourceState,
    mapping: Vec<ReferenceByteMapSegment>,
    changed_old: Vec<ByteSpan>,
    changed_new: Vec<ByteSpan>,
}

impl ReferenceEditResult {
    /// Digest of the predecessor this transaction was addressed against.
    #[must_use]
    pub const fn predecessor_digest(&self) -> &ContentDigest {
        &self.predecessor_digest
    }

    /// Generation of the predecessor this transaction was addressed against.
    #[must_use]
    pub const fn predecessor_generation(&self) -> u64 {
        self.predecessor_generation
    }

    /// The accepted transaction, in canonical ascending predecessor order.
    #[must_use]
    pub const fn transaction(&self) -> &ReferenceEditTransaction {
        &self.transaction
    }

    /// The successor subject, ready to address a following generation.
    #[must_use]
    pub const fn state(&self) -> &ReferenceSourceState {
        &self.successor
    }

    /// Exact successor bytes.
    #[must_use]
    pub fn source(&self) -> &str {
        self.successor.source()
    }

    /// Digest of the exact successor bytes.
    #[must_use]
    pub const fn digest(&self) -> &ContentDigest {
        self.successor.digest()
    }

    /// Successor generation, one past the predecessor's.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.successor.generation()
    }

    /// Successor line geometry, derived from the successor bytes.
    #[must_use]
    pub const fn lines(&self) -> &LineRecordTable {
        self.successor.lines()
    }

    /// The complete ordered old-to-new byte mapping.
    #[must_use]
    pub fn mapping(&self) -> &[ReferenceByteMapSegment] {
        &self.mapping
    }

    /// Predecessor spans the transaction replaced, in ascending order.
    #[must_use]
    pub fn changed_old(&self) -> &[ByteSpan] {
        &self.changed_old
    }

    /// Successor spans the replacements occupy, in ascending order.
    #[must_use]
    pub fn changed_new(&self) -> &[ByteSpan] {
        &self.changed_new
    }

    /// Length of the predecessor this transaction was addressed against.
    ///
    /// Recovered from the mapping, which tiles the predecessor completely, so
    /// the last segment ends at the predecessor's length. An empty mapping
    /// occurs only for an empty predecessor with no edits, where zero is
    /// correct.
    #[must_use]
    pub fn predecessor_len(&self) -> usize {
        self.mapping.last().map_or(0, |segment| segment.old().end)
    }

    /// Translates a predecessor byte offset into the successor.
    ///
    /// Offsets are positions, not just indices: `0..=predecessor_len` is the
    /// addressable range, matching the edit coordinates this model accepts
    /// (an insertion at `predecessor_len` appends at EOF). The predecessor's
    /// end-of-source position therefore maps to the successor's, which is what
    /// lets a half-open range ending at EOF be translated whole.
    ///
    /// Returns `None` for an offset strictly inside a replaced span, where no
    /// offset-preserving image exists, and for an offset beyond the predecessor.
    ///
    /// One convention is worth naming: when the transaction ends by inserting
    /// at EOF, the predecessor's end position is ambiguous — it could map
    /// before or after the inserted text. This returns the successor's end,
    /// placing it after, which is what an editor caret at end-of-document does
    /// when text is appended.
    #[must_use]
    pub fn map_old_to_new(&self, old_byte: usize) -> Option<usize> {
        if old_byte == self.predecessor_len() {
            return Some(self.successor.source.len());
        }
        self.mapping.iter().find_map(|segment| match *segment {
            ReferenceByteMapSegment::Unchanged { old, new } if old.contains(old_byte) => {
                Some(new.start + (old_byte - old.start))
            }
            _ => None,
        })
    }
}
