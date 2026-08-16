//! Regression contracts for the CI hardening in issue #4657.

use std::fs;
use std::path::PathBuf;

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

/// Extract one top-level job block from a workflow file.
///
/// Jobs are keyed at two-space indentation under `jobs:`; the block runs until
/// the next key at that same indentation. Returns `None` when the job is absent
/// so callers can fail loudly rather than assert against an empty string.
fn job_block<'a>(workflow: &'a str, job: &str) -> Option<&'a str> {
    let header = format!("\n  {job}:\n");
    let start = workflow.find(&header)? + 1;
    let rest = &workflow[start..];
    let body_offset = rest.find('\n')? + 1;
    let end = rest[body_offset..]
        .match_indices('\n')
        .find(|(idx, _)| {
            let line = &rest[body_offset + idx + 1..];
            // Next top-level job: exactly two spaces of indent, then content.
            line.starts_with("  ") && !line.starts_with("   ") && !line.starts_with("  #")
        })
        .map_or(rest.len(), |(idx, _)| body_offset + idx + 1);
    Some(&rest[..end])
}

/// #9594: the bit-rot guard must not pin the pull-request head SHA.
///
/// For a `pull_request` event the workflow definition comes from the base
/// branch. Pinning `head.sha` runs base's step list against the candidate's
/// tree, so a commit that adds a required step together with the file it needs
/// makes this required check fail on every older branch — on branch age rather
/// than on content. Checking out the event's own ref keeps the definition and
/// the tree one integration subject.
#[test]
fn compile_all_targets_checks_out_the_integration_subject(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))?;

    let job = job_block(&ci, "check-all-targets")
        .ok_or("ci.yml no longer defines a `check-all-targets` job")?;

    // Guard the extractor itself: a silently-empty block would make the
    // assertion below pass for the wrong reason.
    assert!(
        job.contains("name: Compile All Targets (bit-rot guard)")
            && job.contains("actions/checkout@"),
        "check-all-targets block was not extracted correctly; got:\n{job}"
    );

    assert!(
        !job.contains("pull_request.head.sha"),
        "the required `Compile All Targets (bit-rot guard)` job must not pin the PR head SHA \
         (#9594): the workflow definition comes from the base branch, so a head-pinned tree \
         fails on branch age rather than on content. Extracted job:\n{job}"
    );

    Ok(())
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
