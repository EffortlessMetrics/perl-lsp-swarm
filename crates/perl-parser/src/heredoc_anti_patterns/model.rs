//! Shared anti-pattern data model types.

/// Source location of a detected anti-pattern.
///
/// All three coordinates are provided so callers can serve both LSP (line/column)
/// and byte-level (offset) consumers without re-computing positions.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    /// Zero-based line number within the scanned source fragment.
    pub line: usize,
    /// Zero-based column (byte offset from the start of the line).
    pub column: usize,
    /// Absolute byte offset from the start of the scanned source fragment.
    pub offset: usize,
}

/// Diagnostic severity level for a detected anti-pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum Severity {
    /// The construct will likely cause a runtime or parse failure.
    Error,
    /// The construct works but is fragile or difficult to analyze statically.
    Warning,
    /// The construct is valid but could be improved for readability or tooling support.
    Info,
}

/// Result of extracting a heredoc delimiter from source.
///
/// Capture-miss (`Unknown`) is not the same as a missing delimiter regex
/// (`Unavailable`). Callers must not treat either as a fabricated identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeredocDelimiter {
    /// A delimiter identifier was extracted from source.
    Extracted(String),
    /// The delimiter pattern ran and did not find an identifier.
    Unknown,
    /// The shared delimiter pattern was not available, so no identifier was read.
    Unavailable,
}

/// Stable identity of one heredoc anti-pattern detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DetectorId {
    /// Heredoc declared inside a `format` body.
    FormatHeredoc,
    /// Heredoc declared inside a `BEGIN { ... }` block.
    BeginTimeHeredoc,
    /// Heredoc terminator determined by a runtime expression.
    DynamicDelimiter,
    /// `use Filter::*` that may rewrite source before analysis.
    SourceFilter,
    /// Heredoc inside a regex code block.
    RegexCodeBlock,
    /// Heredoc inside a string argument to `eval`.
    EvalString,
    /// Heredoc written to a tied handle.
    TiedHandle,
}

impl DetectorId {
    /// Stable machine identity for receipts and reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FormatHeredoc => "format_heredoc",
            Self::BeginTimeHeredoc => "begin_time_heredoc",
            Self::DynamicDelimiter => "dynamic_delimiter",
            Self::SourceFilter => "source_filter",
            Self::RegexCodeBlock => "regex_code_block",
            Self::EvalString => "eval_string",
            Self::TiedHandle => "tied_handle",
        }
    }
}

/// Bounded reason a detector or helper pattern is not ready.
///
/// Public projections carry detector/pattern identity only. They do not
/// embed regex-library error prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectorFailureReason {
    /// One or more source-authored patterns failed to compile.
    PatternUnavailable {
        /// Stable pattern identities such as `FORMAT_PATTERN`.
        pattern_ids: Vec<&'static str>,
    },
}

/// Initialization and execution disposition of one detector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectorState {
    /// The detector ran with all required patterns available.
    Complete,
    /// The detector ran, but a helper pattern was unavailable.
    Limited {
        /// Why the detector could not use every helper it normally uses.
        reason: DetectorFailureReason,
    },
    /// The detector did not run because a required pattern was unavailable.
    Unavailable {
        /// Why the detector could not start.
        reason: DetectorFailureReason,
    },
}

impl DetectorState {
    /// Whether this detector executed its scan.
    pub fn ran(&self) -> bool {
        matches!(self, Self::Complete | Self::Limited { .. })
    }
}

/// One detector's observed disposition in a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectorObservation {
    /// Which detector this observation describes.
    pub id: DetectorId,
    /// Whether that detector completed, ran with a limitation, or did not run.
    pub state: DetectorState,
}

/// Completeness of a detector suite scan.
///
/// `Complete` is not implied by an empty diagnostic list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionStatus {
    /// Every applicable detector completed without limitation.
    Complete,
    /// At least one detector ran and at least one did not fully complete.
    Partial,
    /// No applicable detector completed.
    Unavailable,
}

impl DetectionStatus {
    /// Stable machine identity for receipts and reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Truthful result of a heredoc anti-pattern scan.
///
/// [`DetectionReport::diagnostics`] is never the completeness authority.
/// Use [`DetectionReport::status`] (or [`DetectionReport::is_complete_clean`])
/// to distinguish complete-clean from partial-empty.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectionReport {
    /// Findings from detectors that ran.
    pub diagnostics: Vec<Diagnostic>,
    /// Per-detector dispositions in stable [`DetectorId`] order.
    pub detectors: Vec<DetectorObservation>,
    /// Aggregate completeness of the scan.
    pub status: DetectionStatus,
}

impl DetectionReport {
    /// Complete scan that produced no findings.
    ///
    /// A partial or unavailable scan with an empty diagnostic list is not
    /// complete-clean.
    pub fn is_complete_clean(&self) -> bool {
        self.status == DetectionStatus::Complete && self.diagnostics.is_empty()
    }
}

/// A specific category of heredoc-related anti-pattern found in Perl source.
///
/// Each variant captures the [`Location`] of the offending construct plus any
/// context needed to produce a useful diagnostic message.
#[derive(Debug, Clone, PartialEq)]
pub enum AntiPattern {
    /// A heredoc declared inside a `format` body.
    FormatHeredoc {
        /// Location of the `format` declaration.
        location: Location,
        /// Name of the format.
        format_name: String,
        /// Delimiter extracted from the format body, or an explicit fallback.
        heredoc_delimiter: HeredocDelimiter,
    },
    /// A heredoc declared inside a `BEGIN { ... }` block, evaluated at compile time.
    BeginTimeHeredoc {
        /// Location of the `BEGIN` block.
        location: Location,
        /// Text scanned inside the block.
        heredoc_content: String,
        /// Documented side-effect notes for the diagnostic.
        side_effects: Vec<String>,
    },
    /// A heredoc whose terminator is determined by a variable or expression at runtime.
    DynamicHeredocDelimiter {
        /// Location of the dynamic delimiter expression.
        location: Location,
        /// The unmatched dynamic delimiter expression text.
        expression: String,
    },
    /// A `use Filter::*` statement that may rewrite source before static analysis runs.
    SourceFilterHeredoc {
        /// Location of the `use Filter::` statement.
        location: Location,
        /// Filter module name captured from source.
        module: String,
    },
    /// A heredoc embedded inside a `(?{ ... })` regex code block.
    RegexCodeBlockHeredoc {
        /// Location of the regex code-block heredoc.
        location: Location,
    },
    /// A heredoc embedded inside a string argument to `eval`.
    EvalStringHeredoc {
        /// Location of the `eval` string heredoc.
        location: Location,
    },
    /// A heredoc written to a filehandle that has been `tie`d to a custom class.
    TiedHandleHeredoc {
        /// Location of the `print` to the tied handle.
        location: Location,
        /// Normalized handle name.
        handle_name: String,
    },
}

/// A fully-formed diagnostic produced by the anti-pattern detector.
///
/// Contains everything needed to display a problem in an IDE or report:
/// the severity, the matched pattern (with location), a human-readable message,
/// a longer explanation, an optional suggested fix, and `perldoc` references.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    /// How serious the problem is.
    pub severity: Severity,
    /// The specific anti-pattern that triggered this diagnostic.
    pub pattern: AntiPattern,
    /// Short one-line summary suitable for an IDE problem marker.
    pub message: String,
    /// Longer explanation of why the construct is problematic.
    pub explanation: String,
    /// Optional concrete suggestion for fixing the problem.
    pub suggested_fix: Option<String>,
    /// Relevant `perldoc` pages or documentation references.
    pub references: Vec<String>,
}

impl AntiPattern {
    /// Byte offset of this finding in the scanned source fragment.
    pub fn offset(&self) -> usize {
        match self {
            Self::FormatHeredoc { location, .. }
            | Self::BeginTimeHeredoc { location, .. }
            | Self::DynamicHeredocDelimiter { location, .. }
            | Self::SourceFilterHeredoc { location, .. }
            | Self::RegexCodeBlockHeredoc { location, .. }
            | Self::EvalStringHeredoc { location, .. }
            | Self::TiedHandleHeredoc { location, .. } => location.offset,
        }
    }
}
