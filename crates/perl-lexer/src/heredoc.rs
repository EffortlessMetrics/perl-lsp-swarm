use std::ops::Range;
use std::sync::Arc;

use perl_source_identity::{ContentDigest, LogicalSourceId, SourceGeneration};

/// Exclusive UTF-8 byte range captured during the original opener scan.
///
/// Ranges are stored as offsets, never as borrowed source slices or host paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HeredocByteRange {
    /// Inclusive start offset.
    start: usize,
    /// Exclusive end offset.
    end: usize,
}

impl HeredocByteRange {
    #[cfg(test)]
    pub(crate) const EMPTY: Self = Self { start: 0, end: 0 };

    pub(crate) fn new(start: usize, end: usize) -> Option<Self> {
        (end >= start).then_some(Self { start, end })
    }

    /// Inclusive start offset.
    #[must_use]
    pub const fn start(self) -> usize {
        self.start
    }

    /// Exclusive end offset.
    #[must_use]
    pub const fn end(self) -> usize {
        self.end
    }

    /// Exclusive `start..end` view of this range.
    #[must_use]
    pub const fn as_range(self) -> Range<usize> {
        self.start..self.end
    }

    /// Source slice for this range, if the offsets are in bounds and on UTF-8 boundaries.
    #[must_use]
    pub fn slice(self, source: &str) -> Option<&str> {
        source.get(self.as_range())
    }
}

/// `<<` versus `<<~` operator captured at admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeredocOperator {
    /// Ordinary `<<`.
    Ordinary,
    /// Indented `<<~` (Perl 5.26).
    Indented,
}

/// Written form of an admitted heredoc label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeredocOpenerForm {
    /// `<<EOF`
    Bare,
    /// `` <<\EOF ``
    Backslash,
    /// `<<'EOF'`
    SingleQuoted,
    /// `<<"EOF"`
    DoubleQuoted,
    /// `` <<`EOF` ``
    Backtick,
}

impl HeredocOpenerForm {
    /// Lexer interpolation disposition (#8779): bare and double-quoted interpolate;
    /// single-quoted, backslash, and backtick do not. Backtick is the command boundary.
    #[must_use]
    pub const fn interpolates(self) -> bool {
        matches!(self, Self::Bare | Self::DoubleQuoted)
    }

    /// Whether this form is a command-execution boundary.
    #[must_use]
    pub const fn is_command(self) -> bool {
        matches!(self, Self::Backtick)
    }
}

/// `#4851` source snapshot copied onto a queued heredoc at admission.
///
/// Tokenization without a bound logical source remains available: `logical_source`
/// is then `None` and `generation` stays [`SourceGeneration::Unknown`]. That path
/// does not authorize checkpoint reuse, currentness, parser attachment, or
/// durable publication.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeredocAdmittedSource {
    logical_source: Option<LogicalSourceId>,
    content: ContentDigest,
    generation: SourceGeneration,
}

impl HeredocAdmittedSource {
    pub(crate) fn snapshot(
        logical_source: Option<LogicalSourceId>,
        content: ContentDigest,
        generation: SourceGeneration,
    ) -> Self {
        Self { logical_source, content, generation }
    }

    /// Bound logical source, if a producer supplied one.
    #[must_use]
    pub fn logical_source(&self) -> Option<&LogicalSourceId> {
        self.logical_source.as_ref()
    }

    /// Canonical content digest of the immutable lexer input.
    #[must_use]
    pub fn content_digest(&self) -> &ContentDigest {
        &self.content
    }

    /// Bound generation. [`SourceGeneration::Unknown`] is never promoted to current.
    #[must_use]
    pub fn generation(&self) -> &SourceGeneration {
        &self.generation
    }

    /// True only when a logical source is bound and the generation is known and non-empty.
    #[must_use]
    pub fn is_publication_ready(&self) -> bool {
        self.logical_source.is_some() && self.generation.is_known()
    }
}

/// Immutable declaration identity captured at successful heredoc queue admission (#13502).
///
/// Private field grouping is the implementation; every queued entry carries one of these.
/// The public [`crate::Token`] API does not carry this contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeredocQueueIdentity {
    ordinal: u32,
    source: HeredocAdmittedSource,
    opener_range: HeredocByteRange,
    operator_range: HeredocByteRange,
    operator: HeredocOperator,
    raw_label_range: HeredocByteRange,
    raw_label_spelling: Arc<str>,
    decoded_label: Arc<str>,
    form: HeredocOpenerForm,
    body_start: usize,
}

pub(crate) struct HeredocAdmission {
    pub ordinal: u32,
    pub source: HeredocAdmittedSource,
    pub opener_range: HeredocByteRange,
    pub operator_range: HeredocByteRange,
    pub operator: HeredocOperator,
    pub raw_label_range: HeredocByteRange,
    pub raw_label_spelling: Arc<str>,
    pub decoded_label: Arc<str>,
    pub form: HeredocOpenerForm,
}

impl HeredocQueueIdentity {
    pub(crate) fn admitted(parts: HeredocAdmission) -> Self {
        Self {
            ordinal: parts.ordinal,
            source: parts.source,
            opener_range: parts.opener_range,
            operator_range: parts.operator_range,
            operator: parts.operator,
            raw_label_range: parts.raw_label_range,
            raw_label_spelling: parts.raw_label_spelling,
            decoded_label: parts.decoded_label,
            form: parts.form,
            body_start: 0,
        }
    }

    pub(crate) fn set_body_start(&mut self, body_start: usize) {
        self.body_start = body_start;
    }

    /// Test-only complete identity used to seed replay-state round trips.
    #[cfg(test)]
    pub(crate) fn test_fixture(decoded_label: &str, body_start: usize, allow_indent: bool) -> Self {
        let operator =
            if allow_indent { HeredocOperator::Indented } else { HeredocOperator::Ordinary };
        let label_end = decoded_label.len();
        let operator_end = if allow_indent { 3 } else { 2 };
        let range = |end: usize| HeredocByteRange::new(0, end).unwrap_or(HeredocByteRange::EMPTY);
        let mut identity = Self::admitted(HeredocAdmission {
            ordinal: 0,
            source: HeredocAdmittedSource::snapshot(
                None,
                ContentDigest::of_bytes(b""),
                SourceGeneration::Unknown,
            ),
            opener_range: range(label_end),
            operator_range: range(operator_end),
            operator,
            raw_label_range: range(label_end),
            raw_label_spelling: Arc::from(decoded_label),
            decoded_label: Arc::from(decoded_label),
            form: HeredocOpenerForm::Bare,
        });
        identity.set_body_start(body_start);
        identity
    }

    /// Declaration ordinal within the pending batch; assigned in lexical order and never reused.
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    /// Source snapshot captured at admission.
    #[must_use]
    pub fn source(&self) -> &HeredocAdmittedSource {
        &self.source
    }

    /// Byte range of the full opener, from the first `<` through the end of the label.
    #[must_use]
    pub const fn opener_range(&self) -> HeredocByteRange {
        self.opener_range
    }

    /// Byte range of `<<` or `<<~` only.
    #[must_use]
    pub const fn operator_range(&self) -> HeredocByteRange {
        self.operator_range
    }

    /// Ordinary `<<` versus indented `<<~`.
    #[must_use]
    pub const fn operator(&self) -> HeredocOperator {
        self.operator
    }

    /// Byte range of the label as written, including quotes or a leading backslash.
    #[must_use]
    pub const fn raw_label_range(&self) -> HeredocByteRange {
        self.raw_label_range
    }

    /// Label spelling as written, including quotes or a leading backslash.
    #[must_use]
    pub fn raw_label_spelling(&self) -> &str {
        &self.raw_label_spelling
    }

    /// Terminator label with quoting removed. Distinct from [`Self::raw_label_spelling`].
    #[must_use]
    pub fn decoded_label(&self) -> &str {
        &self.decoded_label
    }

    /// Written form captured during the original opener scan.
    #[must_use]
    pub const fn form(&self) -> HeredocOpenerForm {
        self.form
    }

    /// Interpolation disposition owned by this entry (#8779).
    #[must_use]
    pub const fn interpolates(&self) -> bool {
        self.form.interpolates()
    }

    /// Command disposition owned by this entry.
    #[must_use]
    pub const fn is_command(&self) -> bool {
        self.form.is_command()
    }

    /// Indentation policy owned by this entry (`<<~`).
    #[must_use]
    pub const fn allow_indent(&self) -> bool {
        matches!(self.operator, HeredocOperator::Indented)
    }

    /// Byte offset where the body begins; `0` until the newline after the declaration line.
    #[must_use]
    pub const fn body_start(&self) -> usize {
        self.body_start
    }
}

/// Specification for a pending heredoc.
#[derive(Clone)]
pub(crate) struct HeredocSpec {
    pub(crate) label: Arc<str>,
    pub(crate) body_start: usize,  // byte offset where the body begins
    pub(crate) allow_indent: bool, // true if we saw <<~ (Perl 5.26 indented heredocs)
    /// Whether the body interpolates (#8779): bareword and `<<"EOF"`
    /// delimiters interpolate; `<<'EOF'`, `<<\EOF`, and backtick delimiters
    /// do not (the backtick form is an intentional command boundary).
    pub(crate) interpolates: bool,
    /// Complete immutable declaration identity captured at admission (#13502).
    pub(crate) identity: HeredocQueueIdentity,
}

impl HeredocSpec {
    pub(crate) fn from_identity(identity: HeredocQueueIdentity) -> Self {
        Self {
            label: Arc::from(identity.decoded_label()),
            body_start: identity.body_start(),
            allow_indent: identity.allow_indent(),
            interpolates: identity.interpolates(),
            identity,
        }
    }

    pub(crate) fn set_body_start(&mut self, body_start: usize) {
        self.body_start = body_start;
        self.identity.set_body_start(body_start);
    }
}
