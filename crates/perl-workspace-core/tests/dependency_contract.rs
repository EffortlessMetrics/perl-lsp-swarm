//! Dependency contract: `perl-workspace-core` is the LSP-free substrate.
//!
//! This crate must remain below LSP, DAP, editor transport, and shipped-product
//! runtime concerns. It consumes parser/semantic/symbol/module/range primitives
//! and produces stable project facts — but it must never pull an editor,
//! protocol, async-runtime, or external-tool-adapter crate into its dependency
//! tree (directly or transitively, via feature unification or otherwise).
//!
//! The test resolves the crate's own normal-edge dependency tree with
//! `cargo tree` and asserts that none of the forbidden crate names appear. If a
//! future change adds a dependency (or enables a feature) that drags one of
//! these in, this test fails loudly at the seam instead of silently eroding the
//! layering.
#![allow(clippy::print_stderr)]

use std::process::Command;

/// Crate names that must never appear in `perl-workspace-core`'s dependency
/// tree. Matched as whole tokens so substrings of unrelated crates do not
/// false-positive.
const FORBIDDEN: &[&str] = &[
    "perl-lsp-rs",
    "perl-lsp-rs-core",
    "perllsp",
    "perl-dap",
    "lsp-types",
    "tokio",
    "tower-lsp",
    "perl-lsp-perltidy",
    "perltidy",
    "perlcritic",
];

/// Parse the crate token out of a `cargo tree` line.
///
/// Lines look like `│   └── lsp-types v0.97.0` or `perl-workspace-core v0.17.0
/// (/path)`. The crate name is the first whitespace-separated token after any
/// leading tree-drawing prefix.
fn crate_token(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches(|c: char| {
        c.is_whitespace() || matches!(c, '│' | '├' | '└' | '─' | '|' | '+' | '\\' | '-')
    });
    let token = trimmed.split_whitespace().next()?;
    if token.is_empty() { None } else { Some(token) }
}

#[test]
fn workspace_core_has_no_editor_or_runtime_deps() {
    let output = Command::new(env!("CARGO"))
        .args(["tree", "-p", "perl-workspace-core", "--edges", "normal"])
        .output();

    let output = match output {
        Ok(o) => o,
        // If cargo is unavailable in the harness, do not spuriously fail — the
        // contract is also enforced by the crate's own `[dependencies]` and by
        // the workspace-level layer check. Skip rather than false-alarm.
        Err(e) => {
            eprintln!("skipping dependency-contract test: cannot run cargo tree: {e}");
            return;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "cargo tree failed:\nstdout:\n{stdout}\nstderr:\n{stderr}");

    let mut violations: Vec<String> = Vec::new();
    for line in stdout.lines() {
        if let Some(token) = crate_token(line) {
            if FORBIDDEN.contains(&token) {
                violations
                    .push(format!("  forbidden dependency `{token}` (line: {})", line.trim()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "perl-workspace-core must not depend on editor/runtime/tool crates, \
         but its dependency tree contains:\n{}\n\nFull tree:\n{stdout}",
        violations.join("\n")
    );
}

#[test]
fn crate_token_parses_tree_lines() {
    assert_eq!(crate_token("perl-workspace-core v0.17.0 (/x)"), Some("perl-workspace-core"));
    assert_eq!(crate_token("│   └── lsp-types v0.97.0"), Some("lsp-types"));
    assert_eq!(crate_token("├── serde v1.0.228"), Some("serde"));
    assert_eq!(
        crate_token("│   └── perl-semantic-facts v0.17.0 (/x) (*)"),
        Some("perl-semantic-facts")
    );
    assert_eq!(crate_token(""), None);
}
