// Integration test: `expect()` carries the assertion message on fixture
// parsing. The workspace-wide deny is a production-code rule.
#![allow(clippy::expect_used)]
//! Regression contract for the provider-native review backstop in issue #6060.

use std::fs;
use std::path::PathBuf;

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn assert_review_backstop(skill: &str, provider: &str) {
    for marker in [
        "## Review predecessor",
        "REVIEW_REQUIRED",
        "CHANGES_REQUIRED",
        "REVIEW_CURRENT",
        "## Integration predecessor",
        "INTEGRATION_READY",
        "PR_IN_FLIGHT",
        "## Protected merge",
        "REVIEW_CURRENT\nAND\nINTEGRATION_READY",
    ] {
        assert!(
            skill.contains(marker),
            "{provider} merge-reconcile must retain review backstop marker {marker:?}"
        );
    }

    let review =
        skill.find("## Review predecessor").expect("review predecessor marker checked above");
    let integration = skill
        .find("## Integration predecessor")
        .expect("integration predecessor marker checked above");
    let merge = skill.find("## Protected merge").expect("protected merge marker checked above");

    assert!(
        review < integration && integration < merge,
        "{provider} merge-reconcile must establish review before integration and integration before merge"
    );
    assert!(
        !skill.contains("REVIEW_PROTOCOL_ENFORCE=1"),
        "{provider} merge-reconcile must not restore the retired exact-head review receipt gate"
    );
}

#[test]
fn merge_requires_review_and_integration() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let codex = fs::read_to_string(root.join(".agents/skills/merge-reconcile/SKILL.md"))?;
    let claude = fs::read_to_string(root.join(".claude/skills/merge-reconcile/SKILL.md"))?;

    assert_review_backstop(&codex, "Codex");
    assert_review_backstop(&claude, "Claude");

    assert!(
        codex.contains("`REVIEW_REQUIRED` → `$finish-pr` / `$final-challenge`"),
        "Codex direct merge invocation must route backward through provider-native PR convergence"
    );
    assert!(
        claude.contains("`REVIEW_REQUIRED` → `finish-pr` / `final-challenge`"),
        "Claude direct merge invocation must route backward through provider-native PR convergence"
    );

    Ok(())
}

fn check_reconciliation_boundaries(
    skill: &str,
    provider: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let reconciliation =
        skill.split_once("## Reconciliation\n").ok_or("missing reconciliation procedure")?.1;
    let reconciliation =
        reconciliation.split_once("\n## ").map_or(reconciliation, |(section, _)| section);
    for marker in [
        "git diff <review-base> <reviewed-head> -- <claim-paths>",
        "git rev-parse <merge>^1",
        "git diff <merge>^1 <merge> -- <claim-paths>",
        "does not need whole-file equality",
        "Provenance does not prove semantic independence",
        "proof/review before claiming the landed result",
        "only for a decision outside existing authority",
        "existing stop/ownership rules for unexpected local uncommitted",
        "When another\nauthorized claim can progress",
        "no remaining useful authorized action",
    ] {
        if !reconciliation.contains(marker) {
            return Err(format!("{provider} reconciliation lost boundary {marker:?}").into());
        }
    }
    Ok(())
}

/// Static procedure-preservation check, not proof of an agent's reconciliation
/// behavior. Actual comparison evidence and independent judgment remain necessary.
#[test]
fn reconciliation_preserves_comparison_and_authority_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    for provider in [".agents", ".claude"] {
        let skill =
            fs::read_to_string(root.join(provider).join("skills/merge-reconcile/SKILL.md"))?;
        check_reconciliation_boundaries(&skill, provider)?;

        let discovery = "git rev-parse <merge>^1";
        let without_discovery = skill.replacen(discovery, "", 1);
        if check_reconciliation_boundaries(&without_discovery, provider).is_ok() {
            return Err(format!("{provider} accepted missing parent discovery").into());
        }
        let misplaced_discovery = format!("{without_discovery}\n## Later procedure\n{discovery}\n");
        if check_reconciliation_boundaries(&misplaced_discovery, provider).is_ok() {
            return Err(format!("{provider} accepted discovery outside reconciliation").into());
        }
    }
    Ok(())
}
