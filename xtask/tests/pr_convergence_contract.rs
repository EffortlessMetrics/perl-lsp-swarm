//! Regression contract for PLSP-SPEC-0006 and issue #4560.

use std::collections::BTreeSet;
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

fn prose(text: &str) -> String {
    text.replace('`', "")
        .replace('*', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn section<'a>(document: &'a str, heading: &str) -> Result<&'a str, String> {
    let start = document
        .find(heading)
        .ok_or_else(|| format!("missing section {heading:?}"))?;
    let tail = &document[start + heading.len()..];
    let end = tail.find("\n## ").unwrap_or(tail.len());
    Ok(&tail[..end])
}

fn table_rows(section: &str) -> Vec<Vec<String>> {
    section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with('|') {
                return None;
            }
            let cells = trimmed
                .trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().trim_matches('`').to_string())
                .collect::<Vec<_>>();
            if cells.is_empty()
                || cells.iter().all(|cell| cell.chars().all(|ch| ch == '-' || ch == ':'))
                || matches!(
                    cells.first().map(String::as_str),
                    Some("Disposition" | "Observation" | "Later event")
                )
            {
                return None;
            }
            Some(cells)
        })
        .collect()
}

fn validate_contract(spec: &str) -> Result<(), String> {
    let required_markers = [
        "# PLSP-SPEC-0006: PR semantic incorporation and disposition",
        "Status: accepted (amended 2026-08-11)",
        "Those requirements are superseded by this amendment.",
        "### Semantic candidate and proof",
        "### Integration",
        "### Live required status",
        "### Merge race and landed result",
        "There is no mechanical one-rebase limit.",
        "gh pr merge <n> --squash --match-head-commit <current-head-sha>",
    ];
    for marker in required_markers {
        if !spec.contains(marker) {
            return Err(format!("missing current semantic-convergence marker {marker:?}"));
        }
    }

    if spec.lines().any(|line| line.starts_with("Linked plan:")) {
        return Err("obsolete release-plan authority remains".to_string());
    }

    let disposition_rows = table_rows(section(spec, "## Canonical dispositions")?);
    let actual_dispositions = disposition_rows
        .iter()
        .filter_map(|row| row.first().cloned())
        .collect::<BTreeSet<_>>();
    let expected_dispositions = [
        "MERGE_EXISTING_CANDIDATE",
        "REPAIR_EXISTING_CANDIDATE",
        "RESOLVE_CONFLICT",
        "REVIEW_INTEGRATION_INTERACTION",
        "RECONCILE_BASE_FOR_CONCRETE_REASON",
        "SALVAGE_UNIQUE_DELTA",
        "SUPERSEDED_WITH_EVIDENCE",
        "NOT_PROVEN",
        "BLOCKED",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();
    if actual_dispositions != expected_dispositions {
        return Err(format!(
            "canonical disposition set changed: expected {expected_dispositions:?}, got {actual_dispositions:?}"
        ));
    }

    let invalidation_rows = table_rows(section(spec, "## Invalidation matrix")?);
    let unrelated_main = invalidation_rows
        .iter()
        .find(|row| row.first().is_some_and(|cell| prose(cell) == "unrelated main movement"))
        .ok_or_else(|| "missing unrelated-main invalidation row".to_string())?;
    if unrelated_main
        .last()
        .is_none_or(|response| !prose(response).contains("leave the candidate unchanged"))
    {
        return Err("unrelated main movement no longer preserves the candidate".to_string());
    }

    let observation_rows = table_rows(section(spec, "## Conflict and unknown-state semantics")?);
    let behind_only = observation_rows
        .iter()
        .find(|row| row.first().is_some_and(|cell| cell == "BEHIND_ONLY"))
        .ok_or_else(|| "missing BEHIND_ONLY observation".to_string())?;
    if behind_only
        .last()
        .is_none_or(|route| !prose(route).contains("no required action"))
    {
        return Err("BEHIND_ONLY must remain a no-action observation".to_string());
    }

    let base_section = prose(section(spec, "## Base reconciliation and rebase")?);
    for required in [
        "a reconcile_base_for_concrete_reason disposition must name at least one reason",
        "an actual textual conflict",
        "current main changed the same semantic contract",
        "an explicit stack prerequisite changed",
        "live branch protection or merge-queue policy requires a current integration basis",
        "selected proof cannot be interpreted without incorporating the prerequisite",
    ] {
        if !base_section.contains(required) {
            return Err(format!("base reconciliation lost prerequisite {required:?}"));
        }
    }
    for insufficient in [
        "the candidate is old or inactive",
        "the branch is many commits behind",
        "unrelated files changed on main",
        "a current status is missing",
        "a prior rebase already happened or has not happened yet",
    ] {
        if !base_section.contains(insufficient) {
            return Err(format!("base reconciliation lost insufficiency rule {insufficient:?}"));
        }
    }

    let normalized = prose(spec);
    for forbidden in [
        "every candidate must update its base before merge",
        "all candidates must rebase before merge",
        "behind branches must be updated before merge",
        "every pull request must rebase onto current main",
        "the branch must be current with main before merge",
    ] {
        if normalized.contains(forbidden) {
            return Err(format!("mandatory-refresh paraphrase restored: {forbidden:?}"));
        }
    }

    Ok(())
}

#[test]
fn accepted_spec_has_closed_semantic_model() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let spec = read(
        &root,
        "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md",
    )?;

    if let Err(error) = validate_contract(&spec) {
        panic!("PLSP-SPEC-0006: {error}");
    }
    Ok(())
}

#[test]
fn ratchet_rejects_paraphrased_mandatory_refresh() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let spec = read(
        &root,
        "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md",
    )?;
    let mutated = format!("{spec}\n\nEvery candidate must update its base before merge.\n");

    assert!(
        validate_contract(&mutated).is_err(),
        "paraphrased mandatory-base-refresh doctrine must fail the contract"
    );
    Ok(())
}

#[test]
fn ratchet_rejects_age_driven_disposition() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let spec = read(
        &root,
        "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md",
    )?;
    let anchor = "| `NOT_PROVEN` | Required source, review, proof, policy, or tool evidence could not be established |";
    let replacement = format!("| `NEEDS_REBASE` | The branch is behind `main` |\n{anchor}");
    let mutated = spec.replacen(anchor, &replacement, 1);

    assert_ne!(mutated, spec, "disposition mutation fixture must apply");
    assert!(
        validate_contract(&mutated).is_err(),
        "an added age/behind disposition must fail the closed model"
    );
    Ok(())
}

#[test]
fn ratchet_rejects_behind_only_update_route() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let spec = read(
        &root,
        "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md",
    )?;
    let current = "| `BEHIND_ONLY` | The candidate is conflict-free while `main` advanced | no required action |";
    let regressed = "| `BEHIND_ONLY` | The candidate is conflict-free while `main` advanced | update the branch before merge |";
    let mutated = spec.replacen(current, regressed, 1);

    assert_ne!(mutated, spec, "BEHIND_ONLY mutation fixture must apply");
    assert!(
        validate_contract(&mutated).is_err(),
        "behind-only branch mutation must fail the contract"
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
