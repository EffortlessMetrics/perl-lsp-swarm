//! End-to-end Mojolicious activation/ownership identity proof (#9688).
//!
//! Drives real Perl source through the whole chain for each admitted profile:
//!
//! ```text
//! source -> parse -> activation-site extraction -> registry detection -> facts -> role
//! ```
//!
//! The full-application and controller rows deliberately go through the
//! Mojo::Base adapter's own pipeline (#9681/#9682) and are only *classified*
//! by the Mojolicious profile, so this test also proves the consumption
//! boundary the issue requires: no second Mojo::Base recognizer exists.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::mojo_base_activation::extract_mojo_base_activation_sites;
use perl_semantic_analyzer::analysis::mojolicious_activation::extract_mojolicious_lite_activation_sites;
use perl_semantic_facts::framework::{
    AdapterCancellation, AdapterDetectionInput, DetectionEvidenceClass, ModuleActivationIdentity,
    ModuleObservationReceipt, ModuleSelectorEvaluation, ModuleSelectorOutcome,
    ModuleVersionEvidence,
};
use perl_semantic_facts::framework_adapters::mojo_base::{
    detect_mojo_base, mojo_base_activation_facts, mojo_base_descriptor,
};
use perl_semantic_facts::framework_adapters::mojolicious::{
    MOJOLICIOUS_LITE_MODULE, MojoliciousActivationFacts, MojoliciousActivationOutcome,
    MojoliciousRole, detect_mojolicious_lite, mojolicious_lite_activation_facts,
    mojolicious_lite_descriptor, mojolicious_role_facts_from_mojo_base,
};
use perl_semantic_facts::{FileId, SourceGeneration};
use perl_tdd_support::must;

const GENERATION: &str = "gen-1";
const DIST_VERSION: &str = "9.34";

fn evaluation(selector: &str, version: &str, generation: &str) -> ModuleSelectorEvaluation {
    let activation = ModuleActivationIdentity::new(
        selector,
        Some(FileId(3)),
        SourceGeneration::known(generation),
    )
    .with_observed_version(ModuleVersionEvidence::new(
        version,
        SourceGeneration::known(generation),
    ));
    ModuleSelectorEvaluation::new(
        selector,
        ModuleSelectorOutcome::Matched {
            activation,
            evidence_class: DetectionEvidenceClass::ResolvedModule,
        },
    )
}

fn receipt(selector: &str, version: &str, generation: &str) -> ModuleObservationReceipt {
    ModuleObservationReceipt::new(
        "module-resolver.v1",
        "root:fixture",
        "project-environment.v1",
        SourceGeneration::known(generation),
        "sha256:fixture-input",
        vec![evaluation(selector, version, generation)],
    )
}

/// Facts for every Lite activation site in `code`.
fn lite_facts(code: &str, generation: &str) -> Vec<MojoliciousActivationFacts> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let sites = extract_mojolicious_lite_activation_sites(
        &ast,
        FileId(1),
        SourceGeneration::known(generation),
    );
    let detection = detect_mojolicious_lite(&AdapterDetectionInput::new(
        mojolicious_lite_descriptor(),
        receipt(MOJOLICIOUS_LITE_MODULE, DIST_VERSION, generation),
        None,
        AdapterCancellation::active(),
    ));
    sites
        .iter()
        .map(|site| mojolicious_lite_activation_facts(&detection, &site.anchor, &site.evidence))
        .collect()
}

/// Facts for every `use Mojo::Base ...;` site in `code`, classified into its
/// Mojolicious role through the consumption seam.
fn derived_facts(code: &str, generation: &str) -> Vec<MojoliciousActivationFacts> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let sites = extract_mojo_base_activation_sites(
        &ast,
        code,
        FileId(1),
        SourceGeneration::known(generation),
    );
    let detection = detect_mojo_base(&AdapterDetectionInput::new(
        mojo_base_descriptor(),
        receipt("Mojo::Base", DIST_VERSION, generation),
        None,
        AdapterCancellation::active(),
    ));
    sites
        .iter()
        .map(|site| {
            let base = mojo_base_activation_facts(&detection, &site.anchor, &site.evidence);
            mojolicious_role_facts_from_mojo_base(&base)
        })
        .collect()
}

fn roles(facts: &[MojoliciousActivationFacts]) -> Vec<Option<MojoliciousRole>> {
    facts.iter().map(MojoliciousActivationFacts::role).collect()
}

// -------------------------------------------------------------------------
// The three admitted profiles
// -------------------------------------------------------------------------

#[test]
fn a_lite_script_owns_the_lite_application_role() {
    let facts = lite_facts(
        "use Mojolicious::Lite;\n\nget '/' => sub { shift->render(text => 'hi') };\n\napp->start;\n",
        GENERATION,
    );
    assert_eq!(roles(&facts), vec![Some(MojoliciousRole::LiteApplication)]);
    assert_eq!(facts[0].package.as_deref(), Some("main"));
    assert_eq!(facts[0].framework_version, DIST_VERSION);
}

#[test]
fn a_lite_signatures_script_owns_the_lite_application_role() {
    let facts = lite_facts(
        "use Mojolicious::Lite -signatures;\n\nget '/' => sub ($c) { $c->render(text => 'hi') };\n",
        GENERATION,
    );
    assert_eq!(roles(&facts), vec![Some(MojoliciousRole::LiteApplication)]);
    assert!(facts[0].signatures);
}

#[test]
fn a_full_application_class_owns_the_application_role() {
    let facts = derived_facts(
        "package MyApp;\nuse Mojo::Base 'Mojolicious';\n\nsub startup { my $self = shift; }\n\n1;\n",
        GENERATION,
    );
    assert_eq!(roles(&facts), vec![Some(MojoliciousRole::Application)]);
    assert_eq!(facts[0].package.as_deref(), Some("MyApp"));
    assert!(facts[0].parent_range.is_some(), "the parent spelling stays source-anchored");
}

#[test]
fn a_controller_class_owns_the_controller_role() {
    let facts = derived_facts(
        "package MyApp::Controller::Users;\nuse Mojo::Base 'Mojolicious::Controller';\n\nsub index { my $self = shift; }\n\n1;\n",
        GENERATION,
    );
    assert_eq!(roles(&facts), vec![Some(MojoliciousRole::Controller)]);
    assert_eq!(facts[0].package.as_deref(), Some("MyApp::Controller::Users"));
}

#[test]
fn the_signatures_option_reaches_a_full_application_class() {
    let facts =
        derived_facts("package MyApp;\nuse Mojo::Base 'Mojolicious', -signatures;\n", GENERATION);
    assert_eq!(roles(&facts), vec![Some(MojoliciousRole::Application)]);
    assert!(facts[0].signatures);
}

// -------------------------------------------------------------------------
// Ownership identity across a realistic multi-package application
// -------------------------------------------------------------------------

#[test]
fn each_package_owns_exactly_its_own_role() {
    let code = concat!(
        "package MyApp;\n",
        "use Mojo::Base 'Mojolicious';\n",
        "package MyApp::Controller::Users;\n",
        "use Mojo::Base 'Mojolicious::Controller';\n",
        "package MyApp::Model::Row;\n",
        "use Mojo::Base -base;\n",
    );
    let facts = derived_facts(code, GENERATION);
    assert_eq!(
        roles(&facts),
        vec![
            Some(MojoliciousRole::Application),
            Some(MojoliciousRole::Controller),
            // Negative control in the same file: a plain Mojo::Base class is
            // not a Mojolicious role.
            None,
        ]
    );
    assert_eq!(facts[0].package.as_deref(), Some("MyApp"));
    assert_eq!(facts[1].package.as_deref(), Some("MyApp::Controller::Users"));
    assert_eq!(facts[2].package.as_deref(), Some("MyApp::Model::Row"));
}

// -------------------------------------------------------------------------
// Negative controls
// -------------------------------------------------------------------------

#[test]
fn loading_the_framework_class_is_not_a_lite_activation() {
    // `use Mojolicious;` loads the class; it does not import the Lite DSL.
    assert!(lite_facts("use Mojolicious;\n", GENERATION).is_empty());
}

#[test]
fn a_project_controller_base_class_owns_no_mojolicious_role() {
    // A very common real shape: controllers inherit from the project's own
    // base controller. Only the exact framework parent activates.
    let facts = derived_facts(
        "package MyApp::Controller::Users;\nuse Mojo::Base 'MyApp::Controller::Base';\n",
        GENERATION,
    );
    assert_eq!(roles(&facts), vec![None]);
    assert!(matches!(
        facts[0].outcome,
        MojoliciousActivationOutcome::AbsentWithCompleteEvidence { .. }
    ));
}

#[test]
fn a_computed_parent_owns_no_role_even_though_it_could_be_mojolicious() {
    let facts = derived_facts(
        "package MyApp;\nour $parent = 'Mojolicious';\nuse Mojo::Base $parent;\n",
        GENERATION,
    );
    assert_eq!(roles(&facts), vec![None]);
    assert!(matches!(
        facts[0].outcome,
        MojoliciousActivationOutcome::DynamicOrUnmodeledParent { .. }
    ));
}

#[test]
fn a_stale_source_generation_retires_every_role() {
    // The strongest currentness falsifier: the detection is current for
    // `gen-2` while the source sites were extracted at `gen-1`.
    let code = concat!(
        "package MyApp;\n",
        "use Mojo::Base 'Mojolicious';\n",
        "package MyApp::Controller::Users;\n",
        "use Mojo::Base 'Mojolicious::Controller';\n",
    );
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let sites =
        extract_mojo_base_activation_sites(&ast, code, FileId(1), SourceGeneration::known("gen-1"));
    let detection = detect_mojo_base(&AdapterDetectionInput::new(
        mojo_base_descriptor(),
        receipt("Mojo::Base", DIST_VERSION, "gen-2"),
        None,
        AdapterCancellation::active(),
    ));
    let facts: Vec<MojoliciousActivationFacts> = sites
        .iter()
        .map(|site| {
            mojolicious_role_facts_from_mojo_base(&mojo_base_activation_facts(
                &detection,
                &site.anchor,
                &site.evidence,
            ))
        })
        .collect();
    assert_eq!(facts.len(), 2);
    assert_eq!(roles(&facts), vec![None, None]);
    for fact in &facts {
        assert!(matches!(
            fact.outcome,
            MojoliciousActivationOutcome::StaleOrIncompleteInput { .. }
        ));
    }
}

#[test]
fn an_unresolvable_framework_retires_the_lite_role() {
    // Module evidence that resolves to nothing must not activate.
    let code = "use Mojolicious::Lite;\n";
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let sites = extract_mojolicious_lite_activation_sites(
        &ast,
        FileId(1),
        SourceGeneration::known(GENERATION),
    );
    let observation = ModuleObservationReceipt::new(
        "module-resolver.v1",
        "root:fixture",
        "project-environment.v1",
        SourceGeneration::known(GENERATION),
        "sha256:fixture-input",
        vec![ModuleSelectorEvaluation::new(MOJOLICIOUS_LITE_MODULE, ModuleSelectorOutcome::Absent)],
    );
    let detection = detect_mojolicious_lite(&AdapterDetectionInput::new(
        mojolicious_lite_descriptor(),
        observation,
        None,
        AdapterCancellation::active(),
    ));
    let facts: Vec<MojoliciousActivationFacts> = sites
        .iter()
        .map(|site| mojolicious_lite_activation_facts(&detection, &site.anchor, &site.evidence))
        .collect();
    assert_eq!(roles(&facts), vec![None]);
}

#[test]
fn the_two_profiles_never_answer_for_each_others_source() {
    // Containment: the Lite extractor sees no Mojo::Base site, and the
    // Mojo::Base extractor sees no Lite site.
    let lite_source = "use Mojolicious::Lite;\n";
    let class_source = "package MyApp;\nuse Mojo::Base 'Mojolicious';\n";
    assert_eq!(lite_facts(lite_source, GENERATION).len(), 1);
    assert!(derived_facts(lite_source, GENERATION).is_empty());
    assert_eq!(derived_facts(class_source, GENERATION).len(), 1);
    assert!(lite_facts(class_source, GENERATION).is_empty());
}
