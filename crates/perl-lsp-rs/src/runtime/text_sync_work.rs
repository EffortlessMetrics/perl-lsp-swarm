//! Deterministic full-source work accounting for the text-sync → parse → snapshot path (#7301).
//!
//! Opt-in like [`super::timing`]: zero allocation on the hot path when disabled. Tests and
//! `expose_lsp_test_api` builds use an in-process capture buffer independent of environment.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

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
        // Best-effort stderr JSONL; never touches LSP stdout.
        if let Ok(line) = serde_json::to_string(&event) {
            let _ = writeln!(std::io::stderr(), "{line}");
        }
    }
}

use std::io::Write;

/// Records [`WorkEventKind::DocumentsLockReleased`] when dropped, after the real mutex guard.
pub(crate) struct DocumentsLockScope {
    uri_tail: String,
    generation: u32,
    stage: &'static str,
    sync_path: &'static str,
    active: bool,
}

impl DocumentsLockScope {
    pub fn enter(
        uri_tail: String,
        client_version: Option<i32>,
        generation: u32,
        stage: &'static str,
        sync_path: &'static str,
    ) -> Self {
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
        Self { uri_tail, generation, stage, sync_path, active }
    }
}

impl Drop for DocumentsLockScope {
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

pub(crate) fn record_rope_edit_applied(
    uri_tail: String,
    client_version: Option<i32>,
    generation: u32,
    sync_path: &'static str,
) {
    WorkEvent {
        kind: WorkEventKind::RopeEditApplied,
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

pub(crate) fn record_parse_worker_enqueue(
    uri_tail: String,
    generation: u32,
    byte_length: u64,
    sync_path: &'static str,
) {
    WorkEvent {
        kind: WorkEventKind::ParseWorkerEnqueue,
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

pub(crate) fn record_parsed_snapshot_source_identity(
    uri_tail: String,
    generation: u32,
    byte_length: u64,
    sync_path: &'static str,
) {
    WorkEvent {
        kind: WorkEventKind::ParsedSnapshotSourceIdentity,
        uri_tail,
        client_version: None,
        generation,
        stage: "parsed_snapshot",
        owner: "parsed_snapshot",
        byte_length,
        sync_path,
        disposition: "current",
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

    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    pub struct ParseWorkerTotals {
        pub jobs_enqueued: u64,
        pub jobs_coalesced: u64,
        pub jobs_rejected_stale: u64,
        pub jobs_published: u64,
    }

    fn kind_to_snake(kind: WorkEventKind) -> &'static str {
        match kind {
            WorkEventKind::RopeEditApplied => "rope_edit_applied",
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

    pub fn build_receipt(
        workload: &str,
        reproduction: &str,
        events: Vec<WorkEvent>,
        parse_worker: Option<ParseWorkerTotals>,
    ) -> TextSyncWorkReceipt {
        let instrument_status =
            if events.is_empty() { InstrumentStatus::NotProven } else { InstrumentStatus::Ok };

        let mut totals = WorkTotals::default();
        let mut max_generation = 0u32;
        let mut lock_acquired = 0u64;
        let mut lock_released = 0u64;
        let records: Vec<WorkEventRecord> = events
            .into_iter()
            .map(|event| {
                max_generation = max_generation.max(event.generation);
                match event.kind {
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
            claim_boundary: "LSP commit→parse→snapshot source-owner accounting (#7301); excludes workspace-index reader clones (#17198)".to_string(),
            reproduction: reproduction.to_string(),
            workload: workload.to_string(),
            instrument_status,
            accepted_generations: max_generation,
            events: records,
            totals,
            parse_worker,
        }
    }
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
pub use receipt::{
    InstrumentStatus, ParseWorkerTotals, TextSyncWorkReceipt, WorkEventRecord, WorkTotals,
    build_receipt,
};

#[cfg(any(test, feature = "expose_lsp_test_api"))]
pub mod capture {
    use super::WorkEvent;
    use parking_lot::Mutex;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};

    static ENABLED: AtomicBool = AtomicBool::new(false);

    fn buffer() -> &'static Mutex<Vec<WorkEvent>> {
        static BUFFER: OnceLock<Mutex<Vec<WorkEvent>>> = OnceLock::new();
        BUFFER.get_or_init(|| Mutex::new(Vec::new()))
    }

    #[inline]
    pub fn is_enabled() -> bool {
        ENABLED.load(Ordering::Relaxed)
    }

    pub fn start() {
        buffer().lock().clear();
        ENABLED.store(true, Ordering::Relaxed);
    }

    pub fn drain() -> Vec<WorkEvent> {
        ENABLED.store(false, Ordering::Relaxed);
        std::mem::take(&mut *buffer().lock())
    }

    pub(super) fn push(event: WorkEvent) {
        if is_enabled() {
            buffer().lock().push(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn empty_capture_is_not_proven() {
        capture::start();
        let receipt = build_receipt("test", "none", capture::drain(), None);
        assert_eq!(receipt.instrument_status, InstrumentStatus::NotProven);
    }

    #[test]
    #[serial_test::serial]
    fn arc_share_does_not_add_copied_bytes() {
        capture::start();
        record_source_buffer_share("file.pl".to_string(), 1, "enqueue", "parse_worker", "async");
        let receipt = build_receipt("share", "unit", capture::drain(), None);
        assert_eq!(receipt.totals.source_buffer_shares, 1);
        assert_eq!(receipt.totals.full_source_bytes_copied, 0);
    }
}
