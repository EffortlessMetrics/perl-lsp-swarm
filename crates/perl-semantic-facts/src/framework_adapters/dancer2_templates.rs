//! Registry-activated Dancer2 template-target fact minting (#8937).
//!
//! This module turns source-extracted Dancer2 `template` calls into the
//! canonical [`TemplateTargetFact`] family. It is the facts side of the
//! #8937 template-target slice and builds directly on the #8914 activation
//! seam:
//!
//! - template facts mint **only through the registry-activated adapter**: a
//!   detected framework and an exact activation are both required; without
//!   them this function returns no facts (never name-only template synthesis);
//! - a call whose `template` keyword the activating import excluded via
//!   `!template` is not a template of this activation — the keyword was never
//!   imported, so no fact is minted;
//! - every fact is generation-owned: the envelope carries the detection
//!   receipt's project generation and invalidation dependencies over the
//!   owning source file and the activating `Dancer2` module;
//! - filesystem resolution is **not** performed here. Canonical project
//!   configuration (#7900) is not on main, so the filesystem slot stays
//!   [`TemplateFilesystemTarget::Unresolved`] and the logical name remains
//!   the exact source-backed identity;
//! - facts are shadow receipts: the adapter disposition remains `Shadow`, so
//!   no provider surface can publish them as production authority (#6822
//!   owns publication).
//!
//! Computed, interpolated, and escaped names stay typed dynamic boundaries.
//! Extra operands (`tokens`, `options`) are admitted by the reviewed
//! `template($name, $tokens, $opts)` arity and are not interpreted.

use crate::framework::AdapterDetectionResult;
use crate::framework_adapters::dancer2::{
    DANCER2_ADAPTER_ID, DANCER2_FRAMEWORK_NAME, Dancer2ActivationFacts, Dancer2KeywordState,
};
use crate::template::{
    TemplateReference, TemplateTargetFact, template_target_envelope, template_target_fact_identity,
};
use crate::{
    AnchorId, BoundaryKind, FileId, InvalidationDependency, SemanticReasonCode, SourceAnchor,
    SourceGeneration,
};

/// Versioned identity of the reviewed Dancer2 template-target contract.
pub const DANCER2_TEMPLATE_CONTRACT_VERSION: &str = "dancer2-templates.1-1.v1";

/// Template keyword of the reviewed Dancer2 DSL contract.
pub const DANCER2_TEMPLATE_KEYWORD: &str = "template";

/// One source-extracted Dancer2 template-target call awaiting minting.
///
/// Produced by the AST extractor in
/// `perl_semantic_analyzer::analysis::dancer2_templates`; this carrier adds the
/// package/file/declaration identity around the canonical payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2TemplateDeclaration {
    /// Package the call appears in (activation scope).
    pub package: Option<String>,
    /// File the call appears in.
    pub file_id: FileId,
    /// Full call range (keyword start to last operand end).
    pub declaration_start_byte: u32,
    /// End of the full call range.
    pub declaration_end_byte: u32,
    /// Canonical template payload (name/filesystem).
    pub template: TemplateReference,
}

/// Mint canonical Dancer2 template-target facts for one activating package.
///
/// Returns an empty vector unless `detection` established the framework and
/// `activation` is exact (registry-activated adapter contract). Calls of
/// other packages and calls behind a `!template` exclusion at the activating
/// import are skipped. All minted facts carry the detection receipt's
/// generation and invalidation dependencies over the owning source file and
/// the `Dancer2` module.
#[must_use]
pub fn dancer2_template_facts(
    detection: &AdapterDetectionResult,
    activation: &Dancer2ActivationFacts,
    package: Option<&str>,
    declarations: &[Dancer2TemplateDeclaration],
) -> Vec<TemplateTargetFact> {
    if !detection.is_detected() || !activation.is_exact() {
        return Vec::new();
    }
    let Dancer2ActivationFacts { state, keywords, .. } = activation;
    let crate::framework_adapters::dancer2::Dancer2ActivationState::Exact {
        application_name,
        framework_version,
        source_generation,
    } = state
    else {
        return Vec::new();
    };

    if !keywords.iter().any(|fact| {
        fact.keyword == DANCER2_TEMPLATE_KEYWORD && fact.state == Dancer2KeywordState::Imported
    }) {
        return Vec::new();
    }

    let mut facts = Vec::new();
    for declaration in declarations {
        if declaration.package.as_deref() != package {
            continue;
        }
        if declaration.template.keyword != DANCER2_TEMPLATE_KEYWORD {
            continue;
        }
        facts.push(mint_template_fact(
            declaration,
            application_name,
            framework_version,
            source_generation,
        ));
    }
    facts
}

fn mint_template_fact(
    declaration: &Dancer2TemplateDeclaration,
    application_name: &str,
    framework_version: &str,
    generation: &SourceGeneration,
) -> TemplateTargetFact {
    let (fact_id, entity_id) = template_target_fact_identity(
        declaration.file_id,
        declaration.template.declaration_index,
        generation,
    );
    let declaration_anchor = SourceAnchor::new(
        Some(AnchorId(u64::from(declaration.declaration_start_byte))),
        declaration.file_id,
        declaration.declaration_start_byte,
        declaration.declaration_end_byte,
    );
    let exact = !declaration.template.has_name_boundary();
    let (boundary_kind, boundary_reason) = primary_boundary(&declaration.template);
    let envelope = template_target_envelope(
        fact_id,
        entity_id,
        declaration.package.as_deref(),
        declaration_anchor,
        generation,
        vec![
            InvalidationDependency::new(
                format!("source:{}", declaration.file_id.0),
                generation.clone(),
            ),
            InvalidationDependency::new("module:Dancer2", generation.clone()),
        ],
        boundary_kind,
        boundary_reason,
        exact,
    );
    TemplateTargetFact::new(
        envelope,
        DANCER2_FRAMEWORK_NAME,
        DANCER2_ADAPTER_ID,
        framework_version,
        application_name,
        declaration.template.clone(),
    )
}

fn primary_boundary(template: &TemplateReference) -> (Option<BoundaryKind>, SemanticReasonCode) {
    if template.has_name_boundary() {
        return (Some(BoundaryKind::DynamicValue), SemanticReasonCode::DynamicValue);
    }
    (None, SemanticReasonCode::ExactSource)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SemanticFactStatus;
    use crate::framework::{
        AdapterCancellation, AdapterDetectionInput, DetectionEvidenceClass,
        ModuleActivationIdentity, ModuleObservationReceipt, ModuleSelectorEvaluation,
        ModuleSelectorOutcome, ModuleVersionEvidence,
    };
    use crate::framework_adapters::dancer2::{
        Dancer2ActivationState, DslSelection, dancer2_activation_facts, dancer2_descriptor,
        detect_dancer2, parse_dancer2_import_args,
    };
    use crate::template::{TemplateFilesystemTarget, TemplateName, TemplateNameSelection};
    use perl_test_must::must_some;

    fn detected_input(generation: &str) -> AdapterDetectionInput {
        let activation = ModuleActivationIdentity::new(
            "Dancer2",
            Some(FileId(7)),
            SourceGeneration::known(generation),
        )
        .with_observed_version(ModuleVersionEvidence::new(
            "1.1.1",
            SourceGeneration::known(generation),
        ));
        let observation = ModuleObservationReceipt::new(
            "module-resolver.v1",
            "root:fixture",
            "project-environment.v1",
            SourceGeneration::known(generation),
            "sha256:fixture-input",
            vec![ModuleSelectorEvaluation::new(
                "Dancer2",
                ModuleSelectorOutcome::Matched {
                    activation,
                    evidence_class: DetectionEvidenceClass::ResolvedModule,
                },
            )],
        );
        AdapterDetectionInput::new(
            dancer2_descriptor(),
            observation,
            None,
            AdapterCancellation::active(),
        )
    }

    fn exact_activation(generation: &str) -> Dancer2ActivationFacts {
        let detection = detect_dancer2(&detected_input(generation));
        dancer2_activation_facts(&detection, Some("App"), &parse_dancer2_import_args(&[]))
    }

    fn index_declaration(index: u32) -> Dancer2TemplateDeclaration {
        Dancer2TemplateDeclaration {
            package: Some("App".to_string()),
            file_id: FileId(1),
            declaration_start_byte: 0,
            declaration_end_byte: 16,
            template: TemplateReference {
                declaration_index: index,
                keyword: DANCER2_TEMPLATE_KEYWORD.to_string(),
                keyword_anchor: SourceAnchor::new(Some(AnchorId(0)), FileId(1), 0, 8),
                name: TemplateNameSelection::Literal(TemplateName {
                    logical: "index".to_string(),
                    anchor: SourceAnchor::new(Some(AnchorId(9)), FileId(1), 9, 16),
                }),
                filesystem: TemplateFilesystemTarget::views_root_unresolved(),
            },
        }
    }

    #[test]
    fn exact_activation_mints_generation_owned_logical_facts() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let activation = exact_activation("gen-1");
        let facts = dancer2_template_facts(
            &detection,
            &activation,
            Some("App"),
            &[index_declaration(0), index_declaration(1)],
        );
        assert_eq!(facts.len(), 2, "same-name calls stay distinct by order");
        for (index, fact) in facts.iter().enumerate() {
            assert_eq!(fact.framework_name, DANCER2_FRAMEWORK_NAME);
            assert_eq!(fact.adapter_id, DANCER2_ADAPTER_ID);
            assert_eq!(fact.framework_version, "1.1.1");
            assert_eq!(fact.application_name, "App");
            assert_eq!(fact.envelope.package.as_deref(), Some("App"));
            assert_eq!(fact.envelope.source_generation, SourceGeneration::known("gen-1"));
            assert_eq!(fact.template.declaration_index, index as u32);
            assert_eq!(fact.template.name.logical(), Some("index"));
            assert_eq!(fact.status(), SemanticFactStatus::Exact);
            assert!(matches!(
                fact.template.filesystem,
                TemplateFilesystemTarget::Unresolved { .. }
            ));
        }
        assert_ne!(facts[0].envelope.fact_id, facts[1].envelope.fact_id);
    }

    #[test]
    fn no_activation_or_detection_mints_no_facts() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let mut activation = exact_activation("gen-1");
        activation.state = Dancer2ActivationState::NotActivated { reason: "test".to_string() };
        assert!(
            dancer2_template_facts(&detection, &activation, Some("App"), &[index_declaration(0)])
                .is_empty()
        );

        let observation = ModuleObservationReceipt::new(
            "module-resolver.v1",
            "root:fixture",
            "project-environment.v1",
            SourceGeneration::known("gen-1"),
            "sha256:fixture-input",
            vec![ModuleSelectorEvaluation::new("Dancer2", ModuleSelectorOutcome::Absent)],
        );
        let input = AdapterDetectionInput::new(
            dancer2_descriptor(),
            observation,
            None,
            AdapterCancellation::active(),
        );
        let undetected = detect_dancer2(&input);
        let activation =
            dancer2_activation_facts(&undetected, Some("App"), &parse_dancer2_import_args(&[]));
        assert!(
            dancer2_template_facts(&undetected, &activation, Some("App"), &[index_declaration(0)])
                .is_empty()
        );
    }

    #[test]
    fn excluded_template_keyword_mints_nothing() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let args: Vec<String> = ["qw(!template)"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_import_args(&args);
        let activation = dancer2_activation_facts(&detection, Some("App"), &evidence);
        assert!(activation.is_exact(), "exclusion alone keeps activation exact");
        assert!(
            dancer2_template_facts(&detection, &activation, Some("App"), &[index_declaration(0)])
                .is_empty(),
            "`!template` at the import means no template keyword was imported"
        );
    }

    #[test]
    fn other_package_declarations_do_not_mint() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let activation = exact_activation("gen-1");
        let mut other = index_declaration(0);
        other.package = Some("Other".to_string());
        assert!(dancer2_template_facts(&detection, &activation, Some("App"), &[other]).is_empty());
    }

    #[test]
    fn dynamic_name_degrades_with_a_dynamic_boundary() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let activation = exact_activation("gen-1");
        let mut computed = index_declaration(0);
        computed.template.name = TemplateNameSelection::Dynamic {
            reason: "computed template name".to_string(),
            anchor: SourceAnchor::new(Some(AnchorId(9)), FileId(1), 9, 14),
        };
        let facts = dancer2_template_facts(&detection, &activation, Some("App"), &[computed]);
        assert_eq!(facts.len(), 1, "the call is retained");
        assert_eq!(facts[0].status(), SemanticFactStatus::Degraded);
        let boundary = must_some(facts[0].envelope.boundary.as_ref());
        assert_eq!(boundary.kind, BoundaryKind::DynamicValue);
        assert!(facts[0].template.name.logical().is_none());
    }

    #[test]
    fn custom_dsl_mints_nothing() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let mut evidence = parse_dancer2_import_args(&[]);
        evidence.dsl = Some(DslSelection::CustomLiteral("My::DSL".to_string()));
        let activation = dancer2_activation_facts(&detection, Some("App"), &evidence);
        assert!(!activation.is_exact());
        assert!(
            dancer2_template_facts(&detection, &activation, Some("App"), &[index_declaration(0)])
                .is_empty()
        );
    }
}
