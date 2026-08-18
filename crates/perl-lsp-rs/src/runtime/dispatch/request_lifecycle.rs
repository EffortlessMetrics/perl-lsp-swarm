//! Connection-scoped ownership for client-to-server requests.
//!
//! A request enters this registry only when it has a valid JSON-RPC ID. The
//! registry owns bounded admission, execution phase, exactly-once terminal
//! selection, response-write disposition, and cleanup evidence. Cancellation
//! and supersession policy are deliberately left to #7100.

use crate::protocol::{JsonRpcError, JsonRpcId};
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

pub(crate) const DEFAULT_INCOMING_REQUEST_CAPACITY: usize = 256;
pub(crate) const MAX_INCOMING_REQUEST_CAPACITY: usize = 8_192;
const MAX_RECENT_TERMINALS: usize = 512;
const MAX_ANOMALIES: usize = 128;
const MAX_METHOD_BYTES: usize = 160;
const MAX_DIAGNOSTIC_BYTES: usize = 384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IncomingRequestPhase {
    Accepted,
    Queued,
    Running,
    TerminalSelected,
}

impl IncomingRequestPhase {
    fn rank(self) -> u8 {
        match self {
            Self::Accepted => 0,
            Self::Queued => 1,
            Self::Running => 2,
            Self::TerminalSelected => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IncomingTerminalKind {
    Result,
    Error,
    Shutdown,
    TransportLost,
}

#[derive(Debug, Clone)]
pub(crate) enum IncomingTerminalOutcome {
    Result(Value),
    Error(JsonRpcError),
}

impl IncomingTerminalOutcome {
    fn kind(&self) -> IncomingTerminalKind {
        match self {
            Self::Result(_) => IncomingTerminalKind::Result,
            Self::Error(_) => IncomingTerminalKind::Error,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SelectedIncomingTerminal {
    pub(crate) id: JsonRpcId,
    pub(crate) method: String,
    pub(crate) accepted_at: Instant,
    pub(crate) selected_at: Instant,
    pub(crate) outcome: IncomingTerminalOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IncomingRequestHandle {
    pub(crate) id: JsonRpcId,
    pub(crate) method: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IncomingRequestSnapshot {
    pub(crate) id: JsonRpcId,
    pub(crate) method: String,
    pub(crate) phase: IncomingRequestPhase,
    pub(crate) accepted_at: Instant,
    pub(crate) phase_changed_at: Instant,
    pub(crate) terminal_kind: Option<IncomingTerminalKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhaseTransitionDisposition {
    Advanced,
    Unchanged,
    AlreadyTerminal,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalSelectionDisposition {
    Selected,
    AlreadyTerminal,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseWriteDisposition {
    WrittenAndCleaned,
    WriteFailedAndCleaned,
    TerminalNotSelected,
    AlreadyCleaned,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IncomingRequestAnomalyKind {
    DuplicateAdmission,
    InvalidPhaseRegression,
    UnknownRequest,
    UnsupportedId,
    DuplicateTerminal,
    ResponseBeforeTerminal,
    ResponseAfterCleanup,
    ResponseWriteFailed,
    TransportCleanup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IncomingRequestAnomaly {
    pub(crate) kind: IncomingRequestAnomalyKind,
    pub(crate) id: Option<JsonRpcId>,
    pub(crate) method: Option<String>,
    pub(crate) detail: String,
    pub(crate) observed_at: Instant,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct IncomingRequestCounters {
    pub(crate) capacity: usize,
    pub(crate) active: usize,
    pub(crate) accepted: usize,
    pub(crate) queued: usize,
    pub(crate) running: usize,
    pub(crate) terminal_selected_active: usize,
    pub(crate) admitted_total: u64,
    pub(crate) notifications_bypassed: u64,
    pub(crate) capacity_rejected: u64,
    pub(crate) duplicate_admission: u64,
    pub(crate) phase_advanced: u64,
    pub(crate) terminal_selected_total: u64,
    pub(crate) result_selected: u64,
    pub(crate) error_selected: u64,
    pub(crate) shutdown_selected: u64,
    pub(crate) duplicate_terminal: u64,
    pub(crate) responses_written: u64,
    pub(crate) response_write_failed: u64,
    pub(crate) transport_cleaned: u64,
    pub(crate) unknown_request: u64,
    pub(crate) unsupported_id: u64,
    pub(crate) anomalies_dropped: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IncomingRequestRegistryError {
    InvalidCapacity { requested: usize, maximum: usize },
    CapacityExhausted { capacity: usize },
    DuplicateId { id: String },
    UnsupportedId { id: String },
}

impl fmt::Display for IncomingRequestRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCapacity { requested, maximum } => write!(
                formatter,
                "incoming request registry capacity {requested} is outside 1..={maximum}"
            ),
            Self::CapacityExhausted { capacity } => {
                write!(formatter, "incoming request registry capacity {capacity} is exhausted")
            }
            Self::DuplicateId { id } => {
                write!(formatter, "incoming request id {id} is already active")
            }
            Self::UnsupportedId { id } => {
                write!(formatter, "incoming request id {id} is not a supported identity")
            }
        }
    }
}

impl Error for IncomingRequestRegistryError {}

#[derive(Clone)]
pub(crate) struct IncomingRequestRegistry {
    inner: Arc<RegistryInner>,
}

struct RegistryInner {
    state: Mutex<RegistryState>,
}

struct RegistryState {
    capacity: usize,
    active: BTreeMap<IncomingRequestKey, IncomingRequestEntry>,
    recent: VecDeque<RecentTerminal>,
    anomalies: VecDeque<IncomingRequestAnomaly>,
    counters: IncomingRequestCounters,
}

struct IncomingRequestEntry {
    method: String,
    accepted_at: Instant,
    phase_changed_at: Instant,
    phase: IncomingRequestPhase,
    terminal_kind: Option<IncomingTerminalKind>,
}

#[derive(Debug, Clone)]
struct RecentTerminal {
    id: IncomingRequestKey,
    method: String,
    terminal_kind: IncomingTerminalKind,
    cleaned_at: Instant,
    response_written: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum IncomingRequestKey {
    Integer(i64),
    String(String),
}

impl IncomingRequestKey {
    /// Project a wire id into a registry identity.
    ///
    /// `JsonRpcId` is `#[non_exhaustive]`, so a future variant must not be
    /// forced into the integer or string identity domain: that would let a new
    /// id collide with a genuine numeric or string request. Such an id is
    /// instead refused, which keeps admission fail-closed and keeps
    /// [`IncomingRequestKey::to_id`] total for every identity the registry
    /// actually stores.
    ///
    /// No variant outside `Integer`/`String` exists today, so this returns
    /// `None` only if `perl-lsp-rs-core` later widens the enum. The branch is
    /// therefore unreachable from a test in this crate and is proven by
    /// construction rather than by a fixture.
    fn from_id(id: &JsonRpcId) -> Option<Self> {
        match id {
            JsonRpcId::Integer(value) => Some(Self::Integer(*value)),
            JsonRpcId::String(value) => Some(Self::String(value.clone())),
            _ => None,
        }
    }

    fn to_id(&self) -> JsonRpcId {
        match self {
            Self::Integer(value) => JsonRpcId::Integer(*value),
            Self::String(value) => JsonRpcId::String(value.clone()),
        }
    }

    fn diagnostic_identity(&self) -> String {
        match self {
            Self::Integer(value) => format!("integer:{value}"),
            Self::String(value) => {
                format!("string:{}", bounded_text(value, MAX_DIAGNOSTIC_BYTES))
            }
        }
    }
}

impl Default for IncomingRequestRegistry {
    fn default() -> Self {
        Self::with_valid_capacity(DEFAULT_INCOMING_REQUEST_CAPACITY)
    }
}

impl IncomingRequestRegistry {
    pub(crate) fn new(capacity: usize) -> Result<Self, IncomingRequestRegistryError> {
        if capacity == 0 || capacity > MAX_INCOMING_REQUEST_CAPACITY {
            return Err(IncomingRequestRegistryError::InvalidCapacity {
                requested: capacity,
                maximum: MAX_INCOMING_REQUEST_CAPACITY,
            });
        }
        Ok(Self::with_valid_capacity(capacity))
    }

    fn with_valid_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                state: Mutex::new(RegistryState {
                    capacity,
                    active: BTreeMap::new(),
                    recent: VecDeque::new(),
                    anomalies: VecDeque::new(),
                    counters: IncomingRequestCounters {
                        capacity,
                        ..IncomingRequestCounters::default()
                    },
                }),
            }),
        }
    }

    /// Admit a request ID, or bypass a notification without creating state.
    pub(crate) fn admit_optional(
        &self,
        id: Option<JsonRpcId>,
        method: impl Into<String>,
    ) -> Result<Option<IncomingRequestHandle>, IncomingRequestRegistryError> {
        let Some(id) = id else {
            let mut state = self.inner.state.lock();
            state.counters.notifications_bypassed =
                state.counters.notifications_bypassed.saturating_add(1);
            return Ok(None);
        };
        self.admit(id, method).map(Some)
    }

    pub(crate) fn admit(
        &self,
        id: JsonRpcId,
        method: impl Into<String>,
    ) -> Result<IncomingRequestHandle, IncomingRequestRegistryError> {
        let method = bounded_text(&method.into(), MAX_METHOD_BYTES);
        let now = Instant::now();
        let mut state = self.inner.state.lock();
        let Some(key) = IncomingRequestKey::from_id(&id) else {
            let rendered = record_unsupported_id(&mut state, &id, "admission", now);
            return Err(IncomingRequestRegistryError::UnsupportedId { id: rendered });
        };
        if state.active.len() >= state.capacity {
            state.counters.capacity_rejected = state.counters.capacity_rejected.saturating_add(1);
            return Err(IncomingRequestRegistryError::CapacityExhausted {
                capacity: state.capacity,
            });
        }
        if state.active.contains_key(&key) {
            state.counters.duplicate_admission =
                state.counters.duplicate_admission.saturating_add(1);
            push_anomaly(
                &mut state,
                IncomingRequestAnomalyKind::DuplicateAdmission,
                Some(key.clone()),
                Some(method.clone()),
                "request ID is already active".to_string(),
                now,
            );
            return Err(IncomingRequestRegistryError::DuplicateId {
                id: key.diagnostic_identity(),
            });
        }

        state.active.insert(
            key,
            IncomingRequestEntry {
                method: method.clone(),
                accepted_at: now,
                phase_changed_at: now,
                phase: IncomingRequestPhase::Accepted,
                terminal_kind: None,
            },
        );
        state.counters.admitted_total = state.counters.admitted_total.saturating_add(1);
        Ok(IncomingRequestHandle { id, method })
    }

    pub(crate) fn mark_queued(&self, id: &JsonRpcId) -> PhaseTransitionDisposition {
        self.advance_phase(id, IncomingRequestPhase::Queued)
    }

    pub(crate) fn mark_running(&self, id: &JsonRpcId) -> PhaseTransitionDisposition {
        self.advance_phase(id, IncomingRequestPhase::Running)
    }

    pub(crate) fn select_result(
        &self,
        id: &JsonRpcId,
        result: Value,
    ) -> (TerminalSelectionDisposition, Option<SelectedIncomingTerminal>) {
        self.select_terminal(id, IncomingTerminalOutcome::Result(result), false)
    }

    pub(crate) fn select_error(
        &self,
        id: &JsonRpcId,
        error: JsonRpcError,
    ) -> (TerminalSelectionDisposition, Option<SelectedIncomingTerminal>) {
        self.select_terminal(id, IncomingTerminalOutcome::Error(error), false)
    }

    /// Select a shutdown error for every active request that has no terminal yet.
    pub(crate) fn select_shutdown_errors(
        &self,
        error_code: i32,
        message: &str,
    ) -> Vec<SelectedIncomingTerminal> {
        let ids = {
            let state = self.inner.state.lock();
            state
                .active
                .iter()
                .filter_map(|(id, entry)| {
                    (entry.phase != IncomingRequestPhase::TerminalSelected).then_some(id.to_id())
                })
                .collect::<Vec<_>>()
        };

        let mut selected = Vec::new();
        for id in ids {
            let outcome = IncomingTerminalOutcome::Error(JsonRpcError {
                code: error_code,
                message: bounded_text(message, MAX_DIAGNOSTIC_BYTES),
                data: None,
            });
            let (disposition, terminal) = self.select_terminal(&id, outcome, true);
            if disposition == TerminalSelectionDisposition::Selected
                && let Some(terminal) = terminal
            {
                selected.push(terminal);
            }
        }
        selected
    }

    pub(crate) fn response_written(&self, id: &JsonRpcId) -> ResponseWriteDisposition {
        self.finish_response(id, true, None)
    }

    pub(crate) fn response_write_failed(
        &self,
        id: &JsonRpcId,
        detail: &str,
    ) -> ResponseWriteDisposition {
        self.finish_response(id, false, Some(detail))
    }

    /// Clean every remaining entry after the transport is gone.
    pub(crate) fn transport_lost(&self) -> usize {
        let now = Instant::now();
        let mut state = self.inner.state.lock();
        let active = std::mem::take(&mut state.active);
        let count = active.len();
        for (id, entry) in active {
            state.counters.transport_cleaned = state.counters.transport_cleaned.saturating_add(1);
            push_recent(
                &mut state,
                RecentTerminal {
                    id: id.clone(),
                    method: entry.method.clone(),
                    terminal_kind: entry
                        .terminal_kind
                        .unwrap_or(IncomingTerminalKind::TransportLost),
                    cleaned_at: now,
                    response_written: false,
                },
            );
            push_anomaly(
                &mut state,
                IncomingRequestAnomalyKind::TransportCleanup,
                Some(id),
                Some(entry.method),
                format!("transport lost while phase={:?}", entry.phase),
                now,
            );
        }
        count
    }

    pub(crate) fn active_count(&self) -> usize {
        self.inner.state.lock().active.len()
    }

    pub(crate) fn snapshots(&self) -> Vec<IncomingRequestSnapshot> {
        self.inner
            .state
            .lock()
            .active
            .iter()
            .map(|(id, entry)| IncomingRequestSnapshot {
                id: id.to_id(),
                method: entry.method.clone(),
                phase: entry.phase,
                accepted_at: entry.accepted_at,
                phase_changed_at: entry.phase_changed_at,
                terminal_kind: entry.terminal_kind,
            })
            .collect()
    }

    pub(crate) fn counters(&self) -> IncomingRequestCounters {
        let state = self.inner.state.lock();
        let mut counters = state.counters;
        counters.active = state.active.len();
        counters.accepted = 0;
        counters.queued = 0;
        counters.running = 0;
        counters.terminal_selected_active = 0;
        for entry in state.active.values() {
            match entry.phase {
                IncomingRequestPhase::Accepted => counters.accepted += 1,
                IncomingRequestPhase::Queued => counters.queued += 1,
                IncomingRequestPhase::Running => counters.running += 1,
                IncomingRequestPhase::TerminalSelected => counters.terminal_selected_active += 1,
            }
        }
        counters
    }

    pub(crate) fn anomalies(&self) -> Vec<IncomingRequestAnomaly> {
        self.inner.state.lock().anomalies.iter().cloned().collect()
    }

    fn advance_phase(
        &self,
        id: &JsonRpcId,
        target: IncomingRequestPhase,
    ) -> PhaseTransitionDisposition {
        let now = Instant::now();
        let mut state = self.inner.state.lock();
        let Some(key) = IncomingRequestKey::from_id(id) else {
            let _ = record_unsupported_id(&mut state, id, "phase transition", now);
            return PhaseTransitionDisposition::Unknown;
        };
        let Some(mut entry) = state.active.remove(&key) else {
            record_unknown(&mut state, &key, "phase transition", now);
            return PhaseTransitionDisposition::Unknown;
        };

        let disposition = if entry.phase == IncomingRequestPhase::TerminalSelected {
            PhaseTransitionDisposition::AlreadyTerminal
        } else if target.rank() < entry.phase.rank() {
            push_anomaly(
                &mut state,
                IncomingRequestAnomalyKind::InvalidPhaseRegression,
                Some(key.clone()),
                Some(entry.method.clone()),
                format!("phase regression {:?} -> {target:?}", entry.phase),
                now,
            );
            PhaseTransitionDisposition::Unchanged
        } else if target == entry.phase {
            PhaseTransitionDisposition::Unchanged
        } else {
            entry.phase = target;
            entry.phase_changed_at = now;
            state.counters.phase_advanced = state.counters.phase_advanced.saturating_add(1);
            PhaseTransitionDisposition::Advanced
        };

        state.active.insert(key, entry);
        disposition
    }

    fn select_terminal(
        &self,
        id: &JsonRpcId,
        outcome: IncomingTerminalOutcome,
        shutdown: bool,
    ) -> (TerminalSelectionDisposition, Option<SelectedIncomingTerminal>) {
        let now = Instant::now();
        let mut state = self.inner.state.lock();
        let Some(key) = IncomingRequestKey::from_id(id) else {
            let _ = record_unsupported_id(&mut state, id, "terminal selection", now);
            return (TerminalSelectionDisposition::Unknown, None);
        };
        let Some(mut entry) = state.active.remove(&key) else {
            let recent = state.recent.iter().find(|recent| recent.id == key).map(|recent| {
                (
                    recent.method.clone(),
                    recent.terminal_kind,
                    recent.response_written,
                    recent.cleaned_at,
                )
            });
            if let Some((method, terminal_kind, response_written, cleaned_at)) = recent {
                state.counters.duplicate_terminal =
                    state.counters.duplicate_terminal.saturating_add(1);
                push_anomaly(
                    &mut state,
                    IncomingRequestAnomalyKind::DuplicateTerminal,
                    Some(key),
                    Some(method),
                    format!(
                        "terminal already cleaned kind={terminal_kind:?} response_written={response_written} cleaned_at={cleaned_at:?}"
                    ),
                    now,
                );
                return (TerminalSelectionDisposition::AlreadyTerminal, None);
            }
            record_unknown(&mut state, &key, "terminal selection", now);
            return (TerminalSelectionDisposition::Unknown, None);
        };

        if entry.phase == IncomingRequestPhase::TerminalSelected {
            state.counters.duplicate_terminal = state.counters.duplicate_terminal.saturating_add(1);
            push_anomaly(
                &mut state,
                IncomingRequestAnomalyKind::DuplicateTerminal,
                Some(key.clone()),
                Some(entry.method.clone()),
                "second terminal selection rejected".to_string(),
                now,
            );
            state.active.insert(key, entry);
            return (TerminalSelectionDisposition::AlreadyTerminal, None);
        }

        let terminal_kind = if shutdown { IncomingTerminalKind::Shutdown } else { outcome.kind() };
        entry.phase = IncomingRequestPhase::TerminalSelected;
        entry.phase_changed_at = now;
        entry.terminal_kind = Some(terminal_kind);
        let selected = SelectedIncomingTerminal {
            id: id.clone(),
            method: entry.method.clone(),
            accepted_at: entry.accepted_at,
            selected_at: now,
            outcome,
        };
        state.active.insert(key, entry);
        state.counters.terminal_selected_total =
            state.counters.terminal_selected_total.saturating_add(1);
        // `select_terminal` assigns Result, Error, or Shutdown only.
        // `TransportLost` is recorded by `transport_lost()`, not here.
        if shutdown {
            state.counters.shutdown_selected = state.counters.shutdown_selected.saturating_add(1);
        } else if matches!(selected.outcome, IncomingTerminalOutcome::Result(_)) {
            state.counters.result_selected = state.counters.result_selected.saturating_add(1);
        } else {
            state.counters.error_selected = state.counters.error_selected.saturating_add(1);
        }
        (TerminalSelectionDisposition::Selected, Some(selected))
    }

    fn finish_response(
        &self,
        id: &JsonRpcId,
        written: bool,
        failure_detail: Option<&str>,
    ) -> ResponseWriteDisposition {
        let now = Instant::now();
        let mut state = self.inner.state.lock();
        let Some(key) = IncomingRequestKey::from_id(id) else {
            let _ = record_unsupported_id(&mut state, id, "response completion", now);
            return ResponseWriteDisposition::Unknown;
        };
        let Some(entry) = state.active.remove(&key) else {
            let recent_method = state
                .recent
                .iter()
                .find(|recent| recent.id == key)
                .map(|recent| recent.method.clone());
            if let Some(method) = recent_method {
                push_anomaly(
                    &mut state,
                    IncomingRequestAnomalyKind::ResponseAfterCleanup,
                    Some(key),
                    Some(method),
                    "response completion repeated after cleanup".to_string(),
                    now,
                );
                return ResponseWriteDisposition::AlreadyCleaned;
            }
            record_unknown(&mut state, &key, "response completion", now);
            return ResponseWriteDisposition::Unknown;
        };

        let Some(terminal_kind) = entry.terminal_kind else {
            let method = entry.method.clone();
            state.active.insert(key.clone(), entry);
            push_anomaly(
                &mut state,
                IncomingRequestAnomalyKind::ResponseBeforeTerminal,
                Some(key),
                Some(method),
                "response write attempted before terminal selection".to_string(),
                now,
            );
            return ResponseWriteDisposition::TerminalNotSelected;
        };

        push_recent(
            &mut state,
            RecentTerminal {
                id: key.clone(),
                method: entry.method.clone(),
                terminal_kind,
                cleaned_at: now,
                response_written: written,
            },
        );

        if written {
            state.counters.responses_written = state.counters.responses_written.saturating_add(1);
            ResponseWriteDisposition::WrittenAndCleaned
        } else {
            state.counters.response_write_failed =
                state.counters.response_write_failed.saturating_add(1);
            push_anomaly(
                &mut state,
                IncomingRequestAnomalyKind::ResponseWriteFailed,
                Some(key),
                Some(entry.method),
                bounded_text(
                    failure_detail.unwrap_or("response write failed"),
                    MAX_DIAGNOSTIC_BYTES,
                ),
                now,
            );
            ResponseWriteDisposition::WriteFailedAndCleaned
        }
    }
}

/// Record an id the registry cannot identify, and return its bounded rendering.
///
/// The anomaly carries no `JsonRpcId` because the registry refuses to claim an
/// identity it cannot round-trip; the detail string carries the debug shape so
/// the operator still sees what arrived.
fn record_unsupported_id(
    state: &mut RegistryState,
    id: &JsonRpcId,
    operation: &str,
    observed_at: Instant,
) -> String {
    let rendered = bounded_text(&format!("{id:?}"), MAX_DIAGNOSTIC_BYTES);
    state.counters.unsupported_id = state.counters.unsupported_id.saturating_add(1);
    push_anomaly(
        state,
        IncomingRequestAnomalyKind::UnsupportedId,
        None,
        None,
        format!("{operation} referenced an unsupported request identity {rendered}"),
        observed_at,
    );
    rendered
}

fn record_unknown(
    state: &mut RegistryState,
    id: &IncomingRequestKey,
    operation: &str,
    observed_at: Instant,
) {
    state.counters.unknown_request = state.counters.unknown_request.saturating_add(1);
    push_anomaly(
        state,
        IncomingRequestAnomalyKind::UnknownRequest,
        Some(id.clone()),
        None,
        format!("{operation} referenced an unknown request"),
        observed_at,
    );
}

fn push_recent(state: &mut RegistryState, terminal: RecentTerminal) {
    state.recent.push_back(terminal);
    while state.recent.len() > MAX_RECENT_TERMINALS {
        let _ = state.recent.pop_front();
    }
}

fn push_anomaly(
    state: &mut RegistryState,
    kind: IncomingRequestAnomalyKind,
    id: Option<IncomingRequestKey>,
    method: Option<String>,
    detail: String,
    observed_at: Instant,
) {
    if state.anomalies.len() == MAX_ANOMALIES {
        let _ = state.anomalies.pop_front();
        state.counters.anomalies_dropped = state.counters.anomalies_dropped.saturating_add(1);
    }
    state.anomalies.push_back(IncomingRequestAnomaly {
        kind,
        id: id.map(|key| key.to_id()),
        method: method.map(|method| bounded_text(&method, MAX_METHOD_BYTES)),
        detail: bounded_text(&detail, MAX_DIAGNOSTIC_BYTES),
        observed_at,
    });
}

fn bounded_text(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_string();
    }
    let suffix = "...";
    let mut end = maximum_bytes.saturating_sub(suffix.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut bounded = value[..end].to_string();
    bounded.push_str(suffix);
    bounded
}

#[cfg(test)]
#[path = "tests/ripr_seam_proof_incoming_request_owner.rs"]
mod ripr_seam_proof_incoming_request_owner;
