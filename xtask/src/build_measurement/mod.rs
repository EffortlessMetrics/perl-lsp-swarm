//! `build_measurement` — the deterministic executor-model measurement
//! protocol and harness (#11639, child of controller #9547).
//!
//! One versioned contract ([`model::PROTOCOL_VERSION`]) plus a
//! repository-owned harness ([`runner::MeasurementHarness`]) that prepares
//! and executes one declared experiment cell while retaining exact subject,
//! environment, cache, storage, process, and timing identities.
//!
//! Claim boundary (see `.spec/11639-build-executor-measurement/`): this is
//! the measuring instrument only. It changes no build command, cache path,
//! wrapper, lock, executor, or policy behavior; it selects no model; the
//! native-host observation matrices belong to #11640/#11641 and the
//! architecture decision to #11642.

pub mod model;
pub mod providers;
pub mod render;
pub mod runner;

#[cfg(test)]
mod tests;

pub use render::{render_human, render_json};

pub use model::{
    CacheAttribution, CacheCounters, CacheObservation, CapacityPolicy, CellVerdict,
    CommandIdentity, DiskAdmission, DiskRefusal, EnvironmentIdentity, ExecutedSubject,
    FilesystemFreeSpace, FilesystemIdentity, HostProfile, LockObservation, LockPolicy,
    LockPrimitive, MeasurementCell, MeasurementRecord, NotProvenReason, Operation,
    PROTOCOL_VERSION, PathRole, PathScope, ProcessObservation, RepetitionOrdinal, RowRefusal,
    SubjectIdentity, Terminality, TimingDecomposition, TimingVerdict, WorkObservation,
    WorkflowClass,
};
pub use providers::{
    CacheMetricsProvider, ClockProvider, CommandOutcome, CommandRunner, CommandSpec,
    DeterministicBarrier, FilesystemProvider, LockPrimitiveProvider, MonotonicClock,
    ProcessObserver, ScriptedCache, ScriptedClock, ScriptedFilesystems, ScriptedLocks,
    ScriptedProcess, ScriptedRunner, SystemCommandRunner,
};
pub use runner::{CacheSnapshotPolicy, CellExecution, MeasurementHarness};
