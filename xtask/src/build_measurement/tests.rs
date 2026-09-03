//! Falsifier suite for the executor measurement protocol (#11639).
//!
//! Each test encodes one shift-left falsifier from the issue or one decision
//! law from controller #9547, exercised through scripted fixture providers
//! (no real Cargo, sccache, sleeps, or host specifics). Names state the
//! falsified claim so review can map findings back to the spec packet.

use super::model::{
    CacheAttribution, CacheCounters, CapacityPolicy, CellVerdict, DiskAdmission,
    EnvironmentIdentity, ExecutionModel, FilesystemIdentity, HostProfile, LockObservation,
    LockPolicy, MeasurementCell, MeasurementRecord, NotProvenReason, Operation, PROTOCOL_VERSION,
    PathRole, PathScope, ProcessObservation, RepetitionOrdinal, RowRefusal, SubjectIdentity,
    Terminality, TimingDecomposition, TimingVerdict, WorkObservation, WorkflowClass,
};
use super::providers::{
    CommandOutcome, CommandSpec, DeterministicBarrier, ScriptedCache, ScriptedClock,
    ScriptedFilesystems, ScriptedLocks, ScriptedProcess, ScriptedRunner,
};
use super::runner::{CacheSnapshotPolicy, CellExecution, MeasurementHarness};
use super::{render_human, render_json};
use color_eyre::eyre::{Result, eyre};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

const COMMIT_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const COMMIT_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn fixture_subject(commit: &str) -> SubjectIdentity {
    SubjectIdentity {
        repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
        commit: commit.to_string(),
        worktree: "wt-a".to_string(),
        package: "perl-lsp-rs-core".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        features: BTreeSet::from(["feat-a".to_string(), "feat-b".to_string()]),
        default_features: true,
        toolchain: "stable-1.00".to_string(),
        build_profile: "dev".to_string(),
        test_runner_profile: Some("nextest-ci".to_string()),
    }
}

fn fixture_growth_paths() -> Vec<PathScope> {
    vec![PathScope {
        role: PathRole::Target,
        path: "/wt-a/target".to_string(),
        filesystem: FilesystemIdentity("devplane-a".to_string()),
    }]
}

#[allow(clippy::too_many_arguments)]
fn fixture_cell(
    execution_model: ExecutionModel,
    workflow_class: WorkflowClass,
    operation: Operation,
    subject: SubjectIdentity,
    host: HostProfile,
    lock_policy: LockPolicy,
) -> MeasurementCell {
    MeasurementCell {
        workflow_class,
        execution_model,
        operation,
        subject,
        host,
        ordinal: RepetitionOrdinal { cold: true, repetition: 0 },
        growth_paths: fixture_growth_paths(),
        lock_policy,
        capacity: CapacityPolicy::CandidatePrivate,
    }
}

fn scripted_clock() -> ScriptedClock {
    // t0=0 | t1=1_000 (preparation) | t2=2_000 (admission) | t3=12_000
    // (after command) | t4=12_200 (after reporting): phase sum 12_200 ==
    // total 12_200, exactly reconciled.
    ScriptedClock::new(vec![0, 1_000, 2_000, 12_000, 12_200])
}

fn matching_filesystems() -> ScriptedFilesystems {
    let mut path_map = BTreeMap::new();
    path_map.insert("/wt-a/target".to_string(), FilesystemIdentity("devplane-a".to_string()));
    let mut free_map = BTreeMap::new();
    free_map.insert("devplane-a".to_string(), Some(1_000_000_000u64));
    ScriptedFilesystems::new(path_map, free_map)
}

fn clean_process() -> ScriptedProcess {
    ScriptedProcess {
        observation: Some(ProcessObservation::Observed {
            descendant_count: 0,
            terminality: Terminality::Clean,
        }),
    }
}

fn attributed_cache_snapshots() -> Vec<CacheCounters> {
    vec![
        CacheCounters { requests: 100, hits: 40, misses: 60, non_cacheable: 0 },
        CacheCounters { requests: 110, hits: 47, misses: 63, non_cacheable: 0 },
    ]
}

fn successful_outcome(commit: &str) -> CommandOutcome {
    CommandOutcome {
        exit_code: Some(0),
        selected_work: Some(4),
        executed_commit: Some(commit.to_string()),
    }
}

fn fixture_command() -> CommandSpec {
    let mut env = BTreeMap::new();
    env.insert("DEVPLANE".to_string(), "devplane-a".to_string());
    CommandSpec {
        program: "cargo".to_string(),
        args: vec!["test".to_string(), "--exact".to_string(), "pkg::t".to_string()],
        env,
    }
}

fn proof_execution() -> CellExecution {
    CellExecution {
        command: fixture_command(),
        environment: EnvironmentIdentity {
            cargo_version: Some("1.00".to_string()),
            rustc_version: Some("1.00".to_string()),
            host_triple: Some("x86_64-unknown-linux-gnu".to_string()),
        },
        cache_snapshot: CacheSnapshotPolicy::ResetThenSnapshot,
        expected_selected_work: Some(4),
    }
}

type HarnessParts = (
    ScriptedClock,
    ScriptedFilesystems,
    ScriptedLocks,
    ScriptedProcess,
    ScriptedCache,
    ScriptedRunner,
);

fn standard_parts(commit: &str) -> HarnessParts {
    (
        scripted_clock(),
        matching_filesystems(),
        ScriptedLocks { flock_available: true, acquire_wait_nanos: 500 },
        clean_process(),
        ScriptedCache::new(
            Some("sccache://fixture-1".to_string()),
            attributed_cache_snapshots(),
            0,
        ),
        ScriptedRunner::new(vec![successful_outcome(commit)]),
    )
}

fn harness_from_parts(
    clock: ScriptedClock,
    filesystems: ScriptedFilesystems,
    locks: ScriptedLocks,
    process: ScriptedProcess,
    cache: ScriptedCache,
    commands: ScriptedRunner,
) -> MeasurementHarness {
    MeasurementHarness {
        clock: Box::new(clock),
        filesystems: Box::new(filesystems),
        locks: Box::new(locks),
        process: Box::new(process),
        cache: Box::new(cache),
        commands: Box::new(commands),
    }
}

/// A fully admitting shared-cache proof cell: the positive control every
/// falsifier is measured against.
fn admitted_shared_cache_record() -> Result<MeasurementRecord> {
    let (clock, filesystems, locks, process, cache, commands) = standard_parts(COMMIT_A);
    let mut harness = harness_from_parts(clock, filesystems, locks, process, cache, commands);
    let cell = fixture_cell(
        ExecutionModel::PrivateTargetSharedCargoSccache,
        WorkflowClass::Proof,
        Operation::ExactTest,
        fixture_subject(COMMIT_A),
        HostProfile::NativePosix,
        LockPolicy::None,
    );
    harness
        .execute_cell(cell, proof_execution())
        .map_err(|error| eyre!("fixture cell executes: {error}"))
}

/// Execute a cell through a fully wired standard harness, mapping harness
/// construction errors into test errors (the workspace denies `expect`).
fn executed(result: color_eyre::Result<MeasurementRecord>) -> Result<MeasurementRecord> {
    result.map_err(|error| eyre!("fixture cell executes: {error}"))
}

fn reasons_of(record: &MeasurementRecord) -> Vec<NotProvenReason> {
    match record.admit() {
        CellVerdict::Admitted => Vec::new(),
        CellVerdict::NotProven { reasons } => reasons,
    }
}

#[test]
fn positive_control_cell_is_admitted() -> Result<()> {
    let record = admitted_shared_cache_record()?;
    assert_eq!(record.protocol_version, PROTOCOL_VERSION);
    let verdict = record.admit();
    assert!(verdict.is_admitted(), "positive control must admit, got {verdict:?}");
    assert!(record.cache.clean_delta().is_some());
    Ok(())
}

/// Falsifier: one row representing all current `cargo-safe` behavior. The
/// direct-leaf and xtask-environment-only invocation shapes are materially
/// different systems; the harness must record different active controls for
/// the same subject.
#[test]
fn current_wrapper_rows_preserve_the_direct_leaf_vs_xtask_split() -> Result<()> {
    // Direct leaf: declares the whole-process flock; the host lacks flock in
    // this scenario, so the row is NOT_PROVEN — never a locked success.
    let (clock, filesystems, _locks, process, cache, commands) = standard_parts(COMMIT_A);
    let locks = ScriptedLocks { flock_available: false, acquire_wait_nanos: 0 };
    let mut harness = harness_from_parts(clock, filesystems, locks, process, cache, commands);
    let leaf_cell = fixture_cell(
        ExecutionModel::CargoSafeDirectLeaf,
        WorkflowClass::Construction,
        Operation::Check,
        fixture_subject(COMMIT_A),
        HostProfile::NativePosix,
        LockPolicy::WholeProcessFlock,
    );
    let leaf_record = executed(harness.execute_cell(leaf_cell, proof_execution()))?;
    assert_eq!(leaf_record.lock, LockObservation::PrimitiveUnavailable);
    assert!(reasons_of(&leaf_record).contains(&NotProvenReason::LockNotAdmitted));

    // xtask environment-only: honestly declares no lock and no disk
    // admission, and its canonical identity differs from the leaf row.
    let (clock, filesystems, locks, process, cache, commands) = standard_parts(COMMIT_A);
    let mut harness = harness_from_parts(clock, filesystems, locks, process, cache, commands);
    let xtask_cell = fixture_cell(
        ExecutionModel::CargoSafeXtaskEnvironmentOnly,
        WorkflowClass::Orchestration,
        Operation::Check,
        fixture_subject(COMMIT_A),
        HostProfile::NativePosix,
        LockPolicy::None,
    );
    let xtask_record = executed(harness.execute_cell(xtask_cell, proof_execution()))?;
    assert_eq!(xtask_record.lock, LockObservation::PolicyDeclaresNone);
    assert_ne!(
        leaf_record.cell.canonical_id(),
        xtask_record.cell.canonical_id(),
        "the two current wrapper rows must never collapse into one identity"
    );
    Ok(())
}

/// Falsifier: a shared target/cache hit from another candidate accepted as
/// correctness. Cache statistics can look excellent while the cell executed
/// another candidate's artifact; the executed-subject oracle must fail the
/// cell regardless of the cache delta (decision law 1).
#[test]
fn false_cache_hit_cannot_substitute_for_subject_identity() -> Result<()> {
    let (clock, filesystems, locks, process, cache, _commands) = standard_parts(COMMIT_A);
    // The command ran candidate A's already-built test binary while the cell
    // declared candidate B.
    let commands = ScriptedRunner::new(vec![successful_outcome(COMMIT_A)]);
    let mut harness = harness_from_parts(clock, filesystems, locks, process, cache, commands);
    let cell = fixture_cell(
        ExecutionModel::PrivateTargetSharedCargoSccache,
        WorkflowClass::Proof,
        Operation::ExactTest,
        fixture_subject(COMMIT_B),
        HostProfile::NativePosix,
        LockPolicy::None,
    );
    let record = executed(harness.execute_cell(cell, proof_execution()))?;
    assert!(record.cache.clean_delta().is_some(), "cache statistics alone look excellent");
    let reasons = reasons_of(&record);
    assert!(
        reasons
            .contains(&NotProvenReason::ExecutedSubjectMismatch { detail: "commit".to_string() }),
        "expected executed-subject mismatch, got {reasons:?}"
    );
    Ok(())
}

/// Falsifier: different subjects compared as one matched pair. A record from
/// a changed toolchain (candidate, profile, features, ...) never satisfies a
/// row requiring the original subject.
#[test]
fn record_from_different_subject_does_not_satisfy_row() -> Result<()> {
    let record = admitted_shared_cache_record()?;
    let mut required = fixture_subject(COMMIT_A);
    required.toolchain = "stable-2.00".to_string();
    let refusal = record
        .satisfies_row(&required, &HostProfile::NativePosix)
        .err()
        .ok_or_else(|| eyre!("expected subject mismatch, got Ok"))?;
    match refusal {
        RowRefusal::SubjectMismatch { differing_fields } => {
            assert_eq!(differing_fields, vec!["toolchain".to_string()]);
        }
        other => return Err(eyre!("expected subject mismatch, got {other:?}")),
    }
    // The unmutated subject does satisfy its own row.
    let required = fixture_subject(COMMIT_A);
    assert!(record.satisfies_row(&required, &HostProfile::NativePosix).is_ok());
    Ok(())
}

/// Falsifier: lock wait omitted from total elapsed time. A record missing
/// the admission/queue/lock wait segment fails reconciliation.
#[test]
fn omitted_wait_segment_fails_reconciliation() -> Result<()> {
    let mut record = admitted_shared_cache_record()?;
    record.timings = TimingDecomposition {
        preparation_nanos: Some(1_000),
        admission_wait_nanos: None,
        execution_nanos: Some(10_000),
        reporting_nanos: Some(200),
        total_wall_nanos: Some(12_200),
        tolerance_nanos: 1_000_000,
    };
    match record.timings.reconcile() {
        TimingVerdict::Incomplete { missing } => {
            assert_eq!(missing, vec!["admission_wait".to_string()])
        }
        other => return Err(eyre!("expected incomplete reconciliation, got {other:?}")),
    }
    assert!(reasons_of(&record).contains(&NotProvenReason::TimingIncomplete {
        missing: vec!["admission_wait".to_string()]
    }));
    Ok(())
}

/// Phase sums that disagree with the declared total beyond the tolerance are
/// refused; overlapping timers are never subtracted into a "real" phase.
#[test]
fn timing_mismatch_beyond_tolerance_is_refused() -> Result<()> {
    let mut record = admitted_shared_cache_record()?;
    record.timings.total_wall_nanos =
        Some(record.timings.total_wall_nanos.unwrap_or(0) + 50_000_000);
    match record.timings.reconcile() {
        TimingVerdict::Mismatch { computed_sum_nanos, declared_total_nanos } => {
            assert_eq!(computed_sum_nanos, 12_200);
            assert_eq!(declared_total_nanos, 50_012_200);
        }
        other => return Err(eyre!("expected mismatch, got {other:?}")),
    }
    assert!(
        reasons_of(&record)
            .iter()
            .any(|reason| matches!(reason, NotProvenReason::TimingMismatch { .. }))
    );
    Ok(())
}

/// Falsifier: disk admission checks a default devplane while actual selected
/// paths grow another filesystem. A path that resolves to a volume other
/// than its declared one refuses the admission.
#[test]
fn growth_on_another_filesystem_than_declared_is_refused() -> Result<()> {
    // The provider resolves the target path to devplane-b while the cell
    // declares devplane-a: measuring devplane-a would have authorized
    // growth on devplane-b.
    let mut path_map = BTreeMap::new();
    path_map.insert("/wt-a/target".to_string(), FilesystemIdentity("devplane-b".to_string()));
    let mut free_map = BTreeMap::new();
    free_map.insert("devplane-b".to_string(), Some(500_000_000u64));
    let filesystems = ScriptedFilesystems::new(path_map, free_map);
    let (clock, _fs, locks, process, cache, commands) = standard_parts(COMMIT_A);
    let mut harness = harness_from_parts(clock, filesystems, locks, process, cache, commands);
    let cell = fixture_cell(
        ExecutionModel::PrivateTargetSharedCargoSccache,
        WorkflowClass::Proof,
        Operation::ExactTest,
        fixture_subject(COMMIT_A),
        HostProfile::NativePosix,
        LockPolicy::None,
    );
    let record = executed(harness.execute_cell(cell, proof_execution()))?;
    assert!(
        reasons_of(&record)
            .iter()
            .any(|reason| matches!(reason, NotProvenReason::DiskAdmissionRefused { .. }))
    );
    Ok(())
}

/// A failed free-space measurement on an actual growth filesystem stays
/// `NOT_PROVEN`; it never becomes "infinite" or "unchecked is fine".
#[test]
fn failed_free_space_measurement_is_not_proven() -> Result<()> {
    let mut free_map = BTreeMap::new();
    free_map.insert("devplane-a".to_string(), None);
    let filesystems = ScriptedFilesystems::new(
        BTreeMap::from([(
            "/wt-a/target".to_string(),
            FilesystemIdentity("devplane-a".to_string()),
        )]),
        free_map,
    );
    let (clock, _fs, locks, process, cache, commands) = standard_parts(COMMIT_A);
    let mut harness = harness_from_parts(clock, filesystems, locks, process, cache, commands);
    let cell = fixture_cell(
        ExecutionModel::RawPrivateWorktree,
        WorkflowClass::Construction,
        Operation::Check,
        fixture_subject(COMMIT_A),
        HostProfile::NativePosix,
        LockPolicy::None,
    );
    let record = executed(harness.execute_cell(cell, proof_execution()))?;
    assert!(
        reasons_of(&record)
            .iter()
            .any(|reason| matches!(reason, NotProvenReason::DiskAdmissionRefused { .. }))
    );
    Ok(())
}

/// Falsifier: missing `flock` silently represented as the locked model.
/// The direct-leaf row without the primitive records `PrimitiveUnavailable`
/// and the human render spells out `not_proven`.
#[test]
fn missing_lock_primitive_is_never_a_locked_success() -> Result<()> {
    let (clock, filesystems, _locks, process, cache, commands) = standard_parts(COMMIT_A);
    let locks = ScriptedLocks { flock_available: false, acquire_wait_nanos: 0 };
    let mut harness = harness_from_parts(clock, filesystems, locks, process, cache, commands);
    let cell = fixture_cell(
        ExecutionModel::CargoSafeDirectLeaf,
        WorkflowClass::Proof,
        Operation::Test,
        fixture_subject(COMMIT_A),
        HostProfile::NativePosix,
        LockPolicy::WholeProcessFlock,
    );
    let record = executed(harness.execute_cell(cell, proof_execution()))?;
    assert_eq!(record.lock, LockObservation::PrimitiveUnavailable);
    assert!(!record.lock.is_admitted());
    let human = render_human(&record);
    assert!(
        human.contains("not_proven (lock primitive unavailable"),
        "render must surface the missing primitive, got:\n{human}"
    );
    Ok(())
}

/// Falsifier: a zero-test run represented as successful proof. Exit zero
/// with zero selected work cannot satisfy a proof cell.
#[test]
fn zero_selected_work_is_not_proven_despite_exit_success() -> Result<()> {
    let (clock, filesystems, locks, process, cache, _commands) = standard_parts(COMMIT_A);
    let commands = ScriptedRunner::new(vec![CommandOutcome {
        exit_code: Some(0),
        selected_work: Some(0),
        executed_commit: Some(COMMIT_A.to_string()),
    }]);
    let mut harness = harness_from_parts(clock, filesystems, locks, process, cache, commands);
    let cell = fixture_cell(
        ExecutionModel::RawPrivateWorktree,
        WorkflowClass::Proof,
        Operation::Test,
        fixture_subject(COMMIT_A),
        HostProfile::NativePosix,
        LockPolicy::None,
    );
    let record = executed(harness.execute_cell(cell, proof_execution()))?;
    assert!(
        reasons_of(&record).contains(&NotProvenReason::SelectedWorkUnproven {
            expected: Some(4),
            observed: Some(0),
        })
    );
    Ok(())
}

/// Falsifier: WSL/Git Bash fills the native-Windows row. Host profiles stay
/// distinct in both directions.
#[test]
fn wsl_or_git_bash_evidence_cannot_fill_native_windows_row() -> Result<()> {
    let (clock, filesystems, locks, process, cache, commands) = standard_parts(COMMIT_A);
    let mut harness = harness_from_parts(clock, filesystems, locks, process, cache, commands);
    let cell = fixture_cell(
        ExecutionModel::RawPrivateWorktree,
        WorkflowClass::Proof,
        Operation::Test,
        fixture_subject(COMMIT_A),
        HostProfile::WslOrGitBash,
        LockPolicy::None,
    );
    let record = executed(harness.execute_cell(cell, proof_execution()))?;
    let required = fixture_subject(COMMIT_A);
    let refusal = record
        .satisfies_row(&required, &HostProfile::NativeWindows)
        .err()
        .ok_or_else(|| eyre!("expected host mismatch, got Ok"))?;
    match refusal {
        RowRefusal::HostMismatch { record_host, required_host } => {
            assert_eq!(record_host, HostProfile::WslOrGitBash);
            assert_eq!(required_host, HostProfile::NativeWindows);
        }
        other => return Err(eyre!("expected host mismatch, got {other:?}")),
    }
    assert!(record.satisfies_row(&required, &HostProfile::WslOrGitBash).is_ok());
    Ok(())
}

/// Falsifier: stale sccache counters satisfying a later cell. An unrelated
/// user observed on the same server between snapshots forces the delta to
/// `Unattributed`; no clean cache claim survives.
#[test]
fn foreign_sccache_user_between_snapshots_forces_unattributed() -> Result<()> {
    let (clock, filesystems, locks, process, _cache, commands) = standard_parts(COMMIT_A);
    let cache = ScriptedCache::new(
        Some("sccache://fixture-1".to_string()),
        attributed_cache_snapshots(),
        1,
    );
    let mut harness = harness_from_parts(clock, filesystems, locks, process, cache, commands);
    let cell = fixture_cell(
        ExecutionModel::PrivateTargetSharedCargoSccache,
        WorkflowClass::Proof,
        Operation::ExactTest,
        fixture_subject(COMMIT_A),
        HostProfile::NativePosix,
        LockPolicy::None,
    );
    let record = executed(harness.execute_cell(cell, proof_execution()))?;
    assert_eq!(
        record.cache.attribution,
        CacheAttribution::Unattributed {
            reason: "1 unrelated users observed on the same server between snapshots".to_string()
        }
    );
    assert!(record.cache.clean_delta().is_none());
    assert!(
        reasons_of(&record)
            .iter()
            .any(|reason| matches!(reason, NotProvenReason::CacheEvidenceUnproven { .. }))
    );
    Ok(())
}

/// Falsifier: process/metrics APIs unavailable become zero usage. An
/// unavailable instrument is `NOT_PROVEN`, and `descendant_count()` returns
/// `None`, never zero.
#[test]
fn process_instrument_unavailable_is_never_zero_descendants() -> Result<()> {
    let (clock, filesystems, locks, _process, cache, commands) = standard_parts(COMMIT_A);
    let process = ScriptedProcess { observation: None };
    let mut harness = harness_from_parts(clock, filesystems, locks, process, cache, commands);
    let cell = fixture_cell(
        ExecutionModel::RawPrivateWorktree,
        WorkflowClass::Construction,
        Operation::Check,
        fixture_subject(COMMIT_A),
        HostProfile::NativePosix,
        LockPolicy::None,
    );
    let record = executed(harness.execute_cell(cell, proof_execution()))?;
    assert_eq!(record.process, ProcessObservation::InstrumentUnavailable);
    assert_eq!(record.process.descendant_count(), None);
    assert!(reasons_of(&record).contains(&NotProvenReason::ProcessInstrumentUnavailable));
    Ok(())
}

/// Falsifier: input order changes canonical cell identity. Feature sets and
/// growth-path lists declared in different orders produce the same id.
#[test]
fn canonical_cell_identity_is_input_order_independent() {
    let mut first = fixture_cell(
        ExecutionModel::RawPrivateWorktree,
        WorkflowClass::Proof,
        Operation::Test,
        fixture_subject(COMMIT_A),
        HostProfile::NativePosix,
        LockPolicy::None,
    );
    let mut second = first.clone();
    first.growth_paths = vec![
        PathScope {
            role: PathRole::Target,
            path: "/wt-a/target".to_string(),
            filesystem: FilesystemIdentity("devplane-a".to_string()),
        },
        PathScope {
            role: PathRole::Temp,
            path: "/wt-a/tmp".to_string(),
            filesystem: FilesystemIdentity("devplane-a".to_string()),
        },
    ];
    second.growth_paths = first.growth_paths.iter().rev().cloned().collect();
    second.subject.features = BTreeSet::from(["feat-b".to_string(), "feat-a".to_string()]);
    assert_eq!(first.canonical_id(), second.canonical_id());
    // Content change still moves the identity.
    second.subject.commit = COMMIT_B.to_string();
    assert_ne!(first.canonical_id(), second.canonical_id());
}

/// Protocol versions are load-bearing: a record under another protocol never
/// satisfies a row under this one.
#[test]
fn records_under_different_protocols_never_match() -> Result<()> {
    let mut record = admitted_shared_cache_record()?;
    record.protocol_version = "build_executor_measurement.v0".to_string();
    let required = fixture_subject(COMMIT_A);
    let refusal = record
        .satisfies_row(&required, &HostProfile::NativePosix)
        .err()
        .ok_or_else(|| eyre!("expected protocol mismatch, got Ok"))?;
    match refusal {
        RowRefusal::ProtocolMismatch { record_protocol } => {
            assert_eq!(record_protocol, "build_executor_measurement.v0");
        }
        other => return Err(eyre!("expected protocol mismatch, got {other:?}")),
    }
    Ok(())
}

/// Decision law: an unsupported host row is never admitted as a measurement
/// of real execution.
#[test]
fn unsupported_host_is_never_admitted() -> Result<()> {
    let (clock, filesystems, locks, process, cache, commands) = standard_parts(COMMIT_A);
    let mut harness = harness_from_parts(clock, filesystems, locks, process, cache, commands);
    let cell = fixture_cell(
        ExecutionModel::RawPrivateWorktree,
        WorkflowClass::Proof,
        Operation::Test,
        fixture_subject(COMMIT_A),
        HostProfile::Unsupported,
        LockPolicy::None,
    );
    let record = executed(harness.execute_cell(cell, proof_execution()))?;
    assert!(reasons_of(&record).contains(&NotProvenReason::UnsupportedHost));
    Ok(())
}

/// Falsifier: the harness changes the current wrapper or selected
/// architecture. The harness only observes: an unobservable run yields a
/// fail-closed record with no invented exit code, work, or subject.
#[test]
fn unobservable_run_is_fail_closed_not_invented() -> Result<()> {
    let (clock, filesystems, locks, process, cache, _commands) = standard_parts(COMMIT_A);
    let commands = ScriptedRunner::new(Vec::new());
    let mut harness = harness_from_parts(clock, filesystems, locks, process, cache, commands);
    let cell = fixture_cell(
        ExecutionModel::RawPrivateWorktree,
        WorkflowClass::Proof,
        Operation::Test,
        fixture_subject(COMMIT_A),
        HostProfile::NativePosix,
        LockPolicy::None,
    );
    let record = executed(harness.execute_cell(cell, proof_execution()))?;
    assert_eq!(record.work.exit_code, None);
    assert_eq!(record.work.observed_selected, None);
    assert_eq!(record.executed_subject, None);
    assert!(reasons_of(&record).contains(&NotProvenReason::ExecutedSubjectUnproven));
    Ok(())
}

/// All eight execution models are representable with distinct spellings and
/// the shared-cache predicate draws the line the controller draws.
#[test]
fn execution_models_all_representable_without_collapse() -> Result<()> {
    let models = [
        ExecutionModel::RawPrivateWorktree,
        ExecutionModel::CargoSafeDirectLeaf,
        ExecutionModel::CargoSafeXtaskEnvironmentOnly,
        ExecutionModel::SeparateWorktreeDevplanes,
        ExecutionModel::ForcedSharedDevplane,
        ExecutionModel::PrivateTargetSharedCargoSccache,
        ExecutionModel::BoundedTargetPoolSharedCache,
        ExecutionModel::PrivateTargetSharedCacheHostCapacityPool,
    ];
    let mut spellings = Vec::new();
    for model in &models {
        let text = serde_json::to_string(model)
            .map_err(|error| eyre!("execution model serializes: {error}"))?;
        spellings.push(text);
    }
    let unique: BTreeSet<&String> = spellings.iter().collect();
    assert_eq!(unique.len(), models.len(), "model spellings must not collapse");
    for (model, spelling) in models.iter().zip(&spellings) {
        let requires = model.requires_cache_evidence();
        let expected = matches!(
            model,
            ExecutionModel::ForcedSharedDevplane
                | ExecutionModel::PrivateTargetSharedCargoSccache
                | ExecutionModel::BoundedTargetPoolSharedCache
                | ExecutionModel::PrivateTargetSharedCacheHostCapacityPool
        );
        assert_eq!(requires, expected, "shared-cache line drifted for {spelling}");
    }
    Ok(())
}

/// Construction, proof, and orchestration workflow classes remain distinct,
/// and only proof-shaped test cells demand selected-work evidence.
#[test]
fn workflow_classes_remain_distinct_and_proof_demands_selected_work() -> Result<()> {
    let classes = [WorkflowClass::Construction, WorkflowClass::Proof, WorkflowClass::Orchestration];
    let mut spellings = Vec::new();
    for class in &classes {
        let text = serde_json::to_string(class)
            .map_err(|error| eyre!("workflow class serializes: {error}"))?;
        spellings.push(text);
    }
    let unique: BTreeSet<&String> = spellings.iter().collect();
    assert_eq!(unique.len(), classes.len());
    let subject = fixture_subject(COMMIT_A);
    for class in classes {
        for operation in [Operation::Test, Operation::Check, Operation::Metadata] {
            let cell = fixture_cell(
                ExecutionModel::RawPrivateWorktree,
                class,
                operation,
                subject.clone(),
                HostProfile::NativePosix,
                LockPolicy::None,
            );
            let expected =
                matches!(class, WorkflowClass::Proof) && matches!(operation, Operation::Test);
            assert_eq!(
                cell.requires_selected_work(),
                expected,
                "selected-work demand drifted for {class:?} {operation:?}"
            );
        }
    }
    Ok(())
}

/// Human/JSON output derives from one typed record and a second render is
/// byte-identical (deterministic second render).
#[test]
fn renders_are_deterministic_and_single_sourced() -> Result<()> {
    let record = admitted_shared_cache_record()?;
    let json_once = render_json(&record);
    let json_twice = render_json(&record);
    let human_once = render_human(&record);
    let human_twice = render_human(&record);
    assert_eq!(json_once, json_twice);
    assert_eq!(human_once, human_twice);
    assert!(json_once.contains(PROTOCOL_VERSION));
    assert!(json_once.contains("\"admission\""));
    assert!(json_once.contains("admitted"));
    assert!(human_once.contains("protocol:           build_executor_measurement.v1"));
    assert!(human_once.contains("verdict:            admitted"));
    // A refused record keeps its reasons visible in both projections.
    let (clock, filesystems, _locks, process, cache, commands) = standard_parts(COMMIT_A);
    let locks = ScriptedLocks { flock_available: false, acquire_wait_nanos: 0 };
    let mut harness = harness_from_parts(clock, filesystems, locks, process, cache, commands);
    let cell = fixture_cell(
        ExecutionModel::CargoSafeDirectLeaf,
        WorkflowClass::Proof,
        Operation::Test,
        fixture_subject(COMMIT_A),
        HostProfile::NativePosix,
        LockPolicy::WholeProcessFlock,
    );
    let refused = executed(harness.execute_cell(cell, proof_execution()))?;
    let refused_json = render_json(&refused);
    let refused_human = render_human(&refused);
    assert!(refused_json.contains("lock_not_admitted"));
    assert!(refused_human.contains("verdict:            not_proven"));
    assert!(refused_human.contains("lock row not admitted"));
    Ok(())
}

/// Raw-evidence and normalized-interpretation digests are separate layers:
/// identical runs produce identical digest pairs, and the two layers differ.
#[test]
fn raw_and_normalized_digests_are_stable_and_distinct() -> Result<()> {
    let first = admitted_shared_cache_record()?;
    let second = admitted_shared_cache_record()?;
    assert_eq!(first.raw_digest, second.raw_digest);
    assert_eq!(first.normalized_digest, second.normalized_digest);
    assert_ne!(first.raw_digest, first.normalized_digest);
    assert!(first.raw_digest.starts_with("sha256:"));
    assert!(first.normalized_digest.starts_with("sha256:"));
    Ok(())
}

/// Deterministic concurrency barrier: orchestration cells coordinate by
/// arrival, never by timing sleeps.
#[test]
fn concurrency_barrier_releases_only_when_all_participants_arrive() {
    let barrier = DeterministicBarrier::new(2);
    assert!(!barrier.arrive("cell-a"));
    // Re-arrival of the same participant does not advance the barrier.
    assert!(!barrier.arrive("cell-a"));
    assert!(barrier.arrive("cell-b"));
    let single = DeterministicBarrier::new(1);
    assert!(single.arrive("solo"));
}

/// House schema cross-check: the struct's serialized field set matches
/// `.ci/receipts/schemas/build-executor-measurement.v1.schema.json`'s
/// `required`/`properties` (`additionalProperties: false`).
#[test]
fn emitted_record_matches_schema_required_and_property_set() -> Result<()> {
    let root = project_root();
    let schema_path = root.join(".ci/receipts/schemas/build-executor-measurement.v1.schema.json");
    let schema_text = fs::read_to_string(&schema_path)
        .map_err(|error| eyre!("reading {}: {error}", schema_path.display()))?;
    let schema: serde_json::Value = serde_json::from_str(&schema_text)?;

    let required: Vec<String> = schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .map(|items| items.iter().filter_map(|v| v.as_str().map(ToOwned::to_owned)).collect())
        .unwrap_or_default();
    let properties: BTreeSet<String> = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default();

    assert!(!required.is_empty(), "schema declares no required fields");
    assert!(!properties.is_empty(), "schema declares no properties");

    let record = admitted_shared_cache_record()?;
    let value = serde_json::to_value(&record)?;
    let object = value.as_object().ok_or_else(|| eyre!("record did not serialize to an object"))?;

    for key in &required {
        assert!(object.contains_key(key), "record missing schema-required field {key}");
    }
    for key in object.keys() {
        assert!(
            properties.contains(key),
            "record field {key} is not declared in schema properties"
        );
    }
    Ok(())
}

/// The refused `DiskAdmission`/`WorkObservation` shapes stay serializable
/// too (schema-level guarantee for `NOT_PROVEN` records).
#[test]
fn refused_shapes_remain_serializable() {
    let admission =
        DiskAdmission { measurements: Vec::new(), declared_path_mismatches: Vec::new() };
    let work =
        WorkObservation { expected_selected: None, observed_selected: None, exit_code: None };
    assert!(serde_json::to_string(&admission).is_ok());
    assert!(serde_json::to_string(&work).is_ok());
}

/// Repository root for the schema cross-check (lib modules cannot see the
/// binary crate's `utils::project_root`; same spelling as
/// `vim_host_toolchain`'s test root).
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}
