//! Checkpoint identity: schema, source/content/generation, and construction policy.
//!
//! Configuration identity is a named typed snapshot of construction-time
//! behavior, not an ad-hoc hash of [`crate::LexerConfig`]. Source/content/
//! generation types come from `perl-source-identity` (#4851). Exact
//! `LocalSymbolTable` source binding remains #8812; this snapshot stores the
//! declared name set so same-table restore can succeed and a different or
//! absent table fails closed.

use std::collections::BTreeSet;

use perl_source_identity::{ContentDigest, LogicalSourceId, SourceGeneration};
use thiserror::Error;

use crate::config::LexerConfig;
use crate::symbol_table::LocalSymbolTable;

/// Checkpoint schema version captured with every live snapshot.
pub const CHECKPOINT_SCHEMA_VERSION: u32 = 1;

/// How source newlines participate in checkpoint identity.
///
/// The lexer has no independent newline-translation control: LF, CRLF, and
/// bare CR remain distinct because they are part of the exact content digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckpointNewlinePolicy {
    /// Identity is the exact source bytes; newline forms are not normalized.
    SourceExact,
}

/// Construction-time lexer policy that must match before restore.
///
/// `track_positions` and the empty `simd` feature are compatibility no-ops and
/// do not participate. Mutable lexical state is not stored here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexerPolicyIdentity {
    interpolation: bool,
    lookahead_limit: usize,
    qw_recovery_enabled: bool,
    emit_heredoc_body_tokens: bool,
    symbol_names: Option<BTreeSet<Box<str>>>,
}

impl LexerPolicyIdentity {
    pub(crate) fn from_construction(
        config: &LexerConfig,
        qw_recovery_enabled: bool,
        emit_heredoc_body_tokens: bool,
    ) -> Self {
        Self {
            interpolation: config.interpolation_enabled(),
            lookahead_limit: config.lookahead_limit(),
            qw_recovery_enabled,
            emit_heredoc_body_tokens,
            symbol_names: config.symbol_table.as_ref().map(LocalSymbolTable::identity_names),
        }
    }

    /// Ordinary-string / `qq` / interpolating-heredoc interpolation switch.
    #[must_use]
    pub fn interpolation_enabled(&self) -> bool {
        self.interpolation
    }

    /// Shared cursor lookahead limit.
    #[must_use]
    pub fn lookahead_limit(&self) -> usize {
        self.lookahead_limit
    }

    /// Whether malformed `qw` uses the recovery path.
    #[must_use]
    pub fn qw_recovery_enabled(&self) -> bool {
        self.qw_recovery_enabled
    }

    /// Whether heredoc bodies are emitted as tokens.
    #[must_use]
    pub fn emit_heredoc_body_tokens(&self) -> bool {
        self.emit_heredoc_body_tokens
    }

    /// Whether a file-local symbol table was bound at construction.
    #[must_use]
    pub fn has_symbol_table(&self) -> bool {
        self.symbol_names.is_some()
    }
}

/// Explicit identity bound to a checkpoint at capture time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexerCheckpointIdentity {
    schema: u32,
    content: ContentDigest,
    logical_source: Option<LogicalSourceId>,
    generation: SourceGeneration,
    policy: LexerPolicyIdentity,
    newline_policy: CheckpointNewlinePolicy,
}

impl LexerCheckpointIdentity {
    pub(crate) fn capture(
        source: &str,
        config: &LexerConfig,
        qw_recovery_enabled: bool,
        emit_heredoc_body_tokens: bool,
        logical_source: Option<LogicalSourceId>,
        generation: SourceGeneration,
    ) -> Self {
        Self {
            schema: CHECKPOINT_SCHEMA_VERSION,
            content: ContentDigest::of_bytes(source.as_bytes()),
            logical_source,
            generation,
            policy: LexerPolicyIdentity::from_construction(
                config,
                qw_recovery_enabled,
                emit_heredoc_body_tokens,
            ),
            newline_policy: CheckpointNewlinePolicy::SourceExact,
        }
    }

    pub(crate) fn retarget_content(&mut self, source: &str) {
        self.content = ContentDigest::of_bytes(source.as_bytes());
    }

    pub(crate) fn set_generation(&mut self, generation: SourceGeneration) {
        self.generation = generation;
    }

    pub(crate) fn set_logical_source(&mut self, logical_source: Option<LogicalSourceId>) {
        self.logical_source = logical_source;
    }

    pub(crate) fn set_schema_for_test(&mut self, schema: u32) {
        self.schema = schema;
    }

    /// Schema version captured with this checkpoint.
    #[must_use]
    pub fn schema(&self) -> u32 {
        self.schema
    }

    /// Canonical digest of the exact source bytes at capture (or last rebind).
    #[must_use]
    pub fn content(&self) -> &ContentDigest {
        &self.content
    }

    /// Logical source, when the producer bound one.
    #[must_use]
    pub fn logical_source(&self) -> Option<&LogicalSourceId> {
        self.logical_source.as_ref()
    }

    /// Source generation bound at capture (or last rebind).
    #[must_use]
    pub fn generation(&self) -> &SourceGeneration {
        &self.generation
    }

    /// Construction-time policy identity.
    #[must_use]
    pub fn policy(&self) -> &LexerPolicyIdentity {
        &self.policy
    }

    /// Newline identity policy.
    #[must_use]
    pub fn newline_policy(&self) -> CheckpointNewlinePolicy {
        self.newline_policy
    }
}

/// Why restoring a checkpoint was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum CheckpointRestoreError {
    /// Logical source identity does not match the target lexer.
    #[error("checkpoint logical source does not match the target lexer")]
    WrongSource,
    /// Exact content digest does not match the target lexer input.
    #[error("checkpoint content digest does not match the target source")]
    WrongContent,
    /// Source generation does not match a known target generation.
    #[error("checkpoint source generation does not match the target lexer")]
    WrongGeneration,
    /// Construction-time policy (interpolation, lookahead, qw, body tokens, symbols) differs.
    #[error("checkpoint lexer configuration does not match the target lexer")]
    WrongConfiguration,
    /// Checkpoint schema is not the version this lexer understands.
    #[error("checkpoint schema is unknown")]
    UnknownSchema,
    /// Byte boundary is not a UTF-8 character boundary.
    #[error("checkpoint byte boundary is not valid UTF-8")]
    InvalidUtf8Boundary,
    /// Boundary is not a live lexer restart position for this source.
    #[error("checkpoint boundary is not a supported live restart")]
    UnsupportedBoundary,
    /// Captured quote, heredoc, or format state is internally incomplete.
    #[error("checkpoint is missing required quote, heredoc, or format state")]
    IncompleteState,
    /// An edit invalidated this checkpoint; it is not a default restart origin.
    #[error("checkpoint was invalidated by an overlapping or unrecoverable edit")]
    Invalidated,
}

impl LexerCheckpointIdentity {
    pub(crate) fn matches_target(
        &self,
        source: &str,
        config: &LexerConfig,
        qw_recovery_enabled: bool,
        emit_heredoc_body_tokens: bool,
        logical_source: Option<&LogicalSourceId>,
        generation: &SourceGeneration,
    ) -> Result<(), CheckpointRestoreError> {
        if self.schema != CHECKPOINT_SCHEMA_VERSION {
            return Err(CheckpointRestoreError::UnknownSchema);
        }
        if self.newline_policy != CheckpointNewlinePolicy::SourceExact {
            return Err(CheckpointRestoreError::WrongConfiguration);
        }
        match (&self.logical_source, logical_source) {
            (None, None) => {}
            (Some(captured), Some(target)) if captured == target => {}
            _ => return Err(CheckpointRestoreError::WrongSource),
        }
        match (&self.generation, generation) {
            (SourceGeneration::Unknown, SourceGeneration::Unknown) => {}
            (SourceGeneration::Known(captured), SourceGeneration::Known(target))
                if captured == target && !captured.is_empty() => {}
            _ => return Err(CheckpointRestoreError::WrongGeneration),
        }
        if self.content != ContentDigest::of_bytes(source.as_bytes()) {
            return Err(CheckpointRestoreError::WrongContent);
        }
        let target_policy = LexerPolicyIdentity::from_construction(
            config,
            qw_recovery_enabled,
            emit_heredoc_body_tokens,
        );
        if self.policy != target_policy {
            return Err(CheckpointRestoreError::WrongConfiguration);
        }
        Ok(())
    }
}
