//! Connection-scoped ownership for requests initiated by the language server.
//!
//! This module deliberately owns only request identity, bounded admission,
//! result-shape decoding, and terminal cleanup. Feature-specific state changes
//! remain with the callers that consume the completion. Incoming response
//! routing is wired separately by #7010.
//!
//! # Bounded identity guarantee
//!
//! Identity ownership here is bounded by *count*, not by time. After a request
//! reaches a terminal outcome its id is retained in a `MAX_RECENT_TERMINALS`
//! ring so a late or duplicate response is classified as
//! [`ServerRequestAnomalyKind::LateOrDuplicateResponse`] instead of
//! [`ServerRequestAnomalyKind::UnknownResponse`], and so the id is refused for
//! reuse. That ring evicts oldest-first: once more than `MAX_RECENT_TERMINALS`
//! further requests complete on the same connection, an evicted id becomes
//! reusable immediately, a late response for it degrades to `UnknownResponse`,
//! and — if a caller re-registers that exact id through
//! [`ServerRequestRegistry::register_with_id`] — a late response may be matched
//! against the newer request.
//!
//! Callers wired by #7010 must therefore treat late-response classification as
//! best-effort evidence over the last `MAX_RECENT_TERMINALS` completions, not
//! as a durable per-connection guarantee. The allocator in
//! [`ServerRequestRegistry::register`] is not exposed to the reuse hazard in
//! practice, because it advances monotonically through the id space before
//! wrapping.
//!
//! Caller-supplied string ids are bounded on admission rather than truncated:
//! truncating would break identity matching against the client's response, so
//! an id longer than `MAX_SERVER_REQUEST_ID_BYTES` is rejected with
//! [`ServerRequestRegistryError::IdTooLong`].

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
/// Upper bound for a caller-supplied string request id.
///
/// The key is retained in `pending` and then copied into `recent_terminals` for
/// up to `MAX_RECENT_TERMINALS` entries, so an unbounded id would hold memory
/// well past the request lifetime. Truncating is not an option: the id must
/// match the client's response byte-for-byte, so oversized ids are rejected.
const MAX_SERVER_REQUEST_ID_BYTES: usize = 128;
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
#[derive(Debug)]
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
    pub(crate) id_too_long_rejected: u64,
    pub(crate) unsupported_id_rejected: u64,
    pub(crate) id_space_exhausted: u64,
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
    IdTooLong { bytes: usize, maximum: usize },
    UnsupportedId { id: String },
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
            Self::IdTooLong { bytes, maximum } => {
                write!(
                    f,
                    "server-request string id of {bytes} bytes exceeds the {maximum}-byte bound"
                )
            }
            Self::UnsupportedId { id } => {
                write!(f, "server-request id {id} is not representable by this registry")
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
    /// Map a JSON-RPC id onto a registry key.
    ///
    /// `JsonRpcId` is `#[non_exhaustive]` in `perl-lsp-rs-core`, so a variant
    /// added there is not representable as a `ServerRequestKey`. Returning
    /// `None` keeps that boundary honest: the registry refuses to own an
    /// identity it cannot compare, rather than collapsing every unrepresentable
    /// id onto one shared key where distinct requests would alias each other.
    fn from_json_rpc_id(id: &JsonRpcId) -> Option<Self> {
        match id {
            JsonRpcId::Integer(value) => Some(Self::Integer(*value)),
            JsonRpcId::String(value) => Some(Self::String(value.clone())),
            _ => None,
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
        let Some(key) = ServerRequestKey::from_json_rpc_id(&id) else {
            state.counters.unsupported_id_rejected =
                state.counters.unsupported_id_rejected.saturating_add(1);
            return Err(ServerRequestRegistryError::UnsupportedId {
                id: bounded_text(&format!("{id:?}"), MAX_DIAGNOSTIC_BYTES),
            });
        };
        if let ServerRequestKey::String(value) = &key
            && value.len() > MAX_SERVER_REQUEST_ID_BYTES
        {
            let bytes = value.len();
            state.counters.id_too_long_rejected =
                state.counters.id_too_long_rejected.saturating_add(1);
            return Err(ServerRequestRegistryError::IdTooLong {
                bytes,
                maximum: MAX_SERVER_REQUEST_ID_BYTES,
            });
        }
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
        let now = Instant::now();
        let detail = || format!("success result {}", bounded_value_summary(&result));
        let Some(key) = ServerRequestKey::from_json_rpc_id(id) else {
            return self.record_unsupported_response(id, detail(), now);
        };
        let delivery = {
            let mut state = self.inner.state.lock();
            let Some(pending) = state.pending.remove(&key) else {
                let detail = detail();
                return record_missing_response(&mut state, &key, detail, now);
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
        let now = Instant::now();
        let detail = || {
            format!(
                "client error code={} message={}",
                error.code,
                bounded_text(&error.message, MAX_DIAGNOSTIC_BYTES)
            )
        };
        let Some(key) = ServerRequestKey::from_json_rpc_id(id) else {
            return self.record_unsupported_response(id, detail(), now);
        };
        let delivery = {
            let mut state = self.inner.state.lock();
            let Some(pending) = state.pending.remove(&key) else {
                let detail = detail();
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
        let now = Instant::now();
        let Some(key) = ServerRequestKey::from_json_rpc_id(id) else {
            return self.record_unsupported_response(
                id,
                format!("terminal outcome {:?}", outcome.kind()),
                now,
            );
        };
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

    /// Hand one terminal completion to its owning caller.
    ///
    /// The completion moves into the channel rather than being cloned: a
    /// successful `SuccessValue` carries an arbitrarily large client result,
    /// and cloning it on every delivery would pay that cost on the hot path
    /// purely to keep a copy for the rare dropped-receiver case. `SendError`
    /// already returns the unsent value, so the diagnostic path recovers it.
    fn deliver(&self, delivery: Delivery) {
        let Delivery { sender, completion } = delivery;
        let Err(mpsc::SendError(completion)) = sender.send(completion) else {
            return;
        };

        let mut state = self.inner.state.lock();
        state.counters.completion_receiver_dropped =
            state.counters.completion_receiver_dropped.saturating_add(1);
        push_anomaly(
            &mut state,
            ServerRequestAnomalyKind::CompletionReceiverDropped,
            ServerRequestKey::from_json_rpc_id(&completion.id),
            Some(completion.method),
            "completion receiver was dropped before terminal delivery".to_string(),
            completion.completed_at,
        );
    }

    /// Record a response whose id `perl-lsp-rs-core` can express but this
    /// registry cannot key on.
    ///
    /// Registration refuses such ids, so no pending entry can exist for one.
    /// The response is therefore genuinely unknown rather than late.
    fn record_unsupported_response(
        &self,
        id: &JsonRpcId,
        detail: String,
        observed_at: Instant,
    ) -> ServerRequestCompletionDisposition {
        let mut state = self.inner.state.lock();
        state.counters.unknown_response = state.counters.unknown_response.saturating_add(1);
        push_anomaly(
            &mut state,
            ServerRequestAnomalyKind::UnknownResponse,
            None,
            None,
            format!("unsupported id {}; {detail}", bounded_text(&format!("{id:?}"), 96)),
            observed_at,
        );
        ServerRequestCompletionDisposition::Unknown
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

/// Reserve the next free numeric id.
///
/// `reject_if_full` has already returned `Ok`, so at most `capacity - 1` integer
/// ids are pending and at most `MAX_RECENT_TERMINALS` are retained — together at
/// most `capacity + MAX_RECENT_TERMINALS - 1` occupied ids. The scan below walks
/// `capacity + MAX_RECENT_TERMINALS + 1` *distinct* consecutive candidates, so a
/// free id always exists and the `IdSpaceExhausted` tail is a defensive floor
/// rather than a live path. `scan_width_exceeds_occupiable_id_count` pins that
/// margin so a future change to either bound cannot silently make reservation
/// fail. The tail keeps its own counter regardless, so if it ever does fire it
/// is not misreported as an admission-capacity rejection.
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

    state.counters.id_space_exhausted = state.counters.id_space_exhausted.saturating_add(1);
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

        assert_eq!(
            registry.pending_count(),
            2,
            "integer 1 and string \"1\" must occupy distinct pending slots"
        );
        assert_eq!(
            registry.complete_success(&JsonRpcId::Integer(1), json!({"kind": "number"})),
            ServerRequestCompletionDisposition::Completed,
            "integer id 1 must complete the numeric registration"
        );
        assert_eq!(
            registry
                .complete_success(&JsonRpcId::String("1".to_string()), json!({"kind": "string"})),
            ServerRequestCompletionDisposition::Completed,
            "string id \"1\" must complete the string registration"
        );
        assert_eq!(
            receive(numeric)?.method,
            "numeric",
            "the integer completion must carry the numeric registration's method"
        );
        assert_eq!(
            receive(string)?.method,
            "string",
            "the string completion must carry the string registration's method"
        );
        assert_eq!(registry.pending_count(), 0, "both registrations must be terminally removed");
        Ok(())
    }

    #[test]
    fn oversized_string_id_is_rejected_without_truncating_identity() -> TestResult {
        let registry = ServerRequestRegistry::new(4)?;
        let oversized = "x".repeat(MAX_SERVER_REQUEST_ID_BYTES + 1);
        let rejected = registry.register_with_id(
            JsonRpcId::String(oversized.clone()),
            "window/showDocument",
            "document:oversized",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Object,
        );
        assert!(
            matches!(
                rejected,
                Err(ServerRequestRegistryError::IdTooLong {
                    bytes,
                    maximum: MAX_SERVER_REQUEST_ID_BYTES,
                }) if bytes == MAX_SERVER_REQUEST_ID_BYTES + 1
            ),
            "an id one byte over the bound must be rejected as IdTooLong, got {rejected:?}"
        );

        let counters = registry.counters();
        assert_eq!(
            counters.id_too_long_rejected, 1,
            "an oversized id must be counted separately from capacity rejection"
        );
        assert_eq!(
            counters.capacity_rejected, 0,
            "an oversized id must not be charged to capacity_rejected"
        );
        assert_eq!(counters.pending, 0, "a rejected id must leave no pending state behind");

        let accepted = registry.register_with_id(
            JsonRpcId::String("y".repeat(MAX_SERVER_REQUEST_ID_BYTES)),
            "window/showDocument",
            "document:at-bound",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Object,
        )?;
        assert_eq!(
            accepted.id,
            JsonRpcId::String("y".repeat(MAX_SERVER_REQUEST_ID_BYTES)),
            "an id exactly at the bound must be admitted with its identity intact"
        );
        assert_eq!(
            registry.cancel(&accepted.id),
            ServerRequestCompletionDisposition::Completed,
            "the admitted at-bound id must still match for terminal completion"
        );
        let _ = receive(accepted)?;
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
        assert!(
            matches!(error, Err(ServerRequestRegistryError::CapacityExhausted { capacity: 1 })),
            "the second registration must be rejected at capacity 1, got {error:?}"
        );
        assert_eq!(
            registry.pending_count(),
            1,
            "a rejected reservation must not add pending state"
        );
        assert_eq!(
            registry.counters().capacity_rejected,
            1,
            "capacity rejection must be counted exactly once"
        );
        assert_eq!(
            registry.cancel(&first.id),
            ServerRequestCompletionDisposition::Completed,
            "the admitted registration must still be cancellable"
        );
        let outcome = receive(first)?.outcome;
        assert!(
            matches!(outcome, ServerRequestTerminalOutcome::Cancelled),
            "cancel must deliver a Cancelled terminal outcome, got {outcome:?}"
        );
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
            ServerRequestCompletionDisposition::Completed,
            "a shape-invalid result must still terminally complete the request"
        );
        let completion = receive(registration)?;
        let ServerRequestTerminalOutcome::MalformedResult(malformed) = completion.outcome else {
            return Err(io::Error::other("expected malformed terminal outcome").into());
        };
        assert_eq!(malformed.expected, "null", "the decoder's expected shape must be reported");
        assert_eq!(malformed.observed, "boolean", "the observed shape must be reported");
        assert_eq!(
            registry.counters().malformed_result,
            1,
            "a malformed result must be counted as malformed, not as success"
        );
        assert_eq!(registry.pending_count(), 0, "a malformed result must remove the pending entry");
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
        assert_eq!(
            registry.expire_deadlines(Instant::now() + Duration::from_secs(1)),
            1,
            "the one past-deadline request must expire exactly once"
        );
        let outcome = receive(registration)?.outcome;
        assert!(
            matches!(outcome, ServerRequestTerminalOutcome::TimedOut),
            "expiry must deliver a TimedOut terminal outcome, got {outcome:?}"
        );
        assert_eq!(
            registry.complete_success(&JsonRpcId::Integer(1), json!([])),
            ServerRequestCompletionDisposition::AlreadyTerminal,
            "a response arriving after expiry must be classified as already terminal"
        );
        let counters = registry.counters();
        assert_eq!(counters.timed_out, 1, "timeout must be counted exactly once");
        assert_eq!(
            counters.late_or_duplicate_response, 1,
            "the late response must be counted as late, not unknown"
        );
        assert_eq!(counters.pending, 0, "the expired request must not remain pending");
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
        assert_eq!(
            selected, 1,
            "exactly one of the racing response and expiry may claim the request \
             (response disposition {response_disposition:?}, expired {expired})"
        );
        let completion = receive(registration)?;
        assert!(
            matches!(
                completion.outcome,
                ServerRequestTerminalOutcome::SuccessValue(_)
                    | ServerRequestTerminalOutcome::TimedOut
            ),
            "the single terminal outcome must be the response or the timeout, got {:?}",
            completion.outcome
        );
        assert_eq!(
            registry.counters().completed_total,
            1,
            "a raced request must be counted as completed exactly once"
        );
        assert_eq!(registry.pending_count(), 0, "the raced request must not remain pending");
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
        assert_eq!(registry.shutdown(), 2, "shutdown must drain both pending requests");
        let first_outcome = receive(first)?.outcome;
        assert!(
            matches!(first_outcome, ServerRequestTerminalOutcome::Shutdown),
            "the first request must observe Shutdown, got {first_outcome:?}"
        );
        let second_outcome = receive(second)?.outcome;
        assert!(
            matches!(second_outcome, ServerRequestTerminalOutcome::Shutdown),
            "the second request must observe Shutdown, got {second_outcome:?}"
        );
        assert_eq!(registry.counters().shutdown, 2, "both drains must be counted as shutdown");
        assert_eq!(registry.pending_count(), 0, "shutdown must leave no pending state");
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
        assert_eq!(
            maximum.id,
            JsonRpcId::Integer(MAX_SERVER_REQUEST_ID),
            "the allocator must issue the maximum id before wrapping"
        );
        assert_eq!(
            two.id,
            JsonRpcId::Integer(2),
            "after wrapping, the allocator must skip the explicitly held id 1"
        );
        assert_eq!(
            registry.cancel(&maximum.id),
            ServerRequestCompletionDisposition::Completed,
            "the wrapped maximum id must remain addressable"
        );
        assert_eq!(
            registry.cancel(&one.id),
            ServerRequestCompletionDisposition::Completed,
            "the explicitly registered id must remain addressable"
        );
        assert_eq!(
            registry.cancel(&two.id),
            ServerRequestCompletionDisposition::Completed,
            "the post-wrap allocated id must remain addressable"
        );
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
            ServerRequestCompletionDisposition::Completed,
            "the registration must reach a terminal outcome before reuse is attempted"
        );
        let _ = receive(registration)?;
        let reuse = registry.register_with_id(
            JsonRpcId::String("stable-id".to_string()),
            "window/showDocument",
            "document:2",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Object,
        );
        assert!(
            matches!(reuse, Err(ServerRequestRegistryError::IdUnavailable { .. })),
            "a recently terminal id must be refused for reuse, got {reuse:?}"
        );
        Ok(())
    }

    #[test]
    fn terminal_id_reuse_protection_is_bounded_by_recent_terminal_count() -> TestResult {
        let registry = ServerRequestRegistry::new(2)?;
        let held = registry.register_with_id(
            JsonRpcId::String("evictable".to_string()),
            "window/showDocument",
            "document:evictable",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Object,
        )?;
        assert_eq!(
            registry.cancel(&held.id),
            ServerRequestCompletionDisposition::Completed,
            "the tracked id must first become recently terminal"
        );
        let _ = receive(held)?;

        // Push the tracked id out of the bounded recent-terminal ring.
        for index in 0..MAX_RECENT_TERMINALS {
            let filler = registry.register(
                "window/showDocument",
                "document:filler",
                Duration::from_secs(5),
                ServerRequestResultDecoder::Object,
            )?;
            assert_eq!(
                registry.cancel(&filler.id),
                ServerRequestCompletionDisposition::Completed,
                "filler registration {index} must complete so it occupies a ring slot"
            );
            let _ = receive(filler)?;
        }

        let reuse = registry.register_with_id(
            JsonRpcId::String("evictable".to_string()),
            "window/showDocument",
            "document:reused",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Object,
        );
        assert!(
            reuse.is_ok(),
            "reuse protection is count-bounded by MAX_RECENT_TERMINALS ({MAX_RECENT_TERMINALS}); \
             once evicted the id must be admissible again, got {:?}",
            reuse.as_ref().err()
        );
        let reused = reuse?;
        assert_eq!(
            registry.cancel(&reused.id),
            ServerRequestCompletionDisposition::Completed,
            "the re-admitted id must be addressable"
        );
        let _ = receive(reused)?;
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
            ServerRequestCompletionDisposition::Completed,
            "a dropped receiver must not change the request's terminal disposition"
        );
        let counters = registry.counters();
        assert_eq!(
            counters.completion_receiver_dropped, 1,
            "the undeliverable completion must be counted exactly once"
        );
        assert_eq!(counters.pending, 0, "a dropped receiver must not retain pending state");
        let anomalies = registry.anomalies();
        assert!(
            anomalies
                .iter()
                .any(|anomaly| anomaly.kind == ServerRequestAnomalyKind::CompletionReceiverDropped),
            "a CompletionReceiverDropped anomaly must be recorded, observed {:?}",
            anomalies.iter().map(|anomaly| anomaly.kind).collect::<Vec<_>>()
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
            assert_eq!(
                disposition,
                ServerRequestCompletionDisposition::Unknown,
                "response {id} was never registered and must be classified as unknown"
            );
        }
        assert_eq!(
            registry.anomalies().len(),
            MAX_ANOMALIES,
            "the anomaly buffer must saturate at MAX_ANOMALIES rather than grow"
        );
        assert_eq!(
            registry.counters().anomalies_dropped,
            10,
            "every anomaly evicted past the bound must be counted"
        );
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
        assert_eq!(snapshots.len(), 1, "exactly one request is pending, got {snapshots:?}");
        assert_eq!(snapshots[0].id, registration.id, "the snapshot must carry the reserved id");
        assert_eq!(
            snapshots[0].method, "workspace/applyEdit",
            "the snapshot must carry the registered method"
        );
        assert_eq!(
            snapshots[0].debug_identity, "rename-operation:42",
            "the snapshot must carry the caller's debug identity"
        );
        assert!(
            snapshots[0].deadline >= snapshots[0].created_at,
            "the deadline must not precede creation (created_at {:?}, deadline {:?})",
            snapshots[0].created_at,
            snapshots[0].deadline
        );
        assert_eq!(
            snapshots[0].decoder,
            ServerRequestResultDecoder::Object,
            "the snapshot must carry the registered result decoder"
        );
        assert_eq!(registry.transport_lost(), 1, "transport loss must drain the pending request");
        let outcome = receive(registration)?.outcome;
        assert!(
            matches!(outcome, ServerRequestTerminalOutcome::TransportLost),
            "transport loss must deliver a TransportLost terminal outcome, got {outcome:?}"
        );
        Ok(())
    }

    #[test]
    fn capacity_rejection_is_not_charged_to_id_space_exhaustion() -> TestResult {
        let registry = ServerRequestRegistry::new(1)?;
        let held = registry.register(
            "held",
            "held",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Null,
        )?;
        let rejected = registry.register(
            "rejected",
            "rejected",
            Duration::from_secs(5),
            ServerRequestResultDecoder::Null,
        );
        assert!(
            matches!(rejected, Err(ServerRequestRegistryError::CapacityExhausted { capacity: 1 })),
            "a full registry must reject on admission capacity, got {rejected:?}"
        );

        let counters = registry.counters();
        assert_eq!(counters.capacity_rejected, 1, "capacity rejection must be counted");
        assert_eq!(
            counters.id_space_exhausted, 0,
            "capacity rejection is a distinct condition and must not be charged to \
             id_space_exhausted"
        );

        assert_eq!(
            registry.cancel(&held.id),
            ServerRequestCompletionDisposition::Completed,
            "the admitted request must remain addressable"
        );
        let _ = receive(held)?;
        Ok(())
    }

    /// `IdSpaceExhausted` is a defensive floor, not a reachable path.
    ///
    /// `allocate_numeric_id` runs only after `reject_if_full` returned `Ok`, so
    /// at most `capacity - 1` integer ids are pending and at most
    /// `MAX_RECENT_TERMINALS` are retained. This pins the scan wide enough to
    /// always clear that occupied set, which is why no test drives the tail: a
    /// test that claimed to reach it would be asserting an unreachable state.
    #[test]
    fn scan_width_exceeds_occupiable_id_count() {
        for capacity in [1_usize, 2, DEFAULT_SERVER_REQUEST_CAPACITY, MAX_SERVER_REQUEST_CAPACITY] {
            let attempts = capacity + MAX_RECENT_TERMINALS + 1;
            let occupiable = (capacity - 1) + MAX_RECENT_TERMINALS;
            assert!(
                attempts > occupiable,
                "at capacity {capacity} the allocator scans {attempts} distinct ids but at most \
                 {occupiable} can be occupied, so a free id must always exist"
            );
            assert!(
                attempts <= usize::try_from(MAX_SERVER_REQUEST_ID).unwrap_or(usize::MAX),
                "at capacity {capacity} the {attempts}-candidate scan must fit inside the \
                 1..={MAX_SERVER_REQUEST_ID} id space for the candidates to stay distinct"
            );
        }
    }
}
