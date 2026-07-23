//! End-to-end proof for #2587: `WorkspaceIndex::index_file` must bridge a
//! module's `@EXPORT` facts into the `ImportExportIndex`, so an importing file's
//! `use M;` sees `M`'s default-exported symbols in its visible set — without any
//! manual `add_module_exports` wiring by the caller.
//!
//! Before the bridge, `add_module_exports` had zero production call sites: the
//! exporter's facts were computed by the HIR layer but never reached the index,
//! so `use M;` resolved to an empty export set and imported names were invisible.

use perl_semantic_facts::VisibleSymbolSource;
use perl_workspace::semantic::queries::SemanticQueries;
use perl_workspace::workspace::workspace_index::WorkspaceIndex;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Indexing an exporter with `our @EXPORT = qw(foo)` and an importer with
/// `use M;` must make `foo` visible in the importer via the production query
/// path — proving the FrameworkFactGraph/export-set → ImportExportIndex bridge
/// is live, not just wired manually in a unit test.
#[test]
fn index_file_bridges_default_exports_into_importer_visible_set() -> Result<()> {
    let index = WorkspaceIndex::new();

    let exporter_uri = "file:///lib/M.pm";
    let exporter_src = "package M;\nour @EXPORT = qw(foo);\n1;\n";
    index.index_file_str(exporter_uri, exporter_src)?;

    let importer_uri = "file:///script.pl";
    let importer_src = "package Main;\nuse M;\nfoo();\n1;\n";
    index.index_file_str(importer_uri, importer_src)?;

    // Query visibility at the `foo()` call, which sits after the `use M;`
    // directive so the imported default export is in scope.
    let offset = importer_src.find("foo();").ok_or("expected foo() call in importer")? as u32;

    let visible = index
        .with_semantic_queries_for_uri(importer_uri, |file_id, queries| {
            queries.visible_symbols_at(file_id, offset, None)
        })
        .ok_or("importer file was not indexed")?;

    let names: Vec<&str> = visible.iter().map(|symbol| symbol.name.as_str()).collect();
    let foo = visible
        .iter()
        .find(|symbol| symbol.name == "foo")
        .ok_or_else(|| format!("expected exported `foo` visible in importer; got {names:?}"))?;

    assert_eq!(
        foo.source,
        VisibleSymbolSource::DefaultExport,
        "`foo` must be visible as a default export bridged from M's @EXPORT; got {:?}",
        foo.source
    );

    Ok(())
}
