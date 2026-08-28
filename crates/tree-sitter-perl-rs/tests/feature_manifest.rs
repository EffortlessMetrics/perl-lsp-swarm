use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const OVERLAY_ONLY_PACKAGES: [&str; 2] = ["perl-module", "perl-semantic-analyzer"];

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn manifest_path() -> PathBuf {
    crate_root().join("Cargo.toml")
}

fn manifest_text() -> Result<String, std::io::Error> {
    fs::read_to_string(manifest_path())
}

fn cargo_binary() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

fn resolved_normal_packages(feature_args: &[&str]) -> TestResult<BTreeSet<String>> {
    let output = Command::new(cargo_binary())
        .arg("tree")
        .arg("--manifest-path")
        .arg(manifest_path())
        .args([
            "--package",
            "tree-sitter-perl-rs",
            "--locked",
            "--edges",
            "normal",
            "--prefix",
            "none",
        ])
        .args(feature_args)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo tree failed for {feature_args:?}: {stderr}").into());
    }

    let stdout = String::from_utf8(output.stdout)?;
    Ok(stdout
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect())
}

fn assert_packages_absent(graph: &BTreeSet<String>, label: &str) {
    for package in OVERLAY_ONLY_PACKAGES {
        assert!(
            !graph.contains(package),
            "{label} resolved overlay-only package {package}: {graph:?}"
        );
    }
}

fn assert_packages_present(graph: &BTreeSet<String>, label: &str) {
    for package in ["perl-module", "perl-pragma", "perl-semantic-analyzer"] {
        assert!(
            graph.contains(package),
            "{label} did not resolve semantic-overlay package {package}: {graph:?}"
        );
    }
}

fn assert_cfg_owned(source: &str, item: &str, path: &Path) {
    let expected = format!("#[cfg(feature = \"semantic-overlay\")]\n{item}");
    assert!(source.contains(&expected), "{} does not feature-own {item:?}", path.display());
}

#[test]
fn semantic_overlay_feature_owns_its_upper_dependencies() -> TestResult {
    let manifest = manifest_text()?;

    for expected in [
        "default = []",
        "queries = [\"dep:regex\"]",
        "semantic-overlay = [\n    \"dep:perl-module\",\n    \"dep:perl-pragma\",\n    \"dep:perl-semantic-analyzer\",\n]",
        "perl-module = { workspace = true, optional = true }",
        "perl-pragma = { workspace = true, optional = true }",
        "perl-semantic-analyzer = { workspace = true, optional = true }",
        "name = \"semantic_overlay_queries\"\nrequired-features = [\"semantic-overlay\"]",
        "name = \"semantic_overlay_tests\"\nrequired-features = [\"semantic-overlay\"]",
        "\"examples/semantic_overlay_queries.rs\"",
    ] {
        assert!(manifest.contains(expected), "manifest contract missing {expected:?}");
    }

    let lib_path = crate_root().join("src/lib.rs");
    let lib = fs::read_to_string(&lib_path)?;
    assert_cfg_owned(&lib, "mod semantic_overlay;", &lib_path);
    assert_cfg_owned(
        &lib,
        "pub use semantic_overlay::{OverlayDefinition, SemanticOverlay, VisibleImport};",
        &lib_path,
    );

    let tree_path = crate_root().join("src/tree.rs");
    let tree = fs::read_to_string(&tree_path)?;
    assert_cfg_owned(&tree, "use crate::SemanticOverlay;", &tree_path);
    assert_cfg_owned(&tree, "pub fn semantic_overlay(&self) -> SemanticOverlay<'_> {", &tree_path);

    Ok(())
}

#[test]
fn default_and_query_graphs_exclude_overlay_only_packages() -> TestResult {
    let base = resolved_normal_packages(&["--no-default-features"])?;
    assert_packages_absent(&base, "no-default-features graph");

    let queries = resolved_normal_packages(&["--no-default-features", "--features", "queries"])?;
    assert_packages_absent(&queries, "queries-only graph");
    assert!(queries.contains("regex"), "queries-only graph did not resolve regex: {queries:?}");

    Ok(())
}

#[test]
fn semantic_overlay_and_all_feature_graphs_include_overlay_packages() -> TestResult {
    let overlay =
        resolved_normal_packages(&["--no-default-features", "--features", "semantic-overlay"])?;
    assert_packages_present(&overlay, "semantic-overlay graph");

    let all = resolved_normal_packages(&["--all-features"])?;
    assert_packages_present(&all, "all-features graph");
    assert!(all.contains("regex"), "all-features graph did not resolve regex: {all:?}");

    Ok(())
}
