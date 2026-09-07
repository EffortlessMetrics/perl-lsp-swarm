//! Provider seams for the executor measurement harness (#11639).
//!
//! Every host-facing capability the harness needs is injected: monotonic
//! time, filesystem/volume resolution and free space, lock primitives,
//! process observation, cache metrics, command execution, and concurrency
//! barriers. The native-host observation lanes (#11640/#11641) wire real
//! providers; protocol tests inject deterministic scripted providers so no
//! fixture relies on timing sleeps, real Cargo/sccache, or host specifics.
//!
//! Real providers must obey the same fail-closed doctrine as the model: an
//! instrument that cannot observe reports `None`/unavailable, never zero.

use super::model::{
    CacheCounters, FilesystemIdentity, HostProfile, LockPrimitive, ProcessObservation,
};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

/// Monotonic clock. Implementations must return non-decreasing nanos.
pub trait ClockProvider {
    fn monotonic_nanos(&self) -> u64;
}

/// Repository-default monotonic clock ([`std::time::Instant`]).
pub struct MonotonicClock {
    start: std::time::Instant,
}

impl MonotonicClock {
    pub fn new() -> Self {
        Self { start: std::time::Instant::now() }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ClockProvider for MonotonicClock {
    fn monotonic_nanos(&self) -> u64 {
        self.start.elapsed().as_nanos().min(u64::MAX as u128) as u64
    }
}

/// Filesystem/volume resolution and free-space probing.
pub trait FilesystemProvider {
    /// The filesystem `path` actually resolves to; `None` when unresolvable.
    fn filesystem_of(&self, path: &str) -> Option<FilesystemIdentity>;
    /// Free bytes on `filesystem`; `None` when the measurement failed.
    fn free_bytes(&self, filesystem: &FilesystemIdentity) -> Option<u64>;
}

/// Lock-primitive availability and acquisition on this host.
pub trait LockPrimitiveProvider {
    /// Whether the host actually provides `primitive`.
    fn available(&self, primitive: LockPrimitive) -> bool;
    /// Monotonic nanos spent acquiring `primitive`; `None` when acquisition
    /// failed.
    fn acquire(&self, primitive: LockPrimitive) -> Option<u64>;
}

/// Process-tree observation (descendants, terminality).
pub trait ProcessObserver {
    /// Observation of the executed process tree; `None` when the instrument
    /// is unavailable (recorded as `NOT_PROVEN`, never as zero descendants).
    fn observe(&self) -> Option<ProcessObservation>;
}

/// Cache metrics (sccache-style counters) under an exact server identity.
pub trait CacheMetricsProvider {
    /// Reset counters so the next snapshot is a fresh baseline.
    fn reset(&mut self);
    /// Exact server/process identity the counters belong to.
    fn server_identity(&self) -> Option<String>;
    /// Current counter snapshot.
    fn snapshot(&self) -> Option<CacheCounters>;
    /// Unrelated users observed on this server since the harness began the
    /// cell. Any positive count forces attribution to `Unattributed`.
    fn foreign_users_observed(&self) -> u64;
}

/// The declared command for one cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

/// What one executed command observably produced. Every field is fail-closed:
/// the harness never infers an exit code, selected work, or an executed
/// commit the command layer did not actually report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    pub exit_code: Option<i32>,
    /// Selected tests/benches actually executed (parsed receipts are the
    /// host lanes' obligation; the harness only records what it is given).
    pub selected_work: Option<u64>,
    /// Exact candidate identity the executed artifact was proven to belong
    /// to. `None` means the executed subject stays unproven.
    pub executed_commit: Option<String>,
}

impl CommandOutcome {
    /// Fail-closed outcome for an unobservable run.
    pub fn unobserved() -> Self {
        Self { exit_code: None, selected_work: None, executed_commit: None }
    }
}

/// Command execution seam.
pub trait CommandRunner {
    fn run(&mut self, command: &CommandSpec) -> CommandOutcome;
}

/// Real command execution via [`std::process::Command`]. Exit codes only:
/// selected-work and executed-subject identity require receipt parsing that
/// the host lanes own, so those stay `None` (fail-closed) here.
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&mut self, command: &CommandSpec) -> CommandOutcome {
        let mut cmd = std::process::Command::new(&command.program);
        cmd.args(&command.args).env_clear().envs(&command.env);
        let status = cmd.status();
        let exit_code = status.ok().and_then(|s| s.code());
        CommandOutcome { exit_code, selected_work: None, executed_commit: None }
    }
}

/// Deterministic concurrency coordination for multi-cell (concurrency)
/// experiments. Participants arrive by name; the return of [`Self::arrive`]
/// is the release signal once the required set has arrived.
///
/// This is deliberately **not a blocking wait primitive**: blocking would
/// require real threads or sleeps, which the protocol forbids as a
/// nondeterministic oracle. Concurrency cells are driven as deterministic
/// interleaved scripts; the barrier proves arrival ordering, and actual
/// parallel scheduling stays with the host observation lanes.
#[derive(Debug)]
pub struct DeterministicBarrier {
    required: usize,
    arrivals: Mutex<BTreeMap<String, ()>>,
}

impl DeterministicBarrier {
    pub fn new(required: usize) -> Self {
        Self { required, arrivals: Mutex::new(BTreeMap::new()) }
    }

    /// Record one arrival. Returns `true` exactly when this arrival completes
    /// the required set (the release signal).
    pub fn arrive(&self, participant: &str) -> bool {
        let mut arrivals = match self.arrivals.lock() {
            Ok(guard) => guard,
            // A poisoned barrier means a participant panicked mid-cell; the
            // honest answer is "never release".
            Err(_) => return false,
        };
        arrivals.insert(participant.to_string(), ());
        arrivals.len() >= self.required
    }
}

/// Scripted monotonic clock: pops scripted nanos in order. When the script
/// runs dry it repeats the final value, preserving the monotonic-clock
/// contract (a dry script never rewinds time to zero).
pub struct ScriptedClock {
    steps: Mutex<VecDeque<u64>>,
}

impl ScriptedClock {
    pub fn new(steps: Vec<u64>) -> Self {
        Self { steps: Mutex::new(VecDeque::from(steps)) }
    }
}

impl ClockProvider for ScriptedClock {
    fn monotonic_nanos(&self) -> u64 {
        let mut steps = match self.steps.lock() {
            Ok(guard) => guard,
            Err(_) => return 0,
        };
        if steps.len() > 1 {
            return steps.pop_front().unwrap_or_default();
        }
        // Last scripted value repeats indefinitely (empty script reads 0).
        *steps.front().unwrap_or(&0)
    }
}

/// Scripted filesystem provider: exact path-to-filesystem mapping plus a
/// free-space table that can model failed measurements (`None` values).
pub struct ScriptedFilesystems {
    path_map: BTreeMap<String, FilesystemIdentity>,
    free_map: BTreeMap<String, Option<u64>>,
}

impl ScriptedFilesystems {
    pub fn new(
        path_map: BTreeMap<String, FilesystemIdentity>,
        free_map: BTreeMap<String, Option<u64>>,
    ) -> Self {
        Self { path_map, free_map }
    }
}

impl FilesystemProvider for ScriptedFilesystems {
    fn filesystem_of(&self, path: &str) -> Option<FilesystemIdentity> {
        self.path_map.get(path).cloned()
    }

    fn free_bytes(&self, filesystem: &FilesystemIdentity) -> Option<u64> {
        self.free_map.get(&filesystem.0).copied().flatten()
    }
}

/// Scripted lock provider: models primitive availability and an acquisition
/// wait. `wait_nanos` is returned only when `available` holds.
pub struct ScriptedLocks {
    pub flock_available: bool,
    pub acquire_wait_nanos: u64,
}

impl LockPrimitiveProvider for ScriptedLocks {
    fn available(&self, primitive: LockPrimitive) -> bool {
        match primitive {
            LockPrimitive::WholeProcessFlock => self.flock_available,
            LockPrimitive::FileLock => true,
        }
    }

    fn acquire(&self, primitive: LockPrimitive) -> Option<u64> {
        if self.available(primitive) { Some(self.acquire_wait_nanos) } else { None }
    }
}

/// Scripted process observer: either a fixed observation or `None`
/// (instrument unavailable).
pub struct ScriptedProcess {
    pub observation: Option<ProcessObservation>,
}

impl ProcessObserver for ScriptedProcess {
    fn observe(&self) -> Option<ProcessObservation> {
        self.observation.clone()
    }
}

/// Scripted cache metrics: scripted successive server identities (baseline
/// then delta — a changed second identity models a restart/reconnection),
/// scripted successive counter snapshots, and a foreign-user count observed
/// between snapshots. A dry script yields `None` (instrument unavailable —
/// fail-closed).
pub struct ScriptedCache {
    server_identities: Mutex<VecDeque<Option<String>>>,
    snapshots: Mutex<VecDeque<CacheCounters>>,
    pub foreign_users: u64,
}

impl ScriptedCache {
    pub fn new(
        server_identities: Vec<Option<String>>,
        snapshots: Vec<CacheCounters>,
        foreign_users: u64,
    ) -> Self {
        Self {
            server_identities: Mutex::new(VecDeque::from(server_identities)),
            snapshots: Mutex::new(VecDeque::from(snapshots)),
            foreign_users,
        }
    }
}

impl CacheMetricsProvider for ScriptedCache {
    fn reset(&mut self) {}

    fn server_identity(&self) -> Option<String> {
        let mut identities = match self.server_identities.lock() {
            Ok(guard) => guard,
            Err(_) => return None,
        };
        if identities.len() > 1 {
            return identities.pop_front().unwrap_or(None);
        }
        identities.front().cloned().flatten()
    }

    fn snapshot(&self) -> Option<CacheCounters> {
        let mut snapshots = match self.snapshots.lock() {
            Ok(guard) => guard,
            Err(_) => return None,
        };
        snapshots.pop_front()
    }

    fn foreign_users_observed(&self) -> u64 {
        self.foreign_users
    }
}

/// Scripted command runner: pops scripted outcomes in order; a dry script
/// yields the fail-closed unobserved outcome.
pub struct ScriptedRunner {
    outcomes: Mutex<VecDeque<CommandOutcome>>,
}

impl ScriptedRunner {
    pub fn new(outcomes: Vec<CommandOutcome>) -> Self {
        Self { outcomes: Mutex::new(VecDeque::from(outcomes)) }
    }
}

impl CommandRunner for ScriptedRunner {
    fn run(&mut self, _command: &CommandSpec) -> CommandOutcome {
        let mut outcomes = match self.outcomes.lock() {
            Ok(guard) => guard,
            Err(_) => return CommandOutcome::unobserved(),
        };
        outcomes.pop_front().unwrap_or_else(CommandOutcome::unobserved)
    }
}

/// Convenience fixture subject host constant for tests and examples.
pub const FIXTURE_HOST: HostProfile = HostProfile::NativePosix;
