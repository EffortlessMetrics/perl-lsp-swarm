//! Typed measurement contract for `build_executor_measurement.v1` (#11639).
//!
//! One experiment cell declares: which workflow class it belongs to, which
//! execution model it measures, which operation runs, the exact subject and
//! host identity, its cold/warm repetition ordinal, the build-state path
//! scopes it expects to grow, and its declared lock policy. One measurement
//! record retains the raw observed facts (literal invocation, effective
//! environment, timings, disk/lock/process/cache/work observations) plus the
//! normalized interpretation, with separate digests for the two layers.
//!
//! Fail-closed doctrine (mirrors `tasks/session_receipt.rs`): every fact the
//! harness cannot actually observe reports `None` / an explicit
//! `Unobserved`/`Unavailable` state, never a plausible zero or a borrowed
//! value from another row. Decision laws from controller #9547 are enforced
//! here as admission checks ([`MeasurementRecord::admit`]), not prose:
//! subject correctness is a hard floor, private/shared/capacity scopes stay
//! separately declared, cache reuse is observed rather than inferred, queue
//! and lock wait remain inside total elapsed time, and missing instruments
//! yield `NOT_PROVEN` rather than zero.

use crate::editor_host::sha256_bytes;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// Version pinned into every cell identity and record. Two records under
/// different protocol versions are never matched pairs.
pub const PROTOCOL_VERSION: &str = "build_executor_measurement.v1";

/// Default reconciliation tolerance between the phase sum and the declared
/// total wall time (1 ms). Overlapping independently sampled timers are never
/// subtracted into a residual phase; the sum must simply agree with the total.
pub const DEFAULT_TOLERANCE_NANOS: u64 = 1_000_000;

/// Which loop a cell serves. Construction, proof, and orchestration remain
/// three distinct classes; no admission law may collapse them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowClass {
    Construction,
    Proof,
    Orchestration,
}

/// The execution model a cell measures. The three current `cargo-safe` shapes
/// are materially different systems and are represented as distinct rows
/// (`raw_private_worktree`, `cargo_safe_direct_leaf`,
/// `cargo_safe_xtask_environment_only`); the five candidate models are
/// separate rows so the #11642 decision can compare them without Boolean
/// collapse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionModel {
    RawPrivateWorktree,
    CargoSafeDirectLeaf,
    CargoSafeXtaskEnvironmentOnly,
    SeparateWorktreeDevplanes,
    ForcedSharedDevplane,
    PrivateTargetSharedCargoSccache,
    BoundedTargetPoolSharedCache,
    PrivateTargetSharedCacheHostCapacityPool,
}

impl ExecutionModel {
    /// Whether the model's claim depends on shared-cache evidence. For these
    /// rows a cache observation that is not attributed to the cell (fresh
    /// counters, one server identity, zero foreign users) is `NOT_PROVEN`;
    /// private-state rows do not lean on shared cache rows.
    pub fn requires_cache_evidence(&self) -> bool {
        matches!(
            self,
            ExecutionModel::ForcedSharedDevplane
                | ExecutionModel::PrivateTargetSharedCargoSccache
                | ExecutionModel::BoundedTargetPoolSharedCache
                | ExecutionModel::PrivateTargetSharedCacheHostCapacityPool
        )
    }
}

/// The measured Cargo-family operation. Test-runner profiles are represented
/// by [`Operation::SelectedNextest`]; the Cargo build profile is a subject
/// field and never collapses into the runner profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Metadata,
    Check,
    Clippy,
    Test,
    ExactTest,
    Build,
    SelectedNextest,
}

/// Host profile of one cell. Native POSIX, native Windows, WSL/Git Bash, and
/// unsupported remain distinct; no profile inherits another's results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostProfile {
    NativeWindows,
    NativePosix,
    WslOrGitBash,
    Unsupported,
}

impl HostProfile {
    /// A record stands in for a required row only under exact host equality:
    /// WSL/Git Bash evidence never fills a native-Windows row, and
    /// `unsupported` never fills any row.
    pub fn satisfies(&self, required: &HostProfile) -> bool {
        self == required
    }
}

/// Lock policy a cell declares. Declaring `None` is an honest fact for models
/// that run unlocked; claiming a locked run while the primitive is missing is
/// refused at admission ([`LockObservation::is_admitted`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockPolicy {
    WholeProcessFlock,
    None,
}

/// Declared host-capacity policy for the cell. Private build state, reusable
/// caches, and host capacity are separate scopes; none of them is derived
/// from another here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityPolicy {
    CandidatePrivate,
    ForcedSharedDevplane,
    BoundedPool { slots: u32 },
    HostCapacityPool,
    Unmanaged,
}

/// Identity of one filesystem/volume. Growth paths on different filesystems
/// require separate free-space measurements; an admission measured on one
/// filesystem cannot authorize growth on another.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FilesystemIdentity(pub String);

/// What a path scope is for. Each role is recorded separately because a
/// default-path free-space check must never silently authorize the other
/// roles' actual locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathRole {
    Target,
    BuildDir,
    Temp,
    CargoHome,
    SccacheCache,
    LockRoot,
}

/// One declared growth path with the filesystem it actually resolves to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PathScope {
    pub role: PathRole,
    pub path: String,
    pub filesystem: FilesystemIdentity,
}

/// Exact subject identity. Every field is load-bearing: a record from another
/// candidate, toolchain, profile, feature set, or worktree is not a matched
/// pair for a row requiring this subject.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SubjectIdentity {
    pub repository: String,
    /// Exact candidate/source identity (commit SHA).
    pub commit: String,
    pub worktree: String,
    pub package: String,
    pub target_triple: String,
    /// Order-independent; input order never changes canonical identity.
    pub features: BTreeSet<String>,
    pub default_features: bool,
    pub toolchain: String,
    /// Cargo build profile, kept separate from the test-runner profile.
    pub build_profile: String,
    pub test_runner_profile: Option<String>,
}

impl SubjectIdentity {
    /// All fields in which `self` differs from `required`, in stable field
    /// order. Empty means the record's subject can stand in for the row.
    pub fn differences(&self, required: &SubjectIdentity) -> Vec<String> {
        let mut diffs = Vec::new();
        if self.repository != required.repository {
            diffs.push("repository".to_string());
        }
        if self.commit != required.commit {
            diffs.push("commit".to_string());
        }
        if self.worktree != required.worktree {
            diffs.push("worktree".to_string());
        }
        if self.package != required.package {
            diffs.push("package".to_string());
        }
        if self.target_triple != required.target_triple {
            diffs.push("target_triple".to_string());
        }
        if self.features != required.features {
            diffs.push("features".to_string());
        }
        if self.default_features != required.default_features {
            diffs.push("default_features".to_string());
        }
        if self.toolchain != required.toolchain {
            diffs.push("toolchain".to_string());
        }
        if self.build_profile != required.build_profile {
            diffs.push("build_profile".to_string());
        }
        if self.test_runner_profile != required.test_runner_profile {
            diffs.push("test_runner_profile".to_string());
        }
        diffs
    }
}

/// Cold/warm and repetition ordinal of the cell within its experiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RepetitionOrdinal {
    pub cold: bool,
    pub repetition: u32,
}

/// The declared experiment cell: the full identity of one measurement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeasurementCell {
    pub workflow_class: WorkflowClass,
    pub execution_model: ExecutionModel,
    pub operation: Operation,
    pub subject: SubjectIdentity,
    pub host: HostProfile,
    pub ordinal: RepetitionOrdinal,
    /// Declared growth paths with their actual filesystems. Canonical order
    /// is (role, path) so input order never changes identity.
    pub growth_paths: Vec<PathScope>,
    pub lock_policy: LockPolicy,
    pub capacity: CapacityPolicy,
}

impl MeasurementCell {
    /// Deterministic canonical form: growth paths sorted by (role, path).
    pub fn canonical(&self) -> MeasurementCell {
        let mut growth = self.growth_paths.clone();
        growth.sort();
        MeasurementCell {
            workflow_class: self.workflow_class,
            execution_model: self.execution_model,
            operation: self.operation,
            subject: self.subject.clone(),
            host: self.host,
            ordinal: self.ordinal,
            growth_paths: growth,
            lock_policy: self.lock_policy,
            capacity: self.capacity.clone(),
        }
    }

    /// Content digest over the canonical cell plus the protocol version. Two
    /// cells with the same identity fields declared in different input orders
    /// produce the same id; cells under different protocol versions never do.
    pub fn canonical_id(&self) -> String {
        let fingerprint = json!({
            "protocol_version": PROTOCOL_VERSION,
            "cell": self.canonical(),
        });
        // serde_json serializing a canonical structure is deterministic.
        match serde_json::to_vec(&fingerprint) {
            Ok(bytes) => sha256_bytes(&bytes).unwrap_or_else(|_| "sha256:unavailable".to_string()),
            Err(_) => "sha256:unavailable".to_string(),
        }
    }

    /// Proof-workflow test-shaped cells must prove they selected and executed
    /// real work; zero selected tests can never satisfy them.
    pub fn requires_selected_work(&self) -> bool {
        matches!(self.workflow_class, WorkflowClass::Proof)
            && matches!(
                self.operation,
                Operation::Test | Operation::ExactTest | Operation::SelectedNextest
            )
    }
}

/// The literal structured invocation the harness executed, with the effective
/// environment it ran under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandIdentity {
    pub program: String,
    pub args: Vec<String>,
    pub effective_env: BTreeMap<String, String>,
}

/// Toolchain/host environment identity. Unprobeable fields stay `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentIdentity {
    pub cargo_version: Option<String>,
    pub rustc_version: Option<String>,
    pub host_triple: Option<String>,
}

/// Phase decomposition of one cell. All values are monotonic-clock nanos and
/// the phases are exhaustive: preparation + admission (queue/lock) wait +
/// execution + reporting must reconcile with the declared total within the
/// tolerance. Queue and lock wait are work and remain inside the total.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimingDecomposition {
    pub preparation_nanos: Option<u64>,
    pub admission_wait_nanos: Option<u64>,
    pub execution_nanos: Option<u64>,
    pub reporting_nanos: Option<u64>,
    pub total_wall_nanos: Option<u64>,
    pub tolerance_nanos: u64,
}

/// Outcome of reconciling the phase decomposition against the total.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimingVerdict {
    Complete,
    Incomplete {
        missing: Vec<String>,
    },
    Mismatch {
        computed_sum_nanos: u64,
        declared_total_nanos: u64,
    },
    /// Phase arithmetic overflowed u64 nanos; a hostile or corrupt record can
    /// never reconcile through wrapping.
    Overflow,
}

impl TimingDecomposition {
    /// Reconcile phase sum against the declared total. Every phase must be
    /// present; a record missing the admission/queue/lock wait segment is
    /// incomplete, never silently reconciled.
    pub fn reconcile(&self) -> TimingVerdict {
        let mut missing = Vec::new();
        if self.preparation_nanos.is_none() {
            missing.push("preparation".to_string());
        }
        if self.admission_wait_nanos.is_none() {
            missing.push("admission_wait".to_string());
        }
        if self.execution_nanos.is_none() {
            missing.push("execution".to_string());
        }
        if self.reporting_nanos.is_none() {
            missing.push("reporting".to_string());
        }
        if self.total_wall_nanos.is_none() {
            missing.push("total_wall".to_string());
        }
        if !missing.is_empty() {
            return TimingVerdict::Incomplete { missing };
        }
        let sum = self
            .preparation_nanos
            .unwrap_or(0)
            .checked_add(self.admission_wait_nanos.unwrap_or(0))
            .and_then(|sum| sum.checked_add(self.execution_nanos.unwrap_or(0)))
            .and_then(|sum| sum.checked_add(self.reporting_nanos.unwrap_or(0)));
        let Some(sum) = sum else {
            return TimingVerdict::Overflow;
        };
        let total = self.total_wall_nanos.unwrap_or(0);
        let delta = sum.abs_diff(total);
        if delta > self.tolerance_nanos {
            TimingVerdict::Mismatch { computed_sum_nanos: sum, declared_total_nanos: total }
        } else {
            TimingVerdict::Complete
        }
    }
}

/// One filesystem's measured free space. `free_bytes: None` means the
/// measurement failed; it never means "infinite" or "unchecked is fine".
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FilesystemFreeSpace {
    pub filesystem: FilesystemIdentity,
    pub free_bytes: Option<u64>,
}

/// Disk admission: the actual filesystems the declared growth paths resolve
/// to, with free space measured on each, plus any declared-vs-actual
/// filesystem mismatches the provider observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskAdmission {
    /// Sorted by filesystem identity for canonical rendering.
    pub measurements: Vec<FilesystemFreeSpace>,
    /// Growth paths whose actual filesystem differs from the declared one.
    /// Sorted; empty means every path resolved where it was declared.
    pub declared_path_mismatches: Vec<DiskRefusal>,
}

/// Why a disk admission does not cover the declared growth paths.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiskRefusal {
    /// The path's actual filesystem differs from the declared one: an
    /// admission measured on the declared volume would authorize growth on
    /// another volume it never observed.
    FilesystemMismatch { path: String, declared: String, actual: String },
    /// The path resolved to no filesystem the provider could identify.
    UnresolvedFilesystem { path: String },
    /// A growth path resolves to a filesystem the admission never measured.
    UnmeasuredFilesystem { path: String, filesystem: String },
    /// The free-space measurement on an actual growth filesystem failed.
    UnmeasuredFreeSpace { filesystem: String },
}

impl DiskAdmission {
    /// Every declared growth path must have resolved where it was declared,
    /// and every actual growth filesystem must have a successful measurement
    /// on that exact filesystem. A default-devplane check cannot authorize a
    /// target/cache/tmp path growing on another volume.
    pub fn covers(&self, growth_paths: &[PathScope]) -> Result<(), DiskRefusal> {
        if let Some(refusal) = self.declared_path_mismatches.first() {
            return Err(refusal.clone());
        }
        for scope in growth_paths {
            let measurement = self.measurements.iter().find(|m| m.filesystem == scope.filesystem);
            match measurement {
                None => {
                    return Err(DiskRefusal::UnmeasuredFilesystem {
                        path: scope.path.clone(),
                        filesystem: scope.filesystem.0.clone(),
                    });
                }
                Some(m) if m.free_bytes.is_none() => {
                    return Err(DiskRefusal::UnmeasuredFreeSpace {
                        filesystem: scope.filesystem.0.clone(),
                    });
                }
                Some(_) => {}
            }
        }
        Ok(())
    }
}

/// The lock primitive a held lock used. The whole-Cargo-process `flock` is
/// recorded as exactly that — it is never renamed into a "compile lock".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockPrimitive {
    WholeProcessFlock,
    FileLock,
}

/// Observed lock state of one cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockObservation {
    Held {
        primitive: LockPrimitive,
        wait_nanos: u64,
    },
    /// The cell's policy honestly declares no lock for this model.
    PolicyDeclaresNone,
    /// The policy wanted a lock but the host does not provide the primitive.
    /// This is `NOT_PROVEN` for the lock row, never a locked success.
    PrimitiveUnavailable,
    /// The lock instrument itself was unavailable.
    Unobserved,
}

impl LockObservation {
    /// Whether the lock row can be admitted **for the cell's declared
    /// policy**. A held lock satisfies only the exact declared primitive
    /// (a generic `FileLock` never stands in for the declared
    /// whole-Cargo-process flock); a lock-free observation satisfies only a
    /// lock-free policy; anything else is `NOT_PROVEN` for the lock row.
    pub fn is_admitted_for(&self, policy: LockPolicy) -> bool {
        matches!(
            (self, policy),
            (
                LockObservation::Held { primitive: LockPrimitive::WholeProcessFlock, .. },
                LockPolicy::WholeProcessFlock
            ) | (LockObservation::PolicyDeclaresNone, LockPolicy::None)
        )
    }
}

/// Process-tree terminality. Process exit is not terminality: residual
/// descendants or lock ownership keep a cell from being declared clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Terminality {
    Clean,
    ResidualDescendants,
}

/// Observed process state of one cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessObservation {
    Observed {
        descendant_count: u64,
        terminality: Terminality,
    },
    /// Process instrumentation was unavailable. This is `NOT_PROVEN`, never
    /// a silent "zero descendants".
    InstrumentUnavailable,
}

impl ProcessObservation {
    /// `Some(count)` only when descendants were actually observed; an
    /// unavailable instrument yields `None`, never zero.
    pub fn descendant_count(&self) -> Option<u64> {
        match self {
            ProcessObservation::Observed { descendant_count, .. } => Some(*descendant_count),
            ProcessObservation::InstrumentUnavailable => None,
        }
    }
}

/// Raw sccache-style counter snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheCounters {
    pub requests: u64,
    pub hits: u64,
    pub misses: u64,
    pub non_cacheable: u64,
}

impl CacheCounters {
    /// Element-wise delta of `self` (the later snapshot) against `baseline`.
    pub fn delta_since(&self, baseline: &CacheCounters) -> CacheCounters {
        CacheCounters {
            requests: self.requests.saturating_sub(baseline.requests),
            hits: self.hits.saturating_sub(baseline.hits),
            misses: self.misses.saturating_sub(baseline.misses),
            non_cacheable: self.non_cacheable.saturating_sub(baseline.non_cacheable),
        }
    }

    /// Whether every counter moved monotonically forward from `baseline`.
    /// Any regression (counter restart, server replacement, or an unrelated
    /// reset) makes the interval unusable for a clean delta.
    pub fn is_monotonic_after(&self, baseline: &CacheCounters) -> bool {
        self.requests >= baseline.requests
            && self.hits >= baseline.hits
            && self.misses >= baseline.misses
            && self.non_cacheable >= baseline.non_cacheable
    }
}

/// Whether the observed cache delta may be attributed to this cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheAttribution {
    /// Fresh counters under one server/process identity with zero foreign
    /// users between snapshots.
    Attributed,
    /// The delta cannot be attributed to the cell; the reason states which
    /// precondition failed.
    Unattributed { reason: String },
    /// No cache metrics instrument was available.
    Unobserved,
}

/// Cache observation of one cell. Environment variables such as
/// `SCCACHE_BASEDIRS` never appear here: reuse is only ever the observed
/// counter delta, and only when attribution holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheObservation {
    /// Exact server/process identity at the baseline snapshot.
    pub server_identity: Option<String>,
    /// Exact server/process identity at the delta snapshot. A restart or
    /// reconnection between snapshots makes the interval unattributable.
    pub delta_server_identity: Option<String>,
    pub baseline: Option<CacheCounters>,
    pub delta: Option<CacheCounters>,
    /// Unrelated users observed on the same server between snapshots.
    pub foreign_users_observed: u64,
    pub attribution: CacheAttribution,
}

impl CacheObservation {
    /// The observed hit/miss delta, only when attribution admits it **and**
    /// the interval itself is clean (monotonic counters, one server identity).
    pub fn clean_delta(&self) -> Option<CacheCounters> {
        match self.attribution {
            CacheAttribution::Attributed => {}
            _ => return None,
        }
        if self.server_identity.is_none() || self.delta_server_identity.is_none() {
            return None;
        }
        if self.server_identity != self.delta_server_identity {
            return None;
        }
        let baseline = self.baseline.as_ref()?;
        let later = self.delta.as_ref()?;
        if !later.is_monotonic_after(baseline) {
            return None;
        }
        Some(later.delta_since(baseline))
    }
}

/// Selected/executed work of one cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkObservation {
    pub expected_selected: Option<u64>,
    pub observed_selected: Option<u64>,
    pub exit_code: Option<i32>,
}

/// The exact candidate identity the cell actually executed, as proven by
/// artifact/output identity metadata. This is **explicitly partial
/// evidence**: only the exact-candidate (commit) dimension is claimed. No
/// other subject dimension (package, target, features, toolchain, profile)
/// is ever synthesized into a proven claim — those remain the declared
/// cell's identity, and proving them from parsed receipts is the host
/// observation lanes' (#11640/#11641) obligation. `None` means the executed
/// subject was never proven; it is never assumed equal to the declared
/// subject.
pub type ExecutedSubjectCommit = Option<String>;

/// One admitted or refused measurement, as typed reasons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellVerdict {
    Admitted,
    NotProven { reasons: Vec<NotProvenReason> },
}

impl CellVerdict {
    /// True only for [`CellVerdict::Admitted`].
    pub fn is_admitted(&self) -> bool {
        matches!(self, CellVerdict::Admitted)
    }
}

/// Why a record fails admission. Every variant is a decision law from
/// controller #9547 expressed as a check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotProvenReason {
    TimingIncomplete {
        missing: Vec<String>,
    },
    TimingMismatch {
        computed_sum_nanos: u64,
        declared_total_nanos: u64,
    },
    UnsupportedHost,
    LockNotAdmitted,
    ProcessInstrumentUnavailable,
    DiskAdmissionMissing,
    DiskAdmissionRefused {
        detail: String,
    },
    /// The executed subject was never proven (artifact/output identity
    /// missing) — fail-closed, never assumed equal to the declared subject.
    ExecutedSubjectUnproven,
    /// The proven executed candidate differs from the declared subject: a
    /// cross-candidate artifact/test substitution. Cache statistics cannot
    /// compensate (decision law 1).
    ExecutedSubjectMismatch {
        detail: String,
    },
    /// The command's exit code was never observed. A run whose success is
    /// unobservable is never an admitted measurement.
    CommandExitUnproven,
    /// The command exited nonzero. A failed run remains a renderable record
    /// but can never be an admitted experiment row.
    CommandFailed {
        exit_code: i32,
    },
    /// Compiler/test descendants remained alive at observation (or a
    /// "clean" observation carried a positive descendant count): process
    /// exit is not terminality, and a residual tree contaminates subsequent
    /// cells (decision law 10).
    ProcessResidual {
        descendant_count: u64,
        terminality: Terminality,
    },
    /// Phase arithmetic overflowed; the timing block cannot be trusted.
    TimingOverflow,
    /// A proof cell ran zero selected work (or the counts did not reconcile).
    SelectedWorkUnproven {
        expected: Option<u64>,
        observed: Option<u64>,
    },
    /// A shared-cache model's cache delta was not attributed to this cell.
    CacheEvidenceUnproven {
        reason: String,
    },
}

/// The complete measurement record for one declared cell: raw facts plus
/// normalized interpretation, with separate digests so the #11642 decision
/// successor can challenge the interpretation without trusting the harness's
/// model labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeasurementRecord {
    pub protocol_version: String,
    pub cell: MeasurementCell,
    pub command: CommandIdentity,
    pub environment: EnvironmentIdentity,
    pub timings: TimingDecomposition,
    pub disk_admission: Option<DiskAdmission>,
    pub lock: LockObservation,
    pub process: ProcessObservation,
    pub cache: CacheObservation,
    pub work: WorkObservation,
    pub executed_subject_commit: ExecutedSubjectCommit,
    /// Digest over the raw observed facts only.
    pub raw_digest: String,
    /// Digest over the normalized interpretation (canonical cell + admission
    /// outcome labels). Kept separate from `raw_digest` by construction.
    pub normalized_digest: String,
}

/// Why a record cannot stand in for a required experiment row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowRefusal {
    SubjectMismatch { differing_fields: Vec<String> },
    HostMismatch { record_host: HostProfile, required_host: HostProfile },
    ProtocolMismatch { record_protocol: String },
}

impl MeasurementRecord {
    /// Whether this record can stand in for a row requiring
    /// `required_subject` on `required_host` under this protocol version.
    /// Subject and host compatibility only; admission ([`Self::admit`]) is a
    /// separate judgment, and the two compose.
    pub fn satisfies_row(
        &self,
        required_subject: &SubjectIdentity,
        required_host: &HostProfile,
    ) -> Result<(), RowRefusal> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(RowRefusal::ProtocolMismatch {
                record_protocol: self.protocol_version.clone(),
            });
        }
        let differing = self.cell.subject.differences(required_subject);
        if !differing.is_empty() {
            return Err(RowRefusal::SubjectMismatch { differing_fields: differing });
        }
        if !self.cell.host.satisfies(required_host) {
            return Err(RowRefusal::HostMismatch {
                record_host: self.cell.host,
                required_host: *required_host,
            });
        }
        Ok(())
    }

    /// Admission: apply every decision law to this record. A clean verdict is
    /// [`CellVerdict::Admitted`]; anything else lists the typed
    /// `NOT_PROVEN` reasons.
    pub fn admit(&self) -> CellVerdict {
        let mut reasons = Vec::new();

        match self.timings.reconcile() {
            TimingVerdict::Complete => {}
            TimingVerdict::Incomplete { missing } => {
                reasons.push(NotProvenReason::TimingIncomplete { missing });
            }
            TimingVerdict::Mismatch { computed_sum_nanos, declared_total_nanos } => {
                reasons.push(NotProvenReason::TimingMismatch {
                    computed_sum_nanos,
                    declared_total_nanos,
                });
            }
            TimingVerdict::Overflow => reasons.push(NotProvenReason::TimingOverflow),
        }

        if self.cell.host == HostProfile::Unsupported {
            reasons.push(NotProvenReason::UnsupportedHost);
        }

        // Lock admission is policy-aware: the observed state must match the
        // declared policy and its exact primitive.
        if !self.lock.is_admitted_for(self.cell.lock_policy) {
            reasons.push(NotProvenReason::LockNotAdmitted);
        }

        match &self.process {
            ProcessObservation::InstrumentUnavailable => {
                reasons.push(NotProvenReason::ProcessInstrumentUnavailable);
            }
            ProcessObservation::Observed { descendant_count, terminality } => {
                // Process exit is not terminality: only a clean tree with
                // zero live descendants admits. "Clean" with a positive
                // count is an inconsistent observation and fails closed.
                if *descendant_count > 0 || *terminality == Terminality::ResidualDescendants {
                    reasons.push(NotProvenReason::ProcessResidual {
                        descendant_count: *descendant_count,
                        terminality: *terminality,
                    });
                }
            }
        }

        match &self.disk_admission {
            None => reasons.push(NotProvenReason::DiskAdmissionMissing),
            Some(admission) => {
                if let Err(refusal) = admission.covers(&self.cell.canonical().growth_paths) {
                    reasons.push(NotProvenReason::DiskAdmissionRefused {
                        detail: refusal_text(&refusal),
                    });
                }
            }
        }

        match &self.executed_subject_commit {
            None => reasons.push(NotProvenReason::ExecutedSubjectUnproven),
            Some(executed_commit) => {
                if executed_commit != &self.cell.subject.commit {
                    reasons.push(NotProvenReason::ExecutedSubjectMismatch {
                        detail: "commit".to_string(),
                    });
                }
            }
        }

        // A run whose success is unobservable or failed is never an admitted
        // experiment row, regardless of any other clean evidence.
        match self.work.exit_code {
            None => reasons.push(NotProvenReason::CommandExitUnproven),
            Some(0) => {}
            Some(exit_code) => reasons.push(NotProvenReason::CommandFailed { exit_code }),
        }

        if self.cell.requires_selected_work() {
            let expected = self.work.expected_selected;
            let observed = self.work.observed_selected;
            let satisfied = matches!((expected, observed), (Some(e), Some(o)) if e == o && e > 0);
            if !satisfied {
                reasons.push(NotProvenReason::SelectedWorkUnproven { expected, observed });
            }
        }

        if self.cell.execution_model.requires_cache_evidence() {
            match &self.cache.attribution {
                CacheAttribution::Attributed => {}
                CacheAttribution::Unattributed { reason } => {
                    reasons.push(NotProvenReason::CacheEvidenceUnproven { reason: reason.clone() });
                }
                CacheAttribution::Unobserved => {
                    reasons.push(NotProvenReason::CacheEvidenceUnproven {
                        reason: "cache metrics instrument unavailable".to_string(),
                    });
                }
            }
        }

        if reasons.is_empty() { CellVerdict::Admitted } else { CellVerdict::NotProven { reasons } }
    }
}

/// Stable human/detail text for a disk refusal (also used in renders).
pub fn refusal_text(refusal: &DiskRefusal) -> String {
    match refusal {
        DiskRefusal::FilesystemMismatch { path, declared, actual } => format!(
            "growth path {path} resolved to filesystem {actual} but was declared on {declared}"
        ),
        DiskRefusal::UnresolvedFilesystem { path } => {
            format!("growth path {path} resolved to no identifiable filesystem")
        }
        DiskRefusal::UnmeasuredFilesystem { path, filesystem } => format!(
            "growth path {path} resolves to filesystem {filesystem} which the admission never measured"
        ),
        DiskRefusal::UnmeasuredFreeSpace { filesystem } => {
            format!("free-space measurement failed on actual growth filesystem {filesystem}")
        }
    }
}
