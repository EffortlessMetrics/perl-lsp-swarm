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

use std::collections::VecDeque;
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

/// Cooperative cancellation flag for one pending operation.
#[derive(Debug, Clone)]
pub(crate) struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub(crate) fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// Retire only the operation holding this token.
    ///
    /// The adapter-side request paths still retire through the shared
    /// `cancel_requested` flag; per-operation cancellation callers arrive
    /// with the caller migrations (#8581 and siblings).
    #[allow(dead_code)]
    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn is_cancelled(&self) -> bool {
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
}

/// The typed operation broker. All submission goes through one serialized
/// table; no broker lock is held while waiting for debugger output.
#[derive(Debug)]
pub(crate) struct OperationBroker {
    pending: Mutex<PendingTable>,
    next_operation_id: AtomicU64,
    session_generation: AtomicU64,
}

impl OperationBroker {
    pub(crate) fn new() -> Self {
        Self {
            pending: Mutex::new(PendingTable::default()),
            next_operation_id: AtomicU64::new(1),
            // Session generations and operation identities are distinct
            // types that are never compared, so the two counters cannot be
            // confused even though both start at 1.
            session_generation: AtomicU64::new(1),
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
        if table.fifo.len() >= MAX_PENDING_OPERATIONS {
            return Err(BrokerTerminal::Rejected(format!(
                "broker pending bound exceeded ({MAX_PENDING_OPERATIONS})"
            )));
        }
        table.fifo.push_back(PendingEntry { operation: operation.clone() });
        Ok(operation)
    }

    /// Whether the operation is still registered as pending.
    fn is_pending(&self, id: OperationId) -> bool {
        lock_or_recover(&self.pending, "operation_broker.pending")
            .fifo
            .iter()
            .any(|entry| entry.operation.id == id)
    }

    /// Retire one operation from the table (cancellation, timeout).
    pub(crate) fn retire(&self, id: OperationId) {
        let mut table = lock_or_recover(&self.pending, "operation_broker.pending");
        table.fifo.retain(|entry| entry.operation.id != id);
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
            table.fifo.clear();
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
        shared_cancel: &AtomicBool,
    ) -> BrokerTerminal {
        let deadline = Instant::now() + operation.timeout;
        let mut next_scan_id = 0_u64;
        let mut saw_begin_marker = false;
        let mut framed_lines: Vec<String> = Vec::new();

        loop {
            // Retire checks run before each poll pass: per-operation
            // cancellation touches only this operation; the shared flag is
            // the outer request's cancellation and is consumed as before.
            if let Some(token) = &operation.cancellation
                && token.is_cancelled()
            {
                self.retire(operation.id);
                return BrokerTerminal::Cancelled;
            }
            if shared_cancel.load(Ordering::Acquire) {
                shared_cancel.store(false, Ordering::Release);
                self.retire(operation.id);
                return BrokerTerminal::Cancelled;
            }
            // A session-end settle removed this operation from the table.
            if !self.is_pending(operation.id) {
                return BrokerTerminal::SessionGone("settled");
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
                            self.retire(operation.id);
                            return BrokerTerminal::Completed(std::mem::take(&mut framed_lines));
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
                self.retire(operation.id);
                return BrokerTerminal::TimedOut;
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
        BrokerFacingLine, BrokerOperation, BrokerOperationSpec, BrokerTerminal, CancellationToken,
        OperationBroker, OperationClass,
    };
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// Short await budget for negative waits: long enough to survive poll
    /// scheduling on a loaded host, short enough to keep the suite fast.
    const NEGATIVE_WAIT: Duration = Duration::from_millis(60);

    fn buffer_with(lines: &[&str]) -> Arc<Mutex<RecentOutputBuffer>> {
        let mut buffer = RecentOutputBuffer::new();
        for text in lines {
            let id = buffer.next_line_id;
            buffer.next_line_id += 1;
            buffer.lines.push_back(RecentOutputLine {
                id,
                raw: (*text).to_string(),
                normalized: (*text).to_string(),
            });
        }
        Arc::new(Mutex::new(buffer))
    }

    fn query_spec(broker: &OperationBroker, timeout: Duration) -> BrokerOperationSpec {
        BrokerOperationSpec {
            class: OperationClass::Query,
            session_generation: broker.current_session_generation(),
            suspension_generation: None,
            timeout,
            cancellation: None,
        }
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

        let terminal =
            broker.await_framed_payload(&first, &begin, &end, &output, &Default::default());
        assert_eq!(
            terminal,
            BrokerTerminal::Completed(payload(&["$x = 42".to_string()])),
            "the first operation correlates against its own frame"
        );

        // The second operation's markers never arrive: it times out instead
        // of being satisfied by the first operation's frame.
        let (begin2, end2) = markers(&second);
        let terminal =
            broker.await_framed_payload(&second, &begin2, &end2, &output, &Default::default());
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

        let terminal =
            broker.await_framed_payload(&operation, &begin, &end, &output, &Default::default());
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
        let terminal = broker.await_framed_payload(
            &cancelled,
            &cancelled_begin,
            &cancelled_end,
            &output,
            &Default::default(),
        );
        assert_eq!(terminal, BrokerTerminal::Cancelled);

        // The survivor still correlates normally after its neighbour retired.
        let (begin, end) = markers(&survivor);
        let filled = buffer_with(&[&begin, "ok", &end]);
        let terminal =
            broker.await_framed_payload(&survivor, &begin, &end, &filled, &Default::default());
        assert_eq!(
            terminal,
            BrokerTerminal::Completed(payload(&["ok".to_string()])),
            "cancellation of one operation must not touch the other pending operation"
        );
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
        let terminal = broker.await_framed_payload(
            &timed_out,
            &timed_out_begin,
            &timed_out_end,
            &empty,
            &Default::default(),
        );
        assert_eq!(terminal, BrokerTerminal::TimedOut);

        // Retiring the expired operation must leave its sibling registered,
        // so a frame arriving afterwards still resolves the survivor.
        let (survivor_begin, survivor_end) = markers(&survivor);
        let output = buffer_with(&[&survivor_begin, "ok", &survivor_end]);
        let terminal = broker.await_framed_payload(
            &survivor,
            &survivor_begin,
            &survivor_end,
            &output,
            &Default::default(),
        );
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
    fn session_end_settles_waiters_without_panic_or_deadlock() {
        let broker = Arc::new(OperationBroker::new());
        let operation = broker.submit(query_spec(&broker, Duration::from_mins(1))).expect("submit");
        let (begin, end) = markers(&operation);
        let output = Arc::new(Mutex::new(RecentOutputBuffer::new()));

        let waiter = {
            let broker = Arc::clone(&broker);
            let operation = operation.clone();
            std::thread::spawn(move || {
                broker.await_framed_payload(&operation, &begin, &end, &output, &Default::default())
            })
        };

        std::thread::sleep(Duration::from_millis(40));
        broker.settle_all("terminated");

        let terminal = waiter.join().expect("waiter thread must not panic");
        assert!(
            matches!(terminal, BrokerTerminal::SessionGone(_)),
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

        let terminal =
            broker.await_framed_payload(&operation, &begin, &end, &output, &Default::default());
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
        let terminal =
            broker.await_framed_payload(&operation, &begin, &end, &output, &Default::default());
        assert!(
            matches!(terminal, BrokerTerminal::SessionGone(_)),
            "a settled operation reports session-gone even under a held output lock: {terminal:?}"
        );
    }
}
