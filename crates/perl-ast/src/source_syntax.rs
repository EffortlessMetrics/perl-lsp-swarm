//! Source-backed string and heredoc payload types (#8593, parent #8246).
//!
//! # Role
//!
//! This module is the **AST data contract** for string and heredoc payloads. It
//! adds no [`NodeKind`](crate::NodeKind) variant, changes no existing variant's
//! child fields, and produces no parser output. Population of these types from
//! lexer and parser facts is owned by the following slices (#8608, #8621,
//! #8636, #8654); retirement of the cooked-only compatibility fields is owned
//! by #8725.
//!
//! Because no variant and no child field changes here, the #8415/#8424
//! structural registry gains no row and no cardinality. Segment children become
//! canonical traversal children only in the population slices that actually
//! attach them.
//!
//! # The propositions this contract keeps distinct
//!
//! Today the AST collapses a string into `String { value, interpolated }` and a
//! heredoc into `Heredoc { delimiter, content, interpolated, indented, command,
//! body_span }`. That is one cooked payload plus booleans, so five distinct
//! facts share one slot:
//!
//! - **raw source spelling** is not the **cooked value**. `"a\nb"` is four
//!   source bytes of escape and a one-byte cooked fragment; a single `String`
//!   field cannot hold both.
//! - **ordered segments** are not a cooked string. An interpolation carries an
//!   ordinary AST child at its own exact range, not the enclosing payload range.
//! - **legitimately empty** is not **unavailable**. `''` has proven zero-length
//!   content; a recovered payload has no content facts at all. Both would be
//!   the empty string in the cooked field.
//! - **complete** is not **unterminated, recovered, or budgeted**. A truncated
//!   payload must not be able to look finished.
//! - a **command** payload (`qx`, backticks, `` <<`EOF` ``) is a dynamic
//!   execution boundary, not a pure literal.
//! - **normalized-away source** is still source. A `<<~` body's stripped indent
//!   exists in raw bytes and not in the cooked value, so it is its own
//!   [`SourceSegmentPayload::Normalization`] run rather than a mislabelled
//!   literal or a hole in an otherwise exact segmentation.
//!
//! Each of those is a separate typed position here, so a consumer can no longer
//! be silently wrong about which one it holds.
//!
//! # Cooked values are never authoritative
//!
//! Raw source stays authoritative. [`CookedValue::proven_text`] is the only
//! accessor that yields a value a consumer may present as the payload's
//! meaning, and it is `Some` only for [`CookedValue::Proven`]. Partial,
//! dynamic, and unavailable dispositions have no proven text by construction.
//!
//! # Coherence is checked, not assumed
//!
//! Every field is public so a producer can build a payload incrementally, which
//! means the field cross-product includes combinations describing no real
//! source: segments recorded out of order, an exact segmentation with a hole, a
//! proven value on an unterminated payload. Naming a variant `Exact` does not
//! make its segments ordered, contiguous, or contained.
//!
//! [`StringSyntax::contradictions`] and [`HeredocSyntax::contradictions`] check
//! the payload against its own geometry, and the accessors that *publish* a
//! result — [`StringSyntax::proven_segments`],
//! [`StringSyntax::compat_value`], and their heredoc counterparts — decline
//! rather than pass a contradictory payload's data off as a proven one.
//! [`SourceSegmentation::observed_segments`] remains the unchecked view and
//! says so.
//!
//! # Compatibility projections
//!
//! The `compat_*` methods are **derived views** computed from the typed state
//! on every call. They hold no storage, they have no setter, and there is no
//! constructor that accepts one. A projection cannot become canonical input:
//! change the typed state and the projection changes with it.
//!
//! The legacy fields cannot express unavailability, so the projections return
//! [`Option`] where the legacy field was total. That gap is the reason for the
//! migration, not an omission.
//!
//! A `compat_*` method reports what the *legacy field* meant, which for a
//! command form was the command's source text rather than its output.
//! [`StringSyntax::proven_literal_value`] and
//! [`HeredocSyntax::proven_literal_value`] are the accessors for a value a
//! consumer may present as the expression's own, and they refuse an execution
//! boundary.
//!
//! # Depth safety
//!
//! [`SourceSegmentPayload::Interpolation`] owns a boxed [`Node`]. `Node`'s
//! `Clone`, `PartialEq`, `Debug`, and `Drop` are already iterative, and the
//! types here add only a flat [`Vec`] over them, so no new recursive
//! whole-tree operation is introduced by this module.

use crate::Node;
use perl_position_tracking::SourceLocation;

/// Whether a cooked (escape- and interpolation-processed) value is provable.
///
/// This is the disposition of a *value*, not of a range. A payload with proven
/// zero-length content has `Proven(String::new())`; a payload whose value was
/// never computed has [`CookedValue::Unavailable`]. Those are different facts
/// and this type keeps them apart.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CookedValue {
    /// The complete cooked value is statically proven for this payload.
    ///
    /// An empty `String` here means the payload's value is provably empty, not
    /// that the value is missing.
    Proven(String),
    /// Some cooked text was recovered, but it is not the complete value.
    ///
    /// Carried for diagnostics and presentation-free inspection. It is not a
    /// value a consumer may present as the payload's meaning; see
    /// [`CookedValue::proven_text`].
    Partial(String),
    /// The value depends on runtime state and has no static cooked form.
    ///
    /// Interpolation of runtime values and command execution land here.
    Dynamic,
    /// No cooked value was computed or could be proven.
    Unavailable,
}

impl CookedValue {
    /// The cooked text a consumer may treat as this payload's meaning.
    ///
    /// `Some` only for [`CookedValue::Proven`]. Partial, dynamic, and
    /// unavailable dispositions deliberately yield `None` rather than a
    /// plausible string.
    #[must_use]
    pub fn proven_text(&self) -> Option<&str> {
        match self {
            Self::Proven(text) => Some(text.as_str()),
            Self::Partial(_) | Self::Dynamic | Self::Unavailable => None,
        }
    }

    /// The incomplete cooked text recovered for this payload, if any.
    ///
    /// `Some` only for [`CookedValue::Partial`]. This text is not the payload's
    /// value and must not be presented as one.
    #[must_use]
    pub fn partial_text(&self) -> Option<&str> {
        match self {
            Self::Partial(text) => Some(text.as_str()),
            Self::Proven(_) | Self::Dynamic | Self::Unavailable => None,
        }
    }

    /// Whether this disposition proves a complete cooked value.
    #[must_use]
    pub const fn is_proven(&self) -> bool {
        matches!(self, Self::Proven(_))
    }

    /// Whether the value is a runtime-dependent one rather than a missing one.
    #[must_use]
    pub const fn is_dynamic(&self) -> bool {
        matches!(self, Self::Dynamic)
    }
}

/// Why a source run could not be classified into a literal, escape, or
/// interpolation segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SegmentRecoveryCause {
    /// An interpolation opened but never closed within the payload.
    UnterminatedInterpolation,
    /// An interpolation was delimited but its expression did not parse.
    UnparsedInterpolation,
    /// An escape introducer was present but the sequence was malformed.
    MalformedEscape,
    /// A scan or allocation budget stopped analysis of this run.
    BudgetExhausted,
}

/// A rule that removes or rewrites source bytes on the way to the cooked value
/// without being an escape sequence.
///
/// These runs exist in raw source and are absent from — or shortened in — the
/// cooked value. They are ordinary, successfully classified source: they are
/// not escapes, and they are not recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NormalizationRule {
    /// `<<~` indentation stripping removed this leading run from a body line.
    ///
    /// The raw body of an indented heredoc contains the indentation; the cooked
    /// body does not. The parser already strips it
    /// (`perl-parser-core::syntax::heredoc`), so the bytes must have a home
    /// here or an exact segmentation could not cover the body's source.
    HeredocIndentStrip,
    /// A line ending was rewritten, so the raw run and its cooked contribution
    /// differ in length (`\r\n` contributing `\n`, for instance).
    LineEnding,
}

/// What one ordered run of payload source is.
///
/// The run's extent and cooked contribution live on [`SourceSegment`]; this
/// enum carries only what distinguishes the kinds from each other.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SourceSegmentPayload {
    /// Verbatim source text contributing itself to the cooked value.
    Literal,
    /// An escape sequence whose cooked contribution differs from its source.
    Escape,
    /// Source removed or rewritten by a normalization rule rather than by an
    /// escape.
    ///
    /// [`SourceSegment::cooked_fragment`] carries what the run contributes,
    /// which is `Proven("")` for a stripped `<<~` indent run.
    Normalization {
        /// Which rule accounts for the difference between raw and cooked.
        rule: NormalizationRule,
    },
    /// An interpolated expression parsed as an ordinary AST child.
    ///
    /// The child's own source range is [`SourceSegment::raw_range`], never the
    /// enclosing payload's range.
    Interpolation {
        /// The parsed expression occupying this segment.
        expression: Box<Node>,
    },
    /// Source that belongs to the payload but could not be classified.
    Recovery {
        /// Why classification failed.
        cause: SegmentRecoveryCause,
    },
}

/// One ordered run of a string or heredoc payload's source.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceSegment {
    /// Exact source range of this run.
    ///
    /// This is the run's own extent. Assigning the enclosing payload's range
    /// here would erase the geometry this contract exists to keep.
    pub raw_range: SourceLocation,
    /// What this run contributes to the payload's cooked value.
    ///
    /// [`CookedValue::Dynamic`] for a runtime-dependent interpolation;
    /// [`CookedValue::Unavailable`] when no fragment was computed.
    pub cooked_fragment: CookedValue,
    /// What kind of run this is.
    pub payload: SourceSegmentPayload,
}

impl SourceSegment {
    /// The interpolated expression occupying this run, if it is one.
    #[must_use]
    pub fn interpolation_expression(&self) -> Option<&Node> {
        match &self.payload {
            SourceSegmentPayload::Interpolation { expression } => Some(expression.as_ref()),
            SourceSegmentPayload::Literal
            | SourceSegmentPayload::Escape
            | SourceSegmentPayload::Normalization { .. }
            | SourceSegmentPayload::Recovery { .. } => None,
        }
    }

    /// Whether this run is a recovery placeholder rather than classified source.
    #[must_use]
    pub const fn is_recovery(&self) -> bool {
        matches!(self.payload, SourceSegmentPayload::Recovery { .. })
    }

    /// Whether this run's kind can produce the cooked fragment it carries.
    ///
    /// A recovery run failed classification and an interpolation run depends on
    /// runtime state, so neither can contribute statically proven text. Literal
    /// and normalization runs can.
    #[must_use]
    pub const fn kind_can_prove_its_fragment(&self) -> bool {
        match self.payload {
            SourceSegmentPayload::Literal
            | SourceSegmentPayload::Escape
            | SourceSegmentPayload::Normalization { .. } => true,
            SourceSegmentPayload::Interpolation { .. } | SourceSegmentPayload::Recovery { .. } => {
                !self.cooked_fragment.is_proven()
            }
        }
    }
}

/// How completely a payload's source has been segmented.
///
/// This type owns the segment list so that "proven to have no segments" and
/// "never segmented" cannot be spelled the same way. An empty [`Vec`] under
/// [`SourceSegmentation::Exact`] is a proven-empty payload;
/// [`SourceSegmentation::Unavailable`] is an unsegmented one.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SourceSegmentation {
    /// Segmentation was not performed, so no segment facts exist.
    ///
    /// A population slice that has not yet proven segmentation uses this rather
    /// than an empty segment list (#8608).
    Unavailable,
    /// The complete ordered segments covering the payload's content.
    ///
    /// May be empty when the content is provably empty.
    Exact(Vec<SourceSegment>),
    /// An ordered prefix of scanned segments; the remainder was not classified.
    ///
    /// The prefix is what a scan recorded before it stopped, not a proven
    /// result. `proven` is reserved for [`SourceSegmentation::Exact`] and
    /// [`CookedValue::Proven`]; nothing here may be treated as authoritative.
    /// [`SourceSegment::is_recovery`] reports what a given run actually is.
    Partial(Vec<SourceSegment>),
}

impl SourceSegmentation {
    /// Every segment recorded so far, in the order they were recorded.
    ///
    /// Recorded order is source order only when the enclosing payload is
    /// coherent; nothing here checks that, because the content range this must
    /// be checked against lives on the payload rather than on this enum. Use
    /// [`StringSyntax::proven_segments`] or [`HeredocSyntax::proven_segments`]
    /// for the checked accessor, and [`StringSyntax::contradictions`] or
    /// [`HeredocSyntax::contradictions`] for what is wrong when they decline.
    ///
    /// Returns an empty slice for [`SourceSegmentation::Unavailable`].
    #[must_use]
    pub fn observed_segments(&self) -> &[SourceSegment] {
        match self {
            Self::Exact(segments) | Self::Partial(segments) => segments.as_slice(),
            Self::Unavailable => &[],
        }
    }

    /// Whether segmentation claims to be complete for this payload.
    ///
    /// This is the claim the producer recorded, not a checked fact; see
    /// [`StringSyntax::proven_segments`].
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        matches!(self, Self::Exact(_))
    }

    /// Whether no segment facts exist at all.
    #[must_use]
    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable)
    }

    /// The interpolated expressions recorded for this payload, in recorded
    /// order.
    ///
    /// Recorded order is source order only for a coherent payload, as with
    /// [`SourceSegmentation::observed_segments`].
    pub fn interpolation_expressions(&self) -> impl Iterator<Item = &Node> + '_ {
        self.observed_segments().iter().filter_map(SourceSegment::interpolation_expression)
    }
}

/// A way a payload's fields contradict each other.
///
/// Every field of [`StringSyntax`] and [`HeredocSyntax`] is public so a
/// producer can build a payload incrementally, which means the field
/// cross-product includes combinations that describe no real source. This enum
/// names the ones that matter, and the checked accessors decline rather than
/// publish a value derived from a contradictory payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PayloadContradiction {
    /// The content region is not inside the payload's raw region.
    ContentRangeOutsideRaw,
    /// Segments were recorded but no content region was established.
    SegmentsWithoutContentRange,
    /// A segment lies outside the content region it claims to cover.
    SegmentOutsideContent {
        /// Index of the offending segment in recorded order.
        index: usize,
    },
    /// A segment starts before the previous one, so recorded order is not
    /// source order.
    SegmentsOutOfOrder {
        /// Index of the segment that goes backwards.
        index: usize,
    },
    /// A segment starts before the previous one ends.
    SegmentsOverlap {
        /// Index of the overlapping segment.
        index: usize,
    },
    /// Exact segmentation leaves a hole between two segments.
    SegmentGap {
        /// Index of the segment that begins after the hole.
        index: usize,
    },
    /// Exact segmentation does not reach both ends of the content region.
    ExactSegmentationLeavesContentUncovered,
    /// A proven cooked value on a payload whose source is not proven
    /// terminated: a truncated payload claiming a complete value.
    ProvenValueOnUnterminatedPayload,
    /// [`PayloadTerminal::Empty`] on a payload whose content region is not
    /// zero-length.
    EmptyTerminalWithNonEmptyContent,
    /// An exact segmentation of a content region that was never established.
    ///
    /// Distinct from [`PayloadContradiction::SegmentsWithoutContentRange`]:
    /// this fires even for an empty segment list, because
    /// `Exact(vec![])` over no region would otherwise read as "proven to have
    /// no segments" for a payload with no geometry at all.
    ExactSegmentationWithoutContentRange,
    /// An interpolation segment whose expression does not sit at the segment's
    /// own range.
    InterpolationRangeMismatch {
        /// Index of the offending segment in recorded order.
        index: usize,
    },
    /// A proven payload value that the payload's own proven segments do not
    /// assemble into.
    CookedValueDisagreesWithSegments,
    /// A heredoc label range that is not inside its declaration's opener.
    LabelRangeOutsideOpener,
    /// A heredoc whose terminal claims termination with no terminator region.
    TerminatedHeredocWithoutTerminator,
    /// A heredoc recorded as unterminated that nonetheless has a terminator.
    UnterminatedHeredocWithTerminator,
    /// A heredoc whose declaration, body, and terminator are not in source
    /// order, or overlap.
    HeredocRegionsOutOfOrder,
    /// A heredoc full region that does not contain one of its own parts.
    FullRegionMissesAPart,
    /// A payload-level range whose start is after its end.
    ///
    /// `SourceLocation` fields are public, so an inverted span is
    /// constructible even though the constructors reject one, and
    /// `SourceLocation::len` panics on it in a debug build. Checked before
    /// any ordering or containment conclusion is drawn from a range.
    MalformedRange,
    /// A segment range whose start is after its end.
    MalformedSegmentRange {
        /// Index of the offending segment in recorded order.
        index: usize,
    },
    /// A terminal that proves delimiters over a content region that was never
    /// established.
    TerminalRequiresContentRange,
    /// [`PayloadTerminal::Complete`] over a zero-length content region.
    ///
    /// `Complete` is defined as proven delimiters with non-empty content; a
    /// provably empty payload is [`PayloadTerminal::Empty`].
    CompleteTerminalWithEmptyContent,
    /// A proven payload value over an exact segmentation that contains a
    /// runtime-dependent run.
    ///
    /// If any segment's contribution is [`CookedValue::Dynamic`], the payload's
    /// value depends on runtime state and cannot have been statically proven.
    /// A merely *unavailable* fragment is different: the value may have been
    /// proven by other means, so that combination is left alone.
    ProvenValueOverDynamicSegments,
    /// A written form and a delimiter pair that cannot occur together.
    ///
    /// `'...'`, `"..."`, and `` `...` `` each fix their own delimiter; only the
    /// `q`-family forms choose one.
    FormDelimiterMismatch,
    /// A segment whose kind cannot produce the cooked fragment it carries.
    ///
    /// Recovery runs failed classification and interpolation runs depend on
    /// runtime state, so neither can contribute statically proven text.
    SegmentKindDisagreesWithCookedFragment {
        /// Index of the offending segment in recorded order.
        index: usize,
    },
    /// A terminal proving empty source alongside a value that contradicts it.
    ///
    /// Distinct from
    /// [`PayloadContradiction::EmptyTerminalWithNonEmptyContent`], which
    /// compares the terminal against the *content region*. This compares it
    /// against the *value*, and is the only check that fires when the region is
    /// correctly empty but the payload still claims text over it.
    ///
    /// Proven-empty source rejects a non-empty [`CookedValue::Proven`] or
    /// [`CookedValue::Partial`] fragment, and any [`CookedValue::Dynamic`]
    /// value, since an empty region leaves nothing to interpolate.
    /// [`CookedValue::Unavailable`] stays legitimate: it claims nothing.
    EmptyTerminalWithContradictoryValue,
    /// A terminated string whose content leaves no room for its own syntax.
    ///
    /// A string's `raw_range` spans the quote operator and both delimiters, so
    /// proven-terminated content must leave at least that many bytes at each
    /// end: one byte per delimiter character, plus `q`, `qq`, or `qx` where the
    /// form carries one. Whitespace is permitted between a quote operator and
    /// its delimiter, so this is a lower bound rather than an exact width.
    ///
    /// Only `Unterminated` may reach the right edge, having no closing
    /// delimiter to fit. Heredoc bodies carry no delimiters, so this is checked
    /// on strings alone.
    ///
    /// This assumes every representable [`StringForm`] is delimited, which
    /// holds today; an undelimited form (recorded on issue 8608) would have to
    /// revisit it.
    TerminatedStringWithoutDelimiterBytes,
}

/// Check one payload's geometry and disposition agreement.
///
/// `content_range` is the region the segments must tile: a string's content
/// between its delimiters, or a heredoc's raw body.
fn payload_contradictions(
    raw_range: Option<SourceLocation>,
    content_range: Option<SourceLocation>,
    segmentation: &SourceSegmentation,
    cooked: &CookedValue,
    terminal: PayloadTerminal,
) -> Vec<PayloadContradiction> {
    let mut found = Vec::new();

    // Wellformedness first: every later conclusion reads these bounds, and
    // `SourceLocation::len` panics on an inverted one.
    if raw_range.is_some_and(is_malformed) || content_range.is_some_and(is_malformed) {
        found.push(PayloadContradiction::MalformedRange);
    }

    if let (Some(raw), Some(content)) = (raw_range, content_range)
        && !raw.contains_span(content)
    {
        found.push(PayloadContradiction::ContentRangeOutsideRaw);
    }

    if cooked.is_proven() && !terminal.is_terminated() {
        found.push(PayloadContradiction::ProvenValueOnUnterminatedPayload);
    }

    if terminal.is_terminated() && content_range.is_none() {
        found.push(PayloadContradiction::TerminalRequiresContentRange);
    }

    if terminal.proves_empty_content() && content_range.is_some_and(|range| !range.is_empty()) {
        found.push(PayloadContradiction::EmptyTerminalWithNonEmptyContent);
    }

    // The region being correctly empty is not enough: the value is a separate
    // proposition, and an unsegmented payload has nothing else to contradict.
    // Proven-empty source can carry no text by any route, so a non-empty
    // fragment contradicts it whether or not the producer calls it proven, and
    // an empty region leaves nothing for a runtime-dependent value to read.
    // `Unavailable` stays legitimate: "not computed" is not a claim.
    let value_contradicts_empty_source = match cooked {
        CookedValue::Proven(text) | CookedValue::Partial(text) => !text.is_empty(),
        CookedValue::Dynamic => true,
        CookedValue::Unavailable => false,
    };
    if terminal.proves_empty_content() && value_contradicts_empty_source {
        found.push(PayloadContradiction::EmptyTerminalWithContradictoryValue);
    }

    if matches!(terminal, PayloadTerminal::Complete)
        && content_range.is_some_and(|range| range.is_empty())
    {
        found.push(PayloadContradiction::CompleteTerminalWithEmptyContent);
    }

    let segments = segmentation.observed_segments();
    let Some(content) = content_range else {
        // An exact segmentation of a region that does not exist proves nothing,
        // and an empty one must not read as "proven to have no segments".
        if segmentation.is_exact() {
            found.push(PayloadContradiction::ExactSegmentationWithoutContentRange);
        }
        if !segments.is_empty() {
            found.push(PayloadContradiction::SegmentsWithoutContentRange);
        }
        return found;
    };

    let mut previous_end = content.start;
    for (index, segment) in segments.iter().enumerate() {
        let range = segment.raw_range;
        if is_malformed(range) {
            found.push(PayloadContradiction::MalformedSegmentRange { index });
        }
        if !segment.kind_can_prove_its_fragment() {
            found.push(PayloadContradiction::SegmentKindDisagreesWithCookedFragment { index });
        }
        if range.start < content.start || range.end > content.end {
            found.push(PayloadContradiction::SegmentOutsideContent { index });
        }
        if range.start < previous_end {
            // Starting before the previous segment ended is either a backwards
            // step or an overlap; report the one that describes it.
            if index > 0 && range.start < segments[index - 1].raw_range.start {
                found.push(PayloadContradiction::SegmentsOutOfOrder { index });
            } else {
                found.push(PayloadContradiction::SegmentsOverlap { index });
            }
        } else if range.start > previous_end {
            // Exact tiles the whole region and Partial is a prefix of it, so a
            // hole inside the recorded run contradicts either name.
            found.push(PayloadContradiction::SegmentGap { index });
        }
        if let SourceSegmentPayload::Interpolation { expression } = &segment.payload {
            if is_malformed(expression.location) {
                found.push(PayloadContradiction::MalformedSegmentRange { index });
            }
            if expression.location != range {
                found.push(PayloadContradiction::InterpolationRangeMismatch { index });
            }
        }
        previous_end = previous_end.max(range.end);
    }

    if segmentation.is_exact() && previous_end != content.end {
        found.push(PayloadContradiction::ExactSegmentationLeavesContentUncovered);
    }

    if cooked.is_proven()
        && segmentation.is_exact()
        && segments.iter().any(|segment| segment.cooked_fragment.is_dynamic())
    {
        found.push(PayloadContradiction::ProvenValueOverDynamicSegments);
    }

    if let Some(payload_text) = cooked.proven_text()
        && segmentation.is_exact()
        && let Some(assembled) = assemble_proven_fragments(segments)
        && assembled != payload_text
    {
        found.push(PayloadContradiction::CookedValueDisagreesWithSegments);
    }

    found
}

/// Whether a range's start is after its end.
///
/// `SourceLocation`'s constructors reject this, but its fields are public, so
/// a struct literal can still produce one.
const fn is_malformed(range: SourceLocation) -> bool {
    range.start > range.end
}

/// Concatenate segment cooked fragments when every one of them is proven.
///
/// `None` as soon as any fragment is partial, dynamic, or unavailable: the
/// payload's value then cannot be reconstructed from its segments, so there is
/// nothing to compare against.
fn assemble_proven_fragments(segments: &[SourceSegment]) -> Option<String> {
    let mut assembled = String::new();
    for segment in segments {
        assembled.push_str(segment.cooked_fragment.proven_text()?);
    }
    Some(assembled)
}

/// Check a heredoc's declaration, body, and terminator geometry.
///
/// Strings carry no terminator range, so this is heredoc-only and sits beside
/// the shared payload check rather than inside it.
fn heredoc_geometry_contradictions(heredoc: &HeredocSyntax) -> Vec<PayloadContradiction> {
    let mut found = Vec::new();
    let declaration = &heredoc.declaration;

    if is_malformed(declaration.opener_range)
        || is_malformed(declaration.label_range)
        || heredoc.terminator_range.is_some_and(is_malformed)
        || heredoc.full_region.is_some_and(is_malformed)
    {
        found.push(PayloadContradiction::MalformedRange);
    }

    if !declaration.opener_range.contains_span(declaration.label_range) {
        found.push(PayloadContradiction::LabelRangeOutsideOpener);
    }

    if heredoc.terminal.is_terminated() && heredoc.terminator_range.is_none() {
        found.push(PayloadContradiction::TerminatedHeredocWithoutTerminator);
    }
    if matches!(heredoc.terminal, PayloadTerminal::Unterminated)
        && heredoc.terminator_range.is_some()
    {
        found.push(PayloadContradiction::UnterminatedHeredocWithTerminator);
    }

    // Declaration, then body, then terminator: no overlap and no going back.
    if let Some(body) = heredoc.raw_body_range
        && declaration.opener_range.end > body.start
    {
        found.push(PayloadContradiction::HeredocRegionsOutOfOrder);
    }
    if let (Some(body), Some(terminator)) = (heredoc.raw_body_range, heredoc.terminator_range)
        && body.end > terminator.start
    {
        found.push(PayloadContradiction::HeredocRegionsOutOfOrder);
    }
    // With no body between them, the declaration and terminator still have to
    // be in order; the two checks above both skip that case.
    if heredoc.raw_body_range.is_none()
        && let Some(terminator) = heredoc.terminator_range
        && declaration.opener_range.end > terminator.start
    {
        found.push(PayloadContradiction::HeredocRegionsOutOfOrder);
    }

    if let Some(full) = heredoc.full_region {
        let covered = full.contains_span(declaration.opener_range)
            && heredoc.raw_body_range.is_none_or(|body| full.contains_span(body))
            && heredoc.terminator_range.is_none_or(|end| full.contains_span(end));
        if !covered {
            found.push(PayloadContradiction::FullRegionMissesAPart);
        }
    }

    found
}

/// How a payload's source region ends.
///
/// Shared by strings and heredocs: both answer the same question about their
/// own delimiters. Emptiness is a *terminated* outcome, so
/// [`PayloadTerminal::Empty`] implies termination just as
/// [`PayloadTerminal::Complete`] does; the two differ only in whether the
/// content region is provably zero-length.
///
/// When more than one variant could apply, the more specific failure wins:
/// a budget that stopped a scan is `Budgeted` even though the payload is also
/// unterminated, and `Empty` requires proven termination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PayloadTerminal {
    /// Opening and closing delimiters are both proven, with non-empty content.
    Complete,
    /// Delimiters are proven and the content region is provably zero-length.
    Empty,
    /// No closing delimiter was found before the end of input.
    Unterminated,
    /// Structure was reconstructed after a parse error and is not source-exact.
    Recovered,
    /// A scan or allocation budget stopped analysis before completion.
    Budgeted,
    /// No terminal facts are available for this payload.
    Unavailable,
}

impl PayloadTerminal {
    /// Whether the payload's closing delimiter is proven.
    #[must_use]
    pub const fn is_terminated(&self) -> bool {
        matches!(self, Self::Complete | Self::Empty)
    }

    /// Whether this terminal proves the content region is zero-length.
    ///
    /// Only [`PayloadTerminal::Empty`] does. An unterminated or recovered
    /// payload with no observed content proves nothing about its length.
    #[must_use]
    pub const fn proves_empty_content(&self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Whether analysis of this payload stopped short of its source.
    #[must_use]
    pub const fn is_incomplete(&self) -> bool {
        matches!(self, Self::Unterminated | Self::Recovered | Self::Budgeted | Self::Unavailable)
    }
}

/// The written form of an ordinary string or quote-like payload.
///
/// Forms are kept apart by spelling, not by behavior: `qx{...}` and
/// `` `...` `` are both execution boundaries but are not the same source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StringForm {
    /// `'literal'`
    SingleQuoted,
    /// `"interpolated"`
    DoubleQuoted,
    /// `q{literal}`
    QLiteral,
    /// `qq{interpolated}`
    QqInterpolating,
    /// `qx{command}`
    QxCommand,
    /// `` `command` ``
    Backtick,
}

impl StringForm {
    /// Whether this form interpolates expressions in its content.
    #[must_use]
    pub const fn interpolates(&self) -> bool {
        match self {
            Self::DoubleQuoted | Self::QqInterpolating | Self::QxCommand | Self::Backtick => true,
            Self::SingleQuoted | Self::QLiteral => false,
        }
    }

    /// Whether this form denotes command execution rather than a pure value.
    #[must_use]
    pub const fn is_execution_boundary(&self) -> bool {
        matches!(self, Self::QxCommand | Self::Backtick)
    }

    /// Whether this form can be written with the given delimiter.
    ///
    /// `'...'`, `"..."`, and `` `...` `` each fix their own delimiter
    /// character. Only the `q`-family forms choose one, and any of them may
    /// record [`StringDelimiter::Unavailable`] when the delimiter was not
    /// captured.
    #[must_use]
    pub const fn admits_delimiter(&self, delimiter: StringDelimiter) -> bool {
        let fixed = match self {
            Self::SingleQuoted => '\'',
            Self::DoubleQuoted => '"',
            Self::Backtick => '`',
            Self::QLiteral | Self::QqInterpolating | Self::QxCommand => return true,
        };
        match delimiter {
            StringDelimiter::Unavailable => true,
            StringDelimiter::Same { delimiter } => delimiter == fixed,
            StringDelimiter::Paired { .. } => false,
        }
    }

    /// Bytes of quote operator written before the opening delimiter.
    ///
    /// `q`, `qq`, and `qx` sit outside the delimiter pair and inside the
    /// payload's raw range, so content cannot begin until after them.
    #[must_use]
    pub const fn operator_len(&self) -> usize {
        match self {
            Self::SingleQuoted | Self::DoubleQuoted | Self::Backtick => 0,
            Self::QLiteral => 1,
            Self::QqInterpolating | Self::QxCommand => 2,
        }
    }
}

/// The delimiter pair a quote-like payload was written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StringDelimiter {
    /// One character both opens and closes: `'x'`, `"x"`, `q!x!`.
    Same {
        /// The delimiter character.
        delimiter: char,
    },
    /// A bracketing pair: `q{x}`, `q(x)`, `q[x]`, `q<x>`.
    Paired {
        /// Opening bracket.
        open: char,
        /// Closing bracket.
        close: char,
    },
    /// Delimiter identity was not recorded.
    Unavailable,
}

impl StringDelimiter {
    /// Whether the payload was written with a bracketing pair.
    #[must_use]
    pub const fn is_paired(&self) -> bool {
        matches!(self, Self::Paired { .. })
    }

    /// Bytes the opening delimiter occupies in source.
    ///
    /// `SourceLocation` counts bytes, so a non-ASCII delimiter is wider than
    /// one. `Unavailable` still reserves one byte: which character opened the
    /// payload is unknown, but that one was written is not.
    #[must_use]
    pub const fn opening_len(&self) -> usize {
        match self {
            Self::Same { delimiter } => delimiter.len_utf8(),
            Self::Paired { open, .. } => open.len_utf8(),
            Self::Unavailable => 1,
        }
    }

    /// Bytes the closing delimiter occupies in source.
    ///
    /// The counterpart of [`StringDelimiter::opening_len`]; a bracketing pair
    /// may close with a different character, and a different width.
    #[must_use]
    pub const fn closing_len(&self) -> usize {
        match self {
            Self::Same { delimiter } => delimiter.len_utf8(),
            Self::Paired { close, .. } => close.len_utf8(),
            Self::Unavailable => 1,
        }
    }
}

/// Source-backed payload of an ordinary string or quote-like construct.
///
/// See the module documentation for what each position keeps distinct.
#[derive(Debug, Clone, PartialEq)]
pub struct StringSyntax {
    /// Written form of the payload.
    pub form: StringForm,
    /// Delimiter pair the payload was written with.
    pub delimiter: StringDelimiter,
    /// Full source region including both delimiters.
    pub raw_range: SourceLocation,
    /// Source region between the delimiters.
    ///
    /// `None` when no content region is established, which is not the same as
    /// a zero-length region.
    pub content_range: Option<SourceLocation>,
    /// Ordered segmentation of the content region.
    pub segmentation: SourceSegmentation,
    /// Disposition of the payload's cooked value.
    pub cooked: CookedValue,
    /// How the payload's source region ends.
    pub terminal: PayloadTerminal,
}

impl StringSyntax {
    /// Every way this payload's fields contradict each other.
    ///
    /// Empty for a payload that describes real source. The checked accessors
    /// below decline when this is non-empty.
    #[must_use]
    pub fn contradictions(&self) -> Vec<PayloadContradiction> {
        let mut found = payload_contradictions(
            Some(self.raw_range),
            self.content_range,
            &self.segmentation,
            &self.cooked,
            self.terminal,
        );
        if !self.form.admits_delimiter(self.delimiter) {
            found.push(PayloadContradiction::FormDelimiterMismatch);
        }
        // `raw_range` spans the quote operator and both delimiters. Malformed
        // bounds are reported by the shared check above and compare
        // meaninglessly, so skip them here.
        let opening = self.form.operator_len() + self.delimiter.opening_len();
        let closing = self.delimiter.closing_len();
        if self.terminal.is_terminated()
            && !is_malformed(self.raw_range)
            && let Some(content) = self.content_range
            && !is_malformed(content)
            && (content.start < self.raw_range.start.saturating_add(opening)
                || content.end.saturating_add(closing) > self.raw_range.end)
        {
            found.push(PayloadContradiction::TerminatedStringWithoutDelimiterBytes);
        }
        found
    }

    /// Whether this payload's fields agree with each other.
    #[must_use]
    pub fn is_coherent(&self) -> bool {
        self.contradictions().is_empty()
    }

    /// The complete ordered segments tiling the content region.
    ///
    /// `Some` only when segmentation is [`SourceSegmentation::Exact`] *and* the
    /// payload is coherent, so the returned slice really is in source order,
    /// non-overlapping, gap-free, and inside the content region. `Some(&[])` is
    /// a proven-empty payload; `None` covers unsegmented, partial, and
    /// contradictory payloads alike — [`StringSyntax::contradictions`] says
    /// which.
    #[must_use]
    pub fn proven_segments(&self) -> Option<&[SourceSegment]> {
        match &self.segmentation {
            SourceSegmentation::Exact(segments) if self.is_coherent() => Some(segments.as_slice()),
            SourceSegmentation::Exact(_)
            | SourceSegmentation::Partial(_)
            | SourceSegmentation::Unavailable => None,
        }
    }

    /// The interpolated expressions of this payload, in recorded order.
    ///
    /// Recorded order is source order only for a coherent payload; check
    /// [`StringSyntax::is_coherent`] before relying on the ordering.
    pub fn interpolation_expressions(&self) -> impl Iterator<Item = &Node> + '_ {
        self.segmentation.interpolation_expressions()
    }

    /// The cooked text a consumer may present as this expression's value.
    ///
    /// `Some` only when the value is proven, the payload's source is proven
    /// terminated, the payload is coherent, **and** the form is not an
    /// execution boundary. A command payload's cooked text is the command's
    /// source, never its output, so it is never this expression's value.
    #[must_use]
    pub fn proven_literal_value(&self) -> Option<&str> {
        if self.form.is_execution_boundary() {
            return None;
        }
        self.compat_value()
    }

    /// Whether this payload denotes command execution.
    #[must_use]
    pub const fn is_execution_boundary(&self) -> bool {
        self.form.is_execution_boundary()
    }

    /// Whether the content region is established and provably zero-length.
    ///
    /// Requires both a proven terminal and a zero-length content range, so an
    /// unterminated payload with no observed content does not qualify.
    ///
    /// `false` also covers a producer contradiction — [`PayloadTerminal::Empty`]
    /// over a non-empty content region — so this predicate alone cannot tell
    /// "not proven empty" from "the fields disagree".
    /// [`StringSyntax::contradictions`] separates them, reporting
    /// [`PayloadContradiction::EmptyTerminalWithNonEmptyContent`] for the
    /// second.
    #[must_use]
    pub fn has_proven_empty_content(&self) -> bool {
        self.terminal.proves_empty_content()
            && self.content_range.is_some_and(|range| range.is_empty())
    }

    /// Derived projection of the legacy `NodeKind::String::value` field.
    ///
    /// Computed from [`StringSyntax::cooked`] on every call; nothing stores it.
    /// `None` where the legacy total field would have had to invent a value.
    ///
    /// A proven value is published only when the payload's source is proven
    /// terminated and the payload is coherent, so an unterminated, recovered,
    /// or budgeted payload cannot present a value that looks complete even
    /// though its fields are individually settable.
    ///
    /// This is the *legacy field's* meaning, which for a command form was the
    /// command's source text rather than its output. Use
    /// [`StringSyntax::proven_literal_value`] for the value a consumer may
    /// present as this expression's own value.
    #[must_use]
    pub fn compat_value(&self) -> Option<&str> {
        if !self.terminal.is_terminated() || !self.is_coherent() {
            return None;
        }
        self.cooked.proven_text()
    }

    /// Derived projection of the legacy `NodeKind::String::interpolated` field.
    ///
    /// Computed from [`StringSyntax::form`] on every call.
    #[must_use]
    pub const fn compat_interpolated(&self) -> bool {
        self.form.interpolates()
    }
}

/// The written form of a heredoc declaration's label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HeredocForm {
    /// `<<EOF` -- bare label, interpolating.
    Bare,
    /// `` <<\EOF `` -- backslash-quoted label, non-interpolating.
    ///
    /// A distinct spelling from `<<'EOF'` with the same interpolation
    /// behaviour. The lexer already recognises it
    /// (`perl-lexer`: "Optional backslash disables interpolation"), so
    /// collapsing it into [`HeredocForm::SingleQuoted`] would lose the source
    /// spelling this contract exists to keep.
    BackslashQuoted,
    /// `<<'EOF'` -- single-quoted label, non-interpolating.
    SingleQuoted,
    /// `<<"EOF"` -- double-quoted label, interpolating.
    DoubleQuoted,
    /// `` <<`EOF` `` -- backticked label, command execution.
    Backtick,
}

impl HeredocForm {
    /// Whether this form interpolates expressions in its body.
    #[must_use]
    pub const fn interpolates(&self) -> bool {
        match self {
            Self::Bare | Self::DoubleQuoted | Self::Backtick => true,
            Self::BackslashQuoted | Self::SingleQuoted => false,
        }
    }

    /// Whether this form denotes command execution rather than a pure value.
    #[must_use]
    pub const fn is_execution_boundary(&self) -> bool {
        matches!(self, Self::Backtick)
    }
}

/// Identity of a heredoc declaration as written at its introducer.
///
/// The declaration is where the heredoc is spelled; the body and terminator
/// live further down the source and are held by [`HeredocSyntax`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeredocDeclarationIdentity {
    /// Source region of the `<<` or `<<~` introducer through the label.
    pub opener_range: SourceLocation,
    /// Source region of the label exactly as spelled, including any quoting.
    pub label_range: SourceLocation,
    /// Label text with declaration quoting removed.
    pub label: String,
    /// Written form of the label.
    pub form: HeredocForm,
    /// Whether the declaration used the `<<~` indentation-stripping form.
    pub indented: bool,
}

/// Source-backed payload of a heredoc.
///
/// Ranges that may legitimately be absent are [`Option`], so an unattached body
/// (`None`) stays distinct from a proven-empty one (`Some` zero-length range).
#[derive(Debug, Clone, PartialEq)]
pub struct HeredocSyntax {
    /// Identity of the declaration at the introducer.
    pub declaration: HeredocDeclarationIdentity,
    /// Raw source region of the body, before any indentation stripping.
    ///
    /// `None` when no body is attached to the declaration.
    pub raw_body_range: Option<SourceLocation>,
    /// Source region of the terminator line.
    ///
    /// `None` when the heredoc is unterminated or its terminator is unknown.
    pub terminator_range: Option<SourceLocation>,
    /// Declaration, body, and terminator as one region.
    ///
    /// `None` when the region cannot be established.
    pub full_region: Option<SourceLocation>,
    /// Ordered segmentation of the body.
    pub segmentation: SourceSegmentation,
    /// Disposition of the body's cooked value.
    pub cooked: CookedValue,
    /// How the heredoc's source region ends.
    pub terminal: PayloadTerminal,
}

impl HeredocSyntax {
    /// Every way this payload's fields contradict each other.
    ///
    /// The body region is what the segments must tile, so `raw_body_range`
    /// plays the part a string's content range plays.
    #[must_use]
    pub fn contradictions(&self) -> Vec<PayloadContradiction> {
        let mut found = payload_contradictions(
            self.full_region,
            self.raw_body_range,
            &self.segmentation,
            &self.cooked,
            self.terminal,
        );
        found.extend(heredoc_geometry_contradictions(self));
        found
    }

    /// Whether this payload's fields agree with each other.
    #[must_use]
    pub fn is_coherent(&self) -> bool {
        self.contradictions().is_empty()
    }

    /// The complete ordered segments tiling the body region.
    ///
    /// Checked exactly as [`StringSyntax::proven_segments`] is.
    #[must_use]
    pub fn proven_segments(&self) -> Option<&[SourceSegment]> {
        match &self.segmentation {
            SourceSegmentation::Exact(segments) if self.is_coherent() => Some(segments.as_slice()),
            SourceSegmentation::Exact(_)
            | SourceSegmentation::Partial(_)
            | SourceSegmentation::Unavailable => None,
        }
    }

    /// The interpolated expressions of this body, in recorded order.
    ///
    /// Recorded order is source order only for a coherent payload; check
    /// [`HeredocSyntax::is_coherent`] before relying on the ordering.
    pub fn interpolation_expressions(&self) -> impl Iterator<Item = &Node> + '_ {
        self.segmentation.interpolation_expressions()
    }

    /// The cooked body a consumer may present as this heredoc's value.
    ///
    /// `Some` only when [`HeredocSyntax::compat_content`] is, and the form is
    /// not an execution boundary: a command heredoc's body is the command's
    /// source, never its output.
    #[must_use]
    pub fn proven_literal_value(&self) -> Option<&str> {
        if self.declaration.form.is_execution_boundary() {
            return None;
        }
        self.compat_content()
    }

    /// Whether this heredoc denotes command execution.
    #[must_use]
    pub const fn is_execution_boundary(&self) -> bool {
        self.declaration.form.is_execution_boundary()
    }

    /// Whether a body region is attached to this declaration at all.
    ///
    /// A proven-empty body is attached; an unresolved one is not.
    #[must_use]
    pub const fn has_attached_body(&self) -> bool {
        self.raw_body_range.is_some()
    }

    /// Whether the body is attached and provably zero-length.
    ///
    /// `false` also covers a producer contradiction, exactly as
    /// [`StringSyntax::has_proven_empty_content`] documents;
    /// [`HeredocSyntax::contradictions`] separates the two cases.
    #[must_use]
    pub fn has_proven_empty_body(&self) -> bool {
        self.terminal.proves_empty_content()
            && self.raw_body_range.is_some_and(|range| range.is_empty())
    }

    /// Derived projection of the legacy `NodeKind::Heredoc::content` field.
    ///
    /// Computed from [`HeredocSyntax::cooked`] on every call; nothing stores it.
    /// Gated exactly as [`StringSyntax::compat_value`] is, so an unterminated,
    /// budgeted, recovered, or contradictory heredoc cannot publish a body that
    /// looks complete. Use [`HeredocSyntax::proven_literal_value`] for the value
    /// a consumer may present as the heredoc's own value.
    #[must_use]
    pub fn compat_content(&self) -> Option<&str> {
        if !self.terminal.is_terminated() || !self.is_coherent() {
            return None;
        }
        self.cooked.proven_text()
    }

    /// Derived projection of the legacy `NodeKind::Heredoc::delimiter` field.
    #[must_use]
    pub fn compat_delimiter(&self) -> &str {
        self.declaration.label.as_str()
    }

    /// Derived projection of the legacy `NodeKind::Heredoc::interpolated` field.
    #[must_use]
    pub const fn compat_interpolated(&self) -> bool {
        self.declaration.form.interpolates()
    }

    /// Derived projection of the legacy `NodeKind::Heredoc::indented` field.
    #[must_use]
    pub const fn compat_indented(&self) -> bool {
        self.declaration.indented
    }

    /// Derived projection of the legacy `NodeKind::Heredoc::command` field.
    #[must_use]
    pub const fn compat_command(&self) -> bool {
        self.declaration.form.is_execution_boundary()
    }

    /// Derived projection of the legacy `NodeKind::Heredoc::body_span` field.
    #[must_use]
    pub const fn compat_body_span(&self) -> Option<SourceLocation> {
        self.raw_body_range
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKind;

    fn span(start: usize, end: usize) -> SourceLocation {
        SourceLocation { start, end }
    }

    fn variable_node(start: usize, end: usize) -> Node {
        Node::new(
            NodeKind::Variable { sigil: "$".to_string(), name: "name".to_string() },
            span(start, end),
        )
    }

    fn interpolation_segment(start: usize, end: usize) -> SourceSegment {
        SourceSegment {
            raw_range: span(start, end),
            cooked_fragment: CookedValue::Dynamic,
            payload: SourceSegmentPayload::Interpolation {
                expression: Box::new(variable_node(start, end)),
            },
        }
    }

    fn literal_segment(start: usize, end: usize, cooked: &str) -> SourceSegment {
        SourceSegment {
            raw_range: span(start, end),
            cooked_fragment: CookedValue::Proven(cooked.to_string()),
            payload: SourceSegmentPayload::Literal,
        }
    }

    fn escape_segment(start: usize, end: usize, cooked: &str) -> SourceSegment {
        SourceSegment {
            raw_range: span(start, end),
            cooked_fragment: CookedValue::Proven(cooked.to_string()),
            payload: SourceSegmentPayload::Escape,
        }
    }

    /// `"a\nb$name"` occupying bytes 0..10.
    fn interpolated_string() -> StringSyntax {
        StringSyntax {
            form: StringForm::DoubleQuoted,
            delimiter: StringDelimiter::Same { delimiter: '"' },
            raw_range: span(0, 11),
            content_range: Some(span(1, 10)),
            segmentation: SourceSegmentation::Exact(vec![
                literal_segment(1, 2, "a"),
                escape_segment(2, 4, "\n"),
                literal_segment(4, 5, "b"),
                interpolation_segment(5, 10),
            ]),
            cooked: CookedValue::Dynamic,
            terminal: PayloadTerminal::Complete,
        }
    }

    /// `"abc"` occupying bytes 0..5 -- proven throughout, no interpolation.
    fn plain_double_quoted_string() -> StringSyntax {
        StringSyntax {
            form: StringForm::DoubleQuoted,
            delimiter: StringDelimiter::Same { delimiter: '"' },
            raw_range: span(0, 5),
            content_range: Some(span(1, 4)),
            segmentation: SourceSegmentation::Exact(vec![literal_segment(1, 4, "abc")]),
            cooked: CookedValue::Proven("abc".to_string()),
            terminal: PayloadTerminal::Complete,
        }
    }

    fn empty_single_quoted_string() -> StringSyntax {
        StringSyntax {
            form: StringForm::SingleQuoted,
            delimiter: StringDelimiter::Same { delimiter: '\'' },
            raw_range: span(0, 2),
            content_range: Some(span(1, 1)),
            segmentation: SourceSegmentation::Exact(Vec::new()),
            cooked: CookedValue::Proven(String::new()),
            terminal: PayloadTerminal::Empty,
        }
    }

    fn heredoc(form: HeredocForm, terminal: PayloadTerminal) -> HeredocSyntax {
        HeredocSyntax {
            declaration: HeredocDeclarationIdentity {
                opener_range: span(0, 7),
                label_range: span(2, 7),
                label: "EOF".to_string(),
                form,
                indented: false,
            },
            raw_body_range: Some(span(8, 14)),
            terminator_range: Some(span(14, 17)),
            full_region: Some(span(0, 17)),
            segmentation: SourceSegmentation::Exact(vec![literal_segment(8, 14, "body\n")]),
            cooked: CookedValue::Proven("body\n".to_string()),
            terminal,
        }
    }

    // --- raw source is not the cooked value -------------------------------

    #[test]
    fn escape_segment_keeps_raw_geometry_apart_from_its_cooked_fragment() {
        let string = interpolated_string();
        let segments = string.segmentation.observed_segments();
        let escape = segments.get(1);

        // Two source bytes of `\n`, one cooked byte. A single cooked payload
        // field cannot hold both facts.
        assert_eq!(escape.map(|segment| segment.raw_range), Some(span(2, 4)));
        assert_eq!(escape.map(|segment| segment.raw_range.len()), Some(2));
        assert_eq!(escape.and_then(|segment| segment.cooked_fragment.proven_text()), Some("\n"));
        assert_eq!(
            escape.and_then(|segment| segment.cooked_fragment.proven_text()).map(str::len),
            Some(1)
        );
    }

    #[test]
    fn interpolated_string_has_no_proven_cooked_value() {
        let string = interpolated_string();
        assert_eq!(string.cooked, CookedValue::Dynamic);
        assert_eq!(string.compat_value(), None);
        assert!(string.cooked.is_dynamic());
    }

    // --- cooked dispositions do not leak a plausible value ----------------

    #[test]
    fn only_a_proven_cooked_value_yields_presentable_text() {
        assert_eq!(CookedValue::Proven("v".to_string()).proven_text(), Some("v"));
        assert_eq!(CookedValue::Partial("v".to_string()).proven_text(), None);
        assert_eq!(CookedValue::Dynamic.proven_text(), None);
        assert_eq!(CookedValue::Unavailable.proven_text(), None);
    }

    #[test]
    fn partial_text_is_reachable_but_never_as_a_proven_value() {
        let partial = CookedValue::Partial("head".to_string());
        assert_eq!(partial.partial_text(), Some("head"));
        assert_eq!(partial.proven_text(), None);
        assert!(!partial.is_proven());

        assert_eq!(CookedValue::Proven("head".to_string()).partial_text(), None);
    }

    #[test]
    fn a_provably_empty_cooked_value_is_not_an_unavailable_one() {
        let empty = CookedValue::Proven(String::new());
        assert_eq!(empty.proven_text(), Some(""));
        assert_eq!(CookedValue::Unavailable.proven_text(), None);
        assert_ne!(empty, CookedValue::Unavailable);
    }

    // --- empty is not unavailable -----------------------------------------

    #[test]
    fn proven_empty_segmentation_is_distinct_from_unsegmented() {
        let mut proven_empty = empty_single_quoted_string();
        proven_empty.segmentation = SourceSegmentation::Exact(Vec::new());
        let mut unsegmented = empty_single_quoted_string();
        unsegmented.segmentation = SourceSegmentation::Unavailable;

        assert!(proven_empty.proven_segments().is_some_and(<[SourceSegment]>::is_empty));
        assert!(unsegmented.proven_segments().is_none());

        // Both observe zero segments; only the disposition tells them apart.
        assert!(proven_empty.segmentation.observed_segments().is_empty());
        assert!(unsegmented.segmentation.observed_segments().is_empty());
        assert!(unsegmented.segmentation.is_unavailable());
        assert_ne!(proven_empty.segmentation, unsegmented.segmentation);
    }

    #[test]
    fn partial_segmentation_reports_its_prefix_but_proves_nothing() {
        let mut partial = interpolated_string();
        partial.segmentation = SourceSegmentation::Partial(vec![literal_segment(1, 2, "a")]);
        assert_eq!(partial.segmentation.observed_segments().len(), 1);
        assert!(partial.proven_segments().is_none());
        assert!(!partial.segmentation.is_exact());
        // A partial prefix is not a contradiction; it simply proves less.
        assert!(partial.is_coherent(), "{:?}", partial.contradictions());
    }

    #[test]
    fn an_empty_string_proves_emptiness_where_a_recovered_one_does_not() {
        let empty = empty_single_quoted_string();
        assert!(empty.has_proven_empty_content());
        assert_eq!(empty.compat_value(), Some(""));

        let mut recovered = empty.clone();
        recovered.terminal = PayloadTerminal::Recovered;
        recovered.cooked = CookedValue::Unavailable;
        assert!(!recovered.has_proven_empty_content());
        assert_eq!(recovered.compat_value(), None);
    }

    #[test]
    fn an_unattached_heredoc_body_is_not_a_proven_empty_body() {
        let mut attached_empty = heredoc(HeredocForm::Bare, PayloadTerminal::Empty);
        attached_empty.raw_body_range = Some(span(8, 8));
        attached_empty.segmentation = SourceSegmentation::Exact(Vec::new());
        attached_empty.cooked = CookedValue::Proven(String::new());

        assert!(attached_empty.has_attached_body());
        assert!(attached_empty.has_proven_empty_body());
        assert_eq!(attached_empty.compat_content(), Some(""));

        let mut unattached = attached_empty.clone();
        unattached.raw_body_range = None;
        unattached.terminal = PayloadTerminal::Unavailable;
        unattached.segmentation = SourceSegmentation::Unavailable;
        unattached.cooked = CookedValue::Unavailable;

        assert!(!unattached.has_attached_body());
        assert!(!unattached.has_proven_empty_body());
        assert_eq!(unattached.compat_content(), None);
        assert_eq!(unattached.compat_body_span(), None);
    }

    // --- incomplete payloads cannot look complete -------------------------

    #[test]
    fn every_heredoc_terminal_state_is_mutually_distinct() {
        let states = [
            PayloadTerminal::Complete,
            PayloadTerminal::Empty,
            PayloadTerminal::Unterminated,
            PayloadTerminal::Recovered,
            PayloadTerminal::Budgeted,
            PayloadTerminal::Unavailable,
        ];

        for (i, left) in states.iter().enumerate() {
            for (j, right) in states.iter().enumerate() {
                assert_eq!(i == j, left == right, "{left:?} vs {right:?}");
            }
        }
    }

    #[test]
    fn only_complete_and_empty_terminals_prove_termination() {
        assert!(PayloadTerminal::Complete.is_terminated());
        assert!(PayloadTerminal::Empty.is_terminated());
        assert!(!PayloadTerminal::Unterminated.is_terminated());
        assert!(!PayloadTerminal::Recovered.is_terminated());
        assert!(!PayloadTerminal::Budgeted.is_terminated());
        assert!(!PayloadTerminal::Unavailable.is_terminated());
    }

    #[test]
    fn only_the_empty_terminal_proves_zero_length_content() {
        assert!(PayloadTerminal::Empty.proves_empty_content());
        for terminal in [
            PayloadTerminal::Complete,
            PayloadTerminal::Unterminated,
            PayloadTerminal::Recovered,
            PayloadTerminal::Budgeted,
            PayloadTerminal::Unavailable,
        ] {
            assert!(!terminal.proves_empty_content(), "{terminal:?}");
        }
    }

    #[test]
    fn incomplete_terminals_never_report_themselves_terminated() {
        for terminal in [
            PayloadTerminal::Unterminated,
            PayloadTerminal::Recovered,
            PayloadTerminal::Budgeted,
            PayloadTerminal::Unavailable,
        ] {
            assert!(terminal.is_incomplete(), "{terminal:?}");
            assert!(!terminal.is_terminated(), "{terminal:?}");
        }
        assert!(!PayloadTerminal::Complete.is_incomplete());
        assert!(!PayloadTerminal::Empty.is_incomplete());
    }

    #[test]
    fn a_budgeted_payload_keeps_its_recovered_prefix_without_claiming_completeness() {
        let mut budgeted = interpolated_string();
        budgeted.terminal = PayloadTerminal::Budgeted;
        budgeted.segmentation = SourceSegmentation::Partial(vec![literal_segment(1, 2, "a")]);
        budgeted.cooked = CookedValue::Partial("a".to_string());

        assert_eq!(budgeted.segmentation.observed_segments().len(), 1);
        assert!(budgeted.proven_segments().is_none());
        assert_eq!(budgeted.compat_value(), None);
        assert!(budgeted.terminal.is_incomplete());
    }

    // --- interpolation children and their exact ranges ---------------------

    #[test]
    fn interpolation_carries_an_ordinary_ast_child_at_its_own_range() {
        let string = interpolated_string();
        let expressions: Vec<&Node> = string.interpolation_expressions().collect();
        assert_eq!(expressions.len(), 1);

        let expression = expressions.first().copied();
        assert!(expression.is_some_and(|node| matches!(node.kind, NodeKind::Variable { .. })));

        // The child's range is its own, strictly inside the payload's range.
        assert_eq!(expression.map(|node| node.location), Some(span(5, 10)));
        assert_ne!(expression.map(|node| node.location), Some(string.raw_range));
        assert!(
            expression.is_some_and(|node| string.raw_range.contains_span(node.location)),
            "the payload range must contain the interpolation range"
        );
    }

    #[test]
    fn interpolation_traversal_preserves_source_order() {
        let mut string = interpolated_string();
        string.raw_range = span(0, 13);
        string.content_range = Some(span(1, 12));
        string.segmentation = SourceSegmentation::Exact(vec![
            interpolation_segment(1, 6),
            literal_segment(6, 7, "-"),
            interpolation_segment(7, 12),
        ]);
        assert!(string.is_coherent(), "{:?}", string.contradictions());

        let ranges: Vec<SourceLocation> =
            string.interpolation_expressions().map(|node| node.location).collect();
        assert_eq!(ranges, vec![span(1, 6), span(7, 12)]);
    }

    #[test]
    fn literal_text_matching_an_interpolation_is_not_an_interpolation() {
        // Source `'$name'` -- the same characters, but a literal run.
        let literal_only = StringSyntax {
            form: StringForm::SingleQuoted,
            delimiter: StringDelimiter::Same { delimiter: '\'' },
            raw_range: span(0, 7),
            content_range: Some(span(1, 6)),
            segmentation: SourceSegmentation::Exact(vec![literal_segment(1, 6, "$name")]),
            cooked: CookedValue::Proven("$name".to_string()),
            terminal: PayloadTerminal::Complete,
        };

        assert_eq!(literal_only.interpolation_expressions().count(), 0);
        assert_eq!(literal_only.compat_value(), Some("$name"));
        assert!(!literal_only.compat_interpolated());

        // The interpolating payload with the same visible text does have one.
        assert_eq!(interpolated_string().interpolation_expressions().count(), 1);
    }

    #[test]
    fn recovery_segments_are_not_mistaken_for_classified_source() {
        let recovery = SourceSegment {
            raw_range: span(5, 10),
            cooked_fragment: CookedValue::Unavailable,
            payload: SourceSegmentPayload::Recovery {
                cause: SegmentRecoveryCause::UnterminatedInterpolation,
            },
        };

        assert!(recovery.is_recovery());
        assert!(recovery.interpolation_expression().is_none());
        assert_eq!(recovery.cooked_fragment.proven_text(), None);
        assert!(!literal_segment(0, 1, "a").is_recovery());
    }

    // --- form, delimiter, and execution boundaries ------------------------

    #[test]
    fn quote_forms_are_not_conflated() {
        let forms = [
            StringForm::SingleQuoted,
            StringForm::DoubleQuoted,
            StringForm::QLiteral,
            StringForm::QqInterpolating,
            StringForm::QxCommand,
            StringForm::Backtick,
        ];
        for (i, left) in forms.iter().enumerate() {
            for (j, right) in forms.iter().enumerate() {
                assert_eq!(i == j, left == right, "{left:?} vs {right:?}");
            }
        }
    }

    #[test]
    fn only_command_forms_are_execution_boundaries() {
        assert!(StringForm::QxCommand.is_execution_boundary());
        assert!(StringForm::Backtick.is_execution_boundary());
        assert!(!StringForm::QqInterpolating.is_execution_boundary());
        assert!(!StringForm::DoubleQuoted.is_execution_boundary());
        assert!(!StringForm::QLiteral.is_execution_boundary());
        assert!(!StringForm::SingleQuoted.is_execution_boundary());

        assert!(HeredocForm::Backtick.is_execution_boundary());
        assert!(!HeredocForm::Bare.is_execution_boundary());
        assert!(!HeredocForm::DoubleQuoted.is_execution_boundary());
        assert!(!HeredocForm::SingleQuoted.is_execution_boundary());
    }

    #[test]
    fn a_command_payload_is_an_execution_boundary_not_a_pure_literal() {
        let mut command = interpolated_string();
        command.form = StringForm::QxCommand;
        command.delimiter = StringDelimiter::Paired { open: '{', close: '}' };
        command.cooked = CookedValue::Dynamic;

        assert!(command.is_execution_boundary());
        assert_eq!(command.compat_value(), None);
        assert!(command.delimiter.is_paired());

        let heredoc_command = heredoc(HeredocForm::Backtick, PayloadTerminal::Complete);
        assert!(heredoc_command.is_execution_boundary());
        assert!(heredoc_command.compat_command());
    }

    #[test]
    fn interpolating_forms_are_exactly_the_non_literal_ones() {
        assert!(StringForm::DoubleQuoted.interpolates());
        assert!(StringForm::QqInterpolating.interpolates());
        assert!(StringForm::QxCommand.interpolates());
        assert!(StringForm::Backtick.interpolates());
        assert!(!StringForm::SingleQuoted.interpolates());
        assert!(!StringForm::QLiteral.interpolates());

        assert!(HeredocForm::Bare.interpolates());
        assert!(HeredocForm::DoubleQuoted.interpolates());
        assert!(HeredocForm::Backtick.interpolates());
        assert!(!HeredocForm::SingleQuoted.interpolates());
    }

    #[test]
    fn delimiter_identity_survives_as_written() {
        assert_eq!(
            StringDelimiter::Paired { open: '{', close: '}' },
            StringDelimiter::Paired { open: '{', close: '}' }
        );
        assert_ne!(
            StringDelimiter::Paired { open: '{', close: '}' },
            StringDelimiter::Paired { open: '(', close: ')' }
        );
        assert_ne!(
            StringDelimiter::Same { delimiter: '/' },
            StringDelimiter::Paired { open: '/', close: '/' }
        );
        assert!(!StringDelimiter::Unavailable.is_paired());
    }

    // --- heredoc declaration geometry --------------------------------------

    #[test]
    fn heredoc_declaration_body_and_terminator_are_separate_regions() {
        let doc = heredoc(HeredocForm::Bare, PayloadTerminal::Complete);

        assert_eq!(doc.declaration.opener_range, span(0, 7));
        assert_eq!(doc.declaration.label_range, span(2, 7));
        assert_eq!(doc.raw_body_range, Some(span(8, 14)));
        assert_eq!(doc.terminator_range, Some(span(14, 17)));
        assert_eq!(doc.full_region, Some(span(0, 17)));

        // The declaration does not overlap the body it introduces.
        assert!(doc.declaration.opener_range.end <= span(8, 14).start);
        assert!(
            doc.full_region.is_some_and(|full| full.contains_span(doc.declaration.opener_range)),
            "the full region must contain the declaration it opens with"
        );
    }

    #[test]
    fn an_unterminated_heredoc_has_no_terminator_region() {
        let mut unterminated = heredoc(HeredocForm::Bare, PayloadTerminal::Unterminated);
        unterminated.terminator_range = None;
        unterminated.cooked = CookedValue::Partial("body\n".to_string());

        assert_eq!(unterminated.terminator_range, None);
        assert!(!unterminated.terminal.is_terminated());
        assert_eq!(unterminated.compat_content(), None);
        // The body it did see is still attached and still addressable.
        assert!(unterminated.has_attached_body());
    }

    #[test]
    fn indentation_mode_is_declaration_identity_not_body_content() {
        let mut indented = heredoc(HeredocForm::Bare, PayloadTerminal::Complete);
        indented.declaration.indented = true;

        assert!(indented.compat_indented());
        assert!(!heredoc(HeredocForm::Bare, PayloadTerminal::Complete).compat_indented());
        // Stripping mode does not disturb the raw body geometry.
        assert_eq!(indented.raw_body_range, Some(span(8, 14)));
    }

    // --- compatibility projections are derived, never stored ---------------

    #[test]
    fn string_compat_value_tracks_the_typed_state_it_projects() {
        // A proven-empty payload projects an empty value, not "no value".
        assert_eq!(empty_single_quoted_string().compat_value(), Some(""));

        // The value is varied on a payload whose terminal permits a non-empty
        // one. An `Empty` terminal would not: it proves the source holds
        // nothing, so `a_proven_empty_payload_may_not_publish_a_nonempty_value`
        // refuses that combination outright.
        let mut string = plain_double_quoted_string();
        assert_eq!(string.compat_value(), Some("abc"));

        // Segmentation is withdrawn alongside the value: an exact segmentation
        // still claiming the old fragments would contradict the new value, and
        // `a_proven_value_the_segments_do_not_assemble_is_refused` covers that.
        string.segmentation = SourceSegmentation::Unavailable;
        string.cooked = CookedValue::Proven("changed".to_string());
        assert_eq!(string.compat_value(), Some("changed"));

        string.cooked = CookedValue::Unavailable;
        assert_eq!(string.compat_value(), None);
    }

    #[test]
    fn string_compat_interpolated_tracks_the_form_it_projects() {
        let mut string = empty_single_quoted_string();
        assert!(!string.compat_interpolated());

        string.form = StringForm::QqInterpolating;
        assert!(string.compat_interpolated());
    }

    #[test]
    fn heredoc_compat_projections_track_the_typed_state_they_project() {
        let mut doc = heredoc(HeredocForm::SingleQuoted, PayloadTerminal::Complete);
        assert_eq!(doc.compat_content(), Some("body\n"));
        assert_eq!(doc.compat_delimiter(), "EOF");
        assert!(!doc.compat_interpolated());
        assert!(!doc.compat_command());
        assert_eq!(doc.compat_body_span(), Some(span(8, 14)));

        doc.declaration.form = HeredocForm::Backtick;
        doc.declaration.label = "CMD".to_string();
        doc.cooked = CookedValue::Dynamic;
        doc.raw_body_range = None;
        doc.segmentation = SourceSegmentation::Unavailable;

        assert_eq!(doc.compat_content(), None);
        assert_eq!(doc.compat_delimiter(), "CMD");
        assert!(doc.compat_interpolated());
        assert!(doc.compat_command());
        assert_eq!(doc.compat_body_span(), None);
    }

    #[test]
    fn compat_projections_are_deterministic_for_unchanged_state() {
        let string = interpolated_string();
        assert_eq!(string.compat_value(), string.compat_value());
        assert_eq!(string.compat_interpolated(), string.compat_interpolated());

        let doc = heredoc(HeredocForm::Bare, PayloadTerminal::Complete);
        assert_eq!(doc.compat_content(), doc.compat_content());
        assert_eq!(doc.compat_body_span(), doc.compat_body_span());
    }

    // --- the payload does not become a second AST child channel ------------

    #[test]
    fn segments_carry_ordinary_nodes_that_compare_and_clone_structurally() {
        let string = interpolated_string();
        let copied = string.clone();
        assert_eq!(string, copied);

        let mut different = string.clone();
        different.segmentation = SourceSegmentation::Exact(vec![interpolation_segment(1, 10)]);
        assert_ne!(string, different);
    }

    // --- stripped `<<~` indentation is honest, classified source -----------

    fn normalization_segment(start: usize, end: usize) -> SourceSegment {
        SourceSegment {
            raw_range: span(start, end),
            cooked_fragment: CookedValue::Proven(String::new()),
            payload: SourceSegmentPayload::Normalization {
                rule: NormalizationRule::HeredocIndentStrip,
            },
        }
    }

    /// `<<~EOF` with a body line of `    text\n`: four stripped indent bytes
    /// followed by the text that survives into the cooked body.
    fn indented_heredoc() -> HeredocSyntax {
        let mut doc = heredoc(HeredocForm::Bare, PayloadTerminal::Complete);
        doc.declaration.indented = true;
        doc.raw_body_range = Some(span(8, 18));
        doc.terminator_range = Some(span(18, 21));
        doc.full_region = Some(span(0, 21));
        doc.segmentation = SourceSegmentation::Exact(vec![
            normalization_segment(8, 12),
            literal_segment(12, 18, "text\n"),
        ]);
        doc.cooked = CookedValue::Proven("text\n".to_string());
        doc
    }

    #[test]
    fn stripped_heredoc_indentation_is_covered_without_being_called_literal() {
        let doc = indented_heredoc();
        assert!(doc.is_coherent(), "{:?}", doc.contradictions());

        // The stripped run is inside the exact segmentation, so the body's
        // source is covered end to end rather than silently dropped.
        let segments = doc.proven_segments().unwrap_or(&[]);
        assert_eq!(segments.len(), 2, "an exact coherent segmentation must be proven");
        let stripped = segments.first();
        assert_eq!(stripped.map(|segment| segment.raw_range), Some(span(8, 12)));

        // It is normalization, not a literal, not an escape, not recovery.
        assert!(stripped.is_some_and(|segment| matches!(
            segment.payload,
            SourceSegmentPayload::Normalization { rule: NormalizationRule::HeredocIndentStrip }
        )));
        assert!(stripped.is_some_and(|segment| !segment.is_recovery()));
        assert!(stripped.is_some_and(|segment| segment.interpolation_expression().is_none()));

        // Four raw bytes contributing nothing to the cooked body.
        assert_eq!(stripped.map(|segment| segment.raw_range.len()), Some(4));
        assert_eq!(stripped.and_then(|segment| segment.cooked_fragment.proven_text()), Some(""));
        assert_eq!(doc.compat_content(), Some("text\n"));
    }

    #[test]
    fn dropping_the_stripped_indent_run_leaves_the_body_uncovered() {
        // The shape this contract previously forced: omit the stripped bytes.
        let mut doc = indented_heredoc();
        doc.segmentation = SourceSegmentation::Exact(vec![literal_segment(12, 18, "text\n")]);

        assert!(!doc.is_coherent());
        assert!(doc.contradictions().contains(&PayloadContradiction::SegmentGap { index: 0 }));
        assert!(doc.proven_segments().is_none());
    }

    // --- segment geometry is checked, not merely named ---------------------

    #[test]
    fn a_coherent_payload_reports_no_contradictions() {
        for payload in [interpolated_string(), empty_single_quoted_string()] {
            assert!(payload.is_coherent(), "{:?}", payload.contradictions());
            assert!(payload.proven_segments().is_some());
        }
        let doc = heredoc(HeredocForm::Bare, PayloadTerminal::Complete);
        assert!(doc.is_coherent(), "{:?}", doc.contradictions());
    }

    #[test]
    fn segments_recorded_out_of_source_order_are_not_proven() {
        let mut string = interpolated_string();
        string.segmentation = SourceSegmentation::Exact(vec![
            literal_segment(5, 10, "later"),
            literal_segment(1, 5, "first"),
        ]);

        assert!(
            string
                .contradictions()
                .contains(&PayloadContradiction::SegmentsOutOfOrder { index: 1 })
        );
        assert!(string.proven_segments().is_none());
        // The unchecked view still reports what was recorded, and says so.
        assert_eq!(string.segmentation.observed_segments().len(), 2);
    }

    #[test]
    fn overlapping_segments_are_not_proven() {
        let mut string = interpolated_string();
        string.segmentation = SourceSegmentation::Exact(vec![
            literal_segment(1, 6, "abcde"),
            literal_segment(4, 10, "overlap"),
        ]);

        assert!(
            string.contradictions().contains(&PayloadContradiction::SegmentsOverlap { index: 1 })
        );
        assert!(string.proven_segments().is_none());
    }

    #[test]
    fn an_exact_segmentation_with_a_hole_is_not_proven() {
        let mut string = interpolated_string();
        string.segmentation = SourceSegmentation::Exact(vec![
            literal_segment(1, 3, "ab"),
            literal_segment(5, 10, "ghijk"),
        ]);

        assert!(string.contradictions().contains(&PayloadContradiction::SegmentGap { index: 1 }));
        assert!(string.proven_segments().is_none());
    }

    #[test]
    fn a_segment_outside_the_content_region_is_not_proven() {
        let mut string = interpolated_string();
        string.segmentation = SourceSegmentation::Exact(vec![literal_segment(1, 40, "runaway")]);

        assert!(
            string
                .contradictions()
                .contains(&PayloadContradiction::SegmentOutsideContent { index: 0 })
        );
        assert!(string.proven_segments().is_none());
    }

    #[test]
    fn an_exact_segmentation_that_stops_short_is_not_proven() {
        let mut string = interpolated_string();
        string.segmentation = SourceSegmentation::Exact(vec![literal_segment(1, 5, "abcd")]);

        assert!(
            string
                .contradictions()
                .contains(&PayloadContradiction::ExactSegmentationLeavesContentUncovered)
        );
        assert!(string.proven_segments().is_none());
    }

    #[test]
    fn segments_without_a_content_region_are_a_contradiction() {
        let mut string = interpolated_string();
        string.content_range = None;

        assert!(
            string.contradictions().contains(&PayloadContradiction::SegmentsWithoutContentRange)
        );
        assert!(string.proven_segments().is_none());
    }

    #[test]
    fn a_content_region_outside_the_raw_region_is_a_contradiction() {
        let mut string = interpolated_string();
        string.raw_range = span(0, 4);

        assert!(string.contradictions().contains(&PayloadContradiction::ContentRangeOutsideRaw));
    }

    // --- a contradictory payload cannot publish a complete-looking value ---

    #[test]
    fn a_proven_value_on_an_unterminated_payload_is_refused() {
        let mut truncated = interpolated_string();
        truncated.terminal = PayloadTerminal::Unterminated;
        truncated.cooked = CookedValue::Proven("looks complete".to_string());

        assert!(
            truncated
                .contradictions()
                .contains(&PayloadContradiction::ProvenValueOnUnterminatedPayload)
        );
        // The projection refuses rather than publishing a truncated payload's
        // value as though the payload had finished.
        assert_eq!(truncated.compat_value(), None);
        assert_eq!(truncated.proven_literal_value(), None);
        // The raw disposition is still inspectable; only publication is gated.
        assert_eq!(truncated.cooked.proven_text(), Some("looks complete"));
    }

    #[test]
    fn a_proven_body_on_an_unterminated_heredoc_is_refused() {
        let mut doc = heredoc(HeredocForm::Bare, PayloadTerminal::Unterminated);
        assert!(
            doc.contradictions().contains(&PayloadContradiction::ProvenValueOnUnterminatedPayload)
        );
        assert_eq!(doc.compat_content(), None);

        for terminal in
            [PayloadTerminal::Recovered, PayloadTerminal::Budgeted, PayloadTerminal::Unavailable]
        {
            doc.terminal = terminal;
            assert_eq!(doc.compat_content(), None, "{terminal:?}");
        }
    }

    #[test]
    fn an_empty_terminal_over_non_empty_content_is_a_contradiction() {
        let mut string = interpolated_string();
        string.terminal = PayloadTerminal::Empty;
        string.cooked = CookedValue::Proven(String::new());

        assert!(
            string
                .contradictions()
                .contains(&PayloadContradiction::EmptyTerminalWithNonEmptyContent)
        );
        assert_eq!(string.compat_value(), None);
        assert!(!string.has_proven_empty_content());
    }

    #[test]
    fn a_contradictory_payload_publishes_no_projection_even_when_terminated() {
        let mut string = plain_double_quoted_string();
        assert_eq!(string.compat_value(), Some("abc"));

        // Same terminal, same cooked value, only the geometry is broken.
        string.segmentation = SourceSegmentation::Exact(vec![literal_segment(1, 40, "runaway")]);
        assert_eq!(string.compat_value(), None);
    }

    #[test]
    fn a_false_emptiness_answer_is_separable_from_a_contradictory_one() {
        // Honestly not proven empty: nothing disagrees, the payload just is
        // not an empty one.
        let not_empty = interpolated_string();
        assert!(!not_empty.has_proven_empty_content());
        assert!(not_empty.is_coherent(), "{:?}", not_empty.contradictions());

        // Also `false`, but because the producer's fields disagree.
        let mut contradictory = interpolated_string();
        contradictory.terminal = PayloadTerminal::Empty;
        assert!(!contradictory.has_proven_empty_content());
        assert!(
            contradictory
                .contradictions()
                .contains(&PayloadContradiction::EmptyTerminalWithNonEmptyContent)
        );

        // The heredoc predicate separates the same two cases.
        let plain = heredoc(HeredocForm::Bare, PayloadTerminal::Complete);
        assert!(!plain.has_proven_empty_body());
        assert!(plain.is_coherent(), "{:?}", plain.contradictions());

        let mut broken = heredoc(HeredocForm::Bare, PayloadTerminal::Empty);
        assert!(!broken.has_proven_empty_body());
        assert!(!broken.is_coherent());
        broken.raw_body_range = Some(span(8, 8));
        broken.segmentation = SourceSegmentation::Exact(Vec::new());
        broken.cooked = CookedValue::Proven(String::new());
        assert!(broken.has_proven_empty_body());
        assert!(broken.is_coherent(), "{:?}", broken.contradictions());
    }

    // --- a command's text is its source, never its value -------------------

    #[test]
    fn a_terminated_command_publishes_its_source_but_never_a_literal_value() {
        // A command with no interpolation: its source text really is proven,
        // which is what makes the execution-boundary refusal the only reason
        // `proven_literal_value` declines.
        let mut command = plain_double_quoted_string();
        command.form = StringForm::QxCommand;
        command.delimiter = StringDelimiter::Paired { open: '{', close: '}' };
        // `qx{ls -l}` is nine bytes: the closing brace is inside `raw_range`.
        command.raw_range = span(0, 9);
        command.content_range = Some(span(3, 8));
        command.segmentation = SourceSegmentation::Exact(vec![literal_segment(3, 8, "ls -l")]);
        command.cooked = CookedValue::Proven("ls -l".to_string());
        assert!(command.is_coherent(), "{:?}", command.contradictions());

        // The legacy field held the command's source text, so the projection
        // faithfully reports it.
        assert_eq!(command.compat_value(), Some("ls -l"));
        // It is still not a value a consumer may present as the expression's.
        assert_eq!(command.proven_literal_value(), None);
        assert!(command.is_execution_boundary());

        // A non-command payload with the same state does publish one.
        let mut literal = command.clone();
        literal.form = StringForm::QqInterpolating;
        assert_eq!(literal.proven_literal_value(), Some("ls -l"));
    }

    #[test]
    fn a_command_heredoc_publishes_its_body_but_never_a_literal_value() {
        let doc = heredoc(HeredocForm::Backtick, PayloadTerminal::Complete);
        assert_eq!(doc.compat_content(), Some("body\n"));
        assert_eq!(doc.proven_literal_value(), None);

        let plain = heredoc(HeredocForm::Bare, PayloadTerminal::Complete);
        assert_eq!(plain.proven_literal_value(), Some("body\n"));
    }

    // --- checks added after the second review round -----------------------

    #[test]
    fn an_exact_segmentation_without_a_content_region_is_not_proven() {
        // `Exact(vec![])` over no geometry would otherwise read as
        // "proven to have no segments" for a payload that established nothing.
        let mut string = empty_single_quoted_string();
        string.content_range = None;
        string.segmentation = SourceSegmentation::Exact(Vec::new());

        assert!(
            string
                .contradictions()
                .contains(&PayloadContradiction::ExactSegmentationWithoutContentRange)
        );
        assert!(string.proven_segments().is_none());

        let mut doc = heredoc(HeredocForm::Bare, PayloadTerminal::Complete);
        doc.raw_body_range = None;
        doc.segmentation = SourceSegmentation::Exact(Vec::new());
        assert!(
            doc.contradictions()
                .contains(&PayloadContradiction::ExactSegmentationWithoutContentRange)
        );
        assert!(doc.proven_segments().is_none());

        // `Unavailable` stays the honest representation of absent geometry.
        doc.segmentation = SourceSegmentation::Unavailable;
        assert!(
            !doc.contradictions()
                .contains(&PayloadContradiction::ExactSegmentationWithoutContentRange)
        );
    }

    #[test]
    fn an_interpolation_expression_off_its_own_segment_range_is_refused() {
        let mut string = interpolated_string();
        // The expression claims the whole payload rather than its own run —
        // #8246's "assigns an interpolation expression the enclosing string
        // range" control, which the type system alone does not prevent.
        string.segmentation = SourceSegmentation::Exact(vec![
            literal_segment(1, 5, "a\nb"),
            SourceSegment {
                raw_range: span(5, 10),
                cooked_fragment: CookedValue::Dynamic,
                payload: SourceSegmentPayload::Interpolation {
                    expression: Box::new(variable_node(0, 11)),
                },
            },
        ]);

        assert!(
            string
                .contradictions()
                .contains(&PayloadContradiction::InterpolationRangeMismatch { index: 1 })
        );
        assert!(string.proven_segments().is_none());

        // The same payload with the expression at its own range is coherent.
        assert!(interpolated_string().is_coherent());
    }

    #[test]
    fn a_proven_value_the_segments_do_not_assemble_is_refused() {
        let mut string = empty_single_quoted_string();
        string.cooked = CookedValue::Proven("changed".to_string());

        assert!(
            string
                .contradictions()
                .contains(&PayloadContradiction::CookedValueDisagreesWithSegments)
        );
        assert_eq!(string.compat_value(), None);

        // Agreement is the coherent case.
        let doc = indented_heredoc();
        assert!(doc.is_coherent(), "{:?}", doc.contradictions());
        assert_eq!(doc.compat_content(), Some("text\n"));

        // A fragment that is not proven withdraws the comparison rather than
        // failing it: an interpolated payload cannot be assembled at all.
        let mut interpolated = interpolated_string();
        interpolated.cooked = CookedValue::Proven("anything".to_string());
        assert!(
            !interpolated
                .contradictions()
                .contains(&PayloadContradiction::CookedValueDisagreesWithSegments)
        );
        // It is still rejected, by the rule that actually applies: a runtime
        // segment means the payload value was never statically proven.
        assert!(
            interpolated
                .contradictions()
                .contains(&PayloadContradiction::ProvenValueOverDynamicSegments)
        );
        assert_eq!(interpolated.compat_value(), None);
    }

    #[test]
    fn a_terminated_heredoc_without_a_terminator_region_is_refused() {
        let mut doc = heredoc(HeredocForm::Bare, PayloadTerminal::Complete);
        doc.terminator_range = None;

        assert!(
            doc.contradictions()
                .contains(&PayloadContradiction::TerminatedHeredocWithoutTerminator)
        );
        assert_eq!(doc.compat_content(), None);

        // And the opposite direction: unterminated with a terminator.
        let mut contradictory = heredoc(HeredocForm::Bare, PayloadTerminal::Unterminated);
        contradictory.cooked = CookedValue::Partial("body\n".to_string());
        assert!(
            contradictory
                .contradictions()
                .contains(&PayloadContradiction::UnterminatedHeredocWithTerminator)
        );
    }

    #[test]
    fn heredoc_regions_recorded_out_of_order_are_refused() {
        let mut doc = heredoc(HeredocForm::Bare, PayloadTerminal::Complete);
        // Body starts before the declaration ends.
        doc.raw_body_range = Some(span(3, 14));
        assert!(doc.contradictions().contains(&PayloadContradiction::HeredocRegionsOutOfOrder));

        // Terminator starts before the body ends.
        let mut overlapping = heredoc(HeredocForm::Bare, PayloadTerminal::Complete);
        overlapping.terminator_range = Some(span(10, 17));
        assert!(
            overlapping.contradictions().contains(&PayloadContradiction::HeredocRegionsOutOfOrder)
        );
    }

    #[test]
    fn a_heredoc_full_region_must_contain_its_own_parts() {
        let mut doc = heredoc(HeredocForm::Bare, PayloadTerminal::Complete);
        doc.full_region = Some(span(0, 10));
        assert!(doc.contradictions().contains(&PayloadContradiction::FullRegionMissesAPart));

        let mut mislabelled = heredoc(HeredocForm::Bare, PayloadTerminal::Complete);
        mislabelled.declaration.label_range = span(20, 25);
        assert!(
            mislabelled.contradictions().contains(&PayloadContradiction::LabelRangeOutsideOpener)
        );
    }

    #[test]
    fn a_backslash_quoted_heredoc_is_its_own_spelling() {
        // `<<\EOF` behaves like `<<'EOF'` but is not spelled like it.
        assert!(!HeredocForm::BackslashQuoted.interpolates());
        assert!(!HeredocForm::SingleQuoted.interpolates());
        assert_ne!(HeredocForm::BackslashQuoted, HeredocForm::SingleQuoted);
        assert_ne!(HeredocForm::BackslashQuoted, HeredocForm::Bare);
        assert!(!HeredocForm::BackslashQuoted.is_execution_boundary());

        let doc = heredoc(HeredocForm::BackslashQuoted, PayloadTerminal::Complete);
        assert!(!doc.compat_interpolated());
        assert!(!doc.compat_command());
        assert_eq!(doc.compat_content(), Some("body\n"));
    }

    #[test]
    fn every_heredoc_form_is_mutually_distinct() {
        let forms = [
            HeredocForm::Bare,
            HeredocForm::BackslashQuoted,
            HeredocForm::SingleQuoted,
            HeredocForm::DoubleQuoted,
            HeredocForm::Backtick,
        ];
        for (i, left) in forms.iter().enumerate() {
            for (j, right) in forms.iter().enumerate() {
                assert_eq!(i == j, left == right, "{left:?} vs {right:?}");
            }
        }
    }

    // --- checks added after the third review round ------------------------

    #[test]
    fn an_inverted_range_is_caught_before_any_length_is_taken() {
        // `SourceLocation`'s constructors reject start > end, but its fields
        // are public, so a struct literal still produces one — and `len()`
        // panics on it with "attempt to subtract with overflow".
        let mut string = interpolated_string();
        string.content_range = Some(SourceLocation { start: 10, end: 1 });
        assert!(string.contradictions().contains(&PayloadContradiction::MalformedRange));
        assert!(string.proven_segments().is_none());

        let mut segmented = interpolated_string();
        segmented.segmentation = SourceSegmentation::Exact(vec![SourceSegment {
            raw_range: SourceLocation { start: 9, end: 2 },
            cooked_fragment: CookedValue::Proven("x".to_string()),
            payload: SourceSegmentPayload::Literal,
        }]);
        assert!(
            segmented
                .contradictions()
                .contains(&PayloadContradiction::MalformedSegmentRange { index: 0 })
        );
        assert!(segmented.proven_segments().is_none());

        let mut doc = heredoc(HeredocForm::Bare, PayloadTerminal::Complete);
        doc.terminator_range = Some(SourceLocation { start: 17, end: 14 });
        assert!(doc.contradictions().contains(&PayloadContradiction::MalformedRange));
    }

    #[test]
    fn a_proven_terminal_requires_the_geometry_it_claims() {
        // `Complete`/`Empty` assert proven delimiters, which needs a region.
        let mut no_region = interpolated_string();
        no_region.content_range = None;
        no_region.segmentation = SourceSegmentation::Unavailable;
        assert!(
            no_region
                .contradictions()
                .contains(&PayloadContradiction::TerminalRequiresContentRange)
        );
        assert_eq!(no_region.compat_value(), None);

        // `Complete` is proven delimiters with non-empty content; a provably
        // empty payload is `Empty`.
        let mut complete_but_empty = empty_single_quoted_string();
        complete_but_empty.terminal = PayloadTerminal::Complete;
        assert!(
            complete_but_empty
                .contradictions()
                .contains(&PayloadContradiction::CompleteTerminalWithEmptyContent)
        );
        assert_eq!(complete_but_empty.compat_value(), None);

        // The correctly-spelled empty payload stays coherent.
        assert!(empty_single_quoted_string().is_coherent());
    }

    #[test]
    fn a_proven_empty_payload_may_not_publish_a_nonempty_value() {
        // `Empty` asserts the source is proven to hold nothing. A non-empty
        // proven value asserts the opposite about the same payload. The region
        // check alone never catches this: the region here is correctly empty,
        // and an unsegmented payload has no segments to disagree with either.
        let mut string = empty_single_quoted_string();
        string.segmentation = SourceSegmentation::Unavailable;
        string.cooked = CookedValue::Proven("abc".to_string());
        assert!(
            string
                .contradictions()
                .contains(&PayloadContradiction::EmptyTerminalWithContradictoryValue)
        );
        assert_eq!(string.compat_value(), None);

        // Calling the same text `Partial` rather than `Proven` does not make it
        // compatible with source proven to hold nothing, and `Dynamic` has
        // nothing to interpolate over an empty region.
        for disposition in [CookedValue::Partial("abc".to_string()), CookedValue::Dynamic] {
            let mut relabelled = empty_single_quoted_string();
            relabelled.segmentation = SourceSegmentation::Unavailable;
            relabelled.cooked = disposition.clone();
            assert!(
                relabelled
                    .contradictions()
                    .contains(&PayloadContradiction::EmptyTerminalWithContradictoryValue),
                "{disposition:?} should contradict a proven-empty terminal"
            );
        }

        // `Unavailable` claims nothing, so it stays compatible with it.
        let mut not_computed = empty_single_quoted_string();
        not_computed.segmentation = SourceSegmentation::Unavailable;
        not_computed.cooked = CookedValue::Unavailable;
        assert!(not_computed.is_coherent(), "{:?}", not_computed.contradictions());

        // The same axis on a heredoc, where the body region plays the content
        // role -- one checker, both payload shapes.
        let mut empty_body = heredoc(HeredocForm::Bare, PayloadTerminal::Empty);
        empty_body.raw_body_range = Some(span(8, 8));
        empty_body.segmentation = SourceSegmentation::Unavailable;
        assert!(
            empty_body
                .contradictions()
                .contains(&PayloadContradiction::EmptyTerminalWithContradictoryValue)
        );
        assert_eq!(empty_body.compat_content(), None);

        // An honestly empty payload proves an empty value, and stays coherent.
        assert!(empty_single_quoted_string().is_coherent());
    }

    #[test]
    fn a_terminated_string_leaves_room_for_both_delimiters() {
        // `raw_range` spans the delimiters, so proven-terminated content cannot
        // touch either edge: `"abc"` is 0..5 with content 1..4.
        let mut swallows_opening = plain_double_quoted_string();
        swallows_opening.content_range = Some(span(0, 4));
        swallows_opening.segmentation =
            SourceSegmentation::Exact(vec![literal_segment(0, 4, "abc")]);
        assert!(
            swallows_opening
                .contradictions()
                .contains(&PayloadContradiction::TerminatedStringWithoutDelimiterBytes)
        );
        assert_eq!(swallows_opening.compat_value(), None);

        let mut swallows_closing = plain_double_quoted_string();
        swallows_closing.content_range = Some(span(1, 5));
        swallows_closing.segmentation =
            SourceSegmentation::Exact(vec![literal_segment(1, 5, "abc")]);
        assert!(
            swallows_closing
                .contradictions()
                .contains(&PayloadContradiction::TerminatedStringWithoutDelimiterBytes)
        );

        // An unterminated string has no closing delimiter to fit, so the right
        // edge stays legitimately available to it. Forbidding it here would
        // make an honest state unrepresentable.
        let mut unterminated = plain_double_quoted_string();
        unterminated.content_range = Some(span(1, 5));
        unterminated.segmentation = SourceSegmentation::Exact(vec![literal_segment(1, 5, "abc")]);
        unterminated.cooked = CookedValue::Unavailable;
        unterminated.terminal = PayloadTerminal::Unterminated;
        assert!(
            !unterminated
                .contradictions()
                .contains(&PayloadContradiction::TerminatedStringWithoutDelimiterBytes)
        );

        // A quote operator sits inside `raw_range` too: `q{abc}` is 0..6 with
        // content 2..5, so content starting at 1 is inside the `q` itself and a
        // one-byte strict-containment rule would have missed it.
        let mut q_literal = plain_double_quoted_string();
        q_literal.form = StringForm::QLiteral;
        q_literal.delimiter = StringDelimiter::Paired { open: '{', close: '}' };
        q_literal.raw_range = span(0, 6);
        q_literal.content_range = Some(span(2, 5));
        q_literal.segmentation = SourceSegmentation::Exact(vec![literal_segment(2, 5, "abc")]);
        assert!(q_literal.is_coherent(), "{:?}", q_literal.contradictions());

        let mut eats_the_operator = q_literal.clone();
        eats_the_operator.content_range = Some(span(1, 5));
        eats_the_operator.segmentation =
            SourceSegmentation::Exact(vec![literal_segment(1, 5, "abc")]);
        assert!(
            eats_the_operator
                .contradictions()
                .contains(&PayloadContradiction::TerminatedStringWithoutDelimiterBytes)
        );

        // `qq` and `qx` spell two operator bytes, not one.
        let mut qq_string = q_literal.clone();
        qq_string.form = StringForm::QqInterpolating;
        qq_string.raw_range = span(0, 7);
        qq_string.content_range = Some(span(3, 6));
        qq_string.segmentation = SourceSegmentation::Exact(vec![literal_segment(3, 6, "abc")]);
        assert!(qq_string.is_coherent(), "{:?}", qq_string.contradictions());

        let mut qq_short = qq_string.clone();
        qq_short.content_range = Some(span(2, 6));
        qq_short.segmentation = SourceSegmentation::Exact(vec![literal_segment(2, 6, "abc")]);
        assert!(
            qq_short
                .contradictions()
                .contains(&PayloadContradiction::TerminatedStringWithoutDelimiterBytes)
        );

        // `SourceLocation` counts bytes, so a multibyte delimiter reserves more
        // than one of them.
        let mut multibyte = q_literal.clone();
        multibyte.delimiter = StringDelimiter::Paired { open: '\u{ab}', close: '\u{bb}' };
        multibyte.raw_range = span(0, 8);
        multibyte.content_range = Some(span(3, 6));
        multibyte.segmentation = SourceSegmentation::Exact(vec![literal_segment(3, 6, "abc")]);
        assert_eq!(multibyte.delimiter.opening_len(), 2);
        assert!(multibyte.is_coherent(), "{:?}", multibyte.contradictions());

        let mut splits_the_delimiter = multibyte.clone();
        splits_the_delimiter.content_range = Some(span(2, 6));
        splits_the_delimiter.segmentation =
            SourceSegmentation::Exact(vec![literal_segment(2, 6, "abc")]);
        assert!(
            splits_the_delimiter
                .contradictions()
                .contains(&PayloadContradiction::TerminatedStringWithoutDelimiterBytes)
        );

        // Heredoc bodies are not delimiter-wrapped, so the rule is string-only.
        assert!(heredoc(HeredocForm::Bare, PayloadTerminal::Complete).is_coherent());
    }

    #[test]
    fn a_segment_kind_that_cannot_prove_text_may_not_carry_it() {
        // Recovery failed classification, so it cannot know the cooked text.
        let mut recovered = interpolated_string();
        recovered.segmentation = SourceSegmentation::Exact(vec![SourceSegment {
            raw_range: span(1, 10),
            cooked_fragment: CookedValue::Proven("invented".to_string()),
            payload: SourceSegmentPayload::Recovery {
                cause: SegmentRecoveryCause::UnparsedInterpolation,
            },
        }]);
        assert!(
            recovered.contradictions().contains(
                &PayloadContradiction::SegmentKindDisagreesWithCookedFragment { index: 0 }
            )
        );
        assert!(recovered.proven_segments().is_none());

        // Interpolation depends on runtime state, so it cannot be proven text.
        let mut interpolated = interpolated_string();
        interpolated.segmentation = SourceSegmentation::Exact(vec![SourceSegment {
            raw_range: span(1, 10),
            cooked_fragment: CookedValue::Proven("static".to_string()),
            payload: SourceSegmentPayload::Interpolation {
                expression: Box::new(variable_node(1, 10)),
            },
        }]);
        assert!(
            interpolated.contradictions().contains(
                &PayloadContradiction::SegmentKindDisagreesWithCookedFragment { index: 0 }
            )
        );

        // Literal and normalization runs can prove their fragments.
        assert!(literal_segment(0, 1, "a").kind_can_prove_its_fragment());
        assert!(normalization_segment(0, 4).kind_can_prove_its_fragment());
        assert!(escape_segment(0, 2, "\n").kind_can_prove_its_fragment());
        assert!(interpolation_segment(0, 5).kind_can_prove_its_fragment());
    }

    #[test]
    fn a_partial_prefix_may_stop_early_but_may_not_skip_source() {
        // A genuine prefix: starts at the content start, stops early.
        let mut prefix = interpolated_string();
        prefix.terminal = PayloadTerminal::Budgeted;
        prefix.cooked = CookedValue::Partial("a".to_string());
        prefix.segmentation = SourceSegmentation::Partial(vec![literal_segment(1, 3, "ab")]);
        assert!(prefix.is_coherent(), "{:?}", prefix.contradictions());

        // A late first segment skips source inside the recorded prefix.
        let mut late_start = prefix.clone();
        late_start.segmentation = SourceSegmentation::Partial(vec![literal_segment(4, 6, "de")]);
        assert!(
            late_start.contradictions().contains(&PayloadContradiction::SegmentGap { index: 0 })
        );

        // So does a hole between two recorded segments.
        let mut internal_gap = prefix.clone();
        internal_gap.segmentation = SourceSegmentation::Partial(vec![
            literal_segment(1, 3, "ab"),
            literal_segment(5, 7, "ef"),
        ]);
        assert!(
            internal_gap.contradictions().contains(&PayloadContradiction::SegmentGap { index: 1 })
        );
    }

    // --- checks added after the fourth review round -----------------------

    #[test]
    fn a_dynamic_segment_means_the_payload_value_was_never_proven() {
        let mut invented = interpolated_string();
        invented.cooked = CookedValue::Proven("hello world".to_string());

        assert!(
            invented
                .contradictions()
                .contains(&PayloadContradiction::ProvenValueOverDynamicSegments)
        );
        assert_eq!(invented.compat_value(), None);
        assert_eq!(invented.proven_literal_value(), None);

        // An *unavailable* fragment is a different matter: the payload value
        // may have been proven by other means, so that combination stands.
        let mut unavailable_fragment = plain_double_quoted_string();
        unavailable_fragment.segmentation = SourceSegmentation::Exact(vec![SourceSegment {
            raw_range: span(1, 4),
            cooked_fragment: CookedValue::Unavailable,
            payload: SourceSegmentPayload::Literal,
        }]);
        assert!(
            !unavailable_fragment
                .contradictions()
                .contains(&PayloadContradiction::ProvenValueOverDynamicSegments)
        );
        assert_eq!(unavailable_fragment.compat_value(), Some("abc"));
    }

    #[test]
    fn a_bodyless_heredoc_still_orders_its_declaration_and_terminator() {
        // Recovered with a known terminator but no attached body: the two
        // body-relative checks both skip, so the pair must be ordered directly.
        let mut doc = heredoc(HeredocForm::Bare, PayloadTerminal::Recovered);
        doc.raw_body_range = None;
        doc.segmentation = SourceSegmentation::Unavailable;
        doc.cooked = CookedValue::Unavailable;
        doc.terminator_range = Some(span(8, 17));
        assert!(doc.is_coherent(), "{:?}", doc.contradictions());

        // Terminator starting inside the declaration is out of order.
        doc.terminator_range = Some(span(3, 17));
        assert!(doc.contradictions().contains(&PayloadContradiction::HeredocRegionsOutOfOrder));
    }

    #[test]
    fn a_form_cannot_be_written_with_a_delimiter_it_does_not_have() {
        // `'...'`, `"..."` and backticks each fix their own delimiter.
        let mut mismatched = empty_single_quoted_string();
        mismatched.delimiter = StringDelimiter::Same { delimiter: '"' };
        assert!(mismatched.contradictions().contains(&PayloadContradiction::FormDelimiterMismatch));
        assert_eq!(mismatched.compat_value(), None);

        let mut bracketed = plain_double_quoted_string();
        bracketed.delimiter = StringDelimiter::Paired { open: '{', close: '}' };
        assert!(bracketed.contradictions().contains(&PayloadContradiction::FormDelimiterMismatch));

        // The q-family chooses its own, and an unrecorded delimiter is allowed
        // for any form.
        assert!(
            StringForm::QLiteral
                .admits_delimiter(StringDelimiter::Paired { open: '(', close: ')' })
        );
        assert!(StringForm::QxCommand.admits_delimiter(StringDelimiter::Same { delimiter: '!' }));
        assert!(StringForm::SingleQuoted.admits_delimiter(StringDelimiter::Unavailable));
        assert!(StringForm::Backtick.admits_delimiter(StringDelimiter::Same { delimiter: '`' }));
        assert!(!StringForm::Backtick.admits_delimiter(StringDelimiter::Same { delimiter: '\'' }));
    }
}
