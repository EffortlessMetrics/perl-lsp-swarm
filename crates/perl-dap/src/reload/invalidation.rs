//! Per-object-kind invalidation dispositions for terminal mutation
//! outcomes.
//!
//! The table answers "which old DAP objects become stale after `reloaded`
//! or `indeterminate_possibly_applied`" with a closed disposition for every
//! enumerated object kind, composed with the suspension authority
//! (`stopped_generation`). Thread references are adapter-synthetic
//! projections, not runtime facts (`features_sot.toml` `dap.threads`: at
//! most one synthetic execution context for the active session), and
//! durable client breakpoint configuration is preserved for the later
//! reconciliation owned by #10102.

use super::generation::RuntimeModuleGeneration;
use super::transaction::LoadedModuleReloadOutcome;
use std::collections::BTreeMap;

/// Every DAP object kind the contract enumerates for invalidation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DapObjectKind {
    /// Stack-frame references.
    FrameReference,
    /// Scope references.
    ScopeReference,
    /// Variable references.
    VariableReference,
    /// Evaluate-result references.
    EvaluateReference,
    /// `modules` module ids (positional per query today).
    ModuleId,
    /// `loadedSources`/`%INC` observations.
    LoadedSourceObservation,
    /// `source` content reads.
    SourceContentRead,
    /// Applied engine breakpoint installations for the affected source.
    AppliedBreakpointInstallation,
    /// Exception and current-stop facts where affected.
    ExceptionAndCurrentStopFacts,
    /// Retained runtime query results that could observe old code.
    RetainedRuntimeQueryResult,
    /// Thread references — adapter-synthetic session projections.
    ThreadReference,
    /// Durable desired client breakpoint configuration (distinct from
    /// applied installations).
    DurableClientBreakpointConfiguration,
}

impl DapObjectKind {
    /// All enumerated kinds in frozen closed order.
    pub const ALL: [DapObjectKind; 12] = [
        DapObjectKind::FrameReference,
        DapObjectKind::ScopeReference,
        DapObjectKind::VariableReference,
        DapObjectKind::EvaluateReference,
        DapObjectKind::ModuleId,
        DapObjectKind::LoadedSourceObservation,
        DapObjectKind::SourceContentRead,
        DapObjectKind::AppliedBreakpointInstallation,
        DapObjectKind::ExceptionAndCurrentStopFacts,
        DapObjectKind::RetainedRuntimeQueryResult,
        DapObjectKind::ThreadReference,
        DapObjectKind::DurableClientBreakpointConfiguration,
    ];

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub const fn as_str(self) -> &'static str {
        match self {
            DapObjectKind::FrameReference => "frame_reference",
            DapObjectKind::ScopeReference => "scope_reference",
            DapObjectKind::VariableReference => "variable_reference",
            DapObjectKind::EvaluateReference => "evaluate_reference",
            DapObjectKind::ModuleId => "module_id",
            DapObjectKind::LoadedSourceObservation => "loaded_source_observation",
            DapObjectKind::SourceContentRead => "source_content_read",
            DapObjectKind::AppliedBreakpointInstallation => "applied_breakpoint_installation",
            DapObjectKind::ExceptionAndCurrentStopFacts => "exception_and_current_stop_facts",
            DapObjectKind::RetainedRuntimeQueryResult => "retained_runtime_query_result",
            DapObjectKind::ThreadReference => "thread_reference",
            DapObjectKind::DurableClientBreakpointConfiguration => {
                "durable_client_breakpoint_configuration"
            }
        }
    }

    /// Parse the closed vocabulary; unknown spellings are refused.
    pub fn parse(code: &str) -> Option<DapObjectKind> {
        DapObjectKind::ALL.into_iter().find(|kind| kind.as_str() == code)
    }
}

/// The invalidation disposition of one object kind under a terminal
/// mutation outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvalidationDisposition {
    /// Stale unconditionally: the object has no cross-outcome identity and
    /// must be re-observed (positional module ids, `%INC` snapshots,
    /// source reads, applied installations, retained query results).
    AlwaysStale,
    /// Stale when either authority moved past the reference's bind point:
    /// the runtime-module generation advanced past the reference's module
    /// generation, or the suspension generation advanced past the
    /// reference's suspension generation. The two authorities are
    /// independent and compose by OR.
    StaleWhenGenerationAdvanced,
    /// Adapter projection: re-projected from session state, never treated
    /// as runtime fact a reload could invalidate (thread references).
    ProjectionReprojected,
    /// Preserved untouched; reconciled later by #10102 (durable desired
    /// client breakpoint configuration).
    PreservedForLaterReconciliation,
}

impl InvalidationDisposition {
    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub const fn as_str(self) -> &'static str {
        match self {
            InvalidationDisposition::AlwaysStale => "always_stale",
            InvalidationDisposition::StaleWhenGenerationAdvanced => {
                "stale_when_generation_advanced"
            }
            InvalidationDisposition::ProjectionReprojected => "projection_reprojected",
            InvalidationDisposition::PreservedForLaterReconciliation => {
                "preserved_for_later_reconciliation"
            }
        }
    }
}

/// The frozen invalidation table for one terminal mutation outcome.
///
/// Build it with [`invalidation_plan_for`]; the explicit constructor
/// exists so verification can challenge a claimed (possibly wrong) table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadInvalidationPlan {
    dispositions: BTreeMap<DapObjectKind, InvalidationDisposition>,
}

impl ReloadInvalidationPlan {
    /// Construct a plan from explicit dispositions (used by verification
    /// and by tests to express wrong candidates).
    pub fn from_dispositions(
        dispositions: &[(DapObjectKind, InvalidationDisposition)],
    ) -> ReloadInvalidationPlan {
        ReloadInvalidationPlan { dispositions: dispositions.iter().copied().collect() }
    }

    /// The disposition for a kind, if the table assigns one.
    pub fn disposition_for(&self, kind: DapObjectKind) -> Option<InvalidationDisposition> {
        self.dispositions.get(&kind).copied()
    }

    /// Whether every enumerated kind has a disposition.
    pub fn covers_every_kind(&self) -> bool {
        DapObjectKind::ALL.into_iter().all(|kind| self.dispositions.contains_key(&kind))
    }

    /// Number of assigned dispositions.
    pub fn len(&self) -> usize {
        self.dispositions.len()
    }

    /// Whether the table is empty (a no-op plan).
    pub fn is_empty(&self) -> bool {
        self.dispositions.is_empty()
    }
}

/// The frozen per-object-kind table for a terminal mutation outcome.
///
/// For `reloaded` and `indeterminate_possibly_applied` the table assigns a
/// disposition to **every** enumerated kind. For refusals and pre-mutation
/// failures the plan is empty: no runtime state changed, so nothing
/// becomes stale.
pub fn invalidation_plan_for(outcome: &LoadedModuleReloadOutcome) -> ReloadInvalidationPlan {
    let mutating = matches!(
        outcome,
        LoadedModuleReloadOutcome::Reloaded
            | LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. }
    );
    if !mutating {
        return ReloadInvalidationPlan::from_dispositions(&[]);
    }
    let entries: Vec<(DapObjectKind, InvalidationDisposition)> = DapObjectKind::ALL
        .into_iter()
        .map(|kind| {
            let disposition = match kind {
                DapObjectKind::FrameReference
                | DapObjectKind::ScopeReference
                | DapObjectKind::VariableReference
                | DapObjectKind::EvaluateReference
                | DapObjectKind::ExceptionAndCurrentStopFacts => {
                    InvalidationDisposition::StaleWhenGenerationAdvanced
                }
                DapObjectKind::ThreadReference => InvalidationDisposition::ProjectionReprojected,
                DapObjectKind::DurableClientBreakpointConfiguration => {
                    InvalidationDisposition::PreservedForLaterReconciliation
                }
                DapObjectKind::ModuleId
                | DapObjectKind::LoadedSourceObservation
                | DapObjectKind::SourceContentRead
                | DapObjectKind::AppliedBreakpointInstallation
                | DapObjectKind::RetainedRuntimeQueryResult => InvalidationDisposition::AlwaysStale,
            };
            (kind, disposition)
        })
        .collect();
    ReloadInvalidationPlan::from_dispositions(&entries)
}

/// A DAP inspection reference's bind point against both authorities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DapReferenceBinding {
    /// Runtime-module generation when the reference was minted.
    pub runtime_module_generation: RuntimeModuleGeneration,
    /// Suspension generation (`stopped_generation`) when the reference was
    /// minted.
    pub stopped_generation: u64,
}

/// Whether a reference is stale under composition of the two authorities.
///
/// A reference is stale when **either** the runtime-module generation
/// advanced past its bind point (old code observation) **or** the
/// suspension generation advanced past it (the existing suspension
/// authority). Both-at-current is current; fail closed on any older bind
/// point.
pub fn reference_is_stale(
    binding: &DapReferenceBinding,
    current_runtime_module: RuntimeModuleGeneration,
    current_stopped: u64,
) -> bool {
    binding.runtime_module_generation < current_runtime_module
        || binding.stopped_generation < current_stopped
}

/// Why a claimed invalidation table violates the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidationPlanError {
    /// A mutating outcome must invalidate frame/scope/variable/evaluate
    /// references and applied installations; a table that keeps them
    /// current after a possibly-applied outcome is exactly the forbidden
    /// "old identities survive" shape.
    StaleIdentitySurvivesPossiblyApplied,
    /// Durable client breakpoint configuration must be preserved, not
    /// invalidated; reconciliation is #10102's.
    DurableConfigurationInvalidated,
    /// Thread references are adapter projections and must be marked for
    /// re-projection, not treated as invalidatable runtime facts.
    ThreadReferenceNotProjection,
}

impl InvalidationPlanError {
    /// All invalidation-plan errors in closed order.
    pub const ALL: [InvalidationPlanError; 3] = [
        InvalidationPlanError::StaleIdentitySurvivesPossiblyApplied,
        InvalidationPlanError::DurableConfigurationInvalidated,
        InvalidationPlanError::ThreadReferenceNotProjection,
    ];

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub const fn code(self) -> &'static str {
        match self {
            InvalidationPlanError::StaleIdentitySurvivesPossiblyApplied => {
                "stale_identity_survives_possibly_applied"
            }
            InvalidationPlanError::DurableConfigurationInvalidated => {
                "durable_configuration_invalidated"
            }
            InvalidationPlanError::ThreadReferenceNotProjection => {
                "thread_reference_not_projection"
            }
        }
    }
}

/// Verify a claimed invalidation table against the frozen semantics for
/// one outcome.
///
/// For a mutating outcome the table must equal the frozen per-kind table
/// ([`invalidation_plan_for`]) for **every** enumerated kind — a plan that
/// preserves positional module ids, loaded-source observations, source
/// content reads, applied installations, exception/stop facts, or retained
/// query results after a terminal mutation outcome is exactly the
/// forbidden old-state-survives shape. Mismatches are reported with the
/// most specific code: durable configuration →
/// [`InvalidationPlanError::DurableConfigurationInvalidated`], thread
/// references → [`InvalidationPlanError::ThreadReferenceNotProjection`],
/// and every other kind (including missing coverage) →
/// [`InvalidationPlanError::StaleIdentitySurvivesPossiblyApplied`]. For a
/// non-mutating outcome the only valid table is empty.
pub fn verify_invalidation_plan(
    plan: &ReloadInvalidationPlan,
    outcome: &LoadedModuleReloadOutcome,
) -> Result<(), InvalidationPlanError> {
    let mutating = matches!(
        outcome,
        LoadedModuleReloadOutcome::Reloaded
            | LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. }
    );
    if !mutating {
        if plan.is_empty() {
            return Ok(());
        }
        return Err(InvalidationPlanError::StaleIdentitySurvivesPossiblyApplied);
    }
    let frozen = invalidation_plan_for(outcome);
    for kind in DapObjectKind::ALL {
        if plan.disposition_for(kind) != frozen.disposition_for(kind) {
            return Err(match kind {
                DapObjectKind::DurableClientBreakpointConfiguration => {
                    InvalidationPlanError::DurableConfigurationInvalidated
                }
                DapObjectKind::ThreadReference => {
                    InvalidationPlanError::ThreadReferenceNotProjection
                }
                _ => InvalidationPlanError::StaleIdentitySurvivesPossiblyApplied,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reload::LoadedModuleReloadEligibility;
    use crate::reload::transaction::{
        IndeterminateCause, PreMutationFailureCause, ReloadTransactionPhase,
    };

    fn reloaded() -> LoadedModuleReloadOutcome {
        LoadedModuleReloadOutcome::Reloaded
    }

    fn indeterminate() -> LoadedModuleReloadOutcome {
        LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
            phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
            cause: IndeterminateCause::AmbiguousAcknowledgement,
        }
    }

    fn refused() -> LoadedModuleReloadOutcome {
        LoadedModuleReloadOutcome::Refused {
            disposition: LoadedModuleReloadEligibility::OutsideLaunchAuthority,
        }
    }

    fn failed() -> LoadedModuleReloadOutcome {
        LoadedModuleReloadOutcome::FailedBeforeMutation {
            phase: ReloadTransactionPhase::Prepare,
            cause: PreMutationFailureCause::CancelledBeforeMutationBegan,
        }
    }

    #[test]
    fn both_terminal_mutation_outcomes_assign_a_disposition_to_every_kind() {
        for outcome in [reloaded(), indeterminate()] {
            let plan = invalidation_plan_for(&outcome);
            assert!(plan.covers_every_kind());
            assert_eq!(plan.len(), DapObjectKind::ALL.len());
            assert!(verify_invalidation_plan(&plan, &outcome).is_ok());
        }
    }

    #[test]
    fn non_mutating_outcomes_invalidate_nothing() {
        for outcome in [refused(), failed()] {
            let plan = invalidation_plan_for(&outcome);
            assert!(plan.is_empty());
            assert!(verify_invalidation_plan(&plan, &outcome).is_ok());
        }
    }

    #[test]
    fn inspection_references_compose_both_generation_authorities() {
        let binding = DapReferenceBinding {
            runtime_module_generation: RuntimeModuleGeneration::new(3),
            stopped_generation: 7,
        };
        // Both current: current.
        assert!(!reference_is_stale(&binding, RuntimeModuleGeneration::new(3), 7));
        // Module generation advanced past the bind point: stale.
        assert!(reference_is_stale(&binding, RuntimeModuleGeneration::new(4), 7));
        // Suspension generation advanced past the bind point: stale.
        assert!(reference_is_stale(&binding, RuntimeModuleGeneration::new(3), 8));
        // Either authority alone invalidates (composition is OR).
        assert!(reference_is_stale(
            &DapReferenceBinding {
                runtime_module_generation: RuntimeModuleGeneration::new(1),
                stopped_generation: 99
            },
            RuntimeModuleGeneration::new(2),
            7
        ));
    }

    #[test]
    fn thread_references_are_projections_and_durable_configuration_is_preserved() {
        for outcome in [reloaded(), indeterminate()] {
            let plan = invalidation_plan_for(&outcome);
            assert_eq!(
                plan.disposition_for(DapObjectKind::ThreadReference),
                Some(InvalidationDisposition::ProjectionReprojected)
            );
            assert_eq!(
                plan.disposition_for(DapObjectKind::DurableClientBreakpointConfiguration),
                Some(InvalidationDisposition::PreservedForLaterReconciliation)
            );
            for kind in [
                DapObjectKind::FrameReference,
                DapObjectKind::ScopeReference,
                DapObjectKind::VariableReference,
                DapObjectKind::EvaluateReference,
                DapObjectKind::ExceptionAndCurrentStopFacts,
            ] {
                assert_eq!(
                    plan.disposition_for(kind),
                    Some(InvalidationDisposition::StaleWhenGenerationAdvanced)
                );
            }
            for kind in [
                DapObjectKind::ModuleId,
                DapObjectKind::LoadedSourceObservation,
                DapObjectKind::SourceContentRead,
                DapObjectKind::AppliedBreakpointInstallation,
                DapObjectKind::RetainedRuntimeQueryResult,
            ] {
                assert_eq!(plan.disposition_for(kind), Some(InvalidationDisposition::AlwaysStale));
            }
        }
    }

    #[test]
    fn old_identities_cannot_survive_a_possibly_applied_outcome() {
        let honest = invalidation_plan_for(&indeterminate());
        // Wrong candidate: frame references kept valid under indeterminate.
        let wrong = ReloadInvalidationPlan::from_dispositions(
            &DapObjectKind::ALL
                .into_iter()
                .map(|kind| {
                    let disposition = if kind == DapObjectKind::FrameReference {
                        InvalidationDisposition::PreservedForLaterReconciliation
                    } else {
                        honest.disposition_for(kind).unwrap_or(InvalidationDisposition::AlwaysStale)
                    };
                    (kind, disposition)
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            verify_invalidation_plan(&wrong, &indeterminate()),
            Err(InvalidationPlanError::StaleIdentitySurvivesPossiblyApplied)
        );

        // Wrong candidate: durable client configuration invalidated.
        let honest_reloaded = invalidation_plan_for(&reloaded());
        let wrong_durable = ReloadInvalidationPlan::from_dispositions(
            &DapObjectKind::ALL
                .into_iter()
                .map(|kind| {
                    let disposition = if kind == DapObjectKind::DurableClientBreakpointConfiguration
                    {
                        InvalidationDisposition::AlwaysStale
                    } else {
                        honest_reloaded
                            .disposition_for(kind)
                            .unwrap_or(InvalidationDisposition::AlwaysStale)
                    };
                    (kind, disposition)
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            verify_invalidation_plan(&wrong_durable, &reloaded()),
            Err(InvalidationPlanError::DurableConfigurationInvalidated)
        );

        // Wrong candidate: thread references treated as invalidatable
        // runtime facts.
        let wrong_threads = ReloadInvalidationPlan::from_dispositions(
            &DapObjectKind::ALL
                .into_iter()
                .map(|kind| {
                    let disposition = if kind == DapObjectKind::ThreadReference {
                        InvalidationDisposition::AlwaysStale
                    } else {
                        honest.disposition_for(kind).unwrap_or(InvalidationDisposition::AlwaysStale)
                    };
                    (kind, disposition)
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            verify_invalidation_plan(&wrong_threads, &indeterminate()),
            Err(InvalidationPlanError::ThreadReferenceNotProjection)
        );
    }

    #[test]
    fn preserved_module_observation_or_retained_result_is_rejected() {
        // Wrong candidate: positional module ids kept valid under
        // indeterminate — every kind must match the frozen table exactly.
        let honest = invalidation_plan_for(&indeterminate());
        let wrong = ReloadInvalidationPlan::from_dispositions(
            &DapObjectKind::ALL
                .into_iter()
                .map(|kind| {
                    let disposition = if kind == DapObjectKind::ModuleId {
                        InvalidationDisposition::PreservedForLaterReconciliation
                    } else {
                        honest.disposition_for(kind).unwrap_or(InvalidationDisposition::AlwaysStale)
                    };
                    (kind, disposition)
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            verify_invalidation_plan(&wrong, &indeterminate()),
            Err(InvalidationPlanError::StaleIdentitySurvivesPossiblyApplied)
        );
        // A diverging-but-stronger disposition is still drift from the
        // frozen table and must be reported.
        let strengthened = ReloadInvalidationPlan::from_dispositions(
            &DapObjectKind::ALL
                .into_iter()
                .map(|kind| {
                    let disposition = if kind == DapObjectKind::ExceptionAndCurrentStopFacts {
                        InvalidationDisposition::AlwaysStale
                    } else {
                        honest.disposition_for(kind).unwrap_or(InvalidationDisposition::AlwaysStale)
                    };
                    (kind, disposition)
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            verify_invalidation_plan(&strengthened, &indeterminate()),
            Err(InvalidationPlanError::StaleIdentitySurvivesPossiblyApplied)
        );
    }

    #[test]
    fn a_non_mutating_outcome_rejects_any_claimed_invalidation() {
        let claimed = invalidation_plan_for(&reloaded());
        assert_eq!(
            verify_invalidation_plan(&claimed, &refused()),
            Err(InvalidationPlanError::StaleIdentitySurvivesPossiblyApplied)
        );
    }

    #[test]
    fn object_kind_vocabulary_is_closed() {
        assert_eq!(DapObjectKind::ALL.len(), 12);
        for kind in DapObjectKind::ALL {
            assert_eq!(DapObjectKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(DapObjectKind::parse("breakpoint"), None);
    }
}
