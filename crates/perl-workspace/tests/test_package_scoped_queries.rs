//! Tests for file-scoped and package-scoped symbol queries.
//!
//! Exercises `WorkspaceIndex::file_packages` and
//! `WorkspaceIndex::file_package_symbols` — the two read-only accessors added
//! by issue #900.

use perl_workspace::workspace::workspace_index::WorkspaceIndex;
use url::Url;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn index_with_code(uri: &str, code: &str) -> Result<WorkspaceIndex, Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    let url = Url::parse(uri)?;
    index.index_file(url, code.to_string())?;
    Ok(index)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Statement form (`package Foo;`), block form (`package Bar { }`), and nested
/// package (`package Foo::Nested { }`) all appear in `file_packages()`.
#[test]
fn test_file_packages_statement_and_block_forms() -> Result<(), Box<dyn std::error::Error>> {
    let code = "package Foo;\nsub hello { 1 }\npackage Bar { sub greet { 2 } }\npackage Foo::Nested { sub nested { 3 } }\n";
    let index = index_with_code("file:///lib/Multi.pm", code)?;

    let mut pkgs = index.file_packages("file:///lib/Multi.pm");
    pkgs.sort();
    assert_eq!(pkgs, vec!["Bar", "Foo", "Foo::Nested"], "all three package forms must appear");
    Ok(())
}

/// Symbols (subs, `our` vars) are attributed to the correct package.
#[test]
fn test_file_package_symbols_correct_attribution() -> Result<(), Box<dyn std::error::Error>> {
    let code = "package Foo;\nsub hello { 1 }\npackage Bar { sub greet { 2 } }\nour $shared = 3;\n";
    let index = index_with_code("file:///lib/PkgSym.pm", code)?;

    let bar_syms = index.file_package_symbols("file:///lib/PkgSym.pm", "Bar");
    let bar_names: Vec<&str> = bar_syms.iter().map(|s| s.name.as_str()).collect();
    assert!(bar_names.contains(&"greet"), "Bar must contain greet");
    assert!(!bar_names.contains(&"hello"), "hello must not be in Bar");

    let foo_syms = index.file_package_symbols("file:///lib/PkgSym.pm", "Foo");
    let foo_names: Vec<&str> = foo_syms.iter().map(|s| s.name.as_str()).collect();
    assert!(foo_names.contains(&"hello"), "Foo must contain hello");
    assert!(!foo_names.contains(&"greet"), "greet must not be in Foo");

    Ok(())
}

/// A file with no explicit `package` declaration returns an empty vec.
/// There is no implicit `main` symbol — the extractor does not emit one.
#[test]
fn test_file_packages_empty_for_no_explicit_package() -> Result<(), Box<dyn std::error::Error>> {
    let code = "sub helper { 42 }\n";
    let index = index_with_code("file:///lib/NoPackage.pm", code)?;
    assert!(
        index.file_packages("file:///lib/NoPackage.pm").is_empty(),
        "file with no explicit package decl must return empty vec"
    );
    Ok(())
}

/// Asking for symbols in a package that does not exist in the file returns
/// empty, not an error.
#[test]
fn test_file_package_symbols_nonexistent_package() -> Result<(), Box<dyn std::error::Error>> {
    let code = "package Foo;\nsub bar { }\n";
    let index = index_with_code("file:///lib/Foo.pm", code)?;
    assert!(
        index.file_package_symbols("file:///lib/Foo.pm", "DoesNotExist").is_empty(),
        "nonexistent package must return empty vec"
    );
    Ok(())
}

/// Both methods return empty for a URI that was never indexed.
/// This exercises the `unwrap_or_default()` path.
#[test]
fn test_missing_uri_returns_empty() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    assert!(
        index.file_packages("file:///does/not/exist.pm").is_empty(),
        "file_packages on unknown URI must return empty"
    );
    assert!(
        index.file_package_symbols("file:///does/not/exist.pm", "Foo").is_empty(),
        "file_package_symbols on unknown URI must return empty"
    );
    Ok(())
}

/// Block-scoped package restores outer context after the block closes.
/// A sub declared after `package Inner { }` belongs to the outer package,
/// not to `Inner`.
///
/// If the parser/extractor does not restore context after a block package,
/// this test will fail. Leave it as `#[ignore]` with a note if that occurs.
#[test]
fn test_block_package_restores_outer_context() -> Result<(), Box<dyn std::error::Error>> {
    let code = "package Outer;\npackage Inner { sub inside { } }\nsub outside { }\n";
    let index = index_with_code("file:///lib/Restore.pm", code)?;

    let outer_syms = index.file_package_symbols("file:///lib/Restore.pm", "Outer");
    let outer_names: Vec<&str> = outer_syms.iter().map(|s| s.name.as_str()).collect();
    assert!(
        outer_names.contains(&"outside"),
        "sub after block belongs to Outer, got: {:?}",
        outer_names
    );

    let inner_syms = index.file_package_symbols("file:///lib/Restore.pm", "Inner");
    let inner_names: Vec<&str> = inner_syms.iter().map(|s| s.name.as_str()).collect();
    assert!(
        inner_names.contains(&"inside"),
        "sub inside block belongs to Inner, got: {:?}",
        inner_names
    );

    Ok(())
}
