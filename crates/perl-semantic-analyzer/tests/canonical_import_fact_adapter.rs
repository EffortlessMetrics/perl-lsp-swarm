//! #7613 / #14599 P2: analyzer import specs originate from HIR import facts.
//!
//! The oracle is `CompileEnvironment::import_specs`. Agreement by module
//! display name alone is insufficient: kind, symbols, provenance, confidence,
//! scope, anchor, and span must match. Overlay rows that HIR does not emit
//! (`RequireThenImport`, standalone `ManualImport`) are allowed extras.

use perl_parser_core::Parser;
use perl_parser_core::hir::lower_ast;
use perl_semantic_analyzer::analysis::import_extractor::ImportExtractor;
use perl_semantic_facts::{FileId, ImportKind, ImportSpec, ImportSymbols, Provenance};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const FILE_ID: FileId = FileId(7);

fn parse_ast(source: &str) -> Result<perl_parser_core::Node, Box<dyn std::error::Error>> {
    let mut parser = Parser::new(source);
    parser.parse().map_err(|error| format!("fixture must parse: {error:?}").into())
}

fn analyzer_specs(source: &str) -> Result<Vec<ImportSpec>, Box<dyn std::error::Error>> {
    let ast = parse_ast(source)?;
    Ok(ImportExtractor::extract(&ast, FILE_ID))
}

fn spec_named<'a>(specs: &'a [ImportSpec], module: &str) -> Result<&'a ImportSpec, String> {
    specs
        .iter()
        .find(|spec| spec.module == module)
        .ok_or_else(|| format!("missing ImportSpec for {module}; got {specs:?}"))
}

fn matching_analyzer_row<'a>(
    hir_spec: &ImportSpec,
    analyzer: &'a [ImportSpec],
) -> Result<&'a ImportSpec, String> {
    analyzer
        .iter()
        .find(|spec| {
            spec.span_start_byte == hir_spec.span_start_byte
                && match hir_spec.kind {
                    ImportKind::Require => {
                        spec.kind == ImportKind::Require
                            || spec.kind == ImportKind::RequireThenImport
                    }
                    _ => spec.kind == hir_spec.kind && spec.module == hir_spec.module,
                }
        })
        .ok_or_else(|| {
            format!(
                "analyzer rows lost HIR identity for {} at {:?}: {analyzer:?}",
                hir_spec.module, hir_spec.span_start_byte
            )
        })
}

fn assert_hir_identity_survives(source: &str) -> TestResult {
    let ast = parse_ast(source)?;
    let hir = lower_ast(&ast);
    let hir_specs = hir.compile_environment.import_specs(FILE_ID);
    let analyzer = ImportExtractor::extract_from_hir(&hir, &ast, FILE_ID);

    for hir_spec in &hir_specs {
        let analyzer_spec = matching_analyzer_row(hir_spec, &analyzer)?;
        if hir_spec.kind == ImportKind::Require
            && analyzer_spec.kind == ImportKind::RequireThenImport
        {
            assert_eq!(analyzer_spec.file_id, hir_spec.file_id);
            assert_eq!(analyzer_spec.anchor_id, hir_spec.anchor_id);
            assert_eq!(analyzer_spec.span_start_byte, hir_spec.span_start_byte);
            continue;
        }
        assert_eq!(analyzer_spec.kind, hir_spec.kind, "kind drifted for {}", hir_spec.module);
        assert_eq!(
            analyzer_spec.symbols, hir_spec.symbols,
            "symbols drifted for {}",
            hir_spec.module
        );
        assert_eq!(
            analyzer_spec.provenance, hir_spec.provenance,
            "provenance drifted for {}",
            hir_spec.module
        );
        assert_eq!(
            analyzer_spec.confidence, hir_spec.confidence,
            "confidence drifted for {}",
            hir_spec.module
        );
        assert_eq!(
            analyzer_spec.scope_id, hir_spec.scope_id,
            "scope_id drifted for {}",
            hir_spec.module
        );
        assert_eq!(
            analyzer_spec.anchor_id, hir_spec.anchor_id,
            "anchor_id drifted for {}",
            hir_spec.module
        );
        assert_eq!(analyzer_spec.file_id, hir_spec.file_id);
        assert_eq!(analyzer_spec.span_start_byte, hir_spec.span_start_byte);
    }
    Ok(())
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
    let specs = analyzer_specs(source)?;

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
    assert_eq!(dynamic.confidence, perl_semantic_facts::Confidence::Low);

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
fn version_tokens_agree_with_hir_and_numeric_versions_are_not_symbols() -> TestResult {
    let source = "use M 1.23;\nuse N v1.23;\n";
    assert_hir_identity_survives(source)?;
    let specs = analyzer_specs(source)?;
    let hir = lower_ast(&parse_ast(source)?);
    let hir_specs = hir.compile_environment.import_specs(FILE_ID);

    let numeric = specs
        .iter()
        .find(|spec| spec.module == "M" || spec.module.starts_with("M "))
        .ok_or_else(|| format!("missing numeric-version ImportSpec; got {specs:?}"))?;
    let hir_numeric = hir_specs
        .iter()
        .find(|spec| spec.module == numeric.module)
        .ok_or_else(|| format!("HIR lost numeric-version identity {}", numeric.module))?;
    assert_eq!(numeric.module, hir_numeric.module);
    assert_eq!(numeric.symbols, hir_numeric.symbols);
    match &numeric.symbols {
        ImportSymbols::Explicit(names) | ImportSymbols::Mixed { names, .. } => {
            assert!(
                !names.iter().any(|name| name == "1.23"),
                "numeric version leaked as an imported name: {names:?}"
            );
        }
        _ => {}
    }

    let vstring = spec_named(&specs, "N")?;
    let hir_vstring = spec_named(&hir_specs, "N")?;
    assert_eq!(
        vstring.symbols, hir_vstring.symbols,
        "analyzer must not invent a stricter version classifier than HIR"
    );
    Ok(())
}

#[test]
fn quoted_require_then_import_replaces_the_hir_path_row() -> TestResult {
    let source = "require \"Foo/Bar.pm\";\nFoo::Bar->import(qw(alpha));\n";
    assert_hir_identity_survives(source)?;
    let specs = analyzer_specs(source)?;
    let spec = spec_named(&specs, "Foo::Bar")?;
    assert_eq!(spec.kind, ImportKind::RequireThenImport);
    assert_eq!(spec.symbols, ImportSymbols::Explicit(vec!["alpha".to_string()]));
    assert_eq!(
        specs
            .iter()
            .filter(|row| {
                row.kind == ImportKind::Require || row.kind == ImportKind::RequireThenImport
            })
            .count(),
        1,
        "quoted require + import must replace the HIR path-like Require row: {specs:?}"
    );
    assert!(
        specs.iter().all(|row| row.module != "Foo/Bar.pm" && row.module != "\"Foo/Bar.pm\""),
        "paired overlay must not leave the unnormalized require target: {specs:?}"
    );
    Ok(())
}

#[test]
fn package_block_require_then_import_does_not_duplicate_overlay_rows() -> TestResult {
    let source = "package Demo {\nrequire Foo::Bar;\nFoo::Bar->import(qw(alpha));\n}\n";
    assert_hir_identity_survives(source)?;
    let specs = analyzer_specs(source)?;
    assert_eq!(
        specs.iter().filter(|row| row.module == "Foo::Bar").count(),
        1,
        "package-block require+import must not be overlaid twice: {specs:?}"
    );
    let spec = spec_named(&specs, "Foo::Bar")?;
    assert_eq!(spec.kind, ImportKind::RequireThenImport);
    Ok(())
}

#[test]
fn standalone_quoted_require_keeps_hir_path_identity() -> TestResult {
    let source = "require \"Foo/Bar.pm\";\n";
    assert_hir_identity_survives(source)?;
    let specs = analyzer_specs(source)?;
    assert_eq!(specs.len(), 1, "standalone quoted require must keep one HIR row: {specs:?}");
    assert_eq!(specs[0].kind, ImportKind::Require);
    Ok(())
}

#[test]
fn require_then_import_overlay_replaces_the_hir_require_row() -> TestResult {
    let source = "require Foo::Bar;\nFoo::Bar->import(qw(alpha beta));\n";
    assert_hir_identity_survives(source)?;
    let specs = analyzer_specs(source)?;
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
fn extract_and_extract_from_hir_agree() -> TestResult {
    let source = "use M qw(foo);\nrequire N;\n";
    let ast = parse_ast(source)?;
    let hir = lower_ast(&ast);
    let via_ast = ImportExtractor::extract(&ast, FILE_ID);
    let via_hir = ImportExtractor::extract_from_hir(&hir, &ast, FILE_ID);
    assert_eq!(via_ast, via_hir);
    Ok(())
}

#[test]
fn recurrence_guard_rejects_an_analyzer_local_use_arg_classifier() {
    let extractor = include_str!("../src/analysis/import_extractor.rs");
    assert!(
        !extractor.contains("fn classify_args("),
        "analyzer must not reintroduce classify_args over flattened Use.args"
    );
    assert!(
        !extractor.contains("fn classify_use("),
        "analyzer must not reintroduce classify_use over flattened Use.args"
    );
    assert!(
        !extractor.contains("fn classify_use_constant("),
        "analyzer must not reintroduce a local use-constant argument classifier"
    );
    assert!(
        !extractor.contains("arguments_outside_configuration_hashes"),
        "analyzer must not reparse flattened directive arguments for canonical import semantics"
    );
    assert!(
        extractor.contains("compile_environment.import_specs"),
        "analyzer import rows must keep consuming CompileEnvironment::import_specs"
    );
    assert!(
        extractor.contains("extract_from_hir"),
        "AST-only public entry must call the HIR extractor rather than reclassify Use.args"
    );
}
