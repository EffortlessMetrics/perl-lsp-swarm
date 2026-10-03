//! Asserts the lower-crate dependency contract for `perl-sync-convergence`.
//!
//! `perl-sync-convergence` owns `perl_lsp.convergence_transaction.v1` (#11282)
//! and must sit below every sync consumer (merge-event intake, projection
//! publication, source landing, sync-health, reverse convergence): no editor,
//! LSP/DAP runtime, async runtime, parser/workspace crate, Git subprocess, or
//! network client, directly or transitively.
//!
//! This test shells out to `cargo tree` and **fails closed**: if the dependency
//! graph cannot be established, the contract is unproven and the test fails.

// Failing closed is the point of this file: an unavailable instrument must
// abort the test rather than return a passing verdict.
#![allow(clippy::panic)]

use std::process::Command;

/// Crates that must never appear in this crate's normal dependency tree.
const FORBIDDEN: &[&str] = &[
    // LSP/DAP/editor runtime
    "lsp-types",
    "tokio",
    "tower-lsp",
    "perl-lsp-rs",
    "perl-lsp-rs-core",
    "perllsp",
    "perl-dap",
    // Workspace/project model
    "perl-workspace",
    "perl-workspace-core",
    // Parser implementation
    "perl-parser",
    "perl-parser-core",
    "perl-lexer",
    "perl-ast",
    // Git/network/process surfaces
    "git2",
    "reqwest",
    "octocrab",
];

/// Exact allowlist of the reviewed normal dependency closure.
///
/// A new transitive dependency fails the contract until it is reviewed and
/// recorded here; a denylist alone would silently admit anything nobody
/// thought to forbid.
const PERMITTED: &[&str] = &[
    "perl-sync-convergence",
    // serde
    "serde",
    "serde_core",
    "serde_derive",
    // serde_json (canonical JSON persistence)
    "serde_json",
    "itoa",
    "memchr",
    "zmij",
    // sha2 (RustCrypto)
    "sha2",
    "block-buffer",
    "cfg-if",
    "const-oid",
    "cpufeatures",
    "crypto-common",
    "digest",
    "hybrid-array",
    "typenum",
    // proc-macro support for serde_derive
    "proc-macro2",
    "quote",
    "syn",
    "unicode-ident",
];

fn dependency_tree() -> String {
    let output = Command::new(env!("CARGO"))
        .args(["tree", "-p", "perl-sync-convergence", "--edges", "normal", "--prefix", "none"])
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "dependency contract is unproven: could not run `cargo tree` ({error}). \
                 The contract fails closed rather than skipping."
            )
        });

    assert!(
        output.status.success(),
        "dependency contract is unproven: `cargo tree` exited with {}. stderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn package_names(tree: &str) -> Vec<&str> {
    let mut names: Vec<&str> = tree
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

#[test]
fn no_forbidden_dependencies() {
    let tree = dependency_tree();
    let names = package_names(&tree);

    for forbidden in FORBIDDEN {
        assert!(
            !names.contains(forbidden),
            "perl-sync-convergence must not depend on `{forbidden}`, \
             but it appeared in `cargo tree`:\n{tree}"
        );
    }
}

#[test]
fn dependency_closure_is_exactly_permitted() {
    let tree = dependency_tree();
    let names = package_names(&tree);

    let unexpected: Vec<&str> =
        names.iter().copied().filter(|name| !PERMITTED.contains(name)).collect();

    assert!(
        unexpected.is_empty(),
        "perl-sync-convergence gained unreviewed dependencies: {unexpected:?}\n\
         Every crate in the closure must be justified and added to `PERMITTED`.\nFull tree:\n{tree}"
    );

    assert!(
        names.contains(&"perl-sync-convergence"),
        "cargo tree output did not contain the crate itself; \
         the instrument is not measuring what it claims:\n{tree}"
    );
}
