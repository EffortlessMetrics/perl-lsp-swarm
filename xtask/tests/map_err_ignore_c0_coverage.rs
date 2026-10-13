//! Executable target-root contract for the C0 `map_err_ignore` cohort (#12598).
//!
//! Every target root of a C0 crate must carry the crate-root deny. A crate
//! joins C0 only by passing the all-features/all-target census first, and a
//! crate leaving C0 must also leave this list. `check-lint-policy` validates
//! the ledger, not per-root coverage — this is the per-root authority the
//! activation review asked for.

use std::process::Command;

/// The 25 C0 crates (census-clean on all features and targets as of #12598).
/// tree-sitter-perl-c was removed to C1: its feature-gated `test-utils`
/// binary carries a live `map_err(|_| ...)` finding.
const C0_CRATES: &[&str] = &[
    "perl-ast-v2",
    "perl-token",
    "perl-source-identity",
    "perl-pragma",
    "perl-regex",
    "perl-parser-bench",
    "perl-core-harness-types",
    "perl-core-test-runner",
    "perl-parser-pest",
    "perl-parser-comparison",
    "perl-semantic-facts",
    "perl-tdd-support",
    "perl-test-must",
    "perl-test-generators",
    "perl-test-facts",
    "perl-lsp-perltidy",
    "perl-diagnostics",
    "perl-symbol",
    "perl-line-index",
    "perl-pod",
    "perl-ripr-facts",
    "perllsp",
    "perl-ci-hygiene",
    "perl-release-readiness",
    "perl-workspace-core",
];

#[test]
fn every_c0_target_root_carries_the_deny() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .output()?;
    assert!(output.status.success(), "cargo metadata must succeed");
    let meta: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let packages = meta["packages"].as_array().ok_or("packages must be an array")?;

    let mut checked = 0usize;
    for package in packages {
        let name = package["name"].as_str().ok_or("package name must be a string")?;
        if !C0_CRATES.contains(&name) {
            continue;
        }
        for target in package["targets"].as_array().ok_or("targets must be an array")? {
            let path = target["src_path"].as_str().ok_or("src_path must be a string")?;
            let content = std::fs::read_to_string(path)?;
            assert!(
                content.contains("deny(clippy::map_err_ignore)"),
                "C0 target root is missing the deny attribute: {path}"
            );
            checked += 1;
        }
    }

    let expected = C0_CRATES.len();
    let mut seen: Vec<&str> = packages
        .iter()
        .filter_map(|package| package["name"].as_str())
        .filter(|name| C0_CRATES.contains(name))
        .collect();
    seen.sort_unstable();
    assert_eq!(seen.len(), expected, "every C0 crate must resolve in cargo metadata");
    assert!(checked > 0, "the contract must cover at least one target root");
    Ok(())
}
