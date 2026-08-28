//! Draft pull-request feedback contract for issue #10006.

use std::fs;
use std::path::PathBuf;

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

/// Extract one top-level job block from a workflow file.
fn job_block<'a>(workflow: &'a str, job: &str) -> Option<&'a str> {
    let header = format!("\n  {job}:\n");
    let start = workflow.find(&header)? + 1;
    let rest = &workflow[start..];
    let body_offset = rest.find('\n')? + 1;
    let end = rest[body_offset..]
        .match_indices('\n')
        .find(|(idx, _)| {
            let line = &rest[body_offset + idx + 1..];
            line.starts_with("  ") && !line.starts_with("   ") && !line.starts_with("  #")
        })
        .map_or(rest.len(), |(idx, _)| body_offset + idx + 1);
    Some(&rest[..end])
}

#[test]
fn drafts_keep_exact_head_cheap_feedback() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))?.replace("\r\n", "\n");

    assert!(
        ci.contains(
            "types: [opened, synchronize, reopened, ready_for_review, converted_to_draft]"
        ),
        "draft/ready state changes must retrigger CI for the current candidate"
    );

    let draft_guard =
        job_block(&ci, "draft-pr-check").ok_or("ci.yml has no `draft-pr-check` job")?;
    assert!(
        draft_guard.contains("echo \"run_ci=false\" >> \"$GITHUB_OUTPUT\"")
            && draft_guard.contains("Rust formatting")
            && draft_guard.contains("Conflict marker check")
            && draft_guard.contains("A skipped job is not verification"),
        "the draft guard must defer the expensive suite while identifying the real exact-head \
         checks and stating that skipped jobs are not proof. Extracted job:\n{draft_guard}"
    );

    let conflict_markers =
        job_block(&ci, "conflict-markers").ok_or("ci.yml has no `conflict-markers` job")?;
    assert!(
        conflict_markers.contains("always() &&")
            && conflict_markers.contains(
                "github.event_name == 'pull_request' && github.event.pull_request.draft == true"
            )
            && conflict_markers.contains("needs.draft-pr-check.outputs.run_ci == 'true'")
            && conflict_markers
                .contains("needs.preflight-latest-check.outputs.is_latest == 'true'")
            && conflict_markers.contains("github.event.pull_request.head.sha"),
        "the conflict-marker job must run against draft heads while preserving the normal \
         ready/push freshness route. Extracted job:\n{conflict_markers}"
    );

    let rust_formatting =
        job_block(&ci, "rust-formatting").ok_or("ci.yml has no `rust-formatting` job")?;
    assert!(
        rust_formatting.contains("Checkout exact formatter subject")
            && rust_formatting.contains("github.event.pull_request.head.sha")
            && !rust_formatting.contains("draft-pr-check")
            && !rust_formatting.contains("run_ci"),
        "candidate-bound rustfmt must remain independent of the draft/full-CI selector. \
         Extracted job:\n{rust_formatting}"
    );

    let compile_all_targets = job_block(&ci, "check-all-targets")
        .ok_or("ci.yml has no `check-all-targets` job")?;
    assert!(
        compile_all_targets.contains(
            "cargo test -p xtask --test draft_pr_feedback_contract --locked -- --nocapture"
        ),
        "the draft feedback contract must execute on the required Compile All Targets surface. \
         Extracted job:\n{compile_all_targets}"
    );

    Ok(())
}
