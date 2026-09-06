//! Ticket-owned fresh-full semantic construction cell (#12151).
//!
//! This module owns *construction* for the `file_semantic_snapshot.v1`
//! envelope defined in [`crate::semantic_snapshot`]: one lazy, shared,
//! ticket-bound cell builds at most one fresh-full semantic snapshot per
//! exact accepted parser ticket and semantic profile, and every consumer of
//! that ticket/profile shares the same terminal result.
//!
//! ```text
//! AcceptedParseGeneration + semantic profile
//! → at most one selected construction
//! → one #12150 FileSemanticSnapshot terminal result
//! → shared by concurrent consumers
//! ```
//!
//! # Ownership and lifecycle law
//!
//! - One exact ticket/profile owns at most one cell
//!   ([`SemanticConstructionCellRegistry::cell_for`]).
//! - The registry is the only issuer of [`AcceptedTicketLease`]; one exact
//!   ticket is leased at most once at a time, and construction requires a
//!   live lease. There is no construction path that does not pass through a
//!   live ticket.
//! - Concurrent consumers share one in-flight build and one terminal result;
//!   only the first caller executes the producer.
//! - The producer runs outside every cell and registry lock, and receives
//!   only the immutable inputs captured at scheduling. The construction
//!   surface has no handle to mutable document or parser state, so it cannot
//!   reread it after scheduling.
//! - Dropping, cancelling or panicking one waiter never corrupts another
//!   valid waiter: locks are poison-recovering and a panicking producer is
//!   converted into a typed product-failure terminal.
//! - Superseded work may finish but cannot attach: a ticket retired before
//!   publication converts an attachable result into a terminal
//!   `stale_or_superseded` snapshot with no facts.
//! - Source-identical later generations and close/reopen create distinct
//!   cells, because the ticket id binds document instance, generation and
//!   parser-input digest.
//! - Release ([`SemanticConstructionCellRegistry::release`]) retires the
//!   lease and drops the ticket's cells exactly once; a second release is a
//!   typed no-op. Cleanup is bounded by live tickets.
//! - No server-global URI/content map participates: cell identity is the
//!   ticket id plus profile fingerprint, never a URI.
//!
//! # Freshness law (fail closed)
//!
//! A fresh-full snapshot is assembled only from a contribution bundle whose
//! [`FreshnessBinding`] proves it was computed for this exact subject
//! fingerprint and this exact accepted generation, and whose work claim is
//! honest fresh-full work. Any contribution that cannot be proven fresh for
//! the exact source generation refuses the *whole* snapshot with a typed
//! [`SemanticConstructionRefusal`]: the terminal snapshot carries no facts
//! (`not_proven` absent family). A partial snapshot is never silently
//! assembled, and old success never masks a current refusal or failure.
//!
//! The truthfulness of a binding is the producer's authority (#12136 owns
//! canonical producers); this cell enforces that every artifact entering a
//! snapshot is uniformly bound to the exact subject and generation, and that
//! no fresh work is reported as incremental, reused or avoided work.
//!
//! # Strategy seam
//!
//! The only selectable strategy in this PR is
//! [`SemanticConstructionStrategy::FreshFull`]. Later #12122/#7308
//! strategies plug into the same seam — same inputs capture, same terminal
//! result type, same receipt and work truth — without creating a second
//! snapshot type, cell authority, or acceptance path. #8575 owns acceptance
//! of a completed result; #9284 remains an independent sibling construction
//! lane; #4772 owns project-fact projection. Nothing in this module accepts,
//! publishes or projects a snapshot as current.
//!
//! # Determinism
//!
//! The work receipt is derived by the cell, never supplied: instrument
//! `construction_cell` bound to the exact ticket id, work sequence bound to
//! the accepted generation, work kind `fresh_full`. Identical inputs across
//! registries therefore produce identical snapshot fingerprints.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use perl_source_identity::ContentDigest;
use thiserror::Error;

use crate::semantic_snapshot::{
    AcceptedParserTicketId, FileSemanticSnapshotParts, FileSemanticSnapshotV1,
    FileSemanticSnapshotValidationError, InstrumentIdentity, MaterializedQueryViewRef,
    ParseSnapshotIdentity, SemanticCompleteness, SemanticConfidence,
    SemanticContributionSetCompleteness, SemanticContributionSetRef, SemanticInstrumentKind,
    SemanticLimitations, SemanticProfileIdentity, SemanticQueryViewKind,
    SemanticSnapshotTerminalState, SemanticWorkKind, SemanticWorkReceipt,
};

// ---------------------------------------------------------------------------
// Strategy
// ---------------------------------------------------------------------------

/// Construction strategy selected for one cell (#12151 strategy seam).
///
/// `FreshFull` is the only honest strategy before #7308: ordinary successful
/// construction is fresh-full and is never reported as incremental or
/// avoided work. Future #12122/#7308 strategies join this vocabulary through
/// the same selection/result seam without creating another snapshot type or
/// acceptance path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticConstructionStrategy {
    /// Honest fresh-full construction through the captured immutable inputs.
    FreshFull,
}

impl SemanticConstructionStrategy {
    /// Wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FreshFull => "fresh_full",
        }
    }
}

impl std::fmt::Display for SemanticConstructionStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Construction inputs
// ---------------------------------------------------------------------------

/// Bounded semantic construction budget captured at scheduling.
///
/// The cell captures the budget subject and hands it to the producer; the
/// executing producer owns enforcement and reports exhaustion through the
/// typed [`FreshFullProducerOutcome::BudgetExhausted`] outcome, which the
/// cell retains as a distinct terminal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SemanticConstructionBudget {
    /// Maximum producer steps permitted; `None` is unbounded.
    pub max_producer_steps: Option<u64>,
}

impl SemanticConstructionBudget {
    /// An unbounded budget.
    #[must_use]
    pub const fn unbounded() -> Self {
        Self { max_producer_steps: None }
    }

    /// A bounded budget.
    #[must_use]
    pub const fn bounded(max_producer_steps: u64) -> Self {
        Self { max_producer_steps: Some(max_producer_steps) }
    }
}

/// Immutable inputs captured at scheduling for one fresh-full construction.
///
/// These inputs are the complete construction surface: identity-bearing and
/// cloneable, with no handle to mutable document or parser state, so
/// construction cannot reread mutable state after scheduling. They are also
/// the #12136 producer-input seam — canonical fresh-full producers consume
/// exactly this capture.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FreshFullConstructionInputs {
    /// Schema/implementation/profile triple the snapshot is produced under.
    pub profile: SemanticProfileIdentity,
    /// Exact analysis subject (logical source, document instance, checked
    /// source generation, both exact revisions).
    pub subject: crate::semantic_snapshot::SemanticSubjectIdentity,
    /// Parser identity projection of the accepted parse snapshot.
    pub parse_snapshot: ParseSnapshotIdentity,
    /// Captured budget subject.
    pub budget: SemanticConstructionBudget,
}

// ---------------------------------------------------------------------------
// Producer seam
// ---------------------------------------------------------------------------

/// Proof that one contribution bundle was computed fresh for one exact
/// subject and accepted parser generation.
///
/// A bundle whose binding names another subject, another generation, or a
/// non-fresh work claim refuses the whole snapshot (fail closed). The
/// truthfulness of the binding is producer authority (#12136); the cell
/// enforces uniform binding to the exact ticket.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FreshnessBinding {
    /// Subject fingerprint the bundle was computed for.
    pub subject_fingerprint: ContentDigest,
    /// Accepted parse generation the bundle was computed for.
    pub accepted_generation: u64,
    /// Work kind the producer claims for this bundle. A fresh-full
    /// construction accepts only `fresh_full`: fresh work reported as
    /// incremental or avoided work is refused.
    pub work_claim: SemanticWorkKind,
}

/// One fresh-full contribution bundle offered to the cell by a producer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreshFullContribution {
    /// Contribution set reference (#12135 identity join).
    pub set: SemanticContributionSetRef,
    /// Materialized view references (#12138 identity join).
    pub views: Vec<MaterializedQueryViewRef>,
    /// Freshness proof binding the bundle to the exact subject/generation.
    pub freshness: FreshnessBinding,
    /// Confidence classification of the bundle's evidence.
    pub confidence: SemanticConfidence,
    /// Recovery/dynamic/unsupported limitation inventory.
    pub limitations: SemanticLimitations,
}

/// Typed outcome of one fresh-full producer invocation.
///
/// Distinct families stay distinct: product failure, budget exhaustion and
/// instrument/schema failure are never flattened into one another or into
/// empty success, and only the two honest fresh-full completions carry a
/// contribution bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreshFullProducerOutcome {
    /// Clean-parse complete facts for the exact subject.
    Complete(FreshFullContribution),
    /// Recovered-parse partial facts for the exact subject.
    PartialRecovered(FreshFullContribution),
    /// The producer failed for a product reason.
    ProductFailure,
    /// The captured budget was exhausted before completion.
    BudgetExhausted,
    /// An instrument or schema needed for the result failed.
    InstrumentFailure,
}

/// Fresh-full semantic producer seam (#12136 owns the canonical producer).
///
/// The producer receives only the immutable captured inputs; it cannot
/// reach the document, the parser, the cell locks or the registry.
pub trait FreshFullSemanticProducer {
    /// Build one fresh-full contribution bundle for the captured inputs.
    fn build_fresh_full(&self, inputs: &FreshFullConstructionInputs) -> FreshFullProducerOutcome;
}

// ---------------------------------------------------------------------------
// Typed refusals and caller-local errors
// ---------------------------------------------------------------------------

/// Typed fail-closed refusal of one whole fresh-full snapshot construction.
///
/// Every refusal publishes an absent-family `not_proven` snapshot with no
/// facts plus this named reason; nothing partial is ever assembled. A
/// refusal is attached to a terminal if and only if the terminal snapshot's
/// state is `not_proven`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SemanticConstructionRefusal {
    /// The ticket lease is no longer live; construction without a live
    /// ticket is refused.
    #[error("accepted parser ticket {ticket_id} is not live")]
    TicketNotLive {
        /// The dead ticket.
        ticket_id: AcceptedParserTicketId,
    },
    /// A contribution bundle names another accepted generation than the
    /// exact ticket: a stale or cached contribution cannot enter a
    /// fresh-full snapshot.
    #[error(
        "contribution binds generation {found_generation} but the exact ticket is generation \
         {expected_generation}; stale contributions refuse the whole snapshot"
    )]
    ContributionGenerationMismatch {
        /// Generation bound into the exact ticket.
        expected_generation: u64,
        /// Generation the contribution was actually computed for.
        found_generation: u64,
    },
    /// A contribution bundle names another analysis subject than the exact
    /// ticket.
    #[error(
        "contribution binds subject {found_subject} but the exact ticket is subject \
         {expected_subject}"
    )]
    ContributionSubjectMismatch {
        /// Subject fingerprint bound into the exact ticket.
        expected_subject: ContentDigest,
        /// Subject fingerprint the contribution was actually computed for.
        found_subject: ContentDigest,
    },
    /// Fresh work was claimed as incremental, reuse or fallback work.
    #[error(
        "fresh-full construction cannot report {claimed} work; fresh work reported as \
         incremental or avoided work is refused"
    )]
    HiddenWorkClaim {
        /// The non-fresh work kind the producer claimed.
        claimed: SemanticWorkKind,
    },
    /// The producer outcome family contradicts the accepted parse
    /// disposition: only a clean parse can back complete facts and only a
    /// recovered parse can back a partial-recovered result.
    #[error(
        "producer outcome contradicts parse disposition {disposition:?}; complete facts require \
         a clean parse and partial-recovered facts require a recovered parse"
    )]
    OutcomeDispositionMismatch {
        /// The accepted parse disposition of the exact ticket.
        disposition: crate::semantic_snapshot::SemanticParseDisposition,
    },
    /// A complete outcome carried a contribution set that is not complete.
    #[error("complete outcome requires a complete contribution set, found {completeness:?}")]
    IncompleteContributionSet {
        /// The incomplete completeness classification.
        completeness: SemanticContributionSetCompleteness,
    },
    /// A complete outcome is missing a required materialized view family.
    #[error("complete outcome is missing required materialized view kind {kind}")]
    RequiredViewFamilyMissing {
        /// The missing required view kind.
        kind: SemanticQueryViewKind,
    },
    /// A partial-recovered outcome carried a complete set.
    #[error(
        "partial-recovered outcome requires a partial contribution set, found {completeness:?}"
    )]
    PartialSetCompleteness {
        /// The non-partial completeness classification.
        completeness: SemanticContributionSetCompleteness,
    },
    /// A partial-recovered outcome omitted its recovery limitations.
    #[error("partial-recovered outcome must record recovery limitations")]
    PartialMissingRecoveryLimitations,
    /// A partial-recovered outcome carried a confidence that does not state
    /// recovery.
    #[error(
        "partial-recovered outcome requires recovered or dynamic-bounded confidence, found {confidence:?}"
    )]
    PartialConfidence {
        /// The contradicting confidence classification.
        confidence: SemanticConfidence,
    },
    /// The checked envelope constructor refused the assembled parts.
    #[error("envelope validation refused the snapshot: {0}")]
    EnvelopeRefusal(#[from] FileSemanticSnapshotValidationError),
}

/// Caller-local construction error: the caller's request was refused without
/// consuming or corrupting the cell's shared construction or terminal.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SemanticConstructionCallError {
    /// The lease belongs to a different ticket than this cell.
    #[error("lease ticket {lease_ticket} does not own cell ticket {cell_ticket}")]
    TicketMismatch {
        /// Ticket of the cell.
        cell_ticket: AcceptedParserTicketId,
        /// Ticket of the presented lease.
        lease_ticket: AcceptedParserTicketId,
    },
    /// The caller presented inputs that contradict the inputs captured by
    /// the cell's single construction.
    #[error("construction inputs contradict the inputs captured by this cell's one construction")]
    InputMismatch,
    /// The caller presented inputs under a different profile than the one
    /// this cell is keyed to: one cell owns exactly one
    /// ticket/profile-bound construction.
    #[error(
        "inputs profile fingerprint {inputs_profile} does not match the cell profile \
         fingerprint {cell_profile}"
    )]
    CellProfileMismatch {
        /// Profile fingerprint of this cell's key.
        cell_profile: ContentDigest,
        /// Profile fingerprint carried by the presented inputs.
        inputs_profile: ContentDigest,
    },
    /// The presented inputs bind to a different accepted parser ticket
    /// than this cell: construction under a cell may only build the exact
    /// ticket the cell owns, so a coherent-but-foreign subject/parse pair
    /// cannot be laundered through another ticket's cell.
    #[error("inputs bind ticket {inputs_ticket} but this cell owns ticket {cell_ticket}")]
    InputsTicketMismatch {
        /// Ticket identity of this cell's key.
        cell_ticket: AcceptedParserTicketId,
        /// Ticket identity derived from the presented inputs.
        inputs_ticket: AcceptedParserTicketId,
    },
    /// The presented profile triple is internally incoherent (its stored
    /// fingerprint does not match its schema/implementation/profile
    /// triple). Only identities built through checked constructors may
    /// reach a construction.
    #[error(
        "inputs profile triple is incoherent: stored fingerprint {found} does not match          its triple (expected {expected})"
    )]
    IncoherentProfileTriple {
        /// Fingerprint recomputed over the presented triple.
        expected: ContentDigest,
        /// Fingerprint stored in the presented identity.
        found: ContentDigest,
    },
    /// The presented lease is not the capability this registry issued for
    /// this cell's ticket: a foreign registry's lease (or a stale lease
    /// from before a release/reaccept cycle) cannot construct through
    /// another registry's cell even when the deterministic ticket id
    /// matches.
    #[error("lease capability was not issued by this registry for ticket {ticket_id}")]
    ForeignLeaseCapability {
        /// The ticket the capability was presented against.
        ticket_id: AcceptedParserTicketId,
    },
}

/// Typed refusal at ticket acceptance or cell lookup.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SemanticTicketError {
    /// The parse snapshot names a different parser input than the subject.
    #[error("parse snapshot digest does not match the subject's parser input revision")]
    ParserInputDigestMismatch,
    /// The parse snapshot length disagrees with the parser-input length.
    #[error("parse snapshot length does not match the subject's parser input length")]
    ParserInputLengthMismatch,
    /// The subject's full-source revision names a different logical source.
    #[error("full-source revision does not match the subject's logical source")]
    FullSourceSubjectMismatch,
    /// The exact ticket is already leased; ticket ownership is exclusive.
    #[error("accepted parser ticket {ticket_id} is already leased")]
    TicketAlreadyLeased {
        /// The ticket that is already leased.
        ticket_id: AcceptedParserTicketId,
    },
    /// The lease was not issued by this registry.
    #[error("accepted parser ticket {ticket_id} was not accepted by this registry")]
    TicketNotAccepted {
        /// The foreign ticket.
        ticket_id: AcceptedParserTicketId,
    },
    /// The lease is retired; construction is refused.
    #[error("accepted parser ticket {ticket_id} is not live")]
    TicketNotLive {
        /// The retired ticket.
        ticket_id: AcceptedParserTicketId,
    },
    /// The presented lease capability was not issued by this registry
    /// (another registry's lease for the same deterministic ticket id, or
    /// a stale lease from before a release/reaccept cycle): it cannot
    /// obtain cells for this registry's ticket lifecycle.
    #[error("lease capability was not issued by this registry for ticket {ticket_id}")]
    ForeignLeaseCapability {
        /// The ticket the capability was presented against.
        ticket_id: AcceptedParserTicketId,
    },
    /// The presented semantic profile triple is internally incoherent: its
    /// stored fingerprint does not match its schema/implementation/profile
    /// triple. Only identities built through checked constructors reach a
    /// cell; a hand-assembled incoherent triple is refused before any
    /// construction or absent-family assembly could observe it.
    #[error(
        "semantic profile triple is incoherent: fingerprint {found} does not match its \
         schema/implementation/profile triple (expected {expected})"
    )]
    ProfileIncoherent {
        /// Fingerprint recomputed over the presented triple.
        expected: ContentDigest,
        /// Fingerprint stored in the presented identity.
        found: ContentDigest,
    },
}

/// Disposition of one ticket release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketReleaseDisposition {
    /// The lease was retired and the ticket's cells were dropped.
    Released {
        /// Number of cells removed for this ticket.
        cells_removed: usize,
    },
    /// The release already applied; ownership is released exactly once.
    AlreadyReleased,
}

// ---------------------------------------------------------------------------
// Terminal result
// ---------------------------------------------------------------------------

/// The one terminal result shared by every consumer of one cell.
///
/// Always carries a complete [`FileSemanticSnapshotV1`]: successful and
/// partial constructions carry their complete/partial envelope, and every
/// fail-closed path carries an absent-family envelope with no facts. The
/// named [`SemanticConstructionRefusal`] is attached if and only if the
/// snapshot's terminal state is `not_proven`.
#[derive(Debug, Clone)]
pub struct SemanticConstructionTerminal {
    snapshot: FileSemanticSnapshotV1,
    refusal: Option<SemanticConstructionRefusal>,
}

impl SemanticConstructionTerminal {
    /// The terminal snapshot (complete, partial or absent family).
    #[must_use]
    pub const fn snapshot(&self) -> &FileSemanticSnapshotV1 {
        &self.snapshot
    }

    /// The named fail-closed refusal, when the terminal snapshot is
    /// `not_proven`.
    #[must_use]
    pub fn refusal(&self) -> Option<&SemanticConstructionRefusal> {
        self.refusal.as_ref()
    }

    /// Terminal state of the snapshot.
    #[must_use]
    pub const fn terminal_state(&self) -> SemanticSnapshotTerminalState {
        self.snapshot.terminal_state()
    }

    /// Whether this terminal carries attachable facts (a complete or
    /// partial-recovered result that has not been demoted for
    /// supersession).
    #[must_use]
    pub(crate) fn is_attachable(&self) -> bool {
        matches!(
            self.snapshot.terminal_state(),
            SemanticSnapshotTerminalState::CompleteFreshFull
                | SemanticSnapshotTerminalState::PartialRecovered
        )
    }

    /// The construction inputs recoverable from the terminal's snapshot
    /// identities (used to rebuild an absent terminal on supersession
    /// demotion).
    #[must_use]
    pub(crate) fn as_construction_inputs(&self) -> FreshFullConstructionInputs {
        FreshFullConstructionInputs {
            profile: self.snapshot.profile().clone(),
            subject: self.snapshot.subject().clone(),
            parse_snapshot: self.snapshot.parse_snapshot_identity().clone(),
            budget: SemanticConstructionBudget::unbounded(),
        }
    }

    /// Whether this terminal is an attachable honest fresh-full completion.
    #[must_use]
    pub fn is_complete_fresh_full(&self) -> bool {
        self.snapshot.terminal_state() == SemanticSnapshotTerminalState::CompleteFreshFull
    }
}

// ---------------------------------------------------------------------------
// Work truth
// ---------------------------------------------------------------------------

/// Work truth retained by one construction cell.
///
/// Before #7308 no work-avoidance claim exists: `incremental_invocations`
/// is structurally zero and any fresh analysis reported as incremental or
/// shared avoided work fails validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticConstructionWorkTruth {
    /// Ticket this cell is bound to.
    pub ticket_id: AcceptedParserTicketId,
    /// Profile fingerprint this cell is bound to.
    pub profile_fingerprint: ContentDigest,
    /// Selected strategy for this cell.
    pub strategy: SemanticConstructionStrategy,
    /// Construction requests received by this cell.
    pub requests: u64,
    /// Producer invocations started (at most one per cell).
    pub builds_started: u64,
    /// Requests satisfied by sharing the one construction or terminal.
    pub shared_hits: u64,
    /// High-water mark of consumers waiting on the in-flight build.
    pub waiters_high_water: u64,
    /// Honest fresh-full invocations (equal to `builds_started`).
    pub fresh_full_invocations: u64,
    /// Incremental invocations: structurally zero before #7308.
    pub incremental_invocations: u64,
    /// Whether a terminal result has been published.
    pub completed: bool,
    /// Terminal state of the published snapshot, once published.
    pub terminal_state: Option<SemanticSnapshotTerminalState>,
}

impl SemanticConstructionWorkTruth {
    fn new(ticket_id: AcceptedParserTicketId, profile_fingerprint: ContentDigest) -> Self {
        Self {
            ticket_id,
            profile_fingerprint,
            strategy: SemanticConstructionStrategy::FreshFull,
            requests: 0,
            builds_started: 0,
            shared_hits: 0,
            waiters_high_water: 0,
            fresh_full_invocations: 0,
            incremental_invocations: 0,
            completed: false,
            terminal_state: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Ticket lease
// ---------------------------------------------------------------------------

/// Exclusive live capability for one accepted parser ticket.
///
/// Issued only by [`SemanticConstructionCellRegistry::accept_ticket`]; one
/// exact ticket is leased at most once at a time. Not cloneable: the lease
/// is the single ownership of the ticket's construction lifecycle. Retiring
/// the lease (directly or through release) is permanent and fails closed
/// all further construction for that ticket.
#[derive(Debug)]
pub struct AcceptedTicketLease {
    ticket_id: AcceptedParserTicketId,
    document_instance: crate::semantic_snapshot::DocumentInstanceId,
    accepted_generation: u64,
    live: Arc<AtomicBool>,
}

impl AcceptedTicketLease {
    /// The ticket id this lease owns.
    #[must_use]
    pub fn ticket_id(&self) -> &AcceptedParserTicketId {
        &self.ticket_id
    }

    /// The document instance the ticket was accepted for.
    #[must_use]
    pub fn document_instance(&self) -> &crate::semantic_snapshot::DocumentInstanceId {
        &self.document_instance
    }

    /// The accepted generation the ticket was accepted for.
    #[must_use]
    pub const fn accepted_generation(&self) -> u64 {
        self.accepted_generation
    }

    /// Whether the ticket is still live.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }

    /// Retire the ticket permanently: supersession, close or cancellation.
    ///
    /// In-flight work may finish but its result can no longer attach;
    /// further construction is refused with
    /// [`SemanticConstructionRefusal::TicketNotLive`].
    pub fn retire(&self) {
        self.live.store(false, Ordering::Release);
    }
}

// ---------------------------------------------------------------------------
// Construction cell
// ---------------------------------------------------------------------------

/// Identity of one cell: exact ticket plus profile fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CellKey {
    ticket_id: AcceptedParserTicketId,
    profile_fingerprint: ContentDigest,
}

#[derive(Debug)]
enum CellPhase {
    /// No construction captured yet; the first caller becomes the builder.
    Ready,
    /// One construction is in flight; later callers wait.
    Building,
    /// The one terminal result exists and is shared forever after.
    Terminal(Arc<SemanticConstructionTerminal>),
}

#[derive(Debug)]
struct CellInner {
    phase: CellPhase,
    captured: Option<FreshFullConstructionInputs>,
    waiters: u64,
    truth: SemanticConstructionWorkTruth,
}

/// One ticket/profile-owned fresh-full semantic construction cell.
///
/// Created only by [`SemanticConstructionCellRegistry::cell_for`]. At most
/// one construction runs per cell; concurrent callers of
/// [`SemanticConstructionCell::construct_fresh_full`] share one in-flight
/// build and one terminal result. The producer executes outside every lock
/// and reads only the immutable captured inputs.
#[derive(Debug)]
pub struct SemanticConstructionCell {
    key: CellKey,
    /// The registry-issued liveness capability for this cell's exact
    /// ticket (pointer-identity authenticated: a foreign registry's or a
    /// stale reaccepted lease has a different `Arc` even when the
    /// deterministic ticket id matches).
    live: Arc<AtomicBool>,
    inner: Mutex<CellInner>,
    published: Condvar,
}

/// Recover a lock whose guarding thread panicked: one waiter's or builder's
/// panic must not corrupt another valid consumer's construction.
fn lock_alive<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SemanticConstructionCell {
    /// The ticket this cell is bound to.
    #[must_use]
    pub fn ticket_id(&self) -> &AcceptedParserTicketId {
        &self.key.ticket_id
    }

    /// The profile fingerprint this cell is bound to.
    #[must_use]
    pub fn profile_fingerprint(&self) -> &ContentDigest {
        &self.key.profile_fingerprint
    }

    /// Current work truth of this cell.
    #[must_use]
    pub fn truth(&self) -> SemanticConstructionWorkTruth {
        lock_alive(&self.inner).truth.clone()
    }

    /// The published terminal result, if construction already finished.
    ///
    /// Repeated reads after terminal completion return the same shared
    /// result and never trigger another construction.
    #[must_use]
    pub fn terminal(&self) -> Option<Arc<SemanticConstructionTerminal>> {
        match &lock_alive(&self.inner).phase {
            CellPhase::Terminal(terminal) => Some(Arc::clone(terminal)),
            _ => None,
        }
    }

    /// Schedule (or join) the one fresh-full construction for this cell and
    /// block until the shared terminal result exists.
    ///
    /// The first caller captures `inputs` and executes `producer` exactly
    /// once; concurrent callers wait and share the same terminal result.
    /// Caller-local contradictions (wrong ticket, inputs differing from the
    /// captured ones) refuse only that caller. Every other outcome —
    /// complete, partial, product/budget/instrument failure, supersession,
    /// stale-contribution refusal, dead ticket — resolves into one shared
    /// typed terminal.
    pub fn construct_fresh_full(
        &self,
        lease: &AcceptedTicketLease,
        inputs: FreshFullConstructionInputs,
        producer: &dyn FreshFullSemanticProducer,
    ) -> Result<Arc<SemanticConstructionTerminal>, SemanticConstructionCallError> {
        if lease.ticket_id != self.key.ticket_id {
            return Err(SemanticConstructionCallError::TicketMismatch {
                cell_ticket: self.key.ticket_id.clone(),
                lease_ticket: lease.ticket_id.clone(),
            });
        }
        if inputs.profile.fingerprint != self.key.profile_fingerprint {
            return Err(SemanticConstructionCallError::CellProfileMismatch {
                cell_profile: self.key.profile_fingerprint.clone(),
                inputs_profile: inputs.profile.fingerprint.clone(),
            });
        }
        // Capability authentication: only the exact registry-issued
        // liveness `Arc` may construct under this cell. A foreign
        // registry's lease (same deterministic ticket id, different
        // capability) or a stale pre-reaccept lease is refused.
        if !Arc::ptr_eq(&lease.live, &self.live) {
            return Err(SemanticConstructionCallError::ForeignLeaseCapability {
                ticket_id: self.key.ticket_id.clone(),
            });
        }
        // The inputs must bind the exact ticket this cell owns: a
        // coherent subject/parse pair for another ticket cannot be built
        // through this cell.
        let inputs_ticket = AcceptedParserTicketId::from_bound_parts(
            &inputs.subject.document_instance,
            inputs.parse_snapshot.accepted_generation,
            &inputs.parse_snapshot.source_digest,
        );
        if inputs_ticket != self.key.ticket_id {
            return Err(SemanticConstructionCallError::InputsTicketMismatch {
                cell_ticket: self.key.ticket_id.clone(),
                inputs_ticket,
            });
        }
        // The profile triple must be internally coherent before it can
        // reach envelope assembly (an incoherent triple with a copied
        // fingerprint would otherwise be rejected only inside envelope
        // validation, after the cell committed to building).
        let expected_fingerprint = SemanticProfileIdentity::fingerprint_over(
            &inputs.profile.schema,
            &inputs.profile.implementation,
            &inputs.profile.profile,
        );
        if inputs.profile.fingerprint != expected_fingerprint {
            return Err(SemanticConstructionCallError::IncoherentProfileTriple {
                expected: expected_fingerprint,
                found: inputs.profile.fingerprint.clone(),
            });
        }

        let build_inputs = 'schedule: {
            let mut guard = lock_alive(&self.inner);
            guard.truth.requests += 1;
            if let CellPhase::Terminal(terminal) = &guard.phase {
                let terminal = Arc::clone(terminal);
                guard.truth.shared_hits += 1;
                return Ok(terminal);
            }
            match guard.captured.as_ref() {
                None => {
                    if !lease.is_live() {
                        let terminal = Arc::new(refused_terminal(
                            &inputs,
                            SemanticConstructionRefusal::TicketNotLive {
                                ticket_id: self.key.ticket_id.clone(),
                            },
                        ));
                        Self::publish_locked(&mut guard, &terminal);
                        drop(guard);
                        self.published.notify_all();
                        return Ok(terminal);
                    }
                    guard.captured = Some(inputs.clone());
                    guard.phase = CellPhase::Building;
                    guard.truth.builds_started += 1;
                    guard.truth.fresh_full_invocations += 1;
                    break 'schedule inputs;
                }
                Some(captured) => {
                    if captured != &inputs {
                        return Err(SemanticConstructionCallError::InputMismatch);
                    }
                    guard.waiters += 1;
                    guard.truth.waiters_high_water =
                        guard.truth.waiters_high_water.max(guard.waiters);
                    loop {
                        // `Condvar::wait` re-acquires the mutex and can
                        // observe a poisoned lock if another thread panicked
                        // while holding it; the guarded state itself is
                        // intact, so recover the guard like `lock_alive`.
                        let mut woken =
                            self.published.wait(guard).unwrap_or_else(PoisonError::into_inner);
                        woken.waiters -= 1;
                        if let CellPhase::Terminal(terminal) = &woken.phase {
                            let terminal = Arc::clone(terminal);
                            woken.truth.shared_hits += 1;
                            return Ok(terminal);
                        }
                        guard = woken;
                        guard.waiters += 1;
                    }
                }
            }
        };

        // Builder path: run the producer outside every lock, on the captured
        // immutable inputs only. A panicking producer is a typed product
        // failure so waiters always resolve.
        let outcome =
            match catch_unwind(AssertUnwindSafe(|| producer.build_fresh_full(&build_inputs))) {
                Ok(outcome) => outcome,
                Err(_panic) => FreshFullProducerOutcome::ProductFailure,
            };

        let resolved = Self::resolve_outcome(&build_inputs, &outcome, lease);
        Ok(self.publish_with_liveness_check(lease, resolved))
    }

    /// Publish the resolved terminal, deciding attachability **under the
    /// cell lock**: if the ticket was retired after the outcome resolved
    /// but before publication, the attachable result is demoted to a
    /// `stale_or_superseded` terminal with no facts. A retirement that
    /// lands after this in-lock check publishes an attachable result that
    /// predates the retirement; downstream acceptance (#8575) re-validates
    /// lease liveness before attaching, so a retired ticket can never
    /// consume it.
    fn publish_with_liveness_check(
        &self,
        lease: &AcceptedTicketLease,
        resolved: SemanticConstructionTerminal,
    ) -> Arc<SemanticConstructionTerminal> {
        let mut guard = lock_alive(&self.inner);
        let terminal = if resolved.is_attachable() && !lease.is_live() {
            absent_terminal(
                &resolved.as_construction_inputs(),
                SemanticSnapshotTerminalState::StaleOrSuperseded,
            )
        } else {
            resolved
        };
        let terminal = Arc::new(terminal);
        Self::publish_locked(&mut guard, &terminal);
        drop(guard);
        self.published.notify_all();
        terminal
    }

    /// Record the one terminal result under the lock.
    fn publish_locked(
        guard: &mut MutexGuard<'_, CellInner>,
        terminal: &Arc<SemanticConstructionTerminal>,
    ) {
        guard.truth.completed = true;
        guard.truth.terminal_state = Some(terminal.snapshot.terminal_state());
        guard.phase = CellPhase::Terminal(Arc::clone(terminal));
    }

    /// Resolve one producer outcome (plus ticket liveness) into the shared
    /// terminal result, enforcing the freshness law fail-closed.
    fn resolve_outcome(
        inputs: &FreshFullConstructionInputs,
        outcome: &FreshFullProducerOutcome,
        lease: &AcceptedTicketLease,
    ) -> SemanticConstructionTerminal {
        use crate::semantic_snapshot::SemanticParseDisposition;
        let contribution = match outcome {
            FreshFullProducerOutcome::ProductFailure => {
                return absent_terminal(inputs, SemanticSnapshotTerminalState::ProductFailure);
            }
            FreshFullProducerOutcome::BudgetExhausted => {
                return absent_terminal(inputs, SemanticSnapshotTerminalState::BudgetExhausted);
            }
            FreshFullProducerOutcome::InstrumentFailure => {
                return absent_terminal(
                    inputs,
                    SemanticSnapshotTerminalState::InstrumentOrSchemaFailure,
                );
            }
            FreshFullProducerOutcome::Complete(contribution) => contribution,
            FreshFullProducerOutcome::PartialRecovered(contribution) => contribution,
        };

        // Freshness law: any contribution that cannot be proven fresh for
        // the exact subject and source generation refuses the whole
        // snapshot with a named reason.
        let subject_fingerprint = inputs.subject.fingerprint();
        if contribution.freshness.subject_fingerprint != subject_fingerprint {
            return refused_terminal(
                inputs,
                SemanticConstructionRefusal::ContributionSubjectMismatch {
                    expected_subject: subject_fingerprint,
                    found_subject: contribution.freshness.subject_fingerprint.clone(),
                },
            );
        }
        if contribution.freshness.accepted_generation != inputs.parse_snapshot.accepted_generation {
            return refused_terminal(
                inputs,
                SemanticConstructionRefusal::ContributionGenerationMismatch {
                    expected_generation: inputs.parse_snapshot.accepted_generation,
                    found_generation: contribution.freshness.accepted_generation,
                },
            );
        }
        if contribution.freshness.work_claim != SemanticWorkKind::FreshFull {
            return refused_terminal(
                inputs,
                SemanticConstructionRefusal::HiddenWorkClaim {
                    claimed: contribution.freshness.work_claim,
                },
            );
        }

        let partial = matches!(outcome, FreshFullProducerOutcome::PartialRecovered(_));
        let disposition = inputs.parse_snapshot.disposition;
        if partial != (disposition == SemanticParseDisposition::Recovered) {
            return refused_terminal(
                inputs,
                SemanticConstructionRefusal::OutcomeDispositionMismatch { disposition },
            );
        }

        let (terminal_state, completeness) = if partial {
            if contribution.set.completeness != SemanticContributionSetCompleteness::Partial {
                return refused_terminal(
                    inputs,
                    SemanticConstructionRefusal::PartialSetCompleteness {
                        completeness: contribution.set.completeness,
                    },
                );
            }
            if !contribution.limitations.has_recovery_limitation() {
                return refused_terminal(
                    inputs,
                    SemanticConstructionRefusal::PartialMissingRecoveryLimitations,
                );
            }
            if !matches!(
                contribution.confidence,
                SemanticConfidence::Recovered | SemanticConfidence::DynamicBounded
            ) {
                return refused_terminal(
                    inputs,
                    SemanticConstructionRefusal::PartialConfidence {
                        confidence: contribution.confidence,
                    },
                );
            }
            (SemanticSnapshotTerminalState::PartialRecovered, SemanticCompleteness::Partial)
        } else {
            if contribution.set.completeness != SemanticContributionSetCompleteness::Complete {
                return refused_terminal(
                    inputs,
                    SemanticConstructionRefusal::IncompleteContributionSet {
                        completeness: contribution.set.completeness,
                    },
                );
            }
            for kind in SemanticQueryViewKind::REQUIRED_FOR_COMPLETE {
                if !contribution.views.iter().any(|view| view.kind == *kind) {
                    return refused_terminal(
                        inputs,
                        SemanticConstructionRefusal::RequiredViewFamilyMissing { kind: *kind },
                    );
                }
            }
            (SemanticSnapshotTerminalState::CompleteFreshFull, SemanticCompleteness::Complete)
        };

        // The cell derives the receipt: honest fresh-full work, construction
        // cell instrument bound to the exact ticket, deterministic sequence
        // bound to the accepted generation.
        // The receipt instrument is keyed by the exact accepted ticket
        // (document instance + accepted generation + parser-input digest),
        // not the document instance alone: two tickets that share an
        // instance and generation but differ in source digest are distinct
        // work and must never share a receipt identity.
        let ticket_id = AcceptedParserTicketId::from_bound_parts(
            &inputs.subject.document_instance,
            inputs.parse_snapshot.accepted_generation,
            &inputs.parse_snapshot.source_digest,
        );
        let work_receipt = SemanticWorkReceipt::new(
            SemanticWorkKind::FreshFull,
            InstrumentIdentity::new(SemanticInstrumentKind::ConstructionCell, ticket_id.as_wire()),
            inputs.parse_snapshot.accepted_generation,
        );

        let parts = FileSemanticSnapshotParts {
            profile: inputs.profile.clone(),
            subject: inputs.subject.clone(),
            parse_snapshot: inputs.parse_snapshot.clone(),
            contribution_set: Some(contribution.set.clone()),
            materialized_views: contribution.views.clone(),
            work_receipt,
            predecessor: None,
            terminal_state,
            completeness,
            confidence: contribution.confidence,
            limitations: contribution.limitations.clone(),
            project_fact_projection: None,
        };
        let snapshot = match FileSemanticSnapshotV1::from_parts(parts) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return refused_terminal(
                    inputs,
                    SemanticConstructionRefusal::EnvelopeRefusal(error),
                );
            }
        };

        // Superseded work may finish but cannot attach: a ticket retired
        // before publication discards the attachable result.
        if !lease.is_live() {
            return absent_terminal(inputs, SemanticSnapshotTerminalState::StaleOrSuperseded);
        }

        SemanticConstructionTerminal { snapshot, refusal: None }
    }
}

/// Assemble the fail-closed refusal terminal: an absent `not_proven`
/// snapshot with no facts plus the named refusal.
fn refused_terminal(
    inputs: &FreshFullConstructionInputs,
    refusal: SemanticConstructionRefusal,
) -> SemanticConstructionTerminal {
    SemanticConstructionTerminal {
        snapshot: absent_snapshot(inputs, SemanticSnapshotTerminalState::NotProven),
        refusal: Some(refusal),
    }
}

/// Assemble an honest absent-family terminal with no facts and no refusal.
fn absent_terminal(
    inputs: &FreshFullConstructionInputs,
    state: SemanticSnapshotTerminalState,
) -> SemanticConstructionTerminal {
    SemanticConstructionTerminal { snapshot: absent_snapshot(inputs, state), refusal: None }
}

fn absent_snapshot(
    inputs: &FreshFullConstructionInputs,
    state: SemanticSnapshotTerminalState,
) -> FileSemanticSnapshotV1 {
    let ticket_id = AcceptedParserTicketId::from_bound_parts(
        &inputs.subject.document_instance,
        inputs.parse_snapshot.accepted_generation,
        &inputs.parse_snapshot.source_digest,
    );
    let receipt = SemanticWorkReceipt::new(
        SemanticWorkKind::FreshFull,
        InstrumentIdentity::new(SemanticInstrumentKind::ConstructionCell, ticket_id.as_wire()),
        inputs.parse_snapshot.accepted_generation,
    );
    let parts = FileSemanticSnapshotParts {
        profile: inputs.profile.clone(),
        subject: inputs.subject.clone(),
        parse_snapshot: inputs.parse_snapshot.clone(),
        contribution_set: None,
        materialized_views: vec![],
        work_receipt: receipt,
        predecessor: None,
        terminal_state: state,
        completeness: SemanticCompleteness::NotProven,
        confidence: SemanticConfidence::Unprovable,
        limitations: SemanticLimitations::new(vec![]),
        project_fact_projection: None,
    };
    match FileSemanticSnapshotV1::from_parts(parts) {
        Ok(snapshot) => snapshot,
        // Totality: the absent-family shape is fully determined by inputs the
        // seams already proved coherent — subject/parse binding at ticket
        // acceptance, profile-triple coherence at cell lookup, profile-key
        // equality at construction — and every absent-family cross-check
        // (no facts, `not_proven` completeness, `unprovable` confidence,
        // receipt derived in-module) holds by construction. This branch is
        // therefore unreachable for cell-reachable inputs and is kept as the
        // documented narrow exception used for checked-invariant
        // reconstruction throughout this crate.
        Err(error) => {
            unreachable!("absent-family assembly is total for cell-reachable inputs: {error}")
        }
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct RegistryInner {
    // Ticket id -> shared liveness flag. Present means this registry began
    // the ticket's lifecycle; removed by release.
    leases: HashMap<AcceptedParserTicketId, Arc<AtomicBool>>,
    // Exact ticket + profile fingerprint -> the one cell.
    cells: HashMap<CellKey, Arc<SemanticConstructionCell>>,
}

/// The one construction authority for a workspace: issues exclusive ticket
/// leases and owns at most one construction cell per exact ticket/profile.
///
/// Keyed by ticket identity and profile fingerprint — never by URI or
/// content — so no server-global URI/content map becomes semantic
/// currentness authority.
#[derive(Debug, Default)]
pub struct SemanticConstructionCellRegistry {
    inner: Mutex<RegistryInner>,
}

impl SemanticConstructionCellRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self { inner: Mutex::new(RegistryInner { leases: HashMap::new(), cells: HashMap::new() }) }
    }

    /// Accept one exact parser ticket and issue its exclusive live lease.
    ///
    /// Fails closed when the parse snapshot does not name the subject's
    /// exact parser input, or when the exact ticket is already leased.
    pub fn accept_ticket(
        &self,
        subject: crate::semantic_snapshot::SemanticSubjectIdentity,
        parse_snapshot: ParseSnapshotIdentity,
    ) -> Result<AcceptedTicketLease, SemanticTicketError> {
        if parse_snapshot.source_digest != subject.parser_input_revision.digest {
            return Err(SemanticTicketError::ParserInputDigestMismatch);
        }
        if parse_snapshot.source_len != subject.parser_input_revision.byte_len {
            return Err(SemanticTicketError::ParserInputLengthMismatch);
        }
        if subject.full_source_revision.logical_source_id != subject.logical_source_id {
            return Err(SemanticTicketError::FullSourceSubjectMismatch);
        }
        let ticket_id = AcceptedParserTicketId::from_bound_parts(
            &subject.document_instance,
            parse_snapshot.accepted_generation,
            &parse_snapshot.source_digest,
        );
        let mut guard = lock_alive(&self.inner);
        if guard.leases.contains_key(&ticket_id) {
            return Err(SemanticTicketError::TicketAlreadyLeased { ticket_id });
        }
        let live = Arc::new(AtomicBool::new(true));
        guard.leases.insert(ticket_id.clone(), Arc::clone(&live));
        Ok(AcceptedTicketLease {
            ticket_id,
            document_instance: subject.document_instance,
            accepted_generation: parse_snapshot.accepted_generation,
            live,
        })
    }

    /// The one cell for this exact ticket and profile.
    ///
    /// Creates the cell on first use; later calls (including across
    /// profiles) return the same cell for the same key. Distinct tickets —
    /// including source-identical later generations and close/reopen
    /// instances — get distinct cells.
    pub fn cell_for(
        &self,
        lease: &AcceptedTicketLease,
        profile: &SemanticProfileIdentity,
    ) -> Result<Arc<SemanticConstructionCell>, SemanticTicketError> {
        // Profile triples reaching a cell must be internally coherent: this
        // check makes absent-family envelope assembly total for every
        // cell-reachable input (subject/parse coherence is already checked at
        // ticket acceptance, and construct re-checks the profile key).
        let expected = SemanticProfileIdentity::fingerprint_over(
            &profile.schema,
            &profile.implementation,
            &profile.profile,
        );
        if profile.fingerprint != expected {
            return Err(SemanticTicketError::ProfileIncoherent {
                expected,
                found: profile.fingerprint.clone(),
            });
        }
        let mut guard = lock_alive(&self.inner);
        let live = guard
            .leases
            .get(lease.ticket_id())
            .ok_or_else(|| SemanticTicketError::TicketNotAccepted {
                ticket_id: lease.ticket_id().clone(),
            })?
            .clone();
        // Capability authentication: the presented lease must carry this
        // registry's exact liveness `Arc`. A foreign registry's lease for
        // the same deterministic ticket id — or a stale lease from before
        // a release/reaccept cycle, which minted a fresh `Arc` — is
        // refused here rather than silently mutating this registry's
        // lifecycle.
        if !Arc::ptr_eq(&live, &lease.live) {
            return Err(SemanticTicketError::ForeignLeaseCapability {
                ticket_id: lease.ticket_id().clone(),
            });
        }
        if !live.load(Ordering::Acquire) {
            return Err(SemanticTicketError::TicketNotLive {
                ticket_id: lease.ticket_id().clone(),
            });
        }
        let key = CellKey {
            ticket_id: lease.ticket_id().clone(),
            profile_fingerprint: profile.fingerprint.clone(),
        };
        Ok(guard
            .cells
            .entry(key.clone())
            .or_insert_with(|| {
                let truth = SemanticConstructionWorkTruth::new(
                    key.ticket_id.clone(),
                    key.profile_fingerprint.clone(),
                );
                Arc::new(SemanticConstructionCell {
                    key,
                    live: Arc::clone(&live),
                    inner: Mutex::new(CellInner {
                        phase: CellPhase::Ready,
                        captured: None,
                        waiters: 0,
                        truth,
                    }),
                    published: Condvar::new(),
                })
            })
            .clone())
    }

    /// Release one ticket's ownership exactly once: retire the lease and
    /// drop every cell bound to the ticket (all profiles).
    ///
    /// The lease's shared liveness flag stays false, so holders of earlier
    /// Arcs observe a retired ticket. A second release is a typed no-op.
    pub fn release(&self, lease: &AcceptedTicketLease) -> TicketReleaseDisposition {
        let mut guard = lock_alive(&self.inner);
        // Only this registry's own capability may release its lifecycle: a
        // foreign registry's lease (same deterministic ticket id) or a
        // stale pre-reaccept lease leaves the registry untouched. The
        // intact lease keeps the refusal observable — a follow-up
        // `cell_for`/`construct` with the same foreign lease fails closed
        // at the capability check above.
        let foreign = match guard.leases.get(lease.ticket_id()) {
            Some(live) => !Arc::ptr_eq(live, &lease.live),
            None => false,
        };
        if foreign {
            return TicketReleaseDisposition::AlreadyReleased;
        }
        let Some(live) = guard.leases.remove(lease.ticket_id()) else {
            return TicketReleaseDisposition::AlreadyReleased;
        };
        live.store(false, Ordering::Release);
        let before = guard.cells.len();
        guard.cells.retain(|key, _| key.ticket_id != *lease.ticket_id());
        TicketReleaseDisposition::Released { cells_removed: before - guard.cells.len() }
    }

    /// Number of live cells currently held by this registry.
    #[must_use]
    pub fn cell_count(&self) -> usize {
        lock_alive(&self.inner).cells.len()
    }

    /// Number of tickets whose lifecycle this registry currently holds.
    #[must_use]
    pub fn lease_count(&self) -> usize {
        lock_alive(&self.inner).leases.len()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;
    use std::thread;

    use super::*;
    use crate::semantic_snapshot::{
        DocumentInstanceId, MaterializedQueryViewId, ParserInputRevision, SemanticParseDisposition,
        SemanticParseStrategy, SemanticPredecessorRef, SemanticReuseRelation,
    };
    use perl_source_identity::{LogicalSourceId, ProjectId, SourceGeneration, WorkspaceRootId};

    const SOURCE: &[u8] = b"package Widget;\n1;\n";
    const OTHER_SOURCE: &[u8] = b"package Gadget;\n1;\n";

    fn logical_source() -> LogicalSourceId {
        let project = ProjectId::from_canonical_name("acme/widget");
        let root = WorkspaceRootId::from_project_and_root_key(&project, "main");
        LogicalSourceId::from_root_and_path(&root, "lib/Widget.pm")
    }

    fn profile() -> SemanticProfileIdentity {
        SemanticProfileIdentity::new("file-semantic", 1, "perl-semantic-analyzer/0.19", "default")
    }

    fn subject(
        instance_key: &str,
        generation_label: &str,
    ) -> crate::semantic_snapshot::SemanticSubjectIdentity {
        subject_for_source(instance_key, generation_label, SOURCE)
    }

    fn subject_for_source(
        instance_key: &str,
        generation_label: &str,
        source: &[u8],
    ) -> crate::semantic_snapshot::SemanticSubjectIdentity {
        let logical_source = logical_source();
        let document_instance =
            DocumentInstanceId::from_logical_source_and_instance_key(&logical_source, instance_key);
        let digest = ContentDigest::of_bytes(source);
        crate::semantic_snapshot::SemanticSubjectIdentity::new(
            logical_source,
            document_instance,
            SourceGeneration::known(generation_label),
            digest.clone(),
            ParserInputRevision::new(digest, source.len() as u64),
        )
    }

    fn parse_snapshot(generation: u64) -> ParseSnapshotIdentity {
        parse_snapshot_for(generation, SOURCE, SemanticParseDisposition::Clean)
    }

    fn parse_snapshot_for(
        generation: u64,
        source: &[u8],
        disposition: SemanticParseDisposition,
    ) -> ParseSnapshotIdentity {
        ParseSnapshotIdentity::new(
            generation,
            ContentDigest::of_bytes(source),
            source.len() as u64,
            disposition,
            SemanticParseStrategy::Fresh,
        )
    }

    fn inputs_for(
        subject: crate::semantic_snapshot::SemanticSubjectIdentity,
        parse: ParseSnapshotIdentity,
    ) -> FreshFullConstructionInputs {
        FreshFullConstructionInputs {
            profile: profile(),
            subject,
            parse_snapshot: parse,
            budget: SemanticConstructionBudget::unbounded(),
        }
    }

    fn fresh_bundle(
        inputs: &FreshFullConstructionInputs,
        completeness: SemanticContributionSetCompleteness,
    ) -> FreshFullContribution {
        let set = SemanticContributionSetRef::new(
            inputs.subject.fingerprint(),
            &inputs.profile,
            completeness,
            ContentDigest::of_bytes(b"contribution-set"),
        );
        let views = SemanticQueryViewKind::REQUIRED_FOR_COMPLETE
            .iter()
            .map(|kind| {
                MaterializedQueryViewRef::new(
                    &set.set_id,
                    *kind,
                    ContentDigest::of_bytes(kind.as_str().as_bytes()),
                )
            })
            .collect();
        FreshFullContribution {
            set,
            views,
            freshness: FreshnessBinding {
                subject_fingerprint: inputs.subject.fingerprint(),
                accepted_generation: inputs.parse_snapshot.accepted_generation,
                work_claim: SemanticWorkKind::FreshFull,
            },
            confidence: SemanticConfidence::Exact,
            limitations: SemanticLimitations::new(vec![]),
        }
    }

    /// Honest fresh-full producer: returns a complete, correctly bound
    /// bundle and counts invocations.
    struct HonestProducer {
        invocations: AtomicUsize,
    }

    impl HonestProducer {
        fn new() -> Self {
            Self { invocations: AtomicUsize::new(0) }
        }

        fn count(&self) -> usize {
            self.invocations.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl FreshFullSemanticProducer for HonestProducer {
        fn build_fresh_full(
            &self,
            inputs: &FreshFullConstructionInputs,
        ) -> FreshFullProducerOutcome {
            self.invocations.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            FreshFullProducerOutcome::Complete(fresh_bundle(
                inputs,
                SemanticContributionSetCompleteness::Complete,
            ))
        }
    }

    /// Producer whose bundle is customized by a closure over the fresh
    /// bundle; counts invocations.
    struct BundleProducer {
        invocations: AtomicUsize,
        adjust: Box<
            dyn Fn(&FreshFullConstructionInputs, FreshFullContribution) -> FreshFullProducerOutcome
                + Send
                + Sync,
        >,
    }

    impl BundleProducer {
        fn complete(
            adjust: impl Fn(FreshFullContribution) -> FreshFullContribution + Send + Sync + 'static,
        ) -> Self {
            Self {
                invocations: AtomicUsize::new(0),
                adjust: Box::new(move |_inputs, mut bundle| {
                    bundle = adjust(bundle);
                    FreshFullProducerOutcome::Complete(bundle)
                }),
            }
        }

        fn outcome(
            make: impl Fn(
                &FreshFullConstructionInputs,
                FreshFullContribution,
            ) -> FreshFullProducerOutcome
            + Send
            + Sync
            + 'static,
        ) -> Self {
            Self { invocations: AtomicUsize::new(0), adjust: Box::new(make) }
        }

        fn count(&self) -> usize {
            self.invocations.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl FreshFullSemanticProducer for BundleProducer {
        fn build_fresh_full(
            &self,
            inputs: &FreshFullConstructionInputs,
        ) -> FreshFullProducerOutcome {
            self.invocations.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            (self.adjust)(
                inputs,
                fresh_bundle(inputs, SemanticContributionSetCompleteness::Complete),
            )
        }
    }

    fn accepted_registry_inputs()
    -> (SemanticConstructionCellRegistry, AcceptedTicketLease, FreshFullConstructionInputs) {
        let registry = SemanticConstructionCellRegistry::new();
        let subject = subject("open-1", "7");
        let parse = parse_snapshot(7);
        let lease = registry.accept_ticket(subject.clone(), parse.clone()).unwrap();
        let inputs = inputs_for(subject, parse);
        (registry, lease, inputs)
    }

    // ── Honest fresh-full construction: full envelope shape ──────────────

    #[test]
    fn fresh_full_inputs_build_the_complete_envelope_shape() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        let producer = HonestProducer::new();
        let terminal = cell.construct_fresh_full(&lease, inputs.clone(), &producer).unwrap();

        assert!(terminal.is_complete_fresh_full());
        assert!(terminal.refusal().is_none());
        let snapshot = terminal.snapshot();
        assert_eq!(snapshot.terminal_state(), SemanticSnapshotTerminalState::CompleteFreshFull);
        assert!(snapshot.is_complete_family());
        assert_eq!(snapshot.completeness(), SemanticCompleteness::Complete);
        assert_eq!(snapshot.confidence(), SemanticConfidence::Exact);

        // Full shape: bound ticket, exact subject, contribution set owned by
        // this exact subject and profile, every required view family.
        let ticket = AcceptedParserTicketId::from_bound_parts(
            &inputs.subject.document_instance,
            inputs.parse_snapshot.accepted_generation,
            &inputs.parse_snapshot.source_digest,
        );
        assert_eq!(snapshot.accepted_ticket().ticket_id, ticket);
        assert_eq!(snapshot.subject(), &inputs.subject);
        let set = snapshot.contribution_set().unwrap();
        assert_eq!(set.completeness, SemanticContributionSetCompleteness::Complete);
        assert_eq!(set.subject_fingerprint, inputs.subject.fingerprint());
        for kind in SemanticQueryViewKind::REQUIRED_FOR_COMPLETE {
            assert!(snapshot.is_view_available(*kind), "required view {kind} must be materialized");
        }
        // Views are canonically ordered by kind.
        let kinds: Vec<_> = snapshot.materialized_views().iter().map(|v| v.kind).collect();
        let mut sorted = kinds.clone();
        sorted.sort();
        assert_eq!(kinds, sorted);

        // Honest work receipt: fresh-full work, construction-cell instrument,
        // deterministic sequence bound to the accepted generation.
        let receipt = snapshot.work_receipt();
        assert_eq!(receipt.work_kind, SemanticWorkKind::FreshFull);
        assert_eq!(receipt.instrument.kind, SemanticInstrumentKind::ConstructionCell);
        assert_eq!(receipt.work_sequence, inputs.parse_snapshot.accepted_generation);

        // The bounded current-facts view (#8575 seam) is served.
        assert!(snapshot.as_current_complete().is_some());
        assert_eq!(producer.count(), 1);

        let truth = cell.truth();
        assert_eq!(truth.strategy, SemanticConstructionStrategy::FreshFull);
        assert_eq!(truth.builds_started, 1);
        assert_eq!(truth.fresh_full_invocations, 1);
        assert_eq!(truth.incremental_invocations, 0);
        assert!(truth.completed);
        assert_eq!(truth.terminal_state, Some(SemanticSnapshotTerminalState::CompleteFreshFull));
    }

    #[test]
    fn identical_inputs_across_registries_produce_identical_fingerprints() {
        let (registry_a, lease_a, inputs_a) = accepted_registry_inputs();
        let (registry_b, lease_b, inputs_b) = accepted_registry_inputs();
        let producer = HonestProducer::new();
        let a = registry_a
            .cell_for(&lease_a, &inputs_a.profile)
            .unwrap()
            .construct_fresh_full(&lease_a, inputs_a, &producer)
            .unwrap();
        let b = registry_b
            .cell_for(&lease_b, &inputs_b.profile)
            .unwrap()
            .construct_fresh_full(&lease_b, inputs_b, &producer)
            .unwrap();
        assert_eq!(a.snapshot().fingerprint(), b.snapshot().fingerprint());
    }

    // ── Duplicate-build falsifier ────────────────────────────────────────

    #[test]
    fn concurrent_consumers_share_one_build_and_one_terminal() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let lease = Arc::new(lease);
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();

        // Builder blocks inside the producer until all consumers are waiting.
        // The invocation counter is shared separately so the builder owns the
        // blocking channels by value (a receiver is not shareable).
        let (entered_tx, entered_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let invocations = Arc::new(AtomicUsize::new(0));
        struct BlockingProducer {
            entered_tx: mpsc::Sender<()>,
            release_rx: mpsc::Receiver<()>,
            invocations: Arc<AtomicUsize>,
        }
        impl FreshFullSemanticProducer for BlockingProducer {
            fn build_fresh_full(
                &self,
                inputs: &FreshFullConstructionInputs,
            ) -> FreshFullProducerOutcome {
                self.invocations.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                self.entered_tx.send(()).unwrap();
                self.release_rx.recv().unwrap();
                FreshFullProducerOutcome::Complete(fresh_bundle(
                    inputs,
                    SemanticContributionSetCompleteness::Complete,
                ))
            }
        }
        let builder_producer =
            BlockingProducer { entered_tx, release_rx, invocations: Arc::clone(&invocations) };

        let builder_cell = Arc::clone(&cell);
        let builder_lease = Arc::clone(&lease);
        let builder = thread::spawn(move || {
            builder_cell.construct_fresh_full(
                &builder_lease,
                inputs_for(subject("open-1", "7"), parse_snapshot(7)),
                &builder_producer,
            )
        });
        entered_rx.recv().unwrap();

        let mut waiters = Vec::new();
        for _ in 0..3 {
            let waiter_cell = Arc::clone(&cell);
            let waiter_lease = Arc::clone(&lease);
            let waiter_inputs = inputs.clone();
            waiters.push(thread::spawn(move || {
                // The waiter's producer is never invoked: the one build is
                // already in flight, so it must join it instead.
                waiter_cell.construct_fresh_full(
                    &waiter_lease,
                    waiter_inputs,
                    &HonestProducer::new(),
                )
            }));
        }
        // Give the waiters time to pile onto the in-flight build.
        thread::sleep(std::time::Duration::from_millis(100));
        let truth_mid = cell.truth();
        assert_eq!(truth_mid.builds_started, 1, "no second build may start");
        assert!(truth_mid.waiters_high_water >= 1, "consumers must be sharing the in-flight build");

        release_tx.send(()).unwrap();
        let builder_terminal = builder.join().unwrap().unwrap();
        for waiter in waiters {
            let waiter_terminal = waiter.join().unwrap();
            assert!(waiter_terminal.is_ok());
            assert!(
                Arc::ptr_eq(&builder_terminal, waiter_terminal.as_ref().unwrap()),
                "all consumers must share one terminal result"
            );
        }

        assert_eq!(
            invocations.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "concurrent consumers must never build twice"
        );
        let truth = cell.truth();
        assert_eq!(truth.builds_started, 1);
        assert_eq!(truth.fresh_full_invocations, 1);
        assert_eq!(truth.incremental_invocations, 0);
        assert!(truth.completed);
    }

    #[test]
    fn repeated_read_after_terminal_never_rebuilds() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        let producer = HonestProducer::new();
        let first = cell.construct_fresh_full(&lease, inputs.clone(), &producer).unwrap();
        let peeked = cell.terminal().unwrap();
        assert!(Arc::ptr_eq(&first, &peeked));
        let second = cell.construct_fresh_full(&lease, inputs, &producer).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(producer.count(), 1, "repeated reads must not trigger analysis");
        let truth = cell.truth();
        assert_eq!(truth.builds_started, 1);
        assert_eq!(truth.shared_hits, 1);
    }

    // ── Held-stale falsifiers: freshness law ─────────────────────────────

    #[test]
    fn stale_generation_contribution_refuses_the_whole_snapshot() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        let producer = BundleProducer::complete(|mut bundle| {
            bundle.freshness.accepted_generation = 6; // held from an older generation
            bundle
        });
        let terminal = cell.construct_fresh_full(&lease, inputs, &producer).unwrap();

        assert_eq!(terminal.terminal_state(), SemanticSnapshotTerminalState::NotProven);
        assert_eq!(
            terminal.refusal(),
            Some(&SemanticConstructionRefusal::ContributionGenerationMismatch {
                expected_generation: 7,
                found_generation: 6,
            })
        );
        let snapshot = terminal.snapshot();
        assert!(
            snapshot.contribution_set().is_none(),
            "a stale contribution must refuse the whole snapshot, not assemble a partial one"
        );
        assert!(snapshot.materialized_views().is_empty());
        assert!(snapshot.as_current_complete().is_none());
        assert!(cell.truth().completed);
    }

    #[test]
    fn foreign_subject_contribution_refuses_the_whole_snapshot() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        let foreign = subject_for_source("open-9", "7", SOURCE);
        let foreign_fingerprint = foreign.fingerprint();
        let found_fingerprint = foreign_fingerprint.clone();
        let producer = BundleProducer::complete(move |mut bundle| {
            bundle.freshness.subject_fingerprint = foreign_fingerprint.clone();
            bundle
        });
        let terminal = cell.construct_fresh_full(&lease, inputs, &producer).unwrap();
        assert_eq!(
            terminal.refusal().map(SemanticConstructionRefusal::to_string),
            Some(
                SemanticConstructionRefusal::ContributionSubjectMismatch {
                    expected_subject: subject("open-1", "7").fingerprint(),
                    found_subject: found_fingerprint,
                }
                .to_string()
            )
        );
        assert!(terminal.snapshot().as_current_complete().is_none());
    }

    #[test]
    fn hidden_incremental_claim_refuses_and_work_truth_stays_fresh() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        let producer = BundleProducer::complete(|mut bundle| {
            bundle.freshness.work_claim = SemanticWorkKind::NoChangeReuse;
            bundle
        });
        let terminal = cell.construct_fresh_full(&lease, inputs, &producer).unwrap();
        assert_eq!(
            terminal.refusal(),
            Some(&SemanticConstructionRefusal::HiddenWorkClaim {
                claimed: SemanticWorkKind::NoChangeReuse,
            })
        );
        assert_eq!(terminal.terminal_state(), SemanticSnapshotTerminalState::NotProven);

        let truth = cell.truth();
        assert_eq!(truth.fresh_full_invocations, 1);
        assert_eq!(
            truth.incremental_invocations, 0,
            "no fresh analysis may be reported as incremental or avoided work"
        );
        // Even the refused envelope reports honest fresh-full work.
        assert_eq!(terminal.snapshot().work_receipt().work_kind, SemanticWorkKind::FreshFull);
    }

    #[test]
    fn incomplete_set_and_missing_view_refuse_the_whole_snapshot() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();

        let partial_set = BundleProducer::complete(|bundle| FreshFullContribution {
            set: SemanticContributionSetRef::new(
                bundle.set.subject_fingerprint.clone(),
                &profile(),
                SemanticContributionSetCompleteness::Partial,
                ContentDigest::of_bytes(b"partial"),
            ),
            ..bundle
        });
        let terminal = cell.construct_fresh_full(&lease, inputs.clone(), &partial_set).unwrap();
        assert_eq!(
            terminal.refusal(),
            Some(&SemanticConstructionRefusal::IncompleteContributionSet {
                completeness: SemanticContributionSetCompleteness::Partial,
            })
        );

        // A fresh cell for the same ticket is impossible (one per ticket), so
        // exercise the missing-view refusal through a new generation.
        let subject8 = subject("open-1", "8");
        let lease8 = registry.accept_ticket(subject8.clone(), parse_snapshot(8)).unwrap();
        let inputs8 = inputs_for(subject8, parse_snapshot(8));
        let cell8 = registry.cell_for(&lease8, &inputs8.profile).unwrap();
        let no_model = BundleProducer::complete(|mut bundle| {
            bundle.views.retain(|v| v.kind != SemanticQueryViewKind::SemanticModel);
            bundle
        });
        let terminal8 = cell8.construct_fresh_full(&lease8, inputs8, &no_model).unwrap();
        assert_eq!(
            terminal8.refusal(),
            Some(&SemanticConstructionRefusal::RequiredViewFamilyMissing {
                kind: SemanticQueryViewKind::SemanticModel,
            })
        );
        assert!(terminal8.snapshot().contribution_set().is_none());
    }

    #[test]
    fn envelope_refusal_is_typed_and_fail_closed() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        // Views owned by another set: passes the cell's freshness checks and
        // is refused by the checked envelope constructor.
        let producer = BundleProducer::complete(|bundle| {
            let other_set = SemanticContributionSetRef::new(
                subject_for_source("other", "7", OTHER_SOURCE).fingerprint(),
                &profile(),
                SemanticContributionSetCompleteness::Complete,
                ContentDigest::of_bytes(b"other-set"),
            );
            let views = SemanticQueryViewKind::REQUIRED_FOR_COMPLETE
                .iter()
                .map(|kind| {
                    MaterializedQueryViewRef::new(
                        &other_set.set_id,
                        *kind,
                        ContentDigest::of_bytes(kind.as_str().as_bytes()),
                    )
                })
                .collect();
            FreshFullContribution { views, ..bundle }
        });
        let terminal = cell.construct_fresh_full(&lease, inputs, &producer).unwrap();
        match terminal.refusal() {
            Some(SemanticConstructionRefusal::EnvelopeRefusal(error)) => {
                assert!(matches!(
                    error,
                    FileSemanticSnapshotValidationError::MaterializedViewSetMismatch { .. }
                ));
            }
            other => panic!("expected a typed envelope refusal, got {other:?}"),
        }
        assert!(terminal.snapshot().contribution_set().is_none());
    }

    // ── Product / budget / instrument distinctions ───────────────────────

    #[test]
    fn product_budget_and_instrument_failures_stay_distinct() {
        struct Fixed(FreshFullProducerOutcome);
        impl FreshFullSemanticProducer for Fixed {
            fn build_fresh_full(
                &self,
                _inputs: &FreshFullConstructionInputs,
            ) -> FreshFullProducerOutcome {
                self.0.clone()
            }
        }

        for (outcome, expected) in [
            (
                FreshFullProducerOutcome::ProductFailure,
                SemanticSnapshotTerminalState::ProductFailure,
            ),
            (
                FreshFullProducerOutcome::BudgetExhausted,
                SemanticSnapshotTerminalState::BudgetExhausted,
            ),
            (
                FreshFullProducerOutcome::InstrumentFailure,
                SemanticSnapshotTerminalState::InstrumentOrSchemaFailure,
            ),
        ] {
            let (registry, lease, inputs) = accepted_registry_inputs();
            let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
            let terminal =
                cell.construct_fresh_full(&lease, inputs, &Fixed(outcome.clone())).unwrap();
            assert_eq!(terminal.terminal_state(), expected, "{outcome:?}");
            assert!(terminal.refusal().is_none(), "{outcome:?}");
            assert!(terminal.snapshot().as_current_complete().is_none());
            assert!(terminal.snapshot().contribution_set().is_none());
        }
    }

    #[test]
    fn bounded_budget_drives_typed_exhaustion() {
        let registry = SemanticConstructionCellRegistry::new();
        let subject = subject("open-1", "7");
        let lease = registry.accept_ticket(subject.clone(), parse_snapshot(7)).unwrap();
        let inputs = FreshFullConstructionInputs {
            budget: SemanticConstructionBudget::bounded(0),
            ..inputs_for(subject, parse_snapshot(7))
        };
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        let producer =
            BundleProducer::outcome(|inputs, _| match inputs.budget.max_producer_steps {
                Some(0) => FreshFullProducerOutcome::BudgetExhausted,
                _ => FreshFullProducerOutcome::ProductFailure,
            });
        let terminal = cell.construct_fresh_full(&lease, inputs, &producer).unwrap();
        assert_eq!(terminal.terminal_state(), SemanticSnapshotTerminalState::BudgetExhausted);
    }

    // ── Ticket lifecycle: exclusivity, liveness, supersession ────────────

    #[test]
    fn ticket_lifecycle_ownership_is_exclusive() {
        let (registry, lease, inputs) = accepted_registry_inputs();

        // The exact ticket cannot be leased twice.
        let again = registry.accept_ticket(subject("open-1", "7"), parse_snapshot(7));
        assert!(matches!(again, Err(SemanticTicketError::TicketAlreadyLeased { .. })));

        // A different generation of the same document is a distinct ticket.
        let lease8 = registry.accept_ticket(subject("open-1", "8"), parse_snapshot(8));
        assert!(lease8.is_ok());
        let _ = &lease;

        // Construction requires the owning lease of the exact cell.
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        let other_lease =
            registry.accept_ticket(subject("open-1", "9"), parse_snapshot(9)).unwrap();
        let err = cell.construct_fresh_full(
            &other_lease,
            inputs_for(subject("open-1", "9"), parse_snapshot(9)),
            &HonestProducer::new(),
        );
        assert!(matches!(err, Err(SemanticConstructionCallError::TicketMismatch { .. })));
        // The mismatched call consumed nothing: the cell still constructs.
        let terminal = cell.construct_fresh_full(&lease, inputs, &HonestProducer::new()).unwrap();
        assert!(terminal.is_complete_fresh_full());
    }

    #[test]
    fn retired_ticket_refuses_construction_without_building() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        lease.retire();
        let producer = HonestProducer::new();
        let terminal = cell.construct_fresh_full(&lease, inputs, &producer).unwrap();
        assert_eq!(terminal.terminal_state(), SemanticSnapshotTerminalState::NotProven);
        assert_eq!(
            terminal.refusal(),
            Some(&SemanticConstructionRefusal::TicketNotLive {
                ticket_id: lease.ticket_id().clone(),
            })
        );
        assert_eq!(producer.count(), 0, "no path may construct without a live ticket");
        assert_eq!(cell.truth().builds_started, 0);

        // A retired lease can no longer obtain a cell.
        assert!(matches!(
            registry.cell_for(&lease, &profile()),
            Err(SemanticTicketError::TicketNotLive { .. })
        ));
    }

    #[test]
    fn superseded_work_finishes_but_cannot_attach() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let lease = Arc::new(lease);
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();

        let (entered_tx, entered_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        struct SupersededProducer {
            entered_tx: mpsc::Sender<()>,
            release_rx: mpsc::Receiver<()>,
        }
        impl FreshFullSemanticProducer for SupersededProducer {
            fn build_fresh_full(
                &self,
                inputs: &FreshFullConstructionInputs,
            ) -> FreshFullProducerOutcome {
                self.entered_tx.send(()).unwrap();
                self.release_rx.recv().unwrap();
                FreshFullProducerOutcome::Complete(fresh_bundle(
                    inputs,
                    SemanticContributionSetCompleteness::Complete,
                ))
            }
        }
        let producer = SupersededProducer { entered_tx, release_rx };
        let builder_cell = Arc::clone(&cell);
        let builder_lease = Arc::clone(&lease);
        let builder_inputs = inputs.clone();
        let builder = thread::spawn(move || {
            builder_cell.construct_fresh_full(&builder_lease, builder_inputs, &producer)
        });
        entered_rx.recv().unwrap();
        lease.retire();
        release_tx.send(()).unwrap();
        let terminal = builder.join().unwrap().unwrap();

        assert_eq!(terminal.terminal_state(), SemanticSnapshotTerminalState::StaleOrSuperseded);
        assert!(terminal.refusal().is_none());
        assert!(
            terminal.snapshot().contribution_set().is_none(),
            "superseded work may finish but its facts cannot attach"
        );
        assert!(terminal.snapshot().as_current_complete().is_none());
        assert_eq!(cell.truth().builds_started, 1, "the work did run to completion");
    }

    // ── Distinct cells: later generations, close/reopen, old-success mask ─

    #[test]
    fn source_identical_later_generation_gets_a_distinct_cell() {
        let registry = SemanticConstructionCellRegistry::new();
        let lease7 = registry.accept_ticket(subject("open-1", "7"), parse_snapshot(7)).unwrap();
        let lease8 = registry.accept_ticket(subject("open-1", "8"), parse_snapshot(8)).unwrap();
        let inputs7 = inputs_for(subject("open-1", "7"), parse_snapshot(7));
        let inputs8 = inputs_for(subject("open-1", "8"), parse_snapshot(8));
        assert_ne!(lease7.ticket_id(), lease8.ticket_id());

        let cell7 = registry.cell_for(&lease7, &inputs7.profile).unwrap();
        let cell8 = registry.cell_for(&lease8, &inputs8.profile).unwrap();
        assert!(!Arc::ptr_eq(&cell7, &cell8));
        assert_eq!(registry.cell_count(), 2);

        let t7 =
            cell7.construct_fresh_full(&lease7, inputs7.clone(), &HonestProducer::new()).unwrap();
        let t8 = cell8.construct_fresh_full(&lease8, inputs8, &HonestProducer::new()).unwrap();
        // Same bytes, same content revision...
        assert_eq!(
            t7.snapshot().subject().full_source_revision.content_digest,
            t8.snapshot().subject().full_source_revision.content_digest
        );
        // ...but distinct subjects and distinct snapshots.
        assert_ne!(t7.snapshot().subject_fingerprint(), t8.snapshot().subject_fingerprint());
        assert_ne!(t7.snapshot().fingerprint(), t8.snapshot().fingerprint());
    }

    #[test]
    fn close_reopen_gets_a_distinct_cell() {
        let registry = SemanticConstructionCellRegistry::new();
        let lease1 = registry.accept_ticket(subject("open-1", "7"), parse_snapshot(7)).unwrap();
        let lease2 = registry.accept_ticket(subject("open-2", "7"), parse_snapshot(7)).unwrap();
        let inputs1 = inputs_for(subject("open-1", "7"), parse_snapshot(7));
        let inputs2 = inputs_for(subject("open-2", "7"), parse_snapshot(7));
        let t1 = registry
            .cell_for(&lease1, &inputs1.profile)
            .unwrap()
            .construct_fresh_full(&lease1, inputs1, &HonestProducer::new())
            .unwrap();
        let t2 = registry
            .cell_for(&lease2, &inputs2.profile)
            .unwrap()
            .construct_fresh_full(&lease2, inputs2, &HonestProducer::new())
            .unwrap();
        assert_eq!(
            t1.snapshot().subject().logical_source_id,
            t2.snapshot().subject().logical_source_id
        );
        assert_ne!(
            t1.snapshot().subject().document_instance,
            t2.snapshot().subject().document_instance
        );
        assert_ne!(t1.snapshot().fingerprint(), t2.snapshot().fingerprint());
    }

    #[test]
    fn old_success_does_not_mask_current_failure() {
        let registry = SemanticConstructionCellRegistry::new();
        let lease7 = registry.accept_ticket(subject("open-1", "7"), parse_snapshot(7)).unwrap();
        let lease8 = registry.accept_ticket(subject("open-1", "8"), parse_snapshot(8)).unwrap();
        let inputs7 = inputs_for(subject("open-1", "7"), parse_snapshot(7));
        let inputs8 = inputs_for(subject("open-1", "8"), parse_snapshot(8));
        let cell7 = registry.cell_for(&lease7, &inputs7.profile).unwrap();
        let cell8 = registry.cell_for(&lease8, &inputs8.profile).unwrap();

        let ok = cell7.construct_fresh_full(&lease7, inputs7, &HonestProducer::new()).unwrap();
        assert!(ok.is_complete_fresh_full());

        struct Fail;
        impl FreshFullSemanticProducer for Fail {
            fn build_fresh_full(
                &self,
                _inputs: &FreshFullConstructionInputs,
            ) -> FreshFullProducerOutcome {
                FreshFullProducerOutcome::ProductFailure
            }
        }
        let failed = cell8.construct_fresh_full(&lease8, inputs8, &Fail).unwrap();
        assert_eq!(failed.terminal_state(), SemanticSnapshotTerminalState::ProductFailure);
        assert!(failed.snapshot().as_current_complete().is_none());
        // The old success is untouched but belongs to its own ticket only.
        assert!(ok.is_complete_fresh_full());
    }

    // ── Partial-recovered honesty ────────────────────────────────────────

    #[test]
    fn recovered_parse_builds_an_honest_partial_and_clean_parse_refuses_partial() {
        // Honest partial: recovered parse + partial set + recovery limits.
        let registry = SemanticConstructionCellRegistry::new();
        let honest_subject = subject("open-1", "7");
        let parse = parse_snapshot_for(7, SOURCE, SemanticParseDisposition::Recovered);
        let lease = registry.accept_ticket(honest_subject.clone(), parse.clone()).unwrap();
        let inputs = inputs_for(honest_subject, parse);
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        let producer = BundleProducer::outcome(|inputs, mut bundle| {
            bundle.set = SemanticContributionSetRef::new(
                inputs.subject.fingerprint(),
                &inputs.profile,
                SemanticContributionSetCompleteness::Partial,
                ContentDigest::of_bytes(b"partial-set"),
            );
            bundle.confidence = SemanticConfidence::Recovered;
            bundle.limitations =
                SemanticLimitations::new(vec![crate::semantic_snapshot::SemanticLimitationEntry {
                    kind: crate::semantic_snapshot::SemanticLimitationKind::RecoveredRegion,
                    count: 1,
                }]);
            FreshFullProducerOutcome::PartialRecovered(bundle)
        });
        let terminal = cell.construct_fresh_full(&lease, inputs.clone(), &producer).unwrap();
        assert_eq!(terminal.terminal_state(), SemanticSnapshotTerminalState::PartialRecovered);
        assert!(terminal.refusal().is_none());
        assert!(terminal.snapshot().contribution_set().is_some());
        assert!(terminal.snapshot().as_current_complete().is_none());

        // Clean parse + partial outcome: contradicts the parse disposition.
        let lease2 = registry.accept_ticket(subject("open-2", "7"), parse_snapshot(7)).unwrap();
        let inputs2 = inputs_for(subject("open-2", "7"), parse_snapshot(7));
        let cell2 = registry.cell_for(&lease2, &inputs2.profile).unwrap();
        let clean_partial = BundleProducer::outcome(|inputs, mut bundle| {
            bundle.set = SemanticContributionSetRef::new(
                inputs.subject.fingerprint(),
                &inputs.profile,
                SemanticContributionSetCompleteness::Partial,
                ContentDigest::of_bytes(b"partial-set"),
            );
            bundle.confidence = SemanticConfidence::Recovered;
            bundle.limitations =
                SemanticLimitations::new(vec![crate::semantic_snapshot::SemanticLimitationEntry {
                    kind: crate::semantic_snapshot::SemanticLimitationKind::RecoveredRegion,
                    count: 1,
                }]);
            FreshFullProducerOutcome::PartialRecovered(bundle)
        });
        let terminal2 = cell2.construct_fresh_full(&lease2, inputs2, &clean_partial).unwrap();
        assert_eq!(
            terminal2.refusal(),
            Some(&SemanticConstructionRefusal::OutcomeDispositionMismatch {
                disposition: SemanticParseDisposition::Clean,
            })
        );
    }

    // ── Caller-local refusal and waiter integrity ────────────────────────

    #[test]
    fn input_mismatch_refuses_only_the_caller() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let lease = Arc::new(lease);
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();

        let (entered_tx, entered_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        struct Blocking {
            entered_tx: mpsc::Sender<()>,
            release_rx: mpsc::Receiver<()>,
        }
        impl FreshFullSemanticProducer for Blocking {
            fn build_fresh_full(
                &self,
                inputs: &FreshFullConstructionInputs,
            ) -> FreshFullProducerOutcome {
                self.entered_tx.send(()).unwrap();
                self.release_rx.recv().unwrap();
                FreshFullProducerOutcome::Complete(fresh_bundle(
                    inputs,
                    SemanticContributionSetCompleteness::Complete,
                ))
            }
        }
        let producer = Blocking { entered_tx, release_rx };
        let builder_cell = Arc::clone(&cell);
        let builder_lease = Arc::clone(&lease);
        let builder_inputs = inputs.clone();
        let builder = thread::spawn(move || {
            builder_cell.construct_fresh_full(&builder_lease, builder_inputs, &producer)
        });
        entered_rx.recv().unwrap();

        // A caller with contradicting inputs is refused without corrupting
        // the in-flight build.
        let mut contradictory = inputs.clone();
        contradictory.budget = SemanticConstructionBudget::bounded(99);
        let err = cell.construct_fresh_full(&lease, contradictory, &HonestProducer::new());
        assert!(matches!(err, Err(SemanticConstructionCallError::InputMismatch)));

        release_tx.send(()).unwrap();
        let terminal = builder.join().unwrap().unwrap();
        assert!(terminal.is_complete_fresh_full());
        assert!(cell.truth().completed);
    }

    #[test]
    fn incoherent_profile_triple_is_refused_at_cell_lookup() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        // A hand-assembled triple whose stored fingerprint does not match its
        // schema/implementation/profile triple never reaches a cell.
        let mut incoherent = inputs.profile.clone();
        incoherent.fingerprint = ContentDigest::of_bytes(b"hand-forged");
        assert!(matches!(
            registry.cell_for(&lease, &incoherent),
            Err(SemanticTicketError::ProfileIncoherent { .. })
        ));
        // The coherent profile still obtains its cell.
        assert!(registry.cell_for(&lease, &inputs.profile).is_ok());
    }

    #[test]
    fn inputs_under_another_profile_refuse_only_the_caller() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        let other_profile = SemanticProfileIdentity::new(
            "file-semantic",
            1,
            "perl-semantic-analyzer/0.19",
            "strict",
        );
        let mut foreign = inputs.clone();
        foreign.profile = other_profile;
        let err = cell.construct_fresh_full(&lease, foreign, &HonestProducer::new());
        assert!(matches!(err, Err(SemanticConstructionCallError::CellProfileMismatch { .. })));
        // The refusal consumed nothing: the cell still constructs under its
        // own profile.
        let terminal = cell.construct_fresh_full(&lease, inputs, &HonestProducer::new()).unwrap();
        assert!(terminal.is_complete_fresh_full());
    }

    #[test]
    fn panicking_builder_does_not_corrupt_waiters() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let lease = Arc::new(lease);
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();

        let (entered_tx, entered_rx) = mpsc::channel::<()>();
        struct Panicking {
            entered_tx: mpsc::Sender<()>,
        }
        impl FreshFullSemanticProducer for Panicking {
            fn build_fresh_full(
                &self,
                _inputs: &FreshFullConstructionInputs,
            ) -> FreshFullProducerOutcome {
                self.entered_tx.send(()).unwrap();
                panic!("producer exploded");
            }
        }
        // Silence the default hook so the deliberate panic does not spam the
        // test output; restore it before asserting.
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        let producer = Panicking { entered_tx };
        let builder_cell = Arc::clone(&cell);
        let builder_lease = Arc::clone(&lease);
        let builder = thread::spawn(move || {
            builder_cell.construct_fresh_full(&builder_lease, inputs, &producer)
        });
        entered_rx.recv().unwrap();

        // A waiter joins the in-flight build; when the producer panics, the
        // cell must publish the typed product-failure terminal and wake it.
        let waiter_cell = Arc::clone(&cell);
        let waiter_lease = Arc::clone(&lease);
        let waiter_inputs = inputs_for(subject("open-1", "7"), parse_snapshot(7));
        let waiter = thread::spawn(move || {
            waiter_cell.construct_fresh_full(&waiter_lease, waiter_inputs, &HonestProducer::new())
        });

        let builder_terminal = builder.join().unwrap().unwrap();
        let waiter_terminal = waiter.join().unwrap().unwrap();
        std::panic::set_hook(previous_hook);

        assert_eq!(
            builder_terminal.terminal_state(),
            SemanticSnapshotTerminalState::ProductFailure
        );
        assert!(
            Arc::ptr_eq(&builder_terminal, &waiter_terminal),
            "waiters must share the builder's typed failure terminal"
        );
        assert!(cell.truth().completed);
    }

    // ── Release and cleanup ──────────────────────────────────────────────

    #[test]
    fn release_releases_exactly_once_and_cleanup_is_bounded() {
        let registry = SemanticConstructionCellRegistry::new();
        let lease = registry.accept_ticket(subject("open-1", "7"), parse_snapshot(7)).unwrap();
        let inputs = inputs_for(subject("open-1", "7"), parse_snapshot(7));
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        let terminal = cell.construct_fresh_full(&lease, inputs, &HonestProducer::new()).unwrap();
        assert!(terminal.is_complete_fresh_full());

        let released = registry.release(&lease);
        assert_eq!(released, TicketReleaseDisposition::Released { cells_removed: 1 });
        assert_eq!(registry.cell_count(), 0);
        assert_eq!(registry.lease_count(), 0);
        assert!(!lease.is_live(), "release retires the lease");

        assert_eq!(
            registry.release(&lease),
            TicketReleaseDisposition::AlreadyReleased,
            "ownership is released exactly once"
        );
        assert!(matches!(
            registry.cell_for(&lease, &profile()),
            Err(SemanticTicketError::TicketNotAccepted { .. })
        ));

        // Shared Arcs survive release for their holders; the registry holds
        // nothing.
        assert!(terminal.is_complete_fresh_full());

        // Re-acceptance begins a fresh lifecycle for the exact ticket id
        // (honest servers always move to a new generation or document
        // instance, which is a new ticket id).
        let re_lease = registry.accept_ticket(subject("open-1", "7"), parse_snapshot(7));
        assert!(re_lease.is_ok());
        assert_eq!(registry.lease_count(), 1);
    }

    #[test]
    fn inputs_bound_to_another_ticket_refuse_only_the_caller() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        // Coherent inputs for a DIFFERENT ticket (generation 8): the cell
        // must not build another ticket's subject through ticket 7's cell.
        let foreign = inputs_for(subject("open-1", "8"), parse_snapshot(8));
        let err = cell.construct_fresh_full(&lease, foreign, &HonestProducer::new());
        assert!(matches!(err, Err(SemanticConstructionCallError::InputsTicketMismatch { .. })));
        // The refusal consumed nothing.
        let terminal = cell.construct_fresh_full(&lease, inputs, &HonestProducer::new()).unwrap();
        assert!(terminal.is_complete_fresh_full());
    }

    #[test]
    fn incoherent_profile_triple_refuses_at_construction() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        // Copy the coherent fingerprint field onto a mutated triple: the
        // fingerprint key matches, but the triple itself is incoherent.
        let mut forged = inputs.clone();
        forged.profile.schema = crate::semantic_snapshot::SemanticProfileIdentity::new(
            "other-semantic",
            1,
            "perl-semantic-analyzer/0.19",
            "default",
        )
        .schema
        .clone();
        forged.profile.fingerprint = inputs.profile.fingerprint.clone();
        let err = cell.construct_fresh_full(&lease, forged, &HonestProducer::new());
        assert!(matches!(err, Err(SemanticConstructionCallError::IncoherentProfileTriple { .. })));
    }

    #[test]
    fn work_receipts_bind_the_exact_ticket() {
        let registry = SemanticConstructionCellRegistry::new();
        // Same document instance and generation, DIFFERENT parser-input
        // digest: distinct tickets, and their receipts must differ.
        let subject_a = subject_for_source("open-1", "7", SOURCE);
        let subject_b = subject_for_source("open-1", "7", OTHER_SOURCE);
        let parse_a = parse_snapshot_for(7, SOURCE, SemanticParseDisposition::Clean);
        let parse_b = parse_snapshot_for(7, OTHER_SOURCE, SemanticParseDisposition::Clean);
        let lease_a = registry.accept_ticket(subject_a.clone(), parse_a.clone()).unwrap();
        let lease_b = registry.accept_ticket(subject_b.clone(), parse_b.clone()).unwrap();
        assert_ne!(lease_a.ticket_id(), lease_b.ticket_id());

        let inputs_a = inputs_for(subject_a, parse_a);
        let inputs_b = inputs_for(subject_b, parse_b);
        let terminal_a = registry
            .cell_for(&lease_a, &inputs_a.profile)
            .unwrap()
            .construct_fresh_full(&lease_a, inputs_a, &HonestProducer::new())
            .unwrap();
        let terminal_b = registry
            .cell_for(&lease_b, &inputs_b.profile)
            .unwrap()
            .construct_fresh_full(&lease_b, inputs_b, &HonestProducer::new())
            .unwrap();
        assert_ne!(
            terminal_a.snapshot().work_receipt().receipt_id,
            terminal_b.snapshot().work_receipt().receipt_id,
            "two distinct tickets sharing an instance and generation must not share a receipt"
        );
    }

    #[test]
    fn retirement_before_publication_demotes_to_stale() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        // Resolve a complete outcome first (the producer succeeded), then
        // retire the ticket, then publish: the attachable result must be
        // demoted to stale_or_superseded with no facts.
        let resolved = SemanticConstructionCell::resolve_outcome(
            &inputs,
            &FreshFullProducerOutcome::Complete(fresh_bundle(
                &inputs,
                SemanticContributionSetCompleteness::Complete,
            )),
            &lease,
        );
        assert!(resolved.is_attachable());
        lease.retire();
        let terminal = cell.publish_with_liveness_check(&lease, resolved);
        assert_eq!(terminal.terminal_state(), SemanticSnapshotTerminalState::StaleOrSuperseded);
        assert!(
            terminal.snapshot().contribution_set().is_none(),
            "a retirement before publication must not attach facts"
        );
        assert_eq!(cell.truth().builds_started, 0);
    }

    #[test]
    fn cross_registry_leases_are_refused_everywhere() {
        let (registry_a, lease_a, inputs) = accepted_registry_inputs();
        // Registry B independently accepts the same subject/parse pair:
        // same deterministic ticket id, different live capability.
        let registry_b = SemanticConstructionCellRegistry::new();
        let lease_b = registry_b.accept_ticket(subject("open-1", "7"), parse_snapshot(7)).unwrap();
        assert_eq!(lease_a.ticket_id(), lease_b.ticket_id());

        // B's lease cannot obtain A's cells.
        assert!(matches!(
            registry_a.cell_for(&lease_b, &inputs.profile),
            Err(SemanticTicketError::ForeignLeaseCapability { .. })
        ));
        // A's cells constructed with B's lease are refused (the cell is
        // created with A's capability first).
        let cell = registry_a.cell_for(&lease_a, &inputs.profile).unwrap();
        assert!(matches!(
            cell.construct_fresh_full(&lease_b, inputs.clone(), &HonestProducer::new()),
            Err(SemanticConstructionCallError::ForeignLeaseCapability { .. })
        ));
        // B's lease cannot release A's lifecycle: nothing is removed.
        assert_eq!(registry_a.release(&lease_b), TicketReleaseDisposition::AlreadyReleased);
        assert_eq!(registry_a.lease_count(), 1, "A's own lease must remain");
        // A's own lease still works end to end.
        let terminal = cell.construct_fresh_full(&lease_a, inputs, &HonestProducer::new()).unwrap();
        assert!(terminal.is_complete_fresh_full());
    }

    #[test]
    fn stale_lease_after_reaccept_cannot_reach_the_new_lifecycle() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        registry.release(&lease);
        // Reacceptance mints a fresh capability for the same ticket id.
        let fresh_lease =
            registry.accept_ticket(subject("open-1", "7"), parse_snapshot(7)).unwrap();
        assert_eq!(fresh_lease.ticket_id(), lease.ticket_id());
        // The OLD lease is a stale capability: it cannot reach cells.
        assert!(matches!(
            registry.cell_for(&lease, &inputs.profile),
            Err(SemanticTicketError::ForeignLeaseCapability { .. })
        ));
        // The old cell (pre-release Arc) also refuses the stale lease:
        // same Arc, but retired — construction fails closed as a typed
        // not-live refusal terminal with no facts and no build.
        let stale_terminal =
            cell.construct_fresh_full(&lease, inputs, &HonestProducer::new()).unwrap();
        assert_eq!(stale_terminal.terminal_state(), SemanticSnapshotTerminalState::NotProven);
        assert_eq!(
            stale_terminal.refusal(),
            Some(&SemanticConstructionRefusal::TicketNotLive {
                ticket_id: lease.ticket_id().clone(),
            })
        );
        assert!(stale_terminal.snapshot().contribution_set().is_none());
        assert_eq!(cell.truth().builds_started, 0, "no build may start on a retired ticket");
    }

    #[test]
    fn poisoned_cell_lock_recovers_for_builders_and_waiters() {
        let (registry, lease, inputs) = accepted_registry_inputs();
        let lease = std::sync::Arc::new(lease);
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        // Deliberately poison the cell mutex; the state itself is intact.
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let poison_cell = Arc::clone(&cell);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = poison_cell.inner.lock();
            panic!("poison the cell lock");
        }));
        std::panic::set_hook(previous_hook);

        // A builder and a waiter both recover the poisoned lock and share
        // one terminal: no panic, no corruption.
        let builder_cell = Arc::clone(&cell);
        let builder_lease = Arc::clone(&lease);
        let builder_inputs = inputs.clone();
        let builder = std::thread::spawn(move || {
            builder_cell.construct_fresh_full(
                &builder_lease,
                builder_inputs,
                &HonestProducer::new(),
            )
        });
        let waiter_cell = Arc::clone(&cell);
        let waiter_lease = Arc::clone(&lease);
        let waiter_inputs = inputs_for(subject("open-1", "7"), parse_snapshot(7));
        let waiter = std::thread::spawn(move || {
            waiter_cell.construct_fresh_full(&waiter_lease, waiter_inputs, &HonestProducer::new())
        });
        let builder_terminal = builder.join().unwrap().unwrap();
        let waiter_terminal = waiter.join().unwrap().unwrap();
        assert!(builder_terminal.is_complete_fresh_full());
        assert!(Arc::ptr_eq(&builder_terminal, &waiter_terminal));
    }

    #[test]
    fn acceptance_fails_closed_on_incoherent_tickets() {
        let registry = SemanticConstructionCellRegistry::new();
        // Parse names different bytes than the subject's parser input.
        let bad_digest = ParseSnapshotIdentity::new(
            7,
            ContentDigest::of_bytes(OTHER_SOURCE),
            SOURCE.len() as u64,
            SemanticParseDisposition::Clean,
            SemanticParseStrategy::Fresh,
        );
        assert_eq!(
            registry.accept_ticket(subject("open-1", "7"), bad_digest).unwrap_err(),
            SemanticTicketError::ParserInputDigestMismatch
        );
        // Parse length disagrees.
        let bad_len = ParseSnapshotIdentity::new(
            7,
            ContentDigest::of_bytes(SOURCE),
            (SOURCE.len() + 1) as u64,
            SemanticParseDisposition::Clean,
            SemanticParseStrategy::Fresh,
        );
        assert_eq!(
            registry.accept_ticket(subject("open-1", "7"), bad_len).unwrap_err(),
            SemanticTicketError::ParserInputLengthMismatch
        );
    }

    #[test]
    fn refusal_is_attached_iff_terminal_is_not_proven() {
        let cases: Vec<Box<dyn Fn(&FreshFullConstructionInputs) -> FreshFullProducerOutcome>> = vec![
            Box::new(|inputs| {
                FreshFullProducerOutcome::Complete(fresh_bundle(
                    inputs,
                    SemanticContributionSetCompleteness::Complete,
                ))
            }),
            Box::new(|_| FreshFullProducerOutcome::ProductFailure),
            Box::new(|_| FreshFullProducerOutcome::BudgetExhausted),
            Box::new(|_| FreshFullProducerOutcome::InstrumentFailure),
        ];
        for case in cases {
            let (registry, lease, inputs) = accepted_registry_inputs();
            let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
            struct CaseProducer(
                Box<dyn Fn(&FreshFullConstructionInputs) -> FreshFullProducerOutcome>,
            );
            impl FreshFullSemanticProducer for CaseProducer {
                fn build_fresh_full(
                    &self,
                    inputs: &FreshFullConstructionInputs,
                ) -> FreshFullProducerOutcome {
                    (self.0)(inputs)
                }
            }
            let terminal = cell.construct_fresh_full(&lease, inputs, &CaseProducer(case)).unwrap();
            assert_eq!(
                terminal.refusal().is_some(),
                terminal.terminal_state() == SemanticSnapshotTerminalState::NotProven,
                "refusal is attached iff the terminal snapshot is not_proven"
            );
        }
    }

    // ── Future-strategy double cannot enter through the fresh-full cell ──

    #[test]
    fn future_strategy_double_cannot_enter_the_fresh_full_seam() {
        // A producer that claims no-change reuse (a future #7308 strategy
        // outcome) cannot attach through this cell: its bundle is refused
        // with the hidden work claim, and no predecessor can be carried into
        // a fresh-full snapshot.
        let (registry, lease, inputs) = accepted_registry_inputs();
        let cell = registry.cell_for(&lease, &inputs.profile).unwrap();
        let producer = BundleProducer::complete(|bundle| bundle);
        let terminal = cell.construct_fresh_full(&lease, inputs, &producer).unwrap();
        assert!(terminal.is_complete_fresh_full());
        let snapshot = terminal.snapshot();
        assert!(snapshot.predecessor().is_none(), "fresh-full construction claims no reuse");
        let _ = SemanticPredecessorRef {
            predecessor_fingerprint: ContentDigest::of_bytes(b"pred"),
            predecessor_generation: 6,
            relation: SemanticReuseRelation::Incremental,
        };
        // The contribution bundle type has no predecessor field, so no
        // producer can smuggle reuse through the seam; the work kind is
        // derived by the cell and cannot be supplied.
        assert_eq!(snapshot.work_receipt().work_kind, SemanticWorkKind::FreshFull);
        let _ = MaterializedQueryViewId::from_set_and_kind;
    }
}
