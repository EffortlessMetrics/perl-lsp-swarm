//! Lexer checkpointing for incremental parsing.

mod behavior_equality;
mod cache;
mod core;
mod diff;
mod identity;

pub use behavior_equality::{
    BEHAVIOR_BEARING_STATE_COMPARE_VERSION, BehaviorBearingStateFingerprint,
    BehaviorBearingStateMismatch,
};
pub use cache::CheckpointCache;
pub(crate) use core::ReplayState;
pub use core::{
    CheckpointContext, Checkpointable, LexerCheckpoint, PendingHeredocCheckpoint,
    QuoteOperatorCheckpoint,
};
pub use diff::CheckpointDiff;
pub(crate) use identity::compute_content_digest;
pub use identity::{
    CHECKPOINT_SCHEMA_VERSION, CheckpointNewlinePolicy, CheckpointRestoreError,
    LexerCheckpointIdentity, LexerPolicyIdentity,
};
#[cfg(test)]
pub(crate) use identity::{diagnostic_digest_counter, reset_diagnostic_digest_counter};

#[cfg(test)]
mod tests;
