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
}

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
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
    pub(crate) anomalies_dropped: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IncomingRequestRegistryError {
    InvalidCapacity { requested: usize, maximum: usize },
    CapacityExhausted { capacity: usize },
    DuplicateId { id: String },
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
    fn from_id(id: &JsonRpcId) -> Self {
        match id {
            JsonRpcId::Integer(value) => Self::Integer(*value),
            JsonRpcId::String(value) => Self::String(value.clone()),
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
            self.inner.state.lock().counters.notifications_bypassed = self
                .inner
                .state
                .lock()
                .counters
                .notifications_bypassed
                .saturating_add(1);
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
        let key = IncomingRequestKey::from_id(&id);
        let now = Instant::now();
        let mut state = self.inner.state.lock();
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
                    terminal_kind: entry.terminal_kind.unwrap_or(IncomingTerminalKind::Error),
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
        let key = IncomingRequestKey::from_id(id);
        let now = Instant::now();
        let mut state = self.inner.state.lock();
        let Some(entry) = state.active.get_mut(&key) else {
            record_unknown(&mut state, &key, "phase transition", now);
            return PhaseTransitionDisposition::Unknown;
        };
        if entry.phase == IncomingRequestPhase::TerminalSelected {
            return PhaseTransitionDisposition::AlreadyTerminal;
        }
        if target.rank() < entry.phase.rank() {
            let method = entry.method.clone();
            let detail = format!("phase regression {:?} -> {target:?}", entry.phase);
            push_anomaly(
                &mut state,
                IncomingRequestAnomalyKind::InvalidPhaseRegression,
                Some(key),
                Some(method),
                detail,
                now,
            );
            return PhaseTransitionDisposition::Unchanged;
        }
        if target == entry.phase {
            return PhaseTransitionDisposition::Unchanged;
        }
        entry.phase = target;
        entry.phase_changed_at = now;
        state.counters.phase_advanced = state.counters.phase_advanced.saturating_add(1);
        PhaseTransitionDisposition::Advanced
    }

    fn select_terminal(
        &self,
        id: &JsonRpcId,
        outcome: IncomingTerminalOutcome,
        shutdown: bool,
    ) -> (TerminalSelectionDisposition, Option<SelectedIncomingTerminal>) {
        let key = IncomingRequestKey::from_id(id);
        let now = Instant::now();
        let mut state = self.inner.state.lock();
        let Some(entry) = state.active.get_mut(&key) else {
            if let Some(recent) = state.recent.iter().find(|recent| recent.id == key) {
                state.counters.duplicate_terminal =
                    state.counters.duplicate_terminal.saturating_add(1);
                let method = recent.method.clone();
                let detail = format!(
                    "terminal already cleaned kind={:?} response_written={} cleaned_at={:?}",
                    recent.terminal_kind, recent.response_written, recent.cleaned_at
                );
                push_anomaly(
                    &mut state,
                    IncomingRequestAnomalyKind::DuplicateTerminal,
                    Some(key),
                    Some(method),
                    detail,
                    now,
                );
                return (TerminalSelectionDisposition::AlreadyTerminal, None);
            }
            record_unknown(&mut state, &key, "terminal selection", now);
            return (TerminalSelectionDisposition::Unknown, None);
        };

        if entry.phase == IncomingRequestPhase::TerminalSelected {
            let method = entry.method.clone();
            state.counters.duplicate_terminal = state.counters.duplicate_terminal.saturating_add(1);
            push_anomaly(
                &mut state,
                IncomingRequestAnomalyKind::DuplicateTerminal,
                Some(key),
                Some(method),
                "second terminal selection rejected".to_string(),
                now,
            );
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
        state.counters.terminal_selected_total =
            state.counters.terminal_selected_total.saturating_add(1);
        if shutdown {
            state.counters.shutdown_selected = state.counters.shutdown_selected.saturating_add(1);
        } else {
            match terminal_kind {
                IncomingTerminalKind::Result => {
                    state.counters.result_selected = state.counters.result_selected.saturating_add(1);
                }
                IncomingTerminalKind::Error => {
                    state.counters.error_selected = state.counters.error_selected.saturating_add(1);
                }
                IncomingTerminalKind::Shutdown => {}
            }
        }
        (TerminalSelectionDisposition::Selected, Some(selected))
    }

    fn finish_response(
        &self,
        id: &JsonRpcId,
        written: bool,
        failure_detail: Option<&str>,
    ) -> ResponseWriteDisposition {
        let key = IncomingRequestKey::from_id(id);
        let now = Instant::now();
        let mut state = self.inner.state.lock();
        let Some(entry) = state.active.remove(&key) else {
            if let Some(recent) = state.recent.iter().find(|recent| recent.id == key) {
                let method = recent.method.clone();
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
            state.active.insert(key.clone(), entry);
            push_anomaly(
                &mut state,
                IncomingRequestAnomalyKind::ResponseBeforeTerminal,
                Some(key),
                None,
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
                bounded_text(failure_detail.unwrap_or("response write failed"), MAX_DIAGNOSTIC_BYTES),
                now,
            );
            ResponseWriteDisposition::WriteFailedAndCleaned
        }
    }
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
mod tests {
    use super::*;
    use serde_json::json;
    use std::io;
    use std::sync::Barrier;
    use std::thread;

    type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

    fn join<T>(handle: thread::JoinHandle<T>) -> TestResult<T> {
        handle.join().map_err(|_| io::Error::other("request lifecycle race thread panicked").into())
    }

    fn error(code: i32, message: &str) -> JsonRpcError {
        JsonRpcError { code, message: message.to_string(), data: None }
    }

    #[test]
    fn notifications_bypass_without_consuming_capacity() -> TestResult {
        let registry = IncomingRequestRegistry::new(1)?;
        let notification = registry.admit_optional(None, "textDocument/didOpen")?;
        assert!(notification.is_none());
        assert_eq!(registry.active_count(), 0);
        assert_eq!(registry.counters().notifications_bypassed, 1);
        Ok(())
    }

    #[test]
    fn numeric_and_string_ids_are_distinct() -> TestResult {
        let registry = IncomingRequestRegistry::new(2)?;
        let numeric = registry.admit(JsonRpcId::Integer(1), "textDocument/hover")?;
        let string = registry.admit(JsonRpcId::String("1".to_string()), "textDocument/hover")?;
        assert_eq!(registry.active_count(), 2);
        assert_ne!(numeric.id, string.id);

        let (numeric_disposition, _) = registry.select_result(&numeric.id, Value::Null);
        let (string_disposition, _) = registry.select_error(&string.id, error(-32800, "cancelled"));
        assert_eq!(numeric_disposition, TerminalSelectionDisposition::Selected);
        assert_eq!(string_disposition, TerminalSelectionDisposition::Selected);
        assert_eq!(registry.response_written(&numeric.id), ResponseWriteDisposition::WrittenAndCleaned);
        assert_eq!(registry.response_written(&string.id), ResponseWriteDisposition::WrittenAndCleaned);
        assert_eq!(registry.active_count(), 0);
        Ok(())
    }

    #[test]
    fn capacity_rejects_before_an_unowned_request_is_accepted() -> TestResult {
        let registry = IncomingRequestRegistry::new(1)?;
        let first = registry.admit(JsonRpcId::Integer(1), "textDocument/completion")?;
        let second = registry.admit(JsonRpcId::Integer(2), "textDocument/hover");
        assert!(matches!(
            second,
            Err(IncomingRequestRegistryError::CapacityExhausted { capacity: 1 })
        ));
        assert_eq!(registry.counters().capacity_rejected, 1);
        let _ = registry.select_error(&first.id, error(-32099, "overload test cleanup"));
        assert_eq!(registry.response_written(&first.id), ResponseWriteDisposition::WrittenAndCleaned);
        Ok(())
    }

    #[test]
    fn phase_progression_is_monotonic() -> TestResult {
        let registry = IncomingRequestRegistry::new(2)?;
        let handle = registry.admit(JsonRpcId::Integer(7), "workspace/symbol")?;
        assert_eq!(registry.mark_queued(&handle.id), PhaseTransitionDisposition::Advanced);
        assert_eq!(registry.mark_running(&handle.id), PhaseTransitionDisposition::Advanced);
        assert_eq!(registry.mark_queued(&handle.id), PhaseTransitionDisposition::Unchanged);
        let snapshot = registry.snapshots().into_iter().next().ok_or("missing snapshot")?;
        assert_eq!(snapshot.phase, IncomingRequestPhase::Running);
        assert!(registry
            .anomalies()
            .iter()
            .any(|anomaly| anomaly.kind == IncomingRequestAnomalyKind::InvalidPhaseRegression));
        let _ = registry.select_result(&handle.id, json!([]));
        assert_eq!(registry.response_written(&handle.id), ResponseWriteDisposition::WrittenAndCleaned);
        Ok(())
    }

    #[test]
    fn terminal_is_selected_once_and_cleanup_is_explicit() -> TestResult {
        let registry = IncomingRequestRegistry::new(2)?;
        let handle = registry.admit(JsonRpcId::Integer(11), "textDocument/definition")?;
        let (first, selected) = registry.select_result(&handle.id, json!({"uri": "file:///a.pm"}));
        assert_eq!(first, TerminalSelectionDisposition::Selected);
        assert!(selected.is_some());
        let (second, duplicate) = registry.select_error(&handle.id, error(-32800, "too late"));
        assert_eq!(second, TerminalSelectionDisposition::AlreadyTerminal);
        assert!(duplicate.is_none());
        assert_eq!(registry.active_count(), 1, "selection alone must not hide write leaks");
        assert_eq!(registry.response_written(&handle.id), ResponseWriteDisposition::WrittenAndCleaned);
        let (third, _) = registry.select_result(&handle.id, Value::Null);
        assert_eq!(third, TerminalSelectionDisposition::AlreadyTerminal);
        assert_eq!(registry.counters().duplicate_terminal, 2);
        Ok(())
    }

    #[test]
    fn response_before_terminal_does_not_drop_the_request() -> TestResult {
        let registry = IncomingRequestRegistry::new(1)?;
        let handle = registry.admit(JsonRpcId::Integer(14), "textDocument/references")?;
        assert_eq!(
            registry.response_written(&handle.id),
            ResponseWriteDisposition::TerminalNotSelected
        );
        assert_eq!(registry.active_count(), 1);
        let _ = registry.select_result(&handle.id, json!([]));
        assert_eq!(registry.response_written(&handle.id), ResponseWriteDisposition::WrittenAndCleaned);
        Ok(())
    }

    #[test]
    fn response_write_failure_is_distinct_and_cleans_once() -> TestResult {
        let registry = IncomingRequestRegistry::new(1)?;
        let handle = registry.admit(JsonRpcId::String("write-failure".to_string()), "textDocument/rename")?;
        let _ = registry.select_error(&handle.id, error(-32603, "internal error"));
        assert_eq!(
            registry.response_write_failed(&handle.id, "broken pipe"),
            ResponseWriteDisposition::WriteFailedAndCleaned
        );
        assert_eq!(registry.active_count(), 0);
        assert_eq!(registry.counters().response_write_failed, 1);
        assert_eq!(registry.response_written(&handle.id), ResponseWriteDisposition::AlreadyCleaned);
        Ok(())
    }

    #[test]
    fn result_and_shutdown_race_select_exactly_one_terminal() -> TestResult {
        let registry = IncomingRequestRegistry::new(2)?;
        let handle = registry.admit(JsonRpcId::Integer(21), "workspace/symbol")?;
        let barrier = Arc::new(Barrier::new(3));

        let result_registry = registry.clone();
        let result_barrier = Arc::clone(&barrier);
        let result_id = handle.id.clone();
        let result_thread = thread::spawn(move || {
            result_barrier.wait();
            result_registry.select_result(&result_id, json!([])).0
        });

        let shutdown_registry = registry.clone();
        let shutdown_barrier = Arc::clone(&barrier);
        let shutdown_thread = thread::spawn(move || {
            shutdown_barrier.wait();
            shutdown_registry.select_shutdown_errors(-32097, "server shutdown")
        });

        barrier.wait();
        let result_disposition = join(result_thread)?;
        let shutdown_terminals = join(shutdown_thread)?;
        let selected_count = usize::from(result_disposition == TerminalSelectionDisposition::Selected)
            + shutdown_terminals.len();
        assert_eq!(selected_count, 1);
        assert_eq!(registry.counters().terminal_selected_total, 1);
        assert_eq!(registry.response_written(&handle.id), ResponseWriteDisposition::WrittenAndCleaned);
        Ok(())
    }

    #[test]
    fn transport_loss_cleans_accepted_queued_running_and_terminal_entries() -> TestResult {
        let registry = IncomingRequestRegistry::new(4)?;
        let accepted = registry.admit(JsonRpcId::Integer(31), "accepted")?;
        let queued = registry.admit(JsonRpcId::Integer(32), "queued")?;
        let running = registry.admit(JsonRpcId::Integer(33), "running")?;
        let terminal = registry.admit(JsonRpcId::Integer(34), "terminal")?;
        let _ = registry.mark_queued(&queued.id);
        let _ = registry.mark_running(&running.id);
        let _ = registry.select_error(&terminal.id, error(-32800, "cancelled"));
        assert_eq!(registry.transport_lost(), 4);
        assert_eq!(registry.active_count(), 0);
        assert_eq!(registry.counters().transport_cleaned, 4);
        assert!(registry
            .anomalies()
            .iter()
            .filter(|anomaly| anomaly.kind == IncomingRequestAnomalyKind::TransportCleanup)
            .count()
            >= 4);
        let _ = accepted;
        Ok(())
    }

    #[test]
    fn duplicate_admission_is_rejected_without_replacing_owner() -> TestResult {
        let registry = IncomingRequestRegistry::new(2)?;
        let handle = registry.admit(JsonRpcId::String("same".to_string()), "first")?;
        let duplicate = registry.admit(JsonRpcId::String("same".to_string()), "second");
        assert!(matches!(duplicate, Err(IncomingRequestRegistryError::DuplicateId { .. })));
        let snapshot = registry.snapshots().into_iter().next().ok_or("missing owner")?;
        assert_eq!(snapshot.method, "first");
        let _ = registry.select_result(&handle.id, Value::Null);
        let _ = registry.response_written(&handle.id);
        Ok(())
    }

    #[test]
    fn anomaly_storage_is_bounded() -> TestResult {
        let registry = IncomingRequestRegistry::new(1)?;
        for index in 0..(MAX_ANOMALIES + 9) {
            let id = JsonRpcId::Integer(i64::try_from(index)? + 10_000);
            assert_eq!(registry.mark_running(&id), PhaseTransitionDisposition::Unknown);
        }
        assert_eq!(registry.anomalies().len(), MAX_ANOMALIES);
        assert_eq!(registry.counters().anomalies_dropped, 9);
        Ok(())
    }
}
