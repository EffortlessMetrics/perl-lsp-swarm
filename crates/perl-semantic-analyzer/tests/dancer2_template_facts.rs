#![deny(clippy::map_err_ignore)]
//! Integration falsifiers for the canonical Dancer2 template-target chain (#8937).
//!
//! Full chain over one source: parse → activation sites → template calls →
//! registry detection → exact activation facts → minted canonical facts.
//! The discriminating seam of this slice is the logical template name with
//! its exact source range: filesystem resolution stays unresolved because
//! canonical project configuration (#7900) is not on main.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::dancer2_activation::extract_dancer2_activation_sites;
use perl_semantic_analyzer::analysis::dancer2_templates::extract_dancer2_template_calls;
use perl_semantic_facts::framework::{
    AdapterCancellation, AdapterDetectionInput, DetectionEvidenceClass, ModuleActivationIdentity,
    ModuleObservationReceipt, ModuleSelectorEvaluation, ModuleSelectorOutcome,
    ModuleVersionEvidence,
};
use perl_semantic_facts::framework_adapters::dancer2::{
    dancer2_activation_facts, dancer2_descriptor, detect_dancer2,
};
use perl_semantic_facts::framework_adapters::dancer2_templates::dancer2_template_facts;
use perl_semantic_facts::template::{
    TemplateFilesystemTarget, TemplateNameSelection, TemplateTargetFact,
    template_target_fact_identity,
};
use perl_semantic_facts::{Confidence, FileId, SemanticFactStatus, SourceGeneration};
use perl_tdd_support::{must, must_some};

fn matched_dancer2(generation: &str) -> ModuleSelectorEvaluation {
    let activation = ModuleActivationIdentity::new(
        "Dancer2",
        Some(FileId(7)),
        SourceGeneration::known(generation),
    )
    .with_observed_version(ModuleVersionEvidence::new(
        "1.1.1",
        SourceGeneration::known(generation),
    ));
    ModuleSelectorEvaluation::new(
        "Dancer2",
        ModuleSelectorOutcome::Matched {
            activation,
            evidence_class: DetectionEvidenceClass::ResolvedModule,
        },
    )
}

fn input(generation: &str) -> AdapterDetectionInput {
    let observation = ModuleObservationReceipt::new(
        "module-resolver.v1",
        "root:fixture",
        "project-environment.v1",
        SourceGeneration::known(generation),
        "sha256:fixture-input",
        vec![matched_dancer2(generation)],
    );
    AdapterDetectionInput::new(
        dancer2_descriptor(),
        observation,
        None,
        AdapterCancellation::active(),
    )
}

fn canonical_facts(code: &str, generation: &str) -> Vec<TemplateTargetFact> {
    canonical_facts_with_input(code, &input(generation))
}

fn canonical_facts_with_input(
    code: &str,
    detection_input: &AdapterDetectionInput,
) -> Vec<TemplateTargetFact> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let sites = extract_dancer2_activation_sites(&ast, FileId(1));
    let declarations = extract_dancer2_template_calls(&ast, FileId(1));
    let detection = detect_dancer2(detection_input);
    let mut facts = Vec::new();
    for site in &sites {
        let activation =
            dancer2_activation_facts(&detection, site.package.as_deref(), &site.evidence);
        facts.extend(dancer2_template_facts(
            &detection,
            &activation,
            site.package.as_deref(),
            &declarations,
        ));
    }
    facts
}

fn logical_name(fact: &TemplateTargetFact) -> &str {
    must_some(fact.template.name.logical())
}

// Falsifier 1: a literal template call mints exactly one exact logical
// reference with every range anchored at exact tokens, and filesystem
// resolution stays unresolved.
#[test]
fn literal_template_call_mints_one_exact_anchored_logical_fact() {
    let code = "package MyApp;\nuse Dancer2;\ntemplate 'index';";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 1);
    let fact = &facts[0];
    assert_eq!(fact.template.keyword, "template");
    assert_eq!(logical_name(fact), "index");
    assert_eq!(fact.application_name, "MyApp");
    assert_eq!(fact.envelope.package.as_deref(), Some("MyApp"));
    assert_eq!(fact.framework_name, "Dancer2");
    assert_eq!(fact.envelope.source_generation, SourceGeneration::known("gen-1"));
    assert_eq!(fact.status(), SemanticFactStatus::Exact);
    assert!(fact.envelope.boundary.is_none());
    assert!(matches!(fact.template.filesystem, TemplateFilesystemTarget::Unresolved { .. }));

    let name = must_some(match &fact.template.name {
        TemplateNameSelection::Literal(name) => Some(name),
        TemplateNameSelection::Dynamic { .. } => None,
        _ => None,
    });
    assert_eq!(&code[name.anchor.start_byte as usize..name.anchor.end_byte as usize], "'index'");
    let keyword = fact.template.keyword_anchor;
    assert_eq!(&code[keyword.start_byte as usize..keyword.end_byte as usize], "template");
    let declaration = fact.envelope.anchor;
    assert_eq!(
        &code[declaration.start_byte as usize..declaration.end_byte as usize],
        "template 'index'"
    );
}

// Falsifier 2: the production form — a literal template call inside a route
// handler — still mints the logical reference.
#[test]
fn literal_template_inside_route_handler_mints() {
    let code = "package App;\nuse Dancer2;\nget '/' => sub { template 'home' };";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 1);
    assert_eq!(logical_name(&facts[0]), "home");
    assert_eq!(facts[0].status(), SemanticFactStatus::Exact);
}

// Falsifier 3: extra tokens/options operands do not hide the logical name.
#[test]
fn extra_operands_keep_the_logical_name() {
    let code =
        "package App;\nuse Dancer2;\ntemplate 'index', { title => 'Hi' }, { layout => 'main' };";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 1);
    assert_eq!(logical_name(&facts[0]), "index");
    assert_eq!(facts[0].status(), SemanticFactStatus::Exact);
}

// Falsifier 4: ZERO template facts without registry activation — ordinary
// `template()` without Dancer2, name-only detection, `use Dancer2::Core` only,
// and `!template` import exclusion all mint nothing.
#[test]
fn zero_template_facts_without_exact_activation() {
    let plain = "template 'index';";
    assert!(canonical_facts(plain, "gen-1").is_empty());

    let core_only = "package App;\nuse Dancer2::Core;\ntemplate 'index';";
    assert!(canonical_facts(core_only, "gen-1").is_empty());

    let dancer2_looking = "package App;\nuse Dancer2;\ntemplate 'index';";
    let absent_observation = ModuleObservationReceipt::new(
        "module-resolver.v1",
        "root:fixture",
        "project-environment.v1",
        SourceGeneration::known("gen-1"),
        "sha256:fixture-input",
        vec![ModuleSelectorEvaluation::new("Dancer2", ModuleSelectorOutcome::Absent)],
    );
    let absent_input = AdapterDetectionInput::new(
        dancer2_descriptor(),
        absent_observation,
        None,
        AdapterCancellation::active(),
    );
    assert!(canonical_facts_with_input(dancer2_looking, &absent_input).is_empty());

    let excluded = "package App;\nuse Dancer2 qw(!template);\ntemplate 'index';";
    assert!(canonical_facts(excluded, "gen-1").is_empty());
}

// Falsifier 5: computed names stay typed dynamic boundaries; the call is
// retained but never exact.
#[test]
fn computed_template_names_stay_boundaries() {
    let code = "package App;\nuse Dancer2;\ntemplate $name;\ntemplate \"page_$id\";";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 2);
    for fact in &facts {
        assert!(matches!(&fact.template.name, TemplateNameSelection::Dynamic { .. }));
        assert_eq!(fact.status(), SemanticFactStatus::Degraded);
        assert!(fact.envelope.boundary.is_some());
        assert!(matches!(fact.template.filesystem, TemplateFilesystemTarget::Unresolved { .. }));
    }
}

// Falsifier 6: a fat-comma bareword is a literal; a comma-separated bareword
// is a computed call, matching the hook auto-quote contract.
#[test]
fn fat_comma_barewords_are_literal_and_comma_barewords_are_not() {
    let promoted = canonical_facts("package App;\nuse Dancer2;\ntemplate index => {};", "gen-1");
    assert_eq!(promoted.len(), 1);
    assert_eq!(logical_name(&promoted[0]), "index");
    assert_eq!(promoted[0].status(), SemanticFactStatus::Exact);

    let called = canonical_facts("package App;\nuse Dancer2;\ntemplate(index);", "gen-1");
    assert_eq!(called.len(), 1);
    assert!(
        matches!(&called[0].template.name, TemplateNameSelection::Dynamic { .. }),
        "a comma-separated bareword calls a sub; it is not an auto-quoted name"
    );
    assert_eq!(called[0].status(), SemanticFactStatus::Degraded);
}

// Falsifier 7: malformed/incomplete template calls mint nothing.
#[test]
fn malformed_template_calls_mint_nothing() {
    let code = "package App;\nuse Dancer2;\ntemplate;\ntemplate();";
    assert!(canonical_facts(code, "gen-1").is_empty());
}

// Falsifier 8: same logical name in distinct apps stays isolated, and
// different generations never collide.
#[test]
fn same_logical_name_stays_isolated_per_app_and_generation() {
    let code = "package App1;\nuse Dancer2;\ntemplate 'index';\npackage App2;\nuse Dancer2;\ntemplate 'index';";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 2);
    assert_eq!(facts[0].application_name, "App1");
    assert_eq!(facts[1].application_name, "App2");
    assert_ne!(facts[0].envelope.fact_id, facts[1].envelope.fact_id);
    assert_ne!(facts[0].envelope.entity_id, facts[1].envelope.entity_id);
    assert_ne!(facts[0].envelope.package.as_deref(), facts[1].envelope.package.as_deref());

    let (gen1_fact, _) =
        template_target_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
    let (gen2_fact, _) =
        template_target_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-2"));
    assert_ne!(gen1_fact, gen2_fact);
    let (other_file, _) =
        template_target_fact_identity(FileId(2), 0, &SourceGeneration::known("gen-1"));
    assert_ne!(gen1_fact, other_file);
}

// Falsifier 9: currentness — editing/removing the template call at a new
// generation replaces the old facts; a stale held result cannot re-mint.
#[test]
fn edit_and_removal_replaces_facts_at_the_new_generation() {
    let before = "package App;\nuse Dancer2;\ntemplate 'index';";
    let first = canonical_facts(before, "gen-1");
    assert_eq!(first.len(), 1);
    assert_eq!(logical_name(&first[0]), "index");
    let first_id = first[0].envelope.fact_id;

    let edited = "package App;\nuse Dancer2;\ntemplate 'about';";
    let second = canonical_facts(edited, "gen-2");
    assert_eq!(second.len(), 1);
    assert_eq!(logical_name(&second[0]), "about");
    assert_ne!(first_id, second[0].envelope.fact_id, "edit mints a new identity");
    assert_eq!(second[0].envelope.source_generation, SourceGeneration::known("gen-2"));

    let removed = "package App;\nuse Dancer2;";
    assert!(canonical_facts(removed, "gen-3").is_empty());
}

// Falsifier 10: template facts round-trip through the JSON transport with the
// constructor re-checking every invariant on decode.
#[test]
fn template_facts_round_trip_through_json_transport() -> Result<(), serde_json::Error> {
    let code = "package App;\nuse Dancer2;\nget '/' => sub { template 'index' };";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 1);
    let serialized = serde_json::to_string(&facts)?;
    let decoded: Vec<TemplateTargetFact> = serde_json::from_str(&serialized)?;
    assert_eq!(decoded, facts);
    assert_eq!(decoded[0].status(), SemanticFactStatus::Exact);
    assert_eq!(
        decoded[0].envelope.confidence,
        perl_semantic_facts::SemanticConfidence::Known(Confidence::High)
    );
    Ok(())
}

// Falsifier 11: a custom DSL import does not inherit default-Dancer2 template
// keyword facts.
#[test]
fn custom_dsl_does_not_inherit_default_template_facts() {
    let code = "package App;\nuse Dancer2 dsl => 'My::DSL';\ntemplate 'index';";
    assert!(canonical_facts(code, "gen-1").is_empty());
}
