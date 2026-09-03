//! Parser-independent exact byte edits and coordinate relations for #13657.
//!
//! This substrate proves transformed bytes and byte-coordinate relationships.
//! It owns no Perl safe-region inference and consumes no parser output.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::str;

use perl_lsp_rs_core::hashing::sha256_hex;
use perl_position_tracking::{LineRecordTable, SOURCE_LINE_POLICY_ID, SourceLineError};
use serde::Serialize;

/// Schema identity for a fully validated transformation receipt.
///
/// Compatibility contract: `transformation_identity` is defined by the exact
/// serialized form of [`TransformationIdentityMaterial`] (serde_json). Any
/// change to that form — field set, names, serde representation, or included
/// values — MUST bump this version, so receipts derived under different
/// contracts are never compared or interpreted as compatible even when the
/// underlying bytes are identical.
pub const TRANSFORMATION_SCHEMA_VERSION: u32 = 1;

/// Schema identity for canonical coordinate-map material.
///
/// Same compatibility contract as [`TRANSFORMATION_SCHEMA_VERSION`]: the
/// coordinate-map identity is defined by the exact serialized form of
/// [`MapIdentityMaterial`]; changes to that form MUST bump this version.
pub const COORDINATE_MAP_SCHEMA_VERSION: u32 = 1;

/// One half-open byte range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ByteRange {
    /// Inclusive start byte.
    pub start: usize,
    /// Exclusive end byte.
    pub end: usize,
}

impl ByteRange {
    /// Construct a byte range. Validation belongs to the consuming subject.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Return the byte width when the range is ordered.
    #[must_use]
    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether this range has zero width.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    fn contains_closed(self, offset: usize) -> bool {
        self.start <= offset && offset <= self.end
    }

    fn contains_range_closed(self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }
}

/// Exact source bytes with a verified algorithm-tagged identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentAddressedSource {
    identity: String,
    bytes: Vec<u8>,
    line_count: usize,
}

impl ContentAddressedSource {
    /// Construct a subject and derive its identity from exact bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, TransformError> {
        let identity = sha256_hex(&bytes);
        Self::from_verified(identity, bytes)
    }

    /// Construct a subject only when the claimed identity matches exact bytes.
    pub fn from_claimed(identity: String, bytes: Vec<u8>) -> Result<Self, TransformError> {
        let observed = sha256_hex(&bytes);
        if identity != observed {
            return Err(TransformError::StaleSourceIdentity { claimed: identity, observed });
        }

        Self::from_verified(identity, bytes)
    }

    /// Assemble a subject whose identity is already verified against bytes,
    /// hashing the source exactly once per construction path.
    fn from_verified(identity: String, bytes: Vec<u8>) -> Result<Self, TransformError> {
        let source = str::from_utf8(&bytes).map_err(TransformError::InvalidSourceUtf8)?;
        let line_table: LineRecordTable =
            source.parse().map_err(TransformError::InvalidSourceGeometry)?;

        Ok(Self { identity, bytes, line_count: line_table.line_count() })
    }

    /// Verified exact source identity.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Exact source bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Logical source-row count under the canonical LF-only policy.
    #[must_use]
    pub const fn line_count(&self) -> usize {
        self.line_count
    }
}

/// One exact edit stated against base-source byte coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExactEdit {
    edit_id: String,
    base_start: usize,
    base_end: usize,
    expected_old: Vec<u8>,
    replacement: Vec<u8>,
}

impl ExactEdit {
    /// Construct an unvalidated edit claim.
    #[must_use]
    pub fn new(
        edit_id: String,
        base_start: usize,
        base_end: usize,
        expected_old: Vec<u8>,
        replacement: Vec<u8>,
    ) -> Self {
        Self { edit_id, base_start, base_end, expected_old, replacement }
    }

    /// Stable edit identity.
    #[must_use]
    pub fn edit_id(&self) -> &str {
        &self.edit_id
    }

    /// Claimed base-source range.
    #[must_use]
    pub const fn base_range(&self) -> ByteRange {
        ByteRange::new(self.base_start, self.base_end)
    }
}

/// Canonical edit after exact-byte and range validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppliedEdit {
    /// Stable edit identity.
    pub edit_id: String,
    /// Removed base-source bytes.
    pub removed_base: ByteRange,
    /// Inserted transformed-source bytes.
    pub inserted_transformed: ByteRange,
    /// Exact bytes required at the base range.
    pub expected_old: Vec<u8>,
    /// Exact bytes inserted into the transformed subject.
    pub replacement: Vec<u8>,
    /// Digest of exact expected old bytes.
    pub expected_old_identity: String,
    /// Digest of exact replacement bytes.
    pub replacement_identity: String,
}

/// One complete segment of the base↔transformed coordinate relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoordinateSegment {
    /// A byte-identical, bijective span.
    Unchanged {
        /// Base-source span.
        base: ByteRange,
        /// Transformed-source span.
        transformed: ByteRange,
    },
    /// One exact edit relation. Empty removed spans are insertions; empty
    /// inserted spans are removals.
    Edit {
        /// Stable edit identity.
        edit_id: String,
        /// Base-source bytes with no interior transformed counterpart.
        removed_base: ByteRange,
        /// Transformed bytes with no interior base counterpart.
        inserted_transformed: ByteRange,
    },
}

/// Result of mapping one byte position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositionRelation {
    /// Coordinate is unchanged.
    Exact {
        /// Mapped offset in the target subject.
        offset: usize,
    },
    /// Coordinate shifts through preceding edits.
    Mapped {
        /// Mapped offset in the target subject.
        offset: usize,
    },
    /// Position lies only inside inserted transformed bytes.
    InsertedOnly {
        /// Queried transformed offset.
        transformed_offset: usize,
        /// Base range replaced by the insertion.
        base: ByteRange,
    },
    /// Position lies only inside removed base bytes.
    RemovedOnly {
        /// Queried base offset.
        base_offset: usize,
        /// Transformed range replacing the removed bytes.
        transformed: ByteRange,
    },
    /// One source boundary has more than one target boundary.
    Ambiguous {
        /// Lower candidate target offset.
        lower: usize,
        /// Upper candidate target offset.
        upper: usize,
    },
    /// Offset lies outside the owning subject.
    Invalid,
}

/// Result of mapping one half-open byte range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeRelation {
    /// Range is unchanged.
    Exact {
        /// Mapped target range.
        range: ByteRange,
    },
    /// Range shifts without crossing an edit relation.
    Mapped {
        /// Mapped target range.
        range: ByteRange,
    },
    /// Range exists only in transformed inserted bytes.
    InsertedOnly {
        /// Queried transformed range.
        transformed: ByteRange,
        /// Base range replaced by the insertion.
        base: ByteRange,
    },
    /// Range exists only in removed base bytes.
    RemovedOnly {
        /// Queried base range.
        base: ByteRange,
        /// Transformed range replacing it.
        transformed: ByteRange,
    },
    /// Range touches or crosses a non-bijective edit relation.
    Ambiguous,
    /// Range is reversed or out of bounds.
    Invalid,
}

/// Total coordinate relation for one validated transformation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoordinateMap {
    base_len: usize,
    transformed_len: usize,
    segments: Vec<CoordinateSegment>,
    identity: String,
}

impl CoordinateMap {
    /// Canonical coordinate-map identity.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Exact base-source byte length covered by this map.
    #[must_use]
    pub const fn base_len(&self) -> usize {
        self.base_len
    }

    /// Exact transformed-source byte length covered by this map.
    #[must_use]
    pub const fn transformed_len(&self) -> usize {
        self.transformed_len
    }

    /// Ordered complete coordinate segments.
    #[must_use]
    pub fn segments(&self) -> &[CoordinateSegment] {
        &self.segments
    }

    /// Map one base-source byte position into transformed coordinates.
    #[must_use]
    pub fn map_base_position(&self, offset: usize) -> PositionRelation {
        if offset > self.base_len {
            return PositionRelation::Invalid;
        }

        if let Some(relation) = edit_boundary_relation(
            offset,
            self.segments.iter().filter_map(|segment| match segment {
                CoordinateSegment::Edit { removed_base, inserted_transformed, .. } => {
                    Some((*removed_base, *inserted_transformed))
                }
                CoordinateSegment::Unchanged { .. } => None,
            }),
        ) {
            return relation;
        }

        for segment in &self.segments {
            if let CoordinateSegment::Edit { removed_base, inserted_transformed, .. } = segment
                && removed_base.start < offset
                && offset < removed_base.end
            {
                return PositionRelation::RemovedOnly {
                    base_offset: offset,
                    transformed: *inserted_transformed,
                };
            }
        }

        for segment in &self.segments {
            if let CoordinateSegment::Unchanged { base, transformed } = segment
                && base.contains_closed(offset)
            {
                let relative = offset.saturating_sub(base.start);
                let Some(mapped) = transformed.start.checked_add(relative) else {
                    return PositionRelation::Invalid;
                };
                return point_relation(offset, mapped);
            }
        }

        PositionRelation::Invalid
    }

    /// Map one transformed-source byte position into base coordinates.
    #[must_use]
    pub fn map_transformed_position(&self, offset: usize) -> PositionRelation {
        if offset > self.transformed_len {
            return PositionRelation::Invalid;
        }

        if let Some(relation) = edit_boundary_relation(
            offset,
            self.segments.iter().filter_map(|segment| match segment {
                CoordinateSegment::Edit { removed_base, inserted_transformed, .. } => {
                    Some((*inserted_transformed, *removed_base))
                }
                CoordinateSegment::Unchanged { .. } => None,
            }),
        ) {
            return relation;
        }

        for segment in &self.segments {
            if let CoordinateSegment::Edit { removed_base, inserted_transformed, .. } = segment
                && inserted_transformed.start < offset
                && offset < inserted_transformed.end
            {
                return PositionRelation::InsertedOnly {
                    transformed_offset: offset,
                    base: *removed_base,
                };
            }
        }

        for segment in &self.segments {
            if let CoordinateSegment::Unchanged { base, transformed } = segment
                && transformed.contains_closed(offset)
            {
                let relative = offset.saturating_sub(transformed.start);
                let Some(mapped) = base.start.checked_add(relative) else {
                    return PositionRelation::Invalid;
                };
                return point_relation(offset, mapped);
            }
        }

        PositionRelation::Invalid
    }

    /// Map one base-source range when its complete interior is bijective.
    #[must_use]
    pub fn map_base_range(&self, range: ByteRange) -> RangeRelation {
        if range.start > range.end || range.end > self.base_len {
            return RangeRelation::Invalid;
        }
        if range.is_empty() {
            return zero_range_relation(self.map_base_position(range.start), range);
        }

        for segment in &self.segments {
            match segment {
                CoordinateSegment::Unchanged { base, transformed }
                    if base.contains_range_closed(range) =>
                {
                    let start_delta = range.start.saturating_sub(base.start);
                    let end_delta = range.end.saturating_sub(base.start);
                    let (Some(start), Some(end)) = (
                        transformed.start.checked_add(start_delta),
                        transformed.start.checked_add(end_delta),
                    ) else {
                        return RangeRelation::Invalid;
                    };
                    let mapped = ByteRange::new(start, end);
                    return range_relation(range, mapped);
                }
                CoordinateSegment::Edit { removed_base, inserted_transformed, .. }
                    if !removed_base.is_empty() && removed_base.contains_range_closed(range) =>
                {
                    return RangeRelation::RemovedOnly {
                        base: range,
                        transformed: *inserted_transformed,
                    };
                }
                _ => {}
            }
        }

        RangeRelation::Ambiguous
    }

    /// Map one transformed-source range when its complete interior is bijective.
    #[must_use]
    pub fn map_transformed_range(&self, range: ByteRange) -> RangeRelation {
        if range.start > range.end || range.end > self.transformed_len {
            return RangeRelation::Invalid;
        }
        if range.is_empty() {
            return zero_range_relation(self.map_transformed_position(range.start), range);
        }

        for segment in &self.segments {
            match segment {
                CoordinateSegment::Unchanged { base, transformed }
                    if transformed.contains_range_closed(range) =>
                {
                    let start_delta = range.start.saturating_sub(transformed.start);
                    let end_delta = range.end.saturating_sub(transformed.start);
                    let (Some(start), Some(end)) =
                        (base.start.checked_add(start_delta), base.start.checked_add(end_delta))
                    else {
                        return RangeRelation::Invalid;
                    };
                    let mapped = ByteRange::new(start, end);
                    return range_relation(range, mapped);
                }
                CoordinateSegment::Edit { removed_base, inserted_transformed, .. }
                    if !inserted_transformed.is_empty()
                        && inserted_transformed.contains_range_closed(range) =>
                {
                    return RangeRelation::InsertedOnly { transformed: range, base: *removed_base };
                }
                _ => {}
            }
        }

        RangeRelation::Ambiguous
    }
}

/// Fully validated transformed subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidatedTransformation {
    /// Schema version for the validated transformation receipt.
    pub schema_version: u32,
    /// Verified source identity.
    pub source_identity: String,
    /// Versioned transformation profile.
    pub profile_id: String,
    /// Identity of canonical logical source-line geometry.
    pub source_line_policy_id: String,
    /// Base logical row count.
    pub base_line_count: usize,
    /// Transformed logical row count.
    pub transformed_line_count: usize,
    /// Canonical edits with resolved transformed ranges.
    pub edits: Vec<AppliedEdit>,
    /// Exact transformed bytes.
    pub final_bytes: Vec<u8>,
    /// Digest computed from exact transformed bytes.
    pub final_source_identity: String,
    /// Digest of schema, source, profile, exact edits, final bytes, and map.
    pub transformation_identity: String,
    /// Total byte-coordinate relation.
    pub coordinate_map: CoordinateMap,
}

/// Fail-closed transformation errors.
#[derive(Debug)]
pub enum TransformError {
    /// Claimed source identity did not match exact bytes.
    StaleSourceIdentity {
        /// Caller-provided identity.
        claimed: String,
        /// Digest of supplied bytes.
        observed: String,
    },
    /// Base source was not valid UTF-8.
    InvalidSourceUtf8(str::Utf8Error),
    /// Canonical line geometry rejected the base source.
    InvalidSourceGeometry(SourceLineError),
    /// Profile identity was empty or contained control characters.
    InvalidProfileId {
        /// Rejected profile.
        profile_id: String,
    },
    /// No edit was supplied.
    EmptyEditPlan,
    /// Edit identity was empty or contained control characters.
    InvalidEditId {
        /// Rejected identity.
        edit_id: String,
    },
    /// Two edits reused one identity.
    DuplicateEditId {
        /// Duplicated identity.
        edit_id: String,
    },
    /// Base range was reversed.
    ReversedRange {
        /// Owning edit.
        edit_id: String,
        /// Claimed start.
        start: usize,
        /// Claimed end.
        end: usize,
    },
    /// Base range exceeded source length.
    OutOfBounds {
        /// Owning edit.
        edit_id: String,
        /// Claimed range.
        range: ByteRange,
        /// Exact base-source length.
        source_len: usize,
    },
    /// Base boundary was inside a UTF-8 scalar.
    InteriorUtf8Boundary {
        /// Owning edit.
        edit_id: String,
        /// Rejected byte offset.
        offset: usize,
    },
    /// Expected old bytes did not equal the claimed base range.
    WrongExpectedBytes {
        /// Owning edit.
        edit_id: String,
        /// Digest of caller expectation.
        expected_identity: String,
        /// Digest of actual base bytes.
        observed_identity: String,
    },
    /// Replacement bytes were not valid UTF-8.
    InvalidReplacementUtf8 {
        /// Owning edit.
        edit_id: String,
        /// Underlying UTF-8 error.
        source: str::Utf8Error,
    },
    /// Edit would not change exact bytes.
    NoOpEdit {
        /// Owning edit.
        edit_id: String,
    },
    /// Two non-empty base ranges overlapped.
    OverlappingEdits {
        /// First canonical edit.
        first_edit_id: String,
        /// Second canonical edit.
        second_edit_id: String,
    },
    /// An insertion shared or entered another edit boundary.
    AmbiguousEditBoundary {
        /// Insertion edit.
        insertion_edit_id: String,
        /// Conflicting edit.
        other_edit_id: String,
        /// Shared base boundary.
        offset: usize,
    },
    /// Final-length or map arithmetic overflowed.
    ArithmeticOverflow,
    /// Exact final bytes were not valid UTF-8.
    InvalidFinalUtf8(str::Utf8Error),
    /// Canonical line geometry rejected final bytes.
    InvalidFinalGeometry(SourceLineError),
    /// Canonical map material could not be serialized.
    Serialize(serde_json::Error),
}

impl fmt::Display for TransformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleSourceIdentity { claimed, observed } => write!(
                formatter,
                "source identity {claimed:?} does not match exact bytes {observed:?}"
            ),
            Self::InvalidSourceUtf8(source) => {
                write!(formatter, "base source is not UTF-8: {source}")
            }
            Self::InvalidSourceGeometry(source) => {
                write!(formatter, "base source geometry is invalid: {source}")
            }
            Self::InvalidProfileId { profile_id } => {
                write!(formatter, "invalid transformation profile {profile_id:?}")
            }
            Self::EmptyEditPlan => {
                write!(formatter, "exact transformation requires at least one edit")
            }
            Self::InvalidEditId { edit_id } => write!(formatter, "invalid edit id {edit_id:?}"),
            Self::DuplicateEditId { edit_id } => write!(formatter, "duplicate edit id {edit_id:?}"),
            Self::ReversedRange { edit_id, start, end } => {
                write!(formatter, "edit {edit_id:?} has reversed range {start}..{end}")
            }
            Self::OutOfBounds { edit_id, range, source_len } => write!(
                formatter,
                "edit {edit_id:?} range {}..{} exceeds source length {source_len}",
                range.start, range.end
            ),
            Self::InteriorUtf8Boundary { edit_id, offset } => {
                write!(formatter, "edit {edit_id:?} boundary {offset} is inside a UTF-8 scalar")
            }
            Self::WrongExpectedBytes { edit_id, expected_identity, observed_identity } => write!(
                formatter,
                "edit {edit_id:?} expected {expected_identity}, observed {observed_identity}"
            ),
            Self::InvalidReplacementUtf8 { edit_id, source } => {
                write!(formatter, "edit {edit_id:?} replacement is not UTF-8: {source}")
            }
            Self::NoOpEdit { edit_id } => {
                write!(formatter, "edit {edit_id:?} does not change exact bytes")
            }
            Self::OverlappingEdits { first_edit_id, second_edit_id } => {
                write!(formatter, "edits {first_edit_id:?} and {second_edit_id:?} overlap")
            }
            Self::AmbiguousEditBoundary { insertion_edit_id, other_edit_id, offset } => write!(
                formatter,
                "insertion {insertion_edit_id:?} shares byte boundary {offset} with edit {other_edit_id:?}"
            ),
            Self::ArithmeticOverflow => {
                write!(formatter, "transformation byte arithmetic overflowed")
            }
            Self::InvalidFinalUtf8(source) => {
                write!(formatter, "transformed source is not UTF-8: {source}")
            }
            Self::InvalidFinalGeometry(source) => {
                write!(formatter, "transformed source geometry is invalid: {source}")
            }
            Self::Serialize(source) => {
                write!(formatter, "failed to serialize canonical coordinate map: {source}")
            }
        }
    }
}

impl Error for TransformError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSourceUtf8(source)
            | Self::InvalidFinalUtf8(source)
            | Self::InvalidReplacementUtf8 { source, .. } => Some(source),
            Self::InvalidSourceGeometry(source) | Self::InvalidFinalGeometry(source) => {
                Some(source)
            }
            Self::Serialize(source) => Some(source),
            Self::StaleSourceIdentity { .. }
            | Self::InvalidProfileId { .. }
            | Self::EmptyEditPlan
            | Self::InvalidEditId { .. }
            | Self::DuplicateEditId { .. }
            | Self::ReversedRange { .. }
            | Self::OutOfBounds { .. }
            | Self::InteriorUtf8Boundary { .. }
            | Self::WrongExpectedBytes { .. }
            | Self::NoOpEdit { .. }
            | Self::OverlappingEdits { .. }
            | Self::AmbiguousEditBoundary { .. }
            | Self::ArithmeticOverflow => None,
        }
    }
}

/// Identity material serialized to define [`CoordinateMap::identity`]; its
/// form is version-gated by [`COORDINATE_MAP_SCHEMA_VERSION`].
#[derive(Serialize)]
struct MapIdentityMaterial<'a> {
    schema_version: u32,
    source_identity: &'a str,
    final_source_identity: &'a str,
    profile_id: &'a str,
    source_line_policy_id: &'a str,
    segments: &'a [CoordinateSegment],
}

/// Identity material serialized to define
/// [`ValidatedTransformation::transformation_identity`]; its form is
/// version-gated by [`TRANSFORMATION_SCHEMA_VERSION`].
#[derive(Serialize)]
struct TransformationIdentityMaterial<'a> {
    schema_version: u32,
    source_identity: &'a str,
    final_source_identity: &'a str,
    profile_id: &'a str,
    source_line_policy_id: &'a str,
    /// Exact base logical row count under the canonical line policy.
    base_line_count: usize,
    /// Exact transformed logical row count under the canonical line policy.
    transformed_line_count: usize,
    edits: &'a [AppliedEdit],
    coordinate_map_identity: &'a str,
}

/// Apply exact validated edits without consulting parser or fixture semantics.
pub fn apply_exact_edits(
    source: &ContentAddressedSource,
    profile_id: &str,
    mut edits: Vec<ExactEdit>,
) -> Result<ValidatedTransformation, TransformError> {
    if !stable_id_is_valid(profile_id) {
        return Err(TransformError::InvalidProfileId { profile_id: profile_id.to_owned() });
    }
    if edits.is_empty() {
        return Err(TransformError::EmptyEditPlan);
    }

    edits.sort_by(|left, right| {
        (left.base_start, left.base_end, left.edit_id.as_str()).cmp(&(
            right.base_start,
            right.base_end,
            right.edit_id.as_str(),
        ))
    });

    let source_text = str::from_utf8(source.bytes()).map_err(TransformError::InvalidSourceUtf8)?;
    let mut edit_ids = BTreeSet::new();
    for edit in &edits {
        if !stable_id_is_valid(&edit.edit_id) {
            return Err(TransformError::InvalidEditId { edit_id: edit.edit_id.clone() });
        }
        if !edit_ids.insert(edit.edit_id.clone()) {
            return Err(TransformError::DuplicateEditId { edit_id: edit.edit_id.clone() });
        }
        validate_edit(source_text, source.bytes(), edit)?;
    }
    validate_edit_relations(&edits)?;

    let final_capacity = edits.iter().try_fold(source.bytes().len(), |len, edit| {
        len.checked_sub(edit.base_end.saturating_sub(edit.base_start))
            .and_then(|reduced| reduced.checked_add(edit.replacement.len()))
            .ok_or(TransformError::ArithmeticOverflow)
    })?;
    let mut final_bytes = Vec::with_capacity(final_capacity);
    let mut segments = Vec::with_capacity(edits.len().saturating_mul(2).saturating_add(1));
    let mut applied = Vec::with_capacity(edits.len());
    let mut base_cursor = 0usize;

    for edit in &edits {
        if base_cursor < edit.base_start {
            let unchanged = source
                .bytes()
                .get(base_cursor..edit.base_start)
                .ok_or(TransformError::ArithmeticOverflow)?;
            let transformed_start = final_bytes.len();
            final_bytes.extend_from_slice(unchanged);
            let transformed_end = final_bytes.len();
            segments.push(CoordinateSegment::Unchanged {
                base: ByteRange::new(base_cursor, edit.base_start),
                transformed: ByteRange::new(transformed_start, transformed_end),
            });
        }

        let transformed_start = final_bytes.len();
        final_bytes.extend_from_slice(&edit.replacement);
        let transformed_end = final_bytes.len();
        let removed_base = ByteRange::new(edit.base_start, edit.base_end);
        let inserted_transformed = ByteRange::new(transformed_start, transformed_end);
        segments.push(CoordinateSegment::Edit {
            edit_id: edit.edit_id.clone(),
            removed_base,
            inserted_transformed,
        });
        applied.push(AppliedEdit {
            edit_id: edit.edit_id.clone(),
            removed_base,
            inserted_transformed,
            expected_old: edit.expected_old.clone(),
            replacement: edit.replacement.clone(),
            expected_old_identity: sha256_hex(&edit.expected_old),
            replacement_identity: sha256_hex(&edit.replacement),
        });
        base_cursor = edit.base_end;
    }

    if base_cursor < source.bytes().len() {
        let unchanged =
            source.bytes().get(base_cursor..).ok_or(TransformError::ArithmeticOverflow)?;
        let transformed_start = final_bytes.len();
        final_bytes.extend_from_slice(unchanged);
        segments.push(CoordinateSegment::Unchanged {
            base: ByteRange::new(base_cursor, source.bytes().len()),
            transformed: ByteRange::new(transformed_start, final_bytes.len()),
        });
    }

    if final_bytes.len() != final_capacity {
        return Err(TransformError::ArithmeticOverflow);
    }

    let final_text = str::from_utf8(&final_bytes).map_err(TransformError::InvalidFinalUtf8)?;
    let final_lines: LineRecordTable =
        final_text.parse().map_err(TransformError::InvalidFinalGeometry)?;
    let final_source_identity = sha256_hex(&final_bytes);
    let map_material = MapIdentityMaterial {
        schema_version: COORDINATE_MAP_SCHEMA_VERSION,
        source_identity: source.identity(),
        final_source_identity: &final_source_identity,
        profile_id,
        source_line_policy_id: SOURCE_LINE_POLICY_ID,
        segments: &segments,
    };
    let map_bytes = serde_json::to_vec(&map_material).map_err(TransformError::Serialize)?;
    let coordinate_map = CoordinateMap {
        base_len: source.bytes().len(),
        transformed_len: final_bytes.len(),
        segments,
        identity: sha256_hex(&map_bytes),
    };
    let transformation_material = TransformationIdentityMaterial {
        schema_version: TRANSFORMATION_SCHEMA_VERSION,
        source_identity: source.identity(),
        final_source_identity: &final_source_identity,
        profile_id,
        source_line_policy_id: SOURCE_LINE_POLICY_ID,
        base_line_count: source.line_count(),
        transformed_line_count: final_lines.line_count(),
        edits: &applied,
        coordinate_map_identity: coordinate_map.identity(),
    };
    let transformation_bytes =
        serde_json::to_vec(&transformation_material).map_err(TransformError::Serialize)?;

    Ok(ValidatedTransformation {
        schema_version: TRANSFORMATION_SCHEMA_VERSION,
        source_identity: source.identity().to_owned(),
        profile_id: profile_id.to_owned(),
        source_line_policy_id: SOURCE_LINE_POLICY_ID.to_owned(),
        base_line_count: source.line_count(),
        transformed_line_count: final_lines.line_count(),
        edits: applied,
        final_bytes,
        final_source_identity,
        transformation_identity: sha256_hex(&transformation_bytes),
        coordinate_map,
    })
}

fn stable_id_is_valid(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(char::is_control)
}

fn validate_edit(
    source_text: &str,
    source_bytes: &[u8],
    edit: &ExactEdit,
) -> Result<(), TransformError> {
    if edit.base_start > edit.base_end {
        return Err(TransformError::ReversedRange {
            edit_id: edit.edit_id.clone(),
            start: edit.base_start,
            end: edit.base_end,
        });
    }
    let range = edit.base_range();
    if edit.base_end > source_bytes.len() {
        return Err(TransformError::OutOfBounds {
            edit_id: edit.edit_id.clone(),
            range,
            source_len: source_bytes.len(),
        });
    }
    for offset in [edit.base_start, edit.base_end] {
        if !source_text.is_char_boundary(offset) {
            return Err(TransformError::InteriorUtf8Boundary {
                edit_id: edit.edit_id.clone(),
                offset,
            });
        }
    }

    let observed = source_bytes
        .get(edit.base_start..edit.base_end)
        .ok_or(TransformError::ArithmeticOverflow)?;
    if observed != edit.expected_old {
        return Err(TransformError::WrongExpectedBytes {
            edit_id: edit.edit_id.clone(),
            expected_identity: sha256_hex(&edit.expected_old),
            observed_identity: sha256_hex(observed),
        });
    }
    str::from_utf8(&edit.replacement).map_err(|source| TransformError::InvalidReplacementUtf8 {
        edit_id: edit.edit_id.clone(),
        source,
    })?;
    if observed == edit.replacement {
        return Err(TransformError::NoOpEdit { edit_id: edit.edit_id.clone() });
    }

    Ok(())
}

fn validate_edit_relations(edits: &[ExactEdit]) -> Result<(), TransformError> {
    for (index, first) in edits.iter().enumerate() {
        for second in edits.iter().skip(index.saturating_add(1)) {
            let first_range = first.base_range();
            let second_range = second.base_range();
            if !first_range.is_empty()
                && !second_range.is_empty()
                && first_range.start < second_range.end
                && second_range.start < first_range.end
            {
                return Err(TransformError::OverlappingEdits {
                    first_edit_id: first.edit_id.clone(),
                    second_edit_id: second.edit_id.clone(),
                });
            }

            if first_range.is_empty() && second_range.contains_closed(first_range.start) {
                return Err(TransformError::AmbiguousEditBoundary {
                    insertion_edit_id: first.edit_id.clone(),
                    other_edit_id: second.edit_id.clone(),
                    offset: first_range.start,
                });
            }
            if second_range.is_empty() && first_range.contains_closed(second_range.start) {
                return Err(TransformError::AmbiguousEditBoundary {
                    insertion_edit_id: second.edit_id.clone(),
                    other_edit_id: first.edit_id.clone(),
                    offset: second_range.start,
                });
            }
        }
    }
    Ok(())
}

fn point_relation(source_offset: usize, target_offset: usize) -> PositionRelation {
    if source_offset == target_offset {
        PositionRelation::Exact { offset: target_offset }
    } else {
        PositionRelation::Mapped { offset: target_offset }
    }
}

fn edit_boundary_relation(
    offset: usize,
    relations: impl Iterator<Item = (ByteRange, ByteRange)>,
) -> Option<PositionRelation> {
    let mut bounds: Option<(usize, usize)> = None;

    for (source, target) in relations {
        for candidate in [
            (offset == source.start).then_some(target.start),
            (offset == source.end).then_some(target.end),
        ]
        .into_iter()
        .flatten()
        {
            bounds = Some(match bounds {
                Some((lower, upper)) => (lower.min(candidate), upper.max(candidate)),
                None => (candidate, candidate),
            });
        }
    }

    bounds.map(|(lower, upper)| {
        if lower == upper {
            point_relation(offset, lower)
        } else {
            PositionRelation::Ambiguous { lower, upper }
        }
    })
}

fn range_relation(source: ByteRange, target: ByteRange) -> RangeRelation {
    if source == target {
        RangeRelation::Exact { range: target }
    } else {
        RangeRelation::Mapped { range: target }
    }
}

fn zero_range_relation(relation: PositionRelation, queried: ByteRange) -> RangeRelation {
    match relation {
        PositionRelation::Exact { offset } => {
            RangeRelation::Exact { range: ByteRange::new(offset, offset) }
        }
        PositionRelation::Mapped { offset } => {
            RangeRelation::Mapped { range: ByteRange::new(offset, offset) }
        }
        PositionRelation::InsertedOnly { base, .. } => {
            RangeRelation::InsertedOnly { transformed: queried, base }
        }
        PositionRelation::RemovedOnly { transformed, .. } => {
            RangeRelation::RemovedOnly { base: queried, transformed }
        }
        PositionRelation::Ambiguous { .. } => RangeRelation::Ambiguous,
        PositionRelation::Invalid => RangeRelation::Invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn Error>>;

    fn subject(source: &str) -> Result<ContentAddressedSource, TransformError> {
        ContentAddressedSource::from_bytes(source.as_bytes().to_vec())
    }

    fn edit(id: &str, start: usize, end: usize, expected: &str, replacement: &str) -> ExactEdit {
        ExactEdit::new(
            id.to_owned(),
            start,
            end,
            expected.as_bytes().to_vec(),
            replacement.as_bytes().to_vec(),
        )
    }

    #[test]
    fn shuffled_disjoint_edits_produce_identical_bytes_digest_and_map() -> TestResult {
        let source = subject("abcdef")?;
        let first = edit("replace-b", 1, 2, "b", "BB");
        let second = edit("delete-e", 4, 5, "e", "");
        let ordered =
            apply_exact_edits(&source, "test.profile.v1", vec![first.clone(), second.clone()])?;
        let shuffled = apply_exact_edits(&source, "test.profile.v1", vec![second, first])?;

        assert_eq!(ordered.final_bytes, b"aBBcdf");
        assert_eq!(ordered.final_bytes, shuffled.final_bytes);
        assert_eq!(ordered.final_source_identity, shuffled.final_source_identity);
        assert_eq!(ordered.coordinate_map, shuffled.coordinate_map);
        assert_eq!(ordered.source_line_policy_id, SOURCE_LINE_POLICY_ID);

        Ok(())
    }

    #[test]
    fn final_and_transformation_identities_commit_to_exact_replacement_bytes() -> TestResult {
        let source = subject("abc")?;
        let upper_b =
            apply_exact_edits(&source, "test.profile.v1", vec![edit("replace-b", 1, 2, "b", "B")])?;
        let upper_c =
            apply_exact_edits(&source, "test.profile.v1", vec![edit("replace-b", 1, 2, "b", "C")])?;

        assert_eq!(upper_b.final_source_identity, sha256_hex(b"aBc"));
        assert_eq!(upper_c.final_source_identity, sha256_hex(b"aCc"));
        assert_ne!(upper_b.final_source_identity, upper_c.final_source_identity);
        assert_ne!(upper_b.coordinate_map.identity(), upper_c.coordinate_map.identity());
        assert_ne!(upper_b.transformation_identity, upper_c.transformation_identity);
        assert_eq!(upper_b.edits[0].expected_old, b"b");
        assert_eq!(upper_b.edits[0].replacement, b"B");

        Ok(())
    }

    #[test]
    fn crlf_and_bare_cr_keep_exact_byte_geometry() -> TestResult {
        let source = subject("a\r\nb\rc")?;
        let transformed = apply_exact_edits(
            &source,
            "test.profile.v1",
            vec![edit("before-second-line", 3, 3, "", "X")],
        )?;

        assert_eq!(source.line_count(), 2);
        assert_eq!(transformed.transformed_line_count, 2);
        assert_eq!(transformed.final_bytes, b"a\r\nXb\rc");
        assert_eq!(
            transformed.coordinate_map.map_base_position(1),
            PositionRelation::Exact { offset: 1 }
        );
        assert_eq!(
            transformed.coordinate_map.map_base_position(2),
            PositionRelation::Exact { offset: 2 }
        );
        assert_eq!(
            transformed.coordinate_map.map_base_position(3),
            PositionRelation::Ambiguous { lower: 3, upper: 4 }
        );
        assert_eq!(
            transformed.coordinate_map.map_base_position(4),
            PositionRelation::Mapped { offset: 5 }
        );
        assert_eq!(
            transformed.coordinate_map.map_base_range(ByteRange::new(0, 3)),
            RangeRelation::Exact { range: ByteRange::new(0, 3) }
        );
        assert_eq!(
            transformed.coordinate_map.map_base_range(ByteRange::new(3, 6)),
            RangeRelation::Mapped { range: ByteRange::new(4, 7) }
        );

        Ok(())
    }

    #[test]
    fn multiline_bijective_range_round_trips_while_insertion_point_stays_ambiguous() -> TestResult {
        let source = subject("aa\nbb\ncc")?;
        let transformed =
            apply_exact_edits(&source, "test.profile.v1", vec![edit("prefix", 0, 0, "", "!")])?;

        let base = ByteRange::new(3, 8);
        let mapped = ByteRange::new(4, 9);
        assert_eq!(
            transformed.coordinate_map.map_base_position(0),
            PositionRelation::Ambiguous { lower: 0, upper: 1 }
        );
        assert_eq!(
            transformed.coordinate_map.map_base_range(base),
            RangeRelation::Mapped { range: mapped }
        );
        assert_eq!(
            transformed.coordinate_map.map_transformed_range(mapped),
            RangeRelation::Mapped { range: base }
        );

        Ok(())
    }

    #[test]
    fn crossing_or_widening_an_edit_is_ambiguous_and_invalid_ranges_fail() -> TestResult {
        let source = subject("abcdef")?;
        let transformed = apply_exact_edits(
            &source,
            "test.profile.v1",
            vec![edit("replace", 2, 4, "cd", "XYZ")],
        )?;

        assert_eq!(
            transformed.coordinate_map.map_transformed_range(ByteRange::new(1, 5)),
            RangeRelation::Ambiguous
        );
        assert_eq!(
            transformed.coordinate_map.map_base_range(ByteRange::new(4, 3)),
            RangeRelation::Invalid
        );
        assert_eq!(
            transformed.coordinate_map.map_base_range(ByteRange::new(0, 7)),
            RangeRelation::Invalid
        );
        assert_eq!(
            transformed.coordinate_map.map_transformed_position(8),
            PositionRelation::Invalid
        );

        Ok(())
    }

    #[test]
    fn insertion_and_removal_keep_one_way_position_semantics() -> TestResult {
        let source = subject("abcdef")?;
        let transformed = apply_exact_edits(
            &source,
            "test.profile.v1",
            vec![edit("insert", 1, 1, "", "XY"), edit("remove", 3, 5, "de", "")],
        )?;

        assert_eq!(
            transformed.coordinate_map.map_base_position(1),
            PositionRelation::Ambiguous { lower: 1, upper: 3 }
        );
        assert_eq!(
            transformed.coordinate_map.map_transformed_position(2),
            PositionRelation::InsertedOnly { transformed_offset: 2, base: ByteRange::new(1, 1) }
        );
        assert_eq!(
            transformed.coordinate_map.map_base_position(4),
            PositionRelation::RemovedOnly { base_offset: 4, transformed: ByteRange::new(5, 5) }
        );
        assert_eq!(
            transformed.coordinate_map.map_transformed_position(5),
            PositionRelation::Ambiguous { lower: 3, upper: 5 }
        );

        Ok(())
    }

    #[test]
    fn adjacent_deletions_aggregate_the_complete_collapsed_boundary() -> TestResult {
        let two_deletions = apply_exact_edits(
            &subject("abc")?,
            "test.profile.v1",
            vec![edit("delete-a", 0, 1, "a", ""), edit("delete-b", 1, 2, "b", "")],
        )?;
        assert_eq!(two_deletions.final_bytes, b"c");
        assert_eq!(
            two_deletions.coordinate_map.map_transformed_position(0),
            PositionRelation::Ambiguous { lower: 0, upper: 2 }
        );
        assert_eq!(
            two_deletions.coordinate_map.map_transformed_range(ByteRange::new(0, 0)),
            RangeRelation::Ambiguous
        );

        let source = subject("abcd")?;
        let transformed = apply_exact_edits(
            &source,
            "test.profile.v1",
            vec![
                edit("delete-a", 0, 1, "a", ""),
                edit("delete-b", 1, 2, "b", ""),
                edit("delete-c", 2, 3, "c", ""),
            ],
        )?;

        assert_eq!(transformed.final_bytes, b"d");
        assert_eq!(
            transformed.coordinate_map.map_transformed_position(0),
            PositionRelation::Ambiguous { lower: 0, upper: 3 }
        );
        assert_eq!(
            transformed.coordinate_map.map_transformed_range(ByteRange::new(0, 0)),
            RangeRelation::Ambiguous
        );
        assert_eq!(
            transformed.coordinate_map.map_base_position(1),
            PositionRelation::Mapped { offset: 0 }
        );
        assert_eq!(
            transformed.coordinate_map.map_base_position(2),
            PositionRelation::Mapped { offset: 0 }
        );

        Ok(())
    }

    #[test]
    fn adjacent_replacement_and_deletion_keep_the_full_reverse_boundary() -> TestResult {
        let source = subject("abc")?;
        let transformed = apply_exact_edits(
            &source,
            "test.profile.v1",
            vec![edit("expand-a", 0, 1, "a", "XX"), edit("delete-b", 1, 2, "b", "")],
        )?;

        assert_eq!(transformed.final_bytes, b"XXc");
        assert_eq!(
            transformed.coordinate_map.map_transformed_position(2),
            PositionRelation::Ambiguous { lower: 1, upper: 2 }
        );
        assert_eq!(
            transformed.coordinate_map.map_transformed_range(ByteRange::new(2, 2)),
            RangeRelation::Ambiguous
        );

        Ok(())
    }

    #[test]
    fn bijective_ranges_map_but_cross_edit_ranges_are_ambiguous() -> TestResult {
        let source = subject("abcdef")?;
        let transformed = apply_exact_edits(
            &source,
            "test.profile.v1",
            vec![edit("replace", 2, 4, "cd", "XYZ")],
        )?;

        assert_eq!(
            transformed.coordinate_map.map_base_range(ByteRange::new(4, 6)),
            RangeRelation::Mapped { range: ByteRange::new(5, 7) }
        );
        assert_eq!(
            transformed.coordinate_map.map_base_range(ByteRange::new(1, 5)),
            RangeRelation::Ambiguous
        );
        assert_eq!(
            transformed.coordinate_map.map_transformed_range(ByteRange::new(2, 5)),
            RangeRelation::InsertedOnly {
                transformed: ByteRange::new(2, 5),
                base: ByteRange::new(2, 4),
            }
        );

        Ok(())
    }

    #[test]
    fn canonical_line_geometry_distinguishes_crlf_bare_cr_bom_and_eof() -> TestResult {
        let source_text = "\u{feff}a\r\nβ\rc";
        let source = subject(source_text)?;
        let eof = source.bytes().len();
        let transformed =
            apply_exact_edits(&source, "test.profile.v1", vec![edit("eof", eof, eof, "", "\n")])?;

        assert_eq!(source.line_count(), 2);
        assert_eq!(transformed.base_line_count, 2);
        assert_eq!(transformed.transformed_line_count, 3);
        assert_eq!(
            transformed.coordinate_map.map_base_position(eof),
            PositionRelation::Ambiguous { lower: eof, upper: eof + 1 }
        );
        assert_eq!(transformed.final_bytes, format!("{source_text}\n").as_bytes());
        assert_eq!(transformed.final_source_identity, sha256_hex(&transformed.final_bytes));

        Ok(())
    }

    #[test]
    fn interior_utf8_boundary_is_rejected_before_construction() -> TestResult {
        let source = subject("aβc")?;
        let result = apply_exact_edits(
            &source,
            "test.profile.v1",
            vec![ExactEdit::new("inside-beta".to_owned(), 2, 3, vec![0xb2], b"x".to_vec())],
        );

        assert!(matches!(
            result,
            Err(TransformError::InteriorUtf8Boundary {
                edit_id,
                offset: 2,
            }) if edit_id == "inside-beta"
        ));

        Ok(())
    }

    #[test]
    fn wrong_old_bytes_overlap_and_shared_insertion_boundary_fail_closed() -> TestResult {
        let source = subject("abcdef")?;

        assert!(matches!(
            apply_exact_edits(&source, "test.profile.v1", vec![edit("wrong", 1, 2, "x", "B")]),
            Err(TransformError::WrongExpectedBytes { .. })
        ));
        assert!(matches!(
            apply_exact_edits(
                &source,
                "test.profile.v1",
                vec![edit("first", 1, 4, "bcd", "B"), edit("second", 3, 5, "de", "D")]
            ),
            Err(TransformError::OverlappingEdits { .. })
        ));
        assert!(matches!(
            apply_exact_edits(
                &source,
                "test.profile.v1",
                vec![edit("replace", 1, 3, "bc", "B"), edit("insert", 3, 3, "", "X")]
            ),
            Err(TransformError::AmbiguousEditBoundary { .. })
        ));

        Ok(())
    }

    #[test]
    fn stale_source_identity_and_invalid_replacement_fail_closed() -> TestResult {
        assert!(matches!(
            ContentAddressedSource::from_claimed("sha256:stale".to_owned(), b"abc".to_vec()),
            Err(TransformError::StaleSourceIdentity { .. })
        ));

        let source = subject("abc")?;
        let result = apply_exact_edits(
            &source,
            "test.profile.v1",
            vec![ExactEdit::new("invalid-utf8".to_owned(), 1, 2, b"b".to_vec(), vec![0xff])],
        );
        assert!(matches!(result, Err(TransformError::InvalidReplacementUtf8 { .. })));

        Ok(())
    }

    #[test]
    fn reversed_out_of_bounds_noop_and_empty_plans_fail_closed() -> TestResult {
        let source = subject("abc")?;

        assert!(matches!(
            apply_exact_edits(&source, "test.profile.v1", vec![edit("reversed", 2, 1, "", "x")]),
            Err(TransformError::ReversedRange { .. })
        ));
        assert!(matches!(
            apply_exact_edits(&source, "test.profile.v1", vec![edit("outside", 3, 4, "", "x")]),
            Err(TransformError::OutOfBounds { .. })
        ));
        assert!(matches!(
            apply_exact_edits(&source, "test.profile.v1", vec![edit("noop", 1, 2, "b", "b")]),
            Err(TransformError::NoOpEdit { .. })
        ));
        assert!(matches!(
            apply_exact_edits(&source, "test.profile.v1", Vec::new()),
            Err(TransformError::EmptyEditPlan)
        ));

        Ok(())
    }

    #[test]
    fn invalid_and_duplicate_identity_gates_fail_closed() -> TestResult {
        let source = subject("abc")?;

        assert!(matches!(
            apply_exact_edits(&source, "", vec![edit("edit", 0, 1, "a", "x")]),
            Err(TransformError::InvalidProfileId { .. })
        ));
        assert!(matches!(
            apply_exact_edits(&source, "ctrl-\u{7}profile", vec![edit("edit", 0, 1, "a", "x")]),
            Err(TransformError::InvalidProfileId { .. })
        ));
        assert!(matches!(
            apply_exact_edits(&source, "test.profile.v1", vec![edit("", 0, 1, "a", "x")]),
            Err(TransformError::InvalidEditId { .. })
        ));
        assert!(matches!(
            apply_exact_edits(
                &source,
                "test.profile.v1",
                vec![edit("duplicate", 0, 1, "a", "x"), edit("duplicate", 1, 2, "b", "y")]
            ),
            Err(TransformError::DuplicateEditId { .. })
        ));

        Ok(())
    }
}
