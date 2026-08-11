//! Regression contract for current maintainer and contributor authority in #4555.

use std::fs;
use std::path::{Path, PathBuf};

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn read(root: &Path, path: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(fs::read_to_string(root.join(path))?)
}

fn assert_contains_all(surface: &str, name: &str, required: &[&str]) {
    for marker in required {
        assert!(
            surface.contains(marker),
            "{name} must retain current-authority marker {marker:?}"
        );
    }
}

fn assert_contains_none(surface: &str, name: &str, forbidden: &[&str]) {
    for marker in forbidden {
        assert!(
            !surface.contains(marker),
            "{name} restored superseded authority marker {marker:?}"
        );
    }
}

#[test]
fn maintainer_contract_is_current() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let doctrine = read(&root, "docs/reference/MAINTAINER_AGENT_DOCTRINE.md")?;

    assert_contains_all(
        &doctrine,
        "maintainer doctrine",
        &[
            "Status: current authority",
            "maintainer or system ruling",
            "evidence, current source, and external constraints",
            "one mutation owner",
            "Behind-only movement requires no action.",
            "There is no mechanical one-rebase limit.",
            "Labels are navigation.",
        ],
    );
    assert_contains_none(
        &doctrine,
        "maintainer doctrine",
        &[
            "prefer GitHub branch update or ordinary rebase",
            "the north star for *why* the conveyor",
            "The conveyor",
            "agents propose; the reconciler disposes",
        ],
    );

    Ok(())
}

#[test]
fn contributing_uses_current_review_model() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let contributing = read(&root, "CONTRIBUTING.md")?;

    assert_contains_all(
        &contributing,
        "CONTRIBUTING.md",
        &[
            "agent contributing guide",
            "There is no fixed two-model review ladder",
            "Labels may help navigation. They are not proof or merge permission.",
            "Behind-only movement requires no action.",
            "At merge, the current head is used as compare-and-swap protection",
        ],
    );
    assert_contains_none(
        &contributing,
        "CONTRIBUTING.md",
        &[
            "haiku-tier",
            "sonnet-tier",
            "The CI merge gate only runs on `merge-ready` PRs",
            "`merge-ready` | Approved and ready for merge",
            "--base origin/master",
        ],
    );

    Ok(())
}

#[test]
fn copilot_is_a_current_route_map() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let copilot = read(&root, ".github/copilot-instructions.md")?;

    assert_contains_all(
        &copilot,
        "Copilot instructions",
        &[
            "This file is a concise route map",
            "Historical articles, forensics, completed implementation specs",
            "one mutation owner",
            "There is no one-rebase quota.",
            "Required GitHub statuses remain attached to the commit they evaluated.",
        ],
    );
    assert_contains_none(
        &copilot,
        "Copilot instructions",
        &[
            "80+ crates",
            "CI is optional/opt-in",
            "`/crates/perl-lsp/`",
            "`perl-workspace-index`",
            "Gate 1",
            "Gate 7",
        ],
    );

    Ok(())
}

#[test]
fn worktree_mutation_has_a_concrete_reason() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let protocol = read(&root, "docs/reference/WORKTREE_PROTOCOL.md")?;

    assert_contains_all(
        &protocol,
        "worktree protocol",
        &[
            "Status: current operational reference",
            "one mutation owner",
            "Behind-only movement requires no action.",
            "There is no mechanical one-rebase limit.",
            "The repository's `scripts/cargo-safe` and `just agent-*` commands are a deliberate",
            "--force-with-lease=\"refs/heads/<branch>:<expected-old-sha>\"",
            "A squash merge does not preserve feature-branch ancestry on `main`.",
        ],
    );
    assert_contains_none(
        &protocol,
        "worktree protocol",
        &[
            "origin/master",
            "main checkout stays on `master`",
            "claim/lease protocol described in issue",
            "restrict each box to a disjoint set of issue numbers",
        ],
    );

    Ok(())
}

#[test]
fn workflow_tracks_current_entrypoints() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let workflow = read(&root, ".github/workflows/active-authority-contract.yml")?;

    for path in [
        "CONTRIBUTING.md",
        ".github/copilot-instructions.md",
        "docs/reference/MAINTAINER_AGENT_DOCTRINE.md",
        "docs/reference/WORKTREE_PROTOCOL.md",
        "xtask/tests/active_authority_contract.rs",
        ".github/workflows/active-authority-contract.yml",
    ] {
        assert!(
            workflow.contains(&format!("- '{path}'")),
            "active-authority workflow must trigger for {path}"
        );
    }
    assert!(
        workflow.contains("cargo test -p xtask --test active_authority_contract --locked"),
        "active-authority workflow must run the regression contract"
    );

    Ok(())
}
