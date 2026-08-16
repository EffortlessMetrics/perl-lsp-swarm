//! Connection-scoped ownership for requests initiated by the language server.
//!
//! This module deliberately owns only request identity, bounded admission,
//! result-shape decoding, and terminal cleanup. Feature-specific state changes
//! remain with the callers that consume the completion. Incoming response
//! routing is wired separately by #7010.

use crate::protocol::{JsonRpcError, JsonRpcId};
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

pub(crate) const DEFAULT_SERVER_REQUEST_CAPACITY: usize = 64;
pub(crate) const MAX_SERVER_REQUEST_CAPACITY: usize = 4_096;
const MAX_SERVER_REQUEST_ID: i64 = i32::MAX as i64;
const MAX_RECENT_TERMINALS: usize = 256;
const MAX_ANOMALIES: usize = 64;
const MAX_METHOD_BYTES: usize = 128;
const MAX_DEBUG_IDENTITY_BYTES: usize = 256;
const MAX_DIAGNOSTIC_BYTES: usize = 384;

/// Result shape expected for one server-initiated request.
///
/// The registry keeps this deliberately small. Feature owners may perform
/// stricter semantic validation after receiving a shape-valid value, while a
/// syntactically valid response with the wrong top-level shape is terminally
/// classified as malformed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServerRequestResultDecoder {
    Any,
    Null,
    Boolean,
    Object,
    Array,
}

impl ServerRequestResultDecoder {
    fn expected(self) -> &'static str {
        match self {
            Self::Any => "any JSON value",
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Object => "object",
            Self::Array => "array",
        }
    }

    fn decode(self, value: Value) -> Result<ServerRequestTerminalOutcome, MalformedResult> {
        let matches = match self {
            Self::Any => true,
            Self::Null => value.is_null(),
            Self::Boolean => value.is_boolean(),
            Self::Object => value.is_object(),
            Self::Array => value.is_array(),
        };

        if !matches {
            return Err(MalformedResult {
                expected: self.expected(),
                observed: json_type_name(&value),
                summary: bounded_value_summary(&value),
            });
        }

        if value.is_null() {
            Ok(ServerRequestTerminalOutcome::SuccessNull)
        } else {
            Ok(ServerRequestTerminalOutcome::SuccessValue(value))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MalformedResult {
    pub(crate) expected: &'static str,
    pub(crate) observed: &'static str,
    pub(crate) summary: String,
}

/// One terminal outcome for a request initiated by the server.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ServerRequestTerminalOutcome {
    SuccessNull,
    SuccessValue(Value),
    ClientError { code: i32, message: String, data_summary: Option<String> },
    MalformedResult(MalformedResult),
    TimedOut,
    Cancelled,
    Shutdown,
    TransportLost,
}

impl ServerRequestTerminalOutcome {
    fn kind(&self) -> TerminalKind {
        match self {
            Self::SuccessNull => TerminalKind::SuccessNull,
            Self::SuccessValue(_) => TerminalKind::SuccessValue,
            Self::ClientError { .. } => TerminalKind::ClientError,
            Self::MalformedResult(_) => TerminalKind::MalformedResult,
            Self::TimedOut => TerminalKind::TimedOut,
            Self::Cancelled => TerminalKind::Cancelled,
            Self::Shutdown => TerminalKind::Shutdown,
            Self::TransportLost => TerminalKind::TransportLost,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ServerRequestCompletion {
    pub(crate) id: JsonRpcId,
    pub(crate) method: String,
    pub(crate) debug_identity: String,
    pub(crate) created_at: Instant,
    pub(crate) completed_at: Instant,
    pub(crate) outcome: ServerRequestTerminalOutcome,
}

/// Reservation returned before an outbound request may be emitted.
///
/// The caller owns the receiver. Dropping it does not retain the pending entry;
/// the registry still terminally removes the request and records bounded
/// diagnostic evidence that the consumer disappeared.
pub(crate) struct ServerRequestRegistration {
    pub(crate) id: JsonRpcId,
    pub(crate) completion: Receiver<ServerRequestCompletion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingServerRequestSnapshot {
    pub(crate) id: JsonRpcId,
    pub(crate) method: String,
    pub(crate) debug_identity: String,
    pub(crate) created_at: Instant,
    pub(crate) deadline: Instant,
    pub(crate) decoder: ServerRequestResultDecoder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServerRequestCompletionDisposition {
    Completed,
    Unknown,
    AlreadyTerminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServerRequestAnomalyKind {
    UnknownResponse,
    LateOrDuplicateResponse,
    CompletionReceiverDropped,
    IdSpaceExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerRequestAnomaly {
    pub(crate) kind: ServerRequestAnomalyKind,
    pub(crate) id: Option<JsonRpcId>,
    pub(crate) method: Option<String>,
    pub(crate) detail: String,
    pub(crate) observed_at: Instant,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ServerRequestRegistryCounters {
    pub(crate) capacity: usize,
    pub(crate) pending: usize,
    pub(crate) registered: u64,
    pub(crate) completed_total: u64,
    pub(crate) success_null: u64,
    pub(crate) success_value: u64,
    pub(crate) client_error: u64,
    pub(crate) malformed_result: u64,
    pub(crate) timed_out: u64,
    pub(crate) cancelled: u64,
    pub(crate) shutdown: u64,
    pub(crate) transport_lost: u64,
    pub(crate) capacity_rejected: u64,
    pub(crate) unknown_response: u64,
    pub(crate) late_or_duplicate_response: u64,
    pub(crate) completion_receiver_dropped: u64,
    pub(crate) anomalies_dropped: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServerRequestRegistryError {
    InvalidCapacity { requested: usize, maximum: usize },
    CapacityExhausted { capacity: usize },
    IdUnavailable { id: String },
    IdSpaceExhausted,
    DeadlineOverflow,
}

impl fmt::Display for ServerRequestRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCapacity { requested, maximum } => {
                write!(f, "server-request registry capacity {requested} is outside 1..={maximum}")
            }
            Self::CapacityExhausted { capacity } => {
                write!(f, "server-request registry capacity {capacity} is exhausted")
            }
            Self::IdUnavailable { id } => {
                write!(f, "server-request id {id} is pending or recently terminal")
            }
            Self::IdSpaceExhausted => write!(f, "server-request numeric id space is exhausted"),
            Self::DeadlineOverflow => write!(f, "server-request deadline overflowed Instant"),
        }
    }
}

impl Error for ServerRequestRegistryError {}

#[derive(Clone)]
pub(crate) struct ServerRequestRegistry {
    inner: Arc<RegistryInner>,
}

struct RegistryInner {
    state: Mutex<RegistryState>,
}

struct RegistryState {
    capacity: usize,
    next_numeric_id: i64,
    pending: BTreeMap<ServerRequestKey, PendingServerRequest>,
    recent_terminals: VecDeque<RecentTerminal>,
    anomalies: VecDeque<ServerRequestAnomaly>,
    counters: ServerRequestRegistryCounters,
}

struct PendingServerRequest {
    method: String,
    debug_identity: String,
    created_at: Instant,
    deadline: Instant,
    decoder: ServerRequestResultDecoder,
    completion: SyncSender<ServerRequestCompletion>,
}

#[derive(Debug, Clone)]
struct RecentTerminal {
    id: ServerRequestKey,
    method: String,
    kind: TerminalKind,
    completed_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalKind {
    SuccessNull,
    SuccessValue,
    ClientError,
    MalformedResult,
    TimedOut,
    Cancelled,
    Shutdown,
    TransportLost,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ServerRequestKey {
    Integer(i64),
    String(String),
}

impl ServerRequestKey {
    fn from_json_rpc_id(id: &JsonRpcId) -> Self {
        match id {
            JsonRpcId::Integer(value) => Self::Integer(*value),
            JsonRpcId::String(value) => Self::String(value.clone()),
        }
    }

    fn to_json_rpc_id(&self) -> JsonRpcId {
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

impl Default for ServerRequestRegistry {
    fn default() -> Self {
        Self::with_valid_capacity(DEFAULT_SERVER_REQUEST_CAPACITY)
    }
}

impl ServerRequestRegistry {
    pub(crate) fn new(capacity: usize) -> Result<Self, ServerRequestRegistryError> {
        if capacity == 0 || capacity > MAX_SERVER_REQUEST_CAPACITY {
            return Err(ServerRequestRegistryError::InvalidCapacity {
                requested: capacity,
                maximum: MAX_SERVER_REQUEST_CAPACITY,
            });
        }
        Ok(Self::with_valid_capacity(capacity))
    }

    fn with_valid_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                state: Mutex::new(RegistryState {
                    capacity,
                    next_numeric_id: 1,
                    pending: BTreeMap::new(),
                    recent_terminals: VecDeque::new(),
                    anomalies: VecDeque::new(),
                    counters: ServerRequestRegistryCounters {
                        capacity,
                        ..ServerRequestRegistryCounters::default()
                    },
                }),
            }),
        }
    }

    pub(crate) fn register(
        &self,
        method: impl Into<String>,
        debug_identity: impl Into<String>,
        timeout: Duration,
        decoder: ServerRequestResultDecoder,
    ) -> Result<ServerRequestRegistration, ServerRequestRegistryError> {
        let mut state = self.inner.state.lock();
        reject_if_full(&mut state)?;
        let key = allocate_numeric_id(&mut state)?;
        register_locked(&mut state, key, method.into(), debug_identity.into(), timeout, decoder)
    }

    pub(crate) fn register_with_id(
        &self,
        id: JsonRpcId,
        method: impl Into<String>,
        debug_identity: impl Into<String>,
        timeout: Duration,
        decoder: ServerRequestResultDecoder,
    ) -> Result<ServerRequestRegistration, ServerRequestRegistryError> {
        let mut state = self.inner.state.lock();
        reject_if_full(&mut state)?;
        let key = ServerRequestKey::from_json_rpc_id(&id);
        if state.pending.contains_key(&key)
            || state.recent_terminals.iter().any(|terminal| terminal.id == key)
        {
            return Err(ServerRequestRegistryError::IdUnavailable {
                id: key.diagnostic_identity(),
            });
        }
        register_locked(&mut state, key, method.into(), debug_identity.into(), timeout, decoder)
    }

    pub(crate) fn complete_success(
        &self,
        id: &JsonRpcId,
        result: Value,
    ) -> ServerRequestCompletionDisposition {
        let key = ServerRequestKey::from_json_rpc_id(id);
        let now = Instant::now();
        let delivery = {
            let mut state = self.inner.state.lock();
            let Some(pending) = state.pending.remove(&key) else {
                return record_missing_response(
                    &mut state,
                    &key,
                    format!("success result {}", bounded_value_summary(&result)),
                    now,
                );
            };
            let outcome = match pending.decoder.decode(result) {
                Ok(outcome) => outcome,
                Err(malformed) => ServerRequestTerminalOutcome::MalformedResult(malformed),
            };
            finalize_locked(&mut state, key, pending, outcome, now)
        };
        self.deliver(delivery);
        ServerRequestCompletionDisposition::Completed
    }

    pub(crate) fn complete_client_error(
        &self,
        id: &JsonRpcId,
        error: JsonRpcError,
    ) -> ServerRequestCompletionDisposition {
        let key = ServerRequestKey::from_json_rpc_id(id);
        let now = Instant::now();
        let delivery = {
            let mut state = self.inner.state.lock();
            let Some(pending) = state.pending.remove(&key) else {
                let detail = format!(
                    "client error code={} message={}",
                    error.code,
                    bounded_text(&error.message, MAX_DIAGNOSTIC_BYTES)
                );
                return record_missing_response(&mut state, &key, detail, now);
            };
            let outcome = ServerRequestTerminalOutcome::ClientError {
                code: error.code,
                message: bounded_text(&error.message, MAX_DIAGNOSTIC_BYTES),
                data_summary: error.data.as_ref().map(bounded_value_summary),
            };
            finalize_locked(&mut state, key, pending, outcome, now)
        };
        self.deliver(delivery);
        ServerRequestCompletionDisposition::Completed
    }

    pub(crate) fn cancel(&self, id: &JsonRpcId) -> ServerRequestCompletionDisposition {
        self.complete_with_terminal(id, ServerRequestTerminalOutcome::Cancelled)
    }

    pub(crate) fn expire_deadlines(&self, now: Instant) -> usize {
        let deliveries = {
            let mut state = self.inner.state.lock();
            let expired = state
                .pending
                .iter()
                .filter_map(|(id, pending)| (pending.deadline <= now).then_some(id.clone()))
                .collect::<Vec<_>>();
            expired
                .into_iter()
                .filter_map(|id| {
                    state.pending.remove(&id).map(|pending| {
                        finalize_locked(
                            &mut state,
                            id,
                            pending,
                            ServerRequestTerminalOutcome::TimedOut,
                            now,
                        )
                    })
                })
                .collect::<Vec<_>>()
        };
        let count = deliveries.len();
        for delivery in deliveries {
            self.deliver(delivery);
        }
        count
    }

    pub(crate) fn shutdown(&self) -> usize {
        self.drain(ServerRequestTerminalOutcome::Shutdown)
    }

    pub(crate) fn transport_lost(&self) -> usize {
        self.drain(ServerRequestTerminalOutcome::TransportLost)
    }

    pub(crate) fn pending_count(&self) -> usize {
        self.inner.state.lock().pending.len()
    }

    pub(crate) fn counters(&self) -> ServerRequestRegistryCounters {
        let state = self.inner.state.lock();
        ServerRequestRegistryCounters { pending: state.pending.len(), ..state.counters }
    }

    pub(crate) fn pending_snapshots(&self) -> Vec<PendingServerRequestSnapshot> {
        let state = self.inner.state.lock();
        state
            .pending
            .iter()
            .map(|(id, pending)| PendingServerRequestSnapshot {
                id: id.to_json_rpc_id(),
                method: pending.method.clone(),
                debug_identity: pending.debug_identity.clone(),
                created_at: pending.created_at,
                deadline: pending.deadline,
                decoder: pending.decoder,
            })
            .collect()
    }

    pub(crate) fn anomalies(&self) -> Vec<ServerRequestAnomaly> {
        self.inner.state.lock().anomalies.iter().cloned().collect()
    }

    fn complete_with_terminal(
        &self,
        id: &JsonRpcId,
        outcome: ServerRequestTerminalOutcome,
    ) -> ServerRequestCompletionDisposition {
        let key = ServerRequestKey::from_json_rpc_id(id);
        let now = Instant::now();
        let delivery = {
            let mut state = self.inner.state.lock();
            let Some(pending) = state.pending.remove(&key) else {
                return record_missing_response(
                    &mut state,
                    &key,
                    format!("terminal outcome {:?}", outcome.kind()),
                    now,
                );
            };
            finalize_locked(&mut state, key, pending, outcome, now)
        };
        self.deliver(delivery);
        ServerRequestCompletionDisposition::Completed
    }

    fn drain(&self, outcome: ServerRequestTerminalOutcome) -> usize {
        let now = Instant::now();
        let deliveries = {
            let mut state = self.inner.state.lock();
            let pending = std::mem::take(&mut state.pending);
            pending
                .into_iter()
                .map(|(id, request)| finalize_locked(&mut state, id, request, outcome.clone(), now))
                .collect::<Vec<_>>()
        };
        let count = deliveries.len();
        for delivery in deliveries {
            self.deliver(delivery);
        }
        count
    }

    fn deliver(&self, delivery: Delivery) {
        if delivery.sender.send(delivery.completion.clone()).is_err() {
            let mut state = self.inner.state.lock();
            state.counters.completion_receiver_dropped =
                state.counters.completion_receiver_dropped.saturating_add(1);
            push_anomaly(
                &mut state,
                ServerRequestAnomalyKind::CompletionReceiverDropped,
                Some(ServerRequestKey::from_json_rpc_id(&delivery.completion.id)),
                Some(delivery.completion.method),
                "completion receiver was dropped before terminal delivery".to_string(),
                delivery.completion.completed_at,
            );
        }
    }

    #[cfg(test)]
    fn set_next_numeric_id_for_test(&self, next: i64) {
        self.inner.state.lock().next_numeric_id = next;
    }
}

struct Delivery {
    sender: SyncSender<ServerRequestCompletion>,
    completion: ServerRequestCompletion,
}

fn reject_if_full(state: &mut RegistryState) -> Result<(), ServerRequestRegistryError> {
    if state.pending.len() < state.capacity {
        return Ok(());
    }
    state.counters.capacity_rejected = state.counters.capacity_rejected.saturating_add(1);
    Err(ServerRequestRegistryError::CapacityExhausted { capacity: state.capacity })
}

fn allocate_numeric_id(
    state: &mut RegistryState,
) -> Result<ServerRequestKey, ServerRequestRegistryError> {
    let attempts = state.capacity.saturating_add(MAX_RECENT_TERMINALS).saturating_add(1);
    for _ in 0..attempts {
        let candidate = state.next_numeric_id.clamp(1, MAX_SERVER_REQUEST_ID);
        state.next_numeric_id = if candidate == MAX_SERVER_REQUEST_ID { 1 } else { candidate + 1 };
        let key = ServerRequestKey::Integer(candidate);
        let unavailable = state.pending.contains_key(&key)
            || state.recent_terminals.iter().any(|terminal| terminal.id == key);
        if !unavailable {
            return Ok(key);
        }
    }

    state.counters.capacity_rejected = state.counters.capacity_rejected.saturating_add(1);
    push_anomaly(
        state,
        ServerRequestAnomalyKind::IdSpaceExhausted,
        None,
        None,
        "numeric request-id scan found no available id".to_string(),
        Instant::now(),
    );
    Err(ServerRequestRegistryError::IdSpaceExhausted)
}

fn register_locked(
    state: &mut RegistryState,
    key: ServerRequestKey,
    method: String,
    debug_identity: String,
    timeout: Duration,
    decoder: ServerRequestResultDecoder,
) -> Result<ServerRequestRegistration, ServerRequestRegistryError> {
    let created_at = Instant::now();
    let Some(deadline) = created_at.checked_add(timeout) else {
        return Err(ServerRequestRegistryError::DeadlineOverflow);
    };
    let method = bounded_text(&method, MAX_METHOD_BYTES);
    let debug_identity = bounded_text(&debug_identity, MAX_DEBUG_IDENTITY_BYTES);
    let (sender, completion) = mpsc::sync_channel(1);
    let id = key.to_json_rpc_id();
    state.pending.insert(
        key,
        PendingServerRequest {
            method,
            debug_identity,
            created_at,
            deadline,
            decoder,
            completion: sender,
        },
    );
    state.counters.registered = state.counters.registered.saturating_add(1);
    Ok(ServerRequestRegistration { id, completion })
}

fn finalize_locked(
    state: &mut RegistryState,
    id: ServerRequestKey,
    pending: PendingServerRequest,
    outcome: ServerRequestTerminalOutcome,
    completed_at: Instant,
) -> Delivery {
    let kind = outcome.kind();
    state.counters.completed_total = state.counters.completed_total.saturating_add(1);
    match kind {
        TerminalKind::SuccessNull => {
            state.counters.success_null = state.counters.success_null.saturating_add(1);
        }
        TerminalKind::SuccessValue => {
            state.counters.success_value = state.counters.success_value.saturating_add(1);
        }
        TerminalKind::ClientError => {
            state.counters.client_error = state.counters.client_error.saturating_add(1);
        }
        TerminalKind::MalformedResult => {
            state.counters.malformed_result = state.counters.malformed_result.saturating_add(1);
        }
        TerminalKind::TimedOut => {
            state.counters.timed_out = state.counters.timed_out.saturating_add(1);
        }
        TerminalKind::Cancelled => {
            state.counters.cancelled = state.counters.cancelled.saturating_add(1);
        }
        TerminalKind::Shutdown => {
            state.counters.shutdown = state.counters.shutdown.saturating_add(1);
        }
        TerminalKind::TransportLost => {
            state.counters.transport_lost = state.counters.transport_lost.saturating_add(1);
        }
    }

    state.recent_terminals.push_back(RecentTerminal {
        id: id.clone(),
        method: pending.method.clone(),
        kind,
        completed_at,
    });
    while state.recent_terminals.len() > MAX_RECENT_TERMINALS {
        state.recent_terminals.pop_front();
    }

    Delivery {
        sender: pending.completion,
        completion: ServerRequestCompletion {
            id: id.to_json_rpc_id(),
            method: pending.method,
            debug_identity: pending.debug_identity,
            created_at: pending.created_at,
            completed_at,
            outcome,
        },
    }
}

fn record_missing_response(
    state: &mut RegistryState,
    id: &ServerRequestKey,
    detail: String,
    observed_at: Instant,
) -> ServerRequestCompletionDisposition {
    if let Some(terminal) = state.recent_terminals.iter().find(|terminal| &terminal.id == id) {
        state.counters.late_or_duplicate_response =
            state.counters.late_or_duplicate_response.saturating_add(1);
        let method = terminal.method.clone();
        let terminal_detail = format!(
            "{detail}; prior terminal={:?} completed_at={:?}",
            terminal.kind, terminal.completed_at
        );
        push_anomaly(
            state,
            ServerRequestAnomalyKind::LateOrDuplicateResponse,
            Some(id.clone()),
            Some(method),
            terminal_detail,
            observed_at,
        );
        ServerRequestCompletionDisposition::AlreadyTerminal
    } else {
        state.counters.unknown_response = state.counters.unknown_response.saturating_add(1);
        push_anomaly(
            state,
            ServerRequestAnomalyKind::UnknownResponse,
            Some(id.clone()),
            None,
            detail,
            observed_at,
        );
        ServerRequestCompletionDisposition::Unknown
    }
}

fn push_anomaly(
    state: &mut RegistryState,
    kind: ServerRequestAnomalyKind,
    id: Option<ServerRequestKey>,
    method: Option<String>,
    detail: String,
    observed_at: Instant,
) {
    if state.anomalies.len() == MAX_ANOMALIES {
        state.anomalies.pop_front();
        state.counters.anomalies_dropped = state.counters.anomalies_dropped.saturating_add(1);
    }
    state.anomalies.push_back(ServerRequestAnomaly {
        kind,
        id: id.map(|value| value.to_json_rpc_id()),
        method: method.map(|value| bounded_text(&value, MAX_METHOD_BYTES)),
        detail: bounded_text(&detail, MAX_DIAGNOSTIC_BYTES),
        observed_at,
    });
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn bounded_value_summary(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => format!("boolean:{value}"),
        Value::Number(value) => format!("number:{value}"),
        Value::String(value) => format!("string(len={})", value.len()),
        Value::Array(values) => format!("array(len={})", values.len()),
        Value::Object(values) => {
            let keys = values
                .keys()
                .take(8)
                .map(|key| bounded_text(key, 48))
                .collect::<Vec<_>>()
                .join(",");
            format!("object(len={},keys=[{}])", values.len(), keys)
        }
    }
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

    fn receive(registration: ServerRequestRegistration) -> TestResult<ServerRequestCompletion> {
        Ok(registration.completion.recv_timeout(Duration::from_secs(1))?)
    }

    fn join<T>(handle: thread::JoinHandle<T>) -> TestResult<T> {
        handle.join().map_err(|_| io::Error::other("registry race thread panicked").into())
    }

    #[test]
    fn numeric_and_string_ids_are_distinct() -> TestResult {
        let registry = ServerRequestRegistry::new(4)?;
        let numeric = registry.register_with_id(
            JsonRpcId::Integer(1),
            "numeric",
            "numeric-operation",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Any,
        )?;
        let string = registry.register_with_id(
            JsonRpcId::String("1".to_string()),
            "string",
            "string-operation",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Any,
        )?;

        assert_eq!(registry.pending_count(), 2);
        assert_eq!(
            registry.complete_success(&JsonRpcId::Integer(1), json!({"kind": "number"})),
            ServerRequestCompletionDisposition::Completed
        );
        assert_eq!(
            registry
                .complete_success(&JsonRpcId::String("1".to_string()), json!({"kind": "string"})),
            ServerRequestCompletionDisposition::Completed
        );
        assert_eq!(receive(numeric)?.method, "numeric");
        assert_eq!(receive(string)?.method, "string");
        assert_eq!(registry.pending_count(), 0);
        Ok(())
    }

    #[test]
    fn capacity_exhaustion_fails_before_a_second_reservation() -> TestResult {
        let registry = ServerRequestRegistry::new(1)?;
        let first = registry.register(
            "first",
            "first-operation",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Null,
        )?;
        let error = registry.register(
            "second",
            "second-operation",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Null,
        );
        assert!(matches!(
            error,
            Err(ServerRequestRegistryError::CapacityExhausted { capacity: 1 })
        ));
        assert_eq!(registry.pending_count(), 1);
        assert_eq!(registry.counters().capacity_rejected, 1);
        assert_eq!(registry.cancel(&first.id), ServerRequestCompletionDisposition::Completed);
        assert!(matches!(receive(first)?.outcome, ServerRequestTerminalOutcome::Cancelled));
        Ok(())
    }

    #[test]
    fn wrong_result_shape_is_terminal_malformed() -> TestResult {
        let registry = ServerRequestRegistry::new(2)?;
        let registration = registry.register(
            "client/registerCapability",
            "registration:watchers",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Null,
        )?;
        assert_eq!(
            registry.complete_success(&registration.id, json!(true)),
            ServerRequestCompletionDisposition::Completed
        );
        let completion = receive(registration)?;
        let ServerRequestTerminalOutcome::MalformedResult(malformed) = completion.outcome else {
            return Err(io::Error::other("expected malformed terminal outcome").into());
        };
        assert_eq!(malformed.expected, "null");
        assert_eq!(malformed.observed, "boolean");
        assert_eq!(registry.counters().malformed_result, 1);
        assert_eq!(registry.pending_count(), 0);
        Ok(())
    }

    #[test]
    fn timeout_removes_once_and_late_response_is_anomaly() -> TestResult {
        let registry = ServerRequestRegistry::new(2)?;
        let registration = registry.register(
            "workspace/configuration",
            "config-generation:4",
            Duration::from_millis(1),
            ServerRequestResultDecoder::Array,
        )?;
        assert_eq!(registry.expire_deadlines(Instant::now() + Duration::from_secs(1)), 1);
        assert!(matches!(receive(registration)?.outcome, ServerRequestTerminalOutcome::TimedOut));
        assert_eq!(
            registry.complete_success(&JsonRpcId::Integer(1), json!([])),
            ServerRequestCompletionDisposition::AlreadyTerminal
        );
        let counters = registry.counters();
        assert_eq!(counters.timed_out, 1);
        assert_eq!(counters.late_or_duplicate_response, 1);
        assert_eq!(counters.pending, 0);
        Ok(())
    }

    #[test]
    fn response_and_timeout_race_selects_one_terminal() -> TestResult {
        let registry = ServerRequestRegistry::new(2)?;
        let registration = registry.register(
            "workspace/configuration",
            "config-generation:race",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Array,
        )?;
        let id = registration.id.clone();
        let barrier = Arc::new(Barrier::new(3));

        let response_registry = registry.clone();
        let response_barrier = barrier.clone();
        let response_id = id.clone();
        let response = thread::spawn(move || {
            response_barrier.wait();
            response_registry.complete_success(&response_id, json!([]))
        });

        let timeout_registry = registry.clone();
        let timeout_barrier = barrier.clone();
        let timeout = thread::spawn(move || {
            timeout_barrier.wait();
            timeout_registry.expire_deadlines(Instant::now() + Duration::from_secs(10))
        });

        barrier.wait();
        let response_disposition = join(response)?;
        let expired = join(timeout)?;
        let selected =
            usize::from(response_disposition == ServerRequestCompletionDisposition::Completed)
                + expired;
        assert_eq!(selected, 1);
        let completion = receive(registration)?;
        assert!(matches!(
            completion.outcome,
            ServerRequestTerminalOutcome::SuccessValue(_) | ServerRequestTerminalOutcome::TimedOut
        ));
        assert_eq!(registry.counters().completed_total, 1);
        assert_eq!(registry.pending_count(), 0);
        Ok(())
    }

    #[test]
    fn shutdown_drains_every_pending_request() -> TestResult {
        let registry = ServerRequestRegistry::new(4)?;
        let first = registry.register(
            "workspace/configuration",
            "config:1",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Array,
        )?;
        let second = registry.register(
            "window/workDoneProgress/create",
            "progress:1",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Null,
        )?;
        assert_eq!(registry.shutdown(), 2);
        assert!(matches!(receive(first)?.outcome, ServerRequestTerminalOutcome::Shutdown));
        assert!(matches!(receive(second)?.outcome, ServerRequestTerminalOutcome::Shutdown));
        assert_eq!(registry.counters().shutdown, 2);
        assert_eq!(registry.pending_count(), 0);
        Ok(())
    }

    #[test]
    fn allocator_wraps_and_skips_pending_or_recent_ids() -> TestResult {
        let registry = ServerRequestRegistry::new(4)?;
        registry.set_next_numeric_id_for_test(MAX_SERVER_REQUEST_ID);
        let maximum = registry.register(
            "maximum",
            "maximum",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Null,
        )?;
        let one = registry.register_with_id(
            JsonRpcId::Integer(1),
            "one",
            "one",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Null,
        )?;
        let two = registry.register(
            "two",
            "two",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Null,
        )?;
        assert_eq!(maximum.id, JsonRpcId::Integer(MAX_SERVER_REQUEST_ID));
        assert_eq!(two.id, JsonRpcId::Integer(2));
        assert_eq!(registry.cancel(&maximum.id), ServerRequestCompletionDisposition::Completed);
        assert_eq!(registry.cancel(&one.id), ServerRequestCompletionDisposition::Completed);
        assert_eq!(registry.cancel(&two.id), ServerRequestCompletionDisposition::Completed);
        let _ = receive(maximum)?;
        let _ = receive(one)?;
        let _ = receive(two)?;
        Ok(())
    }

    #[test]
    fn recently_terminal_id_cannot_be_reused_explicitly() -> TestResult {
        let registry = ServerRequestRegistry::new(2)?;
        let registration = registry.register_with_id(
            JsonRpcId::String("stable-id".to_string()),
            "window/showDocument",
            "document:1",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Object,
        )?;
        assert_eq!(
            registry.cancel(&registration.id),
            ServerRequestCompletionDisposition::Completed
        );
        let _ = receive(registration)?;
        let reuse = registry.register_with_id(
            JsonRpcId::String("stable-id".to_string()),
            "window/showDocument",
            "document:2",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Object,
        );
        assert!(matches!(reuse, Err(ServerRequestRegistryError::IdUnavailable { .. })));
        Ok(())
    }

    #[test]
    fn dropped_completion_receiver_is_bounded_evidence_not_retained_state() -> TestResult {
        let registry = ServerRequestRegistry::new(2)?;
        let registration = registry.register(
            "workspace/semanticTokens/refresh",
            "semantic-refresh:1",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Null,
        )?;
        let id = registration.id.clone();
        drop(registration);
        assert_eq!(
            registry.complete_success(&id, Value::Null),
            ServerRequestCompletionDisposition::Completed
        );
        let counters = registry.counters();
        assert_eq!(counters.completion_receiver_dropped, 1);
        assert_eq!(counters.pending, 0);
        assert!(
            registry
                .anomalies()
                .iter()
                .any(|anomaly| anomaly.kind == ServerRequestAnomalyKind::CompletionReceiverDropped)
        );
        Ok(())
    }

    #[test]
    fn anomaly_buffer_is_bounded() -> TestResult {
        let registry = ServerRequestRegistry::new(1)?;
        for id in 0..(MAX_ANOMALIES + 10) {
            let disposition = registry.complete_success(
                &JsonRpcId::Integer(i64::try_from(id)? + 10_000),
                json!({"ignored": id}),
            );
            assert_eq!(disposition, ServerRequestCompletionDisposition::Unknown);
        }
        assert_eq!(registry.anomalies().len(), MAX_ANOMALIES);
        assert_eq!(registry.counters().anomalies_dropped, 10);
        Ok(())
    }

    #[test]
    fn pending_snapshot_retains_method_deadline_and_debug_identity() -> TestResult {
        let registry = ServerRequestRegistry::new(2)?;
        let registration = registry.register(
            "workspace/applyEdit",
            "rename-operation:42",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Object,
        )?;
        let snapshots = registry.pending_snapshots();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].id, registration.id);
        assert_eq!(snapshots[0].method, "workspace/applyEdit");
        assert_eq!(snapshots[0].debug_identity, "rename-operation:42");
        assert!(snapshots[0].deadline >= snapshots[0].created_at);
        assert_eq!(snapshots[0].decoder, ServerRequestResultDecoder::Object);
        assert_eq!(registry.transport_lost(), 1);
        assert!(matches!(
            receive(registration)?.outcome,
            ServerRequestTerminalOutcome::TransportLost
        ));
        Ok(())
    }
}
