//! #16823: workspace import rows must originate from HIR import facts.
//!
//! The oracle is `CompileEnvironment::import_specs`. Agreement by module
//! display name alone is insufficient: kind, symbols, provenance, confidence,
//! scope, anchor, and span must match. Overlay rows that HIR does not emit
//! (`RequireThenImport`, standalone `ManualImport`) are allowed extras.

use perl_parser_core::Parser;
use perl_parser_core::hir::lower_ast;
use perl_semantic_facts::{
    Confidence, FileId, ImportKind, ImportSpec, ImportSymbols, Provenance, VisibleSymbolSource,
};
use perl_workspace::semantic::queries::SemanticQueries;
use perl_workspace::semantic::workspace_import_extractor::{
    extract_import_specs, extract_import_specs_from_hir, extract_import_specs_with_source,
};
use perl_workspace::workspace::workspace_index::WorkspaceIndex;
use url::Url;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const FILE_ID: FileId = FileId(7);

fn parse_ast(source: &str) -> Result<perl_parser_core::Node, Box<dyn std::error::Error>> {
    let mut parser = Parser::new(source);
    parser.parse().map_err(|error| format!("fixture must parse: {error:?}").into())
}

fn workspace_specs(source: &str) -> Result<Vec<ImportSpec>, Box<dyn std::error::Error>> {
    let ast = parse_ast(source)?;
    Ok(extract_import_specs_with_source(&ast, FILE_ID, source))
}

fn spec_named<'a>(specs: &'a [ImportSpec], module: &str) -> Result<&'a ImportSpec, String> {
    specs
        .iter()
        .find(|spec| spec.module == module)
        .ok_or_else(|| format!("missing ImportSpec for {module}; got {specs:?}"))
}

fn matching_workspace_row<'a>(
    hir_spec: &ImportSpec,
    workspace: &'a [ImportSpec],
) -> Result<&'a ImportSpec, String> {
    workspace
        .iter()
        .find(|spec| {
            spec.module == hir_spec.module
                && spec.span_start_byte == hir_spec.span_start_byte
                && match hir_spec.kind {
                    ImportKind::Require => {
                        spec.kind == ImportKind::Require
                            || spec.kind == ImportKind::RequireThenImport
                    }
                    _ => spec.kind == hir_spec.kind,
                }
        })
        .ok_or_else(|| {
            format!(
                "workspace rows lost HIR identity for {} at {:?}: {workspace:?}",
                hir_spec.module, hir_spec.span_start_byte
            )
        })
}

fn assert_hir_identity_survives(source: &str) -> TestResult {
    let ast = parse_ast(source)?;
    let hir = lower_ast(&ast);
    let hir_specs = hir.compile_environment.import_specs(FILE_ID);
    let workspace = extract_import_specs_from_hir(&hir, &ast, FILE_ID, Some(source));

    for hir_spec in &hir_specs {
        let workspace_spec = matching_workspace_row(hir_spec, &workspace)?;
        if hir_spec.kind == ImportKind::Require
            && workspace_spec.kind == ImportKind::RequireThenImport
        {
            assert_eq!(workspace_spec.file_id, hir_spec.file_id);
            assert_eq!(workspace_spec.anchor_id, hir_spec.anchor_id);
            assert_eq!(workspace_spec.span_start_byte, hir_spec.span_start_byte);
            continue;
        }
        assert_eq!(workspace_spec.kind, hir_spec.kind, "kind drifted for {}", hir_spec.module);
        assert_eq!(
            workspace_spec.symbols, hir_spec.symbols,
            "symbols drifted for {}",
            hir_spec.module
        );
        assert_eq!(
            workspace_spec.provenance, hir_spec.provenance,
            "provenance drifted for {}",
            hir_spec.module
        );
        assert_eq!(
            workspace_spec.confidence, hir_spec.confidence,
            "confidence drifted for {}",
            hir_spec.module
        );
        assert_eq!(
            workspace_spec.scope_id, hir_spec.scope_id,
            "scope_id drifted for {}",
            hir_spec.module
        );
        assert_eq!(
            workspace_spec.anchor_id, hir_spec.anchor_id,
            "anchor_id drifted for {}",
            hir_spec.module
        );
        assert_eq!(workspace_spec.file_id, hir_spec.file_id);
        assert_eq!(workspace_spec.span_start_byte, hir_spec.span_start_byte);
    }
    Ok(())
}

fn assert_ordered(specs: &[ImportSpec]) {
    let starts: Vec<u32> = specs.iter().filter_map(|spec| spec.span_start_byte).collect();
    assert!(
        starts.windows(2).all(|window| window[0] <= window[1]),
        "workspace import rows must keep source order: {starts:?}"
    );
}

#[test]
fn required_directive_shapes_keep_hir_identity() -> TestResult {
    let source = "\
package Demo;\n\
use M;\n\
use M ();\n\
use VersionedNum 1.23;\n\
use Versioned v1.23;\n\
use M qw(foo bar);\n\
use Tagged qw(:tag);\n\
use Quoted 'foo';\n\
use Dynamic $dynamic;\n\
use Renamed foo => { -as => 'bar' };\n\
use Configured { -config => 1 };\n\
use Trailing 'param1', 'param2', {key => 'value'};\n\
use Sub::Exporter -setup => { exports => [qw(foo bar)] };\n\
";
    assert_hir_identity_survives(source)?;
    let specs = workspace_specs(source)?;
    assert_ordered(&specs);

    let default = spec_named(&specs, "M")?;
    assert_eq!(default.kind, ImportKind::Use);
    assert_eq!(default.symbols, ImportSymbols::Default);

    let empty = specs
        .iter()
        .find(|spec| spec.module == "M" && spec.kind == ImportKind::UseEmpty)
        .ok_or("expected explicit-empty use M ()")?;
    assert_eq!(empty.symbols, ImportSymbols::None);
    assert_ne!(
        default.span_start_byte, empty.span_start_byte,
        "absent and explicit-empty imports must remain distinct occurrences"
    );

    let quoted = spec_named(&specs, "Quoted")?;
    assert_eq!(quoted.symbols, ImportSymbols::Explicit(vec!["foo".to_string()]));

    let tagged = spec_named(&specs, "Tagged")?;
    assert_eq!(tagged.kind, ImportKind::UseTag);
    assert_eq!(tagged.symbols, ImportSymbols::Tags(vec!["tag".to_string()]));

    let dynamic = spec_named(&specs, "Dynamic")?;
    assert_eq!(dynamic.symbols, ImportSymbols::Dynamic);
    assert_eq!(dynamic.provenance, Provenance::DynamicBoundary);
    assert_eq!(dynamic.confidence, Confidence::Low);

    let renamed = spec_named(&specs, "Renamed")?;
    assert_eq!(renamed.symbols, ImportSymbols::Explicit(vec!["bar".to_string()]));

    let configured = spec_named(&specs, "Configured")?;
    assert_eq!(configured.kind, ImportKind::UseEmpty);
    assert_eq!(configured.symbols, ImportSymbols::None);

    let trailing = spec_named(&specs, "Trailing")?;
    assert_eq!(
        trailing.symbols,
        ImportSymbols::Explicit(vec!["param1".to_string(), "param2".to_string()])
    );

    let setup = spec_named(&specs, "Sub::Exporter")?;
    assert_eq!(setup.kind, ImportKind::UseEmpty);
    assert_eq!(setup.symbols, ImportSymbols::None);
    Ok(())
}

#[test]
fn version_tokens_are_not_imported_symbols() -> TestResult {
    let source = "use M 1.23;\nuse N v1.23;\n";
    assert_hir_identity_survives(source)?;
    let specs = workspace_specs(source)?;
    for spec in &specs {
        match &spec.symbols {
            ImportSymbols::Explicit(names) | ImportSymbols::Mixed { names, .. } => {
                assert!(
                    names.iter().all(|name| name != "1.23" && name != "v1.23"),
                    "version tokens leaked as imported names in {}: {names:?}",
                    spec.module
                );
            }
            _ => {}
        }
    }
    Ok(())
}

#[test]
fn unicode_and_crlf_rows_keep_hir_spans() -> TestResult {
    let source = "package Café;\r\nuse M::Ünicode;\r\nuse Empty ();\r\n";
    assert_hir_identity_survives(source)?;
    let specs = workspace_specs(source)?;
    assert_ordered(&specs);
    assert!(spec_named(&specs, "M::Ünicode")?.span_start_byte.is_some());
    assert_eq!(spec_named(&specs, "Empty")?.kind, ImportKind::UseEmpty);
    Ok(())
}

#[test]
fn package_scope_changes_are_preserved_from_hir() -> TestResult {
    let source = "package First;\nuse Alpha;\npackage Second;\nuse Beta;\n";
    assert_hir_identity_survives(source)?;
    let specs = workspace_specs(source)?;
    let alpha = spec_named(&specs, "Alpha")?;
    let beta = spec_named(&specs, "Beta")?;
    assert_ne!(
        alpha.scope_id, beta.scope_id,
        "package change must produce distinct HIR scope identity: {alpha:?} vs {beta:?}"
    );
    assert!(alpha.scope_id.is_some(), "HIR scope_id must survive the adapter");
    Ok(())
}

#[test]
fn malformed_recovered_input_agrees_with_hir_limitations() -> TestResult {
    let source = "use Recovered (foo\nuse After;\n";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    let hir_specs = hir.compile_environment.import_specs(FILE_ID);
    let workspace = extract_import_specs_from_hir(&hir, &output.ast, FILE_ID, Some(source));
    for hir_spec in &hir_specs {
        matching_workspace_row(hir_spec, &workspace)?;
    }
    Ok(())
}

#[test]
fn require_then_import_overlay_replaces_the_hir_require_row() -> TestResult {
    let source = "require Foo::Bar;\nFoo::Bar->import(qw(alpha beta));\n";
    assert_hir_identity_survives(source)?;
    let specs = workspace_specs(source)?;
    let spec = spec_named(&specs, "Foo::Bar")?;
    assert_eq!(spec.kind, ImportKind::RequireThenImport);
    assert_eq!(
        spec.symbols,
        ImportSymbols::Explicit(vec!["alpha".to_string(), "beta".to_string()])
    );
    assert_eq!(
        specs.iter().filter(|spec| spec.module == "Foo::Bar").count(),
        1,
        "require + import must not leave a duplicate Require row"
    );
    Ok(())
}

#[test]
fn index_file_default_and_empty_imports_stay_distinct() -> TestResult {
    let index = WorkspaceIndex::new();
    index.index_file_str("file:///lib/M.pm", "package M;\nour @EXPORT = qw(foo);\n1;\n")?;
    index.index_file_str("file:///default.pl", "package Main;\nuse M;\nfoo();\n1;\n")?;
    index.index_file_str("file:///empty.pl", "package Main;\nuse M ();\nfoo();\n1;\n")?;

    let default_visible = index
        .with_semantic_queries_for_uri("file:///default.pl", |file_id, queries| {
            queries.visible_symbols_at(file_id, 30, None)
        })
        .ok_or("default importer was not indexed")?;
    assert!(
        default_visible.iter().any(|symbol| {
            symbol.name == "foo" && symbol.source == VisibleSymbolSource::DefaultExport
        }),
        "bare use M must keep default exports; got {default_visible:?}"
    );

    let empty_visible = index
        .with_semantic_queries_for_uri("file:///empty.pl", |file_id, queries| {
            queries.visible_symbols_at(file_id, 32, None)
        })
        .ok_or("empty importer was not indexed")?;
    assert!(
        empty_visible.iter().all(|symbol| {
            !(symbol.name == "foo" && symbol.source == VisibleSymbolSource::DefaultExport)
        }),
        "use M () must not receive default exports; got {empty_visible:?}"
    );
    Ok(())
}

#[test]
fn later_generation_replaces_import_rows_and_stale_generation_does_not() -> TestResult {
    let index = WorkspaceIndex::new();
    index.index_file_str("file:///lib/M.pm", "package M;\nour @EXPORT = qw(foo);\n1;\n")?;
    let uri = Url::parse("file:///script.pl")?;
    index.index_file_with_generation(
        uri.clone(),
        "package Main;\nuse M;\nfoo();\n1;\n".to_string(),
        2,
    )?;
    index.index_file_with_generation(
        uri.clone(),
        "package Main;\nuse M ();\nfoo();\n1;\n".to_string(),
        1,
    )?;

    let after_stale = index
        .with_semantic_queries_for_uri(uri.as_str(), |file_id, queries| {
            queries.visible_symbols_at(file_id, 30, None)
        })
        .ok_or("importer missing after stale generation")?;
    assert!(
        after_stale.iter().any(|symbol| {
            symbol.name == "foo" && symbol.source == VisibleSymbolSource::DefaultExport
        }),
        "generation 1 must not replace generation 2 facts; got {after_stale:?}"
    );

    index.index_file_with_generation(
        uri,
        "package Main;\nuse M ();\nfoo();\n1;\n".to_string(),
        3,
    )?;
    let after_newer = index
        .with_semantic_queries_for_uri("file:///script.pl", |file_id, queries| {
            queries.visible_symbols_at(file_id, 32, None)
        })
        .ok_or("importer missing after newer generation")?;
    assert!(
        after_newer.iter().all(|symbol| {
            !(symbol.name == "foo" && symbol.source == VisibleSymbolSource::DefaultExport)
        }),
        "generation 3 empty import must replace generation 2 default import; got {after_newer:?}"
    );
    Ok(())
}

#[test]
fn same_module_spelling_in_another_root_does_not_contribute_rows() -> TestResult {
    let index = WorkspaceIndex::new();
    index
        .index_file_str("file:///root-a/lib/M.pm", "package M;\nour @EXPORT = qw(alpha);\n1;\n")?;
    index.index_file_str("file:///root-b/lib/M.pm", "package M;\nour @EXPORT = qw(beta);\n1;\n")?;
    index.index_file_str("file:///root-a/script.pl", "package Main;\nuse M;\nalpha();\n1;\n")?;

    let visible = index
        .with_semantic_queries_for_uri("file:///root-a/script.pl", |file_id, queries| {
            queries.visible_symbols_at(file_id, 30, None)
        })
        .ok_or("root-a importer was not indexed")?;
    assert!(
        visible.iter().any(|symbol| symbol.name == "alpha"),
        "root-a importer should see alpha; got {visible:?}"
    );
    assert!(
        visible.iter().all(|symbol| symbol.name != "beta"),
        "root-b exporter must not contribute rows into root-a; got {visible:?}"
    );
    Ok(())
}

#[test]
fn delete_and_readd_rebuilds_current_import_rows() -> TestResult {
    let index = WorkspaceIndex::new();
    let uri = "file:///script.pl";
    index.index_file_str(uri, "use First;\n")?;
    index.remove_file(uri);
    index.index_file_str(uri, "use Second;\n")?;
    let specs = workspace_specs("use Second;\n")?;
    assert!(spec_named(&specs, "Second").is_ok());
    assert!(specs.iter().all(|spec| spec.module != "First"));
    Ok(())
}

#[test]
fn recurrence_guard_rejects_a_workspace_local_use_arg_classifier() {
    let extractor = include_str!("../src/semantic/workspace_import_extractor.rs");
    assert!(
        !extractor.contains("fn classify_args("),
        "workspace must not reintroduce classify_args over flattened Use.args"
    );
    assert!(
        !extractor.contains("arguments_outside_configuration_hashes"),
        "workspace must not reparse flattened directive arguments for canonical import semantics"
    );
    assert!(
        extractor.contains("compile_environment.import_specs"),
        "workspace import rows must keep consuming CompileEnvironment::import_specs"
    );
    let semantic_src = include_str!("../src/semantic/mod.rs");
    assert!(
        semantic_src.contains("extract_import_specs_from_hir"),
        "production wrapper must keep the HIR adapter entry"
    );
}

#[test]
fn extract_import_specs_without_source_still_projects_hir_facts() -> TestResult {
    let source = "use M ();\n";
    let ast = parse_ast(source)?;
    let specs = extract_import_specs(&ast, FILE_ID);
    let spec = spec_named(&specs, "M")?;
    assert_eq!(spec.kind, ImportKind::UseEmpty);
    assert_eq!(spec.symbols, ImportSymbols::None);
    Ok(())
}
