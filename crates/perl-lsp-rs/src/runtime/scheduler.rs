//! Request classification, prioritization, and scheduling for concurrent dispatch.
//!
//! Classifies incoming LSP methods into scheduling categories that determine
//! how they are executed:
//!
//! - **Control**: Processed inline immediately (cancellation, progress cancel)
//! - **Lifecycle**: Exclusive access (initialize, shutdown, exit)
//! - **Mutation**: Exclusive access (didOpen, didChange, didClose, etc.)
//! - **ReadOnly**: Concurrent access (hover, completion, definition, etc.)
//!
//! # Request Prioritization
//!
//! Read-only requests are processed by priority rather than strict FIFO.
//! The priority ordering (highest to lowest) is:
//!
//! 1. `Hover` — cursor queries are latency-sensitive
//! 2. `Completion` — inline suggestions block the user
//! 3. `References` — navigation is slightly less urgent
//! 4. `Other` — background/bulk operations (workspace symbols, diagnostics, etc.)
//!
//! # Stale Request Cancellation
//!
//! Position-sensitive reads are prevented from delivering stale results in two
//! ways:
//!
//! 1. **Position dedupe**: if a newer request arrives for the same
//!    `(method, uri, line, character)` key, the older request is cancelled.
//!    Useful for the "same cursor position, repeated query" case.
//!
//! 2. **Generation freshness** (PR 4 of the 0.15.1 Neovim latency lane):
//!    at ingress every position-sensitive read captures the document's
//!    current generation. The dispatcher compares that snapshot to the
//!    document's current generation before execution and again before
//!    delivering the handler response. If the document has moved on (i.e. a
//!    `didChange` bumped the generation while the read was queued or while a
//!    slow handler was running), the read is cancelled. This is the case that
//!    position dedupe misses — normal typing moves the cursor and changes
//!    the position key on every keystroke, so the dedup map sees only
//!    unique entries.
//!
//! The [`Scheduler`] struct manages dedicated worker queues so the ingress loop
//! never performs heavy work — it only classifies and enqueues.

use crate::protocol::{
    INTERNAL_ERROR, JsonRpcError, JsonRpcId, JsonRpcRequest, JsonRpcResponse, REQUEST_CANCELLED,
};
use crate::transport::log_response;
use std::cmp::Ordering as CmpOrdering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinSet;

use super::{LspServer, outbound::OutboundSender};

// =========================================================================
// Request priority
// =========================================================================

/// Priority tier for read-only LSP requests.
///
/// Requests are dispatched highest-priority-first within the read queue.
/// Numeric value: lower = higher priority (so `BinaryHeap` max-heap works
/// correctly after inversion in `Ord`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestPriority {
    /// textDocument/hover — cursor queries, most latency-sensitive.
    Hover = 0,
    /// textDocument/completion — inline suggestions.
    Completion = 1,
    /// textDocument/references, definition, declaration.
    References = 2,
    /// Everything else (workspace/symbol, diagnostics, bulk operations).
    Other = 3,
}

impl RequestPriority {
    /// Numeric priority value; lower value = higher priority.
    fn value(self) -> u8 {
        self as u8
    }
}

/// Map an LSP method string to its dispatch priority.
pub(crate) fn request_priority(method: &str) -> RequestPriority {
    match method {
        "textDocument/hover" => RequestPriority::Hover,
        "textDocument/completion" | "completionItem/resolve" | "textDocument/inlineCompletion" => {
            RequestPriority::Completion
        }
        "textDocument/references"
        | "textDocument/definition"
        | "textDocument/declaration"
        | "textDocument/typeDefinition"
        | "textDocument/implementation" => RequestPriority::References,
        _ => RequestPriority::Other,
    }
}

// =========================================================================
// Dedup key for stale-request cancellation
// =========================================================================

/// A dedup key identifies a position-sensitive request by `(method, uri, line, character)`.
///
/// When a newer request arrives with the same key, the earlier pending request
/// is superseded and its slot is cancelled before execution begins.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RequestDedupKey {
    /// Normalised LSP method (e.g. `"textDocument/hover"`).
    pub method: String,
    /// Document URI (e.g. `"file:///path/to/file.pl"`).
    pub uri: String,
    /// Zero-based line number.
    pub line: u64,
    /// Zero-based character offset.
    pub character: u64,
}

/// Extract a dedup key from a position-sensitive request, if possible.
///
/// Returns `None` for requests without a `textDocument` + `position` payload,
/// or when priority is `Other` (those are not deduplicated).
pub(crate) fn extract_dedup_key(
    method: &str,
    params: Option<&serde_json::Value>,
    priority: RequestPriority,
) -> Option<RequestDedupKey> {
    // Only deduplicate position-sensitive requests.
    if priority == RequestPriority::Other {
        return None;
    }

    let params = params?;
    let uri = params.get("textDocument")?.get("uri")?.as_str()?.to_string();
    let position = params.get("position")?;
    let line = position.get("line")?.as_u64()?;
    let character = position.get("character")?.as_u64()?;

    Some(RequestDedupKey { method: method.to_string(), uri, line, character })
}

// =========================================================================
// Generation-aware freshness for stale-read cancellation
// =========================================================================

/// Document freshness snapshot captured at request ingress.
///
/// Used by the scheduler to cancel position-sensitive reads whose
/// document has moved on between ingress and dispatch. Without this,
/// every keystroke during a typing storm produces a unique
/// `(uri, line, character)` dedup key, so position dedupe alone cannot
/// collapse the storm into "latest request wins."
#[derive(Debug, Clone)]
pub(crate) struct ReadFreshness {
    /// Document URI captured from the request.
    pub uri: String,
    /// Generation counter as observed at ingress. `None` when the
    /// document was not yet open at ingress (e.g. a hover arriving before
    /// the matching `didOpen`); in that case freshness is not enforced.
    pub document_generation: Option<u32>,
    /// Generation counter identity captured at ingress. A close/reopen can
    /// reuse the numeric generation, so the allocation identity is part of
    /// the freshness contract.
    pub document_instance: Option<Arc<std::sync::atomic::AtomicU32>>,
    /// LSP document version as observed at ingress. Retained for
    /// diagnostics / future use; the cancellation decision uses
    /// `document_generation`.
    #[allow(dead_code)]
    pub document_version: Option<i32>,
}

/// Extract a freshness snapshot for a position-sensitive request, querying
/// the live document map.
///
/// Mirrors the gating of [`extract_dedup_key`] — non-position requests get
/// `None`. The returned `document_generation` / `document_version` are
/// taken from the live document state at the moment of the call; the
/// dispatcher later compares those snapshots against the then-current
/// document generation to decide if the read is stale.
pub(crate) fn extract_freshness(
    server: &super::LspServer,
    method: &str,
    params: Option<&serde_json::Value>,
    priority: RequestPriority,
) -> Option<ReadFreshness> {
    let _ = method;
    if priority == RequestPriority::Other {
        return None;
    }
    let params = params?;
    let uri = params.get("textDocument")?.get("uri")?.as_str()?.to_string();
    let (document_generation, document_version, document_instance) = server
        .document_freshness(&uri)
        .map_or((None, None, None), |(generation, version, instance)| {
            (Some(generation), Some(version), Some(instance))
        });
    Some(ReadFreshness { uri, document_generation, document_instance, document_version })
}

/// Decide whether a queued read is stale given the document's current
/// generation. Returns `Some((captured, current))` when the read should
/// be cancelled.
///
/// Stale rule: a freshness snapshot is stale when both
///
/// - the document was open at ingress (snapshot generation is `Some`), and
/// - the document is still open (current generation is `Some`), and
/// - the current generation is strictly greater than the snapshot.
///
/// Notes:
///
/// - A document that was closed between ingress and dispatch is *not*
///   reported as stale here; the provider will surface the missing-document
///   error itself.
/// - A snapshot whose `document_generation` is `None` is treated as a
///   non-tracked read (e.g. hover sent before `didOpen`) and is allowed to
///   run.
pub(crate) fn is_read_stale(
    freshness: &ReadFreshness,
    current_generation: Option<u32>,
) -> Option<(u32, u32)> {
    let captured = freshness.document_generation?;
    let current = current_generation?;
    if current > captured { Some((captured, current)) } else { None }
}

// =========================================================================
// Scheduling class
// =========================================================================

/// Scheduling class for an incoming LSP method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestClass {
    /// `$/cancelRequest`, `window/workDoneProgress/cancel`
    /// — processed inline, immediately, no lock.
    Control,
    /// `initialize`, `initialized`, `shutdown`, `exit`
    /// — exclusive, ordered.
    Lifecycle,
    /// `didOpen`, `didChange`, `didClose`, `didSave`, etc.
    /// — exclusive (document mutations).
    Mutation,
    /// `completion`, `hover`, `definition`, `references`, etc.
    /// — concurrent (read-only queries).
    ReadOnly,
}

/// Classify an LSP method string into its scheduling category.
pub(crate) fn classify(method: &str) -> RequestClass {
    match method {
        // Control: processed inline
        "$/cancelRequest" | "window/workDoneProgress/cancel" => RequestClass::Control,

        // Lifecycle: exclusive, ordered
        "initialize" | "initialized" | "shutdown" | "exit" | "$/setTrace" => {
            RequestClass::Lifecycle
        }

        // Mutation: exclusive (modifies document state)
        "textDocument/didOpen"
        | "textDocument/didChange"
        | "textDocument/didClose"
        | "textDocument/didSave"
        | "textDocument/willSave"
        | "textDocument/willSaveWaitUntil"
        | "notebookDocument/didOpen"
        | "notebookDocument/didChange"
        | "notebookDocument/didSave"
        | "notebookDocument/didClose"
        | "workspace/didChangeWatchedFiles"
        | "workspace/didChangeWorkspaceFolders"
        | "workspace/didChangeConfiguration"
        | "workspace/didRenameFiles"
        | "workspace/didDeleteFiles"
        | "workspace/didCreateFiles" => RequestClass::Mutation,

        // Everything else is read-only
        _ => RequestClass::ReadOnly,
    }
}

// =========================================================================
// Scheduler
// =========================================================================

/// Worker-queue scheduler for concurrent LSP dispatch.
///
/// Routes classified requests to dedicated worker queues:
///
/// - **Mutation worker**: Single exclusive worker processes lifecycle and mutation
///   requests one at a time (sequential drain from a bounded `mpsc` channel).
/// - **Read dispatcher**: A single dispatcher drains the read queue and launches
///   read-only work onto the blocking pool, capped by a semaphore. This avoids
///   receiver-lock contention while still bounding concurrency.
///
/// Read-only requests are ordered by priority (hover > completion > references > other)
/// and stale position-sensitive requests are cancelled before execution.
///
/// The ingress loop (`serve_async`) only reads, classifies, and enqueues.
/// Heavy work never blocks the message reader.
///
/// ## Shutdown policy
///
/// When the ingress channel closes (EOF / drop), `shutdown()` drops the sender
/// halves. Workers drain remaining items and exit. `spawn_blocking` tasks cannot
/// be aborted — they run to completion. This is cooperative shutdown.
pub(crate) struct Scheduler {
    /// Channel for mutation/lifecycle work (single exclusive worker drains this).
    mutation_tx: tokio::sync::mpsc::Sender<QueuedMutation>,
    /// Channel for read-only work (dispatcher drains this).
    read_tx: tokio::sync::mpsc::Sender<QueuedRead>,
    /// Join handles for background workers (used for shutdown drain).
    workers: Vec<tokio::task::JoinHandle<()>>,
    /// Monotonic sequence assigned to mutations/lifecycle requests at ingress.
    mutation_seq_next: Arc<AtomicU64>,
    /// Highest mutation sequence that has completed processing.
    mutation_seq_done: Arc<AtomicU64>,
    /// Wakes read workers waiting for earlier mutations to finish.
    mutation_notify: Arc<Notify>,
    /// Server reference retained at the scheduler level so ingress paths
    /// (`send_read`) can snapshot document generation without waiting for a
    /// worker. Workers receive their own `Arc` clones via the spawn closures.
    server: Arc<LspServer>,
}

/// Bounded channel capacity for both mutation and read queues.
const QUEUE_CAPACITY: usize = 64;

/// Number of concurrent read-pool workers.
const READ_WORKERS: usize = 4;

/// Mutation/lifecycle request tagged with its ingress-order sequence number.
struct QueuedMutation {
    request: JsonRpcRequest,
    seq: u64,
    /// Wall-clock instant the request was enqueued, used only to measure
    /// `scheduler.mutation_wait` (queue latency) when `PERL_LSP_TIMING` is on.
    enqueued: std::time::Instant,
}

/// Read-only request with priority metadata for ordered dispatch.
///
/// Implements `Ord` so that a `BinaryHeap<QueuedRead>` is a max-heap ordered
/// first by **highest priority** (lowest `priority.value()`), then by **latest
/// arrival** (highest `arrival_seq`) to break ties in favour of newer requests.
struct QueuedRead {
    /// The original JSON-RPC request.
    request: JsonRpcRequest,
    /// Latest mutation sequence observed at ingress; used to gate execution.
    wait_for_seq: u64,
    /// Dispatch priority computed from the request method.
    priority: RequestPriority,
    /// Monotonic ingress counter (used for tie-breaking and stale detection).
    arrival_seq: u64,
    /// Dedup key for stale-request cancellation (None for non-position requests).
    dedup_key: Option<RequestDedupKey>,
    /// Document freshness snapshot for generation-based stale cancellation.
    /// `None` for non-position requests; otherwise carries the document
    /// generation observed at ingress so the dispatcher can detect that
    /// the document moved on before this read had a chance to run.
    freshness: Option<ReadFreshness>,
}

impl PartialEq for QueuedRead {
    fn eq(&self, other: &Self) -> bool {
        self.priority.value() == other.priority.value() && self.arrival_seq == other.arrival_seq
    }
}

impl Eq for QueuedRead {}

impl PartialOrd for QueuedRead {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueuedRead {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Higher priority (lower value) first.  On equal priority, prefer
        // the *newer* request (higher arrival_seq).
        match other.priority.value().cmp(&self.priority.value()) {
            CmpOrdering::Equal => self.arrival_seq.cmp(&other.arrival_seq),
            ord => ord,
        }
    }
}

/// Global read arrival counter; incremented at ingress for each read request.
static READ_ARRIVAL_SEQ: AtomicU64 = AtomicU64::new(0);

/// Reason a stale read was cancelled before execution or delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StaleReason {
    /// A newer request with the same `(method, uri, line, character)` arrived.
    PositionSuperseded,
    /// The document generation moved on between ingress and dispatch.
    DocumentGenerationAdvanced { captured: u32, current: u32 },
    /// The document was closed or replaced by a new instance.
    DocumentInstanceChanged,
}

/// What a request handler produced, once a handler panic has been turned into a
/// response the client can actually receive.
///
/// Handlers run on the blocking pool, so a panic surfaces as a `JoinError`
/// instead of unwinding into the scheduler. Discarding that error leaves a
/// request that carries an id with no reply at all, and an LSP client waits for
/// its reply forever — the editor hangs with no error and no recovery short of
/// restarting the server (#5206).
#[derive(Debug)]
enum HandlerOutcome {
    /// The handler returned a response. Subject to the caller's freshness policy.
    Response(JsonRpcResponse),
    /// The handler panicked; this is the synthesized `InternalError`. It is
    /// delivered as-is, because a crash is not a stale result and must never be
    /// suppressed by a freshness check.
    Panicked(JsonRpcResponse),
    /// Nothing to send: a notification, or a panic on a request carrying no id.
    Empty,
}

/// Releases a scheduler-owned request ID even when the handler panics.
struct PendingRequestGuard {
    server: Arc<LspServer>,
    id: Option<JsonRpcId>,
}

impl Drop for PendingRequestGuard {
    fn drop(&mut self) {
        if let Some(id) = self.id.as_ref() {
            self.server.clear_request_pending(id);
        }
    }
}

impl Scheduler {
    /// Create a new scheduler and spawn worker tasks.
    ///
    /// Spawns one exclusive mutation worker and one read dispatcher.
    /// All workers use `spawn_blocking` for CPU-bound handler execution.
    pub fn new(server: Arc<LspServer>) -> Self {
        let (mutation_tx, mutation_rx) = tokio::sync::mpsc::channel(QUEUE_CAPACITY);
        let (read_tx, read_rx) = tokio::sync::mpsc::channel(QUEUE_CAPACITY);
        let mutation_seq_next = Arc::new(AtomicU64::new(0));
        let mutation_seq_done = Arc::new(AtomicU64::new(0));
        let mutation_notify = Arc::new(Notify::new());

        let workers = vec![
            // Single exclusive mutation worker — processes lifecycle and mutation
            // requests one at a time, preserving ordering guarantees.
            tokio::spawn(Self::mutation_worker(
                mutation_rx,
                Arc::clone(&server),
                Arc::clone(&mutation_seq_done),
                Arc::clone(&mutation_notify),
            )),
            // Single dispatcher drains the read queue and fans work out to the
            // blocking pool, capped by a semaphore instead of a receiver mutex.
            tokio::spawn(Self::read_dispatcher(
                read_rx,
                Arc::clone(&server),
                Arc::clone(&mutation_seq_done),
                Arc::clone(&mutation_notify),
            )),
        ];

        // Install diagnostic debouncer now that server is wrapped in Arc.
        // Use the runtime-configured interval so e2e mode (debounce=0) and
        // user-tuned values from the CLI/env take effect.
        let debounce_server = Arc::clone(&server);
        let debounce_interval = server.runtime_tuning().diagnostic_debounce();
        let debouncer = super::diagnostic_debounce::DiagnosticDebouncer::with_interval(
            debounce_interval,
            move |uri| {
                debounce_server.publish_diagnostics(uri);
            },
        );
        server.install_diagnostic_debouncer(debouncer);

        // Install file watcher debouncer now that server is wrapped in Arc.
        let fw_server = Arc::clone(&server);
        let fw_debouncer = super::file_watcher_debounce::FileWatcherDebouncer::new(move |uris| {
            fw_server.handle_watched_file_batch(uris);
        });
        server.install_file_watcher_debouncer(fw_debouncer);

        // Install the off-lock async parse worker (#3396 Phase 3) now that
        // server is wrapped in Arc. This is the production default: real
        // editor `didChange` traffic goes through this `Scheduler`, so the
        // mutation worker below only ever text-applies -- it never parses.
        server.install_default_parse_worker();

        Self {
            mutation_tx,
            read_tx,
            workers,
            mutation_seq_next,
            mutation_seq_done,
            mutation_notify,
            server,
        }
    }

    /// Send a mutation or lifecycle request to the exclusive worker.
    ///
    /// Returns `Err(())` if the mutation worker has exited (channel closed).
    pub async fn send_mutation(&self, request: JsonRpcRequest) -> Result<(), ()> {
        let pending_id = request.id.clone();
        if let Some(id) = pending_id.as_ref() {
            self.server.mark_request_pending(id);
        }
        let seq = self.mutation_seq_next.fetch_add(1, Ordering::SeqCst) + 1;
        let enqueued = std::time::Instant::now();
        let result = self.mutation_tx.send(QueuedMutation { request, seq, enqueued }).await;
        if result.is_err()
            && let Some(id) = pending_id.as_ref()
        {
            self.server.clear_request_pending(id);
        }
        result.map_err(|_| {
            self.mutation_seq_done.store(seq, Ordering::SeqCst);
            self.mutation_notify.notify_waiters();
        })
    }

    /// Send a read-only request to the priority read pool.
    ///
    /// Computes the request priority and dedup key at ingress so the dispatcher
    /// can reorder and deduplicate without parsing params again.
    ///
    /// Returns `Err(())` if all read workers have exited (channel closed).
    pub async fn send_read(&self, request: JsonRpcRequest) -> Result<(), ()> {
        let pending_id = request.id.clone();
        if let Some(id) = pending_id.as_ref() {
            self.server.mark_request_pending(id);
        }
        let wait_for_seq = self.mutation_seq_next.load(Ordering::SeqCst);
        let priority = request_priority(&request.method);
        let dedup_key = extract_dedup_key(&request.method, request.params.as_ref(), priority);
        let freshness =
            extract_freshness(&self.server, &request.method, request.params.as_ref(), priority);
        let arrival_seq = READ_ARRIVAL_SEQ.fetch_add(1, Ordering::Relaxed);
        let result = self
            .read_tx
            .send(QueuedRead { request, wait_for_seq, priority, arrival_seq, dedup_key, freshness })
            .await;
        if result.is_err()
            && let Some(id) = pending_id.as_ref()
        {
            self.server.clear_request_pending(id);
        }
        result.map_err(|_| ())
    }

    /// Shut down all workers by dropping senders and awaiting completion.
    ///
    /// Dropping the sender halves closes the channels. Workers drain any
    /// remaining items and exit. `spawn_blocking` tasks run to completion
    /// and cannot be aborted — this is cooperative shutdown by design.
    pub async fn shutdown(self) {
        // Drop senders so worker recv loops see channel closed.
        drop(self.mutation_tx);
        drop(self.read_tx);

        // Wait for all workers to finish draining.
        for handle in self.workers {
            let _ = handle.await;
        }
    }

    /// Release bookkeeping for mutations abandoned after the outbound channel
    /// closes, including the sequence barrier needed by queued reads.
    fn settle_abandoned_mutation(
        queued: QueuedMutation,
        server: &Arc<LspServer>,
        mutation_seq_done: &Arc<AtomicU64>,
        mutation_notify: &Arc<Notify>,
    ) {
        if let Some(id) = queued.request.id.as_ref() {
            server.clear_request_pending(id);
        }
        mutation_seq_done.store(queued.seq, Ordering::SeqCst);
        mutation_notify.notify_waiters();
    }

    /// Single exclusive mutation worker.
    ///
    /// Drains the mutation channel sequentially, running each handler on the
    /// blocking thread pool via `spawn_blocking`. This ensures lifecycle and
    /// document-mutation requests never overlap.
    async fn mutation_worker(
        mut rx: tokio::sync::mpsc::Receiver<QueuedMutation>,
        server: Arc<LspServer>,
        mutation_seq_done: Arc<AtomicU64>,
        mutation_notify: Arc<Notify>,
    ) {
        while let Some(queued) = rx.recv().await {
            // Phase-1 latency instrumentation (opt-in): queue latency from
            // enqueue to worker pickup. The single exclusive worker serializes
            // mutations, so this is where a queued didChange storm backs up.
            if crate::runtime::timing::is_enabled() {
                crate::runtime::timing::emit(crate::runtime::timing::TimingSpan::labeled(
                    "scheduler.mutation_wait",
                    crate::runtime::timing::elapsed_ms(queued.enqueued),
                    queued.request.method.clone(),
                ));
            }

            // Run on blocking thread: handlers are CPU-bound and use
            // parking_lot locks which must not block the tokio runtime.
            let srv = Arc::clone(&server);
            let id = queued.request.id.clone();
            let method = queued.request.method.clone();
            let seq = queued.seq;
            let pending_guard = PendingRequestGuard { server: Arc::clone(&srv), id: id.clone() };
            let outcome = Self::run_handler(
                move || {
                    let _pending_guard = pending_guard;
                    srv.handle_request(queued.request)
                },
                id,
                &method,
            )
            .await;

            // Reads that were enqueued after this mutation can proceed once state is updated.
            // This must happen even when the handler panicked, or every read
            // waiting on this sequence number would stall behind it.
            mutation_seq_done.store(seq, Ordering::SeqCst);
            mutation_notify.notify_waiters();

            // A panicked mutation still owes its caller a reply, so `Panicked`
            // delivers alongside the normal response path. Matched exhaustively
            // on purpose: dropping an arm here is how #5206 happened, and an
            // exhaustive match makes that a compile error rather than a silent
            // hang.
            let to_send = match outcome {
                HandlerOutcome::Response(response) | HandlerOutcome::Panicked(response) => {
                    Some(response)
                }
                HandlerOutcome::Empty => None,
            };
            if let Some(response) = to_send {
                log_response(&response);
                if server.outbound.send_response(response).is_err() {
                    // The outbound channel is gone, so the worker cannot
                    // deliver responses for requests still buffered in the
                    // mutation queue. Settle their scheduler ownership before
                    // dropping the receiver; otherwise their cancellation
                    // markers stay pinned and reads waiting on their sequence
                    // numbers can remain blocked forever.
                    while let Ok(abandoned) = rx.try_recv() {
                        Self::settle_abandoned_mutation(
                            abandoned,
                            &server,
                            &mutation_seq_done,
                            &mutation_notify,
                        );
                    }
                    break;
                }
            }
        }
    }

    /// Priority read queue dispatcher.
    ///
    /// Collects incoming read requests into a `BinaryHeap` and dispatches them
    /// highest-priority-first. Before dispatching each request, checks for stale
    /// deduplication: if a newer request with the same `(method, uri, position)`
    /// key was enqueued, the older request is cancelled immediately without
    /// running on the blocking pool.
    ///
    /// The semaphore enforces the desired concurrency limit. Each spawned task
    /// waits for preceding mutations to complete before executing.
    async fn read_dispatcher(
        mut rx: tokio::sync::mpsc::Receiver<QueuedRead>,
        server: Arc<LspServer>,
        mutation_seq_done: Arc<AtomicU64>,
        mutation_notify: Arc<Notify>,
    ) {
        let permits = Arc::new(Semaphore::new(READ_WORKERS));
        let mut in_flight = JoinSet::new();

        // Priority queue: highest-priority (lowest value) requests pop first.
        let mut pending: BinaryHeap<QueuedRead> = BinaryHeap::new();
        // Maps dedup key -> latest arrival_seq seen for that key.
        // Capped to prevent unbounded growth over long sessions (#5032 item 1).
        // When the cap is exceeded, the map is cleared — dedup is an optimization,
        // not a correctness requirement, so clearing only causes a brief loss of
        // coalescing for in-flight requests.
        const DEDUP_MAP_CAP: usize = 4096;
        let mut latest_seq: HashMap<RequestDedupKey, u64> = HashMap::new();

        loop {
            // Drain all currently available messages into the priority heap
            // before committing to one, giving priority re-ordering a chance
            // to work even under burst traffic.
            loop {
                match rx.try_recv() {
                    Ok(queued) => {
                        // Track latest arrival_seq for dedup keys.
                        if let Some(ref key) = queued.dedup_key {
                            // Evict the entire map when it exceeds the cap to
                            // prevent unbounded growth (#5032 item 1).
                            if latest_seq.len() >= DEDUP_MAP_CAP {
                                latest_seq.clear();
                            }
                            latest_seq
                                .entry(key.clone())
                                .and_modify(|seq| {
                                    if queued.arrival_seq > *seq {
                                        *seq = queued.arrival_seq;
                                    }
                                })
                                .or_insert(queued.arrival_seq);
                        }
                        pending.push(queued);
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        // Channel closed — drain whatever is left in pending.
                        while let Some(queued) = pending.pop() {
                            Self::dispatch_one(
                                queued,
                                &latest_seq,
                                &permits,
                                &mut in_flight,
                                &server,
                                &mutation_seq_done,
                                &mutation_notify,
                            )
                            .await;
                        }
                        while in_flight.join_next().await.is_some() {}
                        return;
                    }
                }
            }

            if let Some(queued) = pending.pop() {
                Self::dispatch_one(
                    queued,
                    &latest_seq,
                    &permits,
                    &mut in_flight,
                    &server,
                    &mutation_seq_done,
                    &mutation_notify,
                )
                .await;

                while in_flight.len() >= READ_WORKERS {
                    if in_flight.join_next().await.is_none() {
                        break;
                    }
                }
            } else {
                // Heap is empty — block until a new message arrives.
                match rx.recv().await {
                    Some(queued) => {
                        if let Some(ref key) = queued.dedup_key {
                            if latest_seq.len() >= DEDUP_MAP_CAP {
                                latest_seq.clear();
                            }
                            latest_seq
                                .entry(key.clone())
                                .and_modify(|seq| {
                                    if queued.arrival_seq > *seq {
                                        *seq = queued.arrival_seq;
                                    }
                                })
                                .or_insert(queued.arrival_seq);
                        }
                        pending.push(queued);
                    }
                    None => {
                        // Channel closed.
                        while in_flight.join_next().await.is_some() {}
                        return;
                    }
                }
            }
        }
    }

    fn stale_read_reason(
        server: &LspServer,
        freshness: Option<&ReadFreshness>,
    ) -> Option<StaleReason> {
        let freshness = freshness?;
        let current = server.document_generation(&freshness.uri);
        let (captured, current) = is_read_stale(freshness, current)?;

        Some(StaleReason::DocumentGenerationAdvanced { captured, current })
    }

    /// Re-capture completion freshness after its ordered mutation barrier.
    ///
    /// Completion requests intentionally describe the document produced by
    /// all preceding `didChange` notifications. Their ingress snapshot can be
    /// stale by construction while those mutations are still queued, so the
    /// barrier result becomes the baseline for dispatch and response delivery.
    fn refresh_read_freshness(
        server: &LspServer,
        freshness: Option<&ReadFreshness>,
    ) -> Option<ReadFreshness> {
        let freshness = freshness?;
        let (document_generation, document_version, document_instance) = server
            .document_freshness(&freshness.uri)
            .map_or((None, None, None), |(generation, version, instance)| {
                (Some(generation), Some(version), Some(instance))
            });
        Some(ReadFreshness {
            uri: freshness.uri.clone(),
            document_generation,
            document_instance,
            document_version,
        })
    }

    fn send_response(outbound: &OutboundSender, response: JsonRpcResponse) {
        log_response(&response);
        let _ = outbound.send_response(response);
    }

    /// Run a request handler on the blocking pool, converting a handler panic
    /// into an `InternalError` response.
    ///
    /// Handlers are CPU-bound and take `parking_lot` locks, so they must not run
    /// on the async runtime. `spawn_blocking` catches a handler panic and hands
    /// it back as a `JoinError` rather than unwinding through the scheduler.
    /// `parking_lot` mutexes release while unwinding and are never poisoned, so
    /// the server itself stays usable after a panicked handler — the only thing
    /// still owed to the client is a reply, which is what this synthesizes.
    ///
    /// Both the mutation worker and the read dispatcher route through here so
    /// neither can reintroduce the dropped-`Err` hang (#5206).
    async fn run_handler<F>(work: F, id: Option<JsonRpcId>, method: &str) -> HandlerOutcome
    where
        F: FnOnce() -> Option<JsonRpcResponse> + Send + 'static,
    {
        match tokio::task::spawn_blocking(work).await {
            Ok(Some(response)) => HandlerOutcome::Response(response),
            Ok(None) => HandlerOutcome::Empty,
            Err(join_error) => {
                let detail = Self::join_failure_detail(join_error);

                // A notification has no id, so there is no reply to address and
                // nothing is left hanging. Log it and move on.
                let Some(id) = id else {
                    tracing::error!(method = %method, "Notification handler panicked: {detail}");
                    return HandlerOutcome::Empty;
                };

                // The payload goes to the log, not onto the wire. A panic
                // message can carry document text, absolute paths, or backend
                // state, and the client-facing error is surfaced in editor UI;
                // the method name is enough for the user to know what broke.
                tracing::error!(method = %method, "Request handler panicked: {detail}");
                HandlerOutcome::Panicked(JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: Some(id),
                    result: None,
                    error: Some(JsonRpcError {
                        code: INTERNAL_ERROR,
                        message: format!(
                            "{method} handler failed unexpectedly; see the Perl LSP server log for details"
                        ),
                        data: None,
                    }),
                })
            }
        }
    }

    /// Best-effort readable text for a failed handler task.
    ///
    /// A panic payload is only recoverable as `&'static str` (a literal
    /// `panic!("...")`) or `String` (a formatted one); anything else is opaque.
    fn join_failure_detail(join_error: tokio::task::JoinError) -> String {
        if !join_error.is_panic() {
            return "handler task was cancelled".to_string();
        }

        let payload = join_error.into_panic();
        payload
            .downcast_ref::<&'static str>()
            .map(|message| (*message).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic payload".to_string())
    }

    /// Send a handler response only while its captured document instance and
    /// generation are still current. The freshness decision and outbound
    /// enqueue share the document-store lock, so a concurrent mutation is
    /// ordered either before this response or after it, never between the
    /// check and the enqueue.
    fn send_response_if_fresh(
        server: &Arc<LspServer>,
        freshness: Option<&ReadFreshness>,
        response: JsonRpcResponse,
    ) -> Option<StaleReason> {
        let Some(freshness) = freshness else {
            Self::send_response(&server.outbound, response);
            return None;
        };

        let Some(captured) = freshness.document_generation else {
            Self::send_response(&server.outbound, response);
            return None;
        };
        let Some(instance) = freshness.document_instance.as_ref() else {
            return Some(StaleReason::DocumentInstanceChanged);
        };

        let normalized_uri = server.normalize_uri_key(&freshness.uri);
        let documents = server.documents.lock();
        let Some(document) = documents.get(&normalized_uri) else {
            return Some(StaleReason::DocumentInstanceChanged);
        };

        if !Arc::ptr_eq(&document.generation, instance) {
            return Some(StaleReason::DocumentInstanceChanged);
        }

        let current = document.current_generation();
        if current != captured {
            return Some(StaleReason::DocumentGenerationAdvanced { captured, current });
        }

        Self::send_response(&server.outbound, response);
        None
    }

    /// Why a stale read was cancelled. Used only for log/error messages.
    fn send_cancellation(
        server: &Arc<LspServer>,
        id: Option<JsonRpcId>,
        method: &str,
        reason: StaleReason,
    ) {
        let message = match reason {
            StaleReason::PositionSuperseded => {
                format!("Request superseded by newer {method} request")
            }
            StaleReason::DocumentGenerationAdvanced { captured, current } => format!(
                "Request superseded: document moved from generation {captured} to {current} while {method} was in flight"
            ),
            StaleReason::DocumentInstanceChanged => format!(
                "Request superseded: document was closed or replaced while {method} was running"
            ),
        };
        let cancelled_response = JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError { code: REQUEST_CANCELLED, message, data: None }),
        };
        Self::send_response(&server.outbound, cancelled_response);
    }

    /// Dispatch a single read request, cancelling it if it becomes stale
    /// before execution or before its response is delivered.
    async fn dispatch_one(
        queued: QueuedRead,
        latest_seq: &HashMap<RequestDedupKey, u64>,
        permits: &Arc<Semaphore>,
        in_flight: &mut JoinSet<()>,
        server: &Arc<LspServer>,
        mutation_seq_done: &Arc<AtomicU64>,
        mutation_notify: &Arc<Notify>,
    ) {
        let refresh_after_barrier =
            queued.request.method == "textDocument/completion" && queued.wait_for_seq > 0;

        // Stale check 1: position dedupe — newer same-position request supersedes.
        if let Some(ref key) = queued.dedup_key
            && let Some(&latest) = latest_seq.get(key)
            && queued.arrival_seq < latest
        {
            if let Some(id) = queued.request.id.as_ref() {
                server.clear_request_pending(id);
            }
            Self::send_cancellation(
                server,
                queued.request.id,
                &queued.request.method,
                StaleReason::PositionSuperseded,
            );
            return;
        }

        // Stale check 2: generation freshness — document moved on between
        // ingress and dispatch. This catches the typing-storm case where
        // every keystroke produces a unique position dedup key.
        if !refresh_after_barrier
            && let Some(reason) = Self::stale_read_reason(server, queued.freshness.as_ref())
        {
            if let Some(id) = queued.request.id.as_ref() {
                server.clear_request_pending(id);
            }
            Self::send_cancellation(server, queued.request.id, &queued.request.method, reason);
            return;
        }

        let permit = match Arc::clone(permits).acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => {
                if let Some(id) = queued.request.id.as_ref() {
                    server.clear_request_pending(id);
                }
                return;
            }
        };

        let srv = Arc::clone(server);
        let outbound = server.outbound.clone();
        let seq_done = Arc::clone(mutation_seq_done);
        let notify = Arc::clone(mutation_notify);
        let wait_for = queued.wait_for_seq;
        // Capture the method (opt-in) before `queued` is moved into the task, so
        // we can attribute the mutation-barrier wait to a concrete read request.
        let read_wait_method =
            crate::runtime::timing::is_enabled().then(|| queued.request.method.clone());
        let mut freshness = queued.freshness.clone();
        let method = queued.request.method.clone();
        let id = queued.request.id.clone();

        in_flight.spawn(async move {
            let _permit = permit;

            // Wait for all mutations that were enqueued before this read.
            // Use the standard Tokio Notify pattern: create the `notified()`
            // future BEFORE re-checking the condition so a `notify_waiters()`
            // that fires in the gap between the load and the park is not lost
            // (#5041). The previous check-then-await loop could park
            // indefinitely if the mutation completed between the `load` and
            // the `notified().await`.
            let t_read_wait = std::time::Instant::now();
            let notified = notify.notified();
            tokio::pin!(notified);
            loop {
                if seq_done.load(Ordering::SeqCst) >= wait_for {
                    break;
                }
                notified.as_mut().await;
                // Re-arm for the next iteration — `notify_waiters()` only
                // wakes permit-saved futures, so each iteration needs a fresh
                // subscription.
                notified.set(notify.notified());
            }
            // The read blocked here until the mutation barrier cleared — this is
            // the keystroke-to-completion wait a queued parse storm inflates.
            if let Some(method) = read_wait_method {
                crate::runtime::timing::emit(crate::runtime::timing::TimingSpan::labeled(
                    "scheduler.read_wait",
                    crate::runtime::timing::elapsed_ms(t_read_wait),
                    method,
                ));
            }

            if refresh_after_barrier {
                freshness = Self::refresh_read_freshness(&srv, freshness.as_ref());
            }

            if let Some(reason) = Self::stale_read_reason(&srv, freshness.as_ref()) {
                if let Some(id) = id.as_ref() {
                    srv.clear_request_pending(id);
                }
                Self::send_cancellation(&srv, id, &method, reason);
                return;
            }

            let pending_guard = PendingRequestGuard { server: Arc::clone(&srv), id: id.clone() };
            let outcome = Self::run_handler(
                {
                    let handler_server = Arc::clone(&srv);
                    move || {
                        let _pending_guard = pending_guard;
                        handler_server.handle_request(queued.request)
                    }
                },
                id.clone(),
                &method,
            )
            .await;

            match outcome {
                // A panic is not a stale result. Deliver the error without
                // consulting freshness, so a crash during an edit cannot leave
                // the request unanswered.
                HandlerOutcome::Panicked(response) => Self::send_response(&outbound, response),
                HandlerOutcome::Response(response) => {
                    // A handler may spend a long time outside the document lock
                    // (for example, waiting for an AI completion backend). A
                    // mutation can advance the document generation while that
                    // work is running, so make the final send decision against
                    // the freshness baseline established before the handler
                    // (at ingress, or after the ordered completion barrier).
                    if response.error.as_ref().is_some_and(|error| error.code == REQUEST_CANCELLED)
                    {
                        // Preserve a cancellation response that the handler
                        // already produced. The scheduler must not emit a second
                        // response when explicit cancellation races with a edit.
                        Self::send_response(&outbound, response);
                    } else if let Some(reason) =
                        Self::send_response_if_fresh(&srv, freshness.as_ref(), response)
                    {
                        Self::send_cancellation(&srv, id, &method, reason);
                    }
                }
                HandlerOutcome::Empty => {}
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::must_some;

    // =====================================================================
    // Existing classification tests
    // =====================================================================

    #[test]
    fn cancel_is_control() {
        assert_eq!(classify("$/cancelRequest"), RequestClass::Control);
        assert_eq!(classify("window/workDoneProgress/cancel"), RequestClass::Control);
    }

    #[test]
    fn lifecycle_methods() {
        assert_eq!(classify("initialize"), RequestClass::Lifecycle);
        assert_eq!(classify("initialized"), RequestClass::Lifecycle);
        assert_eq!(classify("shutdown"), RequestClass::Lifecycle);
        assert_eq!(classify("exit"), RequestClass::Lifecycle);
    }

    #[test]
    fn mutation_methods() {
        assert_eq!(classify("textDocument/didOpen"), RequestClass::Mutation);
        assert_eq!(classify("textDocument/didChange"), RequestClass::Mutation);
        assert_eq!(classify("textDocument/didClose"), RequestClass::Mutation);
        assert_eq!(classify("textDocument/didSave"), RequestClass::Mutation);
        assert_eq!(classify("textDocument/willSave"), RequestClass::Mutation);
        assert_eq!(classify("textDocument/willSaveWaitUntil"), RequestClass::Mutation);
        assert_eq!(classify("workspace/didChangeConfiguration"), RequestClass::Mutation);
        assert_eq!(classify("workspace/didCreateFiles"), RequestClass::Mutation);
        assert_eq!(classify("workspace/didDeleteFiles"), RequestClass::Mutation);
        assert_eq!(classify("workspace/didRenameFiles"), RequestClass::Mutation);
        assert_eq!(classify("workspace/didChangeWorkspaceFolders"), RequestClass::Mutation);
    }

    #[test]
    fn notebook_mutation_methods() {
        assert_eq!(classify("notebookDocument/didOpen"), RequestClass::Mutation);
        assert_eq!(classify("notebookDocument/didChange"), RequestClass::Mutation);
        assert_eq!(classify("notebookDocument/didSave"), RequestClass::Mutation);
        assert_eq!(classify("notebookDocument/didClose"), RequestClass::Mutation);
    }

    #[test]
    fn set_trace_is_lifecycle() {
        assert_eq!(classify("$/setTrace"), RequestClass::Lifecycle);
    }

    #[test]
    fn read_only_methods() {
        assert_eq!(classify("textDocument/hover"), RequestClass::ReadOnly);
        assert_eq!(classify("textDocument/completion"), RequestClass::ReadOnly);
        assert_eq!(classify("textDocument/definition"), RequestClass::ReadOnly);
        assert_eq!(classify("textDocument/references"), RequestClass::ReadOnly);
        assert_eq!(classify("workspace/symbol"), RequestClass::ReadOnly);
    }

    #[test]
    fn unknown_methods_are_read_only() {
        assert_eq!(classify("custom/unknown"), RequestClass::ReadOnly);
    }

    // =====================================================================
    // Priority tests (issue #2354)
    // =====================================================================

    #[test]
    fn hover_has_highest_priority() {
        assert_eq!(request_priority("textDocument/hover"), RequestPriority::Hover);
        assert!(
            RequestPriority::Hover.value() < RequestPriority::Completion.value(),
            "hover must outrank completion"
        );
    }

    #[test]
    fn completion_priority() {
        assert_eq!(request_priority("textDocument/completion"), RequestPriority::Completion);
        assert_eq!(request_priority("completionItem/resolve"), RequestPriority::Completion);
    }

    #[test]
    fn references_priority() {
        assert_eq!(request_priority("textDocument/references"), RequestPriority::References);
        assert_eq!(request_priority("textDocument/definition"), RequestPriority::References);
        assert_eq!(request_priority("textDocument/declaration"), RequestPriority::References);
        assert_eq!(request_priority("textDocument/typeDefinition"), RequestPriority::References);
        assert_eq!(request_priority("textDocument/implementation"), RequestPriority::References);
    }

    #[test]
    fn workspace_symbol_is_other() {
        assert_eq!(request_priority("workspace/symbol"), RequestPriority::Other);
        assert_eq!(request_priority("workspace/diagnostic"), RequestPriority::Other);
        assert_eq!(request_priority("custom/unknown"), RequestPriority::Other);
    }

    #[test]
    fn priority_ordering_hover_beats_completion() {
        // In a BinaryHeap (max-heap), the item that compares "greater" pops first.
        // We want hover > completion, so hover must be Ord-greater than completion.
        let hover = make_queued_read("textDocument/hover", 0);
        let completion = make_queued_read("textDocument/completion", 1);
        assert!(hover > completion, "hover must be ordered before completion");
    }

    #[test]
    fn priority_ordering_completion_beats_references() {
        let completion = make_queued_read("textDocument/completion", 0);
        let references = make_queued_read("textDocument/references", 1);
        assert!(completion > references, "completion must be ordered before references");
    }

    #[test]
    fn priority_ordering_references_beats_other() {
        let references = make_queued_read("textDocument/references", 0);
        let other = make_queued_read("workspace/symbol", 1);
        assert!(references > other, "references must be ordered before workspace/symbol");
    }

    #[test]
    fn same_priority_newer_arrival_wins() {
        // Two hover requests: the one with higher arrival_seq should pop first.
        let older = make_queued_read_with_seq("textDocument/hover", 0, 1);
        let newer = make_queued_read_with_seq("textDocument/hover", 0, 5);
        assert!(newer > older, "newer arrival should pop before older within the same priority");
    }

    #[test]
    fn priority_heap_drains_hover_first() {
        let mut heap: BinaryHeap<QueuedRead> = BinaryHeap::new();
        heap.push(make_queued_read("workspace/symbol", 0));
        heap.push(make_queued_read("textDocument/references", 1));
        heap.push(make_queued_read("textDocument/completion", 2));
        heap.push(make_queued_read("textDocument/hover", 3));

        // Pop order must be: hover, completion, references, workspace/symbol
        let mut pop_method = || must_some(heap.pop()).request.method;
        assert_eq!(pop_method(), "textDocument/hover");
        assert_eq!(pop_method(), "textDocument/completion");
        assert_eq!(pop_method(), "textDocument/references");
        assert_eq!(pop_method(), "workspace/symbol");
    }

    // =====================================================================
    // Dedup key tests (issue #2354)
    // =====================================================================

    #[test]
    fn dedup_key_extracted_for_hover() {
        let params = serde_json::json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 5, "character": 10 }
        });
        let key = extract_dedup_key("textDocument/hover", Some(&params), RequestPriority::Hover);
        let key = must_some(key);
        assert_eq!(key.method, "textDocument/hover");
        assert_eq!(key.uri, "file:///test.pl");
        assert_eq!(key.line, 5);
        assert_eq!(key.character, 10);
    }

    #[test]
    fn dedup_key_extracted_for_completion() {
        let params = serde_json::json!({
            "textDocument": { "uri": "file:///foo.pl" },
            "position": { "line": 0, "character": 3 }
        });
        let key = extract_dedup_key(
            "textDocument/completion",
            Some(&params),
            RequestPriority::Completion,
        );
        assert!(key.is_some());
    }

    #[test]
    fn dedup_key_none_for_other_priority() {
        let params = serde_json::json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 0, "character": 0 }
        });
        // workspace/symbol is Other priority — no dedup key.
        let key = extract_dedup_key("workspace/symbol", Some(&params), RequestPriority::Other);
        assert!(key.is_none());
    }

    #[test]
    fn dedup_key_none_when_no_params() {
        let key = extract_dedup_key("textDocument/hover", None, RequestPriority::Hover);
        assert!(key.is_none());
    }

    #[test]
    fn dedup_key_none_when_no_position() {
        let params = serde_json::json!({
            "textDocument": { "uri": "file:///test.pl" }
            // no "position"
        });
        let key = extract_dedup_key("textDocument/hover", Some(&params), RequestPriority::Hover);
        assert!(key.is_none());
    }

    #[test]
    fn dedup_key_none_when_uri_missing() {
        let params = serde_json::json!({
            "textDocument": {},
            "position": { "line": 1, "character": 2 }
        });
        let key = extract_dedup_key("textDocument/hover", Some(&params), RequestPriority::Hover);
        assert!(key.is_none());
    }

    #[test]
    fn dedup_key_is_supported_for_references_priority() {
        let params = serde_json::json!({
            "textDocument": { "uri": "file:///refs.pl" },
            "position": { "line": 12, "character": 4 }
        });
        let key = extract_dedup_key(
            "textDocument/references",
            Some(&params),
            RequestPriority::References,
        );

        assert_eq!(
            key,
            Some(RequestDedupKey {
                method: "textDocument/references".to_string(),
                uri: "file:///refs.pl".to_string(),
                line: 12,
                character: 4,
            })
        );
    }

    #[test]
    fn dedup_key_none_for_non_numeric_position_fields() {
        let params = serde_json::json!({
            "textDocument": { "uri": "file:///bad-pos.pl" },
            "position": { "line": "12", "character": "4" }
        });
        let key = extract_dedup_key("textDocument/hover", Some(&params), RequestPriority::Hover);
        assert!(key.is_none());
    }

    #[test]
    fn dedup_key_none_when_position_is_signed() {
        let params = serde_json::json!({
            "textDocument": { "uri": "file:///bad-pos.pl" },
            "position": { "line": 1, "character": -1 }
        });
        let key = extract_dedup_key("textDocument/hover", Some(&params), RequestPriority::Hover);
        assert!(key.is_none());
    }

    #[test]
    fn different_positions_produce_different_keys() {
        let params1 = serde_json::json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 1, "character": 0 }
        });
        let params2 = serde_json::json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 2, "character": 0 }
        });
        let key1 = extract_dedup_key("textDocument/hover", Some(&params1), RequestPriority::Hover);
        let key2 = extract_dedup_key("textDocument/hover", Some(&params2), RequestPriority::Hover);
        assert_ne!(key1, key2);
    }

    #[test]
    fn different_uris_produce_different_keys() {
        let params1 = serde_json::json!({
            "textDocument": { "uri": "file:///a.pl" },
            "position": { "line": 0, "character": 0 }
        });
        let params2 = serde_json::json!({
            "textDocument": { "uri": "file:///b.pl" },
            "position": { "line": 0, "character": 0 }
        });
        let key1 = extract_dedup_key("textDocument/hover", Some(&params1), RequestPriority::Hover);
        let key2 = extract_dedup_key("textDocument/hover", Some(&params2), RequestPriority::Hover);
        assert_ne!(key1, key2);
    }

    // =====================================================================
    // Helper constructors for tests
    // =====================================================================

    #[test]
    fn inline_completion_gets_completion_priority() {
        assert_eq!(request_priority("textDocument/inlineCompletion"), RequestPriority::Completion);
    }

    #[test]
    fn inline_completion_gets_dedup_key() {
        let params = serde_json::json!({
            "textDocument": { "uri": "file:///test.pl" },
            "position": { "line": 5, "character": 10 }
        });
        let key = extract_dedup_key(
            "textDocument/inlineCompletion",
            Some(&params),
            RequestPriority::Completion,
        );
        let key = must_some(key);
        assert_eq!(key.method, "textDocument/inlineCompletion");
        assert_eq!(key.uri, "file:///test.pl");
        assert_eq!(key.line, 5);
        assert_eq!(key.character, 10);
    }

    fn make_queued_read(method: &str, arrival_seq: u64) -> QueuedRead {
        make_queued_read_with_seq(method, 0, arrival_seq)
    }

    fn make_queued_read_with_seq(method: &str, wait_for_seq: u64, arrival_seq: u64) -> QueuedRead {
        let priority = request_priority(method);
        QueuedRead {
            request: JsonRpcRequest {
                _jsonrpc: "2.0".to_string(),
                id: None,
                method: method.to_string(),
                params: None,
            },
            wait_for_seq,
            priority,
            arrival_seq,
            dedup_key: None,
            freshness: None,
        }
    }

    #[test]
    fn abandoned_mutation_settlement_releases_pending_marker_and_barrier() {
        let server = Arc::new(crate::LspServer::new());
        let id = JsonRpcId::Integer(901);
        server.mark_request_pending(&id);

        let queued = QueuedMutation {
            request: JsonRpcRequest {
                _jsonrpc: "2.0".to_string(),
                id: Some(id.clone()),
                method: "textDocument/didChange".to_string(),
                params: None,
            },
            seq: 7,
            enqueued: std::time::Instant::now(),
        };
        let mutation_seq_done = Arc::new(AtomicU64::new(0));
        let mutation_notify = Arc::new(Notify::new());

        Scheduler::settle_abandoned_mutation(queued, &server, &mutation_seq_done, &mutation_notify);

        assert!(!server.pending_request_ids.lock().contains(&id));
        assert_eq!(mutation_seq_done.load(Ordering::SeqCst), 7);
    }

    // =====================================================================
    // Generation-aware freshness tests (PR 4 of 0.15.1 Neovim latency lane)
    // =====================================================================

    #[derive(Clone)]
    struct CapturedOutput {
        bytes: Arc<parking_lot::Mutex<Vec<u8>>>,
        write_signal: Option<std::sync::mpsc::Sender<()>>,
    }

    impl std::io::Write for CapturedOutput {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.bytes.lock().extend_from_slice(buf);
            if let Some(signal) = &self.write_signal {
                let _ = signal.send(());
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn server_with_captured_output() -> (Arc<crate::LspServer>, Arc<parking_lot::Mutex<Vec<u8>>>) {
        let bytes = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let writer = CapturedOutput { bytes: Arc::clone(&bytes), write_signal: None };
        let output: Arc<parking_lot::Mutex<Box<dyn std::io::Write + Send>>> =
            Arc::new(parking_lot::Mutex::new(Box::new(writer)));
        (Arc::new(crate::LspServer::with_output(output)), bytes)
    }

    fn server_with_signalled_output()
    -> (Arc<crate::LspServer>, Arc<parking_lot::Mutex<Vec<u8>>>, std::sync::mpsc::Receiver<()>)
    {
        let bytes = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let (write_signal, writes) = std::sync::mpsc::channel();
        let writer = CapturedOutput { bytes: Arc::clone(&bytes), write_signal: Some(write_signal) };
        let output: Arc<parking_lot::Mutex<Box<dyn std::io::Write + Send>>> =
            Arc::new(parking_lot::Mutex::new(Box::new(writer)));
        (Arc::new(crate::LspServer::with_output(output)), bytes, writes)
    }

    fn position_params(uri: &str) -> serde_json::Value {
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": 4, "character": 7 }
        })
    }

    fn position_params_at(uri: &str, line: u64, character: u64) -> serde_json::Value {
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        })
    }

    fn queued_completion_read(
        server: &crate::LspServer,
        uri: &str,
        line: u64,
        character: u64,
        arrival_seq: u64,
        id: i64,
    ) -> QueuedRead {
        let params = position_params_at(uri, line, character);
        let priority = request_priority("textDocument/completion");
        let dedup_key = extract_dedup_key("textDocument/completion", Some(&params), priority);
        let freshness =
            extract_freshness(server, "textDocument/completion", Some(&params), priority);
        QueuedRead {
            request: JsonRpcRequest {
                _jsonrpc: "2.0".to_string(),
                id: Some(JsonRpcId::Integer(id)),
                method: "textDocument/completion".to_string(),
                params: Some(params),
            },
            wait_for_seq: 0,
            priority,
            arrival_seq,
            dedup_key,
            freshness,
        }
    }

    fn queued_inline_completion_read(
        server: &crate::LspServer,
        uri: &str,
        arrival_seq: u64,
        id: i64,
    ) -> QueuedRead {
        let params = position_params_at(uri, 0, 4);
        let method = "textDocument/inlineCompletion";
        let priority = request_priority(method);
        let dedup_key = extract_dedup_key(method, Some(&params), priority);
        let freshness = extract_freshness(server, method, Some(&params), priority);
        QueuedRead {
            request: JsonRpcRequest {
                _jsonrpc: "2.0".to_string(),
                id: Some(JsonRpcId::Integer(id)),
                method: method.to_string(),
                params: Some(params),
            },
            wait_for_seq: 0,
            priority,
            arrival_seq,
            dedup_key,
            freshness,
        }
    }

    struct BlockingInlineCompletionBackend {
        started: Arc<std::sync::Barrier>,
        release: Arc<std::sync::Barrier>,
    }

    const STALE_INLINE_REQUEST_ID: i64 = 77;
    const FRESH_INLINE_REQUEST_ID: i64 = 78;
    const REOPENED_INLINE_REQUEST_ID: i64 = 79;
    const STALE_INLINE_RESULT: &str = "STALE_AI_RESULT";

    impl perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend
        for BlockingInlineCompletionBackend
    {
        fn stream(
            &self,
            _req: &perl_lsp_rs_core::providers::inline_completion::BackendRequest,
            sink: &mut dyn FnMut(
                perl_lsp_rs_core::providers::inline_completion::StreamChunk,
            )
                -> perl_lsp_rs_core::providers::inline_completion::StreamControl,
        ) -> Result<(), perl_lsp_rs_core::providers::inline_completion::BackendError> {
            self.started.wait();
            self.release.wait();
            let _ = sink(perl_lsp_rs_core::providers::inline_completion::StreamChunk {
                text: STALE_INLINE_RESULT.to_string(),
                is_final: true,
            });
            Ok(())
        }
    }

    fn initialize_scheduler_test_server(
        server: &crate::LspServer,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let response = server
            .handle_request(JsonRpcRequest {
                _jsonrpc: "2.0".to_string(),
                id: Some(JsonRpcId::Integer(1)),
                method: "initialize".to_string(),
                params: Some(serde_json::json!({
                    "processId": 1,
                    "capabilities": {},
                })),
            })
            .ok_or("initialize must produce a response")?;
        if response.error.is_some() {
            return Err("initialize must succeed".into());
        }
        Ok(())
    }

    fn wait_for_response_id(
        output: &Arc<parking_lot::Mutex<Vec<u8>>>,
        writes: &std::sync::mpsc::Receiver<()>,
        id: i64,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            let bytes = output.lock().clone();
            let mut cursor = 0;
            while let Some(relative_header_end) =
                bytes[cursor..].windows(4).position(|window| window == b"\r\n\r\n")
            {
                let header_end = cursor + relative_header_end;
                let header = std::str::from_utf8(&bytes[cursor..header_end])?;
                let content_length = header
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .ok_or("response is missing Content-Length")?
                    .parse::<usize>()?;
                let body_start = header_end + 4;
                let body_end = body_start + content_length;
                if bytes.len() < body_end {
                    break;
                }
                let value: serde_json::Value =
                    serde_json::from_slice(&bytes[body_start..body_end])?;
                if value.get("id").and_then(serde_json::Value::as_i64) == Some(id) {
                    return Ok(value);
                }
                cursor = body_end;
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(format!("timed out waiting for response {id}").into());
            }
            writes.recv_timeout(remaining)?;
        }
    }

    fn rapid_typing_source(suffix_len: usize) -> String {
        format!(
            "use strict;\nuse warnings;\n\nmy $value = 42;\nmy $other = $v{}\n",
            "a".repeat(suffix_len)
        )
    }

    fn make_freshness(uri: &str, generation: Option<u32>, version: Option<i32>) -> ReadFreshness {
        ReadFreshness {
            uri: uri.to_string(),
            document_generation: generation,
            document_instance: None,
            document_version: version,
        }
    }

    #[test]
    fn extract_freshness_captures_generation_for_open_document_hover() -> Result<(), JsonRpcError> {
        let server = crate::LspServer::new();
        server.test_apply_did_open("file:///x.pl", "my $a;\n", 1)?;
        let params = position_params("file:///x.pl");
        let f = must_some(extract_freshness(
            &server,
            "textDocument/hover",
            Some(&params),
            RequestPriority::Hover,
        ));
        assert_eq!(f.uri, "file:///x.pl");
        assert_eq!(f.document_generation, Some(0));
        assert_eq!(f.document_version, Some(1));
        Ok(())
    }

    #[test]
    fn extract_freshness_captures_generation_for_completion() -> Result<(), JsonRpcError> {
        let server = crate::LspServer::new();
        server.test_apply_did_open("file:///c.pl", "my $a;\n", 1)?;
        let params = position_params("file:///c.pl");
        let freshness = extract_freshness(
            &server,
            "textDocument/completion",
            Some(&params),
            RequestPriority::Completion,
        );
        assert!(freshness.is_some(), "completion must capture freshness");
        Ok(())
    }

    #[test]
    fn extract_freshness_captures_generation_for_definition() -> Result<(), JsonRpcError> {
        let server = crate::LspServer::new();
        server.test_apply_did_open("file:///d.pl", "sub f {}\n", 1)?;
        let params = position_params("file:///d.pl");
        let freshness = extract_freshness(
            &server,
            "textDocument/definition",
            Some(&params),
            RequestPriority::References,
        );
        assert!(freshness.is_some(), "definition must capture freshness");
        Ok(())
    }

    #[test]
    fn extract_freshness_none_for_unopened_document_returns_none_generation() {
        let server = crate::LspServer::new();
        let params = position_params("file:///not-open.pl");
        let f = must_some(extract_freshness(
            &server,
            "textDocument/hover",
            Some(&params),
            RequestPriority::Hover,
        ));
        assert_eq!(f.document_generation, None, "no open doc => no generation");
        assert_eq!(f.document_version, None);
    }

    #[test]
    fn extract_freshness_none_for_other_priority() {
        let server = crate::LspServer::new();
        let params = position_params("file:///x.pl");
        let freshness =
            extract_freshness(&server, "workspace/symbol", Some(&params), RequestPriority::Other);
        assert!(freshness.is_none(), "non-position requests must not capture freshness");
    }

    #[test]
    fn is_read_stale_returns_some_when_generation_advanced() {
        let f = make_freshness("file:///a.pl", Some(3), Some(1));
        let staleness = is_read_stale(&f, Some(5));
        assert_eq!(staleness, Some((3, 5)));
    }

    #[test]
    fn is_read_stale_returns_none_when_generation_unchanged() {
        let f = make_freshness("file:///a.pl", Some(3), Some(1));
        let staleness = is_read_stale(&f, Some(3));
        assert!(staleness.is_none());
    }

    #[test]
    fn is_read_stale_returns_none_when_freshness_has_no_generation() {
        let f = make_freshness("file:///a.pl", None, None);
        assert!(is_read_stale(&f, Some(7)).is_none());
    }

    #[test]
    fn is_read_stale_returns_none_when_document_closed() {
        let f = make_freshness("file:///a.pl", Some(3), Some(1));
        // Document closed between ingress and dispatch — provider should
        // surface the missing-doc error itself; the freshness gate is silent.
        assert!(is_read_stale(&f, None).is_none());
    }

    #[test]
    fn stale_hover_cancelled_after_newer_generation() -> Result<(), JsonRpcError> {
        // End-to-end at the freshness level: capture for hover, bump gen,
        // observe staleness.
        let server = crate::LspServer::new();
        server.test_apply_did_open("file:///h.pl", "my $a;\n", 1)?;
        let params = position_params("file:///h.pl");
        let freshness = must_some(extract_freshness(
            &server,
            "textDocument/hover",
            Some(&params),
            RequestPriority::Hover,
        ));
        server.test_apply_did_change("file:///h.pl", "my $aa;\n", 2)?;
        let current = server.document_generation(&freshness.uri);
        let staleness = is_read_stale(&freshness, current);
        assert!(staleness.is_some(), "hover queued at gen=0 with doc now at gen=1 must be stale");
        Ok(())
    }

    #[test]
    fn stale_completion_cancelled_after_newer_generation() -> Result<(), JsonRpcError> {
        let server = crate::LspServer::new();
        server.test_apply_did_open("file:///comp.pl", "my $a;\n", 1)?;
        let params = position_params("file:///comp.pl");
        let freshness = must_some(extract_freshness(
            &server,
            "textDocument/completion",
            Some(&params),
            RequestPriority::Completion,
        ));
        server.test_apply_did_change("file:///comp.pl", "my $ab;\n", 2)?;
        let current = server.document_generation(&freshness.uri);
        assert!(is_read_stale(&freshness, current).is_some());
        Ok(())
    }

    #[test]
    fn stale_definition_cancelled_after_newer_generation() -> Result<(), JsonRpcError> {
        let server = crate::LspServer::new();
        server.test_apply_did_open("file:///def.pl", "sub f {}\n", 1)?;
        let params = position_params("file:///def.pl");
        let freshness = must_some(extract_freshness(
            &server,
            "textDocument/definition",
            Some(&params),
            RequestPriority::References,
        ));
        server.test_apply_did_change("file:///def.pl", "sub f { 1 }\n", 2)?;
        let current = server.document_generation(&freshness.uri);
        assert!(is_read_stale(&freshness, current).is_some());
        Ok(())
    }

    #[test]
    fn newest_request_for_generation_runs() -> Result<(), JsonRpcError> {
        // The dispatcher must not cancel a request whose snapshot equals the
        // current generation. (The dedup map still cancels earlier requests at
        // the same position — that's a separate axis.)
        let server = crate::LspServer::new();
        server.test_apply_did_open("file:///fresh.pl", "my $a;\n", 1)?;
        server.test_apply_did_change("file:///fresh.pl", "my $ab;\n", 2)?;
        let params = position_params("file:///fresh.pl");
        let freshness = must_some(extract_freshness(
            &server,
            "textDocument/hover",
            Some(&params),
            RequestPriority::Hover,
        ));
        // Snapshot is current; no further mutation; staleness must be None.
        let current = server.document_generation(&freshness.uri);
        assert!(
            is_read_stale(&freshness, current).is_none(),
            "snapshot matching current gen must NOT be stale"
        );
        Ok(())
    }

    #[test]
    fn generation_cancellation_independent_of_cursor_position() -> Result<(), JsonRpcError> {
        // The plan emphasises: "Do not require cursor position to match."
        // Two requests at different positions on the same document should
        // both see staleness once gen advances.
        let server = crate::LspServer::new();
        server.test_apply_did_open("file:///z.pl", "my $aa;\nmy $bb;\n", 1)?;
        let params_a = serde_json::json!({
            "textDocument": { "uri": "file:///z.pl" },
            "position": { "line": 0, "character": 3 }
        });
        let params_b = serde_json::json!({
            "textDocument": { "uri": "file:///z.pl" },
            "position": { "line": 1, "character": 5 }
        });
        let fa = must_some(extract_freshness(
            &server,
            "textDocument/hover",
            Some(&params_a),
            RequestPriority::Hover,
        ));
        let fb = must_some(extract_freshness(
            &server,
            "textDocument/hover",
            Some(&params_b),
            RequestPriority::Hover,
        ));
        // Bump generation once; both snapshots became stale even though
        // their (line, character) keys differ.
        server.test_apply_did_change("file:///z.pl", "my $aaa;\nmy $bb;\n", 2)?;
        let current = server.document_generation("file:///z.pl");
        assert!(is_read_stale(&fa, current).is_some(), "a is stale");
        assert!(is_read_stale(&fb, current).is_some(), "b is stale");
        Ok(())
    }

    #[test]
    fn stale_read_reason_reports_advanced_generation() -> Result<(), JsonRpcError> {
        let server = crate::LspServer::new();
        let uri = "file:///reason.pl";
        server.test_apply_did_open(uri, "my $a;\n", 1)?;
        let freshness = make_freshness(uri, Some(0), Some(1));

        assert_eq!(Scheduler::stale_read_reason(&server, Some(&freshness)), None);

        server.test_apply_did_change(uri, "my $aa;\n", 2)?;
        assert_eq!(
            Scheduler::stale_read_reason(&server, Some(&freshness)),
            Some(StaleReason::DocumentGenerationAdvanced { captured: 0, current: 1 })
        );
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn completion_refreshes_freshness_after_ordered_mutation_wait()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, output) = server_with_captured_output();
        initialize_scheduler_test_server(&server)?;
        output.lock().clear();
        let uri = "file:///mutation-wait-race.pl";
        server.test_apply_did_open(uri, &rapid_typing_source(1), 1)?;

        let mut queued = queued_completion_read(&server, uri, 4, 14, 1, 77);
        queued.wait_for_seq = 1;

        let one_permit = Arc::new(Semaphore::new(1));
        let mut in_flight = JoinSet::new();
        let mutation_seq_done = Arc::new(AtomicU64::new(0));
        let mutation_notify = Arc::new(Notify::new());
        let latest_seq = HashMap::new();

        Scheduler::dispatch_one(
            queued,
            &latest_seq,
            &one_permit,
            &mut in_flight,
            &server,
            &mutation_seq_done,
            &mutation_notify,
        )
        .await;
        assert_eq!(in_flight.len(), 1, "fresh read should wait behind mutation barrier");

        server.test_apply_did_change(uri, &rapid_typing_source(2), 2)?;
        mutation_seq_done.store(1, Ordering::SeqCst);
        mutation_notify.notify_waiters();

        let completed =
            tokio::time::timeout(std::time::Duration::from_millis(500), in_flight.join_next())
                .await;
        assert!(completed.is_ok(), "completion should run promptly after mutation barrier opens");

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let bytes = output.lock().clone();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains("document moved from generation 0 to 1"),
            "ordered mutations must become the completion freshness baseline; output={text}"
        );
        assert!(
            text.contains("\"id\":77"),
            "completion handler must answer after the ordered mutation barrier; output={text}"
        );

        Ok(())
    }

    #[test]
    fn refreshed_completion_rejects_a_later_mutation_at_delivery() -> Result<(), JsonRpcError> {
        let (server, output) = server_with_captured_output();
        let uri = "file:///completion-refresh-race.pl";
        server.test_apply_did_open(uri, "my $value;\n", 1)?;
        let ingress = must_some(extract_freshness(
            &server,
            "textDocument/completion",
            Some(&position_params(uri)),
            RequestPriority::Completion,
        ));

        server.test_apply_did_change(uri, "my $value = 1;\n", 2)?;
        let refreshed = must_some(Scheduler::refresh_read_freshness(&server, Some(&ingress)));
        assert_eq!(Scheduler::stale_read_reason(&server, Some(&refreshed)), None);

        server.test_apply_did_change(uri, "my $value = 12;\n", 3)?;
        assert_eq!(
            Scheduler::send_response_if_fresh(
                &server,
                Some(&refreshed),
                JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: JsonRpcId::from_value(&serde_json::json!(78)),
                    result: Some(serde_json::json!([])),
                    error: None,
                },
            ),
            Some(StaleReason::DocumentGenerationAdvanced { captured: 1, current: 2 })
        );
        assert!(output.lock().is_empty(), "post-handler stale result must not be delivered");
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stale_read_cancelled_after_handler_execution() -> Result<(), Box<dyn std::error::Error>>
    {
        let (server, output, writes) = server_with_signalled_output();
        initialize_scheduler_test_server(&server)?;
        output.lock().clear();

        let uri = "file:///inline-stale-after-execution.pl";
        server.test_apply_did_open(uri, "use ", 1)?;
        server.test_configure_ai_completion(true, false);
        let started = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        server.test_install_ai_backend(Some(Arc::new(BlockingInlineCompletionBackend {
            started: Arc::clone(&started),
            release: Arc::clone(&release),
        })));

        let queued = queued_inline_completion_read(&server, uri, 1, STALE_INLINE_REQUEST_ID);
        let one_permit = Arc::new(Semaphore::new(1));
        let mut in_flight = JoinSet::new();
        let mutation_seq_done = Arc::new(AtomicU64::new(0));
        let mutation_notify = Arc::new(Notify::new());
        let latest_seq = HashMap::new();

        Scheduler::dispatch_one(
            queued,
            &latest_seq,
            &one_permit,
            &mut in_flight,
            &server,
            &mutation_seq_done,
            &mutation_notify,
        )
        .await;

        let started_wait = Arc::clone(&started);
        tokio::task::spawn_blocking(move || started_wait.wait()).await?;
        server.test_apply_did_change(uri, "use strict;\n", 2)?;
        release.wait();
        while in_flight.join_next().await.is_some() {}

        let response = wait_for_response_id(&output, &writes, STALE_INLINE_REQUEST_ID)?;
        if response.pointer("/error/code").and_then(serde_json::Value::as_i64)
            != Some(i64::from(REQUEST_CANCELLED))
        {
            return Err(format!("stale inline completion must be cancelled: {response}").into());
        }
        if response.to_string().contains(STALE_INLINE_RESULT)
            || response.get("result").is_some_and(|result| !result.is_null())
        {
            return Err(format!("stale inline completion must not be sent: {response}").into());
        }
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stale_read_cancelled_after_document_reopen() -> Result<(), Box<dyn std::error::Error>>
    {
        let (server, output, writes) = server_with_signalled_output();
        initialize_scheduler_test_server(&server)?;
        output.lock().clear();

        let uri = "file:///inline-reopened-after-execution.pl";
        server.test_apply_did_open(uri, "use ", 1)?;
        server.test_configure_ai_completion(true, false);
        let started = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        server.test_install_ai_backend(Some(Arc::new(BlockingInlineCompletionBackend {
            started: Arc::clone(&started),
            release: Arc::clone(&release),
        })));

        let queued = queued_inline_completion_read(&server, uri, 1, REOPENED_INLINE_REQUEST_ID);
        let one_permit = Arc::new(Semaphore::new(1));
        let mut in_flight = JoinSet::new();
        let mutation_seq_done = Arc::new(AtomicU64::new(0));
        let mutation_notify = Arc::new(Notify::new());
        let latest_seq = HashMap::new();

        Scheduler::dispatch_one(
            queued,
            &latest_seq,
            &one_permit,
            &mut in_flight,
            &server,
            &mutation_seq_done,
            &mutation_notify,
        )
        .await;

        let started_wait = Arc::clone(&started);
        tokio::task::spawn_blocking(move || started_wait.wait()).await?;
        server.test_apply_did_close(uri)?;
        server.test_apply_did_open(uri, "new buffer", 1)?;
        release.wait();
        while in_flight.join_next().await.is_some() {}

        let response = wait_for_response_id(&output, &writes, REOPENED_INLINE_REQUEST_ID)?;
        if response.pointer("/error/code").and_then(serde_json::Value::as_i64)
            != Some(i64::from(REQUEST_CANCELLED))
        {
            return Err(format!("reopened document result must be cancelled: {response}").into());
        }
        if response.to_string().contains(STALE_INLINE_RESULT) {
            return Err(format!("reopened document must not receive old result: {response}").into());
        }
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fresh_read_delivers_handler_result_after_execution()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, output, writes) = server_with_signalled_output();
        initialize_scheduler_test_server(&server)?;
        output.lock().clear();

        let uri = "file:///inline-fresh-after-execution.pl";
        server.test_apply_did_open(uri, "use ", 1)?;
        server.test_configure_ai_completion(true, false);
        let started = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        server.test_install_ai_backend(Some(Arc::new(BlockingInlineCompletionBackend {
            started: Arc::clone(&started),
            release: Arc::clone(&release),
        })));

        let queued = queued_inline_completion_read(&server, uri, 1, FRESH_INLINE_REQUEST_ID);
        let one_permit = Arc::new(Semaphore::new(1));
        let mut in_flight = JoinSet::new();
        let mutation_seq_done = Arc::new(AtomicU64::new(0));
        let mutation_notify = Arc::new(Notify::new());
        let latest_seq = HashMap::new();

        Scheduler::dispatch_one(
            queued,
            &latest_seq,
            &one_permit,
            &mut in_flight,
            &server,
            &mutation_seq_done,
            &mutation_notify,
        )
        .await;

        let started_wait = Arc::clone(&started);
        tokio::task::spawn_blocking(move || started_wait.wait()).await?;
        release.wait();
        while in_flight.join_next().await.is_some() {}

        let response = wait_for_response_id(&output, &writes, FRESH_INLINE_REQUEST_ID)?;
        if !response.to_string().contains(STALE_INLINE_RESULT)
            || response.get("result").is_none_or(serde_json::Value::is_null)
        {
            return Err(format!("fresh inline completion must be sent: {response}").into());
        }
        if response.pointer("/error/code").is_some() {
            return Err(format!("fresh inline completion must not be cancelled: {response}").into());
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rapid_typing_stale_reads_cancel_before_worker_permit_receipt()
    -> Result<(), JsonRpcError> {
        let server = Arc::new(crate::LspServer::new());
        let uri = "file:///typing-pressure.pl";
        server.test_apply_did_open(uri, &rapid_typing_source(1), 1)?;

        let mut stale_reads = Vec::new();
        for i in 0..12 {
            // Capture a read at the current document generation and then
            // immediately simulate the next keystroke. The cursor position
            // changes each time, so position-key dedupe alone cannot save us.
            stale_reads.push(queued_completion_read(&server, uri, 4, 14 + i, i, 10_000 + i as i64));
            server.test_apply_did_change(
                uri,
                &rapid_typing_source(i as usize + 2),
                i as i32 + 2,
            )?;
        }

        let latest = queued_completion_read(&server, uri, 4, 30, 99, 20_000);
        let current_generation = server.document_generation(uri);
        assert!(
            latest
                .freshness
                .as_ref()
                .and_then(|freshness| is_read_stale(freshness, current_generation))
                .is_none(),
            "latest generation request must not be stale"
        );

        let zero_permits = Arc::new(Semaphore::new(0));
        let mut in_flight = JoinSet::new();
        let mutation_seq_done = Arc::new(AtomicU64::new(0));
        let mutation_notify = Arc::new(Notify::new());
        let latest_seq = HashMap::new();
        let stale_count = stale_reads.len();

        for queued in stale_reads {
            let completed = tokio::time::timeout(
                std::time::Duration::from_millis(100),
                Scheduler::dispatch_one(
                    queued,
                    &latest_seq,
                    &zero_permits,
                    &mut in_flight,
                    &server,
                    &mutation_seq_done,
                    &mutation_notify,
                ),
            )
            .await;
            assert!(
                completed.is_ok(),
                "stale reads must cancel before waiting for a worker permit"
            );
        }
        assert_eq!(in_flight.len(), 0, "stale reads must not spawn worker tasks");

        let one_permit = Arc::new(Semaphore::new(1));
        Scheduler::dispatch_one(
            latest,
            &latest_seq,
            &one_permit,
            &mut in_flight,
            &server,
            &mutation_seq_done,
            &mutation_notify,
        )
        .await;
        assert_eq!(in_flight.len(), 1, "latest generation request must reach a worker");
        while in_flight.join_next().await.is_some() {}

        println!(
            "{}",
            serde_json::json!({
                "profile": "neovim_lean",
                "stale_reads_queued": stale_count,
                "stale_reads_cancelled_before_worker_permit": stale_count,
                "latest_generation_request_reached_worker": true,
                "cursor_positions_changed": true
            })
        );

        Ok(())
    }

    // =====================================================================
    // URI normalization regression tests for #965
    // =====================================================================

    #[test]
    fn extract_freshness_captures_generation_with_mixed_case_uri() -> Result<(), JsonRpcError> {
        let server = crate::LspServer::new();
        // Open with uppercase drive letter — text_sync normalizes to lowercase on store.
        // document_generation must normalize the query URI too, or it returns None.
        server.test_apply_did_open("file:///C:/project/foo.pl", "my $a;\n", 1)?;
        let params = position_params("file:///C:/project/foo.pl");
        let f = must_some(extract_freshness(
            &server,
            "textDocument/hover",
            Some(&params),
            RequestPriority::Hover,
        ));
        // Before fix: None (raw uppercase key misses normalized lowercase entry)
        // After fix: Some(0)
        assert_eq!(
            f.document_generation,
            Some(0),
            "mixed-case URI must resolve to open document generation"
        );
        Ok(())
    }

    #[test]
    fn stale_hover_cancelled_with_mixed_case_uri() -> Result<(), JsonRpcError> {
        let server = crate::LspServer::new();
        server.test_apply_did_open("file:///C:/project/bar.pl", "my $a;\n", 1)?;
        let params = position_params("file:///C:/project/bar.pl");
        let freshness = must_some(extract_freshness(
            &server,
            "textDocument/hover",
            Some(&params),
            RequestPriority::Hover,
        ));
        server.test_apply_did_change("file:///C:/project/bar.pl", "my $aa;\n", 2)?;
        let current = server.document_generation("file:///C:/project/bar.pl");
        assert!(
            is_read_stale(&freshness, current).is_some(),
            "hover queued at gen=0 with doc now at gen=1 must be stale (mixed-case URI)"
        );
        Ok(())
    }

    #[test]
    fn document_version_resolves_with_mixed_case_uri() -> Result<(), JsonRpcError> {
        let server = crate::LspServer::new();
        server.test_apply_did_open("file:///C:/x.pl", "my $a;\n", 7)?;
        // Before fix: None; After fix: Some(7)
        assert_eq!(
            server.document_version("file:///C:/x.pl"),
            Some(7),
            "document_version must normalize the URI before lookup"
        );
        Ok(())
    }

    #[test]
    fn buffer_text_resolves_with_mixed_case_uri() -> Result<(), JsonRpcError> {
        let server = crate::LspServer::new();
        server.test_apply_did_open("file:///C:/y.pl", "use strict;\n", 1)?;
        // Before fix: None; After fix: Some("use strict;\n")
        assert_eq!(
            server.buffer_text("file:///C:/y.pl"),
            Some("use strict;\n".to_string()),
            "buffer_text must normalize the URI before lookup"
        );
        Ok(())
    }

    // =====================================================================
    // Panicked handler must never leave a request unanswered (#5206)
    //
    // These drive `Scheduler::run_handler` — the same function the mutation
    // worker and the read dispatcher both call — with a closure that really
    // panics on the blocking pool, so the `JoinError` under test is a real one.
    // =====================================================================

    /// The synthesized response for a panicked handler, or `None` for any other
    /// outcome. Lets the assertions below use `must_some` instead of unwrapping.
    fn panicked_response(outcome: HandlerOutcome) -> Option<JsonRpcResponse> {
        match outcome {
            HandlerOutcome::Panicked(response) => Some(response),
            HandlerOutcome::Response(_) | HandlerOutcome::Empty => None,
        }
    }

    /// The handler response for a healthy outcome, or `None` otherwise.
    fn healthy_response(outcome: HandlerOutcome) -> Option<JsonRpcResponse> {
        match outcome {
            HandlerOutcome::Response(response) => Some(response),
            HandlerOutcome::Panicked(_) | HandlerOutcome::Empty => None,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::panic, reason = "the handler under test must actually panic")]
    async fn panicked_request_handler_answers_with_internal_error() {
        let outcome = Scheduler::run_handler(
            || panic!("provider exploded"),
            Some(JsonRpcId::Integer(7)),
            "textDocument/hover",
        )
        .await;

        // Before the fix the `Err(JoinError)` was dropped by
        // `if let Ok(Some(response))` and the client waited forever.
        let response = must_some(panicked_response(outcome));

        assert_eq!(
            response.id,
            Some(JsonRpcId::Integer(7)),
            "the reply must carry the panicked request's id, or the client cannot match it"
        );
        assert!(response.result.is_none(), "a panicked handler has no result");

        let error = must_some(response.error);
        assert_eq!(error.code, INTERNAL_ERROR);
        assert!(
            error.message.contains("textDocument/hover"),
            "error should name the method that panicked; got {:?}",
            error.message
        );
        // The payload belongs in the server log, not on the wire: a panic
        // message can carry document text or absolute paths, and this error is
        // surfaced in editor UI.
        assert!(
            !error.message.contains("provider exploded"),
            "panic payload must not be echoed to the client; got {:?}",
            error.message
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::panic, reason = "the handler under test must actually panic")]
    async fn panicked_notification_handler_sends_nothing() {
        // A notification has no id, so there is no reply to address and nothing
        // is left hanging. Answering anyway would be a protocol violation.
        let outcome =
            Scheduler::run_handler(|| panic!("boom"), None, "textDocument/didChange").await;

        assert!(
            matches!(outcome, HandlerOutcome::Empty),
            "a panicked notification must not produce a response, got {outcome:?}"
        );
    }

    /// The `JoinError` a real panicking blocking task produces.
    #[allow(clippy::panic, reason = "the task under test must actually panic")]
    async fn join_error_from<F>(work: F) -> tokio::task::JoinError
    where
        F: FnOnce() + Send + 'static,
    {
        match tokio::task::spawn_blocking(work).await {
            Ok(()) => panic!("task was expected to panic but returned normally"),
            Err(join_error) => join_error,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::panic, reason = "the task under test must actually panic")]
    async fn join_failure_detail_recovers_static_str_payload() {
        // A literal `panic!("...")` carries a `&'static str` payload.
        let detail =
            Scheduler::join_failure_detail(join_error_from(|| panic!("provider exploded")).await);
        assert_eq!(detail, "provider exploded");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::panic, reason = "the task under test must actually panic")]
    async fn join_failure_detail_recovers_formatted_string_payload() {
        // `panic!("{}", x)` carries a `String` payload rather than a `&'static str`.
        // These are separate downcast arms, so both need covering — the log line
        // is the only place a panic payload is still reported.
        let detail = Scheduler::join_failure_detail(
            join_error_from(|| panic!("{}", "index out of range".to_string())).await,
        );
        assert_eq!(detail, "index out of range");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn healthy_handler_outcomes_are_unchanged() {
        // Guards the refactor: the non-panicking paths must behave exactly as
        // the previous `if let Ok(Some(response))` did.
        let response = JsonRpcResponse {
            jsonrpc: "2.0",
            id: Some(JsonRpcId::Integer(3)),
            result: Some(serde_json::json!({"ok": true})),
            error: None,
        };
        let outcome = Scheduler::run_handler(
            move || Some(response),
            Some(JsonRpcId::Integer(3)),
            "initialize",
        )
        .await;
        let delivered = must_some(healthy_response(outcome));
        assert_eq!(delivered.id, Some(JsonRpcId::Integer(3)));

        let empty = Scheduler::run_handler(|| None, None, "textDocument/didOpen").await;
        assert!(
            matches!(empty, HandlerOutcome::Empty),
            "a notification handler returning None must stay Empty, got {empty:?}"
        );
    }
}
