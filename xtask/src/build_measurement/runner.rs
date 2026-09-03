//! Cell preparation, execution, and digest computation for the executor
//! measurement harness (#11639).
//!
//! The harness prepares one declared [`MeasurementCell`], executes it through
//! injected providers, and produces one [`MeasurementRecord`] whose raw
//! facts and normalized interpretation carry separate digests. It never
//! repairs the measured wrappers, never alters their behavior, and never
//! selects an architecture: it is the measuring instrument only.

use super::model::{
    CacheAttribution, CacheCounters, CacheObservation, CommandIdentity, DEFAULT_TOLERANCE_NANOS,
    DiskAdmission, DiskRefusal, EnvironmentIdentity, FilesystemFreeSpace, FilesystemIdentity,
    LockObservation, LockPolicy, LockPrimitive, MeasurementCell, MeasurementRecord,
    PROTOCOL_VERSION, ProcessObservation, TimingDecomposition, WorkObservation,
};
use super::providers::{
    CacheMetricsProvider, ClockProvider, CommandRunner, CommandSpec, FilesystemProvider,
    LockPrimitiveProvider, ProcessObserver,
};
use crate::editor_host::sha256_bytes;
use color_eyre::eyre::{Result, eyre};
use serde_json::json;
use std::collections::BTreeMap;

/// Everything the harness needs to execute one declared cell beyond the cell
/// itself: the literal command, the probed environment identity, the cache
/// snapshot policy, and the expected selected work.
#[derive(Debug, Clone)]
pub struct CellExecution {
    pub command: CommandSpec,
    pub environment: EnvironmentIdentity,
    pub cache_snapshot: CacheSnapshotPolicy,
    pub expected_selected_work: Option<u64>,
}

/// Whether the harness resets counters before taking the baseline snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheSnapshotPolicy {
    ResetThenSnapshot,
    SnapshotOnly,
}

/// The measuring instrument: provider seams wired by the caller. Protocol
/// tests wire scripted providers; the native-host observation lanes
/// (#11640/#11641) wire real ones.
pub struct MeasurementHarness {
    pub clock: Box<dyn ClockProvider>,
    pub filesystems: Box<dyn FilesystemProvider>,
    pub locks: Box<dyn LockPrimitiveProvider>,
    pub process: Box<dyn ProcessObserver>,
    pub cache: Box<dyn CacheMetricsProvider>,
    pub commands: Box<dyn CommandRunner>,
}

/// Free-space measurements for every distinct filesystem among the declared
/// growth paths, sorted by filesystem identity, plus every declared-vs-actual
/// filesystem mismatch. The admission is only ever constructed from the
/// actual growth filesystems, never from a default path: each declared growth
/// path is resolved through the provider and a path that lands on a volume
/// other than its declared one is recorded as a refusal instead of silently
/// measured.
fn measure_disk_admission(
    filesystems: &dyn FilesystemProvider,
    canonical: &MeasurementCell,
) -> DiskAdmission {
    let mut mismatches = Vec::new();
    let mut measured: Vec<FilesystemIdentity> = Vec::new();
    for scope in &canonical.growth_paths {
        let actual = filesystems.filesystem_of(&scope.path);
        match actual {
            Some(actual) if actual == scope.filesystem => {
                if !measured.contains(&scope.filesystem) {
                    measured.push(scope.filesystem.clone());
                }
            }
            Some(actual) => mismatches.push(DiskRefusal::FilesystemMismatch {
                path: scope.path.clone(),
                declared: scope.filesystem.0.clone(),
                actual: actual.0,
            }),
            None => mismatches.push(DiskRefusal::UnresolvedFilesystem { path: scope.path.clone() }),
        }
    }
    let mut measurements = Vec::new();
    for filesystem in &measured {
        measurements.push(FilesystemFreeSpace {
            filesystem: filesystem.clone(),
            free_bytes: filesystems.free_bytes(filesystem),
        });
    }
    measurements.sort();
    mismatches.sort();
    mismatches.dedup();
    DiskAdmission { measurements, declared_path_mismatches: mismatches }
}

impl MeasurementHarness {
    /// Execute one declared cell and produce its measurement record.
    ///
    /// Phase boundaries (monotonic, exhaustive):
    /// preparation | admission (lock + disk) | execution (baseline snapshot +
    /// command + delta snapshot) | reporting (interpretation + digests). The
    /// phase sum equals the total wall time by construction; queue and lock
    /// wait stay inside the total.
    pub fn execute_cell(
        &mut self,
        cell: MeasurementCell,
        execution: CellExecution,
    ) -> Result<MeasurementRecord> {
        let canonical = cell.canonical();

        // Preparation (subject materialization boundary).
        let t0 = self.clock.monotonic_nanos();
        let t1 = self.clock.monotonic_nanos();

        // Admission: declared lock policy + actual disk admission.
        let lock = match canonical.lock_policy {
            LockPolicy::WholeProcessFlock => {
                let primitive = LockPrimitive::WholeProcessFlock;
                if self.locks.available(primitive) {
                    match self.locks.acquire(primitive) {
                        Some(wait_nanos) => LockObservation::Held { primitive, wait_nanos },
                        None => LockObservation::Unobserved,
                    }
                } else {
                    LockObservation::PrimitiveUnavailable
                }
            }
            LockPolicy::None => LockObservation::PolicyDeclaresNone,
        };
        let disk_admission = measure_disk_admission(self.filesystems.as_ref(), &canonical);
        let t2 = self.clock.monotonic_nanos();

        // Execution: baseline snapshot, command, delta snapshot. The cache
        // server/process identity is sampled with BOTH snapshots so a
        // restart or reconnection mid-cell can never produce an attributed
        // delta across two different servers.
        if execution.cache_snapshot == CacheSnapshotPolicy::ResetThenSnapshot {
            self.cache.reset();
        }
        let baseline_identity = self.cache.server_identity();
        let baseline = self.cache.snapshot();
        let outcome = self.commands.run(&execution.command);
        let delta = self.cache.snapshot();
        let delta_identity = self.cache.server_identity();
        let foreign_users_observed = self.cache.foreign_users_observed();
        let t3 = self.clock.monotonic_nanos();

        // Reporting: process observation, interpretation, and record
        // assembly all happen inside the measured reporting window.
        let process = self.process.observe().unwrap_or(ProcessObservation::InstrumentUnavailable);
        let cache = attribute_cache(
            baseline_identity,
            delta_identity,
            baseline,
            delta,
            foreign_users_observed,
        );

        let command_identity = CommandIdentity {
            program: execution.command.program.clone(),
            args: execution.command.args.clone(),
            effective_env: execution.command.env.clone(),
        };

        // Explicitly partial evidence: only the exact-candidate (commit)
        // identity is proven here; no other subject dimension is synthesized.
        let executed_subject_commit = outcome.executed_commit.clone();

        let work = WorkObservation {
            expected_selected: execution.expected_selected_work,
            observed_selected: outcome.selected_work,
            exit_code: outcome.exit_code,
        };

        let t4 = self.clock.monotonic_nanos();

        let timings = TimingDecomposition {
            preparation_nanos: Some(t1.saturating_sub(t0)),
            admission_wait_nanos: Some(t2.saturating_sub(t1)),
            execution_nanos: Some(t3.saturating_sub(t2)),
            reporting_nanos: Some(t4.saturating_sub(t3)),
            total_wall_nanos: Some(t4.saturating_sub(t0)),
            tolerance_nanos: DEFAULT_TOLERANCE_NANOS,
        };

        let mut record = MeasurementRecord {
            protocol_version: PROTOCOL_VERSION.to_string(),
            cell: canonical,
            command: command_identity,
            environment: execution.environment,
            timings,
            disk_admission: Some(disk_admission),
            lock,
            process,
            cache,
            work,
            executed_subject_commit,
            raw_digest: String::new(),
            normalized_digest: String::new(),
        };

        // Digest computation sits outside the phase decomposition by
        // construction: it is the harness's identity overhead, identical for
        // every cell and model, so excluding it uniformly cannot flatter any
        // candidate. The digests finalize the record's evidence chain; the
        // raw digest is recomputable by anyone via [`raw_facts_digest`].
        record.raw_digest = raw_facts_digest(&record)?;
        record.normalized_digest = normalized_digest(&record)?;

        Ok(record)
    }
}

/// Digest over the raw observed facts only — no model labels, no admission
/// verdicts. The #11642 decision successor can re-derive interpretation from
/// these facts without trusting the harness's preferred labels, and any
/// consumer can verify a record's raw digest by recomputation.
pub fn raw_facts_digest(record: &MeasurementRecord) -> Result<String> {
    let bytes = serde_json::to_vec(&json!({
    "command": record.command,
    "environment": record.environment,
    "timings": record.timings,
    "disk_admission": record.disk_admission,
    "lock": record.lock,
    "process": record.process,
    "cache": {
        "server_identity": record.cache.server_identity,
        "delta_server_identity": record.cache.delta_server_identity,
        "baseline": record.cache.baseline,
        "delta": record.cache.delta,
        "foreign_users_observed": record.cache.foreign_users_observed,
    },
    "work": record.work,
    "executed_subject_commit": record.executed_subject_commit,
    }))
    .map_err(|error| eyre!("serializing raw facts: {error}"))?;
    sha256_bytes(&bytes).map_err(|error| eyre!("digesting raw facts: {error}"))
}

/// Digest over the normalized interpretation: canonical cell identity plus
/// the admission outcome. Kept separate from the raw digest by construction
/// and by content. Recomputable by any consumer via this function.
pub fn normalized_digest(record: &MeasurementRecord) -> Result<String> {
    let bytes = serde_json::to_vec(&json!({
    "cell_id": record.cell.canonical_id(),
    "protocol_version": record.protocol_version,
    "verdict": record.admit(),
    }))
    .map_err(|error| eyre!("serializing interpretation: {error}"))?;
    sha256_bytes(&bytes).map_err(|error| eyre!("digesting interpretation: {error}"))
}

/// Attribution is decided from observed preconditions only: one known,
/// stable server identity across BOTH snapshots, fresh baseline and delta
/// snapshots, monotonically forward counters, and zero foreign users between
/// them. Environment variables never appear here.
fn attribute_cache(
    baseline_identity: Option<String>,
    delta_identity: Option<String>,
    baseline: Option<CacheCounters>,
    delta: Option<CacheCounters>,
    foreign_users_observed: u64,
) -> CacheObservation {
    let attribution = match (&baseline_identity, &delta_identity, &baseline, &delta) {
        (None, _, _, _) | (_, None, _, _) => CacheAttribution::Unattributed {
            reason: "cache server identity unresolved".to_string(),
        },
        (Some(before), Some(after), _, _) if before != after => CacheAttribution::Unattributed {
            reason: "cache server identity changed between snapshots".to_string(),
        },
        (_, _, None, _) | (_, _, _, None) => {
            CacheAttribution::Unattributed { reason: "counter snapshots incomplete".to_string() }
        }
        (Some(_), Some(_), Some(before), Some(after)) if !after.is_monotonic_after(before) => {
            CacheAttribution::Unattributed {
                reason: "counter regression observed (possible restart or unrelated reset)"
                    .to_string(),
            }
        }
        (Some(_), Some(_), Some(_), Some(_)) if foreign_users_observed > 0 => {
            CacheAttribution::Unattributed {
                reason: format!(
                    "{foreign_users_observed} unrelated users observed on the same server between snapshots"
                ),
            }
        }
        (Some(_), Some(_), Some(_), Some(_)) => CacheAttribution::Attributed,
    };
    CacheObservation {
        server_identity: baseline_identity,
        delta_server_identity: delta_identity,
        baseline,
        delta,
        foreign_users_observed,
        attribution,
    }
}

/// Convenience: default empty effective-environment map for scripted specs.
pub fn no_env() -> BTreeMap<String, String> {
    BTreeMap::new()
}
