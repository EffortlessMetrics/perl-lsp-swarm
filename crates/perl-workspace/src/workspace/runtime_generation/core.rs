//! Process-local workspace-root generation and publication eligibility.
//!
//! This module is deliberately transport-neutral. It owns only the identity and
//! lifecycle proposition needed to decide whether root-scoped work still belongs
//! to the current process/session generation. Workspace facts, configuration,
//! reload semantics, persistence, hydration, provider readiness, and protocol
//! projection remain with their existing owners.
//!
//! The module is hidden from generated package documentation while the v0.x
//! cross-crate integration settles. Its public Rust visibility exists only so
//! the LSP composition crate can consume the workspace-owned authority.

use parking_lot::{Mutex, RwLock};
use std::{
    collections::{BTreeMap, VecDeque},
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const MAX_OBSERVATIONS: usize = 128;

macro_rules! opaque_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
            /// Construct an opaque process-local identity.
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            /// Return the opaque numeric value.
            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

opaque_id!(
    /// Identifies one server/application session. It is never portable semantic state.
    WorkspaceRuntimeSessionId
);
opaque_id!(
    /// Identifies one logical workspace independently from a display path or URI.
    LogicalWorkspaceId
);
opaque_id!(
    /// Identifies one logical workspace root independently from a display path or URI.
    WorkspaceRootId
);
opaque_id!(
    /// Identifies the accepted workspace-folder set generation.
    WorkspaceFolderSetGeneration
);
opaque_id!(
    /// Identifies the accepted configuration generation.
    WorkspaceConfigurationGeneration
);
opaque_id!(
    /// Identifies the accepted trust-policy generation.
    WorkspaceTrustGeneration
);
opaque_id!(
    /// Identifies the accepted project-environment snapshot.
    WorkspaceEnvironmentIdentity
);
opaque_id!(
    /// Identifies the host/session product profile relevant to root behavior.
    WorkspaceHostProfileId
);
opaque_id!(
    /// Identifies the accepted source/path authority contract version.
    WorkspaceSourceAuthorityVersion
);
opaque_id!(
    /// Identifies one caller-defined root-scoped operation.
    WorkspaceRuntimeOperationId
);
opaque_id!(
    /// Identifies one registered root-scoped application task.
    WorkspaceRuntimeTaskId
);

/// Immutable behavior-bearing inputs accepted for one root-runtime generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceRuntimeInputs {
    workspace_id: LogicalWorkspaceId,
    workspace_folder_generation: WorkspaceFolderSetGeneration,
    configuration_generation: WorkspaceConfigurationGeneration,
    trust_generation: WorkspaceTrustGeneration,
    environment_identity: WorkspaceEnvironmentIdentity,
    host_profile: WorkspaceHostProfileId,
    source_authority_version: WorkspaceSourceAuthorityVersion,
}

impl WorkspaceRuntimeInputs {
    /// Construct the accepted input identity for a replacement generation.
    pub const fn new(
        workspace_id: LogicalWorkspaceId,
        workspace_folder_generation: WorkspaceFolderSetGeneration,
        configuration_generation: WorkspaceConfigurationGeneration,
        trust_generation: WorkspaceTrustGeneration,
        environment_identity: WorkspaceEnvironmentIdentity,
        host_profile: WorkspaceHostProfileId,
        source_authority_version: WorkspaceSourceAuthorityVersion,
    ) -> Self {
        Self {
            workspace_id,
            workspace_folder_generation,
            configuration_generation,
            trust_generation,
            environment_identity,
            host_profile,
            source_authority_version,
        }
    }

    /// Return the logical workspace identity.
    pub const fn workspace_id(self) -> LogicalWorkspaceId {
        self.workspace_id
    }

    /// Return the accepted workspace-folder generation.
    pub const fn workspace_folder_generation(self) -> WorkspaceFolderSetGeneration {
        self.workspace_folder_generation
    }

    /// Return the accepted configuration generation.
    pub const fn configuration_generation(self) -> WorkspaceConfigurationGeneration {
        self.configuration_generation
    }

    /// Return the accepted trust generation.
    pub const fn trust_generation(self) -> WorkspaceTrustGeneration {
        self.trust_generation
    }

    /// Return the accepted project-environment identity.
    pub const fn environment_identity(self) -> WorkspaceEnvironmentIdentity {
        self.environment_identity
    }

    /// Return the accepted host/session profile.
    pub const fn host_profile(self) -> WorkspaceHostProfileId {
        self.host_profile
    }

    /// Return the accepted source/path authority version.
    pub const fn source_authority_version(self) -> WorkspaceSourceAuthorityVersion {
        self.source_authority_version
    }
}

/// Process-local generation identity for one logical workspace root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceRuntimeGeneration {
    session_id: WorkspaceRuntimeSessionId,
    root_id: WorkspaceRootId,
    sequence: u64,
}

impl WorkspaceRuntimeGeneration {
    /// Return the owning process/application session.
    pub const fn session_id(self) -> WorkspaceRuntimeSessionId {
        self.session_id
    }

    /// Return the logical root identity.
    pub const fn root_id(self) -> WorkspaceRootId {
        self.root_id
    }

    /// Return the process-local monotonically increasing generation sequence.
    pub const fn sequence(self) -> u64 {
        self.sequence
    }
}

/// Current lifecycle state of one root-runtime generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRuntimeLifecycleState {
    /// The replacement generation exists but has not admitted domain work.
    Creating,
    /// Process-free local configuration and authority are being established.
    InitializingLocalAuthority,
    /// A compatible historical snapshot is being considered.
    LoadingSnapshotCandidate,
    /// Current semantic/project state is being indexed or recomputed.
    IndexingOrRecomputing,
    /// The root generation is current and available for exact use.
    ActiveCurrent,
    /// Accepted configuration inputs are changing.
    Reconfiguring,
    /// Accepted trust inputs are changing.
    TrustTransition,
    /// Exact use is unavailable while terminal cleanup proceeds.
    Removing,
    /// The root is detached and no work may publish.
    Detached,
    /// The application session is shutting down and no work may publish.
    Shutdown,
    /// The generation is current only in an explicit failed or limited state.
    FailedOrLimited,
}

impl WorkspaceRuntimeLifecycleState {
    fn accepts_task_registration(self) -> bool {
        !matches!(self, Self::Removing | Self::Detached | Self::Shutdown)
    }

    fn accepts_publication(self) -> bool {
        !matches!(self, Self::Creating | Self::Removing | Self::Detached | Self::Shutdown)
    }

    fn exact_use_available(self) -> bool {
        matches!(self, Self::ActiveCurrent)
    }

    fn is_terminal(self) -> bool {
        matches!(self, Self::Detached | Self::Shutdown)
    }
}

/// Reason an accepted root transition minted a replacement generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRuntimeTransitionReason {
    /// Admit a newly discovered logical root.
    AddRoot,
    /// Replace the accepted workspace-folder set.
    WorkspaceFoldersChanged,
    /// Replace accepted configuration inputs.
    ConfigurationChanged,
    /// Replace accepted trust inputs.
    TrustChanged,
    /// Replace accepted project-environment inputs.
    EnvironmentChanged,
    /// Replace the underlying root authority while retaining the logical root identity.
    ReplaceRoot,
    /// Begin terminal root removal.
    RemoveRoot,
    /// Create a fresh process/session generation after restart.
    Restart,
    /// Retry a failed or limited current root.
    Recover,
}

impl WorkspaceRuntimeTransitionReason {
    fn initial_state(self) -> WorkspaceRuntimeLifecycleState {
        match self {
            Self::AddRoot | Self::Restart | Self::Recover => {
                WorkspaceRuntimeLifecycleState::Creating
            }
            Self::WorkspaceFoldersChanged
            | Self::ConfigurationChanged
            | Self::EnvironmentChanged
            | Self::ReplaceRoot => WorkspaceRuntimeLifecycleState::Reconfiguring,
            Self::TrustChanged => WorkspaceRuntimeLifecycleState::TrustTransition,
            Self::RemoveRoot => WorkspaceRuntimeLifecycleState::Removing,
        }
    }
}

/// Root-scoped publication classes guarded by the current generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRuntimePublicationKind {
    /// Canonical workspace or project facts.
    WorkspaceFacts,
    /// Provider/readiness state owned by the canonical readiness authority.
    Readiness,
    /// Accepted configuration state.
    Configuration,
    /// Watcher registration or detach state.
    WatcherRegistration,
    /// Accepted reload replacement state.
    ReloadReplacement,
    /// Compatible snapshot adoption.
    HydrationAdoption,
    /// A durable checkpoint current-pointer transition.
    CheckpointCurrentPointer,
    /// Root-owned operational or presentation cache state.
    OperationalCache,
    /// An externally visible provider result.
    ProviderResult,
}

/// Terminal reason attached to root detach or supersession evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRuntimeTerminalReason {
    /// A newer generation superseded this generation.
    Superseded,
    /// The logical root was removed.
    Removed,
    /// The root authority was replaced.
    Replaced,
    /// The application session shut down.
    Shutdown,
    /// The generation failed and was explicitly detached.
    Failed,
}

/// One bounded observation emitted by the root-runtime authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceRuntimeObservation {
    sequence: u64,
    generation: WorkspaceRuntimeGeneration,
    kind: WorkspaceRuntimeObservationKind,
    detail: WorkspaceRuntimeObservationDetail,
}

impl WorkspaceRuntimeObservation {
    /// Return the observation sequence within this controller.
    pub const fn sequence(self) -> u64 {
        self.sequence
    }

    /// Return the generation involved in the observation.
    pub const fn generation(self) -> WorkspaceRuntimeGeneration {
        self.generation
    }

    /// Return the observation kind.
    pub const fn kind(self) -> WorkspaceRuntimeObservationKind {
        self.kind
    }

    /// Return the typed observation detail.
    pub const fn detail(self) -> WorkspaceRuntimeObservationDetail {
        self.detail
    }
}

/// Coarse root-runtime observation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRuntimeObservationKind {
    /// A replacement generation was created.
    TransitionStarted,
    /// A prior generation was superseded and its tasks were cancelled.
    GenerationSuperseded,
    /// A root-scoped task was registered.
    TaskRegistered,
    /// A root-scoped task completed.
    TaskCompleted,
    /// A publication passed the current-generation guard.
    PublicationAccepted,
    /// A publication failed the current-generation guard.
    PublicationRejected,
    /// The current generation changed lifecycle state.
    StateChanged,
    /// The current generation detached.
    Detached,
    /// The application session shut down.
    Shutdown,
}

/// Typed detail attached to a root-runtime observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRuntimeObservationDetail {
    /// The accepted transition reason.
    Transition(WorkspaceRuntimeTransitionReason),
    /// A root-scoped task and its caller-defined operation identity.
    Task {
        /// Registered task identity.
        task_id: WorkspaceRuntimeTaskId,
        /// Caller-defined operation identity.
        operation_id: WorkspaceRuntimeOperationId,
    },
    /// A guarded publication class.
    Publication(WorkspaceRuntimePublicationKind),
    /// A resulting lifecycle state.
    State(WorkspaceRuntimeLifecycleState),
    /// A terminal disposition.
    Terminal(WorkspaceRuntimeTerminalReason),
}

/// Bounded root-runtime observation window with explicit truncation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRuntimeObservationSnapshot {
    observations: Vec<WorkspaceRuntimeObservation>,
    dropped: u64,
}

impl WorkspaceRuntimeObservationSnapshot {
    /// Return the currently retained observations in sequence order.
    pub fn observations(&self) -> &[WorkspaceRuntimeObservation] {
        &self.observations
    }

    /// Return how many older observations were dropped by the bounded window.
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// Immutable current context for a root-runtime generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceRuntimeContext {
    generation: WorkspaceRuntimeGeneration,
    inputs: WorkspaceRuntimeInputs,
}

impl WorkspaceRuntimeContext {
    /// Return the process-local root-runtime generation.
    pub const fn generation(self) -> WorkspaceRuntimeGeneration {
        self.generation
    }

    /// Return the immutable behavior-bearing inputs.
    pub const fn inputs(self) -> WorkspaceRuntimeInputs {
        self.inputs
    }
}

/// Point-in-time view of one current root generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceRuntimeView {
    context: WorkspaceRuntimeContext,
    lifecycle_state: WorkspaceRuntimeLifecycleState,
    terminal_reason: Option<WorkspaceRuntimeTerminalReason>,
    active_task_count: usize,
}

impl WorkspaceRuntimeView {
    /// Return the current immutable root context.
    pub const fn context(self) -> WorkspaceRuntimeContext {
        self.context
    }

    /// Return the current lifecycle state.
    pub const fn lifecycle_state(self) -> WorkspaceRuntimeLifecycleState {
        self.lifecycle_state
    }

    /// Return the terminal reason, when the generation has detached or shut down.
    pub const fn terminal_reason(self) -> Option<WorkspaceRuntimeTerminalReason> {
        self.terminal_reason
    }

    /// Return the number of root-scoped tasks still owned by this generation.
    pub const fn active_task_count(self) -> usize {
        self.active_task_count
    }

    /// Return whether exact provider/readiness use is currently permitted.
    pub fn exact_use_available(self) -> bool {
        self.lifecycle_state.exact_use_available()
    }
}

/// Cancellation-aware handle returned for one root-scoped task.
#[derive(Clone)]
pub struct WorkspaceRuntimeTaskHandle {
    id: WorkspaceRuntimeTaskId,
    operation_id: WorkspaceRuntimeOperationId,
    generation: WorkspaceRuntimeGeneration,
    cancelled: Arc<AtomicBool>,
}

impl WorkspaceRuntimeTaskHandle {
    /// Return the task identity.
    pub const fn id(&self) -> WorkspaceRuntimeTaskId {
        self.id
    }

    /// Return the caller-defined operation identity.
    pub const fn operation_id(&self) -> WorkspaceRuntimeOperationId {
        self.operation_id
    }

    /// Return the owning root generation.
    pub const fn generation(&self) -> WorkspaceRuntimeGeneration {
        self.generation
    }

    /// Return whether a replacement, detach, or shutdown cancelled this task.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl fmt::Debug for WorkspaceRuntimeTaskHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkspaceRuntimeTaskHandle")
            .field("id", &self.id)
            .field("operation_id", &self.operation_id)
            .field("generation", &self.generation)
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// Fail-closed root-runtime identity, lifecycle, or task error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRuntimeError {
    /// The requested logical root is not registered.
    UnknownRoot(WorkspaceRootId),
    /// The attempted generation is not the current generation for the root.
    StaleGeneration {
        /// The current generation.
        current: WorkspaceRuntimeGeneration,
        /// The attempted older or unrelated generation.
        attempted: WorkspaceRuntimeGeneration,
    },
    /// The attempted generation belongs to another process/application session.
    WrongSession {
        /// The controller session.
        current: WorkspaceRuntimeSessionId,
        /// The attempted session.
        attempted: WorkspaceRuntimeSessionId,
    },
    /// The current lifecycle state does not accept task registration.
    TaskRegistrationRejected(WorkspaceRuntimeLifecycleState),
    /// The current lifecycle state does not accept publication.
    PublicationRejected(WorkspaceRuntimeLifecycleState),
    /// The task identity is not owned by the current generation.
    UnknownTask(WorkspaceRuntimeTaskId),
    /// The process-local generation sequence is exhausted.
    GenerationExhausted,
    /// The process-local task identity sequence is exhausted.
    TaskIdentityExhausted,
    /// The application session has begun shutdown and accepts no new root work.
    ControllerShutdown,
    /// A terminal state must be entered through detach or shutdown.
    TerminalStateRequiresExplicitDisposition(WorkspaceRuntimeLifecycleState),
}

impl fmt::Display for WorkspaceRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownRoot(root_id) => {
                write!(formatter, "unknown workspace root {}", root_id.get())
            }
            Self::StaleGeneration { current, attempted } => write!(
                formatter,
                "workspace generation {} is stale; current generation is {}",
                attempted.sequence(),
                current.sequence()
            ),
            Self::WrongSession { current, attempted } => write!(
                formatter,
                "workspace generation belongs to session {}, not current session {}",
                attempted.get(),
                current.get()
            ),
            Self::TaskRegistrationRejected(state) => {
                write!(formatter, "workspace lifecycle state {state:?} rejects new tasks")
            }
            Self::PublicationRejected(state) => {
                write!(formatter, "workspace lifecycle state {state:?} rejects publication")
            }
            Self::UnknownTask(task_id) => {
                write!(formatter, "unknown workspace task {}", task_id.get())
            }
            Self::GenerationExhausted => {
                formatter.write_str("workspace runtime generation sequence exhausted")
            }
            Self::TaskIdentityExhausted => {
                formatter.write_str("workspace runtime task identity sequence exhausted")
            }
            Self::ControllerShutdown => {
                formatter.write_str("workspace runtime controller is shut down")
            }
            Self::TerminalStateRequiresExplicitDisposition(state) => {
                write!(formatter, "workspace lifecycle state {state:?} requires detach or shutdown")
            }
        }
    }
}

impl Error for WorkspaceRuntimeError {}

/// Transport-neutral owner of process-local root generations and publication eligibility.
///
/// The public surface is the facade type in the parent module; this core type
/// is crate-internal so its items stay reachable-pub clean.
#[derive(Clone)]
pub(crate) struct WorkspaceRuntimeController {
    inner: Arc<ControllerInner>,
}

impl WorkspaceRuntimeController {
    /// Construct an empty controller for one process/application session.
    pub(crate) fn new(session_id: WorkspaceRuntimeSessionId) -> Self {
        Self {
            inner: Arc::new(ControllerInner {
                session_id,
                next_generation: AtomicU64::new(1),
                next_task_id: AtomicU64::new(1),
                next_observation: AtomicU64::new(1),
                shutdown: AtomicBool::new(false),
                roots: RwLock::new(BTreeMap::new()),
                observations: Mutex::new(ObservationState {
                    observations: VecDeque::new(),
                    dropped: 0,
                }),
            }),
        }
    }

    /// Begin an accepted root transition and mint a replacement generation.
    ///
    /// Existing root-scoped tasks are cancelled after the replacement entry
    /// becomes current. Once the root map changes, old work cannot pass another
    /// current-generation check even if cancellation settlement is still in
    /// progress.
    pub(crate) fn begin_transition(
        &self,
        root_id: WorkspaceRootId,
        reason: WorkspaceRuntimeTransitionReason,
        inputs: WorkspaceRuntimeInputs,
    ) -> Result<WorkspaceRuntimeContext, WorkspaceRuntimeError> {
        let generation = self.allocate_generation(root_id)?;
        let context = WorkspaceRuntimeContext { generation, inputs };
        let replacement = Arc::new(Mutex::new(RootEntry {
            context,
            lifecycle_state: reason.initial_state(),
            terminal_reason: None,
            tasks: BTreeMap::new(),
        }));

        let prior = {
            let mut roots = self.inner.roots.write();
            if self.inner.shutdown.load(Ordering::Acquire) {
                return Err(WorkspaceRuntimeError::ControllerShutdown);
            }
            roots.insert(root_id, Arc::clone(&replacement))
        };

        if let Some(prior) = prior {
            let prior_generation = {
                let mut prior = prior.lock();
                prior.cancel_all_tasks();
                prior.lifecycle_state = WorkspaceRuntimeLifecycleState::Detached;
                prior.terminal_reason = Some(WorkspaceRuntimeTerminalReason::Superseded);
                prior.context.generation
            };
            self.push_observation(
                prior_generation,
                WorkspaceRuntimeObservationKind::GenerationSuperseded,
                WorkspaceRuntimeObservationDetail::Terminal(
                    WorkspaceRuntimeTerminalReason::Superseded,
                ),
            );
        }

        self.push_observation(
            generation,
            WorkspaceRuntimeObservationKind::TransitionStarted,
            WorkspaceRuntimeObservationDetail::Transition(reason),
        );
        Ok(context)
    }

    /// Return the current context and lifecycle state for a logical root.
    pub(crate) fn current_root_context(
        &self,
        root_id: WorkspaceRootId,
    ) -> Option<WorkspaceRuntimeView> {
        let roots = self.inner.roots.read();
        let entry = roots.get(&root_id)?;
        let view = entry.lock().view();
        Some(view)
    }

    /// Register one root-scoped task under the exact current generation.
    pub(crate) fn register_root_task(
        &self,
        generation: WorkspaceRuntimeGeneration,
        operation_id: WorkspaceRuntimeOperationId,
    ) -> Result<WorkspaceRuntimeTaskHandle, WorkspaceRuntimeError> {
        self.ensure_not_shutdown()?;
        let task_id = self.allocate_task_id()?;
        let cancelled = Arc::new(AtomicBool::new(false));

        self.with_current_entry(generation, |entry| {
            if !entry.lifecycle_state.accepts_task_registration() {
                return Err(WorkspaceRuntimeError::TaskRegistrationRejected(entry.lifecycle_state));
            }
            entry
                .tasks
                .insert(task_id, TaskEntry { operation_id, cancelled: Arc::clone(&cancelled) });
            Ok(())
        })?;

        self.push_observation(
            generation,
            WorkspaceRuntimeObservationKind::TaskRegistered,
            WorkspaceRuntimeObservationDetail::Task { task_id, operation_id },
        );

        Ok(WorkspaceRuntimeTaskHandle { id: task_id, operation_id, generation, cancelled })
    }

    /// Record terminal settlement for one current root-scoped task.
    pub(crate) fn complete_task(
        &self,
        handle: &WorkspaceRuntimeTaskHandle,
    ) -> Result<(), WorkspaceRuntimeError> {
        self.with_current_entry(handle.generation, |entry| {
            let task =
                entry.tasks.get(&handle.id).ok_or(WorkspaceRuntimeError::UnknownTask(handle.id))?;
            if task.operation_id != handle.operation_id {
                return Err(WorkspaceRuntimeError::UnknownTask(handle.id));
            }
            entry.tasks.remove(&handle.id);
            Ok(())
        })?;

        self.push_observation(
            handle.generation,
            WorkspaceRuntimeObservationKind::TaskCompleted,
            WorkspaceRuntimeObservationDetail::Task {
                task_id: handle.id,
                operation_id: handle.operation_id,
            },
        );
        Ok(())
    }

    /// Check and record whether a root-scoped publication remains eligible.
    ///
    /// Domain owners still decide semantic completeness, readiness, edit
    /// safety, and storage validity. This guard proves only that the publication
    /// belongs to the current non-terminal root generation.
    pub(crate) fn accept_publication(
        &self,
        generation: WorkspaceRuntimeGeneration,
        publication: WorkspaceRuntimePublicationKind,
    ) -> Result<(), WorkspaceRuntimeError> {
        let result = self.ensure_not_shutdown().and_then(|()| {
            self.with_current_entry(generation, |entry| {
                if !entry.lifecycle_state.accepts_publication() {
                    return Err(WorkspaceRuntimeError::PublicationRejected(entry.lifecycle_state));
                }
                Ok(())
            })
        });

        let kind = if result.is_ok() {
            WorkspaceRuntimeObservationKind::PublicationAccepted
        } else {
            WorkspaceRuntimeObservationKind::PublicationRejected
        };
        self.push_observation(
            generation,
            kind,
            WorkspaceRuntimeObservationDetail::Publication(publication),
        );
        result
    }

    /// Complete one non-terminal lifecycle phase for the current generation.
    pub(crate) fn complete_transition(
        &self,
        generation: WorkspaceRuntimeGeneration,
        resulting_state: WorkspaceRuntimeLifecycleState,
    ) -> Result<WorkspaceRuntimeView, WorkspaceRuntimeError> {
        self.ensure_not_shutdown()?;
        if resulting_state.is_terminal()
            || resulting_state == WorkspaceRuntimeLifecycleState::Removing
        {
            return Err(WorkspaceRuntimeError::TerminalStateRequiresExplicitDisposition(
                resulting_state,
            ));
        }

        let view = self.with_current_entry(generation, |entry| {
            entry.lifecycle_state = resulting_state;
            entry.terminal_reason = None;
            Ok(entry.view())
        })?;
        self.push_observation(
            generation,
            WorkspaceRuntimeObservationKind::StateChanged,
            WorkspaceRuntimeObservationDetail::State(resulting_state),
        );
        Ok(view)
    }

    /// Return whether exact provider/readiness use is currently permitted.
    pub(crate) fn exact_use_available(
        &self,
        generation: WorkspaceRuntimeGeneration,
    ) -> Result<bool, WorkspaceRuntimeError> {
        self.ensure_not_shutdown()?;
        self.with_current_entry(generation, |entry| Ok(entry.lifecycle_state.exact_use_available()))
    }

    /// Detach the exact current root generation and cancel every owned task.
    pub(crate) fn detach_root(
        &self,
        generation: WorkspaceRuntimeGeneration,
        reason: WorkspaceRuntimeTerminalReason,
    ) -> Result<WorkspaceRuntimeView, WorkspaceRuntimeError> {
        self.ensure_not_shutdown()?;
        let view = self.with_current_entry(generation, |entry| {
            entry.cancel_all_tasks();
            entry.lifecycle_state = WorkspaceRuntimeLifecycleState::Detached;
            entry.terminal_reason = Some(reason);
            Ok(entry.view())
        })?;
        self.push_observation(
            generation,
            WorkspaceRuntimeObservationKind::Detached,
            WorkspaceRuntimeObservationDetail::Terminal(reason),
        );
        Ok(view)
    }

    /// Shut down every current root generation and cancel every owned task.
    pub(crate) fn shutdown(&self) {
        if self.inner.shutdown.swap(true, Ordering::AcqRel) {
            return;
        }

        let roots = self.inner.roots.read();
        let mut generations = Vec::with_capacity(roots.len());
        for entry in roots.values() {
            let mut entry = entry.lock();
            entry.cancel_all_tasks();
            entry.lifecycle_state = WorkspaceRuntimeLifecycleState::Shutdown;
            entry.terminal_reason = Some(WorkspaceRuntimeTerminalReason::Shutdown);
            generations.push(entry.context.generation);
        }
        drop(roots);

        for generation in generations {
            self.push_observation(
                generation,
                WorkspaceRuntimeObservationKind::Shutdown,
                WorkspaceRuntimeObservationDetail::Terminal(
                    WorkspaceRuntimeTerminalReason::Shutdown,
                ),
            );
        }
    }

    /// Return the bounded current observation window and its truncation count.
    pub(crate) fn observations(&self) -> WorkspaceRuntimeObservationSnapshot {
        let state = self.inner.observations.lock();
        WorkspaceRuntimeObservationSnapshot {
            observations: state.observations.iter().copied().collect(),
            dropped: state.dropped,
        }
    }

    fn allocate_generation(
        &self,
        root_id: WorkspaceRootId,
    ) -> Result<WorkspaceRuntimeGeneration, WorkspaceRuntimeError> {
        let sequence = allocate_sequence(
            &self.inner.next_generation,
            WorkspaceRuntimeError::GenerationExhausted,
        )?;
        Ok(WorkspaceRuntimeGeneration { session_id: self.inner.session_id, root_id, sequence })
    }

    fn allocate_task_id(&self) -> Result<WorkspaceRuntimeTaskId, WorkspaceRuntimeError> {
        allocate_sequence(&self.inner.next_task_id, WorkspaceRuntimeError::TaskIdentityExhausted)
            .map(WorkspaceRuntimeTaskId::new)
    }

    fn ensure_not_shutdown(&self) -> Result<(), WorkspaceRuntimeError> {
        if self.inner.shutdown.load(Ordering::Acquire) {
            Err(WorkspaceRuntimeError::ControllerShutdown)
        } else {
            Ok(())
        }
    }

    fn with_current_entry<T>(
        &self,
        generation: WorkspaceRuntimeGeneration,
        operation: impl FnOnce(&mut RootEntry) -> Result<T, WorkspaceRuntimeError>,
    ) -> Result<T, WorkspaceRuntimeError> {
        if generation.session_id() != self.inner.session_id {
            return Err(WorkspaceRuntimeError::WrongSession {
                current: self.inner.session_id,
                attempted: generation.session_id(),
            });
        }

        let roots = self.inner.roots.read();
        let entry = roots
            .get(&generation.root_id())
            .ok_or(WorkspaceRuntimeError::UnknownRoot(generation.root_id()))?;
        let mut entry = entry.lock();
        if entry.context.generation != generation {
            return Err(WorkspaceRuntimeError::StaleGeneration {
                current: entry.context.generation,
                attempted: generation,
            });
        }
        operation(&mut entry)
    }

    fn push_observation(
        &self,
        generation: WorkspaceRuntimeGeneration,
        kind: WorkspaceRuntimeObservationKind,
        detail: WorkspaceRuntimeObservationDetail,
    ) {
        let Ok(sequence) = self.inner.next_observation.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |current| current.checked_add(1),
        ) else {
            let mut state = self.inner.observations.lock();
            state.dropped = state.dropped.saturating_add(1);
            return;
        };

        let mut state = self.inner.observations.lock();
        if state.observations.len() == MAX_OBSERVATIONS {
            state.observations.pop_front();
            state.dropped = state.dropped.saturating_add(1);
        }
        state.observations.push_back(WorkspaceRuntimeObservation {
            sequence,
            generation,
            kind,
            detail,
        });
    }
}

impl fmt::Debug for WorkspaceRuntimeController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkspaceRuntimeController")
            .field("session_id", &self.inner.session_id)
            .field("root_count", &self.inner.roots.read().len())
            .field("observation_count", &self.inner.observations.lock().observations.len())
            .finish()
    }
}

struct ControllerInner {
    session_id: WorkspaceRuntimeSessionId,
    next_generation: AtomicU64,
    next_task_id: AtomicU64,
    next_observation: AtomicU64,
    shutdown: AtomicBool,
    roots: RwLock<BTreeMap<WorkspaceRootId, Arc<Mutex<RootEntry>>>>,
    observations: Mutex<ObservationState>,
}

struct RootEntry {
    context: WorkspaceRuntimeContext,
    lifecycle_state: WorkspaceRuntimeLifecycleState,
    terminal_reason: Option<WorkspaceRuntimeTerminalReason>,
    tasks: BTreeMap<WorkspaceRuntimeTaskId, TaskEntry>,
}

impl RootEntry {
    fn view(&self) -> WorkspaceRuntimeView {
        WorkspaceRuntimeView {
            context: self.context,
            lifecycle_state: self.lifecycle_state,
            terminal_reason: self.terminal_reason,
            active_task_count: self.tasks.len(),
        }
    }

    fn cancel_all_tasks(&mut self) {
        for task in self.tasks.values() {
            task.cancelled.store(true, Ordering::Release);
        }
        self.tasks.clear();
    }
}

struct TaskEntry {
    operation_id: WorkspaceRuntimeOperationId,
    cancelled: Arc<AtomicBool>,
}

struct ObservationState {
    observations: VecDeque<WorkspaceRuntimeObservation>,
    dropped: u64,
}

fn allocate_sequence(
    counter: &AtomicU64,
    exhausted: WorkspaceRuntimeError,
) -> Result<u64, WorkspaceRuntimeError> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| current.checked_add(1))
        .map_err(|_| exhausted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    fn inputs(configuration: u64, trust: u64, environment: u64) -> WorkspaceRuntimeInputs {
        WorkspaceRuntimeInputs::new(
            LogicalWorkspaceId::new(1),
            WorkspaceFolderSetGeneration::new(1),
            WorkspaceConfigurationGeneration::new(configuration),
            WorkspaceTrustGeneration::new(trust),
            WorkspaceEnvironmentIdentity::new(environment),
            WorkspaceHostProfileId::new(1),
            WorkspaceSourceAuthorityVersion::new(1),
        )
    }

    fn active_root(
        controller: &WorkspaceRuntimeController,
        root_id: WorkspaceRootId,
    ) -> Result<WorkspaceRuntimeContext> {
        let context = controller.begin_transition(
            root_id,
            WorkspaceRuntimeTransitionReason::AddRoot,
            inputs(1, 1, 1),
        )?;
        controller.complete_transition(
            context.generation(),
            WorkspaceRuntimeLifecycleState::ActiveCurrent,
        )?;
        Ok(context)
    }

    #[test]
    fn create_root_and_reach_active_current() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(11));
        let root_id = WorkspaceRootId::new(7);
        let context = active_root(&controller, root_id)?;

        assert!(controller.is_current(context.generation()));
        assert!(controller.exact_use_available(context.generation())?);
        controller.accept_publication(
            context.generation(),
            WorkspaceRuntimePublicationKind::WorkspaceFacts,
        )?;
        let view = controller
            .current_root_context(root_id)
            .ok_or(WorkspaceRuntimeError::UnknownRoot(root_id))?;
        assert_eq!(view.lifecycle_state(), WorkspaceRuntimeLifecycleState::ActiveCurrent);
        assert_eq!(view.terminal_reason(), None);
        Ok(())
    }

    #[test]
    fn replacement_rejects_old_publication_and_cancels_old_task() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(12));
        let root_id = WorkspaceRootId::new(8);
        let first = active_root(&controller, root_id)?;
        let task = controller
            .register_root_task(first.generation(), WorkspaceRuntimeOperationId::new(101))?;

        let replacement = controller.begin_transition(
            root_id,
            WorkspaceRuntimeTransitionReason::ConfigurationChanged,
            inputs(2, 1, 1),
        )?;

        assert_ne!(first.generation(), replacement.generation());
        assert!(task.is_cancelled());
        assert!(matches!(
            controller.accept_publication(
                first.generation(),
                WorkspaceRuntimePublicationKind::WorkspaceFacts,
            ),
            Err(WorkspaceRuntimeError::StaleGeneration { .. })
        ));
        Ok(())
    }

    #[test]
    fn task_preserves_caller_operation_identity() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(13));
        let root_id = WorkspaceRootId::new(9);
        let current = active_root(&controller, root_id)?;
        let operation_id = WorkspaceRuntimeOperationId::new(9001);
        let task = controller.register_root_task(current.generation(), operation_id)?;

        assert_eq!(task.operation_id(), operation_id);
        controller.complete_task(&task)?;
        Ok(())
    }

    #[test]
    fn trust_transition_cancels_registered_work() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(14));
        let root_id = WorkspaceRootId::new(10);
        let current = active_root(&controller, root_id)?;
        let task = controller
            .register_root_task(current.generation(), WorkspaceRuntimeOperationId::new(102))?;

        let replacement = controller.begin_transition(
            root_id,
            WorkspaceRuntimeTransitionReason::TrustChanged,
            inputs(1, 2, 1),
        )?;

        assert!(task.is_cancelled());
        assert_eq!(
            controller
                .current_root_context(root_id)
                .ok_or(WorkspaceRuntimeError::UnknownRoot(root_id))?
                .lifecycle_state(),
            WorkspaceRuntimeLifecycleState::TrustTransition
        );
        assert!(!controller.exact_use_available(replacement.generation())?);
        Ok(())
    }

    #[test]
    fn removal_invalidates_exact_use_before_detach() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(15));
        let root_id = WorkspaceRootId::new(11);
        let _ = active_root(&controller, root_id)?;

        let removing = controller.begin_transition(
            root_id,
            WorkspaceRuntimeTransitionReason::RemoveRoot,
            inputs(1, 1, 1),
        )?;

        assert!(!controller.exact_use_available(removing.generation())?);
        assert!(matches!(
            controller
                .register_root_task(removing.generation(), WorkspaceRuntimeOperationId::new(103),),
            Err(WorkspaceRuntimeError::TaskRegistrationRejected(
                WorkspaceRuntimeLifecycleState::Removing
            ))
        ));
        assert!(matches!(
            controller.accept_publication(
                removing.generation(),
                WorkspaceRuntimePublicationKind::Readiness,
            ),
            Err(WorkspaceRuntimeError::PublicationRejected(
                WorkspaceRuntimeLifecycleState::Removing
            ))
        ));
        Ok(())
    }

    #[test]
    fn detach_rejects_every_late_task_and_publication() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(16));
        let root_id = WorkspaceRootId::new(12);
        let current = active_root(&controller, root_id)?;
        let task = controller
            .register_root_task(current.generation(), WorkspaceRuntimeOperationId::new(104))?;

        let view = controller
            .detach_root(current.generation(), WorkspaceRuntimeTerminalReason::Removed)?;

        assert!(task.is_cancelled());
        assert_eq!(view.terminal_reason(), Some(WorkspaceRuntimeTerminalReason::Removed));
        assert!(matches!(
            controller
                .register_root_task(current.generation(), WorkspaceRuntimeOperationId::new(105),),
            Err(WorkspaceRuntimeError::TaskRegistrationRejected(
                WorkspaceRuntimeLifecycleState::Detached
            ))
        ));
        assert!(matches!(
            controller.accept_publication(
                current.generation(),
                WorkspaceRuntimePublicationKind::ProviderResult,
            ),
            Err(WorkspaceRuntimeError::PublicationRejected(
                WorkspaceRuntimeLifecycleState::Detached
            ))
        ));
        Ok(())
    }

    #[test]
    fn rapid_remove_and_readd_same_root_mints_distinct_generation() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(17));
        let root_id = WorkspaceRootId::new(13);
        let first = active_root(&controller, root_id)?;

        let removing = controller.begin_transition(
            root_id,
            WorkspaceRuntimeTransitionReason::RemoveRoot,
            inputs(1, 1, 1),
        )?;
        controller.detach_root(removing.generation(), WorkspaceRuntimeTerminalReason::Removed)?;
        let readded = controller.begin_transition(
            root_id,
            WorkspaceRuntimeTransitionReason::AddRoot,
            inputs(1, 1, 1),
        )?;

        assert_ne!(first.generation(), readded.generation());
        assert_ne!(removing.generation(), readded.generation());
        assert!(matches!(
            controller.accept_publication(
                first.generation(),
                WorkspaceRuntimePublicationKind::WorkspaceFacts,
            ),
            Err(WorkspaceRuntimeError::StaleGeneration { .. })
        ));
        Ok(())
    }

    #[test]
    fn root_transitions_are_isolated() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(18));
        let root_a = WorkspaceRootId::new(14);
        let root_b = WorkspaceRootId::new(15);
        let first_a = active_root(&controller, root_a)?;
        let current_b = active_root(&controller, root_b)?;
        let task_b = controller
            .register_root_task(current_b.generation(), WorkspaceRuntimeOperationId::new(106))?;

        let replacement_a = controller.begin_transition(
            root_a,
            WorkspaceRuntimeTransitionReason::EnvironmentChanged,
            inputs(1, 1, 2),
        )?;

        assert_ne!(first_a.generation(), replacement_a.generation());
        assert!(!task_b.is_cancelled());
        assert!(controller.is_current(current_b.generation()));
        controller.accept_publication(
            current_b.generation(),
            WorkspaceRuntimePublicationKind::WorkspaceFacts,
        )?;
        Ok(())
    }

    #[test]
    fn task_terminal_removes_owned_task() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(19));
        let root_id = WorkspaceRootId::new(16);
        let current = active_root(&controller, root_id)?;
        let task = controller
            .register_root_task(current.generation(), WorkspaceRuntimeOperationId::new(107))?;

        assert_eq!(
            controller
                .current_root_context(root_id)
                .ok_or(WorkspaceRuntimeError::UnknownRoot(root_id))?
                .active_task_count(),
            1
        );
        controller.complete_task(&task)?;
        assert_eq!(
            controller
                .current_root_context(root_id)
                .ok_or(WorkspaceRuntimeError::UnknownRoot(root_id))?
                .active_task_count(),
            0
        );
        Ok(())
    }

    #[test]
    fn another_session_generation_fails_closed() -> Result<()> {
        let first = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(20));
        let second = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(21));
        let root_id = WorkspaceRootId::new(17);
        let foreign = active_root(&first, root_id)?;

        assert!(matches!(
            second.accept_publication(
                foreign.generation(),
                WorkspaceRuntimePublicationKind::WorkspaceFacts,
            ),
            Err(WorkspaceRuntimeError::WrongSession { .. })
        ));
        Ok(())
    }

    #[test]
    fn shutdown_cancels_tasks_and_rejects_publication() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(22));
        let root_id = WorkspaceRootId::new(18);
        let current = active_root(&controller, root_id)?;
        let task = controller
            .register_root_task(current.generation(), WorkspaceRuntimeOperationId::new(108))?;

        controller.shutdown();

        assert!(task.is_cancelled());
        assert!(matches!(
            controller.accept_publication(
                current.generation(),
                WorkspaceRuntimePublicationKind::Readiness,
            ),
            Err(WorkspaceRuntimeError::ControllerShutdown)
        ));
        Ok(())
    }

    #[test]
    fn shutdown_rejects_new_root_transitions() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(23));
        controller.shutdown();

        assert!(matches!(
            controller.begin_transition(
                WorkspaceRootId::new(19),
                WorkspaceRuntimeTransitionReason::AddRoot,
                inputs(1, 1, 1),
            ),
            Err(WorkspaceRuntimeError::ControllerShutdown)
        ));
        Ok(())
    }

    #[test]
    fn observation_window_is_bounded_and_reports_truncation() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(23));
        let root_id = WorkspaceRootId::new(19);
        let current = active_root(&controller, root_id)?;

        for _ in 0..(MAX_OBSERVATIONS + 32) {
            controller.accept_publication(
                current.generation(),
                WorkspaceRuntimePublicationKind::OperationalCache,
            )?;
        }

        let snapshot = controller.observations();
        assert_eq!(snapshot.observations().len(), MAX_OBSERVATIONS);
        assert!(snapshot.dropped() > 0);
        assert!(
            snapshot
                .observations()
                .windows(2)
                .all(|window| window[0].sequence() < window[1].sequence())
        );
        Ok(())
    }

    #[test]
    fn root_entries_are_sharded_below_a_shared_read_map() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(24));
        let root_a = WorkspaceRootId::new(20);
        let root_b = WorkspaceRootId::new(21);
        let _ = active_root(&controller, root_a)?;
        let current_b = active_root(&controller, root_b)?;

        let root_a_entry = {
            let roots = controller.inner.roots.read();
            Arc::clone(roots.get(&root_a).ok_or(WorkspaceRuntimeError::UnknownRoot(root_a))?)
        };
        let _root_a_guard = root_a_entry.lock();

        assert!(controller.is_current(current_b.generation()));
        Ok(())
    }
}
