use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const OVERLAY_ONLY_PACKAGES: [&str; 2] = ["perl-module", "perl-semantic-analyzer"];

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn require(condition: bool, message: String) -> TestResult {
    if condition { Ok(()) } else { Err(message.into()) }
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn manifest_path() -> PathBuf {
    crate_root().join("Cargo.toml")
}

fn manifest_text() -> Result<String, std::io::Error> {
    fs::read_to_string(manifest_path())
}

fn normalize_whitespace(value: &str) -> String {
    value.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
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

fn assert_packages_absent(graph: &BTreeSet<String>, label: &str) -> TestResult {
    for package in OVERLAY_ONLY_PACKAGES {
        require(
            !graph.contains(package),
            format!("{label} resolved overlay-only package {package}: {graph:?}"),
        )?;
    }
    Ok(())
}

fn assert_packages_present(graph: &BTreeSet<String>, label: &str) -> TestResult {
    for package in ["perl-module", "perl-pragma", "perl-semantic-analyzer"] {
        require(
            graph.contains(package),
            format!("{label} did not resolve semantic-overlay package {package}: {graph:?}"),
        )?;
    }
    Ok(())
}

fn assert_cfg_owned(source: &str, item: &str, path: &Path) -> TestResult {
    let expected = format!("#[cfg(feature = \"semantic-overlay\")]\n{item}");
    require(
        normalize_whitespace(source).contains(&normalize_whitespace(&expected)),
        format!("{} does not feature-own {item:?}", path.display()),
    )?;
    Ok(())
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
        "\"examples/**\"",
    ] {
        require(
            normalize_whitespace(&manifest).contains(&normalize_whitespace(expected)),
            format!("manifest contract missing {expected:?}"),
        )?;
    }

    for relative_path in ["examples/semantic_overlay_queries.rs", "tests/semantic_overlay_tests.rs"]
    {
        let path = crate_root().join(relative_path);
        require(path.is_file(), format!("manifest contract path missing {}", path.display()))?;
    }

    // The facade is split into modules, so the ledger pins the feature gate at
    // the two compilation-relevant ownership points — the gated `mod`
    // declaration plus its re-export in `lib.rs`, and the gated `Tree`
    // entry-point method in `tree.rs` — and then asserts that every
    // overlay-owned item (upper-stack imports, public types, query impl, and
    // crate-private traversal helper) lives in the feature-owned module file.
    // Items inside that file need no per-item gate: it never compiles without
    // the feature. Each lib/tree assertion is matched together with the
    // attribute immediately above it, so deleting a single gate fails here even
    // though the crate still compiles with the feature on.
    let lib_path = crate_root().join("src/lib.rs");
    let lib = fs::read_to_string(&lib_path)?;

    assert_cfg_owned(&lib, "mod semantic_overlay;", &lib_path)?;
    assert_cfg_owned(
        &lib,
        "pub use semantic_overlay::{OverlayDefinition, SemanticOverlay, VisibleImport};",
        &lib_path,
    )?;

    // The `Tree` entry point and its body: a gate on the signature alone would
    // still admit a body that resolved the overlay unconditionally.
    let tree_path = crate_root().join("src/tree.rs");
    let tree_source = fs::read_to_string(&tree_path)?;
    assert_cfg_owned(
        &tree_source,
        "pub fn semantic_overlay(&self) -> SemanticOverlay<'_> {\n        SemanticOverlay { tree: self }\n    }",
        &tree_path,
    )?;

    // The feature-owned module file: the three direct upper-stack imports (the
    // edges that make the dependency-graph assertions below reachable at all),
    // the three public overlay types (each anchored by its first doc line), the
    // query implementations, and the crate-private traversal helper they are
    // the only callers of.
    let overlay_path = crate_root().join("src/semantic_overlay.rs");
    let overlay = fs::read_to_string(&overlay_path)?;

    for import in [
        "use perl_module::parse_module_import_head;",
        "use perl_pragma::{PragmaState, PragmaTracker};",
        "use perl_semantic_analyzer::semantic::SemanticModel;",
    ] {
        require(
            normalize_whitespace(&overlay).contains(&normalize_whitespace(import)),
            format!(
                "{} does not contain the feature-owned import {import:?}",
                overlay_path.display()
            ),
        )?;
    }

    for doc_anchor in [
        "/// Experimental semantic overlay query handle.",
        "/// Symbol definition returned by [`SemanticOverlay`] queries.",
        "/// Import statement visible at a specific source offset.",
    ] {
        require(
            normalize_whitespace(&overlay).contains(&normalize_whitespace(doc_anchor)),
            format!(
                "{} does not contain the feature-owned item {doc_anchor:?}",
                overlay_path.display()
            ),
        )?;
    }

    require(
        normalize_whitespace(&overlay)
            .contains(&normalize_whitespace("impl<'tree> SemanticOverlay<'tree> {")),
        format!("{} does not contain the overlay query impl", overlay_path.display()),
    )?;
    require(
        normalize_whitespace(&overlay)
            .contains(&normalize_whitespace("fn collect_visible_use_imports(")),
        format!("{} does not contain the overlay traversal helper", overlay_path.display()),
    )?;

    Ok(())
}

#[test]
fn default_and_query_graphs_exclude_overlay_only_packages() -> TestResult {
    let base = resolved_normal_packages(&["--no-default-features"])?;
    assert_packages_absent(&base, "no-default-features graph")?;
    require(
        base.contains("perl-pragma"),
        format!(
            "no-default-features graph did not resolve parser-owned transitive edge perl-pragma through perl-parser-core: {base:?}"
        ),
    )?;

    let queries = resolved_normal_packages(&["--no-default-features", "--features", "queries"])?;
    assert_packages_absent(&queries, "queries-only graph")?;
    require(
        queries.contains("perl-pragma"),
        format!(
            "queries-only graph did not resolve parser-owned transitive edge perl-pragma through perl-parser-core: {queries:?}"
        ),
    )?;
    require(
        queries.contains("regex"),
        format!("queries-only graph did not resolve regex: {queries:?}"),
    )?;

    Ok(())
}

#[test]
fn semantic_overlay_and_all_feature_graphs_include_overlay_packages() -> TestResult {
    let overlay =
        resolved_normal_packages(&["--no-default-features", "--features", "semantic-overlay"])?;
    assert_packages_present(&overlay, "semantic-overlay graph")?;

    let all = resolved_normal_packages(&["--all-features"])?;
    assert_packages_present(&all, "all-features graph")?;
    require(all.contains("regex"), format!("all-features graph did not resolve regex: {all:?}"))?;

    Ok(())
}
