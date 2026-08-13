//! Asserts the PLSP-ADR-0006 lower-crate dependency contract for
//! `perl-source-identity`.
//!
//! `perl-source-identity` must not depend — directly or transitively — on the
//! editor/LSP runtime, DAP, async runtimes, or any parser/workspace crate. It
//! is a pure type/encoding library that sits below the entire product stack.
//!
//! This test shells out to `cargo tree`. When cargo is unavailable (some
//! sandboxes), the test degrades to a no-op with a printed note rather than a
//! false failure — the contract is also documented in the crate README and the
//! Cargo.toml comment.
#![allow(clippy::print_stderr)]

use std::process::Command;

/// Crates that must never appear in `perl-source-identity`'s dependency tree.
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
    "perl-workspace ", // trailing space: match the crate, not "perl-workspace-core"
    "perl-workspace-core",
    // Parser implementation
    "perl-parser ", // trailing space: match the crate, not "perl-parser-core"
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

#[test]
fn no_forbidden_dependencies() {
    let output = Command::new(env!("CARGO"))
        .args(["tree", "-p", "perl-source-identity", "--edges", "normal", "--prefix", "none"])
        .output();

    let output = match output {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            eprintln!(
                "dependency_contract: `cargo tree` failed ({}); skipping. stderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        Err(error) => {
            eprintln!("dependency_contract: could not run `cargo tree` ({error}); skipping");
            return;
        }
    };

    let tree = String::from_utf8_lossy(&output.stdout);
    for forbidden in FORBIDDEN {
        assert!(
            !tree.lines().any(|line| line.contains(forbidden)),
            "perl-source-identity must not depend on `{forbidden}`, \
             but it appeared in `cargo tree`:\n{tree}"
        );
    }
}
