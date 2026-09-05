//! End-to-end registry-activated Mojo::Base object-fact proof (#9682).
//!
//! Drives real Perl source through the whole #9682 seam: the #9681 activation
//! extractor and checked adapter detection, the `has` attribute extractor in
//! `perl_semantic_analyzer::analysis::mojo_base_attributes`, and the fact
//! minting in
//! `perl_semantic_facts::framework_adapters::mojo_base_facts`. Every
//! acceptance row and every negative control named in #9682 appears as one
//! named test.
//!
//! Legacy generated-member extraction is deliberately not in this path: the
//! authoritative Mojo::Base producer starts from the checked activation, so a
//! same-named `has` call without an exact activation stays a hard negative.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::mojo_base_activation::{
    MojoBaseActivationSite, extract_mojo_base_activation_sites,
};
use perl_semantic_analyzer::analysis::mojo_base_attributes::extract_mojo_base_attribute_declarations;
use perl_semantic_facts::framework::{
    AdapterCancellation, AdapterDetectionInput, AdapterDetectionResult, DetectionEvidenceClass,
    ModuleActivationIdentity, ModuleObservationReceipt, ModuleSelectorEvaluation,
    ModuleSelectorOutcome, ModuleVersionEvidence,
};
use perl_semantic_facts::framework_adapters::mojo_base::{
    detect_mojo_base, mojo_base_activation_facts, mojo_base_descriptor,
};
use perl_semantic_facts::framework_adapters::mojo_base_facts::{
    MojoBaseAttributeDeclaration, MojoBaseAttributeDefault, MojoBaseExplicitMethodState,
    MojoBaseObjectFacts, mojo_base_object_facts,
};
use perl_semantic_facts::{
    CallableResultLimitation, CallableResultRelation, Confidence, FileId, GeneratedMemberKind,
    PackageEdgeKind, Provenance, SemanticFactStatus, SemanticReasonCode, SourceGeneration,
    ValueShape,
};
use perl_tdd_support::{must, must_some};

// ── Harness ─────────────────────────────────────────────────────────────

fn matched_mojo_base(version: Option<&str>, generation: &str) -> ModuleSelectorEvaluation {
    let activation = ModuleActivationIdentity::new(
        "Mojo::Base",
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
        "Mojo::Base",
        ModuleSelectorOutcome::Matched {
            activation,
            evidence_class: DetectionEvidenceClass::ResolvedModule,
        },
    )
}

fn detection(version: &str, generation: &str) -> AdapterDetectionResult {
    detect_mojo_base(&AdapterDetectionInput::new(
        mojo_base_descriptor(),
        ModuleObservationReceipt::new(
            "module-resolver.v1",
            "root:fixture",
            "project-environment.v1",
            SourceGeneration::known(generation),
            "sha256:fixture-input",
            vec![matched_mojo_base(Some(version), generation)],
        ),
        None,
        AdapterCancellation::active(),
    ))
}

fn sites(code: &str, file_id: FileId, generation: &str) -> Vec<MojoBaseActivationSite> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    extract_mojo_base_activation_sites(&ast, code, file_id, SourceGeneration::known(generation))
}

fn declarations(
    code: &str,
    file_id: FileId,
    generation: &str,
) -> Vec<MojoBaseAttributeDeclaration> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    extract_mojo_base_attribute_declarations(&ast, file_id, SourceGeneration::known(generation))
}

/// Run the whole seam over `code` and return the object facts of every
/// activation site, in source order.
fn object_facts_for(code: &str, version: &str, generation: &str) -> Vec<MojoBaseObjectFacts> {
    object_facts_in(code, FileId(1), version, generation)
}

fn object_facts_in(
    code: &str,
    file_id: FileId,
    version: &str,
    generation: &str,
) -> Vec<MojoBaseObjectFacts> {
    let detection = detection(version, generation);
    let declarations = declarations(code, file_id, generation);
    sites(code, file_id, generation)
        .iter()
        .map(|site| {
            let activation = mojo_base_activation_facts(&detection, &site.anchor, &site.evidence);
            mojo_base_object_facts(
                &detection,
                &activation,
                file_id,
                site.anchor.package.as_deref(),
                &declarations,
            )
        })
        .collect()
}

fn only_facts(code: &str) -> MojoBaseObjectFacts {
    let facts = object_facts_for(code, "9.34", "gen-1");
    assert_eq!(facts.len(), 1, "fixture must carry exactly one activation site");
    must_some(facts.into_iter().next())
}

fn member_names(facts: &MojoBaseObjectFacts) -> Vec<String> {
    facts.members.iter().map(|fact| fact.member.name.clone()).collect()
}

// ── Generated accessor members ──────────────────────────────────────────

#[test]
fn admitted_has_forms_emit_source_anchored_generated_accessors() {
    let code = "package App;\nuse Mojo::Base -base;\nhas 'name';\nhas port => 8080;\n";
    let facts = only_facts(code);
    assert_eq!(member_names(&facts), ["name", "port"]);
    for fact in &facts.members {
        // Mojo::Base accessors are read-write: one method reads and writes.
        assert_eq!(fact.member.kind, GeneratedMemberKind::Accessor);
        // The generator anchor must be the real `has` declaration; a generated
        // member never receives a fabricated body.
        let start = fact.envelope.anchor.start_byte as usize;
        let end = fact.envelope.anchor.end_byte as usize;
        assert!(
            code[start..end].starts_with("has "),
            "the generated member must anchor its real `has` declaration, got {:?}",
            &code[start..end]
        );
        assert_eq!(fact.member.source_anchor_id.0, fact.envelope.anchor.start_byte as u64);
    }
}

#[test]
fn an_array_reference_declares_one_accessor_per_name() {
    let facts = only_facts("package App;\nuse Mojo::Base -base;\nhas [qw(host port)];\n");
    assert_eq!(member_names(&facts), ["host", "port"]);
    assert_ne!(
        facts.members[0].member.entity_id, facts.members[1].member.entity_id,
        "names of one statement must own distinct entities"
    );
}

#[test]
fn generated_members_never_claim_explicit_source_provenance() {
    let facts = only_facts("package App;\nuse Mojo::Base -base;\nhas 'name';\n");
    let member = &facts.members[0];
    assert_eq!(member.member.provenance, Provenance::FrameworkSynthesis);
    assert_eq!(member.member.confidence, Confidence::Medium);
    assert_eq!(member.envelope.reason_code, SemanticReasonCode::GeneratedFromSource);
    assert_ne!(
        member.envelope.status(),
        SemanticFactStatus::Exact,
        "source-backed generated is never promoted to explicit source"
    );
}

// ── Reader result versus fluent setter ──────────────────────────────────

#[test]
fn read_result_and_setter_return_self_are_distinct_facts() {
    let facts = only_facts("package App;\nuse Mojo::Base -base;\nhas name => 'anon';\n");
    let reader = &facts.reader_results[0];
    let setter = &facts.setter_results[0];
    assert_eq!(setter.relation, CallableResultRelation::ReceiverSelf);
    assert_eq!(reader.relation, CallableResultRelation::Concrete(ValueShape::Scalar));
    assert_ne!(reader.envelope.fact_id, setter.envelope.fact_id);
    assert_eq!(
        reader.envelope.entity_id, setter.envelope.entity_id,
        "both relations describe the same accessor entity"
    );
    assert_eq!(reader.envelope.entity_id, Some(facts.members[0].member.entity_id));
}

#[test]
fn default_uncertainty_limits_the_reader_without_erasing_the_setter_relation() {
    // A lazy builder's value is not evaluated here, so the read result stays
    // unknown — but the framework's write contract is still determinate.
    let facts = only_facts("package App;\nuse Mojo::Base -base;\nhas config => sub { {} };\n");
    assert_eq!(facts.reader_results[0].relation, CallableResultRelation::Unknown);
    assert_eq!(facts.setter_results[0].relation, CallableResultRelation::ReceiverSelf);
}

#[test]
fn callable_results_carry_no_fabricated_exit_contributor() {
    let facts = only_facts("package App;\nuse Mojo::Base -base;\nhas 'name';\n");
    for fact in facts.reader_results.iter().chain(facts.setter_results.iter()) {
        assert!(
            fact.exit_anchors().is_empty(),
            "a generated accessor has a generator anchor, not a return statement"
        );
        assert!(fact.limitations().contains(&CallableResultLimitation::GeneratedNoSource));
        assert_ne!(fact.status(), SemanticFactStatus::Exact);
    }
}

// ── Literal parent relationship ─────────────────────────────────────────

#[test]
fn base_activation_inherits_from_mojo_base() {
    let facts = only_facts("package App;\nuse Mojo::Base -base;\n");
    assert_eq!(facts.parents.len(), 1);
    assert_eq!(facts.parents[0].edge.from_package, "App");
    assert_eq!(facts.parents[0].edge.to_package, "Mojo::Base");
    assert_eq!(facts.parents[0].edge.kind, PackageEdgeKind::Inherits);
}

#[test]
fn literal_parent_activation_emits_one_inheritance_relation_anchored_at_its_spelling() {
    let code = "package Log;\nuse Mojo::Base 'Mojo::EventEmitter', -signatures;\n";
    let facts = only_facts(code);
    assert_eq!(facts.parents.len(), 1);
    let parent = &facts.parents[0];
    assert_eq!(parent.edge.to_package, "Mojo::EventEmitter");
    assert_eq!(parent.edge.provenance, Provenance::ExactAst);
    let start = parent.envelope.anchor.start_byte as usize;
    let end = parent.envelope.anchor.end_byte as usize;
    assert_eq!(
        &code[start..end],
        "'Mojo::EventEmitter'",
        "the parent fact must anchor the literal spelling"
    );
}

#[test]
fn a_computed_parent_establishes_no_inheritance_relation() {
    // A dynamic parent is not an exact activation, so nothing mints at all.
    let code = "package App;\nuse Mojo::Base $parent;\nhas 'name';\n";
    // The import *is* an activation site; only its profile is inexact, so the
    // emptiness below is a real refusal rather than an absent site.
    assert_eq!(sites(code, FileId(1), "gen-1").len(), 1);
    let facts = object_facts_for(code, "9.34", "gen-1");
    assert_eq!(facts.len(), 1);
    assert!(
        facts.iter().all(MojoBaseObjectFacts::is_empty),
        "a dynamic parent must not produce object facts"
    );
}

// ── Negative controls ───────────────────────────────────────────────────

#[test]
fn a_same_named_has_without_activation_emits_no_facts() {
    // The load-bearing #9682 negative control: `has` is never activation
    // evidence, so an ordinary package that merely calls `has` mints nothing.
    let code = "package Plain;\nhas 'name';\nhas port => 1;\n";
    assert!(
        sites(code, FileId(1), "gen-1").is_empty(),
        "no activation site exists in an unactivated package"
    );
    assert!(
        object_facts_for(code, "9.34", "gen-1").is_empty(),
        "without an activation site there is nothing to mint over"
    );
    // The same `has` calls under an exact activation do mint, so the emptiness
    // above isolates activation rather than the extractor.
    let activated = format!("package Plain;\nuse Mojo::Base -base;\n{}", &code[15..]);
    assert_eq!(only_facts(&activated).members.len(), 2);
}

#[test]
fn has_calls_of_another_package_never_join_a_foreign_activation() {
    let code = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "has 'owned';\n",
        "package Plain;\n",
        "has 'foreign';\n",
    );
    let facts = only_facts(code);
    assert_eq!(
        member_names(&facts),
        ["owned"],
        "an activation must not reach across a package boundary"
    );
}

#[test]
fn an_unsupported_module_version_mints_no_object_facts() {
    let code = "package App;\nuse Mojo::Base -base;\nhas 'name';\n";
    // Same source mints three members under a reviewed version, so the
    // emptiness below isolates the version gate rather than a missing site.
    assert_eq!(only_facts(code).members.len(), 1);
    let facts = object_facts_for(code, "10.0.0", "gen-1");
    assert_eq!(facts.len(), 1, "the activation site still exists");
    assert!(
        facts.iter().all(MojoBaseObjectFacts::is_empty),
        "an unreviewed version cannot activate exact object semantics"
    );
}

#[test]
fn a_stale_source_generation_mints_no_object_facts() {
    // The site was extracted from an older source generation than the
    // detection receipt: stale evidence must not be reused.
    let code = "package App;\nuse Mojo::Base -base;\nhas 'name';\n";
    let detection = detection("9.34", "gen-2");
    let declarations = declarations(code, FileId(1), "gen-2");
    let stale_site = must_some(sites(code, FileId(1), "gen-1").into_iter().next());
    let activation =
        mojo_base_activation_facts(&detection, &stale_site.anchor, &stale_site.evidence);
    let facts = mojo_base_object_facts(
        &detection,
        &activation,
        FileId(1),
        stale_site.anchor.package.as_deref(),
        &declarations,
    );
    assert!(facts.is_empty(), "stale activation evidence mints nothing");
}

#[test]
fn a_computed_accessor_name_mints_no_member() {
    let facts = only_facts("package App;\nuse Mojo::Base -base;\nhas $field => 1;\nhas 'ok';\n");
    assert_eq!(
        member_names(&facts),
        ["ok"],
        "a computed name stays a boundary and is never a guessed accessor"
    );
}

#[test]
fn an_explicit_source_method_keeps_the_collision_visible() {
    let code = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "has 'name';\n",
        "sub name { 'explicit' }\n",
    );
    let facts = only_facts(code);
    assert_eq!(facts.members[0].explicit_method, MojoBaseExplicitMethodState::Collides);
    assert!(
        facts.members[0].envelope.boundary.is_some(),
        "an explicit method outranks the generated accessor; the conflict stays evidence"
    );
}

#[test]
fn a_non_code_reference_default_stays_an_unsupported_boundary() {
    let code = "package App;\nuse Mojo::Base -base;\nhas 'list' => [];\n";
    let declared = declarations(code, FileId(1), "gen-1");
    assert!(matches!(declared[0].default, MojoBaseAttributeDefault::Unsupported { .. }));
    let facts = only_facts(code);
    assert_eq!(facts.reader_results[0].relation, CallableResultRelation::Unknown);
    assert!(
        facts.reader_results[0].limitations().contains(&CallableResultLimitation::Unsupported),
        "Mojo::Base rejects a non-code reference default at runtime"
    );
}

#[test]
fn a_has_call_before_the_activating_import_is_not_an_accessor() {
    // `Mojo::Base` installs `has` at import time, so a call earlier in the
    // package is a different function entirely — the later activation must not
    // retroactively turn it into an accessor.
    let code =
        concat!("package App;\n", "has 'before';\n", "use Mojo::Base -base;\n", "has 'after';\n",);
    // Both calls are observed by extraction; only the later one may mint.
    assert_eq!(declarations(code, FileId(1), "gen-1").len(), 2, "extraction observes both calls");
    let facts = only_facts(code);
    assert_eq!(member_names(&facts), ["after"], "a pre-import `has` is not this framework's `has`");
}

#[test]
fn a_declaration_from_another_file_cannot_join_this_activation() {
    // Same package name, different file: the carrier must not contribute.
    let code = "package App;\nuse Mojo::Base -base;\nhas 'owned';\n";
    let mut foreign = declarations(code, FileId(2), "gen-1");
    assert_eq!(foreign.len(), 1);
    let mut mixed = declarations(code, FileId(1), "gen-1");
    mixed.append(&mut foreign);
    let detection = detection("9.34", "gen-1");
    let site = must_some(sites(code, FileId(1), "gen-1").into_iter().next());
    let activation = mojo_base_activation_facts(&detection, &site.anchor, &site.evidence);
    let facts = mojo_base_object_facts(
        &detection,
        &activation,
        FileId(1),
        site.anchor.package.as_deref(),
        &mixed,
    );
    assert_eq!(
        facts.members.len(),
        1,
        "only the activating file's declaration may mint, despite the shared package name"
    );
}

#[test]
fn a_declaration_from_an_older_parse_cannot_be_restamped_as_current() {
    // The attribute existed at gen-1 and was removed at gen-2. Handing the
    // stale carrier to a current activation must not resurrect it.
    let old = "package App;\nuse Mojo::Base -base;\nhas 'removed';\n";
    let new = "package App;\nuse Mojo::Base -base;\n";
    let stale = declarations(old, FileId(1), "gen-1");
    assert_eq!(stale.len(), 1, "the attribute really existed at gen-1");
    let detection = detection("9.34", "gen-2");
    let site = must_some(sites(new, FileId(1), "gen-2").into_iter().next());
    let activation = mojo_base_activation_facts(&detection, &site.anchor, &site.evidence);
    assert!(activation.is_exact(), "the gen-2 activation itself is exact");
    let facts = mojo_base_object_facts(
        &detection,
        &activation,
        FileId(1),
        site.anchor.package.as_deref(),
        &stale,
    );
    assert!(
        facts.members.is_empty(),
        "a removed accessor must not reappear as a fresh fact under a newer generation"
    );
}

#[test]
fn a_conditional_has_declares_no_accessor() {
    // Under a conditional the call may never run; an unconditional accessor
    // would be an overclaim.
    let code = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "if ($ENV{X}) { has 'maybe'; }\n",
        "has 'always';\n",
    );
    let facts = only_facts(code);
    assert_eq!(member_names(&facts), ["always"]);
}

#[test]
fn a_corpus_option_bearing_declaration_keeps_its_accessor_and_write_contract() {
    // Verbatim from the bundled corpus
    // (test_corpus/real_projects/mojolicious_skeleton/lib/Mojolicious/Controller.pm).
    // `Mojo::Base` generates an ordinary read-write accessor for a `weak`
    // attribute, so the member and the write contract are unaffected; only the
    // read result is limited, because an unmodeled option can change what a
    // read yields.
    let code =
        "package Mojolicious::Controller;\nuse Mojo::Base -base;\nhas app => undef, weak => 1;\n";
    let facts = only_facts(code);
    assert_eq!(
        member_names(&facts),
        ["app"],
        "an option-bearing `has` still declares its accessor"
    );
    assert_eq!(
        facts.setter_results[0].relation,
        CallableResultRelation::ReceiverSelf,
        "a weak attribute's write still returns the invocant"
    );
    assert_eq!(facts.reader_results[0].relation, CallableResultRelation::Unknown);
    assert!(
        facts.reader_results[0].limitations().contains(&CallableResultLimitation::Unsupported),
        "an unmodeled option must limit the read result"
    );
}

// ── Currentness, isolation, determinism ─────────────────────────────────

#[test]
fn a_source_edit_invalidates_old_identities_before_replacement() {
    let code = "package App;\nuse Mojo::Base -base;\nhas 'name';\n";
    let before = must_some(object_facts_for(code, "9.34", "gen-1").into_iter().next());
    let after = must_some(object_facts_for(code, "9.34", "gen-2").into_iter().next());
    assert_ne!(
        before.members[0].envelope.fact_id, after.members[0].envelope.fact_id,
        "a new generation must mint a new fact identity"
    );
    for fact in &after.members {
        assert!(
            fact.envelope
                .invalidation_dependencies()
                .iter()
                .any(|dependency| dependency.dependency_key == "module:Mojo::Base"),
            "every fact depends on the activating module"
        );
        assert!(
            fact.envelope
                .invalidation_dependencies()
                .iter()
                .any(|dependency| dependency.dependency_key.starts_with("file:")),
            "every fact depends on its owning source file"
        );
    }
}

#[test]
fn same_named_packages_in_different_files_stay_isolated() {
    let code = "package App;\nuse Mojo::Base -base;\nhas 'name';\n";
    let first = must_some(object_facts_in(code, FileId(1), "9.34", "gen-1").into_iter().next());
    let second = must_some(object_facts_in(code, FileId(2), "9.34", "gen-1").into_iter().next());
    assert_ne!(
        first.members[0].member.entity_id, second.members[0].member.entity_id,
        "one package name in two files must not share a member entity"
    );
}

#[test]
fn minting_is_deterministic_and_source_ordered() {
    let code = "package App;\nuse Mojo::Base -base;\nhas 'a';\nhas 'b';\nhas 'c';\n";
    let first = only_facts(code);
    let second = only_facts(code);
    assert_eq!(first, second, "repeated minting must be byte-identical");
    assert_eq!(member_names(&first), ["a", "b", "c"]);
}

#[test]
fn every_minted_family_stays_aligned_one_to_one() {
    let code = "package App;\nuse Mojo::Base -base;\nhas 'a';\nhas [qw(b c)];\n";
    let facts = only_facts(code);
    assert_eq!(facts.members.len(), 3);
    assert_eq!(facts.reader_results.len(), facts.members.len());
    assert_eq!(facts.setter_results.len(), facts.members.len());
    for (index, member) in facts.members.iter().enumerate() {
        assert_eq!(facts.reader_results[index].envelope.entity_id, Some(member.member.entity_id));
        assert_eq!(facts.setter_results[index].envelope.entity_id, Some(member.member.entity_id));
    }
}
