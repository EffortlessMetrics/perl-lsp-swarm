//! Typed, generation-aware perl5db operation broker (#8564).
//!
//! One internal seam wraps the existing framed-query machinery:
//!
//! ```text
//! typed operation
//! → serialized submission and pending registration
//! → operation identity
//! → correlated output frame or terminal condition
//! → typed result
//! ```
//!
//! The broker owns operation identity, the pending-operation table, session
//! generation, and the typed terminal outcomes. It performs no transport
//! write itself: callers write the framed markers to the debugger transport
//! and submit the operation for correlation, and the broker wraps the
//! begin/end-marker query primitive so that marker framing and the output
//! scan move behind [`OperationBroker::await_framed_payload`] while
//! observable behavior stays identical for existing callers.
//!
//! Deliberate migration debt (#8564): direct native-debugger writes outside
//! this broker still exist. They are registered debt, not silent ownership —
//! follow-up families (inspection, mutation, execution control) migrate onto
//! this seam without inventing another correlation mechanism.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::patterns::{DEBUGGER_FRAME_POLL_MS, RecentOutputBuffer, RecentOutputLine, prompt_re};
use super::sync_utils::lock_or_recover;

/// Bound on simultaneously pending broker operations.
///
/// Submission is serialized and bounded: a session that piles up unclaimed
/// operations is a bug, so the excess is rejected immediately instead of
/// queueing without limit.
pub(crate) const MAX_PENDING_OPERATIONS: usize = 16;

/// What kind of debugger operation is being brokered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationClass {
    /// Framed read-back query (the extracted begin/end-marker primitive).
    Query,
    /// A request-scoped inspection that performs its own bounded scan rather
    /// than using framed transport (for example AST-backed target lookup).
    Inspection,
    /// Write acknowledged by the debugger without payload correlation.
    /// Constructed by the mutation-family migration (#8591), not this PR.
    #[allow(dead_code)]
    Mutation,
    /// Continue/step-family operation interacting with suspension state.
    /// Constructed by the execution-control migration (#8602), not this PR.
    #[allow(dead_code)]
    ExecutionControl,
}

/// Identity of one brokered operation. Unique for the adapter's lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct OperationId(u64);

/// Monotonic session epoch. A generation is superseded by restart,
/// terminate, disconnect, EOF, failed launch, and adapter drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SessionGeneration(u64);

/// Optional suspension epoch for operations that must correlate against one
/// specific stopped state rather than any live session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SuspensionGeneration(u64);

impl SuspensionGeneration {
    pub(crate) fn from_u64(value: u64) -> Self {
        Self(value)
    }

    pub(crate) fn as_u64(self) -> u64 {
        self.0
    }
}

/// Cooperative cancellation flag for one pending operation.
#[derive(Debug, Clone)]
pub(crate) struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub(crate) fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// Retire only the operation holding this token.
    ///
    /// Retire only this operation; request cancellation resolves its token
    /// through the broker-owned request map.
    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

/// Submission request for one brokered operation.
#[derive(Debug)]
pub(crate) struct BrokerOperationSpec {
    /// DAP request sequence, when this operation is cancellable from the
    /// protocol. The broker owns this correlation, not the adapter.
    pub(crate) request_seq: Option<i64>,
    pub(crate) class: OperationClass,
    /// Session epoch the caller observed for this operation. Submission
    /// against a superseded epoch is refused (`StaleGeneration`), so late
    /// callers can never satisfy or queue behind a newer session.
    pub(crate) session_generation: SessionGeneration,
    /// Suspension epoch this operation must correlate against, when the
    /// caller cares about one specific stopped state.
    pub(crate) suspension_generation: Option<SuspensionGeneration>,
    /// Bounded wait budget for the correlated outcome.
    pub(crate) timeout: Duration,
    /// Per-operation cancellation; retirement touches only this operation.
    pub(crate) cancellation: Option<CancellationToken>,
}

/// A pending operation accepted by the broker.
#[derive(Debug, Clone)]
pub(crate) struct BrokerOperation {
    pub(crate) id: OperationId,
    pub(crate) request_seq: Option<i64>,
    /// Operation class is stamped at submission and consumed by the
    /// family-specific migrations (#8591/#8602); the query path does not
    /// branch on it yet.
    #[allow(dead_code)]
    pub(crate) class: OperationClass,
    /// The generation the operation is bound to. Correlation refuses
    /// cross-generation satisfaction by construction (unique markers plus
    /// submit-time generation checks); the field stays on the operation as
    /// the typed receipt of that binding.
    #[allow(dead_code)]
    pub(crate) session_generation: SessionGeneration,
    /// Suspension binding for execution-control migrations (#8602).
    #[allow(dead_code)]
    pub(crate) suspension_generation: Option<SuspensionGeneration>,
    /// Bounded wait budget stamped from the spec at submission.
    pub(crate) timeout: Duration,
    pub(crate) cancellation: Option<CancellationToken>,
}

/// Typed terminal outcomes for a brokered operation.
///
/// Source errors are preserved: rejections, transport failures, and protocol
/// failures carry the reason instead of collapsing into `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BrokerTerminal {
    /// The framed payload was captured between the begin/end markers.
    Completed(Vec<String>),
    /// The mutation was accepted by the debugger (no payload to correlate).
    /// Produced by the mutation-family migration (#8591), not this PR.
    #[allow(dead_code)]
    Acknowledged,
    /// The debugger refused the operation.
    Rejected(String),
    /// Retired by the operation's cancellation token.
    Cancelled,
    /// The deadline elapsed before the end marker arrived.
    TimedOut,
    /// The operation was submitted against a superseded session generation.
    StaleGeneration,
    /// The session ended before the operation settled. The payload names the
    /// settle reason (`debugger_eof`, `terminated`, `restart`, `disconnect`,
    /// `launch_failed`, `adapter_dropped`).
    SessionGone(&'static str),
    /// Writing to the debugger transport failed. Produced by the
    /// mutation/execution-control migrations, not this PR.
    #[allow(dead_code)]
    TransportFailure(String),
    /// Output framing violated the correlation contract. Reserved for the
    /// strict framing migration; the query path still times out instead.
    #[allow(dead_code)]
    ProtocolFailure(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelRequestDisposition {
    Accepted(OperationId),
    AlreadyTerminal(OperationId),
    Unknown,
}

impl BrokerTerminal {
    /// Stable receipt name for logs and tests.
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Completed(_) => "completed",
            Self::Acknowledged => "acknowledged",
            Self::Rejected(_) => "rejected",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::StaleGeneration => "stale_generation",
            Self::SessionGone(_) => "session_gone",
            Self::TransportFailure(_) => "transport_failure",
            Self::ProtocolFailure(_) => "protocol_failure",
        }
    }
}

/// Broker-facing parser contract for one normalized debugger output line
/// (#8564). The three faces are distinct so migrating callers never have to
/// guess whether a line is debugger protocol, prompt state, or debuggee text.
/// Classification is consumed by the caller migrations (#8581/#8591/#8602);
/// this PR establishes the contract.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BrokerFacingLine {
    /// A non-empty line inside a begin/end frame: debugger protocol output.
    DebuggerControlPayload(String),
    /// A perl5db prompt line (`DB<n>`), wherever it appears.
    Prompt(String),
    /// Any line outside a frame: observable debuggee output that must never
    /// be consumed as control payload.
    DebuggeeOutput(String),
}

/// How the framed scan disposes of one buffered line.
enum ScanDisposition {
    /// The begin marker opened this operation's frame.
    Begin,
    /// The end marker closed the frame with the accumulated payload.
    End,
    /// A non-empty payload line inside the frame.
    Payload,
    /// Empty or otherwise ignorable line.
    Ignore,
}

#[derive(Debug)]
struct PendingEntry {
    operation: BrokerOperation,
}

#[derive(Debug, Default)]
struct PendingTable {
    fifo: VecDeque<PendingEntry>,
    /// Terminal outcomes retained until the corresponding waiter observes them.
    settled: HashMap<OperationId, BrokerTerminal>,
    requests: HashMap<i64, OperationId>,
    terminal_requests: HashMap<i64, OperationId>,
    terminal_order: VecDeque<(i64, OperationId)>,
}

/// The typed operation broker. All submission goes through one serialized
/// table; no broker lock is held while waiting for debugger output.
#[derive(Debug)]
pub(crate) struct OperationBroker {
    pending: Mutex<PendingTable>,
    next_operation_id: AtomicU64,
    session_generation: AtomicU64,
    accepting: AtomicBool,
}

impl OperationBroker {
    fn prune_terminal_requests(table: &mut PendingTable) {
        table.terminal_order.retain(|(request_seq, operation)| {
            table.terminal_requests.get(request_seq) == Some(operation)
        });
        while table.terminal_requests.len() > MAX_PENDING_OPERATIONS * 4 {
            let Some((request_seq, operation)) = table.terminal_order.pop_front() else { break };
            if table.terminal_requests.get(&request_seq) == Some(&operation) {
                table.terminal_requests.remove(&request_seq);
            }
        }
    }

    pub(crate) fn new() -> Self {
        Self {
            pending: Mutex::new(PendingTable::default()),
            next_operation_id: AtomicU64::new(1),
            // Session generations and operation identities are distinct
            // types that are never compared, so the two counters cannot be
            // confused even though both start at 1.
            session_generation: AtomicU64::new(1),
            accepting: AtomicBool::new(true),
        }
    }

    /// The current session epoch. Callers stamp this into their
    /// [`BrokerOperationSpec`] so a superseded session refuses the operation.
    pub(crate) fn current_session_generation(&self) -> SessionGeneration {
        SessionGeneration(self.session_generation.load(Ordering::Acquire))
    }

    /// Submit one operation for correlation.
    ///
    /// Serialized and bounded: generation validation, the pending bound, and
    /// registration all run under one acquisition of the table lock, and the
    /// pending bound rejects excess operations instead of queueing them
    /// without limit. Validating under the same lock that
    /// [`Self::settle_all`] clears and bumps under closes the
    /// submit-versus-settle window: an operation can never be registered into
    /// the live table of a session that has already been settled
    /// (#8564 review). Operation identities are minted before validation, so
    /// a refused submission leaves a gap in the id sequence; identities are
    /// unique, never dense.
    pub(crate) fn submit(
        &self,
        spec: BrokerOperationSpec,
    ) -> Result<BrokerOperation, BrokerTerminal> {
        let id = OperationId(self.next_operation_id.fetch_add(1, Ordering::AcqRel));
        let operation = BrokerOperation {
            id,
            request_seq: spec.request_seq,
            class: spec.class,
            session_generation: spec.session_generation,
            suspension_generation: spec.suspension_generation,
            timeout: spec.timeout,
            cancellation: spec.cancellation,
        };

        let mut table = lock_or_recover(&self.pending, "operation_broker.pending");
        if spec.session_generation != self.current_session_generation() {
            return Err(BrokerTerminal::StaleGeneration);
        }
        if !self.accepting.load(Ordering::Acquire) {
            return Err(BrokerTerminal::SessionGone("session_not_ready"));
        }
        if table.fifo.len() >= MAX_PENDING_OPERATIONS {
            return Err(BrokerTerminal::Rejected(format!(
                "broker pending bound exceeded ({MAX_PENDING_OPERATIONS})"
            )));
        }
        table.fifo.push_back(PendingEntry { operation: operation.clone() });
        if let Some(request_seq) = operation.request_seq {
            table.terminal_requests.remove(&request_seq);
            if let Some(previous) = table.requests.insert(request_seq, operation.id) {
                let previous_token = table
                    .fifo
                    .iter()
                    .find(|entry| entry.operation.id == previous)
                    .and_then(|entry| entry.operation.cancellation.clone());
                if let Some(token) = previous_token {
                    token.cancel();
                }
                table.fifo.retain(|entry| entry.operation.id != previous);
                table.settled.insert(previous, BrokerTerminal::Cancelled);
            }
        }
        Ok(operation)
    }

    /// Register a request-scoped operation whose caller performs work outside
    /// the framed transport primitive. The broker still creates the token and
    /// owns its identity/settlement.
    pub(crate) fn register_request(
        self: &Arc<Self>,
        request_seq: i64,
        class: OperationClass,
        timeout: Duration,
    ) -> Result<RegisteredOperation, BrokerTerminal> {
        let operation = self.submit(BrokerOperationSpec {
            request_seq: Some(request_seq),
            class,
            session_generation: self.current_session_generation(),
            suspension_generation: None,
            timeout,
            cancellation: Some(CancellationToken::new()),
        })?;
        Ok(RegisteredOperation { broker: Arc::clone(self), operation })
    }

    /// Cancel the live operation currently named by a DAP request sequence.
    /// Bounded terminal request identities distinguish a late cancel from an
    /// unknown request. Waiter outcomes have a separate operation-keyed lifetime.
    pub(crate) fn cancel_request(&self, request_seq: i64) -> CancelRequestDisposition {
        let table = lock_or_recover(&self.pending, "operation_broker.pending");
        if let Some(id) = table.requests.get(&request_seq).copied()
            && let Some(entry) = table.fifo.iter().find(|entry| entry.operation.id == id)
            && let Some(token) = &entry.operation.cancellation
        {
            token.cancel();
            return CancelRequestDisposition::Accepted(id);
        }
        if let Some(id) = table.terminal_requests.get(&request_seq).copied() {
            return CancelRequestDisposition::AlreadyTerminal(id);
        }
        CancelRequestDisposition::Unknown
    }

    /// Settle a non-framed request operation and return its terminal result,
    /// consuming an earlier supersession or session-settlement outcome if needed.
    /// The terminal request identity remains available for late cancel lookup.
    pub(crate) fn settle_operation(
        &self,
        id: OperationId,
        terminal: BrokerTerminal,
    ) -> BrokerTerminal {
        let mut table = lock_or_recover(&self.pending, "operation_broker.pending");
        let before = table.fifo.len();
        let mut request_seq = None;
        table.fifo.retain(|entry| {
            if entry.operation.id == id {
                request_seq = entry.operation.request_seq;
                false
            } else {
                true
            }
        });
        if let Some(request_seq) = request_seq {
            table.requests.remove(&request_seq);
            table.terminal_requests.insert(request_seq, id);
            table.terminal_order.push_back((request_seq, id));
            Self::prune_terminal_requests(&mut table);
        }
        if before != table.fifo.len() {
            terminal
        } else {
            table.settled.remove(&id).unwrap_or(BrokerTerminal::SessionGone("settled"))
        }
    }

    /// Run one final acceptance step while arbitrating against session
    /// settlement.  The callback must be bounded and must not wait: the
    /// pending-table lock is held so `settle_under_lock` cannot invalidate the
    /// generation between validation and acceptance. The callback must not perform
    /// I/O or acquire another broker lock.
    pub(crate) fn accept_if_current<T>(
        &self,
        expected: SessionGeneration,
        accept: impl FnOnce() -> T,
    ) -> Result<T, BrokerTerminal> {
        let table = lock_or_recover(&self.pending, "operation_broker.pending");
        if expected != self.current_session_generation() {
            return Err(BrokerTerminal::StaleGeneration);
        }
        if !self.accepting.load(Ordering::Acquire) {
            return Err(BrokerTerminal::SessionGone("session_not_ready"));
        }
        let accepted = accept();
        drop(table);
        Ok(accepted)
    }

    /// Whether the operation is still registered as pending.
    fn is_pending(&self, id: OperationId) -> bool {
        lock_or_recover(&self.pending, "operation_broker.pending")
            .fifo
            .iter()
            .any(|entry| entry.operation.id == id)
    }

    /// Retire an operation only if it is still pending. Completion and session
    /// settlement race on this arbitration point; whichever removes the entry
    /// first owns the terminal outcome.
    fn retire_for_completion(&self, id: OperationId) -> bool {
        let mut table = lock_or_recover(&self.pending, "operation_broker.pending");
        let before = table.fifo.len();
        let mut request_seq = None;
        table.fifo.retain(|entry| {
            if entry.operation.id == id {
                request_seq = entry.operation.request_seq;
                false
            } else {
                true
            }
        });
        if let Some(request_seq) = request_seq {
            table.requests.remove(&request_seq);
        }
        if let Some(request_seq) = request_seq {
            table.terminal_requests.insert(request_seq, id);
            table.terminal_order.push_back((request_seq, id));
            Self::prune_terminal_requests(&mut table);
        }
        table.fifo.len() != before
    }

    fn retire_or_settled(&self, id: OperationId, own_terminal: BrokerTerminal) -> BrokerTerminal {
        if self.retire_for_completion(id) {
            own_terminal
        } else {
            self.take_settled(id).unwrap_or(BrokerTerminal::SessionGone("settled"))
        }
    }

    /// Remove a query after transport write failure and consume any terminal
    /// outcome saved by a concurrent session settlement.
    pub(crate) fn retire_after_write_failure(&self, id: OperationId) {
        let _ = self.retire_or_settled(id, BrokerTerminal::TimedOut);
    }

    fn take_settled(&self, id: OperationId) -> Option<BrokerTerminal> {
        lock_or_recover(&self.pending, "operation_broker.pending").settled.remove(&id)
    }

    /// Admit operations for the newly installed session after its transport
    /// is ready. Session teardown closes admission until this is called.
    pub(crate) fn open_session(&self) {
        let _table = lock_or_recover(&self.pending, "operation_broker.pending");
        self.accepting.store(true, Ordering::Release);
    }

    /// Settle every pending operation as `SessionGone` and advance the
    /// session generation.
    ///
    /// Called on EOF, failed launch, disconnect, terminate, restart, and
    /// adapter drop. Waiters observe the removal from the table and the
    /// generation bump; nothing blocks and nothing panics.
    ///
    /// The clear and the generation bump run under one acquisition of the
    /// pending-table lock — the same lock [`Self::submit`] validates and
    /// registers under — so a submit can never interleave between the two
    /// halves of the settle and land a stale operation in the live table
    /// (#8564 review).
    pub(crate) fn settle_all(&self, reason: &'static str) {
        self.settle_under_lock(reason, None);
    }

    /// Settle every pending operation as `SessionGone` only while `expected`
    /// is still the current session generation, reporting whether the settle
    /// applied.
    ///
    /// Reader threads capture the generation at spawn; a stale reader still
    /// draining its pipe after a restart or attach replacement must not clear
    /// the replacement session's pending table or advance its generation when
    /// it finally observes EOF or a read error, so late settles from that
    /// reader are skipped (#8564 review). [`Self::settle_all`] stays the
    /// unconditional form for the generation-transition paths
    /// (`begin_session_generation`, adapter drop) that always own the live
    /// session.
    pub(crate) fn settle_all_if_current(
        &self,
        reason: &'static str,
        expected: SessionGeneration,
    ) -> bool {
        self.settle_under_lock(reason, Some(expected))
    }

    fn settle_under_lock(&self, reason: &'static str, expected: Option<SessionGeneration>) -> bool {
        {
            let mut table = lock_or_recover(&self.pending, "operation_broker.pending");
            if expected.is_some_and(|expected| expected != self.current_session_generation()) {
                tracing::debug!(
                    reason,
                    "operation broker skipped settle for a superseded session generation"
                );
                return false;
            }
            let pending = table.fifo.drain(..).collect::<Vec<_>>();
            for entry in pending {
                if let Some(token) = &entry.operation.cancellation {
                    token.cancel();
                }
                table.settled.insert(entry.operation.id, BrokerTerminal::SessionGone(reason));
            }
            // Session-scoped request identities must never be visible after
            // teardown; waiter outcomes remain keyed by operation identity.
            table.requests.clear();
            table.terminal_requests.clear();
            table.terminal_order.clear();
            self.accepting.store(false, Ordering::Release);
            self.session_generation.fetch_add(1, Ordering::AcqRel);
        }
        tracing::debug!(reason, "operation broker settled all pending operations");
        true
    }

    /// Await the framed payload for one submitted query operation.
    ///
    /// This is the extracted begin/end-marker query primitive: the scan and
    /// polling loop behind the adapter's previous
    /// `capture_framed_debugger_output`, now typed and generation-aware. The
    /// output-buffer lock is taken only for the bounded scan of one poll
    /// pass — never across the sleep or the deadline wait.
    pub(crate) fn await_framed_payload(
        &self,
        operation: &BrokerOperation,
        begin_marker: &str,
        end_marker: &str,
        recent_output: &Arc<Mutex<RecentOutputBuffer>>,
    ) -> BrokerTerminal {
        let deadline = Instant::now() + operation.timeout;
        let mut next_scan_id = 0_u64;
        let mut saw_begin_marker = false;
        let mut framed_lines: Vec<String> = Vec::new();

        loop {
            // Retire checks run before each poll pass. Nested queries share
            // only their owning request's cancellation token.
            if let Some(token) = &operation.cancellation
                && token.is_cancelled()
            {
                return self.retire_or_settled(operation.id, BrokerTerminal::Cancelled);
            }
            // A session-end settle removed this operation from the table.
            if !self.is_pending(operation.id) {
                return self
                    .take_settled(operation.id)
                    .unwrap_or(BrokerTerminal::SessionGone("settled"));
            }

            {
                let output = lock_or_recover(recent_output, "operation_broker.recent_output");
                for line in output.lines.iter().filter(|line| line.id >= next_scan_id) {
                    match Self::dispose_of_scanned_line(
                        line,
                        &mut saw_begin_marker,
                        begin_marker,
                        end_marker,
                        &mut framed_lines,
                    ) {
                        ScanDisposition::End => {
                            if self.retire_for_completion(operation.id) {
                                return BrokerTerminal::Completed(std::mem::take(
                                    &mut framed_lines,
                                ));
                            }
                            return self
                                .take_settled(operation.id)
                                .unwrap_or(BrokerTerminal::SessionGone("settled"));
                        }
                        ScanDisposition::Begin | ScanDisposition::Payload => {}
                        ScanDisposition::Ignore => {}
                    }
                }

                if let Some(last) = output.lines.back() {
                    next_scan_id = last.id.saturating_add(1);
                }
            }

            if Instant::now() >= deadline {
                return self.retire_or_settled(operation.id, BrokerTerminal::TimedOut);
            }

            std::thread::sleep(Duration::from_millis(DEBUGGER_FRAME_POLL_MS));
        }
    }

    /// Classify one normalized line under the broker-facing parser contract.
    ///
    /// `inside_frame` marks lines observed between this operation's begin and
    /// end markers. Prompt-shaped lines are Prompt wherever they appear; only
    /// in-frame non-empty lines are control payload; everything else is
    /// observable debuggee output.
    #[allow(dead_code)]
    pub(crate) fn classify_broker_line(normalized: &str, inside_frame: bool) -> BrokerFacingLine {
        let trimmed = normalized.trim();
        if prompt_re().is_some_and(|re| re.is_match(trimmed)) {
            return BrokerFacingLine::Prompt(normalized.to_string());
        }
        if inside_frame && !trimmed.is_empty() {
            return BrokerFacingLine::DebuggerControlPayload(normalized.to_string());
        }
        BrokerFacingLine::DebuggeeOutput(normalized.to_string())
    }

    /// Whether `line` contains `marker` as a whole word.
    ///
    /// Marker-adjacent word characters disqualify the hit, so marker-like
    /// text inside a value (`DAP_END_1x`, `xDAP_END_1`) can never close a
    /// frame that did not open for it.
    pub(crate) fn line_contains_full_marker(line: &str, marker: &str) -> bool {
        line.match_indices(marker).any(|(idx, _)| {
            let before = line[..idx].chars().next_back();
            let after = line[idx + marker.len()..].chars().next();
            let before_ok =
                before.is_none_or(|ch| !matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_'));
            let after_ok =
                after.is_none_or(|ch| !matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_'));
            before_ok && after_ok
        })
    }

    fn dispose_of_scanned_line(
        line: &RecentOutputLine,
        saw_begin_marker: &mut bool,
        begin_marker: &str,
        end_marker: &str,
        framed_lines: &mut Vec<String>,
    ) -> ScanDisposition {
        if !*saw_begin_marker {
            if Self::line_contains_full_marker(&line.normalized, begin_marker) {
                *saw_begin_marker = true;
                framed_lines.clear();
                return ScanDisposition::Begin;
            }
            return ScanDisposition::Ignore;
        }
        if Self::line_contains_full_marker(&line.normalized, end_marker) {
            return ScanDisposition::End;
        }
        if !line.normalized.trim().is_empty() {
            framed_lines.push(line.normalized.clone());
            return ScanDisposition::Payload;
        }
        ScanDisposition::Ignore
    }
}

/// RAII lease for a request-scoped broker operation. Dropping an unsettled
/// lease records an abandoned terminal outcome and removes its request map.
#[derive(Debug)]
pub(crate) struct RegisteredOperation {
    broker: Arc<OperationBroker>,
    operation: BrokerOperation,
}

impl RegisteredOperation {
    #[allow(dead_code)]
    pub(crate) fn id(&self) -> OperationId {
        self.operation.id
    }
    pub(crate) fn token(&self) -> Option<&CancellationToken> {
        self.operation.cancellation.as_ref()
    }
    pub(crate) fn session_generation(&self) -> SessionGeneration {
        self.operation.session_generation
    }
    pub(crate) fn settle(self, terminal: BrokerTerminal) -> BrokerTerminal {
        self.broker.settle_operation(self.operation.id, terminal)
    }
}

impl Drop for RegisteredOperation {
    fn drop(&mut self) {
        let _ = self
            .broker
            .settle_operation(self.operation.id, BrokerTerminal::SessionGone("request_dropped"));
    }
}

impl Default for OperationBroker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]
    use super::super::patterns::{RecentOutputBuffer, RecentOutputLine};
    use super::lock_or_recover;
    use super::{
        BrokerFacingLine, BrokerOperation, BrokerOperationSpec, BrokerTerminal,
        CancelRequestDisposition, CancellationToken, MAX_PENDING_OPERATIONS, OperationBroker,
        OperationClass, RegisteredOperation,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::{Duration, Instant};

    /// Short await budget for negative waits: long enough to survive poll
    /// scheduling on a loaded host, short enough to keep the suite fast.
    const NEGATIVE_WAIT: Duration = Duration::from_millis(60);

    fn buffer_with(lines: &[&str]) -> Arc<Mutex<RecentOutputBuffer>> {
        let mut buffer = RecentOutputBuffer::new();
        for text in lines {
            let id = buffer.next_line_id;
            buffer.next_line_id += 1;
            buffer.lines.push_back(RecentOutputLine { id, normalized: (*text).to_string() });
        }
        Arc::new(Mutex::new(buffer))
    }

    fn query_spec(broker: &OperationBroker, timeout: Duration) -> BrokerOperationSpec {
        BrokerOperationSpec {
            request_seq: None,
            class: OperationClass::Query,
            session_generation: broker.current_session_generation(),
            suspension_generation: None,
            timeout,
            cancellation: None,
        }
    }

    #[test]
    fn request_cancel_targets_only_the_current_registered_operation() -> Result<(), String> {
        let broker = Arc::new(OperationBroker::new());
        let first = broker
            .register_request(41, OperationClass::Inspection, Duration::from_secs(1))
            .map_err(|error| format!("register first: {error:?}"))?;
        let second = broker
            .register_request(41, OperationClass::Inspection, Duration::from_secs(1))
            .map_err(|error| format!("register replacement: {error:?}"))?;
        if !first.token().ok_or("first token missing")?.is_cancelled() {
            return Err("reusing request sequence did not retire the prior token".to_string());
        }
        let targeted = broker.cancel_request(41);
        if targeted != CancelRequestDisposition::Accepted(second.id())
            || !second.token().ok_or("second token missing")?.is_cancelled()
        {
            return Err("cancel reached the wrong request operation".to_string());
        }
        Ok(())
    }

    #[test]
    fn settling_request_removes_mapping_and_raii_cleans_pending_entry() -> Result<(), String> {
        let broker = Arc::new(OperationBroker::new());
        let operation = broker
            .register_request(42, OperationClass::Inspection, Duration::from_secs(1))
            .map_err(|error| format!("register: {error:?}"))?;
        let id = operation.id();
        let terminal = operation.settle(BrokerTerminal::Completed(Vec::new()));
        if terminal != BrokerTerminal::Completed(Vec::new()) {
            return Err(format!("unexpected terminal outcome: {terminal:?}"));
        }
        if broker.cancel_request(42) != CancelRequestDisposition::AlreadyTerminal(id) {
            return Err("settled request remained cancellable".to_string());
        }
        if broker.settle_operation(id, BrokerTerminal::TimedOut)
            != BrokerTerminal::SessionGone("settled")
        {
            return Err("settling an already removed request changed its outcome".to_string());
        }
        Ok(())
    }

    #[test]
    fn session_cleanup_preserves_unread_waiter_outcomes() -> Result<(), String> {
        let broker = Arc::new(OperationBroker::new());
        let mut operations = Vec::new();
        for request in 0..70_i64 {
            operations.push(
                broker
                    .register_request(request, OperationClass::Inspection, NEGATIVE_WAIT)
                    .map_err(|error| format!("register: {error:?}"))?,
            );
            broker.settle_all("terminated");
            broker.open_session();
        }
        let ids = operations.iter().map(RegisteredOperation::id).collect::<Vec<_>>();
        for id in ids {
            if broker.take_settled(id) != Some(BrokerTerminal::SessionGone("terminated")) {
                return Err(format!("unread terminal for {id:?} was discarded"));
            }
        }
        Ok(())
    }

    #[test]
    fn terminal_window_keeps_newest_results_and_live_requests() -> Result<(), String> {
        let broker = Arc::new(OperationBroker::new());
        let live = broker
            .register_request(1000, OperationClass::Inspection, NEGATIVE_WAIT)
            .map_err(|error| format!("live registration: {error:?}"))?;
        let mut completed = Vec::new();
        for seq in 0..70 {
            let operation = broker
                .register_request(seq, OperationClass::Inspection, NEGATIVE_WAIT)
                .map_err(|error| format!("registration {seq}: {error:?}"))?;
            completed.push((seq, operation.id()));
            if operation.settle(BrokerTerminal::Completed(Vec::new()))
                != BrokerTerminal::Completed(Vec::new())
            {
                return Err(format!("request {seq} did not complete"));
            }
            if live.token().ok_or("live token missing")?.is_cancelled() {
                return Err("terminal pruning cancelled a live request".into());
            }
        }
        for (seq, id) in completed {
            let expected = if seq < 6 {
                CancelRequestDisposition::Unknown
            } else {
                CancelRequestDisposition::AlreadyTerminal(id)
            };
            if broker.cancel_request(seq) != expected {
                return Err(format!("incorrect terminal window at request {seq}"));
            }
        }
        if broker.cancel_request(1000) != CancelRequestDisposition::Accepted(live.id()) {
            return Err("terminal pruning removed a live request".into());
        }
        Ok(())
    }

    #[test]
    fn repeated_request_reuse_bounds_terminal_order() -> Result<(), String> {
        let broker = Arc::new(OperationBroker::new());
        for _ in 0..1000 {
            let operation = broker
                .register_request(77, OperationClass::Inspection, NEGATIVE_WAIT)
                .map_err(|error| format!("register: {error:?}"))?;
            let _ = operation.settle(BrokerTerminal::Completed(Vec::new()));
        }
        let table = lock_or_recover(&broker.pending, "test.pending");
        if table.terminal_requests.len() > MAX_PENDING_OPERATIONS * 4
            || table.terminal_order.len() > MAX_PENDING_OPERATIONS * 4 + 1
        {
            return Err(format!(
                "terminal retention grew without bound: map={}, order={}",
                table.terminal_requests.len(),
                table.terminal_order.len()
            ));
        }
        Ok(())
    }

    #[test]
    fn teardown_cancels_and_forgets_old_request_mapping() -> Result<(), String> {
        let broker = Arc::new(OperationBroker::new());
        let operation = broker
            .register_request(88, OperationClass::Inspection, NEGATIVE_WAIT)
            .map_err(|error| format!("register: {error:?}"))?;
        broker.settle_all("disconnect");
        if !operation.token().ok_or("token missing")?.is_cancelled() {
            return Err("teardown did not cancel the active token".to_string());
        }
        if broker.cancel_request(88) != CancelRequestDisposition::Unknown {
            return Err("teardown retained the old request mapping".to_string());
        }
        broker.open_session();
        let fresh = broker
            .register_request(88, OperationClass::Inspection, NEGATIVE_WAIT)
            .map_err(|error| format!("register fresh: {error:?}"))?;
        if fresh.token().ok_or("fresh token missing")?.is_cancelled() {
            return Err("fresh session inherited old cancellation".to_string());
        }
        Ok(())
    }

    #[test]
    fn dropping_old_lease_cannot_remove_replacement_mapping() -> Result<(), String> {
        let broker = Arc::new(OperationBroker::new());
        let old = broker
            .register_request(99, OperationClass::Inspection, NEGATIVE_WAIT)
            .map_err(|error| format!("register old: {error:?}"))?;
        let replacement = broker
            .register_request(99, OperationClass::Inspection, NEGATIVE_WAIT)
            .map_err(|error| format!("register replacement: {error:?}"))?;
        drop(old);
        if broker.cancel_request(99) != CancelRequestDisposition::Accepted(replacement.id()) {
            return Err("old lease removed the replacement request mapping".to_string());
        }
        Ok(())
    }

    #[test]
    fn final_acceptance_rejects_generation_after_settle() -> Result<(), String> {
        let broker = OperationBroker::new();
        let expected = broker.current_session_generation();
        broker.settle_all("terminated");

        let result = broker.accept_if_current(expected, || "accepted");
        if result != Err(BrokerTerminal::StaleGeneration) {
            return Err(format!("settled generation was accepted: {result:?}"));
        }
        Ok(())
    }

    #[test]
    fn final_acceptance_serializes_before_later_settle() -> Result<(), String> {
        let broker = OperationBroker::new();
        let expected = broker.current_session_generation();
        let accepted = broker.accept_if_current(expected, || 42);
        if accepted != Ok(42) {
            return Err(format!("current generation was not accepted: {accepted:?}"));
        }

        // Settlement after the acceptance is a later linearization point. It
        // may invalidate the session afterward, but cannot retroactively
        // turn the already-committed callback into a stale acceptance.
        broker.settle_all("restart");
        let rejected = broker.accept_if_current(expected, || 7);
        if rejected != Err(BrokerTerminal::StaleGeneration) {
            return Err(format!("settled generation was accepted again: {rejected:?}"));
        }
        Ok(())
    }

    #[test]
    fn final_acceptance_excludes_settlement_during_callback() -> Result<(), String> {
        // The callback performs only bounded, nonblocking work. A competing
        // settle is started after callback entry; with the arbitration lock it
        // must wait until the callback has returned.
        for _ in 0..16 {
            let broker = Arc::new(OperationBroker::new());
            let expected = broker.current_session_generation();
            let (entered_tx, entered_rx) = mpsc::sync_channel(1);
            let settled = Arc::new(AtomicBool::new(false));
            let overlapped = Arc::new(AtomicBool::new(false));
            let callback_reentered = Arc::new(AtomicBool::new(false));

            let accepting = {
                let broker = Arc::clone(&broker);
                let broker_for_probe = Arc::clone(&broker);
                let settled = Arc::clone(&settled);
                let overlapped = Arc::clone(&overlapped);
                let callback_reentered = Arc::clone(&callback_reentered);
                std::thread::spawn(move || {
                    broker.accept_if_current(expected, || {
                        if !matches!(
                            broker_for_probe.pending.try_lock(),
                            Err(std::sync::TryLockError::WouldBlock)
                        ) {
                            callback_reentered.store(true, Ordering::Release);
                        }
                        let _ = entered_tx.try_send(());
                        for _ in 0..250_000 {
                            if settled.load(Ordering::Acquire) {
                                overlapped.store(true, Ordering::Release);
                            }
                            std::hint::spin_loop();
                        }
                        42
                    })
                })
            };
            let settling = {
                let broker = Arc::clone(&broker);
                let settled = Arc::clone(&settled);
                std::thread::spawn(move || {
                    entered_rx
                        .recv_timeout(Duration::from_secs(1))
                        .map_err(|_| "acceptance callback did not start")?;
                    broker.settle_all("terminated");
                    settled.store(true, Ordering::Release);
                    Ok::<(), String>(())
                })
            };

            let accepted = accepting.join().map_err(|_| "acceptance thread panicked")?;
            settling
                .join()
                .map_err(|_| "settlement thread panicked")?
                .map_err(|error| error.to_string())?;
            if accepted != Ok(42) {
                return Err(format!("current acceptance was rejected: {accepted:?}"));
            }
            if overlapped.load(Ordering::Acquire) {
                return Err("settlement entered while final acceptance callback was running".into());
            }
            if callback_reentered.load(Ordering::Acquire) {
                return Err("acceptance callback reacquired the broker arbitration lock".into());
            }
        }
        Ok(())
    }

    fn markers(operation: &BrokerOperation) -> (String, String) {
        (format!("DAP_BEGIN_{}", operation.id.0), format!("DAP_END_{}", operation.id.0))
    }

    fn payload(lines: &[String]) -> Vec<String> {
        lines.to_vec()
    }

    #[test]
    fn fifo_submission_correlates_each_request_with_its_own_frame() {
        let broker = OperationBroker::new();
        let first = broker.submit(query_spec(&broker, NEGATIVE_WAIT)).expect("first submit");
        let second = broker.submit(query_spec(&broker, NEGATIVE_WAIT)).expect("second submit");

        assert!(
            first.id < second.id,
            "submission order is FIFO: first operation gets the smaller identity"
        );

        // Only the FIRST operation's frame is in the buffer.
        let (begin, end) = markers(&first);
        let output = buffer_with(&[&begin, "$x = 42", &end]);

        let terminal = broker.await_framed_payload(&first, &begin, &end, &output);
        assert_eq!(
            terminal,
            BrokerTerminal::Completed(payload(&["$x = 42".to_string()])),
            "the first operation correlates against its own frame"
        );

        // The second operation's markers never arrive: it times out instead
        // of being satisfied by the first operation's frame.
        let (begin2, end2) = markers(&second);
        let terminal = broker.await_framed_payload(&second, &begin2, &end2, &output);
        assert_eq!(terminal, BrokerTerminal::TimedOut);
    }

    #[test]
    fn marker_like_text_inside_values_cannot_close_another_operation() {
        let broker = OperationBroker::new();
        let operation = broker.submit(query_spec(&broker, NEGATIVE_WAIT)).expect("submit");
        let (begin, end) = markers(&operation);

        // The corrupted close marker carries word characters adjacent to the
        // marker text, so it must not close the frame; the real close marker
        // below it does.
        let output = buffer_with(&[&begin, "value DAP_END_1x stays inside", "x1DAP_END_1", &end]);

        let terminal = broker.await_framed_payload(&operation, &begin, &end, &output);
        assert_eq!(
            terminal,
            BrokerTerminal::Completed(payload(&[
                "value DAP_END_1x stays inside".to_string(),
                "x1DAP_END_1".to_string(),
            ])),
            "marker-adjacent word characters disqualify a close hit"
        );
    }

    #[test]
    fn prompt_lines_classify_as_prompt_before_and_after_a_framed_payload() {
        let before = OperationBroker::classify_broker_line("DB<1> ", false);
        let after = OperationBroker::classify_broker_line("  DB<2>", false);
        let inside = OperationBroker::classify_broker_line("DB<3>", true);

        assert_eq!(before, BrokerFacingLine::Prompt("DB<1> ".to_string()));
        assert_eq!(after, BrokerFacingLine::Prompt("  DB<2>".to_string()));
        assert_eq!(inside, BrokerFacingLine::Prompt("DB<3>".to_string()));

        let control = OperationBroker::classify_broker_line("$x = 1", true);
        let debuggee = OperationBroker::classify_broker_line("$x = 1", false);
        assert_eq!(control, BrokerFacingLine::DebuggerControlPayload("$x = 1".to_string()));
        assert_eq!(debuggee, BrokerFacingLine::DebuggeeOutput("$x = 1".to_string()));
    }

    #[test]
    fn cancellation_retires_only_the_intended_operation() {
        let broker = OperationBroker::new();
        let mut cancelled_spec = query_spec(&broker, NEGATIVE_WAIT);
        let token = CancellationToken::new();
        cancelled_spec.cancellation = Some(token.clone());
        let cancelled = broker.submit(cancelled_spec).expect("cancelled submit");
        let survivor = broker.submit(query_spec(&broker, NEGATIVE_WAIT)).expect("survivor submit");

        token.cancel();

        let (cancelled_begin, cancelled_end) = markers(&cancelled);
        let output = Arc::new(Mutex::new(RecentOutputBuffer::new()));
        let terminal =
            broker.await_framed_payload(&cancelled, &cancelled_begin, &cancelled_end, &output);
        assert_eq!(terminal, BrokerTerminal::Cancelled);

        // The survivor still correlates normally after its neighbour retired.
        let (begin, end) = markers(&survivor);
        let filled = buffer_with(&[&begin, "ok", &end]);
        let terminal = broker.await_framed_payload(&survivor, &begin, &end, &filled);
        assert_eq!(
            terminal,
            BrokerTerminal::Completed(payload(&["ok".to_string()])),
            "cancellation of one operation must not touch the other pending operation"
        );
    }

    #[test]
    fn settlement_wins_cancellation_and_consumes_saved_terminal() {
        let broker = OperationBroker::new();
        let operation = broker.submit(query_spec(&broker, NEGATIVE_WAIT)).expect("submit");
        broker.settle_all("debugger_eof");

        assert_eq!(
            broker.retire_or_settled(operation.id, BrokerTerminal::Cancelled),
            BrokerTerminal::SessionGone("debugger_eof")
        );
        assert!(broker.take_settled(operation.id).is_none());
    }

    #[test]
    fn settlement_wins_timeout_and_consumes_saved_terminal() {
        let broker = OperationBroker::new();
        let operation = broker.submit(query_spec(&broker, NEGATIVE_WAIT)).expect("submit");
        broker.settle_all("restart");

        assert_eq!(
            broker.retire_or_settled(operation.id, BrokerTerminal::TimedOut),
            BrokerTerminal::SessionGone("restart")
        );
        assert!(broker.take_settled(operation.id).is_none());
    }

    #[test]
    fn write_failure_cleanup_consumes_settled_terminal() {
        let broker = OperationBroker::new();
        let operation = broker.submit(query_spec(&broker, NEGATIVE_WAIT)).expect("submit");
        broker.settle_all("debugger_eof");

        broker.retire_after_write_failure(operation.id);
        assert!(broker.take_settled(operation.id).is_none());
    }

    #[test]
    fn timeout_retires_only_the_intended_operation() {
        let broker = OperationBroker::new();
        let timed_out =
            broker.submit(query_spec(&broker, NEGATIVE_WAIT)).expect("timed-out submit");
        let survivor =
            broker.submit(query_spec(&broker, Duration::from_secs(1))).expect("survivor submit");

        let (timed_out_begin, timed_out_end) = markers(&timed_out);
        let empty = Arc::new(Mutex::new(RecentOutputBuffer::new()));
        let terminal =
            broker.await_framed_payload(&timed_out, &timed_out_begin, &timed_out_end, &empty);
        assert_eq!(terminal, BrokerTerminal::TimedOut);

        // Retiring the expired operation must leave its sibling registered,
        // so a frame arriving afterwards still resolves the survivor.
        let (survivor_begin, survivor_end) = markers(&survivor);
        let output = buffer_with(&[&survivor_begin, "ok", &survivor_end]);
        let terminal =
            broker.await_framed_payload(&survivor, &survivor_begin, &survivor_end, &output);
        assert_eq!(
            terminal,
            BrokerTerminal::Completed(payload(&["ok".to_string()])),
            "timeout of one operation must not touch the other pending operation"
        );
    }

    #[test]
    fn stale_generation_submission_is_refused_before_queueing() {
        let broker = Arc::new(OperationBroker::new());
        let stale_generation = broker.current_session_generation();
        broker.settle_all("restart");

        let spec = BrokerOperationSpec {
            request_seq: None,
            class: OperationClass::Query,
            session_generation: stale_generation,
            suspension_generation: None,
            timeout: NEGATIVE_WAIT,
            cancellation: None,
        };

        assert!(
            matches!(broker.submit(spec), Err(BrokerTerminal::StaleGeneration)),
            "an operation submitted against a superseded session is refused, never queued"
        );
    }

    #[test]
    fn closed_session_refuses_new_submissions_until_transport_ready() {
        let broker = OperationBroker::new();
        let old_generation = broker.current_session_generation();
        broker.settle_all("restart");
        let current_generation = broker.current_session_generation();

        let refused = broker.submit(query_spec(&broker, NEGATIVE_WAIT));
        assert!(
            matches!(refused, Err(BrokerTerminal::SessionGone("session_not_ready"))),
            "a settled transport must close admission for the replacement gap: {refused:?}"
        );
        assert_ne!(old_generation, current_generation);

        broker.open_session();
        assert!(broker.submit(query_spec(&broker, NEGATIVE_WAIT)).is_ok());
    }

    #[test]
    fn settled_operation_cannot_complete_from_late_frame() {
        let broker = OperationBroker::new();
        let operation = broker.submit(query_spec(&broker, NEGATIVE_WAIT)).expect("submit");
        let (begin, end) = markers(&operation);
        broker.settle_all("restart");
        let output = buffer_with(&[&begin, "late", &end]);

        let terminal = broker.await_framed_payload(&operation, &begin, &end, &output);
        assert_eq!(
            terminal,
            BrokerTerminal::SessionGone("restart"),
            "a late frame from the closed generation cannot win completion arbitration"
        );
    }

    #[test]
    fn session_end_settles_waiters_without_panic_or_deadlock() {
        let broker = Arc::new(OperationBroker::new());
        let operation = broker.submit(query_spec(&broker, Duration::from_mins(1))).expect("submit");
        let (begin, end) = markers(&operation);
        let output = Arc::new(Mutex::new(RecentOutputBuffer::new()));

        let waiter = {
            let broker = Arc::clone(&broker);
            let operation = operation.clone();
            std::thread::spawn(move || {
                broker.await_framed_payload(&operation, &begin, &end, &output)
            })
        };

        std::thread::sleep(Duration::from_millis(40));
        broker.settle_all("terminated");

        let terminal = waiter.join().expect("waiter thread must not panic");
        assert!(
            matches!(terminal, BrokerTerminal::SessionGone("terminated")),
            "session end must settle the waiter: got {terminal:?}"
        );
    }

    #[test]
    fn submit_racing_a_settle_never_queues_a_stale_generation_operation() {
        // Regression (#8564 review P1): `submit` validated the session
        // generation outside the pending-table lock while `settle_all`
        // cleared the table and bumped the generation outside it. A submit
        // that passed validation and was then parked on the table lock could
        // insert its stale-generation operation into the live table AFTER the
        // settle completed; the waiter then spun to its full budget and
        // reported `TimedOut` instead of `SessionGone`. Validation and
        // insertion must be atomic against the settle's clear-and-bump under
        // one lock ordering.
        for _ in 0..64 {
            let broker = Arc::new(OperationBroker::new());
            let racing_generation = broker.current_session_generation();

            // Hold the table lock so the spawned submit must park at (or
            // past) its validation point instead of completing.
            let held = lock_or_recover(&broker.pending, "test hold pending");
            let submitter = {
                let broker = Arc::clone(&broker);
                std::thread::spawn(move || {
                    broker.submit(BrokerOperationSpec {
                        request_seq: None,
                        class: OperationClass::Query,
                        session_generation: racing_generation,
                        suspension_generation: None,
                        timeout: Duration::from_mins(1),
                        cancellation: None,
                    })
                })
            };

            // Operation identities are minted before the table lock, so the
            // advanced counter proves the submitter is in flight and contending
            // for the lock rather than still waiting to run.
            let parked = Instant::now() + Duration::from_secs(5);
            while broker.next_operation_id.load(Ordering::Acquire) == 1 {
                assert!(Instant::now() < parked, "submitter never reached the pending-table lock");
                std::thread::sleep(Duration::from_millis(1));
            }

            // With the submitter in flight, run a full settle. The submitter
            // must never queue its operation into the live table of the
            // settled (bumped) generation.
            drop(held);
            broker.settle_all("restart");

            let submitted = submitter.join().expect("submitter thread must not panic");
            match submitted {
                // Refused at the validation point because the settle won.
                Err(BrokerTerminal::StaleGeneration) => {}
                Ok(operation) => {
                    // `Ok` is only acceptable when the insert happened while
                    // the generation was still current (the settle then
                    // retired it) — never when a stale operation survives in
                    // the live table of a superseded session.
                    let still_pending = lock_or_recover(&broker.pending, "test inspect pending")
                        .fifo
                        .iter()
                        .any(|entry| entry.operation.id == operation.id);
                    assert!(
                        !still_pending
                            || broker.current_session_generation() == operation.session_generation,
                        "stale operation queued into the live table: operation generation \
                         {:?}, current generation {:?}",
                        operation.session_generation,
                        broker.current_session_generation(),
                    );
                }
                Err(other) => {
                    assert!(
                        matches!(other, BrokerTerminal::Rejected(_)),
                        "unexpected submit outcome: {other:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn stale_reader_settle_does_not_clear_a_newer_generation() {
        // Regression (#8564 review P1): reader threads settle pending
        // operations on EOF/read-error, but a reader that spawns, then loses
        // its session to a restart or attach replacement, must not clear the
        // replacement session's pending table or advance its generation when
        // it finally drains its pipe. A settle is only applied while the
        // caller's captured generation is still current.
        let broker = OperationBroker::new();

        // The reader captured its generation at spawn...
        let reader_generation = broker.current_session_generation();

        // ...but a replacement session advanced the epoch and queued its own
        // operation before the stale reader observed EOF.
        broker.settle_all("restart");
        broker.open_session();
        let replacement =
            broker.submit(query_spec(&broker, Duration::from_mins(1))).expect("replacement submit");
        let replacement_generation = broker.current_session_generation();

        let settled = broker.settle_all_if_current("debugger_eof", reader_generation);
        assert!(!settled, "a stale reader's settle must be skipped, not applied");
        assert!(
            broker.is_pending(replacement.id),
            "the replacement session's pending operation must survive a stale reader's settle"
        );
        assert_eq!(
            broker.current_session_generation(),
            replacement_generation,
            "a skipped settle must not advance the replacement session's generation"
        );

        // The live session's own settle path still applies and retires the
        // operation.
        assert!(broker.settle_all_if_current("terminated", replacement_generation));
        assert!(!broker.is_pending(replacement.id));
    }

    #[test]
    fn unframed_debuggee_output_is_observable_but_never_consumed_as_payload() {
        let broker = OperationBroker::new();
        let operation = broker.submit(query_spec(&broker, NEGATIVE_WAIT)).expect("submit");
        let (begin, end) = markers(&operation);

        // Debuggee chatter before the begin marker and after the end marker
        // frames: observable in the buffer, never part of the payload.
        let output =
            buffer_with(&["debuggee: starting", &begin, "$result", &end, "debuggee: exiting"]);

        let terminal = broker.await_framed_payload(&operation, &begin, &end, &output);
        assert_eq!(
            terminal,
            BrokerTerminal::Completed(payload(&["$result".to_string()])),
            "only in-frame lines become control payload"
        );
        let buffer = output.lock().expect("test buffer lock");
        assert_eq!(buffer.lines.len(), 5, "debuggee output stays observable in the buffer");
    }

    #[test]
    fn settle_path_does_not_require_the_output_buffer_lock() {
        let broker = OperationBroker::new();
        let operation = broker.submit(query_spec(&broker, Duration::from_mins(1))).expect("submit");
        let output = Arc::new(Mutex::new(RecentOutputBuffer::new()));

        // Hold the output-buffer lock externally: the settle path touches only
        // the pending table, so it must complete while the lock is held.
        let _held = output.lock().expect("hold output lock for the test");
        broker.settle_all("disconnect");

        let (begin, end) = markers(&operation);
        let terminal = broker.await_framed_payload(&operation, &begin, &end, &output);
        assert!(
            matches!(terminal, BrokerTerminal::SessionGone(_)),
            "a settled operation reports session-gone even under a held output lock: {terminal:?}"
        );
    }
}
