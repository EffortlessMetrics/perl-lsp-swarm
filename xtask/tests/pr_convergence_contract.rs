//! Regression contract for PLSP-SPEC-0006 and issue #4560.

use std::fs;
use std::path::PathBuf;

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn read(root: &std::path::Path, path: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(fs::read_to_string(root.join(path))?)
}

#[test]
fn accepted_spec_uses_semantic_incorporation() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let spec = read(
        &root,
        "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md",
    )?;

    for required in [
        "# PLSP-SPEC-0006: PR semantic incorporation and disposition",
        "Status: accepted (amended 2026-08-11)",
        "Those requirements are superseded by this amendment.",
        "### Semantic candidate and proof",
        "### Integration",
        "### Live required status",
        "### Merge race and landed result",
        "There is no mechanical one-rebase limit.",
        "There is no age-driven or behind-driven `needs-rebase` disposition.",
        "Disposition: `MERGE_EXISTING_CANDIDATE`.",
        "gh pr merge <n> --squash --match-head-commit <current-head-sha>",
    ] {
        assert!(
            spec.contains(required),
            "PLSP-SPEC-0006 must retain current semantic-convergence marker {required:?}"
        );
    }

    Ok(())
}

#[test]
fn accepted_spec_does_not_restore_branch_freshness_ceremony(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let spec = read(
        &root,
        "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md",
    )?;

    for forbidden in [
        "Linked plan: [0.14.0 Readiness Queue]",
        "4. Rebase onto current `master`.",
        "Valuable but not yet reviewed against current `master`",
        "fresh proof on current `master`",
        "What proof ran after rebase?",
        "All PR dispositions must run at least the proof required by the touched surface after rebase.",
    ] {
        assert!(
            !spec.contains(forbidden),
            "PLSP-SPEC-0006 restored superseded mandatory-rebase wording {forbidden:?}"
        );
    }

    assert!(
        !spec.lines().any(|line| line.starts_with("Linked plan:")),
        "PLSP-SPEC-0006 must not retain an obsolete release-plan authority"
    );

    Ok(())
}

#[test]
fn catalogs_name_the_current_contract() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let catalog = read(&root, "docs/specs/README.md")?;
    let index = read(&root, "docs/INDEX.md")?;

    assert!(
        catalog.contains("PLSP-SPEC-0006: PR semantic incorporation and disposition"),
        "spec catalog must expose the amended title"
    );
    assert!(
        index.contains("PR Semantic Incorporation and Disposition Spec"),
        "documentation index must point to the amended contract"
    );
    assert!(
        !index.contains("0.14.0 Readiness Queue](releases/0.14.0-readiness.md) — current-release"),
        "documentation index must not present the historical 0.14.0 queue as current"
    );

    Ok(())
}
