//! Canonical POD parse model types (#8444).

/// Version tag for the structured POD parse envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PodModelVersion {
    /// Initial ranged paragraph model.
    V1,
}

/// Inclusive-start, exclusive-end byte range in the original source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PodRange {
    /// Start byte offset.
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
}

impl PodRange {
    /// Slice `source` using this range.
    #[must_use]
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

/// One ranged POD command paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodCommand {
    /// Command name without the leading `=`.
    pub name: String,
    /// Byte range of the full command paragraph line in the source.
    pub range: PodRange,
    /// Byte range of the argument text after the command word.
    pub argument_range: PodRange,
}

/// Ranged ordinary or verbatim paragraph bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodTextBlock {
    /// Exact source bytes for the paragraph (may include interior newlines).
    pub range: PodRange,
}

/// Structured diagnostic emitted while parsing POD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodDiagnostic {
    /// Human-readable message.
    pub message: String,
    /// Optional source range the diagnostic refers to.
    pub range: Option<PodRange>,
}

/// A recognized POD block inside an active POD region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PodBlock {
    /// Known POD directive (`=head1`, `=cut`, …).
    Command(PodCommand),
    /// Ordinary (non-verbatim) paragraph text.
    Ordinary(PodTextBlock),
    /// Verbatim paragraph (leading space or tab on the first line).
    Verbatim(PodTextBlock),
    /// Unknown but syntactically valid `=word` directive.
    UnknownCommand(PodCommand),
}

/// Canonical ranged POD parse result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodParse {
    /// Ordered POD blocks in source order.
    pub blocks: Vec<PodBlock>,
    /// Non-fatal parse notes.
    pub diagnostics: Vec<PodDiagnostic>,
    /// Model schema version.
    pub model_version: PodModelVersion,
}

impl PodParse {
    /// Empty parse envelope.
    #[must_use]
    pub fn empty() -> Self {
        Self { blocks: Vec::new(), diagnostics: Vec::new(), model_version: PodModelVersion::V1 }
    }
}
