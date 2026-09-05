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
    MojoBaseAttributeDeclaration, MojoBaseAttributeDefault, MojoBaseExecutionPhase,
    MojoBaseExplicitMethodState, MojoBaseObjectFacts, mojo_base_object_facts,
};
use perl_semantic_facts::{
    BoundaryKind, CallableResultLimitation, CallableResultRelation, Confidence, FileId,
    GeneratedMemberKind, PackageEdgeKind, Provenance, SemanticFactStatus, SemanticReasonCode,
    SourceGeneration, ValueShape,
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
fn an_early_phaser_above_the_activating_import_is_not_an_accessor() {
    // Source order against the import decides only for code that also runs at
    // compile time. Verified against `perl`: `use` runs inside an implicit
    // `BEGIN`, so a phaser written above the activating import runs before
    // `Mojo::Base` has installed `has`, and one written below it does not.
    let code = concat!(
        "package App;\n",
        "BEGIN { has('too_early'); }\n",
        "use Mojo::Base -base;\n",
        "BEGIN { has('after_import'); }\n",
        "has 'ordinary';\n",
    );
    assert_eq!(
        declarations(code, FileId(1), "gen-1").len(),
        3,
        "extraction observes all three calls; only minting rejects"
    );
    let facts = only_facts(code);
    assert_eq!(member_names(&facts), ["after_import", "ordinary"]);
}

#[test]
fn an_ordinary_has_above_the_activating_import_still_declares_an_accessor() {
    // The counterpart to the control above, and the reason phase is carried
    // rather than source position alone. An ordinary statement runs after the
    // whole file is compiled, so every `use` in the file has already imported
    // by then — the call reaches `Mojo::Base`'s `has` even though it is
    // written above the import. Rejecting it on byte order would drop a real
    // accessor.
    //
    // The spelling is parenthesized deliberately: `has 'before';` above the
    // import is a Perl syntax error ("String found where operator expected"),
    // because `has` is not predeclared at that point. So this arrangement can
    // only occur in compiling source with parentheses, and that is exactly the
    // form that does declare an attribute.
    let code =
        concat!("package App;\n", "has('before');\n", "use Mojo::Base -base;\n", "has 'after';\n",);
    let facts = only_facts(code);
    assert_eq!(member_names(&facts), ["before", "after"]);
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

#[test]
fn a_declaration_in_an_end_block_declares_no_accessor() {
    // `END` runs at process shutdown, after the program it would serve has
    // finished. An accessor installed there exists for no part of the run, so
    // reporting it as a class member is an overclaim. Every earlier phase
    // (`BEGIN`, `UNITCHECK`, `CHECK`, `INIT`) completes before the run phase,
    // so those accessors do exist and must still mint.
    let code = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "END { has 'shutdown_only'; }\n",
        "BEGIN { has 'compile_time'; }\n",
        "INIT { has 'init_time'; }\n",
        "has 'always';\n",
    );
    let facts = only_facts(code);
    assert_eq!(member_names(&facts), ["compile_time", "init_time", "always"]);
}

#[test]
fn a_package_other_than_the_activating_one_mints_nothing() {
    // The activation owns `App`. Asking for `Other`'s members under it must
    // fail closed: neither `Other`'s `has` calls nor an `Other inherits
    // Mojo::Base` edge are established by an activation `Other` never made.
    let code = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "package Other;\n",
        "has 'borrowed';\n",
    );
    let declarations = declarations(code, FileId(1), "gen-1");
    assert_eq!(
        declarations.len(),
        1,
        "`Other`'s declaration really is extracted; only minting may reject it"
    );
    let detection = detection("9.34", "gen-1");
    let site = must_some(sites(code, FileId(1), "gen-1").into_iter().next());
    let activation = mojo_base_activation_facts(&detection, &site.anchor, &site.evidence);
    assert!(activation.is_exact(), "the `App` activation itself is exact");
    assert_eq!(activation.package.as_deref(), Some("App"));
    let facts =
        mojo_base_object_facts(&detection, &activation, FileId(1), Some("Other"), &declarations);
    assert!(facts.members.is_empty(), "`App`'s activation cannot generate accessors for `Other`");
    assert!(
        facts.parents.is_empty(),
        "`App`'s activation cannot make `Other` inherit from Mojo::Base"
    );
}

#[test]
fn an_early_phaser_declares_an_accessor_wherever_it_is_nested() {
    // Verified against `perl` itself: BEGIN/UNITCHECK/CHECK/INIT are scheduled
    // at compile time and run regardless of what lexically encloses them — a
    // false conditional, a loop, a sub body, or an `END` block. The accessor
    // therefore exists for the whole run and is a genuine class member.
    //
    // `END` nested inside `END` still runs at shutdown, so it stays excluded:
    // the rule is the phaser's own schedule, not merely "a phaser encloses it".
    let code = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "END { BEGIN { has 'begin_in_end'; } }\n",
        "if (0) { BEGIN { has 'begin_under_false_cond'; } }\n",
        "while (0) { INIT { has 'init_in_loop'; } }\n",
        "sub helper { CHECK { has 'check_in_sub'; } }\n",
        "END { END { has 'end_in_end'; } }\n",
        "END { has 'plain_in_end'; }\n",
        "if (0) { has 'plain_under_cond'; }\n",
        "sub other { has 'plain_in_sub'; }\n",
        "has 'always';\n",
    );
    let facts = only_facts(code);
    assert_eq!(
        member_names(&facts),
        ["begin_in_end", "begin_under_false_cond", "init_in_loop", "check_in_sub", "always"],
        "an early phaser mints wherever it is nested; a deferred or conditional \
         position without one mints nothing"
    );
}

#[test]
fn a_compile_phase_collision_reports_an_undetermined_winner() {
    // Which of a colliding pair survives depends on the declaration's phase,
    // verified against `perl`:
    //
    //   BEGIN { *name = sub {...} }  sub name {...}   -> the explicit sub wins
    //   sub name {...}  BEGIN { *name = sub {...} }   -> the accessor wins
    //
    // A run-phase `has` runs after the whole file is compiled, so it always
    // overwrites the explicit sub and the accessor is determinately live. A
    // compile-phase `has` interleaves with subroutine compilation, so the
    // winner depends on relative position — which this producer does not
    // resolve, and therefore must not claim.
    let run_phase = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "has 'name';\n",
        "sub name { 'explicit' }\n",
    );
    let compile_phase = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "BEGIN { has('name'); }\n",
        "sub name { 'explicit' }\n",
    );

    let run_facts = only_facts(run_phase);
    assert_eq!(run_facts.members[0].explicit_method, MojoBaseExplicitMethodState::Collides);
    let run_boundary = must_some(run_facts.members[0].envelope.boundary.clone());
    assert_eq!(
        run_boundary.kind,
        BoundaryKind::Compatibility,
        "a run-phase accessor determinately overwrites the explicit sub"
    );

    let compile_facts = only_facts(compile_phase);
    assert_eq!(
        declarations(compile_phase, FileId(1), "gen-1")[0].execution_phase,
        MojoBaseExecutionPhase::CompileImmediate,
        "the declaration really is carried as BEGIN-phase"
    );
    assert_eq!(compile_facts.members[0].explicit_method, MojoBaseExplicitMethodState::Collides);
    let compile_boundary = must_some(compile_facts.members[0].envelope.boundary.clone());
    assert_eq!(
        compile_boundary.kind,
        BoundaryKind::CompileTimeExecution,
        "a compile-phase collision cannot claim the accessor is the live method"
    );
    assert_ne!(
        run_boundary.kind, compile_boundary.kind,
        "the two phases must not collapse to one boundary"
    );
}

#[test]
fn only_a_literal_parent_activation_claims_exact_source_provenance() {
    // `use Mojo::Base 'Parent'` spells the parent in source, so the edge
    // repeats a literal and is exact. `use Mojo::Base -base` spells no parent
    // at all: that the superclass is `Mojo::Base` is knowledge about the
    // framework, not a reading of this file, so it must carry synthesis
    // provenance like every other generated fact here.
    let literal = "package App;\nuse Mojo::Base 'Parent';\n";
    let base = "package App;\nuse Mojo::Base -base;\n";

    let literal_facts = only_facts(literal);
    assert_eq!(literal_facts.parents.len(), 1);
    assert_eq!(literal_facts.parents[0].edge.provenance, Provenance::ExactAst);
    assert_eq!(literal_facts.parents[0].edge.confidence, Confidence::High);
    assert_eq!(
        literal_facts.parents[0].envelope.reason_code,
        SemanticReasonCode::ExactSource,
        "the parent spelling is literally in source"
    );

    let base_facts = only_facts(base);
    assert_eq!(base_facts.parents.len(), 1);
    assert_eq!(
        base_facts.parents[0].edge.provenance,
        Provenance::FrameworkSynthesis,
        "`-base` names no parent in source; the edge is framework knowledge"
    );
    assert_eq!(base_facts.parents[0].edge.confidence, Confidence::Medium);
    assert_eq!(base_facts.parents[0].envelope.reason_code, SemanticReasonCode::GeneratedFromSource);
}

#[test]
fn a_declaration_inside_defer_declares_no_accessor() {
    // `defer { ... }` runs at scope exit, so the accessor does not exist for
    // the code that precedes that exit.
    let code = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "defer { has 'at_scope_exit'; }\n",
        "has 'always';\n",
    );
    let facts = only_facts(code);
    assert_eq!(member_names(&facts), ["always"]);
}

#[test]
fn a_has_inside_a_callback_block_declares_no_accessor() {
    // `map`, `grep` and `sort` run their block once per element, so a `has`
    // inside one runs zero times for an empty list and many times otherwise —
    // never exactly once at package load. A nested early phaser still runs on
    // schedule, so it keeps its accessor, the same rule that applies inside
    // any other deferred region.
    let code = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "map { has 'in_map'; } (1, 2, 3);\n",
        "my @kept = grep { has 'in_grep'; } (1, 2);\n",
        "map { BEGIN { has('phaser_in_map'); } } (1);\n",
        "has 'always';\n",
    );
    let facts = only_facts(code);
    assert_eq!(member_names(&facts), ["phaser_in_map", "always"]);
}

#[test]
fn an_array_reference_declaration_keeps_its_accessors_when_options_follow() {
    // `attr` binds `($self, $attrs, $value, %kv)`, so trailing options are
    // legal alongside an array-reference name list and every listed accessor
    // is still generated. The parser puts the options in the same hash literal
    // as the default, which previously made the whole declaration
    // unrecognisable and dropped both accessors silently.
    let code = "package App;\nuse Mojo::Base -base;\nhas [qw(a b)] => undef, weak => 1;\n";
    let declared = declarations(code, FileId(1), "gen-1");
    assert_eq!(declared.len(), 2, "both listed names are still declared");
    assert_eq!(declared[0].unmodeled_options, ["weak"], "the option key is recorded, not dropped");
    let facts = only_facts(code);
    assert_eq!(member_names(&facts), ["a", "b"]);
    // Same limitation the flat option-bearing form already carries: an
    // unmodeled option can change what a read yields, but not the write
    // contract.
    assert!(
        facts.reader_results[0].limitations().contains(&CallableResultLimitation::Unsupported),
        "an unmodeled option limits the read result"
    );
    assert_eq!(facts.setter_results[0].relation, CallableResultRelation::ReceiverSelf);
}

#[test]
fn only_begin_is_order_sensitive_against_the_activating_import() {
    // Verified against `perl` with the import written *below* each phaser:
    // `BEGIN` alone does not see it, because `BEGIN` runs at its own position
    // during compilation. `UNITCHECK`, `CHECK` and `INIT` are scheduled to run
    // once compilation has finished, so the import has already happened and
    // their accessors are real.
    let code = concat!(
        "package App;\n",
        "BEGIN     { has('begin_above'); }\n",
        "UNITCHECK { has('unitcheck_above'); }\n",
        "CHECK     { has('check_above'); }\n",
        "INIT      { has('init_above'); }\n",
        "use Mojo::Base -base;\n",
        "has 'ordinary';\n",
    );
    let facts = only_facts(code);
    assert_eq!(
        member_names(&facts),
        ["unitcheck_above", "check_above", "init_above", "ordinary"],
        "only `BEGIN` above the import is too early"
    );
}

#[test]
fn a_post_compile_collision_keeps_a_determinate_winner() {
    // `CHECK`/`INIT`/`UNITCHECK` run after every `sub` in the file is
    // compiled, so `monkey_patch` overwrites the explicit method exactly as a
    // run-phase `has` does — verified against `perl`. Only `BEGIN`, which
    // interleaves with subroutine compilation, is undetermined.
    let code = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "CHECK { has('name'); }\n",
        "sub name { 'explicit' }\n",
    );
    let declared = declarations(code, FileId(1), "gen-1");
    assert_eq!(declared[0].execution_phase, MojoBaseExecutionPhase::PostCompile);
    let facts = only_facts(code);
    let boundary = must_some(facts.members[0].envelope.boundary.clone());
    assert_eq!(
        boundary.kind,
        BoundaryKind::Compatibility,
        "a post-compile accessor determinately overwrites the explicit sub"
    );
}

#[test]
fn an_odd_option_tail_keeps_the_declared_default() {
    // `%kv` is an ordinary hash assignment, so Perl binds the dangling key to
    // `undef` (with a warning) and `attr` still generates the accessor with
    // the default it was given. Treating the parity as a rejected default
    // would make a determinate reader falsely unknown.
    let code = "package App;\nuse Mojo::Base -base;\nhas name => 'anon', weak;\n";
    let declared = declarations(code, FileId(1), "gen-1");
    assert_eq!(declared.len(), 1);
    assert_eq!(
        declared[0].default,
        MojoBaseAttributeDefault::Constant,
        "the default is still a source-literal constant despite the odd tail"
    );
    assert_eq!(declared[0].unmodeled_options, ["weak"], "the dangling key is still an option key");
    let facts = only_facts(code);
    assert_eq!(member_names(&facts), ["name"]);
}

#[test]
fn a_brace_argument_is_deferred_even_when_it_may_be_an_immediate_hash() {
    // Pins a deliberate under-report, so it stays a known limitation rather
    // than drifting silently.
    //
    // Under `perl`, braces after a plain sub are an immediately-evaluated hash
    // constructor, while braces after a `(&@)`-prototyped sub are a deferred
    // callback. The parser emits the identical `FunctionCall` -> `Block` shape
    // for both and the prototype is not available here, so the two cannot be
    // distinguished. Both are deferred: omitting an accessor that exists is
    // recoverable, publishing one that cannot exist is not.
    let code = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "some_function { has 'maybe_immediate'; };\n",
        "has 'always';\n",
    );
    let facts = only_facts(code);
    assert_eq!(
        member_names(&facts),
        ["always"],
        "an ambiguous brace argument is deferred rather than guessed either way"
    );
}

#[test]
fn a_subroutine_nested_in_another_subroutine_still_collides() {
    // Perl installs a named `sub` into the package symbol table at compile
    // time wherever it is written, verified directly:
    //
    //   perl -e 'sub outer { sub inner { ... } } inner();'   -> inner runs
    //
    // So `sub outer { sub name { ... } }` really can shadow the accessor.
    // `SubroutineTargetIndex` does not index nested bodies, and missing the
    // collision would mint the member with no boundary at all — asserting
    // there is no conflict when there is one.
    let nested = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "has 'name';\n",
        "sub outer { sub name { 'explicit' } }\n",
    );
    let facts = only_facts(nested);
    assert_eq!(member_names(&facts), ["name"]);
    assert_eq!(
        facts.members[0].explicit_method,
        MojoBaseExplicitMethodState::Collides,
        "a nested named package sub is still an explicit method"
    );
    assert!(facts.members[0].envelope.boundary.is_some());

    // Control: without the nested sub the same source reports no collision, so
    // the assertion above isolates the nesting rather than always holding.
    let clean = "package App;\nuse Mojo::Base -base;\nhas 'name';\n";
    let clean_facts = only_facts(clean);
    assert_eq!(clean_facts.members[0].explicit_method, MojoBaseExplicitMethodState::None);
    assert!(clean_facts.members[0].envelope.boundary.is_none());
}

#[test]
fn a_nested_method_in_an_unqualified_file_still_collides() {
    // An unqualified file's caller package is `main`, which is how the
    // declaration walk already scopes it. The nested-subroutine supplement
    // must start from the same implicit package, or a collision in such a file
    // is missed and the member is minted with no boundary — asserting there is
    // no conflict when there is one.
    let code = concat!(
        "use Mojo::Base -base;\n",
        "has 'name';\n",
        "sub outer { sub name { 'explicit' } }\n",
    );
    let facts = only_facts(code);
    assert_eq!(member_names(&facts), ["name"]);
    assert_eq!(
        facts.members[0].explicit_method,
        MojoBaseExplicitMethodState::Collides,
        "the implicit `main` package must be scoped the same way in both walks"
    );
    assert!(facts.members[0].envelope.boundary.is_some());
}

#[test]
fn a_conditional_expression_context_declares_no_accessor() {
    // A `do { ... }` block runs once on its own, so it is not control flow —
    // but under a ternary branch or the right side of a short-circuit operator
    // it runs only conditionally. Extracting those would publish an accessor
    // that may never exist, the over-reporting direction this producer treats
    // as unsafe.
    let code = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "$ENV{X} ? do { has 'ternary_then'; } : do { has 'ternary_else'; };\n",
        "$ENV{X} && do { has 'right_of_and'; };\n",
        "$ENV{X} || do { has 'right_of_or'; };\n",
        "do { has 'plain_do'; };\n",
        "has 'always';\n",
    );
    let facts = only_facts(code);
    assert_eq!(
        member_names(&facts),
        ["plain_do", "always"],
        "a bare `do` block still declares; a conditional one does not"
    );
}

#[test]
fn a_fully_qualified_subroutine_still_collides() {
    // `sub App::name` names its own package regardless of what package is
    // current, so it shadows `App`'s accessor. Recording the full spelling as
    // the bare slot would make the lookup miss and mint the member with no
    // boundary.
    let qualified = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "has 'name';\n",
        "package Other;\n",
        "sub App::name { 'explicit' }\n",
    );
    let facts = only_facts(qualified);
    assert_eq!(member_names(&facts), ["name"]);
    assert_eq!(
        facts.members[0].explicit_method,
        MojoBaseExplicitMethodState::Collides,
        "a qualified sub declared under another package still collides"
    );

    // Control: the same qualified sub naming a different package must not
    // collide, so the lookup is not simply matching on the bare suffix.
    let elsewhere = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "has 'name';\n",
        "package Other;\n",
        "sub Unrelated::name { 'explicit' }\n",
    );
    let clean = only_facts(elsewhere);
    assert_eq!(clean.members[0].explicit_method, MojoBaseExplicitMethodState::None);
}

#[test]
fn a_repeated_attribute_name_mints_one_live_member() {
    // `Mojo::Base` installs each accessor into the same package slot with an
    // unconditional `monkey_patch`, so a repeated name leaves exactly one live
    // method. Minting two independent unbounded members would publish an
    // accessor that no longer exists and let a consumer select the stale
    // reader semantics.
    let code = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "has name => 'first';\n",
        "has name => 'second';\n",
        "has 'other';\n",
    );
    let facts = only_facts(code);
    assert_eq!(member_names(&facts), ["name", "other"], "one live member per slot");
    assert_eq!(facts.reader_results.len(), facts.members.len());
    assert_eq!(facts.setter_results.len(), facts.members.len());
    // The surviving declaration is the later one, so its anchor is the second
    // `has` statement.
    let start = facts.members[0].envelope.anchor.start_byte as usize;
    assert!(
        code[start..].starts_with("has name => 'second'"),
        "the later declaration is the live one, got {:?}",
        &code[start..start + 20.min(code.len() - start)]
    );
    // Two run-phase declarations execute in source order, so the winner is
    // determinate and the surviving fact needs no caveat.
    assert!(
        facts.members[0].envelope.boundary.is_none(),
        "a run-phase repeat resolves determinately"
    );
    assert!(facts.members[1].envelope.boundary.is_none());
}

#[test]
fn a_repeat_contested_across_phases_reports_an_undetermined_winner() {
    // A run-phase `has` executes after every phaser, so it wins here — but the
    // relative order of phasers is not modelled (`CHECK` blocks run in reverse
    // declaration order), so a contested slot involving one is best-effort and
    // must say so rather than present the survivor as settled.
    let code = concat!(
        "package App;
",
        "use Mojo::Base -base;
",
        "BEGIN { has(name => 'from_begin'); }
",
        "has name => 'from_run';
",
    );
    let facts = only_facts(code);
    assert_eq!(member_names(&facts), ["name"], "still one live member");
    let boundary = must_some(facts.members[0].envelope.boundary.clone());
    assert_eq!(
        boundary.kind,
        BoundaryKind::CompileTimeExecution,
        "a phaser in the contest makes the survivor undetermined"
    );
    let start = facts.members[0].envelope.anchor.start_byte as usize;
    assert!(
        code[start..].starts_with("has name => 'from_run'"),
        "the run-phase declaration executes last and is the best-effort survivor"
    );
}

#[test]
fn an_undetermined_slot_degrades_the_reader_but_not_the_write_contract() {
    // When the surviving declaration for a slot is a best-effort choice, its
    // default proves nothing about what a read yields — the live accessor may
    // be a different declaration with a different default. The reader must not
    // present that shape as established.
    //
    // The setter is deliberately unaffected: every `Mojo::Base` accessor
    // returns the invocant on write whichever declaration won, so the
    // `ReceiverSelf` relation stays exact. The uncertainty is about the value,
    // not the write contract.
    let contested = concat!(
        "package App;\n",
        "use Mojo::Base -base;\n",
        "CHECK { has(name => 'from_check'); }\n",
        "INIT { has(name => 'from_init'); }\n",
    );
    let facts = only_facts(contested);
    assert_eq!(member_names(&facts), ["name"]);
    assert_eq!(
        facts.reader_results[0].relation,
        CallableResultRelation::Unknown,
        "a best-effort survivor cannot claim its default's value shape"
    );
    assert_eq!(
        facts.setter_results[0].relation,
        CallableResultRelation::ReceiverSelf,
        "the write contract holds whichever declaration won"
    );

    // Control: an uncontested constant default still yields a concrete shape,
    // so the degradation above isolates the contest rather than always firing.
    let settled = "package App;\nuse Mojo::Base -base;\nhas name => 'anon';\n";
    let settled_facts = only_facts(settled);
    assert_eq!(
        settled_facts.reader_results[0].relation,
        CallableResultRelation::Concrete(ValueShape::Scalar)
    );
}
