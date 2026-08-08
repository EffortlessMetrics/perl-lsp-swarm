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
