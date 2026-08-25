//! Transaction phases, terminal outcomes, and the possibly-applied boundary.

use super::eligibility::{
    LoadedModuleReloadEligibility, ReloadAdmissionObservation, classify_reload_eligibility,
};
use super::generation::GenerationEffect;
use super::subject::{LoadedModuleSubject, ModuleClassification};

/// One bounded reload transaction's phases in frozen order.
///
/// `runtime_mutation_begins` marks the possibly-applied boundary: a failure
/// before it changes no runtime generation; a timeout, transport loss, or
/// ambiguous response at or after it is
/// [`LoadedModuleReloadOutcome::IndeterminatePossiblyApplied`] and advances
/// the runtime-module generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ReloadTransactionPhase {
    /// Eligibility and identity admission.
    Admission,
    /// Readiness, authority, and saved-source preflight.
    Preflight,
    /// Prepare the bounded transaction (no runtime mutation yet).
    Prepare,
    /// The boundary: runtime mutation begins. From here on, unknown
    /// outcomes are possibly applied.
    RuntimeMutationBegins,
    /// Runtime acknowledgement / read-back of the mutation.
    RuntimeAcknowledgementReadBack,
    /// Commit the runtime-module generation.
    CommitGeneration,
    /// Post-reload reconciliation of invalidated state.
    PostReloadReconciliation,
    /// Terminal projection to the client.
    TerminalProjection,
}

impl ReloadTransactionPhase {
    /// All eight phases in frozen transaction order.
    pub const ALL: [ReloadTransactionPhase; 8] = [
        ReloadTransactionPhase::Admission,
        ReloadTransactionPhase::Preflight,
        ReloadTransactionPhase::Prepare,
        ReloadTransactionPhase::RuntimeMutationBegins,
        ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
        ReloadTransactionPhase::CommitGeneration,
        ReloadTransactionPhase::PostReloadReconciliation,
        ReloadTransactionPhase::TerminalProjection,
    ];

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub const fn as_str(self) -> &'static str {
        match self {
            ReloadTransactionPhase::Admission => "admission",
            ReloadTransactionPhase::Preflight => "preflight",
            ReloadTransactionPhase::Prepare => "prepare",
            ReloadTransactionPhase::RuntimeMutationBegins => "runtime_mutation_begins",
            ReloadTransactionPhase::RuntimeAcknowledgementReadBack => {
                "runtime_acknowledgement_read_back"
            }
            ReloadTransactionPhase::CommitGeneration => "commit_generation",
            ReloadTransactionPhase::PostReloadReconciliation => "post_reload_reconciliation",
            ReloadTransactionPhase::TerminalProjection => "terminal_projection",
        }
    }

    /// Parse the closed vocabulary; unknown spellings are refused.
    pub fn parse(code: &str) -> Option<ReloadTransactionPhase> {
        ReloadTransactionPhase::ALL.into_iter().find(|phase| phase.as_str() == code)
    }

    /// Whether this phase is at or past the possibly-applied boundary.
    pub fn is_mutation_begun(self) -> bool {
        self >= ReloadTransactionPhase::RuntimeMutationBegins
    }
}

/// Why a transaction failed before runtime mutation began.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreMutationFailureCause {
    /// Prepare failed deterministically before any mutation was issued.
    PrepareFailed,
    /// The operation was cancelled before the boundary.
    CancelledBeforeMutationBegan,
}

impl PreMutationFailureCause {
    /// All pre-mutation failure causes in closed order.
    pub const ALL: [PreMutationFailureCause; 2] = [
        PreMutationFailureCause::PrepareFailed,
        PreMutationFailureCause::CancelledBeforeMutationBegan,
    ];

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub const fn as_str(self) -> &'static str {
        match self {
            PreMutationFailureCause::PrepareFailed => "prepare_failed",
            PreMutationFailureCause::CancelledBeforeMutationBegan => {
                "cancelled_before_mutation_began"
            }
        }
    }
}

/// Why a post-boundary outcome is indeterminate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndeterminateCause {
    /// The framed acknowledgement timed out after mutation began.
    TimeoutAfterMutationBegan,
    /// The transport was lost after mutation began.
    TransportLossAfterMutationBegan,
    /// The acknowledgement was ambiguous (for example a prompt observed
    /// where an engine acknowledgement was required).
    AmbiguousAcknowledgement,
    /// The read-back could not establish the post-mutation state.
    ReadBackInconclusive,
}

impl IndeterminateCause {
    /// All indeterminate causes in closed order.
    pub const ALL: [IndeterminateCause; 4] = [
        IndeterminateCause::TimeoutAfterMutationBegan,
        IndeterminateCause::TransportLossAfterMutationBegan,
        IndeterminateCause::AmbiguousAcknowledgement,
        IndeterminateCause::ReadBackInconclusive,
    ];

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub const fn as_str(self) -> &'static str {
        match self {
            IndeterminateCause::TimeoutAfterMutationBegan => "timeout_after_mutation_began",
            IndeterminateCause::TransportLossAfterMutationBegan => {
                "transport_loss_after_mutation_began"
            }
            IndeterminateCause::AmbiguousAcknowledgement => "ambiguous_acknowledgement",
            IndeterminateCause::ReadBackInconclusive => "read_back_inconclusive",
        }
    }
}

/// Terminal outcome of one bounded reload transaction.
///
/// The four terminal states are `reloaded`, `refused`, `failed` (before
/// mutation), and `indeterminate (possibly applied)`. Only
/// [`LoadedModuleReloadOutcome::Reloaded`] is a clean success;
/// [`LoadedModuleReloadOutcome::IndeterminatePossiblyApplied`] must never
/// be projected as clean or empty.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LoadedModuleReloadOutcome {
    /// The runtime accepted and read back the replacement; the
    /// runtime-module generation advanced.
    Reloaded,
    /// Admission or preflight refused the transaction; nothing was
    /// attempted and no generation advanced.
    Refused {
        /// The refusal disposition (always a non-admitted class).
        disposition: LoadedModuleReloadEligibility,
    },
    /// The transaction failed before runtime mutation began; no generation
    /// advanced.
    FailedBeforeMutation {
        /// The phase at which the failure was observed (before the
        /// boundary by construction).
        phase: ReloadTransactionPhase,
        /// Why it failed.
        cause: PreMutationFailureCause,
    },
    /// A timeout, transport loss, or ambiguous response at or after the
    /// mutation boundary; the runtime-module generation advanced and old
    /// exact state is invalid.
    IndeterminatePossiblyApplied {
        /// The phase at which the outcome became indeterminate (at or
        /// after the boundary by construction).
        phase: ReloadTransactionPhase,
        /// Why the outcome is unknown.
        cause: IndeterminateCause,
    },
}

impl LoadedModuleReloadOutcome {
    /// Stable closed-vocabulary kind code used by the `.spec` fixtures.
    pub fn kind_code(&self) -> &'static str {
        match self {
            LoadedModuleReloadOutcome::Reloaded => "reloaded",
            LoadedModuleReloadOutcome::Refused { .. } => "refused",
            LoadedModuleReloadOutcome::FailedBeforeMutation { .. } => "failed_before_mutation",
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. } => {
                "indeterminate_possibly_applied"
            }
        }
    }

    /// Generation effect required by the frozen semantics: `reloaded` and
    /// `indeterminate_possibly_applied` advance the runtime-module
    /// generation; refusals and pre-mutation failures advance nothing.
    ///
    /// Fail-closed for malformed outcomes: a `FailedBeforeMutation`
    /// carrying a phase at or after the mutation boundary violates the
    /// contract's phase/kind pairing (`phase_permits_outcome`) — the
    /// boundary was crossed, so the runtime may have mutated and the
    /// generation **must** advance rather than leaving old references
    /// current.
    pub fn generation_effect(&self) -> GenerationEffect {
        match self {
            LoadedModuleReloadOutcome::Reloaded
            | LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. } => {
                GenerationEffect::Advance
            }
            LoadedModuleReloadOutcome::Refused { .. } => GenerationEffect::None,
            LoadedModuleReloadOutcome::FailedBeforeMutation { phase, .. } => {
                if phase.is_mutation_begun() {
                    GenerationEffect::Advance
                } else {
                    GenerationEffect::None
                }
            }
        }
    }

    /// Whether the outcome may be projected to the client as a clean
    /// (success/empty) terminal state. Indeterminate outcomes never are:
    /// mapping an unknown post-mutation result to empty is exactly the
    /// forbidden indeterminate-as-clean mapping (see
    /// `debug_adapter/output.rs` `query_inc_entries`, which today maps a
    /// framed-query timeout to an empty list for read-only queries — a
    /// mutation transaction must not).
    pub fn projects_as_clean(&self) -> bool {
        !matches!(self, LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. })
    }

    /// Assemble the contract-mandated outcome for a query whose transport
    /// result became unknown at or after the mutation boundary.
    ///
    /// This function exists so no consumer can hand-roll an empty/clean
    /// projection for a post-boundary unknown: the only contract-valid
    /// answer is indeterminate-possibly-applied with a generation advance.
    pub fn unknown_after_mutation(
        phase: ReloadTransactionPhase,
        cause: IndeterminateCause,
    ) -> Result<LoadedModuleReloadOutcome, &'static str> {
        if !phase.is_mutation_begun() {
            return Err(
                "unknown outcome before the mutation boundary is a pre-mutation failure, not indeterminate",
            );
        }
        Ok(LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { phase, cause })
    }
}

/// Project a runtime observation whose framed transport result timed out
/// after the mutation boundary.
///
/// The frozen law: this is `indeterminate_possibly_applied`, never a clean
/// or empty answer, and it advances the runtime-module generation.
pub fn project_unknown_after_mutation(
    phase: ReloadTransactionPhase,
    cause: IndeterminateCause,
) -> LoadedModuleReloadOutcome {
    match LoadedModuleReloadOutcome::unknown_after_mutation(phase, cause) {
        Ok(outcome) => outcome,
        // The phase is contract-invalid for this projection; fail closed to
        // the safest terminal state rather than a clean one.
        Err(_) => LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
            phase: ReloadTransactionPhase::RuntimeMutationBegins,
            cause,
        },
    }
}

/// Whether the phase/outcome pair is contract-valid.
///
/// - `refused` is valid only in `admission` or `preflight`;
/// - `failed_before_mutation` is valid only before the boundary;
/// - `indeterminate_possibly_applied` is valid only at or after the
///   boundary;
/// - `reloaded` is valid only at `commit_generation` or later.
pub fn phase_permits_outcome(
    phase: ReloadTransactionPhase,
    outcome: &LoadedModuleReloadOutcome,
) -> bool {
    match outcome {
        LoadedModuleReloadOutcome::Reloaded => phase >= ReloadTransactionPhase::CommitGeneration,
        LoadedModuleReloadOutcome::Refused { .. } => {
            phase == ReloadTransactionPhase::Admission || phase == ReloadTransactionPhase::Preflight
        }
        LoadedModuleReloadOutcome::FailedBeforeMutation { phase: at, .. } => {
            *at == phase && !phase.is_mutation_begun()
        }
        LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { phase: at, .. } => {
            *at == phase && phase.is_mutation_begun()
        }
    }
}

/// A validated admission artifact for one bounded reload transaction.
///
/// Produced only by [`plan_reload`]; carries the exact subject and the
/// generation context at which admission was earned. Executing it is
/// #10098's job; this type records what was admitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedModuleReloadPlan {
    subject: LoadedModuleSubject,
    admitted_session_generation: u64,
    admitted_suspension_generation: u64,
}

impl LoadedModuleReloadPlan {
    /// The exact admitted subject.
    pub fn subject(&self) -> &LoadedModuleSubject {
        &self.subject
    }

    /// Process/session generation at which admission was earned.
    pub fn admitted_session_generation(&self) -> u64 {
        self.admitted_session_generation
    }

    /// Suspension generation at which admission was earned.
    pub fn admitted_suspension_generation(&self) -> u64 {
        self.admitted_suspension_generation
    }

    /// The frozen transaction phase order this plan executes through.
    pub fn transaction_phases() -> [ReloadTransactionPhase; 8] {
        ReloadTransactionPhase::ALL
    }
}

/// Admit or refuse one proposed reload transaction.
///
/// Refusal is total: any non-admitted classification — including an active
/// frame in the target, any unsupported module class, dirty client source,
/// or an inexact/stale identity — returns the refusal disposition and no
/// plan. A bound subject whose classification is not the admitted class is
/// refused by its own classification regardless of the observation's
/// claim, so a stale subject cannot smuggle a wider cohort through a
/// favorable observation.
pub fn plan_reload(
    subject: &LoadedModuleSubject,
    observation: &ReloadAdmissionObservation,
) -> Result<LoadedModuleReloadPlan, LoadedModuleReloadEligibility> {
    let subject_refusal = match subject.module_classification() {
        ModuleClassification::SourceBackedPerlModule => None,
        ModuleClassification::MainProgram => {
            Some(LoadedModuleReloadEligibility::MainProgramNotModule)
        }
        ModuleClassification::XsOrNative => Some(LoadedModuleReloadEligibility::XsOrNativeModule),
        ModuleClassification::SourceFilterOrCompileHook => {
            Some(LoadedModuleReloadEligibility::SourceFilterOrCompileHookBoundary)
        }
        ModuleClassification::GeneratedOrEval => {
            Some(LoadedModuleReloadEligibility::GeneratedOrEvalSource)
        }
    };
    if let Some(refusal) = subject_refusal {
        return Err(refusal);
    }
    match classify_reload_eligibility(observation) {
        LoadedModuleReloadEligibility::EligibleSourceBackedPerlModule => {
            Ok(LoadedModuleReloadPlan {
                subject: subject.clone(),
                admitted_session_generation: subject.session_generation(),
                admitted_suspension_generation: subject.suspension_generation(),
            })
        }
        refusal => Err(refusal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reload::RuntimeModuleGenerationClock;

    #[test]
    fn phase_vocabulary_is_closed_and_ordered() {
        assert_eq!(ReloadTransactionPhase::ALL.len(), 8);
        for (index, phase) in ReloadTransactionPhase::ALL.into_iter().enumerate() {
            assert_eq!(ReloadTransactionPhase::parse(phase.as_str()), Some(phase));
            assert_eq!(phase as usize, index);
        }
        assert_eq!(ReloadTransactionPhase::parse("mutation_begins"), None);
        let boundary = ReloadTransactionPhase::ALL
            .into_iter()
            .filter(|phase| phase.is_mutation_begun())
            .count();
        assert_eq!(boundary, 5, "exactly the five phases from the boundary onward");
        for phase in ReloadTransactionPhase::ALL {
            assert_eq!(phase.is_mutation_begun(), phase as usize >= 3);
        }
    }

    #[test]
    fn only_reloaded_and_indeterminate_advance_the_generation() {
        let advancing = vec![
            LoadedModuleReloadOutcome::Reloaded,
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
                cause: IndeterminateCause::TimeoutAfterMutationBegan,
            },
        ];
        for outcome in advancing {
            assert_eq!(outcome.generation_effect(), GenerationEffect::Advance);
        }
        let inert = vec![
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::NotLoaded,
            },
            LoadedModuleReloadOutcome::FailedBeforeMutation {
                phase: ReloadTransactionPhase::Prepare,
                cause: PreMutationFailureCause::PrepareFailed,
            },
        ];
        for outcome in inert {
            assert_eq!(outcome.generation_effect(), GenerationEffect::None);
        }
    }

    #[test]
    fn indeterminate_never_projects_as_clean() {
        let causes = [
            IndeterminateCause::TimeoutAfterMutationBegan,
            IndeterminateCause::TransportLossAfterMutationBegan,
            IndeterminateCause::AmbiguousAcknowledgement,
            IndeterminateCause::ReadBackInconclusive,
        ];
        for cause in causes {
            let outcome = LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeMutationBegins,
                cause,
            };
            assert!(!outcome.projects_as_clean());
            assert_eq!(outcome.generation_effect(), GenerationEffect::Advance);
        }
        assert!(LoadedModuleReloadOutcome::Reloaded.projects_as_clean());
        assert!(
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::NotLoaded
            }
            .projects_as_clean()
        );
    }

    #[test]
    fn unknown_after_mutation_boundary_is_always_indeterminate_advancing() {
        let outcome = project_unknown_after_mutation(
            ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
            IndeterminateCause::TimeoutAfterMutationBegan,
        );
        assert_eq!(outcome.kind_code(), "indeterminate_possibly_applied");
        assert_eq!(outcome.generation_effect(), GenerationEffect::Advance);
        assert!(!outcome.projects_as_clean());
    }

    #[test]
    fn unknown_before_the_boundary_cannot_be_projected_indeterminate() {
        let invalid = LoadedModuleReloadOutcome::unknown_after_mutation(
            ReloadTransactionPhase::Prepare,
            IndeterminateCause::TimeoutAfterMutationBegan,
        );
        assert!(invalid.is_err());
    }

    #[test]
    fn malformed_post_boundary_pre_mutation_failure_advances_fail_closed() {
        // A FailedBeforeMutation carrying a post-boundary phase violates
        // the phase/kind pairing; the boundary was crossed, so the clock
        // must advance rather than leaving old references current.
        let malformed = LoadedModuleReloadOutcome::FailedBeforeMutation {
            phase: ReloadTransactionPhase::RuntimeMutationBegins,
            cause: PreMutationFailureCause::PrepareFailed,
        };
        assert!(!phase_permits_outcome(ReloadTransactionPhase::RuntimeMutationBegins, &malformed));
        assert_eq!(malformed.generation_effect(), GenerationEffect::Advance);
        let mut clock = RuntimeModuleGenerationClock::new();
        assert!(clock.apply(&malformed).advanced());
        // The well-formed pre-boundary shape still advances nothing.
        let well_formed = LoadedModuleReloadOutcome::FailedBeforeMutation {
            phase: ReloadTransactionPhase::Prepare,
            cause: PreMutationFailureCause::PrepareFailed,
        };
        assert_eq!(well_formed.generation_effect(), GenerationEffect::None);
    }

    #[test]
    fn phase_outcome_matrix_holds_the_boundary() {
        let refused = || LoadedModuleReloadOutcome::Refused {
            disposition: LoadedModuleReloadEligibility::NotLoaded,
        };
        let failed = |phase| LoadedModuleReloadOutcome::FailedBeforeMutation {
            phase,
            cause: PreMutationFailureCause::PrepareFailed,
        };
        let indeterminate = |phase| LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
            phase,
            cause: IndeterminateCause::TransportLossAfterMutationBegan,
        };

        assert!(phase_permits_outcome(ReloadTransactionPhase::Admission, &refused()));
        assert!(phase_permits_outcome(ReloadTransactionPhase::Preflight, &refused()));
        assert!(!phase_permits_outcome(ReloadTransactionPhase::Prepare, &refused()));

        assert!(phase_permits_outcome(
            ReloadTransactionPhase::Prepare,
            &failed(ReloadTransactionPhase::Prepare)
        ));
        assert!(!phase_permits_outcome(
            ReloadTransactionPhase::Prepare,
            &failed(ReloadTransactionPhase::RuntimeMutationBegins)
        ));
        assert!(!phase_permits_outcome(
            ReloadTransactionPhase::RuntimeMutationBegins,
            &failed(ReloadTransactionPhase::Prepare)
        ));

        assert!(phase_permits_outcome(
            ReloadTransactionPhase::RuntimeMutationBegins,
            &indeterminate(ReloadTransactionPhase::RuntimeMutationBegins)
        ));
        assert!(phase_permits_outcome(
            ReloadTransactionPhase::TerminalProjection,
            &indeterminate(ReloadTransactionPhase::TerminalProjection)
        ));
        assert!(!phase_permits_outcome(
            ReloadTransactionPhase::Prepare,
            &indeterminate(ReloadTransactionPhase::RuntimeMutationBegins)
        ));

        assert!(!phase_permits_outcome(
            ReloadTransactionPhase::Prepare,
            &LoadedModuleReloadOutcome::Reloaded
        ));
        assert!(phase_permits_outcome(
            ReloadTransactionPhase::CommitGeneration,
            &LoadedModuleReloadOutcome::Reloaded
        ));
        assert!(phase_permits_outcome(
            ReloadTransactionPhase::TerminalProjection,
            &LoadedModuleReloadOutcome::Reloaded
        ));
    }

    #[test]
    fn plan_reload_admits_only_the_earned_cohort() -> Result<(), Box<dyn std::error::Error>> {
        use super::super::subject::{SubjectCandidate, SubjectCurrentnessView};
        let candidate = SubjectCandidate {
            session_generation: Some(4),
            suspension_generation: Some(12),
            observation_generation: Some(3),
            inc_key: "App/Core.pm".to_string(),
            resolved_runtime_path: "/ws/lib/App/Core.pm".to_string(),
            saved_content_digest: "sha256:9f2c".to_string(),
            logical_source_uri: "file:///ws/lib/App/Core.pm".to_string(),
            perl_identity: "perl 5.42.0".to_string(),
            launch_root: "/ws".to_string(),
            module_classification: Some(ModuleClassification::SourceBackedPerlModule),
            operation_identity: 9,
        };
        let subject = candidate.bind().map_err(|_| "candidate must bind")?;
        assert!(subject.is_current_against(&SubjectCurrentnessView {
            session_generation: 4,
            suspension_generation: 12,
            observation_generation: 3,
            saved_content_digest: "sha256:9f2c".to_string(),
            perl_identity: "perl 5.42.0".to_string(),
        }));
        let observation = ReloadAdmissionObservation {
            stopped_and_command_ready: true,
            runtime_supported: true,
            loaded_in_runtime: true,
            within_launch_authority: true,
            runtime_mapping_unambiguous: true,
            identity_binding_complete: true,
            identity_current: true,
            client_source_matches_saved: true,
            module_classification: ModuleClassification::SourceBackedPerlModule,
            active_frame_in_target: false,
        };
        let plan = plan_reload(&subject, &observation).map_err(|_| "earned cohort must admit")?;
        assert_eq!(plan.admitted_session_generation(), 4);
        assert_eq!(plan.admitted_suspension_generation(), 12);
        assert_eq!(plan.subject().inc_key(), "App/Core.pm");
        assert_eq!(LoadedModuleReloadPlan::transaction_phases(), ReloadTransactionPhase::ALL);

        // An active frame refuses even with everything else earned.
        let active_frame =
            ReloadAdmissionObservation { active_frame_in_target: true, ..observation.clone() };
        assert_eq!(
            plan_reload(&subject, &active_frame),
            Err(LoadedModuleReloadEligibility::ActiveFrameInTarget)
        );

        // A subject of a refused class cannot be smuggled through a
        // favorable observation.
        let xs_candidate = SubjectCandidate {
            module_classification: Some(ModuleClassification::XsOrNative),
            ..candidate.clone()
        };
        let xs_subject = xs_candidate.bind().map_err(|_| "xs candidate must bind")?;
        assert_eq!(
            plan_reload(&xs_subject, &observation),
            Err(LoadedModuleReloadEligibility::XsOrNativeModule)
        );
        Ok(())
    }
}
