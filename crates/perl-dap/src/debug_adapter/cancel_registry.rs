//! Session-bound, request-scoped cancellation registry (#9074).
//!
//! One seam maps the DAP client request sequence to the cancellable backend
//! operation it started:
//!
//! ```text
//! DAP request seq
//! → registry entry (operation id, cancellation token, state)
//! → cancel(requestId) retires exactly that operation
//! → terminal settlement records the real outcome
//! → no other request, and no later session, is ever affected
//! ```
//!
//! This replaces the adapter-global `cancel_requested` atomic whose single
//! bit was consumed by whichever poll loop ran next: a cancel intended for
//! request A could truncate request B, a static scan, or an unrelated
//! session teardown. The typed identity / token / serialized-transition
//! shape follows the reviewed perl5db operation broker pattern (#8564, PR
//! #14183); this registry is the DAP request correlation layer above that
//! seam and does not depend on the broker being merged.
//!
//! Wire semantics (DAP `cancel`, pinned by #6737):
//!
//! - `requestId` targets the operation mapped to that request sequence;
//!   when both `requestId` and `progressId` are supplied, `requestId`
//!   takes precedence.
//! - The adapter registers no progress identity, so a progress-only target
//!   deterministically affects nothing.
//! - Unknown, already-terminal, malformed, absent, and wrong-session
//!   targets never cancel "the current operation": the cancel response is
//!   still a successful, empty protocol acknowledgement, but the recorded
//!   disposition distinguishes [`CancelDisposition::Accepted`],
//!   [`CancelDisposition::AlreadyTerminal`], and
//!   [`CancelDisposition::UnknownTarget`].
//! - A duplicate cancel of one operation is idempotent and cannot spill
//!   into any later operation.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::sync_utils::lock_or_recover;

/// Upper bound on retained registry entries.
///
/// Live entries are never evicted. Terminal entries are retained so a late
/// cancel observes `AlreadyTerminal` instead of a misleading `UnknownTarget`,
/// but only a bounded window of the most recent terminal entries is kept;
/// long sessions cannot grow the registry without limit.
const MAX_RETAINED_ENTRIES: usize = 64;

/// Identity of one registered cancellable operation. Unique for the
/// adapter's lifetime; identities are minted even for registrations that
/// supersede a prior record, so they are unique but never dense.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct OperationId(u64);

/// Cooperative cancellation token for exactly one registered operation
/// (pattern from the #8564 broker). Polling takes an `Acquire` load on a
/// dedicated flag — never the registry lock — so framed-output wait loops
/// keep their lock-free polling shape.
#[derive(Debug, Clone)]
pub(crate) struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// Retire only the operation holding this token.
    fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Whether this exact operation has been retired by a cancel request.
    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Typed terminal outcome recorded at settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationOutcome {
    /// The operation ran to completion (or produced its response) before
    /// any cancel arrived.
    Completed,
    /// The operation ended because a cancel targeted it.
    Cancelled,
    /// The owning handler exited without an explicit settlement. RAII
    /// records this so no entry can outlive its handler as live.
    Abandoned,
}

/// What a cancel request observed for its exact target identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelDisposition {
    /// The cancel was accepted against the live operation mapped to the
    /// target request sequence.
    Accepted { operation: OperationId },
    /// The mapped operation had already reached a terminal outcome before
    /// the cancel arrived. Accepted-cancel and terminal outcome stay
    /// distinct: a cancel never fabricates termination, and a completed
    /// operation is never reported as cancelled.
    AlreadyTerminal { operation: OperationId },
    /// No operation is mapped to the target in this session. An unknown
    /// explicit target never falls back to "cancel whatever is current".
    UnknownTarget,
}

impl CancelDisposition {
    /// Whether the cancel reached the mapped live operation.
    #[cfg(test)]
    pub(crate) fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted { .. })
    }
}

#[derive(Debug)]
struct OperationRecord {
    operation: OperationId,
    /// Registration order, used only for bounded eviction of terminal
    /// entries.
    registered_epoch: u64,
    token: CancellationToken,
    state: RecordState,
}

#[derive(Debug, PartialEq, Eq)]
enum RecordState {
    Live,
    CancelRequested,
    Terminal(OperationOutcome),
}

/// One registered cancellable operation, handed to the request handler that
/// created it.
///
/// The handler polls [`Self::is_cancelled`] in its wait loops and settles
/// the real outcome with [`Self::settle`]. If the handler returns without
/// settling, the RAII drop settles the operation as
/// [`OperationOutcome::Abandoned`], so terminal settlement is atomic with
/// the handler's exit and no entry can stay live behind a finished request.
pub(crate) struct RegisteredOperation {
    request_seq: i64,
    operation: OperationId,
    token: CancellationToken,
    registry: Arc<CancelRegistry>,
    settled: AtomicBool,
}

impl RegisteredOperation {
    /// The identity of this registered operation.
    #[cfg(test)]
    pub(crate) fn operation(&self) -> OperationId {
        self.operation
    }

    /// The token polled by this operation's wait loops.
    pub(crate) fn token(&self) -> &CancellationToken {
        &self.token
    }

    /// Whether a cancel request retired this exact operation.
    pub(crate) fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Record the real terminal outcome for this operation. Idempotent:
    /// the first settlement wins; later settlements (including the RAII
    /// fallback) cannot overwrite it.
    pub(crate) fn settle(&self, outcome: OperationOutcome) {
        if self.settled.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            self.registry.settle(self.request_seq, self.operation, outcome);
        }
    }
}

impl Drop for RegisteredOperation {
    fn drop(&mut self) {
        if !self.settled.load(Ordering::Acquire) {
            self.settled.store(true, Ordering::Release);
            self.registry.settle(self.request_seq, self.operation, OperationOutcome::Abandoned);
        }
    }
}

impl std::fmt::Debug for RegisteredOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisteredOperation")
            .field("request_seq", &self.request_seq)
            .field("operation", &self.operation)
            .field("settled", &self.settled.load(Ordering::Acquire))
            .finish()
    }
}

/// The per-session registry. The adapter owns the only `Arc`, so the
/// mapping is session-bound by construction: a registry cannot outlive its
/// session, and a request sequence reused by a later adapter resolves to
/// nothing.
#[derive(Debug, Default)]
pub(crate) struct CancelRegistry {
    entries: Mutex<HashMap<i64, OperationRecord>>,
    next_operation_id: AtomicU64,
    next_registered_epoch: AtomicU64,
}

impl CancelRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Register one cancellable operation for `request_seq`, before the
    /// operation becomes observable.
    ///
    /// A client that reuses a sequence while a prior record for it still
    /// exists supersedes that record: the prior operation's token is woken
    /// and the sequence maps to the new operation, so a stale cancel for
    /// the reused sequence can reach only the operation the sequence now
    /// names.
    pub(crate) fn register(
        self: &Arc<Self>,
        request_seq: i64,
        command: &str,
    ) -> RegisteredOperation {
        let operation = OperationId(self.next_operation_id.fetch_add(1, Ordering::AcqRel));
        let token = CancellationToken::new();
        let epoch = self.next_registered_epoch.fetch_add(1, Ordering::AcqRel);

        {
            let mut entries = lock_or_recover(&self.entries, "cancel_registry.entries");
            if let Some(prior) = entries.remove(&request_seq) {
                prior.token.cancel();
            }
            entries.insert(
                request_seq,
                OperationRecord {
                    operation,
                    registered_epoch: epoch,
                    token: token.clone(),
                    state: RecordState::Live,
                },
            );
            Self::prune_terminal(&mut entries);
        }
        tracing::debug!(request_seq, command, operation = ?operation, "cancellable operation registered");

        RegisteredOperation {
            request_seq,
            operation,
            token,
            registry: Arc::clone(self),
            settled: AtomicBool::new(false),
        }
    }

    /// Retire exactly the operation mapped to `request_seq`.
    ///
    /// All validation and the state transition run under one acquisition
    /// of the registry lock, mirroring the broker's serialized
    /// submit/settle discipline: a cancel can never interleave with a
    /// settlement and land on an operation that is already terminal.
    pub(crate) fn cancel_request(&self, request_seq: i64) -> CancelDisposition {
        let mut entries = lock_or_recover(&self.entries, "cancel_registry.entries");
        match entries.get_mut(&request_seq) {
            None => CancelDisposition::UnknownTarget,
            Some(record) => match record.state {
                RecordState::Terminal(_) => {
                    CancelDisposition::AlreadyTerminal { operation: record.operation }
                }
                RecordState::Live => {
                    record.token.cancel();
                    record.state = RecordState::CancelRequested;
                    CancelDisposition::Accepted { operation: record.operation }
                }
                RecordState::CancelRequested => {
                    // Duplicate cancel of the same operation is idempotent.
                    CancelDisposition::Accepted { operation: record.operation }
                }
            },
        }
    }

    /// Settle every live operation — wake its waiter — and drop all
    /// mappings.
    ///
    /// Called from session teardown (disconnect, terminate, restart,
    /// adapter drop). Afterwards no request sequence resolves in this
    /// registry: a stale identity from this session cannot reach anything,
    /// and the next session starts from an empty mapping.
    pub(crate) fn settle_all(&self) {
        let mut entries = lock_or_recover(&self.entries, "cancel_registry.entries");
        for record in entries.values_mut() {
            if !matches!(record.state, RecordState::Terminal(_)) {
                record.token.cancel();
            }
        }
        entries.clear();
    }

    /// Resolve a target identity without cancelling it (test/diagnostic
    /// surface for the typed dispositions).
    #[cfg(test)]
    pub(crate) fn peek_disposition(&self, request_seq: i64) -> CancelDisposition {
        let entries = lock_or_recover(&self.entries, "cancel_registry.entries");
        match entries.get(&request_seq) {
            None => CancelDisposition::UnknownTarget,
            Some(record) => match record.state {
                RecordState::Live | RecordState::CancelRequested => {
                    CancelDisposition::Accepted { operation: record.operation }
                }
                RecordState::Terminal(_) => {
                    CancelDisposition::AlreadyTerminal { operation: record.operation }
                }
            },
        }
    }

    /// Record the terminal outcome of one operation under the lock.
    ///
    /// An operation whose record was replaced (superseded) has no mapping
    /// left to mark; its replacement stays untouched. A successful
    /// transition also runs the retained-entry prune (#9074 review): the
    /// bounded terminal window must hold even when many live operations
    /// finish without any later `register` call, and pruning here can never
    /// evict live entries.
    fn settle(&self, request_seq: i64, operation: OperationId, outcome: OperationOutcome) {
        let mut entries = lock_or_recover(&self.entries, "cancel_registry.entries");
        if let Some(record) = entries.get_mut(&request_seq)
            && record.operation == operation
        {
            record.state = RecordState::Terminal(outcome);
        }
        Self::prune_terminal(&mut entries);
    }

    /// Keep the retained-entry bound: evict the oldest terminal entries
    /// when the table exceeds the cap. Live entries are never evicted.
    fn prune_terminal(entries: &mut HashMap<i64, OperationRecord>) {
        if entries.len() <= MAX_RETAINED_ENTRIES {
            return;
        }
        let mut terminal_epochs: Vec<(u64, i64)> = entries
            .iter()
            .filter(|(_, record)| matches!(record.state, RecordState::Terminal(_)))
            .map(|(seq, record)| (record.registered_epoch, *seq))
            .collect();
        terminal_epochs.sort_unstable();
        let excess = entries.len() - MAX_RETAINED_ENTRIES;
        for (_, seq) in terminal_epochs.into_iter().take(excess) {
            entries.remove(&seq);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // #9074 required-test rows proven at the registry seam. The adapter-level
    // cross-request rows live in `mod.rs` tests and the integration suites.

    #[test]
    fn cancel_targets_only_the_mapped_operation() {
        let registry = Arc::new(CancelRegistry::new());
        let a = registry.register(11, "evaluate");
        let b = registry.register(12, "stackTrace");

        assert_eq!(
            registry.cancel_request(11),
            CancelDisposition::Accepted { operation: a.operation }
        );
        assert!(a.is_cancelled(), "cancel(A) must retire operation A");
        assert!(!b.is_cancelled(), "cancel(A) must never retire operation B");
    }

    #[test]
    fn unknown_target_cannot_cancel_current_operation() {
        let registry = Arc::new(CancelRegistry::new());
        let live = registry.register(21, "stackTrace");

        assert_eq!(registry.cancel_request(999), CancelDisposition::UnknownTarget);
        assert!(!live.is_cancelled(), "unknown target must not cancel what is current");
    }

    #[test]
    fn duplicate_cancel_is_idempotent() {
        let registry = Arc::new(CancelRegistry::new());
        let op = registry.register(31, "gotoTargets");

        let first = registry.cancel_request(31);
        let second = registry.cancel_request(31);
        assert_eq!(first, second, "duplicate cancel must be idempotent");
        assert!(matches!(first, CancelDisposition::Accepted { .. }));
        assert!(op.is_cancelled());
    }

    #[test]
    fn terminal_outcome_first_keeps_cancel_distinct() {
        let registry = Arc::new(CancelRegistry::new());
        let op = registry.register(41, "evaluate");
        op.settle(OperationOutcome::Completed);

        assert_eq!(
            registry.cancel_request(41),
            CancelDisposition::AlreadyTerminal { operation: op.operation },
            "a completed operation is never reported as cancellable"
        );
        assert!(!op.is_cancelled(), "late cancel must not retire a settled operation");
    }

    #[test]
    fn cancelled_settlement_records_the_real_outcome() {
        let registry = Arc::new(CancelRegistry::new());
        let op = registry.register(51, "loadedSources");
        assert_eq!(
            registry.cancel_request(51),
            CancelDisposition::Accepted { operation: op.operation }
        );
        op.settle(OperationOutcome::Cancelled);

        // The recorded outcome is distinct from accepted-cancel.
        assert_eq!(
            registry.peek_disposition(51),
            CancelDisposition::AlreadyTerminal { operation: op.operation }
        );
    }

    #[test]
    fn sequence_reuse_supersedes_the_prior_operation() {
        let registry = Arc::new(CancelRegistry::new());
        let first = registry.register(61, "evaluate");
        let second = registry.register(61, "evaluate");

        assert!(
            first.is_cancelled(),
            "the superseded operation must be woken, not silently orphaned"
        );
        // The reused sequence maps to the new operation only.
        assert_eq!(
            registry.cancel_request(61),
            CancelDisposition::Accepted { operation: second.operation }
        );
        assert!(second.is_cancelled());
    }

    #[test]
    fn session_teardown_settles_and_clears() {
        let registry = Arc::new(CancelRegistry::new());
        let op = registry.register(71, "stackTrace");
        registry.settle_all();

        assert!(op.is_cancelled(), "teardown must wake the live waiter");
        assert_eq!(
            registry.cancel_request(71),
            CancelDisposition::UnknownTarget,
            "a stale identity from a settled session reaches nothing"
        );
    }

    #[test]
    fn raii_drop_settles_unsettled_operations() {
        let registry = Arc::new(CancelRegistry::new());
        let operation = {
            let op = registry.register(81, "gotoTargets");
            op.operation
        };

        assert_eq!(
            registry.peek_disposition(81),
            CancelDisposition::AlreadyTerminal { operation },
            "a handler exit must settle its operation atomically"
        );
    }

    #[test]
    fn abandoned_settlement_is_independent_of_cancellation() {
        let registry = Arc::new(CancelRegistry::new());
        let operation = {
            // RAII settles the operation as Abandoned when it goes out of
            // scope.
            let op = registry.register(91, "modules");
            op.operation()
        };

        assert_eq!(
            registry.cancel_request(91),
            CancelDisposition::AlreadyTerminal { operation },
            "a late cancel must observe the recorded terminal outcome"
        );
    }

    /// #9074 review: settling runs the retained-entry prune, so the bounded
    /// terminal window holds even when more than the cap goes live and then
    /// finishes without any later `register` call — and live entries are
    /// never evicted by that prune.
    #[test]
    fn settling_an_oversized_live_set_prunes_terminal_records() {
        let registry = Arc::new(CancelRegistry::new());
        let live: Vec<_> = (0..(MAX_RETAINED_ENTRIES as i64 + 16))
            .map(|seq| registry.register(seq, "evaluate"))
            .collect();

        {
            let entries = lock_or_recover(&registry.entries, "cancel_registry.entries");
            assert_eq!(
                entries.len(),
                MAX_RETAINED_ENTRIES + 16,
                "live entries are never evicted, even past the cap"
            );
        }

        for op in &live {
            op.settle(OperationOutcome::Completed);
        }

        {
            let entries = lock_or_recover(&registry.entries, "cancel_registry.entries");
            assert!(
                entries.len() <= MAX_RETAINED_ENTRIES,
                "settled terminal records must be pruned without requiring another \
                 registration: retained {}",
                entries.len()
            );
            assert!(
                entries.values().all(|record| matches!(record.state, RecordState::Terminal(_))),
                "pruning must only ever evict terminal entries"
            );
        }
    }
}
