use super::{FormatResult, TextRange};
use serde::{Deserialize, Serialize};

pub use perl_tidy::{
    FormatChangeSummary, FormatContext, FormatDisposition, FormatEngine, FormatEvidenceState,
    FormatExecutionStrategy, FormatIdentity, FormatLineEndingDisposition, FormatReasonCode,
    FormatSafetyEvidence,
};

/// Requested formatting target in the former UTF-16 interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FormatRequestTarget {
    /// Format the complete document.
    Document,
    /// Format one requested source range.
    Range {
        /// Original requested UTF-16 range, retained independently of edits.
        range: TextRange,
    },
}

/// Derive the native execution strategy from the immutable requested target.
#[must_use]
pub const fn execution_for_requested_native_target(
    target: &FormatRequestTarget,
) -> FormatExecutionStrategy {
    match target {
        FormatRequestTarget::Document => FormatExecutionStrategy::NativeDocument,
        FormatRequestTarget::Range { .. } => FormatExecutionStrategy::NativeRangeOrIsland,
    }
}

/// Projection of the canonical terminal outcome into the former DTO shape.
///
/// Source/configuration fingerprints follow the canonical `perl-tidy` schema.
/// New consumers must use its outcome and edit plan to retain admitted bytes,
/// explicit widening and source-bound application evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatOutcome {
    /// Exhaustive terminal disposition.
    pub disposition: FormatDisposition,
    /// Stable machine-readable reason.
    pub reason: FormatReasonCode,
    /// Source, implementation, mode, and configuration identity.
    pub identity: FormatIdentity,
    /// Immutable caller request target.
    pub target: FormatRequestTarget,
    /// Engine path actually executed, independent of returned edit shape.
    pub execution: FormatExecutionStrategy,
    /// Bounded edit and changed-source summary.
    pub change: FormatChangeSummary,
    /// Safety evidence used to authorize or refuse edits.
    pub safety: FormatSafetyEvidence,
    /// Optional bounded next action for callers and presentation layers.
    pub next_action: Option<String>,
}

/// Compatibility result plus its explicit terminal outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedFormatResult {
    /// Former formatted text, UTF-16 edits, and diagnostics contract.
    pub result: FormatResult,
    /// Projection of the canonical disposition, identity and safety evidence.
    pub outcome: FormatOutcome,
}

impl TypedFormatResult {
    /// Consume the typed wrapper and return the compatibility result.
    #[must_use]
    pub fn into_result(self) -> FormatResult {
        self.result
    }
}
