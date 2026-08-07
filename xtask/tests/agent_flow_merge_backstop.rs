//! Structural regression tests for the provider-native merge backstop.
//!
//! These tests compile the irreversible route contract without inspecting live
//! GitHub state. They ensure that a direct `merge-reconcile` entry cannot infer
//! substantive review from checks, threads, or mergeability and cannot reach merge
//! without both predecessor judgments.

use std::{fs, path::PathBuf};

const PROVIDER_SKILLS: &[&str] = &[
    ".agents/skills/merge-reconcile/SKILL.md",
    ".claude/skills/merge-reconcile/SKILL.md",
];

const REQUIRED_MARKERS: &[&str] = &[
    "## Predecessor judgments",
    "review missing or not reconstructable",
    "REVIEW_REQUIRED",
    "CHANGES_REQUIRED",
    "address-review-comments",
    "BLOCKED_BY_PREREQUISITE",
    "REVIEW_CURRENT` + `PR_IN_FLIGHT",
    "REVIEW_CURRENT` + `MERGE_BLOCKED",
    "REVIEW_CURRENT` + integration `NOT_PROVEN",
    "REVIEW_CURRENT` + `INTEGRATION_READY",
    "No other combination reaches merge.",
    "compare-and-swap protection",
];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must be inside the repository root")
        .to_path_buf()
}

fn load(relative: &str) -> String {
    let path = repository_root().join(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

fn missing_markers(text: &str) -> Vec<&'static str> {
    REQUIRED_MARKERS
        .iter()
        .copied()
        .filter(|marker| !text.contains(marker))
        .collect()
}

#[test]
fn both_provider_merge_skills_require_review_and_integration() {
    for relative in PROVIDER_SKILLS {
        let text = load(relative);
        let missing = missing_markers(&text);
        assert!(
            missing.is_empty(),
            "{relative} is missing merge-backstop markers: {missing:?}"
        );
    }
}

#[test]
fn review_backstop_marker_is_discriminating() {
    let text = load(PROVIDER_SKILLS[0]);
    let mutated = text.replacen("review missing or not reconstructable", "review already assumed", 1);
    let missing = missing_markers(&mutated);
    assert!(
        missing.contains(&"review missing or not reconstructable"),
        "removing the direct-entry review backstop must fail validation"
    );
}

#[test]
fn integration_conjunction_marker_is_discriminating() {
    let text = load(PROVIDER_SKILLS[0]);
    let mutated = text.replacen(
        "`REVIEW_CURRENT` + `INTEGRATION_READY`",
        "`INTEGRATION_READY`",
        1,
    );
    let missing = missing_markers(&mutated);
    assert!(
        missing.contains(&"REVIEW_CURRENT` + `INTEGRATION_READY"),
        "removing REVIEW_CURRENT from the merge conjunction must fail validation"
    );
}

#[test]
fn pending_integration_marker_is_discriminating() {
    let text = load(PROVIDER_SKILLS[0]);
    let mutated = text.replacen(
        "`REVIEW_CURRENT` + `PR_IN_FLIGHT`",
        "`REVIEW_CURRENT` + `INTEGRATION_READY`",
        1,
    );
    let missing = missing_markers(&mutated);
    assert!(
        missing.contains(&"REVIEW_CURRENT` + `PR_IN_FLIGHT"),
        "pending integration must remain a non-merge route"
    );
}

#[test]
fn explicit_exclusivity_marker_is_discriminating() {
    let text = load(PROVIDER_SKILLS[0]);
    let mutated = text.replacen(
        "No other combination reaches merge.",
        "Other combinations may merge.",
        1,
    );
    let missing = missing_markers(&mutated);
    assert!(
        missing.contains(&"No other combination reaches merge."),
        "the exclusive merge conjunction must remain explicit"
    );
}
