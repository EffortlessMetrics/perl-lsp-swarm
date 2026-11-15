//! Checked Moose and Moose::Role activation detection (#7788).
//!
//! This module owns activation identity only. It does not interpret `has`,
//! generate members, resolve roles or inheritance, classify constraints, or
//! change provider behavior. Exact detection requires both:
//!
//! - one canonical descriptor (`Moose` for classes or `Moose::Role` for
//!   roles); and
//! - current resolved module identity and Moose 2.x version evidence from the
//!   checked framework input.
//!
//! Class and role activation remain separate descriptor identities. Package
//! shape, same-named DSL calls, and ambient installations never change one
//! descriptor into the other.

use perl_semantic_facts::Confidence;
use perl_semantic_facts::framework::{
    AdapterDescriptor, AdapterDetectionInput, AdapterDetectionResult, AdapterDisposition,
    DetectionAbsenceReason, DetectionOutcome, ModuleSelectorEvaluation, ModuleSelectorOutcome,
    UnavailableReason,
};
use perl_semantic_facts::framework_adapters::moose::*;

/// Canonical descriptor for class activation through `use Moose`.
///
/// Shadow disposition, matching the not-yet-wired substrate boundary of the
/// Mojo::Base sibling: no rs-core consumer reads this adapter yet, so output is
/// comparison-only and authority validation rejects it as publication
/// authority. The registry/publication cutover (#6821) must land and assign
/// final identities before this can become `Production`.
#[must_use]
pub fn moose_class_descriptor() -> AdapterDescriptor {
    moose_descriptor(MooseActivationKind::Class)
}

/// Canonical descriptor for role activation through `use Moose::Role`.
///
/// Shadow disposition, matching the not-yet-wired substrate boundary of the
/// Mojo::Base sibling: no rs-core consumer reads this adapter yet, so output is
/// comparison-only and authority validation rejects it as publication
/// authority. The registry/publication cutover (#6821) must land and assign
/// final identities before this can become `Production`.
#[must_use]
pub fn moose_role_descriptor() -> AdapterDescriptor {
    moose_descriptor(MooseActivationKind::Role)
}

/// Deterministic descriptor inventory, ordered class then role.
#[must_use]
pub fn moose_descriptors() -> [AdapterDescriptor; 2] {
    [moose_class_descriptor(), moose_role_descriptor()]
}

fn moose_descriptor(kind: MooseActivationKind) -> AdapterDescriptor {
    AdapterDescriptor::new(
        kind.adapter_id(),
        kind.adapter_name(),
        kind.module_name(),
        Some(MOOSE_VERSION_CONSTRAINT.to_string()),
        MOOSE_DESCRIPTOR_REVISION,
        AdapterDisposition::Shadow,
    )
}

/// Detect class activation from one checked input.
#[must_use]
pub fn detect_moose_class(input: &AdapterDetectionInput) -> AdapterDetectionResult {
    detect_moose(input, MooseActivationKind::Class)
}

/// Detect role activation from one checked input.
#[must_use]
pub fn detect_moose_role(input: &AdapterDetectionInput) -> AdapterDetectionResult {
    detect_moose(input, MooseActivationKind::Role)
}

fn detect_moose(
    input: &AdapterDetectionInput,
    kind: MooseActivationKind,
) -> AdapterDetectionResult {
    let canonical = moose_descriptor(kind);
    if input.descriptor != canonical {
        return AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unsupported {
                reason: format!(
                    "input descriptor does not match the canonical {} descriptor",
                    kind.module_name()
                ),
            },
        );
    }
    if input.cancellation.is_cancelled {
        return AdapterDetectionResult::for_input(input, DetectionOutcome::Cancelled);
    }
    if !input.module_observation.generation.is_known() {
        return AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unavailable { reason: UnavailableReason::MissingGeneration },
        );
    }
    if input.detector_policy_identity.trim().is_empty()
        || input.module_observation.resolver_identity.trim().is_empty()
        || input.module_observation.scope_identity.trim().is_empty()
        || input.module_observation.environment_identity.trim().is_empty()
        || input.module_observation.content_digest.trim().is_empty()
    {
        return AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unavailable { reason: UnavailableReason::InternalError },
        );
    }

    let selector = kind.module_name();
    let owned: Vec<&ModuleSelectorEvaluation> = input
        .module_observation
        .evaluations
        .iter()
        .filter(|evaluation| evaluation.selector == selector)
        .collect();
    let [evaluation] = owned.as_slice() else {
        return if owned.is_empty() {
            AdapterDetectionResult::for_input(
                input,
                DetectionOutcome::Unavailable { reason: UnavailableReason::NoModulesAvailable },
            )
        } else {
            AdapterDetectionResult::for_input(
                input,
                DetectionOutcome::Conflicting {
                    conflict_descriptions: vec![format!(
                        "selector `{selector}` carries {} terminal evaluations; completeness \
                         requires exactly one",
                        owned.len()
                    )],
                },
            )
        };
    };
    let evaluation = *evaluation;

    match &evaluation.outcome {
        ModuleSelectorOutcome::Absent => AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Absent { reason: DetectionAbsenceReason::RequiredModulesMissing },
        ),
        ModuleSelectorOutcome::Unresolved { .. } | ModuleSelectorOutcome::Unavailable { .. } => {
            AdapterDetectionResult::for_input(
                input,
                DetectionOutcome::Unavailable { reason: UnavailableReason::NoModulesAvailable },
            )
        }
        ModuleSelectorOutcome::Ambiguous { .. } => AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Conflicting {
                conflict_descriptions: vec![format!(
                    "selector `{selector}` matched more than one module identity"
                )],
            },
        ),
        ModuleSelectorOutcome::Matched { activation, evidence_class } => {
            if activation.module_name != selector {
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Conflicting {
                        conflict_descriptions: vec![format!(
                            "selector `{selector}` resolved to foreign module `{}`",
                            activation.module_name
                        )],
                    },
                );
            }
            if !activation.generation.is_known()
                || activation.generation != input.module_observation.generation
            {
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unavailable { reason: UnavailableReason::MissingGeneration },
                );
            }

            let identity_confidence = evidence_class.confidence_ceiling();
            if identity_confidence != Confidence::High {
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: format!(
                            "{selector} matched with {identity_confidence:?} identity evidence; \
                             exact activation requires resolved module or import identity"
                        ),
                    },
                );
            }

            let Some(version) = &activation.observed_version else {
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: format!(
                            "{selector} activation lacks observed version evidence; the reviewed \
                             version constraint cannot be checked"
                        ),
                    },
                );
            };
            if version.generation != activation.generation {
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unavailable { reason: UnavailableReason::MissingGeneration },
                );
            }

            match perl_semantic_facts::framework::version_constraint_matches(
                MOOSE_VERSION_CONSTRAINT,
                &version.version,
            ) {
                Some(true) => AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Detected {
                        confidence: Confidence::High,
                        framework_version: Some(version.version.clone()),
                    },
                )
                .with_contributing_modules(vec![activation.clone()])
                .with_version_evidence(version.clone()),
                Some(false) => AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Absent {
                        reason: DetectionAbsenceReason::VersionConstraintNotSatisfied,
                    },
                )
                .with_contributing_modules(vec![activation.clone()])
                .with_version_evidence(version.clone()),
                None => AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: format!(
                            "observed {selector} version `{}` is not comparable with the reviewed \
                             constraint `{MOOSE_VERSION_CONSTRAINT}`",
                            version.version
                        ),
                    },
                ),
            }
        }

        _ => AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unsupported {
                reason: "unrecognized module observation outcome".to_string(),
            },
        ),
    }
}
