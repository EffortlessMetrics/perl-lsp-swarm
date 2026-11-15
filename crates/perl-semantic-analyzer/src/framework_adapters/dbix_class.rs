//! Registry-backed DBIx::Class result-class and result-source profile facts
//! (#9736).
//!
//! This shadow adapter establishes only the reviewed static identity seam:
//! exact `DBIx::Class::Core` module evidence, an exact `use base` or
//! `use parent` activation, and a static `__PACKAGE__->table(...)` source
//! declaration. Result classes and result sources receive distinct,
//! root-scoped identities. Columns, relationships, key schemas, ResultSet
//! types, provider behavior, and runtime schema inspection are deliberately
//! outside this module.

use perl_semantic_facts::Confidence;
#[cfg(test)]
use perl_semantic_facts::framework::ModuleActivationIdentity;
use perl_semantic_facts::framework::{
    AdapterDescriptor, AdapterDetectionInput, AdapterDetectionResult, AdapterDisposition,
    DetectionAbsenceReason, DetectionOutcome, ModuleSelectorEvaluation, ModuleSelectorOutcome,
    UnavailableReason,
};
use perl_semantic_facts::framework_adapters::dbix_class::*;
#[cfg(test)]
use perl_semantic_facts::{AnchorId, FileId, SourceGeneration};

/// Build the canonical DBIx::Class adapter descriptor.
#[must_use]
pub fn dbix_class_descriptor() -> AdapterDescriptor {
    let mut descriptor = AdapterDescriptor::new(
        DBIX_CLASS_ADAPTER_ID,
        "dbix-class-result-source",
        DBIX_CLASS_FRAMEWORK_NAME,
        Some(DBIX_CLASS_VERSION_CONSTRAINT.to_string()),
        DBIX_CLASS_DESCRIPTOR_REVISION,
        AdapterDisposition::Shadow,
    );
    descriptor.required_module_selectors = vec![DBIX_CLASS_CORE_MODULE.to_string()];
    descriptor
}

/// Run checked DBIx::Class module/version detection.
///
/// The detector accepts exactly one terminal observation for the canonical
/// `DBIx::Class::Core` selector. Name-only, unresolved, ambiguous,
/// unsupported-version, duplicate, or cancelled evidence fails closed.
#[must_use]
pub fn detect_dbix_class(input: &AdapterDetectionInput) -> AdapterDetectionResult {
    if input.descriptor != dbix_class_descriptor() {
        return AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unsupported {
                reason: "input descriptor does not match the canonical DBIx::Class adapter"
                    .to_string(),
            },
        );
    }
    if input.cancellation.is_cancelled {
        return AdapterDetectionResult::for_input(input, DetectionOutcome::Cancelled);
    }

    let owned: Vec<&ModuleSelectorEvaluation> = input
        .module_observation
        .evaluations
        .iter()
        .filter(|evaluation| evaluation.selector == DBIX_CLASS_CORE_MODULE)
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
                        "selector `{DBIX_CLASS_CORE_MODULE}` carries {} terminal evaluations; \
                         exactly one is required",
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
                    "selector `{DBIX_CLASS_CORE_MODULE}` matched more than one module identity"
                )],
            },
        ),
        ModuleSelectorOutcome::Matched { activation, evidence_class } => {
            let identity_confidence = evidence_class.confidence_ceiling();
            if activation.module_name != DBIX_CLASS_CORE_MODULE {
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Conflicting {
                        conflict_descriptions: vec![format!(
                            "selector `{DBIX_CLASS_CORE_MODULE}` resolved unrelated module `{}`",
                            activation.module_name
                        )],
                    },
                );
            }
            if identity_confidence != Confidence::High {
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: format!(
                            "DBIx::Class::Core matched with {identity_confidence:?} identity \
                             evidence; exact activation requires resolved module identity"
                        ),
                    },
                );
            }
            let Some(version) = &activation.observed_version else {
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: "DBIx::Class::Core activation lacks observed version evidence"
                            .to_string(),
                    },
                );
            };
            match perl_semantic_facts::framework::version_constraint_matches(
                DBIX_CLASS_VERSION_CONSTRAINT,
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
                .with_version_evidence(version.clone()),
                None => AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: format!(
                            "observed DBIx::Class version `{}` is not comparable with `{}`",
                            version.version, DBIX_CLASS_VERSION_CONSTRAINT
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

const SHADOW_LIMITATIONS: &[&str] = &[
    "shadow adapter: comparison-only output; not publication authority",
    "columns, relationships, key schemas, ResultSet types, and providers are not modeled",
    "runtime schema registration and database metadata are not consulted",
];

/// Build checked result-class and result-source identity facts.
///
/// Exact output requires a current checked module/version detection, one exact
/// static inheritance form, one static table/source declaration, current known
/// generations, a non-empty package, and source ranges that are structurally
/// valid. A table declaration without exact DBIx::Class activation never
/// becomes framework evidence.
#[must_use]
pub fn dbix_result_profile_facts(
    detection: &AdapterDetectionResult,
    anchor: &DbixResultSiteAnchor,
    inheritance: &DbixClassInheritanceEvidence,
    table: &DbixTableEvidence,
) -> DbixResultProfileFacts {
    let mut facts = DbixResultProfileFacts::new(
        DbixResultProfileOutcome::Absent { reason: "profile not evaluated".to_string() },
        DBIX_CLASS_PROFILE_VERSION,
        Confidence::Low,
        DbixFactCompleteness::Bounded,
        anchor.source_generation.clone(),
        detection.project_generation.clone(),
        detection
            .input_identity
            .as_ref()
            .map(|identity| identity.module_observation.scope_identity.clone()),
        detection
            .input_identity
            .as_ref()
            .map(|identity| identity.module_observation.environment_identity.clone()),
        detection.contributing_modules.first().cloned(),
        None,
        None,
        None,
        SHADOW_LIMITATIONS.iter().map(ToString::to_string).collect(),
    );

    match &detection.outcome {
        DetectionOutcome::Cancelled => {
            facts.outcome = DbixResultProfileOutcome::InstrumentFailure {
                reason: "detection was cancelled".to_string(),
            };
            return facts;
        }
        DetectionOutcome::BudgetExhausted => {
            facts.outcome = DbixResultProfileOutcome::InstrumentFailure {
                reason: "detection exhausted its budget".to_string(),
            };
            return facts;
        }
        DetectionOutcome::Unavailable { reason } => {
            facts.outcome =
                DbixResultProfileOutcome::MissingOrUnavailable { reason: format!("{reason:?}") };
            return facts;
        }
        DetectionOutcome::Conflicting { conflict_descriptions } => {
            facts.outcome = DbixResultProfileOutcome::AmbiguousOrConflicting {
                reason: conflict_descriptions.join("; "),
            };
            return facts;
        }
        DetectionOutcome::Unsupported { reason } => {
            facts.outcome = DbixResultProfileOutcome::Unsupported { reason: reason.clone() };
            return facts;
        }
        DetectionOutcome::Absent {
            reason: DetectionAbsenceReason::VersionConstraintNotSatisfied,
        } => {
            facts.outcome = DbixResultProfileOutcome::Unsupported {
                reason: format!(
                    "observed version does not satisfy `{DBIX_CLASS_VERSION_CONSTRAINT}`"
                ),
            };
            return facts;
        }
        DetectionOutcome::Absent { reason } => {
            facts.outcome = DbixResultProfileOutcome::Absent { reason: format!("{reason:?}") };
            return facts;
        }
        DetectionOutcome::Detected { confidence, framework_version } => {
            facts.confidence = *confidence;
            facts.framework_version = framework_version.clone();
        }

        _ => {
            facts.outcome = DbixResultProfileOutcome::Unsupported {
                reason: "unrecognized detection outcome".to_string(),
            };
            return facts;
        }
    }

    match inheritance {
        DbixClassInheritanceEvidence::Missing => {
            facts.outcome = DbixResultProfileOutcome::Absent {
                reason: "static table/source declaration lacks exact DBIx::Class activation"
                    .to_string(),
            };
            return facts;
        }
        DbixClassInheritanceEvidence::Dynamic { reason } => {
            facts.outcome = DbixResultProfileOutcome::DynamicBoundary { reason: reason.clone() };
            return facts;
        }
        DbixClassInheritanceEvidence::Unsupported { reason } => {
            facts.outcome = DbixResultProfileOutcome::Unsupported { reason: reason.clone() };
            return facts;
        }
        DbixClassInheritanceEvidence::Recovered { reason } => {
            facts.outcome = DbixResultProfileOutcome::RecoveredSource { reason: reason.clone() };
            return facts;
        }
        DbixClassInheritanceEvidence::Exact { .. } => {}

        _ => {
            facts.outcome = DbixResultProfileOutcome::Unsupported {
                reason: "unrecognized inheritance evidence".to_string(),
            };
            return facts;
        }
    }
    if !inheritance.is_exact() {
        facts.outcome = DbixResultProfileOutcome::Unsupported {
            reason: "inheritance evidence is outside the reviewed profile".to_string(),
        };
        return facts;
    }

    let (table_name, table_anchor_id, table_range) = match table {
        DbixTableEvidence::Static { name, anchor_id, source_range } => {
            (name.trim(), *anchor_id, *source_range)
        }
        DbixTableEvidence::Missing => {
            facts.outcome = DbixResultProfileOutcome::Absent {
                reason: "result class has no static table/source declaration".to_string(),
            };
            return facts;
        }
        DbixTableEvidence::Dynamic { reason } => {
            facts.outcome = DbixResultProfileOutcome::DynamicBoundary { reason: reason.clone() };
            return facts;
        }
        DbixTableEvidence::Recovered { reason } => {
            facts.outcome = DbixResultProfileOutcome::RecoveredSource { reason: reason.clone() };
            return facts;
        }

        _ => {
            facts.outcome = DbixResultProfileOutcome::Unsupported {
                reason: "unrecognized table evidence".to_string(),
            };
            return facts;
        }
    };

    let Some(package) =
        anchor.package.as_deref().map(str::trim).filter(|package| !package.is_empty())
    else {
        facts.outcome = DbixResultProfileOutcome::StaleOrIncomplete {
            reason: "result class lacks a package identity".to_string(),
        };
        return facts;
    };
    if table_name.trim().is_empty() || table_range.1 <= table_range.0 {
        facts.outcome = DbixResultProfileOutcome::RecoveredSource {
            reason: "static table/source spelling or range is empty".to_string(),
        };
        return facts;
    }
    let Some((activation_start, activation_end)) = anchor.activation_range else {
        facts.outcome = DbixResultProfileOutcome::StaleOrIncomplete {
            reason: "exact activation lacks a source declaration range".to_string(),
        };
        return facts;
    };
    if activation_end <= activation_start || anchor.activation_anchor_id.is_none() {
        facts.outcome = DbixResultProfileOutcome::RecoveredSource {
            reason: "activation anchor or range is invalid".to_string(),
        };
        return facts;
    }
    if let Some(reason) = detection_completeness_reason(detection, anchor) {
        facts.outcome = DbixResultProfileOutcome::StaleOrIncomplete { reason };
        return facts;
    }

    let Some(scope_identity) =
        facts.scope_identity.as_deref().map(str::trim).filter(|scope| !scope.is_empty())
    else {
        facts.outcome = DbixResultProfileOutcome::StaleOrIncomplete {
            reason: "checked detection lacks a non-empty root/scope identity".to_string(),
        };
        return facts;
    };

    let result_class = DbixResultClassIdentity::new(
        package.to_string(),
        scope_identity.to_string(),
        anchor.root_identity.clone(),
        anchor.schema_identity.clone(),
    );
    facts.result_source = Some(DbixResultSourceIdentity::new(
        result_class.key.clone(),
        table_name.to_string(),
        table_anchor_id,
        table_range,
    ));
    facts.result_class = Some(result_class);
    facts.outcome = DbixResultProfileOutcome::Exact;
    facts.completeness = DbixFactCompleteness::Complete;
    facts
}

fn detection_completeness_reason(
    detection: &AdapterDetectionResult,
    anchor: &DbixResultSiteAnchor,
) -> Option<String> {
    if detection.descriptor != dbix_class_descriptor() {
        return Some("detection belongs to a different adapter descriptor".to_string());
    }
    if detection.confidence_is_not_high() {
        return Some("detection confidence is not high".to_string());
    }
    if detection.contributing_modules.len() != 1 || detection.version_evidence.is_none() {
        return Some(
            "detected result lacks exactly one contributing module and version evidence"
                .to_string(),
        );
    }
    let Some(identity) = &detection.input_identity else {
        return Some("detected result lacks its checked input identity".to_string());
    };
    if identity.descriptor != dbix_class_descriptor() {
        return Some("input identity belongs to a different adapter descriptor".to_string());
    }
    if !anchor.source_generation.is_known() || !detection.project_generation.is_known() {
        return Some("source or project generation is unknown".to_string());
    }
    if anchor.source_generation != detection.project_generation {
        return Some(format!(
            "site generation {:?} does not match detection generation {:?}",
            anchor.source_generation, detection.project_generation
        ));
    }
    let module = &detection.contributing_modules[0];
    if module.module_name != DBIX_CLASS_CORE_MODULE
        || module.generation != detection.project_generation
        || !module.generation.is_known()
    {
        return Some("contributing module identity or generation is not current".to_string());
    }
    let Some(version) = &detection.version_evidence else {
        return Some("version evidence is missing".to_string());
    };
    if !version.generation.is_known() || version.generation != detection.project_generation {
        return Some("version evidence generation is not current".to_string());
    }
    if identity.module_observation.generation != detection.project_generation {
        return Some("module observation generation does not match detection".to_string());
    }
    let owned: Vec<&ModuleSelectorEvaluation> = identity
        .module_observation
        .evaluations
        .iter()
        .filter(|evaluation| evaluation.selector == DBIX_CLASS_CORE_MODULE)
        .collect();
    let [evaluation] = owned.as_slice() else {
        return Some(format!(
            "input identity carries {} evaluations for `{DBIX_CLASS_CORE_MODULE}`",
            owned.len()
        ));
    };
    match &evaluation.outcome {
        ModuleSelectorOutcome::Matched { activation, .. }
            if activation.module_name == DBIX_CLASS_CORE_MODULE
                && activation.generation == detection.project_generation =>
        {
            None
        }
        _ => Some("owned selector does not reconcile with the detection".to_string()),
    }
}

trait DetectionConfidenceExt {
    fn confidence_is_not_high(&self) -> bool;
}

impl DetectionConfidenceExt for AdapterDetectionResult {
    fn confidence_is_not_high(&self) -> bool {
        !matches!(self.outcome, DetectionOutcome::Detected { confidence: Confidence::High, .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_semantic_facts::framework::{
        AdapterCancellation, DetectionEvidenceClass, ModuleObservationReceipt,
        ModuleSelectorEvaluation, ModuleVersionEvidence,
    };
    use perl_test_must::must_some_with;

    fn detection(scope: &str, generation: &str, version: &str) -> AdapterDetectionResult {
        let generation = SourceGeneration::known(generation);
        let activation =
            ModuleActivationIdentity::new(DBIX_CLASS_CORE_MODULE, None, generation.clone())
                .with_observed_version(ModuleVersionEvidence::new(version, generation.clone()));
        let input = AdapterDetectionInput::new(
            dbix_class_descriptor(),
            ModuleObservationReceipt::new(
                "module-resolver.v1",
                scope,
                "project-environment.v1",
                generation,
                "sha256:dbix-class-fixture",
                vec![ModuleSelectorEvaluation::matched(
                    DBIX_CLASS_CORE_MODULE,
                    activation,
                    DetectionEvidenceClass::ResolvedModule,
                )],
            ),
            None,
            AdapterCancellation::active(),
        );
        detect_dbix_class(&input)
    }

    fn anchor(generation: &str) -> DbixResultSiteAnchor {
        DbixResultSiteAnchor::new(
            FileId(1),
            Some("App::Schema::Result::User".to_string()),
            Some(AnchorId(10)),
            Some((10, 46)),
            SourceGeneration::known(generation),
            Some("root:fixture".to_string()),
            Some("App::Schema".to_string()),
        )
    }

    fn base_inheritance() -> DbixClassInheritanceEvidence {
        DbixClassInheritanceEvidence::Exact {
            form: DbixClassInheritanceForm::Base,
            module: DBIX_CLASS_CORE_MODULE.to_string(),
        }
    }

    fn table(name: &str) -> DbixTableEvidence {
        DbixTableEvidence::Static {
            name: name.to_string(),
            anchor_id: AnchorId(50),
            source_range: (69, 76),
        }
    }

    #[test]
    fn descriptor_is_core_selective_and_shadow() {
        let descriptor = dbix_class_descriptor();
        assert_eq!(descriptor.required_module_selectors, vec![DBIX_CLASS_CORE_MODULE.to_string()]);
        assert_eq!(descriptor.disposition, AdapterDisposition::Shadow);
        assert_eq!(
            descriptor.framework_version_constraint.as_deref(),
            Some(DBIX_CLASS_VERSION_CONSTRAINT)
        );
    }

    #[test]
    fn exact_base_and_static_table_emit_distinct_identities() {
        let facts = dbix_result_profile_facts(
            &detection("root:fixture", "gen-1", "0.082843"),
            &anchor("gen-1"),
            &base_inheritance(),
            &table("users"),
        );
        assert!(facts.outcome.is_exact());
        assert_eq!(facts.completeness, DbixFactCompleteness::Complete);
        let identities = "exact facts must carry both identities";
        let result_class = must_some_with(facts.result_class.as_ref(), identities);
        let result_source = must_some_with(facts.result_source.as_ref(), identities);
        assert_ne!(result_class.key, result_source.key);
        assert_eq!(result_source.result_class_key, result_class.key);
        assert_eq!(result_source.table_name, "users");
    }

    #[test]
    fn parent_form_is_equally_admitted() {
        let inheritance = DbixClassInheritanceEvidence::Exact {
            form: DbixClassInheritanceForm::Parent,
            module: DBIX_CLASS_CORE_MODULE.to_string(),
        };
        let facts = dbix_result_profile_facts(
            &detection("root:fixture", "gen-1", "0.082843"),
            &anchor("gen-1"),
            &inheritance,
            &table("users"),
        );
        assert!(facts.outcome.is_exact());
    }

    #[test]
    fn table_without_activation_is_not_framework_evidence() {
        let facts = dbix_result_profile_facts(
            &detection("root:fixture", "gen-1", "0.082843"),
            &anchor("gen-1"),
            &DbixClassInheritanceEvidence::Missing,
            &table("users"),
        );
        assert!(matches!(facts.outcome, DbixResultProfileOutcome::Absent { .. }));
        assert!(facts.result_class.is_none());
        assert!(facts.result_source.is_none());
    }

    #[test]
    fn same_table_spelling_is_isolated_by_root_scope() {
        let first = dbix_result_profile_facts(
            &detection("root:first", "gen-1", "0.082843"),
            &anchor("gen-1"),
            &base_inheritance(),
            &table("users"),
        );
        let second = dbix_result_profile_facts(
            &detection("root:second", "gen-1", "0.082843"),
            &anchor("gen-1"),
            &base_inheritance(),
            &table("users"),
        );
        let identities = "both exact profiles must carry result-source identities";
        let first_source = must_some_with(first.result_source.as_ref(), identities);
        let second_source = must_some_with(second.result_source.as_ref(), identities);
        assert_ne!(first_source.key, second_source.key);
    }

    #[test]
    fn stale_source_generation_cannot_be_exact() {
        let facts = dbix_result_profile_facts(
            &detection("root:fixture", "gen-2", "0.082843"),
            &anchor("gen-1"),
            &base_inheritance(),
            &table("users"),
        );
        assert!(matches!(facts.outcome, DbixResultProfileOutcome::StaleOrIncomplete { .. }));
    }

    #[test]
    fn dynamic_parent_is_an_explicit_boundary() {
        let facts = dbix_result_profile_facts(
            &detection("root:fixture", "gen-1", "0.082843"),
            &anchor("gen-1"),
            &DbixClassInheritanceEvidence::Dynamic {
                reason: "parent expression is computed".to_string(),
            },
            &table("users"),
        );
        assert!(matches!(facts.outcome, DbixResultProfileOutcome::DynamicBoundary { .. }));
    }

    #[test]
    fn unsupported_version_cannot_be_exact() {
        let facts = dbix_result_profile_facts(
            &detection("root:fixture", "gen-1", "0.090000"),
            &anchor("gen-1"),
            &base_inheritance(),
            &table("users"),
        );
        assert!(matches!(facts.outcome, DbixResultProfileOutcome::Unsupported { .. }));
    }

    #[test]
    fn identity_order_and_spelling_are_deterministic() {
        let first = dbix_result_profile_facts(
            &detection("root:fixture", "gen-1", "0.082843"),
            &anchor("gen-1"),
            &base_inheritance(),
            &table("users"),
        );
        let second = dbix_result_profile_facts(
            &detection("root:fixture", "gen-1", "0.082843"),
            &anchor("gen-1"),
            &base_inheritance(),
            &table("users"),
        );
        assert_eq!(first.result_class, second.result_class);
        assert_eq!(first.result_source, second.result_source);
    }
}
