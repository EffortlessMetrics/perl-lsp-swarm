#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! Shadow-parity receipts for the #8928 Dancer2 provider cutover.
//!
//! Before the legacy route-path synthesis was retired for the admitted
//! forms, parity was proven by comparing the canonical admission against a
//! frozen copy of the retired legacy behavior (the exact statement patterns
//! `try_extract_web_route_declaration` matched, with its verb table and
//! route-name selection). This file keeps that oracle:
//!
//! 1. **Coverage parity**: every declaration the frozen legacy oracle
//!    synthesizes under a source-exact default-DSL activation is admitted
//!    by the canonical extractor, and the informational payload agrees
//!    (path identity; method identity modulo the reviewed intended deltas:
//!    GET+HEAD, the full `any` vocabulary, `options` coverage).
//! 2. **Intended deltas**: the canonical method profile is the reviewed
//!    upstream profile, not the legacy approximation.
//! 3. **Retirement boundary**: the live `SymbolExtractor` synthesizes no
//!    route-path `Subroutine` for the admitted set; handler-local symbols
//!    are still indexed; unadmitted forms keep the legacy path.
//! 4. **Skeleton corpus**: the committed Dancer2 skeleton fixture produces
//!    the same parity outcome on its real route declaration.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::dancer2_activation::extract_dancer2_activation_sites;
use perl_semantic_analyzer::analysis::dancer2_routes::extract_dancer2_route_contexts;
use perl_semantic_analyzer::symbol::{SymbolExtractor, SymbolKind, SymbolTable};
use perl_semantic_facts::FileId;
use perl_semantic_facts::framework_adapters::dancer2::DslSelection;
use perl_semantic_facts::route::{RouteMethodSet, RoutePatternKind};
use perl_tdd_support::must;
use std::collections::HashSet;
use std::path::PathBuf;

fn parse(source: &str) -> perl_parser_core::Node {
    let mut parser = Parser::new(source);
    perl_tdd_support::must(parser.parse())
}

/// Frozen copy of the retired legacy verb table.
fn legacy_http_method(keyword: &str) -> String {
    match keyword {
        "get" => "GET".to_string(),
        "post" => "POST".to_string(),
        "put" => "PUT".to_string(),
        "del" | "delete" => "DELETE".to_string(),
        "patch" => "PATCH".to_string(),
        "any" => "ANY".to_string(),
        other => other.to_string(),
    }
}

/// Frozen copy of the retired legacy oracle: the (name, http_method) pairs
/// the legacy synthesis would have produced for Dancer2-activated packages.
///
/// Mirrors the two statement shapes `try_extract_web_route_declaration`
/// matched: the FunctionCall form `VERB PATTERN => sub { }` and the
/// two-statement `VERB; { PATTERN => sub { } }` form.
fn frozen_legacy_route_symbols(ast: &perl_parser_core::Node) -> Vec<(String, String)> {
    use perl_parser_core::NodeKind;

    // The packages an exact `use Dancer2` activated (framework flags
    // semantics: per-package `use Dancer2` sets the Dancer2 flag).
    let mut activated_packages: HashSet<String> = HashSet::new();
    let mut current_package = "main".to_string();
    fn walk_flags(
        node: &perl_parser_core::Node,
        current_package: &mut String,
        activated: &mut HashSet<String>,
    ) {
        if let NodeKind::Package { name, .. } = &node.kind {
            *current_package = name.clone();
        }
        if let NodeKind::Use { module, .. } = &node.kind
            && module == "Dancer2"
        {
            activated.insert(current_package.clone());
        }
        for child in node.children() {
            walk_flags(child, current_package, activated);
        }
    }
    walk_flags(ast, &mut current_package, &mut activated_packages);

    // Statement shapes over top-level statements (the legacy extractor ran
    // over statement lists of each block scope; the fixtures are flat).
    let mut symbols = Vec::new();
    fn walk_statements(
        node: &perl_parser_core::Node,
        package: &mut String,
        activated: &HashSet<String>,
        symbols: &mut Vec<(String, String)>,
    ) {
        const NO_STATEMENTS: &[perl_parser_core::Node] = &[];
        let statements: &[perl_parser_core::Node] = match &node.kind {
            NodeKind::Block { statements } | NodeKind::Program { statements } => statements,
            _ => NO_STATEMENTS,
        };
        if !statements.is_empty() {
            let mut index = 0;
            while index < statements.len() {
                let statement = &statements[index];
                if let NodeKind::ExpressionStatement { expression } = &statement.kind {
                    match &expression.kind {
                        NodeKind::FunctionCall { name, args }
                            if matches!(
                                name.as_str(),
                                "get" | "post" | "put" | "del" | "delete" | "patch" | "any"
                            ) && activated.contains(package.as_str()) =>
                        {
                            if let Some(pattern) = first_literal_string(args) {
                                symbols.push((pattern, legacy_http_method(name).to_string()));
                            }
                        }
                        NodeKind::Identifier { name }
                            if matches!(
                                name.as_str(),
                                "get" | "post" | "put" | "del" | "delete" | "patch" | "any"
                            ) && activated.contains(package.as_str()) =>
                        {
                            // Two-statement form: the next statement is a
                            // hash literal with the pattern as the first key.
                            if let Some(next) = statements.get(index + 1)
                                && let NodeKind::ExpressionStatement { expression } = &next.kind
                                && let NodeKind::HashLiteral { pairs } = &expression.kind
                                && let Some((pattern_node, _)) = pairs.first()
                                && let NodeKind::String { value, .. } = &pattern_node.kind
                            {
                                let pattern = value.trim_matches('\'').trim_matches('"');
                                if !pattern.is_empty() {
                                    symbols.push((
                                        pattern.to_string(),
                                        legacy_http_method(name).to_string(),
                                    ));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if let NodeKind::Package { name, .. } = &statement.kind {
                    *package = name.clone();
                }
                walk_statements(statement, package, activated, symbols);
                index += 1;
            }
        }
        // Program/Block statements were walked above with per-statement
        // recursion; other node kinds descend through children.
        if !matches!(&node.kind, NodeKind::Block { .. } | NodeKind::Program { .. }) {
            for child in node.children() {
                walk_statements(child, package, activated, symbols);
            }
        }
    }
    let mut package = "main".to_string();
    walk_statements(ast, &mut package, &activated_packages, &mut symbols);
    symbols
}

fn first_literal_string(args: &[perl_parser_core::Node]) -> Option<String> {
    let first = args.first()?;
    if let perl_parser_core::NodeKind::String { value, .. } = &first.kind {
        let stripped = value.trim_matches('\'').trim_matches('"');
        if !stripped.is_empty() && stripped.starts_with('/') {
            return Some(stripped.to_string());
        }
    }
    None
}

/// Source-side canonical admission (mirrors the retirement gate in
/// `symbol.rs`).
fn canonical_admission(ast: &perl_parser_core::Node) -> HashSet<(String, String, String)> {
    let file_id = FileId(0);
    let contexts = extract_dancer2_route_contexts(ast, file_id);
    let sites = extract_dancer2_activation_sites(ast, file_id);
    let mut exact_packages: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for site in &sites {
        let package = site.package.clone().unwrap_or_else(|| "main".to_string());
        if exact_packages.contains_key(&package) {
            continue;
        }
        let source_exact =
            site.evidence.dsl.as_ref().is_none_or(|dsl| matches!(dsl, DslSelection::Default))
                && site.evidence.unmodeled_options.is_empty();
        if source_exact {
            exact_packages.insert(package, site.evidence.excluded_keywords.clone());
        }
    }
    let route_keywords: std::collections::HashSet<&str> =
        perl_semantic_facts::framework_adapters::dancer2_routes::DANCER2_ROUTE_KEYWORDS
            .iter()
            .copied()
            .collect();
    let mut admitted = HashSet::new();
    for declaration in &contexts.routes {
        let Some(package) = &declaration.package else { continue };
        let Some(exclusions) = exact_packages.get(package) else { continue };
        let keyword = declaration.route.keyword.as_str();
        if !route_keywords.contains(keyword)
            || exclusions.iter().any(|excluded| excluded == keyword)
        {
            continue;
        }
        if let Some(pattern) = &declaration.route.pattern.value {
            admitted.insert((package.clone(), pattern.clone(), keyword.to_string()));
        }
    }
    admitted
}

fn table_has_route_path_subroutine(table: &SymbolTable, path: &str) -> bool {
    table
        .symbols
        .get(path)
        .is_some_and(|symbols| symbols.iter().any(|symbol| symbol.kind == SymbolKind::Subroutine))
}

#[test]
fn parity_admitted_forms_cover_frozen_legacy_oracle() {
    let fixtures = [
        "package App;\nuse Dancer2;\nget '/hello' => sub { 'x' };\n",
        "package App;\nuse Dancer2;\npost '/api/users' => sub { 1 };\n",
        "package App;\nuse Dancer2;\nput '/api/users/:id' => sub { 'updated' };\n",
        "package App;\nuse Dancer2;\ndel '/api/users/:id' => sub { 'deleted' };\n",
        "package App;\nuse Dancer2;\npatch '/api/users/:id' => sub { 'patched' };\n",
        "package App;\nuse Dancer2;\nany '/multi' => sub { 'multi' };\n",
        "use Dancer2;\nget '/x' => sub { 1 };\n",
    ];
    for source in fixtures {
        let ast = parse(source);
        let legacy = frozen_legacy_route_symbols(&ast);
        assert_eq!(legacy.len(), 1, "oracle shape for: {source}");
        let (path, http_method) = &legacy[0];
        let admitted = canonical_admission(&ast);
        let matched = admitted.iter().any(|(package, pattern, keyword)| {
            let package_matches = package == "App" || package == "main";
            package_matches && pattern == path && &legacy_http_method(keyword) == http_method
        });
        assert!(
            matched,
            "canonical admission must cover the frozen legacy oracle entry \
             ({path}, {http_method}) for: {source}"
        );
    }
}

#[test]
fn parity_intended_method_deltas_are_the_reviewed_profile() {
    let source = "package App;\nuse Dancer2;\nget '/g' => sub { 1 };\nany '/a' => sub { 1 };\noptions '/o' => sub { 1 };\n";
    let ast = parse(source);
    let contexts = extract_dancer2_route_contexts(&ast, FileId(0));
    for declaration in &contexts.routes {
        assert_eq!(declaration.route.pattern.kind, RoutePatternKind::Literal);
        match &declaration.route.methods {
            RouteMethodSet::Exact(methods) => match declaration.route.keyword.as_str() {
                "get" => {
                    assert_eq!(methods, &["GET".to_string(), "HEAD".to_string()]);
                }
                "any" => {
                    assert_eq!(methods.len(), 7, "reviewed default vocabulary");
                    assert!(!methods.contains(&"ANY".to_string()));
                }
                "options" => assert_eq!(methods, &["OPTIONS".to_string()]),
                _ => {}
            },
            other => {
                assert!(matches!(other, RouteMethodSet::Exact(_)), "literal route must be exact")
            }
        }
    }
}

#[test]
fn retirement_boundary_and_handler_local_indexing() {
    let source = "package App;\nuse Dancer2;\nget '/x' => sub { my $handler_local = 1; };\n";
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let table = SymbolExtractor::new_with_source(source).extract(&ast);
    assert!(
        !table_has_route_path_subroutine(&table, "/x"),
        "the admitted form is retired from the legacy synthesis"
    );
    assert!(
        table.symbols.contains_key("handler_local"),
        "handler-local lexical symbols stay indexed after retirement"
    );
}

#[test]
fn retirement_boundary_keeps_unadmitted_forms_on_legacy_path() {
    let excluded = "package App;\nuse Dancer2 '!get';\nget '/x' => sub { 1 };\n";
    let mut parser = Parser::new(excluded);
    let ast = must(parser.parse());
    let table = SymbolExtractor::new_with_source(excluded).extract(&ast);
    assert!(
        table_has_route_path_subroutine(&table, "/x"),
        "excluded-keyword form keeps the legacy path (recorded boundary)"
    );

    let custom_dsl = "package App;\nuse Dancer2 dsl => 'My::DSL';\nget '/x' => sub { 1 };\n";
    let mut parser = Parser::new(custom_dsl);
    let ast = must(parser.parse());
    let table = SymbolExtractor::new_with_source(custom_dsl).extract(&ast);
    assert!(
        table_has_route_path_subroutine(&table, "/x"),
        "custom-DSL form keeps the legacy path (recorded boundary)"
    );
}

#[test]
fn retirement_boundary_regex_route_form() {
    // The parser produces a two-statement shape for regex patterns; the
    // canonical extractor admits it and the retirement gate must match the
    // keyword anchor at the first statement's start.
    let source = "package App;\nuse Dancer2;\nget qr{^/re/(\\d+)$} => sub { 1 };\n";
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let admitted = canonical_admission(&ast);
    assert!(
        admitted.iter().any(|(package, pattern, keyword)| {
            package == "App" && keyword == "get" && pattern.contains("/re")
        }),
        "the regex route form is admitted: {admitted:?}"
    );
    let table = SymbolExtractor::new_with_source(source).extract(&ast);
    assert!(
        table.symbols.keys().all(|name| !name.starts_with('/')),
        "the admitted regex route form retires the legacy synthesis"
    );
}

#[test]
fn skeleton_corpus_parity_and_retirement() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|parent| parent.parent())
        .map(|root| root.join("test_corpus/real_projects/dancer2_skeleton"))
        .unwrap_or_else(|| PathBuf::from("test_corpus/real_projects/dancer2_skeleton"));
    let basic = std::fs::read_to_string(root.join("t/basic.t")).unwrap_or_default();
    let ast = parse(&basic);
    let legacy = frozen_legacy_route_symbols(&ast);
    assert_eq!(legacy.len(), 1, "skeleton carries one route declaration");
    assert_eq!(legacy[0].0, "/", "the skeleton route is `/`");

    let admitted = canonical_admission(&ast);
    assert!(
        admitted.contains(&("MyApp".to_string(), "/".to_string(), "get".to_string())),
        "canonical admission covers the skeleton route: {admitted:?}"
    );

    let table = SymbolExtractor::new_with_source(&basic).extract(&ast);
    assert!(
        !table_has_route_path_subroutine(&table, "/"),
        "the skeleton route is retired from legacy synthesis"
    );
}
