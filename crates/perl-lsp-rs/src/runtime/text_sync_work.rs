//! Deterministic full-source work accounting for the text-sync → parse → snapshot path (#7301).
//!
//! Opt-in like [`super::timing`]: zero allocation on the hot path when disabled. Tests and
//! `expose_lsp_test_api` builds use an in-process capture buffer independent of environment.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[cfg(any(test, feature = "expose_lsp_test_api"))]
thread_local! {
    static CAPTURE_OWNER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    static CAPTURE_DOCUMENT: std::cell::RefCell<Option<String>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
pub(crate) struct CaptureOwnerScope(Option<usize>);

#[cfg(any(test, feature = "expose_lsp_test_api"))]
impl Drop for CaptureOwnerScope {
    fn drop(&mut self) {
        CAPTURE_OWNER.with(|current| current.set(self.0));
    }
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
pub(crate) fn owner_scope(owner: usize) -> CaptureOwnerScope {
    let previous = CAPTURE_OWNER.with(|current| current.replace(Some(owner)));
    CaptureOwnerScope(previous)
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
pub(crate) struct CaptureDocumentScope {
    active: bool,
    previous: Option<String>,
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
impl Drop for CaptureDocumentScope {
    fn drop(&mut self) {
        if self.active {
            CAPTURE_DOCUMENT.with(|current| current.replace(self.previous.take()));
        }
    }
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
pub(crate) fn document_scope(normalized_uri: &str) -> CaptureDocumentScope {
    if !capture::is_enabled() {
        return CaptureDocumentScope { active: false, previous: None };
    }
    let previous =
        CAPTURE_DOCUMENT.with(|current| current.replace(Some(normalized_uri.to_string())));
    CaptureDocumentScope { active: true, previous }
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
pub(crate) fn capture_owner() -> Option<usize> {
    CAPTURE_OWNER.with(std::cell::Cell::get)
}

#[cfg(not(any(test, feature = "expose_lsp_test_api")))]
pub(crate) fn capture_owner() -> Option<usize> {
    None
}

#[cfg(not(any(test, feature = "expose_lsp_test_api")))]
pub(crate) struct CaptureOwnerScope;

#[cfg(not(any(test, feature = "expose_lsp_test_api")))]
pub(crate) fn owner_scope(_owner: usize) -> CaptureOwnerScope {
    CaptureOwnerScope
}

#[cfg(not(any(test, feature = "expose_lsp_test_api")))]
pub(crate) struct CaptureDocumentScope;

#[cfg(not(any(test, feature = "expose_lsp_test_api")))]
pub(crate) fn document_scope(_normalized_uri: &str) -> CaptureDocumentScope {
    CaptureDocumentScope
}

/// Stable receipt schema id consumed by #7251 / #1373 work-volume fields.
#[cfg(any(test, feature = "expose_lsp_test_api"))]
pub const RECEIPT_SCHEMA_VERSION: &str = "text_sync_work_receipt.v1";

/// Whether instrumentation is active (env sink or in-process capture).
#[inline]
pub(crate) fn is_enabled() -> bool {
    env_enabled() || capture_enabled()
}

#[inline]
fn env_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| match std::env::var("PERL_LSP_TEXT_SYNC_WORK") {
        Ok(value) => {
            let v = value.trim();
            !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("off")
        }
        Err(_) => false,
    })
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
#[inline]
fn capture_enabled() -> bool {
    capture::is_enabled()
}

#[cfg(not(any(test, feature = "expose_lsp_test_api")))]
#[inline]
fn capture_enabled() -> bool {
    false
}

/// Logical event kinds required by #7301 (exact names in receipts use snake_case strings).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkEventKind {
    RopeEditApplied,
    RopeRebuilt,
    FullSourceMaterialized,
    FullSourceAllocation,
    FullSourceBytesCopied,
    SourceBufferShare,
    CanonicalDigestComputed,
    NoncanonicalFullHashComputed,
    DocumentsLockAcquired,
    DocumentsLockReleased,
    IndexingTransitionLockAcquired,
    IndexingTransitionLockReleased,
    ParseWorkerEnqueue,
    ParseWorkerCoalesced,
    ParseWorkerRejectedStale,
    ParsedSnapshotSourceIdentity,
    SourceBufferReleased,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkEvent {
    pub kind: WorkEventKind,
    /// Bounded uri tail only — never a host path or full URI with secrets.
    pub uri_tail: String,
    pub client_version: Option<i32>,
    pub generation: u32,
    pub stage: &'static str,
    pub owner: &'static str,
    pub byte_length: u64,
    pub sync_path: &'static str,
    pub disposition: &'static str,
}

impl WorkEvent {
    pub fn record(self) {
        if !is_enabled() {
            return;
        }
        push_event(self);
    }
}

fn push_event(event: WorkEvent) {
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    capture::push(event.clone());

    if env_enabled() {
        // Best-effort stderr JSONL; never touches LSP stdout or retains filenames.
        let mut log_event = event;
        log_event.uri_tail.clear();
        if let Ok(line) = serde_json::to_string(&log_event) {
            let _ = writeln!(std::io::stderr(), "{line}");
        }
    }
}

use std::io::Write;

/// Owns the real documents mutex guard, so its events share the guard's lifetime.
pub(crate) struct DocumentsLockScope<'a, T> {
    guard: parking_lot::MutexGuard<'a, T>,
    uri_tail: String,
    generation: u32,
    stage: &'static str,
    sync_path: &'static str,
    active: bool,
}

impl<'a, T> DocumentsLockScope<'a, T> {
    pub fn enter(
        mutex: &'a parking_lot::Mutex<T>,
        uri_tail: String,
        client_version: Option<i32>,
        generation: u32,
        stage: &'static str,
        sync_path: &'static str,
    ) -> Self {
        let guard = mutex.lock();
        let active = is_enabled();
        if active {
            WorkEvent {
                kind: WorkEventKind::DocumentsLockAcquired,
                uri_tail: uri_tail.clone(),
                client_version,
                generation,
                stage,
                owner: "documents",
                byte_length: 0,
                sync_path,
                disposition: "current",
            }
            .record();
        }
        Self { guard, uri_tail, generation, stage, sync_path, active }
    }

    /// Borrow the actual guard for APIs whose signature requires it.
    pub fn guard(&self) -> &parking_lot::MutexGuard<'a, T> {
        &self.guard
    }
}

impl<T> std::ops::Deref for DocumentsLockScope<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<T> std::ops::DerefMut for DocumentsLockScope<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl<T> Drop for DocumentsLockScope<'_, T> {
    fn drop(&mut self) {
        if self.active {
            WorkEvent {
                kind: WorkEventKind::DocumentsLockReleased,
                uri_tail: self.uri_tail.clone(),
                client_version: None,
                generation: self.generation,
                stage: self.stage,
                owner: "documents",
                byte_length: 0,
                sync_path: self.sync_path,
                disposition: "current",
            }
            .record();
        }
    }
}

pub(crate) fn record_rope_rebuilt(
    uri_tail: String,
    client_version: Option<i32>,
    generation: u32,
    sync_path: &'static str,
) {
    WorkEvent {
        kind: WorkEventKind::RopeRebuilt,
        uri_tail,
        client_version,
        generation,
        stage: "did_change_apply",
        owner: "document_state",
        byte_length: 0,
        sync_path,
        disposition: "current",
    }
    .record();
}

pub(crate) fn record_full_source_materialized(
    uri_tail: String,
    client_version: Option<i32>,
    generation: u32,
    byte_length: u64,
    stage: &'static str,
    sync_path: &'static str,
) {
    WorkEvent {
        kind: WorkEventKind::FullSourceMaterialized,
        uri_tail,
        client_version,
        generation,
        stage,
        owner: "document_state",
        byte_length,
        sync_path,
        disposition: "current",
    }
    .record();
}

pub(crate) fn record_full_source_allocation(
    uri_tail: String,
    client_version: Option<i32>,
    generation: u32,
    byte_length: u64,
    stage: &'static str,
    owner: &'static str,
    sync_path: &'static str,
) {
    WorkEvent {
        kind: WorkEventKind::FullSourceAllocation,
        uri_tail,
        client_version,
        generation,
        stage,
        owner,
        byte_length,
        sync_path,
        disposition: "current",
    }
    .record();
}

pub(crate) fn record_full_source_bytes_copied(
    uri_tail: String,
    client_version: Option<i32>,
    generation: u32,
    byte_length: u64,
    stage: &'static str,
    owner: &'static str,
    sync_path: &'static str,
) {
    WorkEvent {
        kind: WorkEventKind::FullSourceBytesCopied,
        uri_tail,
        client_version,
        generation,
        stage,
        owner,
        byte_length,
        sync_path,
        disposition: "current",
    }
    .record();
}

pub(crate) fn record_source_buffer_share(
    uri_tail: String,
    generation: u32,
    stage: &'static str,
    owner: &'static str,
    sync_path: &'static str,
) {
    WorkEvent {
        kind: WorkEventKind::SourceBufferShare,
        uri_tail,
        client_version: None,
        generation,
        stage,
        owner,
        byte_length: 0,
        sync_path,
        disposition: "current",
    }
    .record();
}

pub(crate) fn record_canonical_digest_computed(
    uri_tail: String,
    generation: u32,
    byte_length: u64,
    stage: &'static str,
    sync_path: &'static str,
) {
    WorkEvent {
        kind: WorkEventKind::CanonicalDigestComputed,
        uri_tail,
        client_version: None,
        generation,
        stage,
        owner: "parsed_snapshot",
        byte_length,
        sync_path,
        disposition: "current",
    }
    .record();
}

fn record_parse_worker_lifecycle(
    kind: WorkEventKind,
    uri_tail: String,
    generation: u32,
    byte_length: u64,
    sync_path: &'static str,
) {
    WorkEvent {
        kind,
        uri_tail,
        client_version: None,
        generation,
        stage: "parse_worker_enqueue",
        owner: "parse_worker",
        byte_length,
        sync_path,
        disposition: "current",
    }
    .record();
}

pub(crate) fn record_parse_worker_enqueue(
    uri_tail: String,
    generation: u32,
    byte_length: u64,
    sync_path: &'static str,
) {
    record_parse_worker_lifecycle(
        WorkEventKind::ParseWorkerEnqueue,
        uri_tail,
        generation,
        byte_length,
        sync_path,
    );
}

pub(crate) fn record_parse_worker_coalesced(uri_tail: String, generation: u32, byte_length: u64) {
    record_parse_worker_lifecycle(
        WorkEventKind::ParseWorkerCoalesced,
        uri_tail,
        generation,
        byte_length,
        "async",
    );
}

pub(crate) fn record_parse_worker_rejected_stale(
    uri_tail: String,
    generation: u32,
    byte_length: u64,
) {
    record_parse_worker_lifecycle(
        WorkEventKind::ParseWorkerRejectedStale,
        uri_tail,
        generation,
        byte_length,
        "async",
    );
}

pub(crate) fn record_accepted_parsed_snapshot(
    uri_tail: String,
    generation: u32,
    byte_length: u64,
    sync_path: &'static str,
) {
    record_canonical_digest_computed(
        uri_tail.clone(),
        generation,
        byte_length,
        "from_parse_result",
        sync_path,
    );
    record_full_source_allocation(
        uri_tail.clone(),
        None,
        generation,
        byte_length,
        "parsed_snapshot_source_arc",
        "parsed_snapshot",
        sync_path,
    );
    // Accepted snapshots own their source in a fresh Arc<str>. Preserve that
    // ownership model and account for the exact full-source copy at the same
    // acceptance boundary as the snapshot identity.
    record_full_source_bytes_copied(
        uri_tail.clone(),
        None,
        generation,
        byte_length,
        "parsed_snapshot_source_arc",
        "parsed_snapshot",
        sync_path,
    );
    WorkEvent {
        kind: WorkEventKind::ParsedSnapshotSourceIdentity,
        uri_tail,
        client_version: None,
        generation,
        stage: "parsed_snapshot",
        owner: "parsed_snapshot",
        byte_length,
        sync_path,
        disposition: "accepted",
    }
    .record();
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
mod receipt {
    use super::*;

    /// Aggregated receipt for one workload exercise.
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct TextSyncWorkReceipt {
        pub schema_version: String,
        pub claim_boundary: String,
        pub reproduction: String,
        pub workload: String,
        pub instrument_status: InstrumentStatus,
        pub accepted_generations: u32,
        pub events: Vec<WorkEventRecord>,
        pub totals: WorkTotals,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub parse_worker: Option<ParseWorkerTotals>,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum InstrumentStatus {
        Ok,
        NotProven,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct WorkEventRecord {
        pub kind: String,
        pub uri_tail: String,
        pub client_version: Option<i32>,
        pub generation: u32,
        pub stage: String,
        pub owner: String,
        pub byte_length: u64,
        pub sync_path: String,
        pub disposition: String,
    }

    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    pub struct WorkTotals {
        pub full_source_materializations: u64,
        pub full_source_allocations: u64,
        pub full_source_bytes_copied: u64,
        pub source_buffer_shares: u64,
        pub canonical_digest_computed: u64,
        pub documents_lock_acquire_release_pairs: u64,
    }

    #[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
    pub struct ParseWorkerTotals {
        pub jobs_enqueued: u64,
        pub jobs_coalesced: u64,
        pub jobs_rejected_stale: u64,
        pub jobs_published: u64,
    }

    fn kind_to_snake(kind: WorkEventKind) -> &'static str {
        match kind {
            WorkEventKind::RopeEditApplied => "rope_edit_applied",
            WorkEventKind::RopeRebuilt => "rope_rebuilt",
            WorkEventKind::FullSourceMaterialized => "full_source_materialized",
            WorkEventKind::FullSourceAllocation => "full_source_allocation",
            WorkEventKind::FullSourceBytesCopied => "full_source_bytes_copied",
            WorkEventKind::SourceBufferShare => "source_buffer_share",
            WorkEventKind::CanonicalDigestComputed => "canonical_digest_computed",
            WorkEventKind::NoncanonicalFullHashComputed => "noncanonical_full_hash_computed",
            WorkEventKind::DocumentsLockAcquired => "documents_lock_acquired",
            WorkEventKind::DocumentsLockReleased => "documents_lock_released",
            WorkEventKind::IndexingTransitionLockAcquired => "indexing_transition_lock_acquired",
            WorkEventKind::IndexingTransitionLockReleased => "indexing_transition_lock_released",
            WorkEventKind::ParseWorkerEnqueue => "parse_worker_enqueue",
            WorkEventKind::ParseWorkerCoalesced => "parse_worker_coalesced",
            WorkEventKind::ParseWorkerRejectedStale => "parse_worker_rejected_stale",
            WorkEventKind::ParsedSnapshotSourceIdentity => "parsed_snapshot_source_identity",
            WorkEventKind::SourceBufferReleased => "source_buffer_released",
        }
    }

    impl From<WorkEvent> for WorkEventRecord {
        fn from(event: WorkEvent) -> Self {
            WorkEventRecord {
                kind: kind_to_snake(event.kind).to_string(),
                uri_tail: event.uri_tail,
                client_version: event.client_version,
                generation: event.generation,
                stage: event.stage.to_string(),
                owner: event.owner.to_string(),
                byte_length: event.byte_length,
                sync_path: event.sync_path.to_string(),
                disposition: event.disposition.to_string(),
            }
        }
    }

    pub fn parse_worker_totals_for_events(events: &[WorkEvent]) -> Option<ParseWorkerTotals> {
        let mut found_worker_event = false;
        let mut totals = ParseWorkerTotals::default();
        for event in events {
            match event.kind {
                WorkEventKind::ParseWorkerEnqueue => {
                    found_worker_event = true;
                    totals.jobs_enqueued = totals.jobs_enqueued.saturating_add(1);
                }
                WorkEventKind::ParseWorkerCoalesced => {
                    found_worker_event = true;
                    totals.jobs_coalesced = totals.jobs_coalesced.saturating_add(1);
                }
                WorkEventKind::ParseWorkerRejectedStale => {
                    found_worker_event = true;
                    totals.jobs_rejected_stale = totals.jobs_rejected_stale.saturating_add(1);
                }
                WorkEventKind::ParsedSnapshotSourceIdentity if event.sync_path == "async" => {
                    found_worker_event = true;
                    totals.jobs_published = totals.jobs_published.saturating_add(1);
                }
                _ => {}
            }
        }
        found_worker_event.then_some(totals)
    }

    pub fn build_receipt(
        workload: &str,
        reproduction: &str,
        events: Vec<WorkEvent>,
        parse_worker: Option<ParseWorkerTotals>,
    ) -> TextSyncWorkReceipt {
        let instrument_status =
            if events.is_empty() { InstrumentStatus::NotProven } else { InstrumentStatus::Ok };

        let mut totals = WorkTotals::default();
        let mut accepted_generations = 0u32;
        let mut lock_acquired = 0u64;
        let mut lock_released = 0u64;
        let records: Vec<WorkEventRecord> = events
            .into_iter()
            .map(|event| {
                match event.kind {
                    WorkEventKind::ParsedSnapshotSourceIdentity => {
                        accepted_generations = accepted_generations.saturating_add(1)
                    }
                    WorkEventKind::FullSourceMaterialized => {
                        totals.full_source_materializations += 1
                    }
                    WorkEventKind::FullSourceAllocation => totals.full_source_allocations += 1,
                    WorkEventKind::FullSourceBytesCopied => {
                        totals.full_source_bytes_copied += event.byte_length
                    }
                    WorkEventKind::SourceBufferShare => totals.source_buffer_shares += 1,
                    WorkEventKind::CanonicalDigestComputed => totals.canonical_digest_computed += 1,
                    WorkEventKind::DocumentsLockAcquired => lock_acquired += 1,
                    WorkEventKind::DocumentsLockReleased => lock_released += 1,
                    _ => {}
                }
                WorkEventRecord::from(event)
            })
            .collect();
        totals.documents_lock_acquire_release_pairs = lock_acquired.min(lock_released);

        TextSyncWorkReceipt {
            schema_version: super::RECEIPT_SCHEMA_VERSION.to_string(),
            claim_boundary: "Accepted LSP commit→parse→snapshot source-owner accounting (#7301); excludes stale pre-publication snapshots and workspace-index reader clones (#17198)".to_string(),
            reproduction: reproduction.to_string(),
            workload: workload.to_string(),
            instrument_status,
            accepted_generations,
            events: records,
            totals,
            parse_worker,
        }
    }
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
pub use receipt::{
    InstrumentStatus, ParseWorkerTotals, TextSyncWorkReceipt, build_receipt,
    parse_worker_totals_for_events,
};

#[cfg(any(test, feature = "expose_lsp_test_api"))]
pub mod capture {
    use super::{CAPTURE_DOCUMENT, CAPTURE_OWNER, ParseWorkerTotals, WorkEvent};
    use parking_lot::{Mutex, MutexGuard};
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    static ENABLED: AtomicBool = AtomicBool::new(false);
    static SESSION: AtomicU64 = AtomicU64::new(0);

    struct CapturedEvent {
        event: WorkEvent,
        document: Option<String>,
    }

    #[derive(Default)]
    struct CaptureBuffer {
        events: Vec<CapturedEvent>,
        worker_baseline: Option<ParseWorkerTotals>,
        owner: Option<usize>,
        session: u64,
        active: bool,
    }

    fn buffer() -> &'static Mutex<CaptureBuffer> {
        static BUFFER: OnceLock<Mutex<CaptureBuffer>> = OnceLock::new();
        BUFFER.get_or_init(|| Mutex::new(CaptureBuffer::default()))
    }

    fn capture_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    /// Excludes overlapping captures across all servers in this process.
    pub struct CaptureGuard {
        _lock: MutexGuard<'static, ()>,
    }

    impl Drop for CaptureGuard {
        fn drop(&mut self) {
            let mut buffer = buffer().lock();
            buffer.active = false;
            ENABLED.store(false, Ordering::Release);
            buffer.events.clear();
            buffer.worker_baseline = None;
            buffer.owner = None;
        }
    }

    #[inline]
    pub fn is_enabled() -> bool {
        ENABLED.load(Ordering::Acquire)
    }

    pub fn test_lock(worker_baseline: impl FnOnce() -> Option<ParseWorkerTotals>) -> CaptureGuard {
        let lock = capture_lock().lock();
        let owner = CAPTURE_OWNER.with(std::cell::Cell::get);
        {
            let mut buffer = buffer().lock();
            buffer.active = false;
            ENABLED.store(false, Ordering::Release);
            buffer.session = SESSION.fetch_add(1, Ordering::AcqRel).saturating_add(1);
            buffer.events.clear();
            // The baseline and empty event buffer begin under the same capture
            // ownership and buffer lock; delayed producers still revalidate below.
            buffer.worker_baseline = worker_baseline();
            buffer.owner = owner;
            buffer.active = true;
            ENABLED.store(true, Ordering::Release);
        }
        CaptureGuard { _lock: lock }
    }

    #[cfg(test)]
    pub fn drain() -> (Vec<WorkEvent>, Option<ParseWorkerTotals>) {
        drain_inner(None)
    }

    pub fn drain_for_uri(normalized_uri: &str) -> (Vec<WorkEvent>, Option<ParseWorkerTotals>) {
        drain_inner(Some(normalized_uri))
    }

    fn drain_inner(normalized_uri: Option<&str>) -> (Vec<WorkEvent>, Option<ParseWorkerTotals>) {
        let mut buffer = buffer().lock();
        buffer.active = false;
        ENABLED.store(false, Ordering::Release);
        let events = std::mem::take(&mut buffer.events)
            .into_iter()
            .filter(|captured| {
                normalized_uri.is_none_or(|uri| captured.document.as_deref() == Some(uri))
            })
            .map(|captured| captured.event)
            .collect();
        (events, buffer.worker_baseline)
    }

    pub(super) fn push(event: WorkEvent) {
        if !is_enabled() {
            return;
        }
        let session = SESSION.load(Ordering::Acquire);
        let owner = CAPTURE_OWNER.with(std::cell::Cell::get);
        let document = CAPTURE_DOCUMENT.with(|current| current.borrow().clone());
        push_if_current(event, session, owner, document);
    }

    fn push_if_current(
        event: WorkEvent,
        session: u64,
        owner: Option<usize>,
        document: Option<String>,
    ) {
        let mut buffer = buffer().lock();
        if !buffer.active || buffer.session != session {
            return;
        }
        if buffer.owner.is_some() && buffer.owner != owner {
            return;
        }
        buffer.events.push(CapturedEvent { event, document });
    }

    #[cfg(test)]
    pub(super) fn baseline_locks_are_held_for_test() -> bool {
        buffer().try_lock().is_none() && capture_lock().try_lock().is_none()
    }

    #[cfg(test)]
    pub(super) fn session_for_test() -> u64 {
        SESSION.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(super) fn push_with_session_for_test(event: WorkEvent, session: u64, owner: Option<usize>) {
        let document = CAPTURE_DOCUMENT.with(|current| current.borrow().clone());
        push_if_current(event, session, owner, document);
    }
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
pub use capture::CaptureGuard;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn empty_capture_is_not_proven() {
        let _capture = capture::test_lock(|| None);
        let (events, baseline) = capture::drain();
        let receipt = build_receipt("test", "none", events, baseline);
        assert_eq!(receipt.instrument_status, InstrumentStatus::NotProven);
    }

    #[test]
    #[serial_test::serial]
    fn worker_baseline_is_sampled_under_capture_locks() {
        let _capture = capture::test_lock(|| {
            assert!(capture::baseline_locks_are_held_for_test());
            None
        });
    }

    #[test]
    #[serial_test::serial]
    fn arc_share_does_not_add_copied_bytes() {
        let _capture = capture::test_lock(|| None);
        record_source_buffer_share("file.pl".to_string(), 1, "enqueue", "parse_worker", "async");
        let (events, baseline) = capture::drain();
        let receipt = build_receipt("share", "unit", events, baseline);
        assert_eq!(receipt.totals.source_buffer_shares, 1);
        assert_eq!(receipt.totals.full_source_bytes_copied, 0);
    }

    #[test]
    #[serial_test::serial]
    fn accepted_snapshots_are_counted_at_publication_not_max_generation() {
        let _capture = capture::test_lock(|| None);
        record_accepted_parsed_snapshot("file.pl".to_string(), 40, 10, "async");
        record_accepted_parsed_snapshot("file.pl".to_string(), 3, 12, "async");
        record_full_source_allocation(
            "file.pl".to_string(),
            None,
            99,
            12,
            "worker_snapshot_source_arc",
            "parsed_snapshot",
            "async",
        );
        let (events, baseline) = capture::drain();
        let receipt = build_receipt("accepted", "unit", events, baseline);
        assert_eq!(receipt.accepted_generations, 2);
    }

    #[test]
    #[serial_test::serial]
    fn worker_totals_are_derived_from_document_events() {
        let _capture = capture::test_lock(|| None);
        record_parse_worker_enqueue("file.pl".to_string(), 4, 10, "async");
        record_parse_worker_coalesced("file.pl".to_string(), 5, 10);
        record_parse_worker_rejected_stale("file.pl".to_string(), 6, 10);
        record_accepted_parsed_snapshot("file.pl".to_string(), 7, 10, "async");

        let (events, _) = capture::drain();
        let totals = parse_worker_totals_for_events(&events).unwrap_or_default();

        assert_eq!(totals.jobs_enqueued, 1);
        assert_eq!(totals.jobs_coalesced, 1);
        assert_eq!(totals.jobs_rejected_stale, 1);
        assert_eq!(totals.jobs_published, 1);
    }

    #[test]
    #[serial_test::serial]
    fn capture_excludes_events_from_other_server_owners() {
        let _scope = owner_scope(1);
        let _capture = capture::test_lock(|| None);
        drop(_scope);
        {
            let _other_server = owner_scope(2);
            record_source_buffer_share(
                "same.pl".to_string(),
                1,
                "enqueue",
                "parse_worker",
                "async",
            );
        }
        {
            let _owning_server = owner_scope(1);
            record_source_buffer_share(
                "same.pl".to_string(),
                2,
                "enqueue",
                "parse_worker",
                "async",
            );
        }

        let (events, baseline) = capture::drain();
        let receipt = build_receipt("owners", "unit", events, baseline);

        assert_eq!(receipt.totals.source_buffer_shares, 1);
        assert_eq!(receipt.events.len(), 1);
        assert_eq!(receipt.events[0].generation, 2);
    }

    #[test]
    #[serial_test::serial]
    fn receipt_excludes_other_documents_on_the_capture_owner() {
        let _scope = owner_scope(1);
        let _capture = capture::test_lock(|| None);
        {
            let _document = document_scope("file:///a.pl");
            record_source_buffer_share("a.pl".to_string(), 1, "enqueue", "parse_worker", "async");
        }
        {
            let _document = document_scope("file:///b.pl");
            record_source_buffer_share("b.pl".to_string(), 2, "enqueue", "parse_worker", "async");
        }

        let (events, baseline) = capture::drain_for_uri("file:///a.pl");
        let receipt = build_receipt("documents", "unit", events, baseline);

        assert_eq!(receipt.events.len(), 1);
        assert_eq!(receipt.events[0].uri_tail, "a.pl");
    }

    #[test]
    #[serial_test::serial]
    fn delayed_producer_cannot_append_to_a_later_capture_session() {
        let _scope = owner_scope(7);
        let first_capture = capture::test_lock(|| None);
        let stale_session = capture::session_for_test();
        let _ = capture::drain();
        drop(first_capture);
        let _next_capture = capture::test_lock(|| None);

        capture::push_with_session_for_test(
            WorkEvent {
                kind: WorkEventKind::SourceBufferShare,
                uri_tail: "old.pl".to_string(),
                client_version: None,
                generation: 1,
                stage: "enqueue",
                owner: "parse_worker",
                byte_length: 0,
                sync_path: "async",
                disposition: "current",
            },
            stale_session,
            Some(7),
        );

        let (events, _) = capture::drain();
        assert!(events.is_empty(), "an earlier session's event contaminated the next receipt");
    }
}
