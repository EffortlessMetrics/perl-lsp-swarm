#!/usr/bin/env python3
"""Route workflow inputs with xtask-owned contracts into the xtask test scope."""

from pathlib import Path


path = Path("xtask/src/tasks/ci_scope.rs")
text = path.read_text(encoding="utf-8")

old_guard = '''fn is_xtask_policy_guarded_input(file: &str) -> bool {
    // Packaging contract: asserted by release_artifact_check's binstall tests.
    file == ".github/workflows/release.yml"
        // Publishable-crate manifests: binstall metadata, publish metadata, and
        // version-sync are all xtask-owned assertions over these files.
        || (file.starts_with("crates/") && file.ends_with("/Cargo.toml"))
}
'''
new_guard = '''fn is_xtask_policy_guarded_input(file: &str) -> bool {
    // Workflow contracts asserted by xtask integration tests.
    matches!(
        file,
        ".github/workflows/release.yml"
            | ".github/workflows/post-merge-status.yml"
            | ".github/workflows/badge-endpoints.yml"
            | ".github/workflows/ripr.yml"
    )
        // Publishable-crate manifests: binstall metadata, publish metadata, and
        // version-sync are all xtask-owned assertions over these files.
        || (file.starts_with("crates/") && file.ends_with("/Cargo.toml"))
}
'''
if text.count(old_guard) != 1:
    raise SystemExit("expected one xtask guarded-input function")
text = text.replace(old_guard, new_guard, 1)

release_test = '''    #[test]
    fn release_workflow_change_selects_xtask() -> Result<()> {
        let files = vec![".github/workflows/release.yml".to_string()];
        let metadata = fake_metadata(&[("xtask", "xtask")]);
        let crates = crates_from_files(&files, &metadata, "/workspace")?;
        assert!(
            crates.contains("xtask"),
            "changing the packaging step must route to the guard that asserts on it"
        );
        Ok(())
    }
'''
workflow_contract_test = release_test + '''
    #[test]
    fn workflow_contract_inputs_select_xtask() -> Result<()> {
        let metadata = fake_metadata(&[("xtask", "xtask")]);
        for workflow in [
            ".github/workflows/post-merge-status.yml",
            ".github/workflows/badge-endpoints.yml",
            ".github/workflows/ripr.yml",
        ] {
            let crates = crates_from_files(&[workflow.to_string()], &metadata, "/workspace")?;
            assert!(
                crates.contains("xtask"),
                "changing {workflow} must route to the xtask contract that reads it"
            );
        }
        Ok(())
    }
'''
if text.count(release_test) != 1:
    raise SystemExit("expected one release-workflow routing test")
text = text.replace(release_test, workflow_contract_test, 1)

old_comment = '''        // The rule is deliberately narrow: every extra path costs a crate in
        // the routed test scope. Only release.yml is a guarded packaging input.
'''
new_comment = '''        // The rule is deliberately narrow: every extra path costs a crate in
        // the routed test scope. Only workflows read by xtask contracts are guarded.
'''
if text.count(old_comment) != 1:
    raise SystemExit("expected one unrelated-workflow scope comment")
text = text.replace(old_comment, new_comment, 1)

path.write_text(text, encoding="utf-8")
