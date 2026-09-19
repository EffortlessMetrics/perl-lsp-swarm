//! Writer preflight — the pure typed subject, observation, and decision
//! domain (#11633).
//!
//! This module owns the provider-neutral semantics of "is the exact writer
//! transition I am about to attempt safe to admit?":
//!
//! - [`WriterPreflightSubject`] — what is being checked (identity + intent,
//!   caller-supplied before any evidence exists);
//! - [`WriterPreflightObservationSet`] — what was observed, with absence,
//!   unsupported evidence, provider failure, staleness, and negative
//!   findings kept distinct;
//! - [`WriterPreflightDecision`] / [`WriterPreflightReason`] /
//!   [`WriterPreflightOutcome`] (`PASS`/`BLOCKED`/`ADVISORY`/`NOT_PROVEN`)
//!   — one total, pure decision over subject × observations.
//!
//! The core is **pure**: no Git, filesystem, process, shell, or network
//! calls exist anywhere under this module. Live evidence collection, the
//! `cargo xtask work preflight` command, and compatibility front doors are
//! #11634; writer-admission mutation consumption is #11635/#3982; parity,
//! race proof, and recurrence closeout are #11636.
//!
//! # Successor seams (#11633's exact successor graph)
//!
//! ```text
//! #11633 THIS domain
//! → #11634 native evidence adapters and front doors
//!     resolves WriterPreflightSubject from CLI args, gathers exactly one
//!     WriterPreflightObservationSet per subject from platform providers,
//!     calls decide() once, and renders human/JSON/explain from that single
//!     decision object.
//! → #11635 writer-admission mutation consumption (#3982)
//!     persists decision.digest() + observations, compares subject_digest
//!     immediately before mutation (compare-and-mutate), and maps
//!     PASS/ADVISORY to REUSE|RESUME|CREATE while BLOCKED/NOT_PROVEN stay
//!     refusals. This module deliberately emits no dispositions of its own.
//! → #11636 parity/race/recurrence closeout
//!     keys every parity cell by (subject digest, observation set, decision
//!     digest); determinism rules in this module make packet identity
//!     stable across platforms for equal normalized inputs.
//! ```
//!
//! # Policy boundaries honored here
//!
//! - Ambient persistent Cargo overrides ([`observation::AmbientCargoSource`])
//!   remain distinct from executor-owned process-local configuration
//!   ([`observation::ExecutorCargoPresence`], #9548).
//! - Behind-only movement, shared stash presence, and unrelated host load
//!   are advisories, never denials.
//! - Branch/path naming conventions are compared as opaque identity tokens;
//!   no machine layout or name shape becomes policy.

mod decision;
mod observation;
mod projection;

pub use decision::{
    ReasonKind, WriterPreflightDecision, WriterPreflightOutcome, WriterPreflightReason, decide,
    digest_subject,
};
pub use observation::{
    AmbientCargoOverride, AmbientCargoSource, CapacityObservation, CheckoutRelation,
    ExecutorCargoPresence, HeadState, IndexState, Observation, ObservationState,
    RemoteBranchPresence, SameCandidateWriter, StashState, WorkingTreeDisposition, WorktreeRecord,
    WriterPreflightObservationSet,
};
pub use projection::{explain, render_human};
pub use subject::{
    CapacityRequirement, ClaimIdentity, RepositoryIdentity, WRITER_PREFLIGHT_SCHEMA_VERSION,
    WriterPreflightOperation, WriterPreflightSubject,
};

#[cfg(test)]
mod tests;

mod subject;
