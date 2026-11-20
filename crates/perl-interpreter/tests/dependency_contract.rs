//! Keep interpreter discovery below the application stack (#19439).
//!
//! Cargo supplies the resolved dependency closure, including aliases and target
//! dependencies. Parse package identities rather than searching manifest text.
//! An unavailable graph is a failed proof, never a skipped test.

use std::collections::BTreeSet;
use std::process::Command;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn package_names(tree: &str) -> Result<BTreeSet<&str>, String> {
    tree.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            match (parts.next(), parts.next()) {
                (Some(name), Some(version)) if version.starts_with('v') => Ok(name),
                _ => Err(format!("unrecognized Cargo package row: {line:?}")),
            }
        })
        .collect()
}

fn dependency_tree(package: &str, edges: &str) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "tree",
            "--locked",
            "--package",
            package,
            "--edges",
            edges,
            "--target",
            "all",
            "--all-features",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "dependency graph is unproven: cargo tree for {package} exited with {}; stderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
        )
        .into());
    }
    let tree = String::from_utf8(output.stdout)?;
    if !package_names(&tree)?.contains(package) {
        return Err(
            format!("Cargo graph did not contain selected package {package}:\n{tree}").into()
        );
    }
    Ok(tree)
}

fn forbidden_leaf_dependency(name: &str) -> bool {
    (name.starts_with("perl-") && !matches!(name, "perl-interpreter" | "perl-test-must"))
        || matches!(name, "perllsp" | "lsp-types" | "tower-lsp" | "tokio" | "async-std" | "smol")
}

#[test]
fn production_dependency_closure_is_only_anyhow() -> TestResult {
    let tree = dependency_tree("perl-interpreter", "normal,build")?;
    assert_eq!(
        package_names(&tree)?,
        BTreeSet::from(["anyhow", "perl-interpreter"]),
        "interpreter discovery gained an unreviewed production dependency:\n{tree}",
    );
    Ok(())
}

#[test]
fn test_and_target_dependencies_cannot_reintroduce_the_product_stack() -> TestResult {
    let tree = dependency_tree("perl-interpreter", "normal,build,dev")?;
    let names = package_names(&tree)?;
    let forbidden: Vec<_> =
        names.into_iter().filter(|name| forbidden_leaf_dependency(name)).collect();
    assert!(forbidden.is_empty(), "forbidden interpreter dependencies: {forbidden:?}\n{tree}");
    Ok(())
}

#[test]
fn language_server_normal_and_build_closure_excludes_the_debugger() -> TestResult {
    let tree = dependency_tree("perl-lsp-rs", "normal,build")?;
    assert!(
        !package_names(&tree)?.contains("perl-dap"),
        "language-server builds must not compile the debugger, directly or transitively:\n{tree}",
    );
    Ok(())
}

#[test]
fn graph_parser_distinguishes_package_identity_from_paths_and_duplicates() -> TestResult {
    let graph = "perl-interpreter v0.17.0 (/checkout/perl-dap/crates/perl-interpreter)\n\
                 anyhow v1.0.102\nanyhow v1.0.102 (*)\n\n";
    assert_eq!(package_names(graph)?, BTreeSet::from(["anyhow", "perl-interpreter"]));
    assert!(package_names("unrecognized output").is_err());
    Ok(())
}

#[test]
fn negative_control_detects_transitive_and_renamed_product_dependencies() -> TestResult {
    // Cargo prints the actual package name even when Cargo.toml renames the
    // dependency. A transitive edge must be rejected just like a direct edge.
    let original_graph = "perl-lsp-rs v0.17.0 (/repo/crates/perl-lsp-rs)\n\
                          intermediary v1.0.0\nperl-dap v0.17.0 (/repo/crates/perl-dap)\n";
    assert!(package_names(original_graph)?.contains("perl-dap"));
    for name in [
        "perl-dap",
        "perl-lsp-rs-core",
        "perl-parser-core",
        "perl-workspace-core",
        "perl-tdd-support",
        "tokio",
    ] {
        assert!(forbidden_leaf_dependency(name), "must reject {name}");
    }
    for name in ["anyhow", "tempfile", "perl-test-must", "perl-interpreter"] {
        assert!(!forbidden_leaf_dependency(name), "must retain {name}");
    }
    Ok(())
}
