use crate::incremental::diagnostics::{LexRestartReport, LexRestartStrategy};
use thiserror::Error;

/// Stable production strategy recorded for one incremental result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum IncrementalStrategy {
    /// No source change and no lexer/parser work.
    Unchanged,
    /// Complete lexer and recovery parser fallback over the final source.
    FullFallback,
    /// Restore a stored full lexer checkpoint, lex to EOF, then run the full parser.
    CheckpointToEofThenFullParse,
    /// Restore and synchronize an exact token suffix, then run the full parser.
    CheckpointToExactTokenSyncThenFullParse,
    /// Patch one exhaustively proven AST leaf without invoking the full parser.
    BoundedAstLeafPatch,
    /// Non-production comparison after ordinary parsing.
    AnalyticalSimilarityOnly,
    /// Experimental or unsupported mechanism outside the production authority.
    UnsupportedOrExperimental,
}

impl IncrementalStrategy {
    /// Whether this strategy claims to avoid a production full-parser invocation.
    #[must_use]
    pub const fn claims_no_full_parser(self) -> bool {
        matches!(self, Self::Unchanged | Self::BoundedAstLeafPatch)
    }
}

/// Operation-local count returned by the actual canonical parser entry point.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ParserInvocationReceipt {
    pub(crate) full_parser_invocations: usize,
    pub(crate) recovery_parser_invocations: usize,
    pub(crate) nodes_constructed: usize,
}

/// Truthful performed-work receipt for one committed incremental result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct IncrementalWorkReceipt {
    /// Strategy that produced the committed result.
    pub strategy: IncrementalStrategy,
    /// Complete production parser invocations.
    pub full_parser_invocations: usize,
    /// Recovery-aware parser invocations.
    pub recovery_parser_invocations: usize,
    /// Fresh parser invocations used only as a validation oracle.
    pub validation_parser_invocations: usize,
    /// Old-source prefix bytes replayed only to reconstruct lexer state.
    pub old_prefix_bytes_replayed: usize,
    /// Current-source bytes freshly lexed.
    pub fresh_bytes_lexed: usize,
    /// Lexer tokens freshly emitted by the selected production path.
    pub fresh_tokens_emitted: usize,
    /// Old prefix tokens retained without fresh lexing.
    pub prefix_tokens_retained: usize,
    /// Old suffix tokens retained only after exact synchronization.
    pub suffix_tokens_retained: usize,
    /// AST nodes constructed by the production parser.
    pub nodes_constructed: usize,
    /// AST nodes retained by reviewed identity, not cloned or compared.
    pub nodes_retained_by_identity: usize,
    /// AST nodes cloned by the production strategy.
    pub nodes_cloned: usize,
    /// AST nodes patched in place or in a candidate clone.
    pub nodes_patched: usize,
    /// AST nodes compared only for analysis or validation.
    pub nodes_compared_only: usize,
    /// Complete lexer checkpoints restored.
    pub checkpoints_restored: usize,
    /// Candidate checkpoints rejected or invalidated.
    pub checkpoints_invalidated: usize,
    /// Complete checkpoints retained in the committed generation.
    pub stored_checkpoint_count: usize,
    /// Exact final source size.
    pub final_source_bytes: usize,
    /// Exact final token count.
    pub final_token_count: usize,
    /// Exact final AST node count.
    pub final_node_count: usize,
}

impl IncrementalWorkReceipt {
    pub(crate) fn from_parts(
        strategy: IncrementalStrategy,
        parser: ParserInvocationReceipt,
        lex: LexRestartReport,
        fresh_tokens_emitted: usize,
        final_source_bytes: usize,
        final_token_count: usize,
        final_node_count: usize,
    ) -> Self {
        let checkpoints_restored = usize::from(matches!(
            lex.strategy,
            LexRestartStrategy::StoredCheckpointToEof
        ));
        Self {
            strategy,
            full_parser_invocations: parser.full_parser_invocations,
            recovery_parser_invocations: parser.recovery_parser_invocations,
            validation_parser_invocations: 0,
            old_prefix_bytes_replayed: lex.old_prefix_bytes_replayed,
            fresh_bytes_lexed: lex.relexed_bytes,
            fresh_tokens_emitted,
            prefix_tokens_retained: lex.reused_prefix_tokens,
            suffix_tokens_retained: lex.reused_suffix_tokens,
            nodes_constructed: parser.nodes_constructed,
            nodes_retained_by_identity: 0,
            nodes_cloned: 0,
            nodes_patched: 0,
            nodes_compared_only: 0,
            checkpoints_restored,
            checkpoints_invalidated: 0,
            stored_checkpoint_count: lex.stored_checkpoint_count,
            final_source_bytes,
            final_token_count,
            final_node_count,
        }
    }

    /// Validate impossible or misleading work combinations.
    pub fn validate(&self) -> Result<(), IncrementalWorkReceiptError> {
        if self.strategy.claims_no_full_parser() && self.full_parser_invocations != 0 {
            return Err(IncrementalWorkReceiptError::HiddenFullParser {
                strategy: self.strategy,
                invocations: self.full_parser_invocations,
            });
        }
        if self.prefix_tokens_retained.saturating_add(self.suffix_tokens_retained)
            > self.final_token_count
        {
            return Err(IncrementalWorkReceiptError::RetainedTokenOverflow);
        }
        if self.nodes_retained_by_identity > self.final_node_count {
            return Err(IncrementalWorkReceiptError::RetainedNodeOverflow);
        }
        if self.suffix_tokens_retained > 0
            && self.strategy != IncrementalStrategy::CheckpointToExactTokenSyncThenFullParse
        {
            return Err(IncrementalWorkReceiptError::SuffixWithoutExactSync);
        }
        if self.validation_parser_invocations > 0
            && self.full_parser_invocations == 0
            && self.strategy == IncrementalStrategy::AnalyticalSimilarityOnly
        {
            return Err(IncrementalWorkReceiptError::AnalysisWithoutProductionResult);
        }
        match self.strategy {
            IncrementalStrategy::Unchanged => {
                if self.full_parser_invocations != 0
                    || self.recovery_parser_invocations != 0
                    || self.fresh_bytes_lexed != 0
                    || self.fresh_tokens_emitted != 0
                    || self.nodes_constructed != 0
                {
                    return Err(IncrementalWorkReceiptError::UnchangedPerformedWork);
                }
            }
            IncrementalStrategy::FullFallback => {
                if self.full_parser_invocations == 0
                    || self.prefix_tokens_retained != 0
                    || self.suffix_tokens_retained != 0
                    || self.nodes_retained_by_identity != 0
                {
                    return Err(IncrementalWorkReceiptError::InvalidFullFallback);
                }
            }
            IncrementalStrategy::CheckpointToEofThenFullParse => {
                if self.full_parser_invocations == 0
                    || self.checkpoints_restored != 1
                    || self.old_prefix_bytes_replayed != 0
                    || self.suffix_tokens_retained != 0
                {
                    return Err(IncrementalWorkReceiptError::InvalidCheckpointToEof);
                }
            }
            IncrementalStrategy::CheckpointToExactTokenSyncThenFullParse
            | IncrementalStrategy::BoundedAstLeafPatch
            | IncrementalStrategy::AnalyticalSimilarityOnly
            | IncrementalStrategy::UnsupportedOrExperimental => {}
        }
        Ok(())
    }
}

/// Invalid or misleading incremental work receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum IncrementalWorkReceiptError {
    /// A no-full-parser strategy invoked the production parser.
    #[error("strategy {strategy:?} invoked the full parser {invocations} time(s)")]
    HiddenFullParser {
        /// Strategy making the claim.
        strategy: IncrementalStrategy,
        /// Observed full-parser invocations.
        invocations: usize,
    },
    /// Retained token counts exceed the final token count.
    #[error("retained token counts exceed the final token count")]
    RetainedTokenOverflow,
    /// Retained node identity count exceeds the final node count.
    #[error("retained node count exceeds the final node count")]
    RetainedNodeOverflow,
    /// Suffix tokens were retained without an exact-sync strategy.
    #[error("suffix tokens require exact synchronization")]
    SuffixWithoutExactSync,
    /// Analysis/oracle work was reported without a production result.
    #[error("analysis-only receipt does not identify a production result")]
    AnalysisWithoutProductionResult,
    /// An unchanged result performed lexer or parser work.
    #[error("unchanged strategy reported fresh lexer or parser work")]
    UnchangedPerformedWork,
    /// Full fallback retained old work or omitted the full parser.
    #[error("full fallback receipt is internally inconsistent")]
    InvalidFullFallback,
    /// Checkpoint-to-EOF receipt omitted restore/full-parse truth or claimed suffix reuse.
    #[error("checkpoint-to-EOF receipt is internally inconsistent")]
    InvalidCheckpointToEof,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_receipt(strategy: IncrementalStrategy) -> IncrementalWorkReceipt {
        IncrementalWorkReceipt {
            strategy,
            full_parser_invocations: 0,
            recovery_parser_invocations: 0,
            validation_parser_invocations: 0,
            old_prefix_bytes_replayed: 0,
            fresh_bytes_lexed: 0,
            fresh_tokens_emitted: 0,
            prefix_tokens_retained: 0,
            suffix_tokens_retained: 0,
            nodes_constructed: 0,
            nodes_retained_by_identity: 0,
            nodes_cloned: 0,
            nodes_patched: 0,
            nodes_compared_only: 0,
            checkpoints_restored: 0,
            checkpoints_invalidated: 0,
            stored_checkpoint_count: 0,
            final_source_bytes: 0,
            final_token_count: 0,
            final_node_count: 0,
        }
    }

    #[test]
    fn hidden_full_parse_fails_a_no_full_parser_strategy() {
        let mut receipt = empty_receipt(IncrementalStrategy::BoundedAstLeafPatch);
        receipt.full_parser_invocations = 1;
        assert!(matches!(
            receipt.validate(),
            Err(IncrementalWorkReceiptError::HiddenFullParser { .. })
        ));
    }

    #[test]
    fn cloning_and_comparison_do_not_become_retained_identity() {
        let mut receipt = empty_receipt(IncrementalStrategy::AnalyticalSimilarityOnly);
        receipt.full_parser_invocations = 1;
        receipt.recovery_parser_invocations = 1;
        receipt.nodes_cloned = 8;
        receipt.nodes_compared_only = 8;
        receipt.final_node_count = 8;
        assert_eq!(receipt.nodes_retained_by_identity, 0);
        assert!(receipt.validate().is_ok());
    }

    #[test]
    fn suffix_retention_requires_the_exact_sync_strategy() {
        let mut receipt = empty_receipt(IncrementalStrategy::CheckpointToEofThenFullParse);
        receipt.full_parser_invocations = 1;
        receipt.recovery_parser_invocations = 1;
        receipt.checkpoints_restored = 1;
        receipt.suffix_tokens_retained = 1;
        receipt.final_token_count = 1;
        assert_eq!(receipt.validate(), Err(IncrementalWorkReceiptError::SuffixWithoutExactSync));
    }
}