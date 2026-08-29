#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! End-to-end canonical Dancer2 route fact proof (#8918).
//!
//! Drives the full #8918 chain — AST route extraction
//! (`perl_semantic_analyzer::analysis::dancer2_routes`) over the registry
//! activation seam (#8914) into `dancer2_route_facts` minting — and
//! shadow-compares the canonical facts with the legacy route-path `Subroutine`
//! synthesis (`frameworks_web`), classifying the intended deltas. The legacy
//! extractor itself is untouched and remains the live provider path until the
//! #8928 cutover.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::dancer2_activation::extract_dancer2_activation_sites;
use perl_semantic_analyzer::analysis::dancer2_routes::{
    extract_dancer2_route_contexts, extract_dancer2_route_declarations,
};
use perl_semantic_analyzer::symbol::{SymbolExtractor, SymbolKind, SymbolTable};
use perl_semantic_facts::framework::{
    AdapterCancellation, AdapterDetectionInput, AdapterDetectionResult, DetectionEvidenceClass,
    ModuleActivationIdentity, ModuleObservationReceipt, ModuleSelectorEvaluation,
    ModuleSelectorOutcome, ModuleVersionEvidence,
};
use perl_semantic_facts::framework_adapters::dancer2::{
    dancer2_activation_facts, dancer2_descriptor, detect_dancer2, parse_dancer2_import_args,
};
use perl_semantic_facts::framework_adapters::dancer2_routes::{
    Dancer2RouteDeclaration, Dancer2RouteFacts, dancer2_route_facts, dancer2_route_family_facts,
};
use perl_semantic_facts::route::{
    RouteEffectivePattern, RouteFact, RouteHandler, RouteHandlerContextFact, RouteMethodSet,
    RouteNameSelection, RouteOptions, RouteParameterFact, RouteParameterKind, RoutePatternKind,
    RoutePrefixFact, route_fact_identity,
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

/// Full chain over one source: parse → activation sites → route declarations
/// → registry detection → exact activation facts → minted canonical facts.
fn canonical_facts(code: &str, generation: &str) -> Vec<RouteFact> {
    canonical_facts_with_input(code, &input(generation))
}

fn canonical_facts_with_input(
    code: &str,
    detection_input: &AdapterDetectionInput,
) -> Vec<RouteFact> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let sites = extract_dancer2_activation_sites(&ast, FileId(1));
    let declarations = extract_dancer2_route_declarations(&ast, FileId(1));
    let detection = detect_dancer2(detection_input);
    let mut facts = Vec::new();
    for site in &sites {
        let activation =
            dancer2_activation_facts(&detection, site.package.as_deref(), &site.evidence);
        facts.extend(dancer2_route_facts(
            &detection,
            &activation,
            site.package.as_deref(),
            &declarations,
        ));
    }
    facts
}

fn legacy_symbols(code: &str) -> SymbolTable {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    SymbolExtractor::new_with_source(code).extract(&ast)
}

fn legacy_route_symbol<'a>(
    table: &'a SymbolTable,
    name: &str,
) -> Option<&'a perl_semantic_analyzer::symbol::Symbol> {
    table
        .symbols
        .get(name)
        .and_then(|symbols| symbols.iter().find(|symbol| symbol.kind == SymbolKind::Subroutine))
}

fn exact_methods(fact: &RouteFact) -> Vec<String> {
    must_some(match &fact.route.methods {
        RouteMethodSet::Exact(methods) => Some(methods.clone()),
        _ => None,
    })
}

fn inline_handler_anchor(fact: &RouteFact) -> perl_semantic_facts::SourceAnchor {
    must_some(match &fact.route.handler {
        RouteHandler::InlineSub { anchor } => Some(*anchor),
        _ => None,
    })
}

fn literal_name_value(fact: &RouteFact) -> String {
    must_some(match &fact.route.route_name {
        RouteNameSelection::Literal(name) => Some(name.value.clone()),
        _ => None,
    })
}

fn option_entries(fact: &RouteFact) -> &Vec<perl_semantic_facts::route::RouteOption> {
    must_some(match &fact.route.options {
        RouteOptions::Map(entries) => Some(entries),
        _ => None,
    })
}

// Falsifier 1: `get '/path' => sub {...}` mints exactly one RouteFact with
// verb/pattern/target anchored at exact tokens.
#[test]
fn simple_get_app_mints_exactly_one_anchored_route_fact() {
    let code = "package MyApp;\nuse Dancer2;\nget '/x' => sub { 1 };\n";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 1, "exactly one route fact per declaration");
    let fact = &facts[0];
    assert_eq!(fact.route.keyword, "get");
    assert_eq!(fact.route.pattern.kind, RoutePatternKind::Literal);
    assert_eq!(fact.route.pattern.value.as_deref(), Some("/x"));
    assert!(matches!(fact.route.handler, RouteHandler::InlineSub { .. }));
    assert_eq!(exact_methods(fact), vec!["GET".to_string(), "HEAD".to_string()]);
    assert_eq!(fact.application_name, "MyApp");
    assert_eq!(fact.envelope.package.as_deref(), Some("MyApp"));
    assert_eq!(fact.status(), SemanticFactStatus::Exact);

    // Anchors point at exact tokens.
    let anchor = fact.route.pattern.anchor;
    assert_eq!(&code[anchor.start_byte as usize..anchor.end_byte as usize], "'/x'");
    let handler = inline_handler_anchor(fact);
    assert_eq!(&code[handler.start_byte as usize..handler.end_byte as usize], "sub { 1 }");
    let keyword = fact.route.keyword_anchor;
    assert_eq!(&code[keyword.start_byte as usize..keyword.end_byte as usize], "get");
}

// Falsifier 2: ZERO route facts without registry activation — a
// Dancer2-looking file in an unactivated context mints nothing.
#[test]
fn zero_route_facts_without_registry_activation() {
    let dancer2_looking = "package App;\nuse Dancer2;\nget '/x' => sub { 1 };\n";

    // No resolved Dancer2 module: detection is absent.
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

    // Name-only identity: detection is explicitly not exact.
    let name_only_activation =
        ModuleActivationIdentity::new("Dancer2", Some(FileId(7)), SourceGeneration::known("gen-1"));
    let name_only_observation = ModuleObservationReceipt::new(
        "module-resolver.v1",
        "root:fixture",
        "project-environment.v1",
        SourceGeneration::known("gen-1"),
        "sha256:fixture-input",
        vec![ModuleSelectorEvaluation::new(
            "Dancer2",
            ModuleSelectorOutcome::Matched {
                activation: name_only_activation,
                evidence_class: DetectionEvidenceClass::NameOnly,
            },
        )],
    );
    let name_only_input = AdapterDetectionInput::new(
        dancer2_descriptor(),
        name_only_observation,
        None,
        AdapterCancellation::active(),
    );
    assert!(canonical_facts_with_input(dancer2_looking, &name_only_input).is_empty());
}

// Falsifier 3: the dancer2_skeleton corpus fixture round-trips through the
// canonical chain and the transport contract.
#[test]
fn dancer2_skeleton_corpus_round_trips() -> Result<(), serde_json::Error> {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test_corpus/real_projects/dancer2_skeleton/t/basic.t"
    );
    let code = must(std::fs::read_to_string(fixture));

    let mut parser = Parser::new(&code);
    let ast = must(parser.parse());
    let sites = extract_dancer2_activation_sites(&ast, FileId(11));
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].package.as_deref(), Some("MyApp"));

    let declarations = extract_dancer2_route_declarations(&ast, FileId(11));
    assert_eq!(declarations.len(), 1, "fixture carries one `get '/' => sub` route");
    assert_eq!(declarations[0].package.as_deref(), Some("MyApp"));
    assert_eq!(declarations[0].route.pattern.value.as_deref(), Some("/"));

    let detection = detect_dancer2(&input("corpus-gen-1"));
    let activation =
        dancer2_activation_facts(&detection, sites[0].package.as_deref(), &sites[0].evidence);
    assert!(activation.is_exact());
    let facts =
        dancer2_route_facts(&detection, &activation, sites[0].package.as_deref(), &declarations);
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].status(), SemanticFactStatus::Exact);

    let serialized = serde_json::to_string(&facts)?;
    let decoded: Vec<RouteFact> = serde_json::from_str(&serialized)?;
    assert_eq!(decoded, facts, "route facts round-trip through the transport");

    // The family chain over the same corpus: the fixture's `/` pattern has no
    // prefix, no parameters, and one inline handler interval.
    let contexts = extract_dancer2_route_contexts(&ast, FileId(11));
    assert!(contexts.prefixes.is_empty(), "the skeleton fixture carries no prefix");
    let family = dancer2_route_family_facts(
        &detection,
        &activation,
        sites[0].package.as_deref(),
        &contexts.routes,
        &contexts.prefixes,
    );
    assert_eq!(family.routes.len(), 1);
    assert!(family.parameters.is_empty(), "`/` has no parameter segments");
    assert_eq!(family.handler_contexts.len(), 1);
    assert_eq!(family.handler_contexts[0].status(), SemanticFactStatus::Exact);
    let handler = family.handler_contexts[0].envelope.anchor;
    assert_eq!(
        &code[handler.start_byte as usize..handler.end_byte as usize],
        "sub { 'Hello World' }"
    );
    Ok(())
}

// Falsifier 4 (#8928 retirement): containment still holds, the admitted
// forms are retired to canonical facts, and the unadmitted forms keep the
// legacy path with the recorded boundary.
#[test]
fn legacy_extractor_retirement_boundary() {
    // Containment: no `use Dancer2`, no legacy route symbol.
    let unactivated = "package App;
get '/hello' => sub { 'x' };
";
    let table = legacy_symbols(unactivated);
    assert!(
        legacy_route_symbol(&table, "/hello").is_none(),
        "bare `get` without `use Dancer2` must not produce a legacy route symbol"
    );

    // Admitted form: retired — the canonical facts own the route identity.
    let activated = "package App;
use Dancer2;
get '/hello' => sub { 'x' };
";
    let table = legacy_symbols(activated);
    assert!(
        legacy_route_symbol(&table, "/hello").is_none(),
        "the admitted form must not synthesize a legacy route-path symbol (#8928)"
    );

    // Unadmitted form (excluded keyword): the legacy path keeps it.
    let excluded = "package App;
use Dancer2 '!get';
get '/hello' => sub { 'x' };
";
    let table = legacy_symbols(excluded);
    let symbol = must_some(legacy_route_symbol(&table, "/hello"));
    assert!(symbol.attributes.iter().any(|attr| attr == "http_method=GET"));

    // Unadmitted form (bare `delete`, not a Dancer2 keyword): legacy keeps it.
    let bare_delete = "package App;
use Dancer2;
delete '/x' => sub { 1 };
";
    let table = legacy_symbols(bare_delete);
    let symbol = must_some(legacy_route_symbol(&table, "/x"));
    assert!(symbol.attributes.iter().any(|attr| attr == "http_method=DELETE"));
}

// Shadow parity (#8918 receipts, post-#8928 retirement): the canonical
// values are unchanged; the legacy route-path symbols for the admitted
// forms are retired. Parity over these forms was proven before retirement
// landed (frozen legacy oracle: `dancer2_provider_cutover_parity.rs`).
#[test]
fn shadow_parity_classifies_intended_deltas() {
    // Exact-equivalent simple literal route: canonical keeps the pattern;
    // the admitted form is retired from the legacy path.
    let simple = "package App;\nuse Dancer2;\nget '/x' => sub { 1 };\n";
    let facts = canonical_facts(simple, "gen-1");
    let table = legacy_symbols(simple);
    assert_eq!(facts.len(), 1);
    assert!(legacy_route_symbol(&table, "/x").is_none(), "admitted form retired (#8928)");
    assert_eq!(facts[0].route.pattern.value.as_deref(), Some("/x"));

    // Intended GET→HEAD enrichment over the retired legacy GET-only value.
    assert!(exact_methods(&facts[0]).contains(&"HEAD".to_string()));

    // Intended named-route correction: the retired legacy path named the
    // symbol by the route NAME and lost the pattern; canonical keeps both
    // distinct.
    let named = "package App;\nuse Dancer2;\nget 'user_show', '/users/:id', sub { 1 };\n";
    let facts = canonical_facts(named, "gen-1");
    let table = legacy_symbols(named);
    assert_eq!(facts.len(), 1);
    assert_eq!(literal_name_value(&facts[0]), "user_show");
    assert_eq!(facts[0].route.pattern.value.as_deref(), Some("/users/:id"));
    assert!(
        legacy_route_symbol(&table, "user_show").is_none(),
        "the admitted named form is retired (#8928); canonical keeps name and pattern distinct"
    );
    assert!(legacy_route_symbol(&table, "/users/:id").is_none());

    // Intended `any` method-set correction over the retired legacy ANY value.
    let any_route = "package App;\nuse Dancer2;\nany '/multi' => sub { 1 };\n";
    let facts = canonical_facts(any_route, "gen-1");
    let table = legacy_symbols(any_route);
    assert!(legacy_route_symbol(&table, "/multi").is_none(), "admitted `any` is retired (#8928)");
    let methods = exact_methods(&facts[0]);
    assert!(!methods.iter().any(|method| method == "ANY"));
    assert_eq!(methods.len(), 7, "bare any records the reviewed default set");

    // Intended `options` coverage: the legacy verb table never covered
    // `options`; canonical does, and the admitted form is retired.
    let options_route = "package App;\nuse Dancer2;\noptions '/x' => sub { 1 };\n";
    let facts = canonical_facts(options_route, "gen-1");
    let table = legacy_symbols(options_route);
    assert_eq!(facts.len(), 1);
    assert_eq!(exact_methods(&facts[0]), vec!["OPTIONS".to_string()]);
    assert!(
        legacy_route_symbol(&table, "/x").is_none(),
        "admitted `options` is retired (#8928); pre-retirement legacy never covered it either"
    );

    // Intended bare-`delete` rejection: legacy accepts it, canonical does not.
    let bare_delete = "package App;\nuse Dancer2;\ndelete '/x' => sub { 1 };\n";
    let facts = canonical_facts(bare_delete, "gen-1");
    let table = legacy_symbols(bare_delete);
    assert!(facts.is_empty(), "canonical rejects bare `delete`");
    let symbol = must_some(legacy_route_symbol(&table, "/x"));
    assert!(symbol.attributes.iter().any(|attr| attr == "http_method=DELETE"));

    // Dynamic boundary: computed method list stays a boundary, not ANY.
    let dynamic = "package App;\nuse Dancer2;\nany $methods => '/x' => sub { 1 };\n";
    let facts = canonical_facts(dynamic, "gen-1");
    assert_eq!(facts.len(), 1);
    assert!(matches!(facts[0].route.methods, RouteMethodSet::Dynamic { .. }));
    assert_eq!(facts[0].status(), SemanticFactStatus::Degraded);

    // Stale/ambiguous activation rejection: without activation neither path
    // synthesizes routes.
    let unactivated = "package App;\nany '/x' => sub { 1 };\n";
    assert!(
        legacy_route_symbol(&legacy_symbols(unactivated), "/x").is_none(),
        "legacy containment (#8910) still gates on exact activation"
    );
}

// Negative/boundary fixtures from the issue.
#[test]
fn excluded_verb_through_import_exclusion_mints_nothing() {
    let code =
        "package App;\nuse Dancer2 qw(!get);\nget '/x' => sub { 1 };\npost '/y' => sub { 1 };\n";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 1, "only the imported verb mints");
    assert_eq!(facts[0].route.keyword, "post");
    assert_eq!(facts[0].route.pattern.value.as_deref(), Some("/y"));
}

#[test]
fn dynamic_boundaries_stay_degraded_not_exact() {
    let code = "package App;\nuse Dancer2;\nget $path => sub { 1 };\nget '/s' => 'str_handler';\n";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 2);
    assert_eq!(facts[0].route.pattern.kind, RoutePatternKind::Dynamic);
    assert_eq!(facts[0].status(), SemanticFactStatus::Degraded);
    // String handlers are not exact Dancer2 subroutine targets.
    assert!(matches!(facts[1].route.handler, RouteHandler::Bounded { .. }));
    assert_eq!(facts[1].status(), SemanticFactStatus::Degraded);
    assert!(facts.iter().all(|fact| fact.route.route_name_literal_value().is_none()));
}

#[test]
fn same_route_in_two_packages_mints_through_own_activations_only() {
    let code =
        "package A;\nuse Dancer2;\nget '/x' => sub { 1 };\npackage B;\nget '/y' => sub { 1 };\n";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 1, "only the activated package's routes mint");
    assert_eq!(facts[0].envelope.package.as_deref(), Some("A"));
    assert_eq!(facts[0].route.pattern.value.as_deref(), Some("/x"));
}

#[test]
fn two_roots_with_same_source_stay_generation_isolated() {
    let code = "package App;\nuse Dancer2;\nget '/x' => sub { 1 };\n";
    let root_a = canonical_facts(code, "gen-a");
    let root_b = canonical_facts(code, "gen-b");
    assert_eq!(root_a.len(), 1);
    assert_eq!(root_b.len(), 1);
    assert_eq!(root_a[0].envelope.source_generation, SourceGeneration::known("gen-a"));
    assert_eq!(root_b[0].envelope.source_generation, SourceGeneration::known("gen-b"));
    assert_ne!(root_a[0].envelope.fact_id, root_b[0].envelope.fact_id);
}

#[test]
fn edit_and_rebuild_mints_fresh_generation_facts() {
    let before = "package App;\nuse Dancer2;\nget '/x' => sub { 1 };\n";
    let after = "package App;\nuse Dancer2;\nget '/y' => sub { 1 };\n";

    let gen1 = canonical_facts(before, "gen-1");
    let gen2 = canonical_facts(after, "gen-2");
    assert_eq!(gen1.len(), 1);
    assert_eq!(gen2.len(), 1);
    assert_eq!(gen1[0].route.pattern.value.as_deref(), Some("/x"));
    assert_eq!(gen2[0].route.pattern.value.as_deref(), Some("/y"));
    // The edited declaration's fact identity changes with the new generation;
    // a held gen-1 fact can no longer represent the current source.
    assert_ne!(gen1[0].envelope.fact_id, gen2[0].envelope.fact_id);
    assert_ne!(gen1[0].envelope.source_generation, gen2[0].envelope.source_generation);
}

#[test]
fn duplicate_looking_routes_stay_distinct_entities() {
    let code = "package App;\nuse Dancer2;\nget '/x' => sub { 1 };\nget '/x' => sub { 2 };\n";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 2);
    assert_ne!(facts[0].envelope.fact_id, facts[1].envelope.fact_id);
    assert_ne!(facts[0].envelope.entity_id, facts[1].envelope.entity_id);
    assert_ne!(facts[0].route.declaration_index, facts[1].route.declaration_index);
}

#[test]
fn static_options_preserve_exact_operand_ranges() {
    let code = "package App;\nuse Dancer2;\nget 'user_show', '/users/:id', { agent => 'curl' }, sub { 1 };\n";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 1);
    let entries = option_entries(&facts[0]);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, "agent");
    assert_eq!(
        &code[entries[0].key_anchor.start_byte as usize..entries[0].key_anchor.end_byte as usize],
        "agent"
    );
    assert_eq!(
        &code[entries[0].value_anchor.start_byte as usize
            ..entries[0].value_anchor.end_byte as usize],
        "'curl'"
    );
    assert_eq!(facts[0].status(), SemanticFactStatus::Exact);
}

#[test]
fn regex_route_from_two_statement_form_mints_regex_kind() {
    let code = "package App;\nuse Dancer2;\nget qr{^/re/(\\d+)$} => sub { 1 };\n";
    let facts = canonical_facts(code, "gen-1");
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].route.pattern.kind, RoutePatternKind::Regex);
    assert!(facts[0].route.pattern.value.is_some());
    assert_eq!(facts[0].status(), SemanticFactStatus::Exact);
}

// Shadow honesty: minted route facts ride a shadow adapter and cannot become
// publication authority (the #6822 exit gate remains closed).
#[test]
fn minted_route_facts_stay_shadow_receipts() {
    use perl_semantic_facts::framework::{
        AdapterAuthorityError, AdapterInput, AdapterSourceScope, FactClass,
    };

    let detection: AdapterDetectionResult = detect_dancer2(&input("gen-1"));
    let code = "package App;\nuse Dancer2;\nget '/x' => sub { 1 };\n";
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let declarations = extract_dancer2_route_declarations(&ast, FileId(3));
    let activation =
        dancer2_activation_facts(&detection, Some("App"), &parse_dancer2_import_args(&[]));
    let facts = dancer2_route_facts(&detection, &activation, Some("App"), &declarations);
    assert_eq!(facts.len(), 1);
    // Envelope contract matches the shadow adapter surface.
    assert_eq!(facts[0].envelope.producer, perl_semantic_facts::SemanticProducer::FrameworkAdapter);
    assert_eq!(
        facts[0].envelope.confidence,
        perl_semantic_facts::SemanticConfidence::Known(Confidence::High)
    );

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
        vec![FactClass::GeneratedMembers],
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
        "no live provider surface is promoted by shadow route facts"
    );
}

// Extraction alone (pure grammar observation) is activation-independent; only
// minting is gated. A Dancer2-looking file without any import still extracts
// declarations, and those declarations mint nothing.
#[test]
fn extraction_is_observable_but_minting_is_activation_gated() {
    let code = "package App;\nget '/x' => sub { 1 };\n";
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let declarations: Vec<Dancer2RouteDeclaration> =
        extract_dancer2_route_declarations(&ast, FileId(1));
    assert_eq!(declarations.len(), 1, "grammar extraction observes the route");
    assert!(canonical_facts(code, "gen-1").is_empty(), "no activation, no facts");
}

// ---------------------------------------------------------------------
// #8921: prefix, route-parameter, and handler-context facts end to end.
// ---------------------------------------------------------------------

/// Full family chain over one source: parse → activation sites → route
/// contexts (routes + prefixes) → registry detection → exact activation
/// facts → minted canonical family facts.
fn family_facts(code: &str, generation: &str) -> Dancer2RouteFacts {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let sites = extract_dancer2_activation_sites(&ast, FileId(1));
    let contexts = extract_dancer2_route_contexts(&ast, FileId(1));
    let detection = detect_dancer2(&input(generation));
    let mut facts = Dancer2RouteFacts::default();
    for site in &sites {
        let activation =
            dancer2_activation_facts(&detection, site.package.as_deref(), &site.evidence);
        let minted = dancer2_route_family_facts(
            &detection,
            &activation,
            site.package.as_deref(),
            &contexts.routes,
            &contexts.prefixes,
        );
        facts.routes.extend(minted.routes);
        facts.prefixes.extend(minted.prefixes);
        facts.parameters.extend(minted.parameters);
        facts.handler_contexts.extend(minted.handler_contexts);
    }
    facts
}

fn parameter_names(facts: &Dancer2RouteFacts) -> Vec<String> {
    facts
        .parameters
        .iter()
        .filter(|parameter| parameter.parameter.name.is_some())
        .map(|parameter| must_some(parameter.parameter.name.clone()))
        .collect()
}

// The issue's primary positive fixture: literal prefix + named parameter +
// inline handler produce the composed projection, the route-scoped key fact,
// and the handler interval — all exact and anchored.
#[test]
fn issue_fixture_mints_prefix_params_and_handler_context() {
    let code = "package MyApp;\nuse Dancer2;\nprefix '/api';\nget '/users/:id' => sub {\n    my $id = route_parameters->{id};\n};\n";
    let facts = family_facts(code, "gen-1");
    assert_eq!(facts.prefixes.len(), 1);
    assert_eq!(facts.prefixes[0].status(), SemanticFactStatus::Exact);
    assert_eq!(facts.routes.len(), 1);
    let route = &facts.routes[0];
    let composed = must_some(match &route.route.effective_pattern {
        RouteEffectivePattern::Composed { value, prefix_declarations } => {
            Some((value.clone(), prefix_declarations.clone()))
        }
        _ => None,
    });
    assert_eq!(composed.0, "/api/users/:id");
    assert_eq!(composed.1, &[0]);
    assert!(
        route
            .envelope
            .invalidation_dependencies()
            .iter()
            .any(|dependency| dependency.dependency_key == "route-prefix:1:0"),
        "the route projection depends on the prefix declaration"
    );
    assert_eq!(route.status(), SemanticFactStatus::Exact);

    assert_eq!(facts.parameters.len(), 1);
    let parameter = &facts.parameters[0];
    assert_eq!(parameter.route_declaration_index, 0);
    assert_eq!(parameter.application_name, "MyApp");
    assert_eq!(parameter.status(), SemanticFactStatus::Exact);
    let anchor = parameter.parameter.anchor;
    assert_eq!(&code[anchor.start_byte as usize..anchor.end_byte as usize], ":id");
    let (_, route_entity) = route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
    assert_eq!(parameter.envelope.entity_id, Some(route_entity));

    assert_eq!(facts.handler_contexts.len(), 1);
    let context = &facts.handler_contexts[0];
    assert_eq!(context.envelope.kind, perl_semantic_facts::SemanticFactKind::RouteHandlerContext);
    assert_eq!(context.dsl_contract_version, "dancer2-dsl.1-1.v2");
    let interval = context.envelope.anchor;
    assert_eq!(
        &code[interval.start_byte as usize..interval.end_byte as usize],
        "sub {\n    my $id = route_parameters->{id};\n}"
    );
    // Nested lexical scopes inside the handler stay inside the interval.
    assert!(
        code[interval.start_byte as usize..interval.end_byte as usize].contains("route_parameters"),
        "the interval covers the handler body where request context applies"
    );
}

// Falsifier: ZERO family facts without registry activation.
#[test]
fn zero_family_facts_without_registry_activation() {
    let code = "package MyApp;\nuse Dancer2;\nprefix '/api';\nget '/users/:id' => sub { 1 };\n";
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
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let sites = extract_dancer2_activation_sites(&ast, FileId(1));
    let contexts = extract_dancer2_route_contexts(&ast, FileId(1));
    assert_eq!(contexts.prefixes.len(), 1, "extraction observes the prefix without activation");
    let detection = detect_dancer2(&absent_input);
    let activation =
        dancer2_activation_facts(&detection, sites[0].package.as_deref(), &sites[0].evidence);
    let facts = dancer2_route_family_facts(
        &detection,
        &activation,
        sites[0].package.as_deref(),
        &contexts.routes,
        &contexts.prefixes,
    );
    assert_eq!(facts, Dancer2RouteFacts::default(), "no detection, no facts of any kind");
}

// Falsifier: `!prefix` at the activating import suppresses prefix facts and
// degrades composed projections — the unimported keyword cannot establish a
// proven application prefix.
#[test]
fn excluded_prefix_keyword_mints_no_prefix_facts() {
    let code = "package App;\nuse Dancer2 qw(!prefix);\nprefix '/api';\nget '/x' => sub { 1 };\n";
    let facts = family_facts(code, "gen-1");
    assert_eq!(facts.routes.len(), 1, "routes still mint");
    assert!(facts.prefixes.is_empty(), "the excluded keyword mints nothing");
    // The composition degrades: without the imported keyword the `prefix`
    // call is not a proven Dancer2 application prefix, so the route must not
    // retain an exact `/api/x` projection or claim dependencies on the
    // never-minted prefix declaration.
    let route = &facts.routes[0];
    assert!(
        matches!(route.route.effective_pattern, RouteEffectivePattern::Boundary { .. }),
        "composed projections over an excluded prefix keyword degrade"
    );
    assert_eq!(route.status(), SemanticFactStatus::Degraded);
    assert!(
        !route
            .envelope
            .invalidation_dependencies()
            .iter()
            .any(|dependency| dependency.dependency_key == "route-prefix:1:0"),
        "no dependency on a prefix declaration that minted no fact"
    );
}

// Falsifier: dynamic prefix stays a boundary end to end — no guessed value.
#[test]
fn computed_prefix_stays_a_boundary_end_to_end() {
    let code = "package App;\nuse Dancer2;\nprefix $base;\nget '/x' => sub { 1 };\n";
    let facts = family_facts(code, "gen-1");
    assert_eq!(facts.prefixes.len(), 1);
    assert_eq!(facts.prefixes[0].status(), SemanticFactStatus::Degraded);
    assert!(facts.prefixes[0].envelope.boundary.is_some());
    assert_eq!(facts.routes.len(), 1);
    assert!(
        matches!(facts.routes[0].route.effective_pattern, RouteEffectivePattern::Boundary { .. }),
        "no effective projection is guessed behind a computed prefix"
    );
    assert_eq!(facts.routes[0].status(), SemanticFactStatus::Degraded);
}

// Falsifier: same parameter name in unrelated routes stays route-scoped; the
// observed keys are never a globally closed schema.
#[test]
fn same_parameter_name_is_route_scoped() {
    let code =
        "package App;\nuse Dancer2;\nget '/a/:id' => sub { 1 };\nget '/b/:id' => sub { 2 };\n";
    let facts = family_facts(code, "gen-1");
    assert_eq!(facts.parameters.len(), 2);
    assert_eq!(parameter_names(&facts), vec!["id".to_string(), "id".to_string()]);
    assert_ne!(facts.parameters[0].envelope.fact_id, facts.parameters[1].envelope.fact_id);
    assert_eq!(facts.parameters[0].route_declaration_index, 0);
    assert_eq!(facts.parameters[1].route_declaration_index, 1);
    for parameter in &facts.parameters {
        assert_eq!(parameter.envelope.package.as_deref(), Some("App"));
    }

    // A same-named key in another package's app mints through its own
    // activation only (package B below never activated).
    let two_apps = "package A;\nuse Dancer2;\nget '/a/:id' => sub { 1 };\npackage B;\nget '/b/:id' => sub { 2 };\n";
    let facts = family_facts(two_apps, "gen-1");
    assert_eq!(facts.parameters.len(), 1);
    assert_eq!(facts.parameters[0].envelope.package.as_deref(), Some("A"));
}

// Falsifier: handler-only DSL availability is the exact inline-handler
// interval — adjacent subs stay outside it, and a bounded handler removes the
// context fact entirely.
#[test]
fn handler_context_interval_does_not_leak() {
    let code = "package App;\nuse Dancer2;\nget '/x' => sub { route_parameters->{id}; };\nsub adjacent { route_parameters->{id}; }\n";
    let facts = family_facts(code, "gen-1");
    assert_eq!(facts.handler_contexts.len(), 1);
    let interval = facts.handler_contexts[0].envelope.anchor;
    let handler_text = &code[interval.start_byte as usize..interval.end_byte as usize];
    assert!(handler_text.starts_with("sub {"));
    assert!(
        !handler_text.contains("sub adjacent"),
        "the adjacent named sub stays outside the route-handler interval"
    );

    // A string handler has no proven interval: no handler-context fact.
    let bounded = "package App;\nuse Dancer2;\nget '/x' => 'handler_name';\n";
    let facts = family_facts(bounded, "gen-1");
    assert_eq!(facts.routes.len(), 1);
    assert!(matches!(facts.routes[0].route.handler, RouteHandler::Bounded { .. }));
    assert!(facts.handler_contexts.is_empty(), "a bounded handler removes the context fact");
}

// Falsifier: prefix/pattern edits invalidate only affected current facts.
#[test]
fn prefix_edit_recomputes_dependent_projections() {
    let before = "package App;\nuse Dancer2;\nprefix '/api';\nget '/x' => sub { 1 };\n";
    let after = "package App;\nuse Dancer2;\nprefix '/v2';\nget '/x' => sub { 1 };\n";
    let gen1 = family_facts(before, "gen-1");
    let gen2 = family_facts(after, "gen-2");
    let composed = |facts: &Dancer2RouteFacts| {
        must_some(match &facts.routes[0].route.effective_pattern {
            RouteEffectivePattern::Composed { value, .. } => Some(value.clone()),
            _ => None,
        })
    };
    assert_eq!(composed(&gen1), "/api/x");
    assert_eq!(composed(&gen2), "/v2/x");
    assert_ne!(gen1.prefixes[0].envelope.fact_id, gen2.prefixes[0].envelope.fact_id);
    assert_ne!(gen1.routes[0].envelope.fact_id, gen2.routes[0].envelope.fact_id);
    assert_ne!(
        gen1.routes[0].envelope.source_generation, gen2.routes[0].envelope.source_generation,
        "a held gen-1 fact cannot represent the edited source"
    );
}

// Falsifier: generation isolation across roots for the whole family.
#[test]
fn family_facts_are_generation_owned() {
    let code = "package App;\nuse Dancer2;\nprefix '/api';\nget '/u/:id' => sub { 1 };\n";
    let root_a = family_facts(code, "gen-a");
    let root_b = family_facts(code, "gen-b");
    assert_eq!(root_a.prefixes.len(), 1);
    assert_ne!(root_a.prefixes[0].envelope.fact_id, root_b.prefixes[0].envelope.fact_id);
    assert_ne!(root_a.parameters[0].envelope.fact_id, root_b.parameters[0].envelope.fact_id);
    assert_ne!(
        root_a.handler_contexts[0].envelope.fact_id,
        root_b.handler_contexts[0].envelope.fact_id
    );
}

// Regex routes mint an unsupported-capture boundary parameter, never guessed
// capture keys.
#[test]
fn regex_route_mints_capture_boundary_not_keys() {
    let code = "package App;\nuse Dancer2;\nget qr{^/re/(\\d+)$} => sub { 1 };\n";
    let facts = family_facts(code, "gen-1");
    assert_eq!(facts.parameters.len(), 1);
    assert!(matches!(facts.parameters[0].parameter.kind, RouteParameterKind::CaptureUnsupported));
    assert_eq!(facts.parameters[0].status(), SemanticFactStatus::Degraded);
    assert!(facts.parameters[0].envelope.boundary.is_some());
}

// Typed parameters carry the declared type name and its limitation.
#[test]
fn typed_parameters_carry_type_and_limitation() {
    let code = "package App;\nuse Dancer2;\nget '/u/:id[Int]' => sub { 1 };\n";
    let facts = family_facts(code, "gen-1");
    assert_eq!(facts.parameters.len(), 1);
    let type_name = must_some(match &facts.parameters[0].parameter.kind {
        RouteParameterKind::Typed { type_name } => Some(type_name.clone()),
        _ => None,
    });
    assert_eq!(type_name, "Int");
    assert!(facts.parameters[0].parameter.limitation.is_some());
    assert_eq!(facts.parameters[0].status(), SemanticFactStatus::Exact);
}

// Splat/megasplat parameters mint keyless capture facts.
#[test]
fn splat_and_megasplat_parameters_mint() {
    let code = "package App;\nuse Dancer2;\nget '/files/*/**' => sub { 1 };\n";
    let facts = family_facts(code, "gen-1");
    assert_eq!(facts.parameters.len(), 2);
    assert!(matches!(facts.parameters[0].parameter.kind, RouteParameterKind::Splat));
    assert!(matches!(facts.parameters[1].parameter.kind, RouteParameterKind::Megasplat));
    let anchors: Vec<&str> = facts
        .parameters
        .iter()
        .map(|parameter| {
            &code[parameter.parameter.anchor.start_byte as usize
                ..parameter.parameter.anchor.end_byte as usize]
        })
        .collect();
    assert_eq!(anchors, vec!["*", "**"]);
}

// The whole family round-trips through the JSON transport.
#[test]
fn family_facts_round_trip_through_json() -> Result<(), serde_json::Error> {
    let code = "package App;\nuse Dancer2;\nprefix '/api';\nprefix '/v1' => sub {\n  get '/u/:id[Int]/*' => sub { 1 };\n};\n";
    let facts = family_facts(code, "gen-1");
    assert_eq!(facts.prefixes.len(), 2);
    assert_eq!(facts.routes.len(), 1);
    assert_eq!(facts.parameters.len(), 2);
    assert_eq!(facts.handler_contexts.len(), 1);
    let routes = serde_json::to_string(&facts.routes)?;
    assert_eq!(
        serde_json::from_str::<Vec<RouteFact>>(&routes)?,
        facts.routes,
        "route facts round-trip"
    );
    let prefixes = serde_json::to_string(&facts.prefixes)?;
    assert_eq!(
        serde_json::from_str::<Vec<RoutePrefixFact>>(&prefixes)?,
        facts.prefixes,
        "prefix facts round-trip"
    );
    let parameters = serde_json::to_string(&facts.parameters)?;
    assert_eq!(
        serde_json::from_str::<Vec<RouteParameterFact>>(&parameters)?,
        facts.parameters,
        "parameter facts round-trip"
    );
    let contexts = serde_json::to_string(&facts.handler_contexts)?;
    assert_eq!(
        serde_json::from_str::<Vec<RouteHandlerContextFact>>(&contexts)?,
        facts.handler_contexts,
        "handler-context facts round-trip"
    );
    Ok(())
}
