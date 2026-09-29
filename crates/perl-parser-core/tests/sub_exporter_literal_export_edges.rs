//! Remaining literal Sub::Exporter `-setup` export edges (#16865).
//!
//! #14530 already lowers a static `-setup` hash into HIR export declarations.
//! These contracts pin the edges that landing still flattened: a generator-backed
//! name must not decide a sibling direct name's confidence; an empty literal
//! export list is exact emptiness, not a missing-setup boundary; and two
//! packages in one file keep distinct export sets. Group expansion, importer
//! visibility, and provider cutover remain #16866 / #16868 / #16869.

mod cpan_test_helpers;

use cpan_test_helpers::assert_clean_parse;
use perl_parser_core::Parser;
use perl_parser_core::hir::{
    ExportDeclarationKind, FrameworkAdapterKind, FrameworkAdapterRegistry,
    FrameworkExportMechanism, FrameworkExportedSymbolKind, HirFile, StashDynamicBoundaryKind,
    lower_ast,
};
use perl_semantic_facts::Confidence;

fn lower(source: &str) -> HirFile {
    assert_clean_parse(source);
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

fn optional_symbols(file: &HirFile, package: &str) -> Vec<String> {
    file.stash_graph
        .export_declarations
        .iter()
        .filter(|declaration| {
            declaration.package == package && declaration.kind == ExportDeclarationKind::Optional
        })
        .flat_map(|declaration| declaration.symbols.clone())
        .collect()
}

fn export_boundaries(file: &HirFile, package: &str) -> Vec<(Option<String>, String)> {
    file.stash_graph
        .dynamic_boundaries
        .iter()
        .filter(|boundary| {
            boundary.package.as_deref() == Some(package)
                && boundary.kind == StashDynamicBoundaryKind::DynamicExportDeclaration
        })
        .map(|boundary| (boundary.symbol.clone(), boundary.reason.clone()))
        .collect()
}

#[test]
fn a_direct_sibling_keeps_high_source_confidence_beside_a_generator() -> Result<(), String> {
    // The live completion gate reads High as "compiler fact, high confidence".
    // A mixed `exports` list used to publish one Medium Optional declaration,
    // so `plain` (a real source sub) inherited the generator's weakening.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [ qw(plain), built => \\&build ] };\n\
         sub plain { 1 }\n\
         sub build { 1 }\n",
    );

    let graph = FrameworkAdapterRegistry::default().project_file(&file);
    let optional: Vec<_> = graph
        .exported_symbols
        .iter()
        .filter(|fact| {
            fact.package == "My::Utils" && fact.kind == FrameworkExportedSymbolKind::Optional
        })
        .collect();

    let plain = optional
        .iter()
        .find(|fact| fact.name == "plain")
        .ok_or("plain must remain a declared optional export")?;
    let built = optional
        .iter()
        .find(|fact| fact.name == "built")
        .ok_or("built must remain a declared optional export")?;

    assert_eq!(plain.adapter, FrameworkAdapterKind::ExporterFamily);
    assert_eq!(built.adapter, FrameworkAdapterKind::ExporterFamily);
    assert_eq!(plain.mechanism, FrameworkExportMechanism::Direct);
    assert_eq!(built.mechanism, FrameworkExportMechanism::GeneratorBacked);
    assert_eq!(
        plain.source_confidence,
        Confidence::High,
        "a direct export must not inherit generator-backed Medium confidence: {plain:?}"
    );
    assert_eq!(
        built.source_confidence,
        Confidence::Medium,
        "a generator-backed export must stay below the High live-completion gate: {built:?}"
    );
    let optional_decl = file
        .stash_graph
        .export_declarations
        .iter()
        .find(|declaration| {
            declaration.package == "My::Utils"
                && declaration.kind == ExportDeclarationKind::Optional
        })
        .ok_or("optional declaration")?;
    assert_eq!(optional_decl.generator_backed, vec!["built".to_string()]);
    assert!(
        plain.declaration_item.is_some() && built.declaration_item.is_some(),
        "both names must stay anchored to the -setup use"
    );
    Ok(())
}

#[test]
fn a_setup_wide_custom_generator_keeps_every_export_below_high() -> Result<(), String> {
    // Opposite-direction control: replacing the default generator really does
    // weaken every name, including a bareword that would otherwise be High.
    let file = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => {\n\
             exports => [qw(plain)],\n\
             generator => \\&build,\n\
         };\n",
    );

    let graph = FrameworkAdapterRegistry::default().project_file(&file);
    let plain = graph
        .exported_symbols
        .iter()
        .find(|fact| fact.package == "My::Utils" && fact.name == "plain")
        .ok_or("plain must still be declared")?;
    assert_eq!(plain.source_confidence, Confidence::Medium);
    assert_eq!(plain.mechanism, FrameworkExportMechanism::GeneratorBacked);
    Ok(())
}

#[test]
fn an_empty_literal_export_list_is_exact_emptiness_not_a_boundary() {
    // Negative control #5: missing/incomplete setup must not become an exact
    // empty export set. The complementary edge is that a *literal* empty list
    // is exact emptiness: no invented names, and no dynamic-export boundary.
    let empty = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => [] };\n",
    );
    assert!(
        optional_symbols(&empty, "My::Utils").is_empty(),
        "a literal empty exports list must not invent names"
    );
    assert!(
        export_boundaries(&empty, "My::Utils").is_empty(),
        "a readable empty list is not an incomplete setup: {:?}",
        export_boundaries(&empty, "My::Utils")
    );

    let computed = lower(
        "package My::Utils;\n\
         use Sub::Exporter -setup => { exports => $list };\n",
    );
    assert!(
        optional_symbols(&computed, "My::Utils").is_empty(),
        "a computed exports value must not become an exact empty list"
    );
    assert!(
        export_boundaries(&computed, "My::Utils")
            .iter()
            .any(|(_, reason)| reason.contains("not statically enumerable")),
        "a computed exports value must record a typed boundary, not silence: {:?}",
        export_boundaries(&computed, "My::Utils")
    );
}

#[test]
fn two_packages_in_one_file_keep_distinct_literal_exports() {
    let file = lower(
        "package First::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(from_first)] };\n\
         package Second::Utils;\n\
         use Sub::Exporter -setup => { exports => [qw(from_second)] };\n",
    );

    assert_eq!(optional_symbols(&file, "First::Utils"), vec!["from_first".to_string()]);
    assert_eq!(optional_symbols(&file, "Second::Utils"), vec!["from_second".to_string()]);
    assert!(
        !optional_symbols(&file, "First::Utils").contains(&"from_second".to_string()),
        "a later package must not supply the earlier package's exports"
    );
    assert!(export_boundaries(&file, "First::Utils").is_empty());
    assert!(export_boundaries(&file, "Second::Utils").is_empty());

    let graph = FrameworkAdapterRegistry::default().project_file(&file);
    let names: Vec<_> = graph
        .exported_symbols
        .iter()
        .filter(|fact| fact.kind == FrameworkExportedSymbolKind::Optional)
        .map(|fact| (fact.package.as_str(), fact.name.as_str()))
        .collect();
    assert!(names.contains(&("First::Utils", "from_first")));
    assert!(names.contains(&("Second::Utils", "from_second")));
    assert!(!names.contains(&("First::Utils", "from_second")));
    assert!(!names.contains(&("Second::Utils", "from_first")));
}
