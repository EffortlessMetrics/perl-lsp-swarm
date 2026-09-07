//! Cell preparation, execution, and digest computation for the executor
//! measurement harness (#11639).
//!
//! The harness prepares one declared [`MeasurementCell`], executes it through
//! injected providers, and produces one [`MeasurementRecord`] whose raw
//! facts and normalized interpretation carry separate digests. It never
//! repairs the measured wrappers, never alters their behavior, and never
//! selects an architecture: it is the measuring instrument only.

use super::model::{
    CacheAttribution, CacheCounters, CacheObservation, CellVerdict, CommandIdentity,
    DEFAULT_TOLERANCE_NANOS, DiskAdmission, DiskRefusal, EnvironmentIdentity, FilesystemFreeSpace,
    FilesystemIdentity, LockObservation, LockPolicy, LockPrimitive, MeasurementCell,
    MeasurementRecord, PROTOCOL_VERSION, ProcessObservation, TimingDecomposition, WorkObservation,
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

/// Subject materialization performed INSIDE the measured preparation phase
/// (passed per execution, never cloned away). Callers that materialize the
/// subject outside the harness pass `None`: the phase then stays honestly
/// zero-width instead of claiming measured work it never measured (#14739
/// review).
pub struct PreparationStep {
    /// What the step materializes, for provenance.
    pub description: String,
    /// The measured operation.
    pub operation: Box<dyn FnOnce()>,
}

impl std::fmt::Debug for PreparationStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparationStep").field("description", &self.description).finish()
    }
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
    /// command + delta snapshot) | reporting (interpretation + record
    /// assembly). The phase sum equals the total wall time by construction;
    /// queue and lock wait stay inside the total. The lock lease — when a
    /// lock is acquired — is held for the whole cell and released on return,
    /// so a whole-process lock actually covers execution (#14739 review). A
    /// monotonic-clock regression aborts the measurement with an error
    /// instead of emitting a zero-phase record that looks reconciled.
    pub fn execute_cell(
        &mut self,
        cell: MeasurementCell,
        execution: CellExecution,
        preparation: Option<PreparationStep>,
    ) -> Result<MeasurementRecord> {
        let canonical = cell.canonical();

        // Preparation (subject materialization boundary). Declared work runs
        // inside the phase; `None` keeps it zero-width.
        let t0 = self.clock.monotonic_nanos();
        if let Some(step) = preparation {
            (step.operation)();
        }
        let t1 = self.clock.monotonic_nanos();

        // Admission: declared lock policy + actual disk admission. A held
        // lease stays alive through the end of the cell.
        let lock_lease = match canonical.lock_policy {
            LockPolicy::WholeProcessFlock
                if self.locks.available(LockPrimitive::WholeProcessFlock) =>
            {
                self.locks.acquire(LockPrimitive::WholeProcessFlock)
            }
            LockPolicy::WholeProcessFlock => None,
            LockPolicy::None => None,
        };
        let lock = match (&lock_lease, canonical.lock_policy) {
            (Some(lease), LockPolicy::WholeProcessFlock) => LockObservation::Held {
                primitive: LockPrimitive::WholeProcessFlock,
                wait_nanos: lease.wait_nanos(),
            },
            (None, LockPolicy::WholeProcessFlock)
                if !self.locks.available(LockPrimitive::WholeProcessFlock) =>
            {
                LockObservation::PrimitiveUnavailable
            }
            (None, LockPolicy::WholeProcessFlock) => LockObservation::Unobserved,
            (_, LockPolicy::None) => LockObservation::PolicyDeclaresNone,
        };
        // A growth-path-free model (e.g. the environment-only wrapper) takes
        // NO disk admission: an empty one would read as "measured, nothing
        // found" for a surface the model never touches (#14739 review).
        let disk_admission = if canonical.execution_model.declares_growth_paths() {
            Some(measure_disk_admission(self.filesystems.as_ref(), &canonical))
        } else {
            None
        };
        let t2 = self.clock.monotonic_nanos();

        // Execution: baseline snapshot, command, delta snapshot. The cache
        // server/process identity is sampled with BOTH snapshots so a
        // restart or reconnection mid-cell can never produce an attributed
        // delta across two different servers. A failed reset is recorded:
        // `ResetThenSnapshot` promised a fresh baseline it did not get.
        let reset_confirmed = match execution.cache_snapshot {
            CacheSnapshotPolicy::ResetThenSnapshot => self.cache.reset(),
            CacheSnapshotPolicy::SnapshotOnly => true,
        };
        let baseline_identity = self.cache.server_identity();
        let baseline = self.cache.snapshot();
        let outcome = self.commands.run(&execution.command);
        let delta = self.cache.snapshot();
        let delta_identity = self.cache.server_identity();
        let foreign_users_observed = self.cache.foreign_users_observed();
        let t3 = self.clock.monotonic_nanos();

        // Reporting: process observation, interpretation, and record
        // assembly all happen inside the measured reporting window; the
        // phase boundary is taken AFTER assembly so the record's own
        // construction cost is included (#14739 review).
        let process = self.process.observe().unwrap_or(ProcessObservation::InstrumentUnavailable);
        let cache = attribute_cache(
            reset_confirmed,
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

        let mut record = MeasurementRecord {
            protocol_version: PROTOCOL_VERSION.to_string(),
            cell: canonical,
            command: command_identity,
            environment: execution.environment,
            timings: TimingDecomposition {
                preparation_nanos: None,
                admission_wait_nanos: None,
                execution_nanos: None,
                reporting_nanos: None,
                total_wall_nanos: None,
                tolerance_nanos: DEFAULT_TOLERANCE_NANOS,
            },
            disk_admission,
            lock,
            process,
            cache,
            work,
            executed_subject_commit,
            raw_digest: String::new(),
            normalized_digest: String::new(),
        };
        let t4 = self.clock.monotonic_nanos();

        // Phases come from checked subtraction: a clock that regressed
        // aborts the measurement instead of yielding zero-width phases that
        // still reconcile (#14739 review).
        let phase = |later: u64, earlier: u64| -> Result<u64> {
            later
                .checked_sub(earlier)
                .ok_or_else(|| eyre!("monotonic clock regressed: {earlier} -> {later}"))
        };
        record.timings = TimingDecomposition {
            preparation_nanos: Some(phase(t1, t0)?),
            admission_wait_nanos: Some(phase(t2, t1)?),
            execution_nanos: Some(phase(t3, t2)?),
            reporting_nanos: Some(phase(t4, t3)?),
            total_wall_nanos: Some(phase(t4, t0)?),
            tolerance_nanos: DEFAULT_TOLERANCE_NANOS,
        };

        // Digest computation sits outside the phase decomposition by
        // construction: it is the harness's identity overhead, identical for
        // every cell and model, so excluding it uniformly cannot flatter any
        // candidate. The digests finalize the record's evidence chain; the
        // raw digest is recomputable by anyone via [`raw_facts_digest`].
        record.raw_digest = raw_facts_digest(&record)?;
        record.normalized_digest = normalized_digest(&record)?;

        drop(lock_lease);
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

/// Digest over the normalized interpretation: canonical cell identity,
/// protocol version, the admission verdict, the attribution label, and the
/// executed-subject evidence boundary. Every interpretation-level claim is
/// digest-visible, so two records that interpret the same raw facts
/// differently never share a normalized digest (#14739 review).
pub fn normalized_digest(record: &MeasurementRecord) -> Result<String> {
    let verdict = record.admit();
    let proven_dimensions: Vec<&str> = match &verdict {
        // Full-subject proof covers every SubjectIdentity dimension.
        CellVerdict::Admitted => vec![
            "repository",
            "commit",
            "worktree",
            "package",
            "target_triple",
            "features",
            "default_features",
            "toolchain",
            "build_profile",
            "test_runner_profile",
        ],
        CellVerdict::AdmittedPartialSubject { proven_dimensions } => {
            proven_dimensions.iter().map(String::as_str).collect()
        }
        CellVerdict::NotProven { .. } => Vec::new(),
    };
    let bytes = serde_json::to_vec(&json!({
    "cell_id": record.cell.canonical_id(),
    "protocol_version": record.protocol_version,
    "executed_subject_evidence": {
        "executed_commit": record.executed_subject_commit,
        "proven_dimensions": proven_dimensions,
    },
    "cache_attribution": record.cache.attribution,
    "verdict": verdict,
    }))
    .map_err(|error| eyre!("serializing interpretation: {error}"))?;
    sha256_bytes(&bytes).map_err(|error| eyre!("digesting interpretation: {error}"))
}

/// Attribution is decided from observed preconditions only: a confirmed
/// reset under `ResetThenSnapshot`, one known, stable server identity across
/// BOTH snapshots, fresh baseline and delta snapshots, monotonically forward
/// counters, and an observed-zero foreign-user count between them.
/// Environment variables never appear here.
fn attribute_cache(
    reset_confirmed: bool,
    baseline_identity: Option<String>,
    delta_identity: Option<String>,
    baseline: Option<CacheCounters>,
    delta: Option<CacheCounters>,
    foreign_users_observed: Option<u64>,
) -> CacheObservation {
    let attribution = if !reset_confirmed {
        CacheAttribution::Unattributed {
            reason: "cache reset failed before the baseline snapshot; counters are not fresh"
                .to_string(),
        }
    } else {
        match (&baseline_identity, &delta_identity, &baseline, &delta, &foreign_users_observed) {
            (None, _, _, _, _) | (_, None, _, _, _) => CacheAttribution::Unattributed {
                reason: "cache server identity unresolved".to_string(),
            },
            (Some(before), Some(after), _, _, _) if before != after => {
                CacheAttribution::Unattributed {
                    reason: "cache server identity changed between snapshots".to_string(),
                }
            }
            (_, _, None, _, _) | (_, _, _, None, _) => CacheAttribution::Unattributed {
                reason: "counter snapshots incomplete".to_string(),
            },
            (_, _, Some(before), Some(after), _) if !after.is_monotonic_after(before) => {
                CacheAttribution::Unattributed {
                    reason: "counter regression observed (possible restart or unrelated reset)"
                        .to_string(),
                }
            }
            // An unavailable foreign-user instrument is unobserved
            // isolation, not observed zero (#14739 review).
            (_, _, _, _, None) => CacheAttribution::Unattributed {
                reason: "foreign-user instrument unavailable; isolation unobserved".to_string(),
            },
            (_, _, _, _, Some(users)) => {
                if *users > 0 {
                    CacheAttribution::Unattributed {
                        reason: format!(
                            "{users} unrelated users observed on the same server between snapshots"
                        ),
                    }
                } else {
                    CacheAttribution::Attributed
                }
            }
        }
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
