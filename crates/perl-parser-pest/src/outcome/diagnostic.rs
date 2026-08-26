//! Bounded diagnostic vocabulary and deterministic ordering.

use super::failure::OutcomeError;
use super::range::SourceRange;
use serde::{Deserialize, Serialize};

/// Machine-readable diagnostic class for parser-domain outcomes.
///
/// Unsupported syntax is never encoded as malformed syntax. Recovery actions are
/// recorded separately on [`ParseDiagnostic::recovery_action`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParseDiagnosticKind {
    /// Original complete-parse rejection retained when recovery is attempted.
    OriginalRejection,
    /// A source range was skipped during recovery.
    SkippedSource,
    /// A fragment was parsed after recovery resumed.
    RecoveredFragment,
    /// AST construction failed for a parsed fragment.
    BuilderFailure,
    /// Trailing input received no parse disposition.
    UnaccountedTrailingInput,
    /// Syntax the parser does not claim to support.
    UnsupportedSyntax,
}

/// Recovery action attached to a diagnostic, when one occurred.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryAction {
    /// The range was omitted from the returned AST.
    Skip,
    /// Parsing resumed after the recorded range.
    ResumeAfter,
    /// The range was replaced with an error/recovery node.
    ReplaceWithErrorNode,
    /// The original complete-parse error was retained as context.
    RetainOriginalError,
}

/// One parser-domain diagnostic bound to original-source bytes.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParseDiagnostic {
    kind: ParseDiagnosticKind,
    range: SourceRange,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recovery_action: Option<RecoveryAction>,
}

impl ParseDiagnosticKind {
    /// Stable sort rank. Lower ranks precede later kinds at the same range.
    #[must_use]
    pub const fn sort_rank(self) -> u8 {
        match self {
            Self::OriginalRejection => 0,
            Self::SkippedSource => 1,
            Self::RecoveredFragment => 2,
            Self::BuilderFailure => 3,
            Self::UnaccountedTrailingInput => 4,
            Self::UnsupportedSyntax => 5,
        }
    }

    /// Stable kebab-case machine name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OriginalRejection => "original-rejection",
            Self::SkippedSource => "skipped-source",
            Self::RecoveredFragment => "recovered-fragment",
            Self::BuilderFailure => "builder-failure",
            Self::UnaccountedTrailingInput => "unaccounted-trailing-input",
            Self::UnsupportedSyntax => "unsupported-syntax",
        }
    }
}

impl RecoveryAction {
    /// Stable sort rank. Lower ranks precede later actions at the same diagnostic.
    #[must_use]
    pub const fn sort_rank(self) -> u8 {
        match self {
            Self::Skip => 0,
            Self::ResumeAfter => 1,
            Self::ReplaceWithErrorNode => 2,
            Self::RetainOriginalError => 3,
        }
    }

    /// Stable kebab-case machine name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::ResumeAfter => "resume-after",
            Self::ReplaceWithErrorNode => "replace-with-error-node",
            Self::RetainOriginalError => "retain-original-error",
        }
    }
}

impl ParseDiagnostic {
    /// Construct a diagnostic. `range` must already be a validated [`SourceRange`].
    #[must_use]
    pub fn new(
        kind: ParseDiagnosticKind,
        range: SourceRange,
        message: impl Into<String>,
        detail: Option<String>,
        recovery_action: Option<RecoveryAction>,
    ) -> Self {
        Self { kind, range, message: message.into(), detail, recovery_action }
    }

    /// Diagnostic class.
    #[must_use]
    pub const fn kind(&self) -> ParseDiagnosticKind {
        self.kind
    }

    /// Original-source byte range.
    #[must_use]
    pub const fn range(&self) -> SourceRange {
        self.range
    }

    /// Human-readable message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Optional extra detail. Not a second message authority.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// Optional recovery action. Absent when the diagnostic is not a recovery event.
    #[must_use]
    pub const fn recovery_action(&self) -> Option<RecoveryAction> {
        self.recovery_action
    }

    /// Sort diagnostics by range, then kind rank, then message, then detail, then action.
    pub fn sort_slice(diagnostics: &mut [Self]) {
        diagnostics.sort_by(|left, right| {
            left.range
                .start()
                .cmp(&right.range.start())
                .then(left.range.end().cmp(&right.range.end()))
                .then(left.kind.sort_rank().cmp(&right.kind.sort_rank()))
                .then(left.message.cmp(&right.message))
                .then(left.detail.cmp(&right.detail))
                .then(
                    recovery_action_sort_rank(left.recovery_action)
                        .cmp(&recovery_action_sort_rank(right.recovery_action)),
                )
        });
    }

    /// Bind every diagnostic range to `source` and return a deterministically ordered copy.
    pub fn ordered_for_source(
        diagnostics: impl Into<Vec<Self>>,
        source: &str,
    ) -> Result<Vec<Self>, OutcomeError> {
        let mut diagnostics = diagnostics.into();
        for diagnostic in &diagnostics {
            diagnostic.range.check_over_source(source)?;
        }
        Self::sort_slice(&mut diagnostics);
        Ok(diagnostics)
    }
}

fn recovery_action_sort_rank(action: Option<RecoveryAction>) -> u8 {
    match action {
        None => 0,
        Some(action) => action.sort_rank().saturating_add(1),
    }
}
