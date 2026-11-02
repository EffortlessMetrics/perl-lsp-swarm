//! Asserts the transport-service dependency contract for `perl-code-intelligence`.
//!
//! The service sits below LSP and MCP adapters and must not import wire framing,
//! editor clients, or the live LSP server runtime (#6957 / #8582).

#![allow(clippy::panic)]
#![deny(clippy::map_err_ignore)]

use std::process::Command;

const FORBIDDEN: &[&str] = &[
    "effortless-lsp",
    "lsp-types",
    "ls-types",
    "rmcp",
    "perl-lsp-rs",
    "perl-lsp-rs-core",
    "perllsp",
    "perl-mcp",
    "perl-dap",
    "tokio",
    "tower-lsp",
];

const PERMITTED: &[&str] = &[
    "perl-code-intelligence",
    "perl-source-identity",
    "serde",
    "serde_core",
    "serde_derive",
    "serde_json",
    "sha2",
    "block-buffer",
    "cfg-if",
    "const-oid",
    "cpufeatures",
    "crypto-common",
    "digest",
    "hybrid-array",
    "typenum",
    "proc-macro2",
    "quote",
    "syn",
    "unicode-ident",
];

fn dependency_tree() -> String {
    let output = Command::new(env!("CARGO"))
        .args(["tree", "-p", "perl-code-intelligence", "--edges", "normal", "--prefix", "none"])
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
            "perl-code-intelligence must not depend on `{forbidden}`, \
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
        "perl-code-intelligence gained unreviewed dependencies: {unexpected:?}\n\
         Full tree:\n{tree}"
    );
}
