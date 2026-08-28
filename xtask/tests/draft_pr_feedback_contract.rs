//! Draft pull-request feedback contract for issue #10006.
//!
//! The compile-time assertions keep the contract load-bearing under
//! `cargo check --workspace --all-targets`; the runtime test additionally
//! verifies that the required strings compose inside the intended jobs.

use std::fs;
use std::path::PathBuf;

const CI_WORKFLOW: &[u8] = include_bytes!("../../.github/workflows/ci.yml");
const DRAFT_EVENT_TYPES: &[u8] =
    b"types: [opened, synchronize, reopened, ready_for_review, converted_to_draft]";
const DRAFT_SELECTOR: &[u8] = b"echo \"run_ci=false\" >> \"$GITHUB_OUTPUT\"";
const SKIPPED_IS_NOT_PROOF: &[u8] = b"A skipped job is not verification";
const DRAFT_CONFLICT_ROUTE: &[u8] =
    b"github.event_name == 'pull_request' && github.event.pull_request.draft == true";
const SURVIVE_SKIPPED_DEPENDENCY: &[u8] = b"always() &&";
const EXACT_PR_HEAD: &[u8] = b"github.event.pull_request.head.sha || github.ref_name";

const fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }

    let mut start = 0;
    while start + needle.len() <= haystack.len() {
        let mut offset = 0;
        while offset < needle.len() && haystack[start + offset] == needle[offset] {
            offset += 1;
        }
        if offset == needle.len() {
            return true;
        }
        start += 1;
    }
    false
}

const _: () = assert!(contains(CI_WORKFLOW, DRAFT_EVENT_TYPES));
const _: () = assert!(contains(CI_WORKFLOW, DRAFT_SELECTOR));
const _: () = assert!(contains(CI_WORKFLOW, SKIPPED_IS_NOT_PROOF));
const _: () = assert!(contains(CI_WORKFLOW, DRAFT_CONFLICT_ROUTE));
const _: () = assert!(contains(CI_WORKFLOW, SURVIVE_SKIPPED_DEPENDENCY));
const _: () = assert!(contains(CI_WORKFLOW, EXACT_PR_HEAD));

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

    Ok(())
}
