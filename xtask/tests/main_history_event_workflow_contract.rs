//! Checked contract for the hosted `main` history-event detector workflow.
//!
//! #10306 requires the hosted surface to stay read-only, to delegate every
//! ancestry judgement to one repository command, and to treat a missing receipt
//! as a failure. Those properties are invisible to the Rust type system and are
//! exactly what a well-meaning later edit erodes, so they are asserted directly
//! against the workflow source.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

fn workflow_path() -> Result<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .with_context(|| format!("{} has no repository-root parent", manifest.display()))?;
    Ok(root.join(".github/workflows/main-history-event.yml"))
}

fn workflow_source() -> Result<String> {
    let path = workflow_path()?;
    fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
}

/// The detector must never gain mutation authority: the whole point of keeping
/// it out of `post-merge-status.yml` was to deny it a writer's trust boundary.
#[test]
fn workflow_has_no_write_authority() -> Result<()> {
    let source = workflow_source()?;

    assert!(
        source.contains("permissions:\n  contents: read"),
        "workflow-level permissions must be read-only"
    );
    assert!(!source.contains("write-all"), "the detector must never request write-all");
    assert!(!source.contains("contents: write"), "the detector must not acquire contents: write");
    assert!(
        !source.contains("pull-requests: write"),
        "the detector must not acquire pull-requests: write"
    );
    assert!(!source.contains("issues: write"), "the detector must not acquire issues: write");
    assert!(
        source.contains("persist-credentials: false"),
        "the checkout must not persist credentials"
    );
    Ok(())
}

/// Negative control: the workflow must not reacquire a private ancestry
/// interpretation. Reading `git fetch` prose or running a second merge-base
/// classifier in YAML is the exact failure that produced the false August 15
/// re-root diagnosis.
#[test]
fn workflow_delegates_every_ancestry_judgement_to_the_repository_command() -> Result<()> {
    let source = workflow_source()?;

    assert!(
        source.contains("--bin main-history-event"),
        "the workflow must invoke the repository command"
    );
    assert!(
        !source.contains("merge-base"),
        "ancestry interpretation belongs to xtask::git_ancestry, not to workflow YAML"
    );
    assert!(!source.contains("rev-list"), "the workflow must not compute its own history relation");
    assert!(
        !source.contains("git fetch"),
        "the workflow must not parse fetch output to decide history movement"
    );
    assert!(
        !source.contains("is-shallow-repository"),
        "shallow interpretation belongs to the shared ancestry authority"
    );
    Ok(())
}

/// A missing receipt is a failed observation. `warn` would let the detector go
/// green while publishing nothing.
#[test]
fn missing_receipt_upload_is_an_error_not_a_warning() -> Result<()> {
    let source = workflow_source()?;

    assert!(
        source.contains("if-no-files-found: error"),
        "a missing history-event receipt must fail the run"
    );
    assert!(
        !source.contains("if-no-files-found: warn"),
        "the detector must not downgrade a missing receipt to a warning"
    );
    assert!(
        !source.contains("if-no-files-found: ignore"),
        "the detector must not ignore a missing receipt"
    );
    Ok(())
}

/// The receipt has to survive the blocking verdict it explains, so the upload
/// must not be skipped when the classifier reports a rewrite.
#[test]
fn receipt_is_published_even_when_the_verdict_blocks() -> Result<()> {
    let source = workflow_source()?;
    let (_, upload) = source
        .split_once("Publish the history-event receipt")
        .context("the workflow no longer contains the receipt-upload step")?;

    assert!(
        upload.contains("if: always()"),
        "the receipt upload must run even after a blocking verdict"
    );
    assert!(
        source.contains("Enforce the detector verdict"),
        "a blocking verdict must still fail the run after the receipt is published"
    );
    Ok(())
}

/// Each push carries an irreplaceable before/after subject, so no run may cancel
/// or displace another. `cancel-in-progress: false` is necessary but not
/// sufficient: a concurrency group holds one running plus one pending run, and a
/// newly queued run displaces the pending one, so a group shared across pushes
/// would silently drop an intermediate push's receipt during a merge burst.
/// Keying the group by the exact event commit makes that structurally impossible.
#[test]
fn no_push_receipt_can_be_displaced_by_a_later_push() -> Result<()> {
    let source = workflow_source()?;

    assert!(
        source.contains("cancel-in-progress: false"),
        "history observations must not be cancelled"
    );
    assert!(
        source.contains("group: main-history-event-${{ github.sha }}"),
        "the concurrency group must be unique per event commit so no push can displace another"
    );
    assert!(
        !source.contains("group: main-history-event-${{ github.ref }}"),
        "a ref-keyed group is shared by every push to main and can discard a pending receipt"
    );
    Ok(())
}

/// A `workflow_dispatch` run carries no push payload, so every manual run would
/// classify as `invalid_event` and fail. The detector is push-bound by nature.
#[test]
fn workflow_is_push_triggered_only() -> Result<()> {
    let source = workflow_source()?;

    assert!(
        !source.contains("workflow_dispatch:"),
        "a dispatch run has no push payload and would always fail as invalid_event"
    );
    assert!(
        source.contains("on:\n  push:\n    branches:\n      - main\n"),
        "the detector must observe pushes to main"
    );
    Ok(())
}

/// A shallow checkout cannot decide movement, so the detector must fetch the
/// complete graph and pin the immutable event commit rather than a moving tip.
#[test]
fn workflow_checks_out_the_complete_graph_at_the_exact_event_commit() -> Result<()> {
    let source = workflow_source()?;

    assert!(source.contains("fetch-depth: 0"), "the detector needs the complete graph");
    assert!(
        source.contains("ref: ${{ github.sha }}"),
        "the detector must pin the immutable event commit"
    );
    Ok(())
}

/// The exact platform subjects must reach the command, and at step scope rather
/// than interpolated into the script body.
#[test]
fn exact_event_subjects_are_forwarded_at_step_scope() -> Result<()> {
    let source = workflow_source()?;

    for subject in [
        "EVENT_BEFORE: ${{ github.event.before }}",
        "EVENT_AFTER: ${{ github.sha }}",
        "EVENT_REF: ${{ github.ref }}",
        "EVENT_FORCED: ${{ github.event.forced }}",
        "EVENT_CREATED: ${{ github.event.created }}",
        "EVENT_DELETED: ${{ github.event.deleted }}",
    ] {
        assert!(source.contains(subject), "the workflow must forward {subject}");
    }
    Ok(())
}
