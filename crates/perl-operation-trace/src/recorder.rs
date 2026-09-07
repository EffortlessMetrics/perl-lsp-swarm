//! In-memory operation recorder enforcing terminal-state, ordering, parent,
//! and budget invariants.

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;

use crate::context::OperationContext;
use crate::event::{EventFieldValue, OperationEvent, OperationEventKind, OperationOutcome};
use crate::ids::{OperationId, ParentOperationId};
use crate::kind::OperationKind;
use crate::registry::{EventRegistry, RegistryError};
use crate::schema::OperationTraceSchemaVersion;

/// The bounds an [`OperationRecorder`] enforces per operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecorderBounds {
    /// Maximum number of event entries retained per operation.
    pub max_events: usize,
    /// Maximum total payload bytes retained per operation.
    pub max_payload_bytes: usize,
}

impl RecorderBounds {
    /// Construct explicit bounds.
    #[must_use]
    pub fn new(max_events: usize, max_payload_bytes: usize) -> Self {
        Self { max_events, max_payload_bytes }
    }
}

/// Why an operation's trace was truncated.
///
/// A truncation marker is recorded explicitly rather than silently dropping
/// the offending event: a silently dropped event is indistinguishable from
/// an event that never happened, which would make the trace lie about what
/// occurred. A consumer reading a truncated operation's entries sees the
/// marker and knows the trace is incomplete, rather than trusting an
/// artificially short but ostensibly complete list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TruncationReason {
    /// `max_events` was reached.
    MaxEventsExceeded,
    /// `max_payload_bytes` was reached.
    MaxPayloadBytesExceeded,
}

/// The result of a [`OperationRecorder::record`] call that did not return an
/// error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOutcome {
    /// The event was recorded normally.
    Recorded,
    /// A bound was reached: an explicit truncation marker was recorded in
    /// place of this event, and further ordinary events for this operation
    /// will be rejected with [`RecordError::EventAfterTruncation`].
    Truncated(TruncationReason),
}

/// A rejected contract violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// The event failed [`EventRegistry`] validation.
    Registry(RegistryError),
    /// A second `Terminal` event was recorded for an operation that already
    /// has one.
    DuplicateTerminal {
        /// The operation with two terminal events.
        operation: OperationId,
    },
    /// An event was recorded after an operation's `Terminal` event.
    EventAfterTerminal {
        /// The already-terminal operation.
        operation: OperationId,
    },
    /// An event was recorded after an operation's trace was truncated.
    EventAfterTruncation {
        /// The already-truncated operation.
        operation: OperationId,
    },
    /// A `Terminal` event was recorded with no well-formed `outcome` field.
    MissingOutcome {
        /// The operation whose terminal event lacks a valid outcome.
        operation: OperationId,
    },
    /// An operation declared itself as its own parent.
    SelfParent {
        /// The self-referential operation.
        operation: OperationId,
    },
    /// An operation's declared parent chain cycles back to itself.
    ParentCycle {
        /// The operation whose parent link would close a cycle.
        operation: OperationId,
        /// The immediate parent that was rejected.
        parent: OperationId,
    },
    /// A later call for an already-known operation supplied a different
    /// parent than the one already established for it. An operation has one
    /// parent; passing `None` on a later call is fine (it leaves the
    /// established parent alone), but asserting a genuinely different one is
    /// a caller contract violation, not a value to silently drop.
    ParentConflict {
        /// The operation with two disagreeing parent assertions.
        operation: OperationId,
        /// The parent already established for this operation (`None` if it
        /// was first recorded with no parent at all).
        existing: Option<OperationId>,
        /// The different parent this call supplied.
        supplied: OperationId,
    },
    /// A later call for an already-known operation supplied a different
    /// [`OperationKind`] than the one already established for it. An
    /// operation has one kind; passing the same kind again on a repeat call
    /// is fine (that is the expected shape of a caller threading one
    /// [`OperationContext`] through repeated calls), but a genuinely
    /// different kind is a caller contract violation.
    KindConflict {
        /// The operation with two disagreeing kind assertions.
        operation: OperationId,
        /// The kind already established for this operation.
        existing: OperationKind,
        /// The different kind this call supplied.
        supplied: OperationKind,
    },
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registry(e) => write!(f, "registry rejected event: {e}"),
            Self::DuplicateTerminal { operation } => {
                write!(f, "operation {operation} already has a terminal event")
            }
            Self::EventAfterTerminal { operation } => {
                write!(f, "operation {operation} already reached a terminal state")
            }
            Self::EventAfterTruncation { operation } => {
                write!(f, "operation {operation}'s trace was already truncated")
            }
            Self::MissingOutcome { operation } => {
                write!(f, "operation {operation}'s terminal event has no well-formed outcome field")
            }
            Self::SelfParent { operation } => {
                write!(f, "operation {operation} cannot be its own parent")
            }
            Self::ParentCycle { operation, parent } => write!(
                f,
                "operation {operation}'s parent chain through {parent} cycles back to itself"
            ),
            Self::ParentConflict { operation, existing, supplied } => match existing {
                Some(existing) => write!(
                    f,
                    "operation {operation} already has parent {existing}, but this call supplied {supplied}"
                ),
                None => write!(
                    f,
                    "operation {operation} was first recorded with no parent, but this call supplied {supplied}"
                ),
            },
            Self::KindConflict { operation, existing, supplied } => write!(
                f,
                "operation {operation} already has kind {existing}, but this call supplied {supplied}"
            ),
        }
    }
}

impl std::error::Error for RecordError {}

/// Whether an operation has reached a proven terminal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalState {
    /// No `Terminal` event has been recorded for this operation (or the
    /// operation is unknown to this recorder). This is the honest state for
    /// an operation that may still be running, may have crashed without
    /// closing out, or was never recorded at all — it is not treated as
    /// success, and querying it never panics.
    NotProven,
    /// A `Terminal` event was recorded with this outcome.
    Proven(OperationOutcome),
}

#[derive(Debug, Clone)]
enum RecordedEntry {
    Event(OperationEvent),
    Truncated(TruncationReason),
}

#[derive(Debug, Clone)]
struct OperationRecord {
    kind: OperationKind,
    parent: Option<OperationId>,
    entries: Vec<RecordedEntry>,
    terminal_outcome: Option<OperationOutcome>,
    truncated: Option<TruncationReason>,
    payload_bytes: usize,
}

impl OperationRecord {
    /// A new, empty record for an operation first seen with `kind`.
    ///
    /// There is no `Default` impl for this type: `kind` has no default
    /// value to fall back to (that would reintroduce exactly the kind of
    /// silent-default behavior [`OperationKind`]'s closed, non-`#[non_exhaustive]`
    /// vocabulary exists to avoid), so a record can only be constructed with
    /// a kind already in hand.
    fn new(kind: OperationKind) -> Self {
        Self {
            kind,
            parent: None,
            entries: Vec::new(),
            terminal_outcome: None,
            truncated: None,
            payload_bytes: 0,
        }
    }
}

/// In-memory recorder for `operation_trace.v1` events.
///
/// Enforces, per operation:
///
/// - exactly one `Terminal` event, ever;
/// - no event after `Terminal` (or after truncation);
/// - no self-parent and no parent cycle;
/// - one [`OperationKind`] and one parent per operation — a later
///   [`OperationContext`] disagreeing with either is rejected, not silently
///   overwritten;
/// - [`RecorderBounds::max_events`] and [`RecorderBounds::max_payload_bytes`],
///   truncating explicitly rather than dropping silently.
///
/// A disabled recorder ([`OperationRecorder::disabled`]) accepts every call
/// and stores nothing: every query reports [`TerminalState::NotProven`] and
/// every [`Self::record`] call returns `Ok`, so callers do not need an
/// `if enabled` branch around their recording call sites.
#[derive(Debug, Clone)]
pub struct OperationRecorder {
    enabled: bool,
    bounds: RecorderBounds,
    operations: BTreeMap<OperationId, OperationRecord>,
}

impl OperationRecorder {
    /// Construct an enabled recorder with the given bounds.
    #[must_use]
    pub fn new(bounds: RecorderBounds) -> Self {
        Self { enabled: true, bounds, operations: BTreeMap::new() }
    }

    /// Construct a recorder that accepts every call but records nothing.
    ///
    /// Intentionally skips all validation (registry, terminal, parent,
    /// bounds) rather than performing it against a throwaway state: the
    /// point of disabling is opting out of recording overhead entirely, not
    /// partial validation with no storage.
    #[must_use]
    pub fn disabled() -> Self {
        Self { enabled: false, bounds: RecorderBounds::new(0, 0), operations: BTreeMap::new() }
    }

    /// Whether this recorder actually stores events.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Record one event for the operation described by `context`.
    ///
    /// `context`'s parent, if any, is only *established* the first time its
    /// operation is seen by this recorder — an operation's parent is fixed
    /// at first mention. Later calls may supply a context with no parent
    /// (the established parent is unaffected) or the *same* parent (accepted
    /// as a no-op, since re-supplying identical context is expected of a
    /// caller that threads one [`OperationContext`] through repeated calls);
    /// supplying a *different* parent than the one already established is a
    /// contract violation ([`RecordError::ParentConflict`]) — an operation
    /// has one parent, and silently accepting a second, different one would
    /// hide a caller bug rather than surface it. The same rule applies to
    /// `context`'s [`OperationKind`]: a repeat call with the same kind is a
    /// no-op, a repeat call with a different kind is
    /// [`RecordError::KindConflict`].
    ///
    /// A caller can still hand-build a self-referential or cyclic context
    /// (see [`OperationContext`]'s docs); this method still checks for both
    /// at record time regardless of how the context was constructed.
    ///
    /// A well-formed `Terminal` event that arrives once
    /// [`RecorderBounds::max_events`] has already been reached is truncated
    /// like any other event, **not** given special priority: its outcome is
    /// discarded along with the event, [`Self::terminal_state`] reports
    /// [`TerminalState::NotProven`] for that operation forever afterward, and
    /// the caller learns this from the returned
    /// [`RecordOutcome::Truncated`] (not a silent `Ok(Recorded)`). This is a
    /// deliberate reading of the bound: `max_events` bounds *retained event
    /// count*, full stop, with no carved-out exception for any one kind —
    /// exceptions would make the bound's actual behavior depend on arrival
    /// order in a way this contract does not promise. A caller that must
    /// guarantee its terminal event is never lost should size
    /// `max_events` accordingly.
    ///
    /// # Errors
    ///
    /// See [`RecordError`]'s variants.
    pub fn record(
        &mut self,
        context: &OperationContext,
        event: OperationEvent,
    ) -> Result<RecordOutcome, RecordError> {
        if !self.enabled {
            return Ok(RecordOutcome::Recorded);
        }

        let operation = context.operation().clone();
        let parent = context.parent().cloned();
        let kind = context.kind();

        let first_time = !self.operations.contains_key(&operation);
        match (first_time, &parent) {
            (true, Some(parent)) => {
                let parent_id = parent.operation_id();
                if *parent_id == operation {
                    return Err(RecordError::SelfParent { operation });
                }
                if self.chain_reaches(parent_id, &operation) {
                    return Err(RecordError::ParentCycle { operation, parent: parent_id.clone() });
                }
            }
            (false, Some(parent)) => {
                let supplied = parent.operation_id().clone();
                let existing = self.operations.get(&operation).and_then(|r| r.parent.clone());
                if existing.as_ref() != Some(&supplied) {
                    return Err(RecordError::ParentConflict { operation, existing, supplied });
                }
            }
            (_, None) => {}
        }

        if !first_time {
            let existing_kind = self.operations.get(&operation).map(|r| r.kind);
            if let Some(existing_kind) = existing_kind
                && existing_kind != kind
            {
                return Err(RecordError::KindConflict {
                    operation,
                    existing: existing_kind,
                    supplied: kind,
                });
            }
        }

        let record =
            self.operations.entry(operation.clone()).or_insert_with(|| OperationRecord::new(kind));
        if first_time {
            record.parent = parent.map(ParentOperationId::into_operation_id);
        }

        if record.terminal_outcome.is_some() {
            return Err(if event.kind() == OperationEventKind::Terminal {
                RecordError::DuplicateTerminal { operation }
            } else {
                RecordError::EventAfterTerminal { operation }
            });
        }
        if record.truncated.is_some() {
            return Err(RecordError::EventAfterTruncation { operation });
        }

        EventRegistry::validate(&event).map_err(RecordError::Registry)?;

        let outcome = if event.kind() == OperationEventKind::Terminal {
            let outcome = event
                .outcome()
                .ok_or_else(|| RecordError::MissingOutcome { operation: operation.clone() })?;
            Some(outcome)
        } else {
            None
        };

        if record.entries.len() >= self.bounds.max_events {
            record.truncated = Some(TruncationReason::MaxEventsExceeded);
            record.entries.push(RecordedEntry::Truncated(TruncationReason::MaxEventsExceeded));
            return Ok(RecordOutcome::Truncated(TruncationReason::MaxEventsExceeded));
        }

        let payload_len = event_payload_len(&event);
        if record.payload_bytes.saturating_add(payload_len) > self.bounds.max_payload_bytes {
            record.truncated = Some(TruncationReason::MaxPayloadBytesExceeded);
            record
                .entries
                .push(RecordedEntry::Truncated(TruncationReason::MaxPayloadBytesExceeded));
            return Ok(RecordOutcome::Truncated(TruncationReason::MaxPayloadBytesExceeded));
        }

        record.payload_bytes += payload_len;
        if let Some(outcome) = outcome {
            record.terminal_outcome = Some(outcome);
        }
        record.entries.push(RecordedEntry::Event(event));
        Ok(RecordOutcome::Recorded)
    }

    /// Whether `operation` has reached a proven terminal state.
    #[must_use]
    pub fn terminal_state(&self, operation: &OperationId) -> TerminalState {
        match self.operations.get(operation).and_then(|r| r.terminal_outcome) {
            Some(outcome) => TerminalState::Proven(outcome),
            None => TerminalState::NotProven,
        }
    }

    /// Whether `operation`'s trace was truncated, and why.
    #[must_use]
    pub fn truncation(&self, operation: &OperationId) -> Option<TruncationReason> {
        self.operations.get(operation).and_then(|r| r.truncated)
    }

    /// The [`OperationKind`] established for `operation`, if this recorder
    /// has seen it at all.
    #[must_use]
    pub fn operation_kind(&self, operation: &OperationId) -> Option<OperationKind> {
        self.operations.get(operation).map(|r| r.kind)
    }

    /// The number of distinct operations this recorder has *touched*.
    ///
    /// This counts every operation id ever passed to [`Self::record`] that
    /// reached the point of being looked up in this recorder's internal
    /// table — including one whose very first `record` call was ultimately
    /// rejected (for example by [`EventRegistry`] validation) and therefore
    /// has zero retained events. It is a "have we seen this id at all"
    /// count, not a "how many operations have at least one recorded event"
    /// count.
    #[must_use]
    pub fn operation_count(&self) -> usize {
        self.operations.len()
    }

    /// Build a serializable, deterministic snapshot of every operation this
    /// recorder has seen, in ascending `(session, sequence)` order.
    #[must_use]
    pub fn snapshot(&self) -> OperationTraceSnapshot {
        OperationTraceSnapshot {
            schema_version: OperationTraceSchemaVersion::V1,
            operations: self
                .operations
                .iter()
                .map(|(operation, record)| RecordedOperation {
                    operation: operation.clone(),
                    parent: record.parent.clone(),
                    kind: record.kind,
                    events: record
                        .entries
                        .iter()
                        .map(|entry| match entry {
                            RecordedEntry::Event(event) => RecordedEventEntry::Event {
                                kind: event.kind(),
                                fields: event.fields().to_vec(),
                            },
                            RecordedEntry::Truncated(reason) => {
                                RecordedEventEntry::Truncated { reason: *reason }
                            }
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Whether following stored parent links from `start` ever reaches
    /// `target`.
    ///
    /// Used to reject a parent cycle before it can be stored. The stored
    /// graph is acyclic by induction as long as every insertion is checked
    /// this way, so this walk always terminates.
    fn chain_reaches(&self, start: &OperationId, target: &OperationId) -> bool {
        let mut current = start;
        loop {
            if current == target {
                return true;
            }
            match self.operations.get(current).and_then(|r| r.parent.as_ref()) {
                Some(next) => current = next,
                None => return false,
            }
        }
    }
}

fn event_payload_len(event: &OperationEvent) -> usize {
    event.fields().iter().map(|(name, value)| name.len() + value.approx_payload_len()).sum()
}

/// A serializable snapshot of every operation an [`OperationRecorder`] has
/// seen.
#[derive(Debug, Clone, Serialize)]
pub struct OperationTraceSnapshot {
    /// The schema version this snapshot conforms to.
    pub schema_version: OperationTraceSchemaVersion,
    /// Every recorded operation, in ascending `(session, sequence)` order.
    pub operations: Vec<RecordedOperation>,
}

/// One recorded operation in an [`OperationTraceSnapshot`].
#[derive(Debug, Clone, Serialize)]
pub struct RecordedOperation {
    /// The operation's own ephemeral id.
    pub operation: OperationId,
    /// The operation's declared parent, if any.
    pub parent: Option<OperationId>,
    /// The operation's established [`OperationKind`].
    pub kind: OperationKind,
    /// The operation's recorded event entries, in recording order.
    ///
    /// This is the "domain payload": `tests/negative_control.rs` proves that
    /// `operation`, `parent`, and `kind` never influence its serialized form.
    pub events: Vec<RecordedEventEntry>,
}

/// One recorded entry: an actual event, or a truncation marker.
#[derive(Debug, Clone, Serialize)]
pub enum RecordedEventEntry {
    /// An ordinarily recorded event.
    Event {
        /// The event's kind.
        kind: OperationEventKind,
        /// The event's fields, in insertion order.
        fields: Vec<(String, EventFieldValue)>,
    },
    /// An explicit truncation marker recorded in place of a dropped event.
    Truncated {
        /// Why the trace was truncated at this point.
        reason: TruncationReason,
    },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::event::OUTCOME_FIELD;
    use crate::privacy::PrivateValue;
    use crate::session::SessionId;

    fn op(session: &str, sequence: u64) -> OperationId {
        OperationId::new(SessionId::new(session).unwrap(), sequence)
    }

    /// A root context under the given session/sequence, defaulting to
    /// `TestRun` when the exact kind does not matter to the test.
    fn ctx(session: &str, sequence: u64) -> OperationContext {
        OperationContext::root(op(session, sequence), OperationKind::TestRun)
    }

    fn ctx_with_kind(session: &str, sequence: u64, kind: OperationKind) -> OperationContext {
        OperationContext::root(op(session, sequence), kind)
    }

    fn admitted() -> OperationEvent {
        OperationEvent::new(OperationEventKind::Admitted)
    }

    // ── Terminal invariants ───────────────────────────────────────────────

    #[test]
    fn duplicate_terminal_is_rejected() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let context = ctx("s1", 0);
        recorder.record(&context, OperationEvent::terminal(OperationOutcome::Completed)).unwrap();
        let err = recorder
            .record(&context, OperationEvent::terminal(OperationOutcome::Failed))
            .unwrap_err();
        assert_eq!(err, RecordError::DuplicateTerminal { operation: context.operation().clone() });
    }

    #[test]
    fn event_after_terminal_is_rejected() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let context = ctx("s1", 0);
        recorder.record(&context, OperationEvent::terminal(OperationOutcome::Completed)).unwrap();
        let err = recorder.record(&context, admitted()).unwrap_err();
        assert_eq!(err, RecordError::EventAfterTerminal { operation: context.operation().clone() });
    }

    #[test]
    fn missing_terminal_reports_not_proven_not_success_and_does_not_panic() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let context = ctx("s1", 0);
        recorder.record(&context, admitted()).unwrap();
        assert_eq!(recorder.terminal_state(context.operation()), TerminalState::NotProven);
    }

    #[test]
    fn unknown_operation_reports_not_proven() {
        let recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        assert_eq!(recorder.terminal_state(&op("s1", 999)), TerminalState::NotProven);
    }

    /// A `Terminal` event with no `outcome` field at all fails
    /// [`crate::EventRegistry`] validation first (`outcome` is a required
    /// field), never reaching the recorder's own outcome extraction — proven
    /// separately below.
    #[test]
    fn terminal_event_with_no_outcome_field_is_rejected_by_the_registry() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let context = ctx("s1", 0);
        let malformed = OperationEvent::new(OperationEventKind::Terminal);
        let err = recorder.record(&context, malformed).unwrap_err();
        assert!(matches!(err, RecordError::Registry(_)), "got {err:?}");
    }

    /// A `Terminal` event that *does* carry an `outcome` field (so it
    /// clears registry validation) but whose value is not one of
    /// [`OperationOutcome`]'s wire strings is the recorder's own
    /// [`RecordError::MissingOutcome`] — the case registry validation cannot
    /// catch, since it only checks field presence and privacy, not value
    /// well-formedness.
    #[test]
    fn terminal_event_with_malformed_outcome_value_is_rejected() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let context = ctx("s1", 0);
        let malformed = OperationEvent::new(OperationEventKind::Terminal)
            .with_field(OUTCOME_FIELD, EventFieldValue::PublicString("not-a-real-outcome".into()));
        let err = recorder.record(&context, malformed).unwrap_err();
        assert_eq!(err, RecordError::MissingOutcome { operation: context.operation().clone() });
    }

    #[test]
    fn well_formed_terminal_is_proven() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let context = ctx("s1", 0);
        recorder.record(&context, OperationEvent::terminal(OperationOutcome::Failed)).unwrap();
        assert_eq!(
            recorder.terminal_state(context.operation()),
            TerminalState::Proven(OperationOutcome::Failed)
        );
    }

    /// A `Terminal` event is not given special priority against
    /// `max_events`: if it arrives once the cap is already reached, it is
    /// truncated exactly like any other event, its outcome is discarded, and
    /// the operation reports `NotProven` forever afterward. This is
    /// deliberate (see `record`'s doc comment) and must stay a designed,
    /// tested behavior rather than an accidental one.
    #[test]
    fn terminal_event_at_the_max_events_cap_is_truncated_and_reports_not_proven() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(1, 1_000_000));
        let context = ctx("s1", 0);
        recorder.record(&context, admitted()).unwrap();

        let outcome = recorder
            .record(&context, OperationEvent::terminal(OperationOutcome::Completed))
            .unwrap();
        assert_eq!(outcome, RecordOutcome::Truncated(TruncationReason::MaxEventsExceeded));
        assert_eq!(
            recorder.terminal_state(context.operation()),
            TerminalState::NotProven,
            "a terminal event lost to truncation must not be reported as proven"
        );
    }

    // ── Parent invariants ─────────────────────────────────────────────────

    #[test]
    fn self_parent_is_rejected() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let root = ctx("s1", 0);
        // Hand-build a self-referential context: `child` on itself, reusing
        // its own operation id for the "child". `OperationContext` cannot
        // prevent this by construction (see its docs); the recorder still
        // must catch it.
        let bad = root.child(root.operation().clone(), OperationKind::TestRun);
        let err = recorder.record(&bad, admitted()).unwrap_err();
        assert_eq!(err, RecordError::SelfParent { operation: root.operation().clone() });
    }

    /// A -> (declared later as) C's parent, B -> A, then C -> B closes the
    /// loop A <- B <- C <- A. Forward references are allowed (an operation
    /// may name a parent this recorder has not seen yet), so the cycle can
    /// only be detected once the final edge is added.
    #[test]
    fn parent_cycle_is_rejected() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let a = op("s1", 0);
        let b = op("s1", 1);
        let c = op("s1", 2);

        // A throwaway context standing in for "C", used only to derive A's
        // parent link — never itself recorded.
        let c_stub = OperationContext::root(c.clone(), OperationKind::TestRun);
        let a_context = c_stub.child(a.clone(), OperationKind::TestRun);
        recorder.record(&a_context, admitted()).unwrap();

        let a_recorded = OperationContext::root(a.clone(), OperationKind::TestRun);
        let b_context = a_recorded.child(b.clone(), OperationKind::TestRun);
        recorder.record(&b_context, admitted()).unwrap();

        let b_recorded = OperationContext::root(b.clone(), OperationKind::TestRun);
        let c_context = b_recorded.child(c.clone(), OperationKind::TestRun);
        let err = recorder.record(&c_context, admitted()).unwrap_err();
        assert_eq!(err, RecordError::ParentCycle { operation: c, parent: b });
    }

    #[test]
    fn a_valid_parent_chain_is_accepted() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let a = ctx("s1", 0);
        recorder.record(&a, admitted()).unwrap();
        let b = a.child(op("s1", 1), OperationKind::TestRun);
        recorder.record(&b, admitted()).unwrap();
        let c = b.child(op("s1", 2), OperationKind::TestRun);
        assert!(recorder.record(&c, admitted()).is_ok());
    }

    #[test]
    fn repeating_the_same_context_on_a_later_call_is_accepted() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let a = ctx("s1", 0);
        recorder.record(&a, admitted()).unwrap();
        let b = a.child(op("s1", 1), OperationKind::TestRun);
        recorder.record(&b, admitted()).unwrap();
        // Same context supplied again on a second call for `b` — legal.
        assert!(recorder.record(&b, admitted()).is_ok());
    }

    #[test]
    fn a_different_parent_on_a_later_call_is_rejected() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let a = ctx("s1", 0);
        let other = ctx("s1", 2);
        recorder.record(&a, admitted()).unwrap();
        recorder.record(&other, admitted()).unwrap();

        let b_via_a = a.child(op("s1", 1), OperationKind::TestRun);
        recorder.record(&b_via_a, admitted()).unwrap();

        let b_via_other = other.child(op("s1", 1), OperationKind::TestRun);
        let err = recorder.record(&b_via_other, admitted()).unwrap_err();
        assert_eq!(
            err,
            RecordError::ParentConflict {
                operation: op("s1", 1),
                existing: Some(a.operation().clone()),
                supplied: other.operation().clone(),
            }
        );
    }

    #[test]
    fn asserting_a_parent_after_first_recording_with_none_is_rejected() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let a = ctx("s1", 0);
        let b_root = ctx("s1", 1);
        recorder.record(&a, admitted()).unwrap();
        recorder.record(&b_root, admitted()).unwrap();

        let b_via_a = a.child(op("s1", 1), OperationKind::TestRun);
        let err = recorder.record(&b_via_a, admitted()).unwrap_err();
        assert_eq!(
            err,
            RecordError::ParentConflict {
                operation: op("s1", 1),
                existing: None,
                supplied: a.operation().clone(),
            }
        );
    }

    // ── Kind invariants ───────────────────────────────────────────────────

    #[test]
    fn repeating_the_same_kind_on_a_later_call_is_accepted() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let context = ctx_with_kind("s1", 0, OperationKind::TestRun);
        recorder.record(&context, admitted()).unwrap();
        assert!(recorder.record(&context, admitted()).is_ok());
        assert_eq!(recorder.operation_kind(context.operation()), Some(OperationKind::TestRun));
    }

    #[test]
    fn a_different_kind_on_a_later_call_is_rejected() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let operation = op("s1", 0);
        let first = OperationContext::root(operation.clone(), OperationKind::TestRun);
        let second = OperationContext::root(operation.clone(), OperationKind::LspRequest);
        recorder.record(&first, admitted()).unwrap();
        let err = recorder.record(&second, admitted()).unwrap_err();
        assert_eq!(
            err,
            RecordError::KindConflict {
                operation,
                existing: OperationKind::TestRun,
                supplied: OperationKind::LspRequest,
            }
        );
    }

    // ── Bounds ────────────────────────────────────────────────────────────

    #[test]
    fn max_events_cap_is_enforced_with_an_explicit_marker() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(2, 1_000_000));
        let context = ctx("s1", 0);
        assert_eq!(recorder.record(&context, admitted()).unwrap(), RecordOutcome::Recorded);
        assert_eq!(recorder.record(&context, admitted()).unwrap(), RecordOutcome::Recorded);

        let outcome = recorder.record(&context, admitted()).unwrap();
        assert_eq!(outcome, RecordOutcome::Truncated(TruncationReason::MaxEventsExceeded));
        assert_eq!(
            recorder.truncation(context.operation()),
            Some(TruncationReason::MaxEventsExceeded)
        );

        let snapshot = recorder.snapshot();
        let recorded = &snapshot.operations[0];
        assert!(
            recorded.events.iter().any(|e| matches!(
                e,
                RecordedEventEntry::Truncated { reason: TruncationReason::MaxEventsExceeded }
            )),
            "truncation marker must be observable in the trace itself"
        );
    }

    #[test]
    fn events_after_truncation_are_rejected() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(1, 1_000_000));
        let context = ctx("s1", 0);
        recorder.record(&context, admitted()).unwrap();
        // Second call trips the bound and truncates.
        recorder.record(&context, admitted()).unwrap();
        let err = recorder.record(&context, admitted()).unwrap_err();
        assert_eq!(
            err,
            RecordError::EventAfterTruncation { operation: context.operation().clone() }
        );
    }

    #[test]
    fn max_payload_bytes_cap_is_enforced_with_an_explicit_marker() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(1_000, 4));
        let context = ctx("s1", 0);
        let big_event = OperationEvent::new(OperationEventKind::Rejected)
            .with_field("reason", EventFieldValue::PublicString("this reason is long".into()));
        let outcome = recorder.record(&context, big_event).unwrap();
        assert_eq!(outcome, RecordOutcome::Truncated(TruncationReason::MaxPayloadBytesExceeded));
        assert_eq!(
            recorder.truncation(context.operation()),
            Some(TruncationReason::MaxPayloadBytesExceeded)
        );
    }

    // ── Disabled mode ─────────────────────────────────────────────────────

    #[test]
    fn disabled_recorder_accepts_every_call_and_stores_nothing() {
        let mut recorder = OperationRecorder::disabled();
        assert!(!recorder.is_enabled());
        let root = ctx("s1", 0);
        let self_parented = root.child(root.operation().clone(), OperationKind::TestRun);

        // The exact call sequence that would otherwise trip duplicate
        // terminal, event-after-terminal, and self-parent all succeeds.
        assert_eq!(
            recorder.record(&root, OperationEvent::terminal(OperationOutcome::Completed)).unwrap(),
            RecordOutcome::Recorded
        );
        assert_eq!(
            recorder.record(&root, admitted()).unwrap(),
            RecordOutcome::Recorded,
            "event after terminal must not error when disabled"
        );
        assert_eq!(
            recorder.record(&self_parented, admitted()).unwrap(),
            RecordOutcome::Recorded,
            "self-parent must not error when disabled"
        );

        assert_eq!(recorder.terminal_state(root.operation()), TerminalState::NotProven);
        assert_eq!(recorder.operation_count(), 0);
    }

    #[test]
    fn enabled_recorder_with_generous_bounds_accepts_the_same_ordinary_sequence() {
        // The counterpart to the disabled test above: an *ordinary*,
        // contract-respecting call sequence succeeds identically whether or
        // not recording is enabled.
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let context = ctx("s1", 0);
        assert_eq!(recorder.record(&context, admitted()).unwrap(), RecordOutcome::Recorded);
        assert_eq!(
            recorder
                .record(&context, OperationEvent::terminal(OperationOutcome::Completed))
                .unwrap(),
            RecordOutcome::Recorded
        );
        assert_eq!(
            recorder.terminal_state(context.operation()),
            TerminalState::Proven(OperationOutcome::Completed)
        );
    }

    // ── Snapshot / negative-control support ──────────────────────────────

    #[test]
    fn snapshot_orders_operations_deterministically() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        recorder.record(&ctx("s1", 5), admitted()).unwrap();
        recorder.record(&ctx("s1", 1), admitted()).unwrap();
        let snapshot = recorder.snapshot();
        let sequences: Vec<u64> =
            snapshot.operations.iter().map(|o| o.operation.sequence()).collect();
        assert_eq!(sequences, vec![1, 5]);
    }

    #[test]
    fn snapshot_carries_the_established_kind() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let context = ctx_with_kind("s1", 0, OperationKind::DebugSession);
        recorder.record(&context, admitted()).unwrap();
        let snapshot = recorder.snapshot();
        assert_eq!(snapshot.operations[0].kind, OperationKind::DebugSession);
    }

    #[test]
    fn redaction_holds_through_the_full_snapshot_serialization() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let context = ctx("s1", 0);
        let event = OperationEvent::new(OperationEventKind::StageStarted)
            .with_field("stage", EventFieldValue::PublicString("parse".into()))
            .with_field(
                "host_path",
                EventFieldValue::Private(PrivateValue::new("/home/alice/.ssh/id_rsa")),
            );
        recorder.record(&context, event).unwrap();

        let json = serde_json::to_string(&recorder.snapshot()).unwrap();
        assert!(
            !json.contains("alice"),
            "private content must not survive full-trace serialization"
        );
        assert!(json.contains("redacted"));
    }

    // ── Serde determinism ─────────────────────────────────────────────────

    #[test]
    fn snapshot_serialization_is_byte_identical_across_repeated_serialize_calls() {
        let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
        let context = ctx("s1", 0);
        recorder.record(&context, admitted()).unwrap();
        recorder.record(&context, OperationEvent::terminal(OperationOutcome::Completed)).unwrap();

        let snapshot = recorder.snapshot();
        let first = serde_json::to_string(&snapshot).unwrap();
        let second = serde_json::to_string(&snapshot).unwrap();
        assert_eq!(first, second, "serializing the same snapshot twice must be byte-identical");
    }

    #[test]
    fn two_recorders_built_from_identical_input_serialize_identically() {
        let build = || {
            let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));
            let context = ctx("s1", 0);
            recorder.record(&context, admitted()).unwrap();
            recorder
                .record(&context, OperationEvent::terminal(OperationOutcome::Completed))
                .unwrap();
            recorder
        };
        let a = serde_json::to_string(&build().snapshot()).unwrap();
        let b = serde_json::to_string(&build().snapshot()).unwrap();
        assert_eq!(a, b, "identical input must produce byte-identical serialized output");
    }
}
