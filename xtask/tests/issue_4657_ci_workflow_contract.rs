//! Regression contracts for the CI hardening in issue #4657.

use std::fs;
use std::path::PathBuf;

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

#[test]
fn ci_workflows_keep_issue_4657_hardening() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let required_checks = fs::read_to_string(root.join(".ci/policies/required-checks.toml"))?;
    let routed_rust = fs::read_to_string(root.join(".github/workflows/em-ci-routed-rust.yml"))?;
    let title_check = fs::read_to_string(root.join(".github/workflows/pr-title-check.yml"))?;
    let version_bump = fs::read_to_string(root.join(".github/workflows/version-bump.yml"))?;

    assert!(
        !required_checks.contains("parser-ratchet.yml"),
        "required-check policy must not reference the absent parser-ratchet workflow"
    );
    assert!(
        routed_rust.contains(
            "if: github.event.pull_request.draft != true || github.event_name != 'pull_request'"
        ),
        "Rust Small routing must skip draft pull requests while allowing ready_for_review"
    );
    assert!(
        routed_rust.contains("if [ \"$ROUTE_RESULT\" = \"skipped\" ]; then")
            && routed_rust.contains("required check is neutral/pass"),
        "Rust Small result aggregation must treat an intentionally skipped draft route as neutral"
    );
    assert!(
        title_check
            .contains("uses: actions/github-script@3a2844b7e9c422d3c10d287c895573f7108da1b3 # v9")
            && !title_check.contains("actions/github-script@v9"),
        "pull_request_target title validation must use an immutable github-script v9 ref"
    );
    assert!(
        version_bump.contains("GIT_CLIFF_VERSION: \"2.13.1\"")
            && version_bump.contains(
                "GIT_CLIFF_LINUX_AMD64_SHA256: \"9a1263f24e59a2f508c7b3d3283c9dea94a8bf697f96dbc18cc783cac6284546\""
            )
            && version_bump.contains("sha256sum -c -")
            && !version_bump.contains("releases/latest"),
        "version bump must use a fixed, checksum-verified git-cliff release"
    );

    Ok(())
}
