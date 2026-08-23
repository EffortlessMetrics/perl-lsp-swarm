//! Asserts the lower-crate dependency contract for `perl-source-identity`.
//!
//! `perl-source-identity` owns `source_identity.v1` and must sit below the
//! entire product stack: no editor/LSP runtime, no DAP, no async runtime, no
//! parser or workspace crate, directly or transitively. See the "Scope
//! boundary: `source_identity.v1`" section of PLSP-ADR-0006 for why this crate
//! — rather than `perl-workspace-core` — owns durable source identity.
//!
//! This test shells out to `cargo tree` and **fails closed**: if the dependency
//! graph cannot be established, the contract is unproven and the test fails.
//! A proof instrument that cannot run is not evidence of compliance.

// Failing closed is the point of this file: an unavailable instrument must
// abort the test rather than return a passing verdict.
#![allow(clippy::panic)]

use std::process::Command;

/// Crates that must never appear in `perl-source-identity`'s dependency tree.
///
/// Matched against the package name parsed out of each `cargo tree` line, so
/// entries are exact names — `perl-workspace` does not match
/// `perl-workspace-core`.
const FORBIDDEN: &[&str] = &[
    // LSP/DAP/editor runtime
    "lsp-types",
    "tokio",
    "tower-lsp",
    "perl-lsp-rs",
    "perl-lsp-rs-core",
    "perllsp",
    "perl-dap",
    // Workspace/project model (these pull lsp-types transitively)
    "perl-workspace",
    "perl-workspace-core",
    // Parser implementation
    "perl-parser",
    "perl-parser-core",
    "perl-lexer",
    "perl-ast",
    "perl-semantic-analyzer",
    "perl-semantic-facts",
    // Product configuration / trust policy
    "perl-kwalitee",
    "perl-corpus",
    "perl-ripr-facts",
];

/// The complete set of crates permitted in the normal dependency closure.
///
/// This is an exact allowlist, not a denylist: a new transitive dependency
/// fails the contract until it is reviewed and recorded here. A denylist alone
/// would silently admit anything nobody thought to forbid.
///
/// `serde` provides stable serialization; `sha2` provides the reviewed
/// collision-resistant digest. The remainder are their required support crates.
const PERMITTED: &[&str] = &[
    "perl-source-identity",
    // serde
    "serde",
    "serde_core",
    "serde_derive",
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

/// Run `cargo tree` for this crate's normal (non-dev, non-build) edges.
///
/// Panics with a precise diagnostic when the instrument cannot run — the
/// contract is unproven in that case, which is a failure, not a pass.
fn dependency_tree() -> String {
    let output = Command::new(env!("CARGO"))
        .args(["tree", "-p", "perl-source-identity", "--edges", "normal", "--prefix", "none"])
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

/// Parse the distinct package names out of `cargo tree --prefix none` output.
///
/// Each line is `<name> v<version>[ (<source>)]`; deduplicated entries are
/// rendered as `<name> v<version> (*)`. Taking the first whitespace-separated
/// token yields the package name and ignores paths, which may otherwise
/// contain crate-like substrings.
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
            "perl-source-identity must not depend on `{forbidden}`, \
             but it appeared in `cargo tree`:\n{tree}"
        );
    }
}

/// The dependency closure must be exactly what has been reviewed.
///
/// This is the check that catches a genuinely new dependency — including one
/// nobody thought to add to `FORBIDDEN`.
#[test]
fn dependency_closure_is_exactly_permitted() {
    let tree = dependency_tree();
    let names = package_names(&tree);

    let unexpected: Vec<&str> =
        names.iter().copied().filter(|name| !PERMITTED.contains(name)).collect();

    assert!(
        unexpected.is_empty(),
        "perl-source-identity gained unreviewed dependencies: {unexpected:?}\n\
         Every crate in the closure must be justified and added to `PERMITTED` \
         (see the crate README and PLSP-ADR-0006).\nFull tree:\n{tree}"
    );

    assert!(
        names.contains(&"perl-source-identity"),
        "cargo tree output did not contain the crate itself; \
         the instrument is not measuring what it claims:\n{tree}"
    );
}

/// Guards the parser used by both contract assertions.
///
/// Without this, a `package_names` bug that returned nothing would make the
/// forbidden-dependency test pass vacuously.
#[test]
fn package_names_parses_cargo_tree_output() {
    let sample = "perl-source-identity v0.1.0 (/repo/crates/perl-source-identity)\n\
                  serde v1.0.0\n\
                  sha2 v0.11.0\n\
                  digest v0.11.0 (*)\n\
                  \n";
    assert_eq!(package_names(sample), vec!["digest", "perl-source-identity", "serde", "sha2"]);

    // A path containing a crate-like substring must not be read as a package.
    let tricky = "perl-source-identity v0.1.0 (/home/dev/perl-workspace-core/checkout)\n";
    assert_eq!(package_names(tricky), vec!["perl-source-identity"]);
}
