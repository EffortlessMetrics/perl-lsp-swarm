//! Lexer checkpointing for incremental parsing.

mod cache;
mod core;
mod diff;

pub use cache::CheckpointCache;
pub use core::{
    CheckpointContext, Checkpointable, LexerCheckpoint, PendingHeredocCheckpoint,
    QuoteOperatorCheckpoint,
};
pub use diff::CheckpointDiff;

#[cfg(test)]
mod tests;
