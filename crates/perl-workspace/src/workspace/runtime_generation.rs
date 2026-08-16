//! Process-local workspace-root generation and publication eligibility.
//!
//! The implementation core owns root identity, lifecycle, tasks, and publication
//! eligibility. This module adds the application-lifecycle admission barrier:
//! ordinary root operations share a read gate, while shutdown takes the write
//! gate before closing the core. An operation therefore linearizes wholly before
//! shutdown or observes the closed controller; it cannot pass an atomic check and
//! be admitted after shutdown has already begun.

mod core;

use parking_lot::RwLock;
use std::{fmt, sync::Arc};

pub use core::{
    LogicalWorkspaceId, WorkspaceConfigurationGeneration, WorkspaceEnvironmentIdentity,
    WorkspaceFolderSetGeneration, WorkspaceHostProfileId, WorkspaceRootId,
    WorkspaceRuntimeContext, WorkspaceRuntimeError, WorkspaceRuntimeGeneration,
    WorkspaceRuntimeInputs, WorkspaceRuntimeLifecycleState, WorkspaceRuntimeObservation,
    WorkspaceRuntimeObservationDetail, WorkspaceRuntimeObservationKind,
    WorkspaceRuntimeObservationSnapshot, WorkspaceRuntimeOperationId,
    WorkspaceRuntimePublicationKind, WorkspaceRuntimeSessionId, WorkspaceRuntimeTaskHandle,
    WorkspaceRuntimeTaskId, WorkspaceRuntimeTerminalReason, WorkspaceRuntimeTransitionReason,
    WorkspaceRuntimeView, WorkspaceSourceAuthorityVersion, WorkspaceTrustGeneration,
};

/// Process-local root authority with one shutdown/admission linearization gate.
#[derive(Clone)]
pub struct WorkspaceRuntimeController {
    inner: core::WorkspaceRuntimeController,
    lifecycle_gate: Arc<RwLock<()>>,
}

impl WorkspaceRuntimeController {
    /// Construct an empty controller for one process/application session.
    pub fn new(session_id: WorkspaceRuntimeSessionId) -> Self {
        Self {
            inner: core::WorkspaceRuntimeController::new(session_id),
            lifecycle_gate: Arc::new(RwLock::new(())),
        }
    }

    /// Begin an accepted root transition and mint a replacement generation.
    pub fn begin_transition(
        &self,
        root_id: WorkspaceRootId,
        reason: WorkspaceRuntimeTransitionReason,
        inputs: WorkspaceRuntimeInputs,
    ) -> Result<WorkspaceRuntimeContext, WorkspaceRuntimeError> {
        let _lifecycle = self.lifecycle_gate.read();
        self.inner.begin_transition(root_id, reason, inputs)
    }

    /// Return the current context and lifecycle state for a logical root.
    pub fn current_root_context(&self, root_id: WorkspaceRootId) -> Option<WorkspaceRuntimeView> {
        let _lifecycle = self.lifecycle_gate.read();
        self.inner.current_root_context(root_id)
    }

    /// Return whether the supplied generation is current and non-terminal.
    pub fn is_current(&self, generation: WorkspaceRuntimeGeneration) -> bool {
        let _lifecycle = self.lifecycle_gate.read();
        let Some(view) = self.inner.current_root_context(generation.root_id()) else {
            return false;
        };
        view.context().generation() == generation
            && !matches!(
                view.lifecycle_state(),
                WorkspaceRuntimeLifecycleState::Detached | WorkspaceRuntimeLifecycleState::Shutdown
            )
    }

    /// Register one root-scoped task under the exact current generation.
    pub fn register_root_task(
        &self,
        generation: WorkspaceRuntimeGeneration,
        operation_id: WorkspaceRuntimeOperationId,
    ) -> Result<WorkspaceRuntimeTaskHandle, WorkspaceRuntimeError> {
        let _lifecycle = self.lifecycle_gate.read();
        self.inner.register_root_task(generation, operation_id)
    }

    /// Record terminal settlement for one current root-scoped task.
    pub fn complete_task(
        &self,
        handle: &WorkspaceRuntimeTaskHandle,
    ) -> Result<(), WorkspaceRuntimeError> {
        let _lifecycle = self.lifecycle_gate.read();
        self.inner.complete_task(handle)
    }

    /// Check whether a root-scoped publication remains eligible.
    pub fn accept_publication(
        &self,
        generation: WorkspaceRuntimeGeneration,
        publication: WorkspaceRuntimePublicationKind,
    ) -> Result<(), WorkspaceRuntimeError> {
        let _lifecycle = self.lifecycle_gate.read();
        self.inner.accept_publication(generation, publication)
    }

    /// Complete one non-terminal lifecycle phase for the current generation.
    pub fn complete_transition(
        &self,
        generation: WorkspaceRuntimeGeneration,
        resulting_state: WorkspaceRuntimeLifecycleState,
    ) -> Result<WorkspaceRuntimeView, WorkspaceRuntimeError> {
        let _lifecycle = self.lifecycle_gate.read();
        self.inner.complete_transition(generation, resulting_state)
    }

    /// Return whether exact provider/readiness use is currently permitted.
    pub fn exact_use_available(
        &self,
        generation: WorkspaceRuntimeGeneration,
    ) -> Result<bool, WorkspaceRuntimeError> {
        let _lifecycle = self.lifecycle_gate.read();
        self.inner.exact_use_available(generation)
    }

    /// Detach the exact current root generation and cancel every owned task.
    pub fn detach_root(
        &self,
        generation: WorkspaceRuntimeGeneration,
        reason: WorkspaceRuntimeTerminalReason,
    ) -> Result<WorkspaceRuntimeView, WorkspaceRuntimeError> {
        let _lifecycle = self.lifecycle_gate.read();
        self.inner.detach_root(generation, reason)
    }

    /// Shut down every root after excluding all concurrent admission paths.
    pub fn shutdown(&self) {
        let _lifecycle = self.lifecycle_gate.write();
        self.inner.shutdown();
    }

    /// Return the bounded current observation window and its truncation count.
    pub fn observations(&self) -> WorkspaceRuntimeObservationSnapshot {
        let _lifecycle = self.lifecycle_gate.read();
        self.inner.observations()
    }
}

impl fmt::Debug for WorkspaceRuntimeController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkspaceRuntimeController")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    fn inputs() -> WorkspaceRuntimeInputs {
        WorkspaceRuntimeInputs::new(
            LogicalWorkspaceId::new(1),
            WorkspaceFolderSetGeneration::new(1),
            WorkspaceConfigurationGeneration::new(1),
            WorkspaceTrustGeneration::new(1),
            WorkspaceEnvironmentIdentity::new(1),
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
            inputs(),
        )?;
        controller.complete_transition(
            context.generation(),
            WorkspaceRuntimeLifecycleState::ActiveCurrent,
        )?;
        Ok(context)
    }

    #[test]
    fn terminal_generation_is_not_current() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(40));
        let root_id = WorkspaceRootId::new(1);
        let current = active_root(&controller, root_id)?;
        assert!(
            controller.is_current(current.generation()),
            "active generation must compare current"
        );

        controller.detach_root(
            current.generation(),
            WorkspaceRuntimeTerminalReason::Removed,
        )?;
        assert!(
            !controller.is_current(current.generation()),
            "detached generation must not compare current"
        );
        Ok(())
    }

    #[test]
    fn shutdown_and_restart_identities_do_not_compare_current() -> Result<()> {
        let first = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(41));
        let root_id = WorkspaceRootId::new(2);
        let old = active_root(&first, root_id)?;
        first.shutdown();
        assert!(
            !first.is_current(old.generation()),
            "shutdown generation must not compare current"
        );

        let restarted = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(42));
        let fresh = active_root(&restarted, root_id)?;
        assert_ne!(
            old.generation(),
            fresh.generation(),
            "restart must create a distinct session-bound generation"
        );
        assert!(
            !restarted.is_current(old.generation()),
            "old-session generation must not compare current after restart"
        );
        assert!(
            restarted.is_current(fresh.generation()),
            "fresh restart generation must compare current"
        );
        Ok(())
    }

    #[test]
    fn shutdown_closes_every_admission_path() -> Result<()> {
        let controller = WorkspaceRuntimeController::new(WorkspaceRuntimeSessionId::new(43));
        let root_id = WorkspaceRootId::new(3);
        let current = active_root(&controller, root_id)?;
        controller.shutdown();

        assert!(
            matches!(
                controller.begin_transition(
                    root_id,
                    WorkspaceRuntimeTransitionReason::Restart,
                    inputs(),
                ),
                Err(WorkspaceRuntimeError::ControllerShutdown)
            ),
            "shutdown must reject new root transitions"
        );
        assert!(
            matches!(
                controller.register_root_task(
                    current.generation(),
                    WorkspaceRuntimeOperationId::new(1),
                ),
                Err(WorkspaceRuntimeError::ControllerShutdown)
            ),
            "shutdown must reject new root tasks"
        );
        assert!(
            matches!(
                controller.accept_publication(
                    current.generation(),
                    WorkspaceRuntimePublicationKind::WorkspaceFacts,
                ),
                Err(WorkspaceRuntimeError::ControllerShutdown)
            ),
            "shutdown must reject root publication"
        );
        Ok(())
    }
}
