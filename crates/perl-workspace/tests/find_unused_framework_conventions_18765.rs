//! Regression tests for #18765 items 3–4: framework lifecycle hooks and Exporter
//! package variables must not be reported as unused by `find_unused_symbols`.

use perl_workspace::workspace_index::WorkspaceIndex;
use url::Url;

fn index_str(index: &WorkspaceIndex, uri: &str, source: &str) -> Result<(), Box<dyn std::error::Error>> {
    let url = Url::parse(uri)?;
    index.index_file(url, source.to_string())?;
    Ok(())
}

#[test]
fn find_unused_symbols_skips_moo_build_hook() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    index_str(
        &index,
        "file:///Product.pm",
        "package Product;\n\
         use Moo;\n\
         sub BUILD {\n\
             my ($self, $args) = @_;\n\
         }\n\
         1;\n",
    )?;

    let unused_names: Vec<String> = index
        .find_unused_symbols()
        .into_iter()
        .map(|s| s.name.clone())
        .collect();

    assert!(
        !unused_names.iter().any(|n| n == "BUILD"),
        "Moo BUILD hook must not be reported unused; got: {unused_names:?}",
    );
    Ok(())
}

#[test]
fn find_unused_symbols_skips_moo_demolish_hook() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    index_str(
        &index,
        "file:///Destroyable.pm",
        "package Destroyable;\n\
         use Moo;\n\
         sub DEMOLISH {\n\
             my ($self) = @_;\n\
         }\n\
         1;\n",
    )?;

    let unused_names: Vec<String> = index
        .find_unused_symbols()
        .into_iter()
        .map(|s| s.name.clone())
        .collect();

    assert!(
        !unused_names.iter().any(|n| n == "DEMOLISH"),
        "Moo DEMOLISH hook must not be reported unused; got: {unused_names:?}",
    );
    Ok(())
}

#[test]
fn find_unused_symbols_skips_exporter_package_arrays() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    index_str(
        &index,
        "file:///Util.pm",
        "package Util;\n\
         use Exporter 'import';\n\
         our @EXPORT_OK = qw(foo);\n\
         sub foo { 1 }\n\
         1;\n",
    )?;

    let unused_names: Vec<String> = index
        .find_unused_symbols()
        .into_iter()
        .map(|s| s.name.clone())
        .collect();

    assert!(
        !unused_names.iter().any(|n| n == "EXPORT_OK" || n == "@EXPORT_OK"),
        "@EXPORT_OK must not be reported unused (read by Exporter); got: {unused_names:?}",
    );
    Ok(())
}
