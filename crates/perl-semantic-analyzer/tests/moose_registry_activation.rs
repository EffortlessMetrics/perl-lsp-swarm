//! End-to-end checked Moose and Moose::Role activation proof (#7788).
//!
//! Exercises source-site extraction and the production detection descriptors
//! without emitting attributes, generated members, package edges, types, or
//! provider output.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::moose_activation::{
    MooseActivationSite, extract_moose_activation_sites,
};
use perl_semantic_facts::framework::{
    AdapterCancellation, AdapterDescriptor, AdapterDetectionInput, AdapterDetectionResult,
    AdapterDisposition, DetectionAbsenceReason, DetectionAuthorityError, DetectionEvidenceClass,
    DetectionOutcome, ModuleActivationIdentity, ModuleObservationReceipt, ModuleSelectorEvaluation,
    ModuleSelectorOutcome, ModuleVersionEvidence, UnavailableReason,
};
use perl_semantic_facts::framework_adapters::moose::{
    MOOSE_ACTIVATION_PROFILE_VERSION, MOOSE_VERSION_CONSTRAINT, MooseActivationKind,
    MooseImportDisposition, detect_moose_class, detect_moose_role, moose_class_descriptor,
    moose_descriptors, moose_role_descriptor,
};
use perl_semantic_facts::{Confidence, FileId, SourceGeneration};
use perl_tdd_support::{must, must_some};

fn descriptor(kind: MooseActivationKind) -> AdapterDescriptor {
    match kind {
        MooseActivationKind::Class => moose_class_descriptor(),
        MooseActivationKind::Role => moose_role_descriptor(),
        _ => moose_class_descriptor(),
    }
}

fn observation(
    evaluations: Vec<ModuleSelectorEvaluation>,
    generation: &str,
    scope: &str,
    environment: &str,
    digest: &str,
) -> ModuleObservationReceipt {
    ModuleObservationReceipt::new(
        "module-resolver.v1",
        scope,
        environment,
        SourceGeneration::known(generation),
        digest,
        evaluations,
    )
}

fn matched(
    kind: MooseActivationKind,
    version: Option<&str>,
    generation: &str,
    evidence_class: DetectionEvidenceClass,
) -> ModuleSelectorEvaluation {
    let module_name = kind.module_name();
    let activation = ModuleActivationIdentity::new(
        module_name,
        Some(FileId(7)),
        SourceGeneration::known(generation),
    );
    let activation = match version {
        Some(version) => activation.with_observed_version(ModuleVersionEvidence::new(
            version,
            SourceGeneration::known(generation),
        )),
        None => activation,
    };
    ModuleSelectorEvaluation::new(
        module_name,
        ModuleSelectorOutcome::Matched { activation, evidence_class },
    )
}

fn input(
    kind: MooseActivationKind,
    evaluations: Vec<ModuleSelectorEvaluation>,
    generation: &str,
    scope: &str,
    environment: &str,
    digest: &str,
) -> AdapterDetectionInput {
    AdapterDetectionInput::new(
        descriptor(kind),
        observation(evaluations, generation, scope, environment, digest),
        None,
        AdapterCancellation::active(),
    )
}

fn detect(kind: MooseActivationKind, input: &AdapterDetectionInput) -> AdapterDetectionResult {
    match kind {
        MooseActivationKind::Class => detect_moose_class(input),
        MooseActivationKind::Role => detect_moose_role(input),
        _ => detect_moose_class(input),
    }
}

fn sites(code: &str, file_id: u64, generation: &str) -> Vec<MooseActivationSite> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    extract_moose_activation_sites(&ast, code, FileId(file_id), SourceGeneration::known(generation))
}

#[test]
fn descriptors_are_distinct_bounded_and_deterministic() {
    let first = moose_descriptors();
    let second = moose_descriptors();
    assert_eq!(first, second, "descriptor order and identity must be stable");
    assert_ne!(first[0].adapter_id, first[1].adapter_id);
    assert_eq!(first[0].required_module_selectors, vec!["Moose"]);
    assert_eq!(first[1].required_module_selectors, vec!["Moose::Role"]);
    assert_eq!(first[0].framework_version_constraint.as_deref(), Some(MOOSE_VERSION_CONSTRAINT));
    assert_eq!(first[1].framework_version_constraint.as_deref(), Some(MOOSE_VERSION_CONSTRAINT));
    assert!(first.iter().all(|item| item.disposition == AdapterDisposition::Production));
    assert_eq!(MOOSE_ACTIVATION_PROFILE_VERSION, "moose.activation.2.v1");
}

#[test]
fn exact_class_and_role_sites_produce_authoritative_checked_results() {
    let code = "package My::Class;\nuse Moose;\npackage My::Role;\nuse Moose::Role;\n";
    let found = sites(code, 1, "gen-1");
    assert_eq!(found.len(), 2);

    for site in &found {
        assert!(site.is_exact());
        let detection_input = input(
            site.kind,
            vec![matched(
                site.kind,
                Some("2.4000"),
                "gen-1",
                DetectionEvidenceClass::ResolvedImport,
            )],
            "gen-1",
            "root:fixture",
            "env:fixture",
            "sha256:fixture",
        );
        let detection = detect(site.kind, &detection_input);
        assert_eq!(
            detection.outcome,
            DetectionOutcome::Detected {
                confidence: Confidence::High,
                framework_version: Some("2.4000".to_string()),
            }
        );
        assert!(
            detection.is_authoritative_against(&detection_input),
            "exact current input must validate"
        );
        let receipt = detection.authority_receipt_against(&detection_input);
        assert!(receipt.authoritative);
        assert_eq!(receipt.error, None);
    }

    assert_eq!(found[0].kind, MooseActivationKind::Class);
    assert_eq!(found[0].anchor.package.as_deref(), Some("My::Class"));
    assert_eq!(found[1].kind, MooseActivationKind::Role);
    assert_eq!(found[1].anchor.package.as_deref(), Some("My::Role"));
}

#[test]
fn complete_absence_is_authoritative_but_partial_discovery_is_not_absence() {
    let absent_input = input(
        MooseActivationKind::Class,
        vec![ModuleSelectorEvaluation::absent("Moose")],
        "gen-1",
        "root:a",
        "env:a",
        "sha256:absent",
    );
    let absent = detect_moose_class(&absent_input);
    assert_eq!(
        absent.outcome,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::RequiredModulesMissing }
    );
    assert!(absent.is_authoritative_against(&absent_input));

    let unresolved_input = input(
        MooseActivationKind::Class,
        vec![ModuleSelectorEvaluation::unresolved(
            "Moose",
            "resolver could not inspect the active include path",
        )],
        "gen-1",
        "root:a",
        "env:a",
        "sha256:partial",
    );
    let unresolved = detect_moose_class(&unresolved_input);
    assert_eq!(
        unresolved.outcome,
        DetectionOutcome::Unavailable { reason: UnavailableReason::NoModulesAvailable }
    );
    assert!(!unresolved.is_authoritative_against(&unresolved_input));
}

#[test]
fn unsupported_version_and_name_only_identity_fail_closed() {
    let unsupported_input = input(
        MooseActivationKind::Class,
        vec![matched(
            MooseActivationKind::Class,
            Some("3.0.0"),
            "gen-1",
            DetectionEvidenceClass::ResolvedModule,
        )],
        "gen-1",
        "root:a",
        "env:a",
        "sha256:unsupported",
    );
    let unsupported = detect_moose_class(&unsupported_input);
    assert_eq!(
        unsupported.outcome,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::VersionConstraintNotSatisfied }
    );
    assert!(
        unsupported.is_authoritative_against(&unsupported_input),
        "supported-version absence carries exact version evidence"
    );

    let name_only_input = input(
        MooseActivationKind::Role,
        vec![matched(
            MooseActivationKind::Role,
            Some("2.4000"),
            "gen-1",
            DetectionEvidenceClass::NameOnly,
        )],
        "gen-1",
        "root:a",
        "env:a",
        "sha256:name-only",
    );
    let name_only = detect_moose_role(&name_only_input);
    assert!(matches!(name_only.outcome, DetectionOutcome::Unsupported { .. }));
    assert!(!name_only.is_authoritative_against(&name_only_input));
}

#[test]
fn missing_version_ambiguous_module_and_duplicate_rows_stay_distinct() {
    let missing_version_input = input(
        MooseActivationKind::Class,
        vec![matched(
            MooseActivationKind::Class,
            None,
            "gen-1",
            DetectionEvidenceClass::ResolvedModule,
        )],
        "gen-1",
        "root:a",
        "env:a",
        "sha256:no-version",
    );
    assert!(matches!(
        detect_moose_class(&missing_version_input).outcome,
        DetectionOutcome::Unsupported { .. }
    ));

    let ambiguous_input = input(
        MooseActivationKind::Class,
        vec![ModuleSelectorEvaluation::new(
            "Moose",
            ModuleSelectorOutcome::Ambiguous { reason: "two roots provide Moose".to_string() },
        )],
        "gen-1",
        "root:a",
        "env:a",
        "sha256:ambiguous",
    );
    assert!(matches!(
        detect_moose_class(&ambiguous_input).outcome,
        DetectionOutcome::Conflicting { .. }
    ));

    let duplicate_input = input(
        MooseActivationKind::Class,
        vec![ModuleSelectorEvaluation::absent("Moose"), ModuleSelectorEvaluation::absent("Moose")],
        "gen-1",
        "root:a",
        "env:a",
        "sha256:duplicate",
    );
    assert!(matches!(
        detect_moose_class(&duplicate_input).outcome,
        DetectionOutcome::Conflicting { .. }
    ));
}

#[test]
fn stale_module_generation_publishes_no_exact_detection() {
    let stale_input = input(
        MooseActivationKind::Class,
        vec![matched(
            MooseActivationKind::Class,
            Some("2.4000"),
            "gen-old",
            DetectionEvidenceClass::ResolvedModule,
        )],
        "gen-current",
        "root:a",
        "env:a",
        "sha256:current",
    );
    let stale = detect_moose_class(&stale_input);
    assert_eq!(
        stale.outcome,
        DetectionOutcome::Unavailable { reason: UnavailableReason::MissingGeneration }
    );
    assert_eq!(
        stale.validate_authority_against(&stale_input),
        Err(DetectionAuthorityError::InvalidModuleEvidence)
    );
}

#[test]
fn import_removal_and_readdition_change_source_and_detection_identity() {
    let initial_sites = sites("package App;\nuse Moose;\n", 1, "gen-1");
    assert_eq!(initial_sites.len(), 1);
    let initial_input = input(
        MooseActivationKind::Class,
        vec![matched(
            MooseActivationKind::Class,
            Some("2.4000"),
            "gen-1",
            DetectionEvidenceClass::ResolvedImport,
        )],
        "gen-1",
        "root:a",
        "env:a",
        "sha256:with-import",
    );
    let initial = detect_moose_class(&initial_input);
    assert!(initial.is_authoritative_against(&initial_input));

    let removed_sites = sites("package App;\n1;\n", 1, "gen-2");
    assert!(removed_sites.is_empty());
    let removed_input = input(
        MooseActivationKind::Class,
        vec![ModuleSelectorEvaluation::absent("Moose")],
        "gen-2",
        "root:a",
        "env:a",
        "sha256:without-import",
    );
    assert_eq!(
        initial.validate_authority_against(&removed_input),
        Err(DetectionAuthorityError::GenerationMismatch)
    );

    let readded_sites = sites("package App;\nuse Moose;\n", 1, "gen-3");
    let readded = must_some(readded_sites.first());
    assert_eq!(&readded.anchor.source_generation, &SourceGeneration::known("gen-3"));
    assert_ne!(&initial_sites[0].anchor.source_generation, &readded.anchor.source_generation);
}

#[test]
fn same_package_in_two_roots_remains_isolated_by_checked_input_identity() {
    let code = "package Shared;\nuse Moose;\n";
    let root_a_sites = sites(code, 1, "gen-a");
    let root_b_sites = sites(code, 2, "gen-b");
    assert_eq!(
        root_a_sites[0].anchor.package.as_deref(),
        root_b_sites[0].anchor.package.as_deref()
    );
    assert_ne!(root_a_sites[0].file_id, root_b_sites[0].file_id);

    let root_a_input = input(
        MooseActivationKind::Class,
        vec![matched(
            MooseActivationKind::Class,
            Some("2.4000"),
            "gen-a",
            DetectionEvidenceClass::ResolvedImport,
        )],
        "gen-a",
        "root:a",
        "env:a",
        "sha256:root-a",
    );
    let root_b_input = input(
        MooseActivationKind::Class,
        vec![matched(
            MooseActivationKind::Class,
            Some("2.4000"),
            "gen-b",
            DetectionEvidenceClass::ResolvedImport,
        )],
        "gen-b",
        "root:b",
        "env:b",
        "sha256:root-b",
    );
    let root_a = detect_moose_class(&root_a_input);
    let root_b = detect_moose_class(&root_b_input);
    assert!(root_a.is_authoritative_against(&root_a_input));
    assert!(root_b.is_authoritative_against(&root_b_input));
    assert_ne!(root_a.input_identity, root_b.input_identity);
}

#[test]
fn unmodeled_import_and_dynamic_wrapper_cannot_establish_exact_activation() {
    let unmodeled = sites("package App;\nuse Moose -traits => 'My::Trait';\n", 1, "gen-1");
    let site = must_some(unmodeled.first());
    assert!(!site.is_exact());
    assert!(matches!(&site.import_disposition, MooseImportDisposition::Unmodeled { .. }));

    let dynamic =
        sites("package Other;\nBEGIN { require Moose; Moose->import($options); }\n", 2, "gen-1");
    assert!(dynamic.is_empty());
}

#[test]
fn empty_import_list_is_not_activation() {
    let found = sites("package App;\nuse Moose ();\n", 1, "gen-1");
    let site = must_some(found.first());
    assert!(!site.is_exact());
    assert!(matches!(
        &site.import_disposition,
        MooseImportDisposition::Unmodeled { arguments }
            if arguments == &["(".to_string(), ")".to_string()]
    ));
}

#[test]
fn package_shape_and_foreign_descriptor_cannot_swap_class_and_role() {
    let shaped = sites(
        "package RoleShaped;\nuse Moose;\nrequires 'work';\n\
         package ClassShaped;\nuse Moose::Role;\nsub new { bless {}, shift }\n",
        1,
        "gen-1",
    );
    assert_eq!(shaped[0].kind, MooseActivationKind::Class);
    assert_eq!(shaped[1].kind, MooseActivationKind::Role);

    let class_input = input(
        MooseActivationKind::Class,
        vec![matched(
            MooseActivationKind::Class,
            Some("2.4000"),
            "gen-1",
            DetectionEvidenceClass::ResolvedModule,
        )],
        "gen-1",
        "root:a",
        "env:a",
        "sha256:class",
    );
    assert!(matches!(
        detect_moose_role(&class_input).outcome,
        DetectionOutcome::Unsupported { .. }
    ));
}

#[test]
fn cancelled_and_broken_instrumentation_are_terminal_non_authoritative_states() {
    let mut cancelled_input = input(
        MooseActivationKind::Class,
        vec![matched(
            MooseActivationKind::Class,
            Some("2.4000"),
            "gen-1",
            DetectionEvidenceClass::ResolvedModule,
        )],
        "gen-1",
        "root:a",
        "env:a",
        "sha256:cancelled",
    );
    cancelled_input.cancellation = AdapterCancellation::cancelled();
    assert_eq!(detect_moose_class(&cancelled_input).outcome, DetectionOutcome::Cancelled);

    let broken_input = input(
        MooseActivationKind::Class,
        vec![matched(
            MooseActivationKind::Class,
            Some("2.4000"),
            "gen-1",
            DetectionEvidenceClass::ResolvedModule,
        )],
        "gen-1",
        "",
        "env:a",
        "sha256:broken",
    );
    assert_eq!(
        detect_moose_class(&broken_input).outcome,
        DetectionOutcome::Unavailable { reason: UnavailableReason::InternalError }
    );
}
