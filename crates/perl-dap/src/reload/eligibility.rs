//! Closed eligibility vocabulary and admission classification.
//!
//! Exactly thirteen dispositions exist. The initial implementation admits
//! only [`LoadedModuleReloadEligibility::EligibleSourceBackedPerlModule`]
//! with no active target-module frame; every other observation fails
//! closed. New refusal classes may be added only by reopening the
//! contract (#10097); the admitted cohort is never silently widened.

use super::subject::ModuleClassification;

/// Closed disposition vocabulary for a proposed loaded-module reload.
///
/// The snake_case codes are the frozen `.spec` vocabulary
/// (`.spec/10097-loaded-module-reload-contract`); the variants are the
/// executable authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LoadedModuleReloadEligibility {
    /// Ordinary source-backed Perl module, exact identity, saved source,
    /// stopped and command-ready, no active frame in the target. The only
    /// admitted class of the initial cohort.
    EligibleSourceBackedPerlModule,
    /// The proposed subject is not present in the current `%INC`
    /// observation.
    NotLoaded,
    /// The subject identity is not exact or no longer current: incomplete
    /// binding (basename/package-only), stale observation/session/
    /// suspension generation, or digest mismatch with the saved disk
    /// subject (removed or renamed after loading, process/session
    /// replacement).
    SourceNotExactOrStale,
    /// The client-declared source revision does not match the saved disk
    /// source; the adapter's subject is saved disk source only.
    DirtyOrUnsavedSource,
    /// An active frame is executing in the target module. No earned rule
    /// admits this yet.
    ActiveFrameInTarget,
    /// The subject is the debuggee's main program, not a loadable module.
    MainProgramNotModule,
    /// XS or native-linked module.
    XsOrNativeModule,
    /// Source filter or compile-hook boundary module.
    SourceFilterOrCompileHookBoundary,
    /// Generated or eval-produced source.
    GeneratedOrEvalSource,
    /// The runtime mapping cannot bind exactly one subject (for example
    /// the same module/package name under two include roots, or unstable
    /// positional module ids).
    AmbiguousRuntimeMapping,
    /// The resolved subject path lies outside the validated launch root.
    OutsideLaunchAuthority,
    /// The selected runtime does not support the reload mechanism family.
    UnsupportedRuntime,
    /// The debuggee is not stopped or not command-ready.
    NotStoppedOrNotCommandReady,
}

impl LoadedModuleReloadEligibility {
    /// All thirteen dispositions in the frozen closed order.
    pub const ALL: [LoadedModuleReloadEligibility; 13] = [
        LoadedModuleReloadEligibility::EligibleSourceBackedPerlModule,
        LoadedModuleReloadEligibility::NotLoaded,
        LoadedModuleReloadEligibility::SourceNotExactOrStale,
        LoadedModuleReloadEligibility::DirtyOrUnsavedSource,
        LoadedModuleReloadEligibility::ActiveFrameInTarget,
        LoadedModuleReloadEligibility::MainProgramNotModule,
        LoadedModuleReloadEligibility::XsOrNativeModule,
        LoadedModuleReloadEligibility::SourceFilterOrCompileHookBoundary,
        LoadedModuleReloadEligibility::GeneratedOrEvalSource,
        LoadedModuleReloadEligibility::AmbiguousRuntimeMapping,
        LoadedModuleReloadEligibility::OutsideLaunchAuthority,
        LoadedModuleReloadEligibility::UnsupportedRuntime,
        LoadedModuleReloadEligibility::NotStoppedOrNotCommandReady,
    ];

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub const fn as_str(self) -> &'static str {
        match self {
            LoadedModuleReloadEligibility::EligibleSourceBackedPerlModule => {
                "eligible_source_backed_perl_module"
            }
            LoadedModuleReloadEligibility::NotLoaded => "not_loaded",
            LoadedModuleReloadEligibility::SourceNotExactOrStale => "source_not_exact_or_stale",
            LoadedModuleReloadEligibility::DirtyOrUnsavedSource => "dirty_or_unsaved_source",
            LoadedModuleReloadEligibility::ActiveFrameInTarget => "active_frame_in_target",
            LoadedModuleReloadEligibility::MainProgramNotModule => "main_program_not_module",
            LoadedModuleReloadEligibility::XsOrNativeModule => "xs_or_native_module",
            LoadedModuleReloadEligibility::SourceFilterOrCompileHookBoundary => {
                "source_filter_or_compile_hook_boundary"
            }
            LoadedModuleReloadEligibility::GeneratedOrEvalSource => "generated_or_eval_source",
            LoadedModuleReloadEligibility::AmbiguousRuntimeMapping => "ambiguous_runtime_mapping",
            LoadedModuleReloadEligibility::OutsideLaunchAuthority => "outside_launch_authority",
            LoadedModuleReloadEligibility::UnsupportedRuntime => "unsupported_runtime",
            LoadedModuleReloadEligibility::NotStoppedOrNotCommandReady => {
                "not_stopped_or_not_command_ready"
            }
        }
    }

    /// Parse the closed vocabulary; unknown spellings are refused, never
    /// normalized.
    pub fn parse(code: &str) -> Option<LoadedModuleReloadEligibility> {
        LoadedModuleReloadEligibility::ALL
            .into_iter()
            .find(|disposition| disposition.as_str() == code)
    }

    /// Whether this disposition admits the subject into the reload cohort.
    pub fn is_admitted(&self) -> bool {
        matches!(self, LoadedModuleReloadEligibility::EligibleSourceBackedPerlModule)
    }
}

/// Observed admission facts for one proposed reload subject.
///
/// A pure model view: each field is an independently observable fact, and
/// [`classify_reload_eligibility`] applies the frozen precedence. The
/// adapter-side producers of these facts are #10098's runtime transaction
/// and #10102's composition; this contract defines only their meaning.
/// There is deliberately no `Default`: every admission fact must be
/// observed, not defaulted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadAdmissionObservation {
    /// Debuggee is stopped and command-ready right now.
    pub stopped_and_command_ready: bool,
    /// The selected runtime admits the reload mechanism family at all.
    pub runtime_supported: bool,
    /// The proposed subject is present in the current `%INC` observation.
    pub loaded_in_runtime: bool,
    /// The resolved subject path lies within the validated launch root.
    pub within_launch_authority: bool,
    /// The runtime mapping binds exactly one subject (no two-root or
    /// unstable-identity ambiguity).
    pub runtime_mapping_unambiguous: bool,
    /// Every required identity binding is present (basename/package-only
    /// identity makes this false).
    pub identity_binding_complete: bool,
    /// The bound identity is still current (session, suspension, and
    /// observation generations, and the saved digest all match).
    pub identity_current: bool,
    /// The client-declared source revision matches the saved disk source
    /// digest.
    pub client_source_matches_saved: bool,
    /// Classification of the proposed subject.
    pub module_classification: ModuleClassification,
    /// An active frame is executing in the target module.
    pub active_frame_in_target: bool,
}

/// Classify a proposed reload subject into the closed disposition
/// vocabulary.
///
/// Frozen precedence, most fundamental refusal first (ADR-0046):
///
/// 1. `not_stopped_or_not_command_ready` — the transaction precondition;
/// 2. `unsupported_runtime` — the runtime admits no mechanism at all;
/// 3. `not_loaded` — nothing to reload;
/// 4. `outside_launch_authority` — path authority fails;
/// 5. `ambiguous_runtime_mapping` — no exact subject to mutate;
/// 6. classification refusals (`main_program_not_module`,
///    `xs_or_native_module`, `source_filter_or_compile_hook_boundary`,
///    `generated_or_eval_source`);
/// 7. `dirty_or_unsaved_source` — client-declared revision mismatch is
///    independently observable and reported first;
/// 8. `source_not_exact_or_stale` — binding gaps or staleness;
/// 9. `active_frame_in_target` — frame safety, no earned rule yet;
/// 10. otherwise `eligible_source_backed_perl_module`.
///
/// Every non-admitted outcome is terminal and deterministic: the same
/// observation always classifies to the same disposition.
pub fn classify_reload_eligibility(
    observation: &ReloadAdmissionObservation,
) -> LoadedModuleReloadEligibility {
    use LoadedModuleReloadEligibility as Disposition;
    use ModuleClassification as Class;
    if !observation.stopped_and_command_ready {
        return Disposition::NotStoppedOrNotCommandReady;
    }
    if !observation.runtime_supported {
        return Disposition::UnsupportedRuntime;
    }
    if !observation.loaded_in_runtime {
        return Disposition::NotLoaded;
    }
    if !observation.within_launch_authority {
        return Disposition::OutsideLaunchAuthority;
    }
    if !observation.runtime_mapping_unambiguous {
        return Disposition::AmbiguousRuntimeMapping;
    }
    match observation.module_classification {
        Class::SourceBackedPerlModule => {}
        Class::MainProgram => return Disposition::MainProgramNotModule,
        Class::XsOrNative => return Disposition::XsOrNativeModule,
        Class::SourceFilterOrCompileHook => return Disposition::SourceFilterOrCompileHookBoundary,
        Class::GeneratedOrEval => return Disposition::GeneratedOrEvalSource,
    }
    if !observation.client_source_matches_saved {
        return Disposition::DirtyOrUnsavedSource;
    }
    if !(observation.identity_binding_complete && observation.identity_current) {
        return Disposition::SourceNotExactOrStale;
    }
    if observation.active_frame_in_target {
        return Disposition::ActiveFrameInTarget;
    }
    Disposition::EligibleSourceBackedPerlModule
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eligible_observation() -> ReloadAdmissionObservation {
        ReloadAdmissionObservation {
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
        }
    }

    #[test]
    fn vocabulary_is_exactly_the_thirteen_frozen_classes() {
        assert_eq!(LoadedModuleReloadEligibility::ALL.len(), 13);
        let codes: Vec<&str> =
            LoadedModuleReloadEligibility::ALL.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(
            codes,
            vec![
                "eligible_source_backed_perl_module",
                "not_loaded",
                "source_not_exact_or_stale",
                "dirty_or_unsaved_source",
                "active_frame_in_target",
                "main_program_not_module",
                "xs_or_native_module",
                "source_filter_or_compile_hook_boundary",
                "generated_or_eval_source",
                "ambiguous_runtime_mapping",
                "outside_launch_authority",
                "unsupported_runtime",
                "not_stopped_or_not_command_ready",
            ]
        );
        for disposition in LoadedModuleReloadEligibility::ALL {
            assert_eq!(
                LoadedModuleReloadEligibility::parse(disposition.as_str()),
                Some(disposition)
            );
        }
        assert_eq!(LoadedModuleReloadEligibility::parse("eligible_xs_module"), None);
    }

    #[test]
    fn every_refusal_class_is_reachable_and_only_one_class_is_admitted() {
        let cases: Vec<(ReloadAdmissionObservation, LoadedModuleReloadEligibility)> = vec![
            (eligible_observation(), LoadedModuleReloadEligibility::EligibleSourceBackedPerlModule),
            (
                ReloadAdmissionObservation {
                    stopped_and_command_ready: false,
                    ..eligible_observation()
                },
                LoadedModuleReloadEligibility::NotStoppedOrNotCommandReady,
            ),
            (
                ReloadAdmissionObservation { runtime_supported: false, ..eligible_observation() },
                LoadedModuleReloadEligibility::UnsupportedRuntime,
            ),
            (
                ReloadAdmissionObservation { loaded_in_runtime: false, ..eligible_observation() },
                LoadedModuleReloadEligibility::NotLoaded,
            ),
            (
                ReloadAdmissionObservation {
                    within_launch_authority: false,
                    ..eligible_observation()
                },
                LoadedModuleReloadEligibility::OutsideLaunchAuthority,
            ),
            (
                ReloadAdmissionObservation {
                    runtime_mapping_unambiguous: false,
                    ..eligible_observation()
                },
                LoadedModuleReloadEligibility::AmbiguousRuntimeMapping,
            ),
            (
                ReloadAdmissionObservation {
                    module_classification: ModuleClassification::MainProgram,
                    ..eligible_observation()
                },
                LoadedModuleReloadEligibility::MainProgramNotModule,
            ),
            (
                ReloadAdmissionObservation {
                    module_classification: ModuleClassification::XsOrNative,
                    ..eligible_observation()
                },
                LoadedModuleReloadEligibility::XsOrNativeModule,
            ),
            (
                ReloadAdmissionObservation {
                    module_classification: ModuleClassification::SourceFilterOrCompileHook,
                    ..eligible_observation()
                },
                LoadedModuleReloadEligibility::SourceFilterOrCompileHookBoundary,
            ),
            (
                ReloadAdmissionObservation {
                    module_classification: ModuleClassification::GeneratedOrEval,
                    ..eligible_observation()
                },
                LoadedModuleReloadEligibility::GeneratedOrEvalSource,
            ),
            (
                ReloadAdmissionObservation {
                    client_source_matches_saved: false,
                    ..eligible_observation()
                },
                LoadedModuleReloadEligibility::DirtyOrUnsavedSource,
            ),
            (
                ReloadAdmissionObservation {
                    identity_binding_complete: false,
                    ..eligible_observation()
                },
                LoadedModuleReloadEligibility::SourceNotExactOrStale,
            ),
            (
                ReloadAdmissionObservation { identity_current: false, ..eligible_observation() },
                LoadedModuleReloadEligibility::SourceNotExactOrStale,
            ),
            (
                ReloadAdmissionObservation {
                    active_frame_in_target: true,
                    ..eligible_observation()
                },
                LoadedModuleReloadEligibility::ActiveFrameInTarget,
            ),
        ];
        for (observation, expected) in cases {
            assert_eq!(classify_reload_eligibility(&observation), expected);
        }
        assert!(LoadedModuleReloadEligibility::EligibleSourceBackedPerlModule.is_admitted());
        assert!(
            LoadedModuleReloadEligibility::ALL
                .iter()
                .filter(|disposition| disposition.is_admitted())
                .count()
                == 1
        );
    }

    #[test]
    fn classification_refusals_outrank_source_recency_refusals() {
        let xs_with_dirty_source = ReloadAdmissionObservation {
            module_classification: ModuleClassification::XsOrNative,
            client_source_matches_saved: false,
            identity_binding_complete: false,
            ..eligible_observation()
        };
        assert_eq!(
            classify_reload_eligibility(&xs_with_dirty_source),
            LoadedModuleReloadEligibility::XsOrNativeModule
        );
    }

    #[test]
    fn dirty_client_source_is_reported_before_identity_staleness() {
        let dirty_and_stale = ReloadAdmissionObservation {
            client_source_matches_saved: false,
            identity_current: false,
            ..eligible_observation()
        };
        assert_eq!(
            classify_reload_eligibility(&dirty_and_stale),
            LoadedModuleReloadEligibility::DirtyOrUnsavedSource
        );
    }

    #[test]
    fn classification_is_deterministic_for_identical_observations() {
        let observation = eligible_observation();
        let first = classify_reload_eligibility(&observation);
        for _ in 0..8 {
            assert_eq!(classify_reload_eligibility(&observation), first);
        }
    }
}
