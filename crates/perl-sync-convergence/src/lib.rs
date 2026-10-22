#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! `perl-sync-convergence` — canonical `perl_lsp.convergence_transaction.v1`
//! state and event model for continuous source synchronization (#11282).
//!
//! This crate is the persistence slice of the source-sync controller family:
//! it owns durable transaction identity, immutable generation receipts,
//! writer leases with takeover, the invalidation graph, and journal-based
//! resumability. It deliberately does not implement projection semantics,
//! open PRs, run admission, merge, or mutate any live surface (#11003
//! decomposition: those belong to sibling slices).
//!
//! # Design
//!
//! - **Content-addressed generations.** A [`GenerationId`] is derived from
//!   the exact direction, release mode, source/swarm parent SHA/tree, and
//!   prior accepted generation. A moved exact input yields a different ID and
//!   therefore forces a successor generation instead of an edit to an
//!   existing receipt.
//! - **Closed vocabularies.** Directions, release-context modes, lifecycle
//!   states, invalidation causes, dispositions, and permitted actions are
//!   closed serde enums. Unknown spellings fail at the deserialization
//!   boundary.
//! - **Journal-first resumability.** [`event::replay`] reconstructs current
//!   state and next legal actions from the append-only journal alone; no
//!   conversation, shell, or worktree-local state participates.
//! - **Fail-closed persistence.** Unsupported schema versions, malformed
//!   JSON, illegal transitions, concurrent active generations, live-lease
//!   conflicts, and receipt edits are refused, never degraded around.
//!
//! # What this crate must not depend on
//!
//! Tier-1 leaf: no parser, workspace, LSP/DAP/editor, async-runtime, Git
//! subprocess, or network dependencies. Only `serde` and `sha2` (plus test
//! dependencies).
//!
//! # Quick start
//!
//! ```no_run
//! use perl_sync_convergence::prelude::*;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let store = ConvergenceStore::open("path/to/convergence-state")?;
//! let tx = TransactionId::new("bridge-2026-08")?;
//! // Open the transaction, start generations, record transitions, claim
//! // leases, and replay to resume after crash or executor loss.
//! let events = store.load_journal(&tx)?;
//! let view = perl_sync_convergence::event::replay(&events)?;
//! let _next_actions = view.active_generation().map(|g| g.next_actions());
//! # Ok(())
//! # }
//! ```

/// Append-only journal and deterministic replay engine.
pub mod event;
mod generation;
mod ids;
mod invalidation;
mod lease;
mod model;
mod state;
pub mod store;

/// Re-exported core vocabulary for downstream consumers.
pub mod prelude {
    pub use crate::event::{
        ConvergenceEvent, ConvergenceView, GenerationRuntime, JOURNAL_SCHEMA_VERSION, ReplayError,
        ReplayErrorKind, is_legal_transition, permitted_writer_actions, replay,
    };
    pub use crate::generation::{
        ConvergenceGeneration, GENERATION_RECEIPT_SCHEMA_VERSION, GenerationReceiptFile,
    };
    pub use crate::ids::{GenerationId, GenerationInputs, TransactionId};
    pub use crate::invalidation::{
        InvalidationCause, InvalidationError, InvalidationRecord, StaleDescendant, StaleDisposition,
    };
    pub use crate::lease::{Lease, LeaseError, Takeover, TimestampMs};
    pub use crate::model::{
        ArtifactDigest, Direction, DurableCopyState, ImportedCommit, LandingMerge, ModelError,
        PublishedCandidate, ReleaseContextMode,
    };
    pub use crate::state::{PermittedAction, TransitionState};
    pub use crate::store::{
        ConvergenceStore, INDEX_SCHEMA_VERSION, StoreError, StoreIndexFile, TransactionIndexEntry,
    };
}

pub use event::{
    ConvergenceEvent, ConvergenceView, GenerationRuntime, JOURNAL_SCHEMA_VERSION,
    is_legal_transition, permitted_writer_actions, replay,
};
pub use generation::{
    ConvergenceGeneration, GENERATION_RECEIPT_SCHEMA_VERSION, GenerationReceiptFile,
};
pub use ids::{GenerationId, GenerationInputs, TransactionId};
pub use invalidation::{
    InvalidationCause, InvalidationError, InvalidationRecord, StaleDescendant, StaleDisposition,
};
pub use lease::{Lease, LeaseError, Takeover, TimestampMs};
pub use model::{
    ArtifactDigest, Direction, DurableCopyState, ImportedCommit, LandingMerge, ModelError,
    PublishedCandidate, ReleaseContextMode,
};
pub use state::{PermittedAction, TransitionState};
pub use store::{
    ConvergenceStore, INDEX_SCHEMA_VERSION, StoreError, StoreIndexFile, TransactionIndexEntry,
};
