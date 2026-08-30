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

use crate::framework::{
    AdapterDescriptor, AdapterDetectionInput, AdapterDetectionResult, AdapterDisposition,
    AdapterId, DetectionAbsenceReason, DetectionOutcome, ModuleSelectorEvaluation,
    ModuleSelectorOutcome, UnavailableReason,
};
use crate::{Confidence, SourceGeneration};

/// Reviewed Moose module/version family.
///
/// The current reviewed primary documentation carries Moose 2.4000. Moose 3.x
/// is deliberately outside this profile until separately reviewed.
pub const MOOSE_VERSION_CONSTRAINT: &str = ">=2.0.0,<3.0.0";

/// Stable identity of the reviewed activation contract.
pub const MOOSE_ACTIVATION_PROFILE_VERSION: &str = "moose.activation.2.v1";

/// Provisional adapter identity for `use Moose`.
pub const MOOSE_CLASS_ADAPTER_ID: AdapterId = AdapterId(0x004D_4F53);

/// Provisional adapter identity for `use Moose::Role`.
pub const MOOSE_ROLE_ADAPTER_ID: AdapterId = AdapterId(0x004D_5253);

/// Current descriptor schema revision.
pub const MOOSE_DESCRIPTOR_REVISION: u32 = crate::framework::FRAMEWORK_ADAPTER_SCHEMA_VERSION;

/// Exact activation kind established by the imported module.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MooseActivationKind {
    /// `use Moose` class activation.
    Class,
    /// `use Moose::Role` role activation.
    Role,
}

impl MooseActivationKind {
    /// Exact module selector owned by this activation kind.
    #[must_use]
    pub const fn module_name(self) -> &'static str {
        match self {
            Self::Class => "Moose",
            Self::Role => "Moose::Role",
        }
    }

    /// Stable human-readable adapter name.
    #[must_use]
    pub const fn adapter_name(self) -> &'static str {
        match self {
            Self::Class => "moose-class",
            Self::Role => "moose-role",
        }
    }

    const fn adapter_id(self) -> AdapterId {
        match self {
            Self::Class => MOOSE_CLASS_ADAPTER_ID,
            Self::Role => MOOSE_ROLE_ADAPTER_ID,
        }
    }
}

/// Source-side import disposition retained with an activation site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MooseImportDisposition {
    /// Reviewed default import, optionally carrying a static version
    /// requirement.
    Exact,
    /// The exact module was imported with arguments outside this activation
    /// profile. The site remains inspectable but cannot establish exact
    /// activation.
    Unmodeled { arguments: Vec<String> },
}

impl MooseImportDisposition {
    /// Whether the import spelling is inside the reviewed activation profile.
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        matches!(self, Self::Exact)
    }
}

/// Load-bearing source identity for one Moose import site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MooseSiteAnchor {
    /// Caller package at the activating import.
    pub package: Option<String>,
    /// First byte of the import statement.
    pub span_start_byte: u32,
    /// Byte immediately after the import statement.
    pub span_end_byte: u32,
    /// Source generation from which this site was extracted.
    pub source_generation: SourceGeneration,
}

impl MooseSiteAnchor {
    /// Construct a source anchor.
    #[must_use]
    pub fn new(
        package: Option<String>,
        span_start_byte: u32,
        span_end_byte: u32,
        source_generation: SourceGeneration,
    ) -> Self {
        Self { package, span_start_byte, span_end_byte, source_generation }
    }
}

/// Canonical descriptor for class activation through `use Moose`.
///
/// Production disposition is intentional: checked detection receipts may be
/// authoritative. This module exposes no semantic-fact emitter, so the
/// descriptor alone cannot change provider output.
#[must_use]
pub fn moose_class_descriptor() -> AdapterDescriptor {
    moose_descriptor(MooseActivationKind::Class)
}

/// Canonical descriptor for role activation through `use Moose::Role`.
///
/// Production disposition is intentional: checked detection receipts may be
/// authoritative. This module exposes no semantic-fact emitter, so the
/// descriptor alone cannot change provider output.
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
        AdapterDisposition::Production,
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

            match crate::framework::version_constraint_matches(
                MOOSE_VERSION_CONSTRAINT,
                &version.version,
            ) {
                Some(true) => {
                    let mut result = AdapterDetectionResult::for_input(
                        input,
                        DetectionOutcome::Detected {
                            confidence: Confidence::High,
                            framework_version: Some(version.version.clone()),
                        },
                    );
                    result = result.with_contributing_modules(vec![activation.clone()]);
                    result.with_version_evidence(version.clone())
                }
                Some(false) => {
                    let mut result = AdapterDetectionResult::for_input(
                        input,
                        DetectionOutcome::Absent {
                            reason: DetectionAbsenceReason::VersionConstraintNotSatisfied,
                        },
                    );
                    result = result.with_contributing_modules(vec![activation.clone()]);
                    result.with_version_evidence(version.clone())
                }
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
    }
}
