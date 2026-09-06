//! End-to-end Dancer2 2.x activation and DSL-registry proof (#13616,
//! leaves L1 + L2).
//!
//! Drives real Perl source through the whole chain — `source -> parse ->
//! extraction -> detection -> facts` — for the bounded 2.x profile: the
//! pinned import semantics (no-op tags, `:nopragmas`, `!keyword`
//! exclusions, the odd-argument compile-time die, suppressed empty import
//! lists, appname/DSL selections) and the corrected 82-keyword core-DSL
//! registry (scope split, prototypes, runtime-croak deprecations,
//! un-overwrite shadowing).
//!
//! Boundary: all version evidence comes from resolved module identities in
//! the detection input; no runtime Perl with Dancer2 participates, and the
//! runtime differential oracle stays a separate hermetic conformance
//! surface (NOT_PROVEN locally).

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::dancer2_two_x_activation::extract_dancer2_two_x_activation_sites;
use perl_semantic_facts::framework::{
    AdapterCancellation, AdapterDetectionInput, DetectionAbsenceReason, DetectionEvidenceClass,
    DetectionOutcome, ModuleActivationIdentity, ModuleObservationReceipt, ModuleSelectorEvaluation,
    ModuleSelectorOutcome, ModuleVersionEvidence,
};
use perl_semantic_facts::framework_adapters::dancer2::{DslKeywordScope, DslSelection};
use perl_semantic_facts::framework_adapters::dancer2_two_x::{
    DANCER2_TWO_X_DSL_CONTRACT_VERSION, DANCER2_TWO_X_IMPORT_DIE_MESSAGE,
    DANCER2_TWO_X_KEYWORD_TOTAL, Dancer2TwoXActivationState, Dancer2TwoXKeywordState,
    dancer2_two_x_activation_facts, dancer2_two_x_descriptor, detect_dancer2_two_x,
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
        dancer2_two_x_descriptor(),
        observation(evaluations, generation),
        None,
        AdapterCancellation::active(),
    )
}

fn detected_two_x(version: &str) -> perl_semantic_facts::framework::AdapterDetectionResult {
    detect_dancer2_two_x(&input(
        vec![matched_dancer2(Some(version), "gen-1", DetectionEvidenceClass::ResolvedModule)],
        "gen-1",
    ))
}

fn sites(
    code: &str,
) -> Vec<perl_semantic_analyzer::analysis::dancer2_two_x_activation::Dancer2TwoXActivationSite> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    extract_dancer2_two_x_activation_sites(&ast, code, FileId(1), SourceGeneration::known("gen-1"))
}

fn facts_for_code(
    code: &str,
    detection: &perl_semantic_facts::framework::AdapterDetectionResult,
) -> perl_semantic_facts::framework_adapters::dancer2_two_x::Dancer2TwoXActivationFacts {
    let found = sites(code);
    assert!(!found.is_empty(), "the source must carry an activation site");
    let site = must_some(found.first().cloned());
    dancer2_two_x_activation_facts(
        detection,
        site.package.as_deref(),
        &site.evidence,
        &site.shadowed_keywords,
    )
}

// L1: a resolved 2.x identity over a bare `use Dancer2;` activates exactly
// once and carries the full pinned keyword contract.
#[test]
fn bare_import_activates_exactly_with_the_pinned_contract() {
    let code = "package MyApp;\nuse Dancer2;\nget '/' => sub { 'Hello' };\n";
    let detection = detected_two_x("2.0.1");
    assert_eq!(
        detection.outcome,
        DetectionOutcome::Detected {
            confidence: Confidence::High,
            framework_version: Some("2.0.1".to_string()),
        },
        "resolved supported 2.x identity activates"
    );
    let facts = facts_for_code(code, &detection);
    assert!(facts.is_exact());
    assert!(
        matches!(&facts.state,
            Dancer2TwoXActivationState::Exact { application_name, framework_version, .. }
            if application_name == "MyApp" && framework_version == "2.0.1"),
        "expected exact activation with caller-package identity, got {:?}",
        facts.state
    );
    assert_eq!(facts.dsl_contract_version, DANCER2_TWO_X_DSL_CONTRACT_VERSION);
    assert_eq!(facts.keywords.len(), DANCER2_TWO_X_KEYWORD_TOTAL);
}

// L1: literal `appname` names the application; computed appname is a dynamic
// boundary.
#[test]
fn appname_forms_are_represented() {
    let detection = detected_two_x("2.0.1");

    let facts = facts_for_code("package App;\nuse Dancer2 appname => 'Named2x';\n", &detection);
    assert!(
        matches!(&facts.state,
            Dancer2TwoXActivationState::Exact { application_name, .. } if application_name == "Named2x"),
        "literal appname must stay exact, got {:?}",
        facts.state
    );

    let facts = facts_for_code("package App;\nuse Dancer2 appname => $app;\n", &detection);
    assert!(
        matches!(facts.state, Dancer2TwoXActivationState::DynamicBoundary { .. }),
        "computed appname must be an explicit dynamic boundary"
    );
}

// L1: `!params` excludes exactly one keyword and mints nothing for it.
#[test]
fn exclusion_is_keyword_scoped_end_to_end() {
    let code = "package App;\nuse Dancer2 '!params';\n";
    let facts = facts_for_code(code, &detected_two_x("2.0.1"));
    assert!(facts.is_exact());
    for fact in &facts.keywords {
        let expected = if fact.keyword == "params" {
            Dancer2TwoXKeywordState::Excluded
        } else {
            Dancer2TwoXKeywordState::Imported
        };
        assert_eq!(fact.state, expected, "keyword `{}`", fact.keyword);
    }
    assert!(facts.unknown_exclusions.is_empty());
}

// L1: an unknown exclusion is recorded, never silently dropped.
#[test]
fn unknown_exclusions_are_recorded() {
    let code = "package App;\nuse Dancer2 '!nonexistent';\n";
    let facts = facts_for_code(code, &detected_two_x("2.0.1"));
    assert!(facts.is_exact());
    assert_eq!(facts.unknown_exclusions, vec!["nonexistent".to_string()]);
}

// L1: `:script`/`:syntax`/`:tests` are silent no-ops; `:nopragmas` sets the
// pragma boundary.
#[test]
fn noop_tags_and_nopragmas_carry_on_exact_facts() {
    let detection = detected_two_x("2.0.1");
    let facts =
        facts_for_code("package App;\nuse Dancer2 ':script' ':syntax' ':tests';\n", &detection);
    assert!(facts.is_exact());
    assert_eq!(facts.no_op_tags, vec![":script", ":syntax", ":tests"]);
    assert!(!facts.nopragmas);

    let facts = facts_for_code("package App;\nuse Dancer2 ':nopragmas';\n", &detection);
    assert!(facts.is_exact());
    assert!(facts.nopragmas, ":nopragmas must reach the facts");
}

// L1: the pinned odd-argument die dominates: no app, no keywords.
#[test]
fn odd_argument_import_dies_with_the_upstream_message() {
    let code = "package App;\nuse Dancer2 'appname';\n";
    let facts = facts_for_code(code, &detected_two_x("2.0.1"));
    assert!(
        matches!(&facts.state,
            Dancer2TwoXActivationState::ImportDied { die_message }
            if die_message == DANCER2_TWO_X_IMPORT_DIE_MESSAGE),
        "expected the exact upstream die, got {:?}",
        facts.state
    );
    assert!(facts.keywords.is_empty(), "a dead import mints no keyword facts");
}

// L1: an explicit empty import list calls no import at all.
#[test]
fn explicit_empty_import_activates_nothing() {
    let code = "package App;\nuse Dancer2 ();\n";
    let facts = facts_for_code(code, &detected_two_x("2.0.1"));
    assert!(!facts.is_exact());
    assert!(matches!(facts.state, Dancer2TwoXActivationState::NotActivated { .. }));
    assert!(facts.keywords.is_empty());
}

// Version gating: 1.x identities fail the 2.x constraint explicitly; the
// whole pinned range activates; the upper bound forces re-review.
#[test]
fn version_gating_bounds_the_profile() {
    let one_x = detect_dancer2_two_x(&input(
        vec![matched_dancer2(Some("1.1.1"), "gen-1", DetectionEvidenceClass::ResolvedModule)],
        "gen-1",
    ));
    assert_eq!(
        one_x.outcome,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::VersionConstraintNotSatisfied }
    );
    for version in ["2.0.0", "2.0.1", "2.1.0"] {
        assert!(detected_two_x(version).is_detected(), "{version} must satisfy the constraint");
    }
    let two_two = detect_dancer2_two_x(&input(
        vec![matched_dancer2(Some("2.2.0"), "gen-1", DetectionEvidenceClass::ResolvedModule)],
        "gen-1",
    ));
    assert!(matches!(two_two.outcome, DetectionOutcome::Absent { .. }), "2.2.0 is unreviewed");
}

// Negative control: name-only identity is not exact activation.
#[test]
fn name_only_identity_is_not_exact() {
    let detection = detect_dancer2_two_x(&input(
        vec![matched_dancer2(Some("2.0.1"), "gen-1", DetectionEvidenceClass::NameOnly)],
        "gen-1",
    ));
    assert!(matches!(detection.outcome, DetectionOutcome::Unsupported { .. }));
    let facts = facts_for_code("package App;\nuse Dancer2;\n", &detection);
    assert!(!facts.is_exact());
    assert!(facts.keywords.is_empty());
}

// Negative control: `Dancer2::Core` never activates (the #8910 containment).
#[test]
fn dancer2_core_modules_never_activate() {
    let code = "package App;\nuse Dancer2::Core;\nuse Dancer2::Core::App;\n";
    assert!(sites(code).is_empty(), "Dancer2::Core selectors are not Dancer2 activation");
    let detection = detect_dancer2_two_x(&input(
        vec![ModuleSelectorEvaluation::new("Dancer2", ModuleSelectorOutcome::Absent)],
        "gen-1",
    ));
    assert!(matches!(detection.outcome, DetectionOutcome::Absent { .. }));
}

// Negative control: plugin keyword surfaces mint nothing as core DSL facts.
#[test]
fn plugin_keyword_surfaces_mint_nothing() {
    let code = "package App;\nuse Dancer2;\nuse Dancer2::Plugin 'session';\n";
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let found = extract_dancer2_two_x_activation_sites(&ast, code, FileId(1), SourceGeneration::known("gen-1"));
    assert_eq!(found.len(), 1, "only the exact `use Dancer2` activates");
    let facts = dancer2_two_x_activation_facts(
        &detected_two_x("2.0.1"),
        found[0].package.as_deref(),
        &found[0].evidence,
        &found[0].shadowed_keywords,
    );
    assert!(facts.is_exact());
    // `session` is a core keyword, but the point is the plugin import: it is
    // not an activation site and contributes no facts of its own.
    assert!(facts.keywords.len() == DANCER2_TWO_X_KEYWORD_TOTAL);
}

// L2 repo correction: `cookie`/`redirect` are route-handler-only; the
// non-upstream 1.x table entries are absent.
#[test]
fn registry_carries_the_repo_correction_end_to_end() {
    let code = "package App;\nuse Dancer2;\n";
    let facts = facts_for_code(code, &detected_two_x("2.0.1"));
    let cookie = facts.keywords.iter().find(|fact| fact.keyword == "cookie");
    let redirect = facts.keywords.iter().find(|fact| fact.keyword == "redirect");
    assert!(
        matches!(cookie, Some(fact) if fact.scope == DslKeywordScope::RouteHandlerOnly),
        "cookie must be route-handler-only in the 2.x registry"
    );
    assert!(
        matches!(redirect, Some(fact) if fact.scope == DslKeywordScope::RouteHandlerOnly),
        "redirect must be route-handler-only in the 2.x registry"
    );
    for absent in ["route", "before", "after", "body"] {
        assert!(
            !facts.keywords.iter().any(|fact| fact.keyword == absent),
            "`{absent}` is not an upstream DSL keyword"
        );
    }
}

// L2: prototypes and deprecations travel on the keyword facts.
#[test]
fn prototypes_and_deprecations_travel_on_keyword_facts() {
    let facts = facts_for_code("package App;\nuse Dancer2;\n", &detected_two_x("2.1.0"));
    let delayed = facts.keywords.iter().find(|fact| fact.keyword == "delayed");
    assert!(
        matches!(delayed, Some(fact)
            if fact.prototype == Some("&@") && fact.scope == DslKeywordScope::RouteHandlerOnly),
        "delayed carries the (&@) prototype and route-handler scope"
    );
    let prepare_app = facts.keywords.iter().find(|fact| fact.keyword == "prepare_app");
    assert!(
        matches!(prepare_app, Some(fact)
            if fact.prototype == Some("&") && fact.scope == DslKeywordScope::Global),
        "prepare_app carries the (&) prototype and global scope"
    );
    let header = facts.keywords.iter().find(|fact| fact.keyword == "header");
    assert!(
        matches!(header, Some(fact)
            if fact.deprecation_replacement == Some("response_header")
                && fact.state == Dancer2TwoXKeywordState::Imported),
        "deprecated keywords are still imported and carry their replacement"
    );
}

// L2: the upstream un-overwrite rule mints no DSL binding for a name a
// same-package named sub already owns.
#[test]
fn shadowed_keywords_never_mint_a_binding() {
    let code = "package App;\nsub template { 1 }\nuse Dancer2;\n";
    let facts = facts_for_code(code, &detected_two_x("2.0.1"));
    assert!(facts.is_exact(), "the import still succeeds upstream");
    let template = facts.keywords.iter().find(|fact| fact.keyword == "template");
    assert!(
        matches!(template, Some(fact) if fact.state == Dancer2TwoXKeywordState::Shadowed),
        "the pre-existing sub owns the name; no DSL binding is minted"
    );
    let get = facts.keywords.iter().find(|fact| fact.keyword == "get");
    assert!(matches!(get, Some(fact) if fact.state == Dancer2TwoXKeywordState::Imported));
}

// L1: custom and computed DSL selections are dynamic boundaries that inherit
// no default keyword facts.
#[test]
fn custom_and_computed_dsl_selections_inherit_nothing() {
    let detection = detected_two_x("2.0.1");
    let custom = facts_for_code("package App;\nuse Dancer2 dsl => 'My::DSL';\n", &detection);
    assert!(matches!(custom.state, Dancer2TwoXActivationState::DynamicBoundary { .. }));
    assert!(custom.keywords.is_empty());

    let computed = facts_for_code("package App;\nuse Dancer2 dsl => $which;\n", &detection);
    assert!(matches!(computed.state, Dancer2TwoXActivationState::DynamicBoundary { .. }));
    assert!(computed.keywords.is_empty());

    // `dsl_class` as an import argument has no import effect upstream but is
    // unmodeled here: recorded, never dropped, and refusal is fail-closed.
    let dsl_class =
        facts_for_code("package App;\nuse Dancer2 dsl_class => 'My::DSL';\n", &detection);
    assert!(matches!(dsl_class.state, Dancer2TwoXActivationState::DynamicBoundary { .. }));
    assert!(dsl_class.keywords.is_empty());
    let default = facts_for_code("package App;\nuse Dancer2;\n", &detection);
    assert!(matches!(default.dsl, DslSelection::Default));
}

// Two packages in one file are two applications; each scope keeps its own
// import evidence.
#[test]
fn two_package_apps_stay_scoped() {
    let code = "package First;\nuse Dancer2;\npackage Second;\nuse Dancer2 '!params';\n";
    let found = sites(code);
    assert_eq!(found.len(), 2);
    let detection = detected_two_x("2.0.1");
    let first = dancer2_two_x_activation_facts(
        &detection,
        found[0].package.as_deref(),
        &found[0].evidence,
        &found[0].shadowed_keywords,
    );
    let second = dancer2_two_x_activation_facts(
        &detection,
        found[1].package.as_deref(),
        &found[1].evidence,
        &found[1].shadowed_keywords,
    );
    assert!(first.is_exact() && second.is_exact());
    assert!(matches!(&first.state, Dancer2TwoXActivationState::Exact { application_name, .. }
            if application_name == "First"));
    assert!(matches!(&second.state, Dancer2TwoXActivationState::Exact { application_name, .. }
            if application_name == "Second"));
    let second_params = second.keywords.iter().find(|fact| fact.keyword == "params");
    assert!(matches!(second_params, Some(fact) if fact.state == Dancer2TwoXKeywordState::Excluded));
    let first_params = first.keywords.iter().find(|fact| fact.keyword == "params");
    assert!(matches!(first_params, Some(fact) if fact.state == Dancer2TwoXKeywordState::Imported));
}

// Edit/refresh: a stale generation's facts cannot survive the import's
// removal.
#[test]
fn generation_refresh_removes_stale_activation() {
    let facts1 = facts_for_code("package App;\nuse Dancer2;\n", &detected_two_x("2.0.1"));
    assert!(
        matches!(&facts1.state, Dancer2TwoXActivationState::Exact { source_generation, .. }
            if source_generation == &SourceGeneration::known("gen-1")),
        "expected exact activation at gen-1, got {:?}",
        facts1.state
    );

    let code = "package App;\nget '/x' => sub { 1 };\n";
    assert!(sites(code).is_empty(), "the import's removal leaves no activation site");
    let detection = detect_dancer2_two_x(&input(
        vec![ModuleSelectorEvaluation::new("Dancer2", ModuleSelectorOutcome::Absent)],
        "gen-2",
    ));
    assert!(matches!(detection.outcome, DetectionOutcome::Absent { .. }));
}

// The committed 2.x skeleton drives an end-to-end activation fact. It proves
// only activation/import and DSL-registry behavior; it is never cited as
// proof of 2.x config, template, serializer, or plugin behavior.
#[test]
fn dancer2_two_x_skeleton_drives_end_to_end_activation() {
    let skeleton_root = format!(
        "{}/../../test_corpus/real_projects/dancer2_2x_skeleton",
        env!("CARGO_MANIFEST_DIR")
    );
    let module_source = must(std::fs::read_to_string(format!("{skeleton_root}/lib/Dancer2.pm")));
    let version = must_some(extract_version(&module_source));
    assert!(
        perl_semantic_facts::framework::version_constraint_matches(">=2.0.0,<2.2.0", &version)
            == Some(true),
        "skeleton version {version} must satisfy the reviewed 2.x constraint"
    );

    let app_source = must(std::fs::read_to_string(format!("{skeleton_root}/t/basic.t")));
    let mut parser = Parser::new(&app_source);
    let ast = must(parser.parse());
    let found = extract_dancer2_two_x_activation_sites(&ast, &app_source, FileId(21), SourceGeneration::known("gen-1"));
    assert_eq!(found.len(), 2, "the skeleton carries two package apps");
    assert_eq!(found[0].package.as_deref(), Some("MyApp2x"));
    assert_eq!(found[1].package.as_deref(), Some("MyApp2xAPI"));

    let detection = detect_dancer2_two_x(&input(
        vec![matched_dancer2(
            Some(&version),
            "corpus-gen-1",
            DetectionEvidenceClass::ResolvedModule,
        )],
        "corpus-gen-1",
    ));
    assert!(detection.is_detected());

    let main_facts = dancer2_two_x_activation_facts(
        &detection,
        found[0].package.as_deref(),
        &found[0].evidence,
        &found[0].shadowed_keywords,
    );
    assert!(main_facts.is_exact());
    assert!(
        matches!(&main_facts.state,
            Dancer2TwoXActivationState::Exact { application_name, .. }
            if application_name == "MyApp2xNamed"),
        "the skeleton's literal appname must name the application, got {:?}",
        main_facts.state
    );
    assert_eq!(main_facts.no_op_tags, vec![":script"], "the skeleton's no-op tag is recorded");
    let prefix = main_facts.keywords.iter().find(|fact| fact.keyword == "prefix");
    assert!(
        matches!(prefix, Some(fact)
            if fact.state == Dancer2TwoXKeywordState::Imported
                && fact.scope == DslKeywordScope::Global),
        "prefix is a registered global keyword in the 2.x contract"
    );

    let api_facts = dancer2_two_x_activation_facts(
        &detection,
        found[1].package.as_deref(),
        &found[1].evidence,
        &found[1].shadowed_keywords,
    );
    assert!(api_facts.is_exact());
    let params = api_facts.keywords.iter().find(|fact| fact.keyword == "params");
    assert!(
        matches!(params, Some(fact) if fact.state == Dancer2TwoXKeywordState::Excluded),
        "the second app's !params exclusion is keyword-scoped"
    );
}

// Shadow honesty: the 2.x adapter's output cannot become publication
// authority.
#[test]
fn shadow_adapter_output_cannot_become_authority() {
    use perl_semantic_facts::framework::{
        AdapterAuthorityError, AdapterInput, AdapterOutcome, AdapterResult, AdapterSourceScope,
        FactClass, FactSink, FactSinkId,
    };
    let detection = detected_two_x("2.0.1");
    let scope = AdapterSourceScope::new(
        FileId(3),
        SourceGeneration::known("gen-1"),
        None,
        None,
        Some("App".to_string()),
    );
    let adapter_input = AdapterInput::new(
        dancer2_two_x_descriptor(),
        scope,
        vec![FactClass::FrameworkImports],
        Vec::new(),
        None,
        AdapterCancellation::active(),
    );
    let result = AdapterResult::new(
        dancer2_two_x_descriptor(),
        adapter_input.source_scope.clone(),
        SourceGeneration::known("gen-1"),
        AdapterOutcome::Applied {
            sink: FactSink::new(FactSinkId(1), dancer2_two_x_descriptor().adapter_id),
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

fn extract_version(source: &str) -> Option<String> {
    let marker = "our $VERSION = '";
    let start = source.find(marker)? + marker.len();
    let end = source[start..].find('\'')? + start;
    Some(source[start..end].to_string())
}
