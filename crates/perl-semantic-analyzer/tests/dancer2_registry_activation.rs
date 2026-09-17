#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! End-to-end registry-backed Dancer2 activation proof (#8914).
//!
//! Drives the #6820 checked SDK surface through the Dancer2 adapter in
//! `perl-semantic-facts::framework_adapters::dancer2` and the AST activation
//! extractor in `perl_semantic_analyzer::analysis::dancer2_activation`,
//! including the `test_corpus/real_projects/dancer2_skeleton` fixture.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::dancer2_activation::extract_dancer2_activation_sites;
use perl_semantic_facts::framework::{
    AdapterCancellation, AdapterDetectionInput, DetectionOutcome, ModuleObservationReceipt,
    ModuleSelectorEvaluation, ModuleSelectorOutcome,
};
use perl_semantic_facts::framework::{
    DetectionAbsenceReason, DetectionEvidenceClass, ModuleActivationIdentity, ModuleVersionEvidence,
};
use perl_semantic_facts::framework_adapters::dancer2::{
    DANCER2_VERSION_CONSTRAINT, Dancer2ActivationState, Dancer2KeywordState, DslKeywordScope,
    DslSelection, dancer2_activation_facts, dancer2_descriptor, detect_dancer2,
    parse_dancer2_import_args,
};
use perl_semantic_facts::{Confidence, FileId, SourceGeneration};
use perl_tdd_support::{must, must_some};

fn observation(
    evaluations: Vec<ModuleSelectorEvaluation>,
    generation: &str,
) -> ModuleObservationReceipt {
    ModuleObservationReceipt::new(
        "module-resolver.v1",
        "root:fixture",
        "project-environment.v1",
        SourceGeneration::known(generation),
        "sha256:fixture-input",
        evaluations,
    )
}

fn matched_dancer2(
    version: Option<&str>,
    generation: &str,
    evidence: DetectionEvidenceClass,
) -> ModuleSelectorEvaluation {
    let activation = ModuleActivationIdentity::new(
        "Dancer2",
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
        "Dancer2",
        ModuleSelectorOutcome::Matched { activation, evidence_class: evidence },
    )
}

fn input(evaluations: Vec<ModuleSelectorEvaluation>, generation: &str) -> AdapterDetectionInput {
    AdapterDetectionInput::new(
        dancer2_descriptor(),
        observation(evaluations, generation),
        None,
        AdapterCancellation::active(),
    )
}

// Falsifier 1: a Dancer2 app file activates via the registry exactly once,
// with no double activation alongside the contained legacy predicate.
#[test]
fn dancer2_app_file_activates_via_registry_exactly_once() {
    let code = "package MyApp;\nuse Dancer2;\nuse Dancer2::Core;\nget '/x' => sub { 1 };\n";
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let sites = extract_dancer2_activation_sites(&ast, FileId(1));
    assert_eq!(sites.len(), 1, "one activation site: `Dancer2::Core` is not one");

    let detection_input = input(
        vec![
            matched_dancer2(Some("1.1.1"), "gen-1", DetectionEvidenceClass::ResolvedModule),
            ModuleSelectorEvaluation::new(
                "Dancer2::Core",
                ModuleSelectorOutcome::Matched {
                    activation: ModuleActivationIdentity::new(
                        "Dancer2::Core",
                        Some(FileId(8)),
                        SourceGeneration::known("gen-1"),
                    ),
                    evidence_class: DetectionEvidenceClass::ResolvedModule,
                },
            ),
        ],
        "gen-1",
    );
    let detection = detect_dancer2(&detection_input);
    assert_eq!(
        detection.outcome,
        DetectionOutcome::Detected {
            confidence: Confidence::High,
            framework_version: Some("1.1.1".to_string())
        },
        "resolved supported Dancer2 identity activates exactly once"
    );

    let facts =
        dancer2_activation_facts(&detection, sites[0].package.as_deref(), &sites[0].evidence);
    assert!(facts.is_exact());
    assert!(
        matches!(&facts.state,
            Dancer2ActivationState::Exact { application_name, framework_version, .. }
            if application_name == "MyApp" && framework_version == "1.1.1"),
        "expected exact activation with caller-package identity, got {:?}",
        facts.state
    );
}

// Falsifier 2: Dancer2::Core modules never activate the registry adapter
// (the #8910 containment preserved at the registry seam).
#[test]
fn dancer2_core_modules_never_activate_the_registry_adapter() {
    let only_core = input(
        vec![ModuleSelectorEvaluation::new("Dancer2", ModuleSelectorOutcome::Absent)],
        "gen-1",
    );
    let detection = detect_dancer2(&only_core);
    assert_eq!(
        detection.outcome,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::RequiredModulesMissing }
    );
    let facts = dancer2_activation_facts(&detection, Some("App"), &parse_dancer2_import_args(&[]));
    assert!(!facts.is_exact());
    assert!(facts.keywords.is_empty(), "no keyword facts without activation");
}

// Falsifier 3: a non-Dancer2 file with similar imports does not activate.
#[test]
fn non_dancer2_files_do_not_activate() {
    let code = "package App;\nuse Dancer;\nuse Mojolicious::Lite;\nuse Dancer2::Plugin qw(hook);\n";
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    assert!(
        extract_dancer2_activation_sites(&ast, FileId(1)).is_empty(),
        "Dancer v1, Mojolicious::Lite, and Dancer2::Plugin are not Dancer2 activation"
    );

    let detection = detect_dancer2(&input(
        vec![ModuleSelectorEvaluation::new("Dancer2", ModuleSelectorOutcome::Absent)],
        "gen-1",
    ));
    assert!(matches!(detection.outcome, DetectionOutcome::Absent { .. }));
}

// Falsifier 4: the dancer2_skeleton corpus fixture drives an end-to-end
// activation fact (version evidence from the fixture, keyword facts from the
// reviewed DSL contract).
#[test]
fn dancer2_skeleton_corpus_drives_end_to_end_activation() {
    let skeleton_dancer2_pm = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test_corpus/real_projects/dancer2_skeleton/lib/Dancer2.pm"
    );
    let module_source = must(std::fs::read_to_string(skeleton_dancer2_pm));
    let version = must_some(extract_version(&module_source));
    assert!(
        perl_semantic_facts::framework::version_constraint_matches(
            DANCER2_VERSION_CONSTRAINT,
            &version
        ) == Some(true),
        "fixture version {version} must satisfy the reviewed constraint"
    );

    let app_source = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test_corpus/real_projects/dancer2_skeleton/t/basic.t"
    );
    let app_code = must(std::fs::read_to_string(app_source));

    let mut parser = Parser::new(&app_code);
    let ast = must(parser.parse());
    let sites = extract_dancer2_activation_sites(&ast, FileId(11));
    let site = must_some(sites.first().cloned());
    assert_eq!(site.package.as_deref(), Some("MyApp"));

    let detection_input = input(
        vec![matched_dancer2(
            Some(&version),
            "corpus-gen-1",
            DetectionEvidenceClass::ResolvedModule,
        )],
        "corpus-gen-1",
    );
    let detection = detect_dancer2(&detection_input);
    assert!(detection.is_detected());

    let facts = dancer2_activation_facts(&detection, site.package.as_deref(), &site.evidence);
    assert!(facts.is_exact());
    let get_fact = must_some(facts.keywords.iter().find(|fact| fact.keyword == "get").cloned());
    assert_eq!(get_fact.state, Dancer2KeywordState::Imported);
    assert_eq!(get_fact.scope, DslKeywordScope::Global);
    let request_fact =
        must_some(facts.keywords.iter().find(|fact| fact.keyword == "request").cloned());
    assert_eq!(request_fact.scope, DslKeywordScope::RouteHandlerOnly);
}

fn extract_version(source: &str) -> Option<String> {
    let marker = "our $VERSION = '";
    let start = source.find(marker)? + marker.len();
    let end = source[start..].find('\'')? + start;
    Some(source[start..end].to_string())
}

// Negative control: name-only Dancer2 identity is not exact activation.
#[test]
fn name_only_dancer2_identity_is_not_exact_activation() {
    let detection = detect_dancer2(&input(
        vec![matched_dancer2(Some("1.1.1"), "gen-1", DetectionEvidenceClass::NameOnly)],
        "gen-1",
    ));
    assert!(
        matches!(detection.outcome, DetectionOutcome::Unsupported { .. }),
        "a module merely named Dancer2 must not produce exact framework facts"
    );
    let facts = dancer2_activation_facts(&detection, Some("App"), &parse_dancer2_import_args(&[]));
    assert!(!facts.is_exact());
}

// Negative control: unsupported framework version.
#[test]
fn unsupported_version_does_not_activate() {
    let detection = detect_dancer2(&input(
        vec![matched_dancer2(Some("2.0.0"), "gen-1", DetectionEvidenceClass::ResolvedModule)],
        "gen-1",
    ));
    assert_eq!(
        detection.outcome,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::VersionConstraintNotSatisfied }
    );
}

// Negative control: missing version evidence stays explicitly unsupported.
#[test]
fn missing_version_evidence_is_not_exact() {
    let detection = detect_dancer2(&input(
        vec![matched_dancer2(None, "gen-1", DetectionEvidenceClass::ResolvedModule)],
        "gen-1",
    ));
    assert!(matches!(detection.outcome, DetectionOutcome::Unsupported { .. }));
}

// `!get` excludes only `get` and does not disable unrelated keywords.
#[test]
fn exclusion_is_keyword_scoped() {
    let args: Vec<String> = ["'!get'"].iter().map(ToString::to_string).collect();
    let evidence = parse_dancer2_import_args(&args);
    let detection = detect_dancer2(&input(
        vec![matched_dancer2(Some("1.1.1"), "gen-1", DetectionEvidenceClass::ResolvedModule)],
        "gen-1",
    ));
    let facts = dancer2_activation_facts(&detection, Some("App"), &evidence);
    assert!(facts.is_exact());
    for fact in &facts.keywords {
        let expected = if fact.keyword == "get" {
            Dancer2KeywordState::Excluded
        } else {
            Dancer2KeywordState::Imported
        };
        assert_eq!(fact.state, expected, "keyword `{}`", fact.keyword);
    }
}

// Literal appname changes application identity; computed appname is bounded.
#[test]
fn appname_forms_are_represented() {
    let detection = detect_dancer2(&input(
        vec![matched_dancer2(Some("1.1.1"), "gen-1", DetectionEvidenceClass::ResolvedModule)],
        "gen-1",
    ));

    let literal: Vec<String> =
        ["appname", "=>", "'Named'"].iter().map(ToString::to_string).collect();
    let facts =
        dancer2_activation_facts(&detection, Some("App"), &parse_dancer2_import_args(&literal));
    assert!(
        matches!(&facts.state, Dancer2ActivationState::Exact { application_name, .. } if application_name == "Named"),
        "literal appname must stay exact, got {:?}",
        facts.state
    );

    let computed: Vec<String> = ["appname", "=>", "$app"].iter().map(ToString::to_string).collect();
    let facts =
        dancer2_activation_facts(&detection, Some("App"), &parse_dancer2_import_args(&computed));
    assert!(
        matches!(facts.state, Dancer2ActivationState::DynamicBoundary { .. }),
        "computed appname must be an explicit dynamic boundary"
    );
}

// Custom/dynamic DSL selection cannot inherit default-Dancer2 exact keyword
// facts without authority.
#[test]
fn custom_dsl_does_not_inherit_default_keyword_facts() {
    let detection = detect_dancer2(&input(
        vec![matched_dancer2(Some("1.1.1"), "gen-1", DetectionEvidenceClass::ResolvedModule)],
        "gen-1",
    ));
    for dsl in [
        DslSelection::CustomLiteral("My::DSL".to_string()),
        DslSelection::Dynamic { reason: "computed".to_string() },
    ] {
        let mut evidence = parse_dancer2_import_args(&[]);
        evidence.dsl = Some(dsl.clone());
        let facts = dancer2_activation_facts(&detection, Some("App"), &evidence);
        assert!(
            matches!(facts.state, Dancer2ActivationState::DynamicBoundary { .. }),
            "{dsl:?} must not inherit default-Dancer2 exact keyword facts"
        );
        assert!(facts.keywords.is_empty());
    }
    assert!(evidence_default_dsl_keeps_keywords(&detection));
}

fn evidence_default_dsl_keeps_keywords(
    detection: &perl_semantic_facts::framework::AdapterDetectionResult,
) -> bool {
    let facts = dancer2_activation_facts(detection, Some("App"), &parse_dancer2_import_args(&[]));
    facts.is_exact() && !facts.keywords.is_empty()
}

// Edit/refresh: a new source generation produces generation-aware facts; the
// stale activation cannot survive the import's removal.
#[test]
fn generation_refresh_removes_stale_activation() {
    let gen1_input = input(
        vec![matched_dancer2(Some("1.1.1"), "gen-1", DetectionEvidenceClass::ResolvedModule)],
        "gen-1",
    );
    let gen1 = detect_dancer2(&gen1_input);
    let facts1 = dancer2_activation_facts(&gen1, Some("App"), &parse_dancer2_import_args(&[]));
    assert!(
        matches!(&facts1.state, Dancer2ActivationState::Exact { source_generation, .. }
            if source_generation == &SourceGeneration::known("gen-1")),
        "expected exact activation at gen-1, got {:?}",
        facts1.state
    );

    // Import removed: no activation site exists, and the fresh generation's
    // detection cannot attach facts to a package that no longer imports.
    let code = "package App;\nget '/x' => sub { 1 };\n";
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    assert!(extract_dancer2_activation_sites(&ast, FileId(1)).is_empty());

    let gen2_input = input(
        vec![ModuleSelectorEvaluation::new("Dancer2", ModuleSelectorOutcome::Absent)],
        "gen-2",
    );
    let gen2 = detect_dancer2(&gen2_input);
    assert!(matches!(gen2.outcome, DetectionOutcome::Absent { .. }));
    let facts2 = dancer2_activation_facts(&gen2, Some("App"), &parse_dancer2_import_args(&[]));
    assert!(!facts2.is_exact());
    assert!(facts2.keywords.is_empty(), "stale keyword facts must not survive");
}

// Two roots with the same package/app name remain isolated (distinct input
// identities scope the activations).
#[test]
fn two_roots_with_same_package_stay_isolated() {
    let root_a = input(
        vec![matched_dancer2(Some("1.1.1"), "gen-a", DetectionEvidenceClass::ResolvedModule)],
        "gen-a",
    );
    let root_b = input(
        vec![matched_dancer2(Some("1.1.1"), "gen-b", DetectionEvidenceClass::ResolvedModule)],
        "gen-b",
    );
    let detection_a = detect_dancer2(&root_a);
    let detection_b = detect_dancer2(&root_b);
    let facts_a =
        dancer2_activation_facts(&detection_a, Some("App"), &parse_dancer2_import_args(&[]));
    let facts_b =
        dancer2_activation_facts(&detection_b, Some("App"), &parse_dancer2_import_args(&[]));
    assert!(facts_a.is_exact() && facts_b.is_exact());
    assert!(
        matches!((&facts_a.state, &facts_b.state),
            (
                Dancer2ActivationState::Exact { source_generation: gen_a, .. },
                Dancer2ActivationState::Exact { source_generation: gen_b, .. },
            ) if gen_a != gen_b),
        "activations are root/generation scoped, got ({:?}, {:?})",
        facts_a.state,
        facts_b.state
    );
}

// Shadow honesty: the adapter's output cannot become publication authority.
#[test]
fn shadow_adapter_output_cannot_become_authority() {
    use perl_semantic_facts::framework::{
        AdapterAuthorityError, AdapterInput, AdapterSourceScope, FactClass,
    };
    let detection = detect_dancer2(&input(
        vec![matched_dancer2(Some("1.1.1"), "gen-1", DetectionEvidenceClass::ResolvedModule)],
        "gen-1",
    ));
    let scope = AdapterSourceScope::new(
        FileId(3),
        SourceGeneration::known("gen-1"),
        None,
        None,
        Some("App".to_string()),
    );
    let adapter_input = AdapterInput::new(
        dancer2_descriptor(),
        scope,
        vec![FactClass::FrameworkImports],
        Vec::new(),
        None,
        AdapterCancellation::active(),
    );
    let result = perl_semantic_facts::framework::AdapterResult::new(
        dancer2_descriptor(),
        adapter_input.source_scope.clone(),
        SourceGeneration::known("gen-1"),
        perl_semantic_facts::framework::AdapterOutcome::Applied {
            sink: perl_semantic_facts::framework::FactSink::new(
                perl_semantic_facts::framework::FactSinkId(1),
                dancer2_descriptor().adapter_id,
            ),
            limitations: Vec::new(),
        },
    );
    assert_eq!(
        result.validate_authority_against(&adapter_input),
        Err(AdapterAuthorityError::NonProduction),
        "no live provider is promoted by this shadow adapter"
    );
    assert!(detection.is_detected());
}
