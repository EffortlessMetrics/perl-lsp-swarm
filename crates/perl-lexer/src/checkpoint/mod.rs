//! Lexer checkpointing for incremental parsing.

mod cache;
mod core;
mod diff;
mod identity;

pub use cache::CheckpointCache;
pub(crate) use core::ReplayState;
pub use core::{
    CheckpointContext, Checkpointable, LexerCheckpoint, PendingHeredocCheckpoint,
    QuoteOperatorCheckpoint,
};
pub use diff::CheckpointDiff;
pub use identity::{
    CHECKPOINT_SCHEMA_VERSION, CheckpointNewlinePolicy, CheckpointRestoreError,
    LexerCheckpointIdentity, LexerPolicyIdentity,
};

#[cfg(test)]
mod tests;
