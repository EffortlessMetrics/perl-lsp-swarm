#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! Integration falsifiers for the canonical Dancer2 hook fact chain (#8924).
//!
//! Full chain over one source: parse → activation sites → hook declarations
//! → registry detection → exact activation facts → minted canonical facts.
//! Mirrors the #8918 route falsifiers; the discriminating seam of #8924 is
//! the handler relation promotion (static `\&handler` coderefs resolve to
//! exact in-file declaration identities) and the reviewed alias contract.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::dancer2_activation::extract_dancer2_activation_sites;
use perl_semantic_analyzer::analysis::dancer2_hooks::extract_dancer2_hook_declarations;
use perl_semantic_facts::framework::{
    AdapterCancellation, AdapterDetectionInput, DetectionEvidenceClass, ModuleActivationIdentity,
    ModuleObservationReceipt, ModuleSelectorEvaluation, ModuleSelectorOutcome,
    ModuleVersionEvidence,
};
use perl_semantic_facts::framework_adapters::dancer2::{
    dancer2_activation_facts, dancer2_descriptor, detect_dancer2,
};
use perl_semantic_facts::framework_adapters::dancer2_hooks::dancer2_hook_facts;
use perl_semantic_facts::handler::{FrameworkHandler, FrameworkHandlerBoundary};
use perl_semantic_facts::hook::{
    HookFact, HookNameNormalization, HookNameSelection, hook_fact_identity,
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

fn canonical_facts(code: &str, generation: &str) -> Vec<HookFact> {
    canonical_facts_with_input(code, &input(generation))
}

fn canonical_facts_with_input(
    code: &str,
    detection_input: &AdapterDetectionInput,
) -> Vec<HookFact> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let sites = extract_dancer2_activation_sites(&ast, FileId(1));
    let declarations = extract_dancer2_hook_declarations(&ast, FileId(1), code);
    let detection = detect_dancer2(detection_input);
    let mut facts = Vec::new();
    for site in &sites {
        let activation =
            dancer2_activation_facts(&detection, site.package.as_deref(), &site.evidence);
        facts.extend(dancer2_hook_facts(
            &detection,
            &activation,
            site.package.as_deref(),
            &declarations,
        ));
    }
    facts
}

fn literal_name(fact: &HookFact) -> &perl_semantic_facts::hook::HookName {
    must_some(match &fact.hook.name {
        HookNameSelection::Literal(name) => Some(name),
        HookNameSelection::Dynamic { .. } => None,
        _ => None,
    })
}

fn canonical_name(fact: &HookFact) -> &str {
    must_some(literal_name(fact).canonical())
}

// Falsifier 1: a canonical application hook mints exactly one exact fact with
// every range anchored at exact tokens.
#[test]
fn canonical_hook_name_mints_one_exact_anchored_fact() {
    let code = "package MyApp;\nuse Dancer2;\nhook 'core.app.before_request' => sub { 1 };";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 1);
    let fact = &facts[0];
    assert_eq!(fact.hook.keyword, "hook");
    assert_eq!(canonical_name(fact), "core.app.before_request");
    assert_eq!(literal_name(fact).normalization, HookNameNormalization::Canonical);
    assert_eq!(fact.application_name, "MyApp");
    assert_eq!(fact.envelope.package.as_deref(), Some("MyApp"));
    assert_eq!(fact.framework_name, "Dancer2");
    assert_eq!(fact.envelope.source_generation, SourceGeneration::known("gen-1"));
    assert_eq!(fact.status(), SemanticFactStatus::Exact);
    assert!(fact.envelope.boundary.is_none());

    // Anchors point at exact tokens.
    let name = literal_name(fact);
    assert_eq!(
        &code[name.anchor.start_byte as usize..name.anchor.end_byte as usize],
        "'core.app.before_request'"
    );
    let keyword = fact.hook.keyword_anchor;
    assert_eq!(&code[keyword.start_byte as usize..keyword.end_byte as usize], "hook");
    let anchor = must_some(match &fact.hook.handler {
        FrameworkHandler::InlineSub { anchor } => Some(*anchor),
        _ => None,
    });
    assert_eq!(&code[anchor.start_byte as usize..anchor.end_byte as usize], "sub { 1 }");
    let declaration = fact.envelope.anchor;
    assert_eq!(
        &code[declaration.start_byte as usize..declaration.end_byte as usize],
        "hook 'core.app.before_request' => sub { 1 }"
    );
}

// Falsifier 2: reviewed aliases normalize deterministically with explicit
// version authority; the literal name stays alongside the canonical name.
#[test]
fn reviewed_aliases_normalize_deterministically() {
    let code =
        "package App;\nuse Dancer2;\nhook 'before' => sub { 1 };\nhook 'init_error' => sub { 2 };";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 2);
    assert_eq!(literal_name(&facts[0]).literal, "before");
    assert_eq!(canonical_name(&facts[0]), "core.app.before_request");
    assert_eq!(literal_name(&facts[1]).literal, "init_error");
    assert_eq!(canonical_name(&facts[1]), "core.error.init");
    for fact in &facts {
        assert_eq!(
            literal_name(fact).normalization,
            HookNameNormalization::Alias { canonical: canonical_name(fact).to_string() }
        );
        assert_eq!(fact.status(), SemanticFactStatus::Exact);
    }
}

// Falsifier 3: the #8924 handler promotion — a static `\&handler` coderef
// resolves to the exact in-file declaration identity (including forward
// declarations) and keeps the fact exact.
#[test]
fn static_coderef_handler_resolves_to_declaration_identity() {
    let code = "package App;\nuse Dancer2;\nhook 'after' => \\&teardown;\nsub teardown { 'x' }";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 1);
    let fact = &facts[0];
    let (name, target, anchor) = must_some(match &fact.hook.handler {
        FrameworkHandler::StaticCoderef { name, target, anchor } => {
            Some((name.as_str(), target, *anchor))
        }
        _ => None,
    });
    assert_eq!(name, "teardown");
    assert_eq!(target.name, "teardown");
    assert_eq!(target.package, "App");
    assert_eq!(&code[anchor.start_byte as usize..anchor.end_byte as usize], "\\&teardown");
    assert_eq!(
        &code[target.name_anchor.start_byte as usize..target.name_anchor.end_byte as usize],
        "teardown",
        "exact declaration identity, never a fictional body"
    );
    let body = must_some(target.body_anchor.as_ref());
    assert_eq!(&code[body.start_byte as usize..body.end_byte as usize], "{ 'x' }");
    assert_eq!(fact.status(), SemanticFactStatus::Exact);
}

// Falsifier 4: an unresolvable coderef and computed handlers stay typed
// boundaries with reasons — the fact is retained but degraded.
#[test]
fn dynamic_and_unresolved_handlers_stay_boundaries() {
    let code = "package App;\nuse Dancer2;\nhook 'before' => \\&missing;\nhook 'after' => $code;\nhook 'init_error' => 'plain_string';";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 3, "every declaration is retained");
    let expected = [
        FrameworkHandlerBoundary::StaticCoderef,
        FrameworkHandlerBoundary::Computed,
        FrameworkHandlerBoundary::String,
    ];
    for (fact, boundary) in facts.iter().zip(expected) {
        let (found, reason) = must_some(match &fact.hook.handler {
            FrameworkHandler::Bounded { boundary, reason, .. } => {
                Some((*boundary, reason.as_str()))
            }
            _ => None,
        });
        assert_eq!(found, boundary);
        assert!(!reason.is_empty(), "every boundary carries a reason");
        assert_eq!(fact.status(), SemanticFactStatus::Degraded);
        assert!(fact.envelope.boundary.is_some(), "degraded facts carry a boundary link");
    }
}

// Falsifier 5: ZERO hook facts without registry activation — ordinary
// `hook()` without Dancer2, name-only detection, `use Dancer2::Core` only,
// and `!hook` import exclusion all mint nothing.
#[test]
fn zero_hook_facts_without_exact_activation() {
    // No `use Dancer2` at all: extraction is observable but nothing mints.
    let plain = "hook 'before' => sub { 1 };";
    assert!(canonical_facts(plain, "gen-1").is_empty());

    // `use Dancer2::Core` never activates the adapter (selector is `Dancer2`).
    let core_only = "package App;\nuse Dancer2::Core;\nhook 'before' => sub { 1 };";
    assert!(canonical_facts(core_only, "gen-1").is_empty());

    // Detection without a resolved Dancer2 module mints nothing.
    let dancer2_looking = "package App;\nuse Dancer2;\nhook 'before' => sub { 1 };";
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

    // `!hook` at the activating import: the keyword was never imported.
    let excluded = "package App;\nuse Dancer2 qw(!hook);\nhook 'before' => sub { 1 };";
    assert!(canonical_facts(excluded, "gen-1").is_empty());
}

// Falsifier 6: computed names and plugin/runtime hook names are typed
// boundaries — dotted spelling never proves ownership.
#[test]
fn computed_and_plugin_names_stay_boundaries() {
    let code = "package App;\nuse Dancer2;\nhook $name => sub { 1 };\nhook 'plugin.database.before_dbi_connect' => sub { 2 };";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 2);
    assert!(matches!(&facts[0].hook.name, HookNameSelection::Dynamic { .. }));
    let plugin = &facts[1];
    assert_eq!(literal_name(plugin).literal, "plugin.database.before_dbi_connect");
    assert!(literal_name(plugin).canonical().is_none());
    assert_eq!(plugin.status(), SemanticFactStatus::Degraded);
}

// A bareword before the fat comma is auto-quoted by Perl, so it is a literal
// hook name, not a computed one. `hook before => sub {...}` is the canonical
// Dancer2 spelling and must reach the same identity as the quoted form.
#[test]
fn fat_comma_barewords_are_literal_hook_names() {
    let code = "package App;\nuse Dancer2;\nhook before => sub { 1 };\nhook 'before' => sub { 2 };\nhook before_request => sub { 3 };";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 3);
    for fact in &facts {
        assert_eq!(canonical_name(fact), "core.app.before_request");
        assert_eq!(fact.status(), SemanticFactStatus::Exact);
    }
    // The bareword and the quoted spelling agree on the literal too.
    assert_eq!(literal_name(&facts[0]).literal, "before");
    assert_eq!(literal_name(&facts[1]).literal, "before");
    assert_eq!(literal_name(&facts[2]).literal, "before_request");
}

// The separator, not the node shape, decides. `hook(before, sub {...})` calls
// `before()` and passes its result — no auto-quoting happens — so it must stay
// computed even though the operand is the same bareword token as the promoted
// form. Accepting the AST node kind alone would wrongly make this a literal
// and hand the handler body a request-context admission it has not earned.
#[test]
fn a_comma_separated_bareword_operand_is_never_promoted() {
    let promoted = "package App;\nuse Dancer2;\nhook before => sub { 1 };";
    let called = "package App;\nuse Dancer2;\nhook(before, sub { 1 });";

    let promoted = canonical_facts(promoted, "gen-1");
    assert_eq!(promoted.len(), 1);
    assert_eq!(canonical_name(&promoted[0]), "core.app.before_request");
    assert_eq!(promoted[0].status(), SemanticFactStatus::Exact);

    let called = canonical_facts(called, "gen-1");
    assert_eq!(called.len(), 1);
    assert!(
        matches!(&called[0].hook.name, HookNameSelection::Dynamic { .. }),
        "a comma-separated bareword calls a sub; it is not an auto-quoted name"
    );
    assert_eq!(called[0].status(), SemanticFactStatus::Degraded);
}

// The fat comma may be separated by whitespace or a newline, and the parser
// already auto-quotes it inside a parenthesised call. Both spellings must
// reach the same literal as the paren-less form.
#[test]
fn fat_comma_promotion_is_independent_of_spacing_and_parentheses() {
    for code in [
        "package App;\nuse Dancer2;\nhook before=>sub { 1 };",
        "package App;\nuse Dancer2;\nhook before\n    => sub { 1 };",
        "package App;\nuse Dancer2;\nhook(before => sub { 1 });",
    ] {
        let facts = canonical_facts(code, "gen-1");
        assert_eq!(facts.len(), 1, "{code}");
        assert_eq!(canonical_name(&facts[0]), "core.app.before_request", "{code}");
    }
}

// The auto-quoting rule is bounded: only a bareword is a literal. A variable
// or a call is still a computed operand, and an unreviewed bareword stays an
// unresolved boundary rather than becoming a guessed canonical name.
#[test]
fn only_barewords_are_promoted_and_unreviewed_ones_stay_boundaries() {
    let code = "package App;\nuse Dancer2;\nhook $name => sub { 1 };\nhook pick_name() => sub { 2 };\nhook some_plugin_position => sub { 3 };";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 3);
    assert!(
        matches!(&facts[0].hook.name, HookNameSelection::Dynamic { .. }),
        "a variable operand is computed"
    );
    assert!(
        matches!(&facts[1].hook.name, HookNameSelection::Dynamic { .. }),
        "a call operand is computed"
    );
    // A bareword outside the reviewed contract is a literal name, but its
    // ownership is still unproven: no canonical identity is invented.
    assert_eq!(literal_name(&facts[2]).literal, "some_plugin_position");
    assert!(literal_name(&facts[2]).canonical().is_none());
    assert!(matches!(
        literal_name(&facts[2]).normalization,
        HookNameNormalization::Unresolved { .. }
    ));
    assert_eq!(facts[2].status(), SemanticFactStatus::Degraded);
}

// Falsifier 7: malformed/incomplete hook calls mint nothing.
#[test]
fn malformed_hook_calls_mint_nothing() {
    let code =
        "package App;\nuse Dancer2;\nhook;\nhook 'before';\nhook 'before', sub { 1 }, sub { 2 };";
    assert!(canonical_facts(code, "gen-1").is_empty());
}

// Falsifier 8: same alias in distinct apps/roots stays isolated — distinct
// applications share nothing, and different generations never collide.
#[test]
fn same_alias_hooks_stay_isolated_per_app_and_generation() {
    let code = "package App1;\nuse Dancer2;\nhook 'before' => sub { 1 };\npackage App2;\nuse Dancer2;\nhook 'before' => sub { 2 };";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 2);
    assert_eq!(facts[0].application_name, "App1");
    assert_eq!(facts[1].application_name, "App2");
    assert_ne!(facts[0].envelope.fact_id, facts[1].envelope.fact_id);
    assert_ne!(facts[0].envelope.entity_id, facts[1].envelope.entity_id);
    assert_ne!(facts[0].envelope.package.as_deref(), facts[1].envelope.package.as_deref());

    // Different roots/edits mint over different generation-owned identities.
    let (gen1_fact, _) = hook_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
    let (gen2_fact, _) = hook_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-2"));
    assert_ne!(gen1_fact, gen2_fact);
    let (other_file, _) = hook_fact_identity(FileId(2), 0, &SourceGeneration::known("gen-1"));
    assert_ne!(gen1_fact, other_file);
}

// Falsifier 9: currentness — editing/removing the hook declaration at a new
// generation replaces the old facts; a stale held result cannot re-mint.
#[test]
fn edit_and_removal_replaces_facts_at_the_new_generation() {
    let before = "package App;\nuse Dancer2;\nhook 'before' => sub { 1 };";
    let first = canonical_facts(before, "gen-1");
    assert_eq!(first.len(), 1);
    assert_eq!(canonical_name(&first[0]), "core.app.before_request");
    let first_id = first[0].envelope.fact_id;

    // Edit: alias → canonical spelling and a different handler.
    let edited = "package App;\nuse Dancer2;\nhook 'after' => \\&done;\nsub done { 1 }";
    let second = canonical_facts(edited, "gen-2");
    assert_eq!(second.len(), 1);
    assert_eq!(canonical_name(&second[0]), "core.app.after_request");
    assert!(matches!(second[0].hook.handler, FrameworkHandler::StaticCoderef { .. }));
    assert_ne!(first_id, second[0].envelope.fact_id, "edit mints a new identity");
    assert_eq!(second[0].envelope.source_generation, SourceGeneration::known("gen-2"));

    // Removal: the new generation mints nothing; the stale held result is not
    // re-minted (generation-owned identity never reproduces).
    let removed = "package App;\nuse Dancer2;";
    assert!(canonical_facts(removed, "gen-3").is_empty());
}

// Falsifier 10: hook facts round-trip through the JSON transport with the
// constructor re-checking every invariant on decode.
#[test]
fn hook_facts_round_trip_through_json_transport() -> Result<(), serde_json::Error> {
    let code = "package App;\nuse Dancer2;\nhook 'before' => \\&on_before;\nsub on_before { 1 }";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 1);
    let serialized = serde_json::to_string(&facts)?;
    let decoded: Vec<HookFact> = serde_json::from_str(&serialized)?;
    assert_eq!(decoded, facts);
    assert_eq!(decoded[0].status(), SemanticFactStatus::Exact);
    assert_eq!(
        decoded[0].envelope.confidence,
        perl_semantic_facts::SemanticConfidence::Known(Confidence::High)
    );
    Ok(())
}

// Falsifier 11: hook declarations inside sub bodies are execution-conditional
// and never mint at load time.
#[test]
fn hooks_inside_sub_bodies_mint_nothing() {
    let code = "package App;\nuse Dancer2;\nsub deferred { hook 'before' => sub { 1 }; }\nhook 'after' => sub { 2 };";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 1);
    assert_eq!(canonical_name(&facts[0]), "core.app.after_request");
}
