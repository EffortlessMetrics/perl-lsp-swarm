//! Ownership invariant for the quality-gate facade (`quality_gate_facade.rs`).
//!
//! Before #15290, the facade re-implemented the engine's output pipeline: it
//! built a temporary workspace, wrote a rewritten policy, called the engine in
//! `check=false, quiet=true` mode, then re-read the engine's JSON and Markdown
//! outputs to remap temp paths, restore lifecycle dates, restore the policy
//! validation error, and re-render the Markdown against the patched receipt.
//! Rendering, freshness comparison, output writes, and exit classification all
//! lived in two places.
//!
//! This contract asserts that the consolidation stayed consolidated: the
//! facade must not import a temp-workspace primitive, must not name a path
//! remapper, must not name a lifecycle restore helper, and must not re-own
//! any of the engine's publish-step primitives. If a future contributor
//! tries to grow the facade back into the engine, this test fires.

use std::{fs, path::PathBuf};

const FORBIDDEN_TOKENS: &[&str] = &[
    // Was the temp-workspace allocator for the OLD facade's evaluation
    // workspace. The consolidated engine writes only to the caller's
    // caller-supplied paths; it must never spin up a sibling directory.
    "TempWorkspace",
    // Was the engine's rewrite-to-recovery loop in the OLD facade. Path
    // remapping now happens at the call-site boundaries (the engine reads
    // the caller policy file directly), so the facade has no text to
    // replace.
    "replace_json_strings",
    // Was the OLD facade's reverse pass for the lifecycle overlay. The
    // engine stamps original committed dates in the receipt natively via
    // [`LifecycleOverlay`], so the facade no longer mutates receipt text.
    "restore_lifecycle_dates",
    // Was the OLD facade's reverse pass for `due_review` validation. The
    // engine injects the validation error and the corresponding next action
    // during evaluation, so the facade no longer rewrites receipt text.
    "restore_policy_validation_error",
    // Was the OLD facade's per-iteration verifier for receipt/summary
    // freshness. The engine's `publish` collects both findings and
    // classifies the exit; the facade has no path comparison to make.
    "assert_current",
    // Was the OLD facade's output writer. The engine's `publish` writes
    // both artifacts under the caller-supplied paths.
    "write_text",
    // Was the OLD facade's wrapper that produced the "remapped" report
    // path string. The engine now prints caller paths directly via
    // `display_path`.
    "remapped_report",
    // Was the OLD facade's path normalization helper. The engine already
    // calls `display_path` inside its publish step.
    "display_path",
    // Was the OLD facade's text-rebuild step that swapped sentinel dates
    // back to committed dates inside the receipt copy. The engine now
    // stamps original dates through `LifecycleOverlay::rows`.
    "normalize_output",
    // Was the OLD facade's literal sentinel. The engine owns the sentinel
    // constant now; duplicating it in the facade would silently drift the
    // adapter off the engine's verdict contract.
    "LIFECYCLE_SENTINEL",
    // Was the OLD facade's normalized-policy record. The engine now
    // produces the equivalent shape inline inside `read_exception_policy`.
    "NormalizedPolicy",
];

fn facade_source() -> String {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = crate_dir.join("src/tasks/quality_gate_facade.rs");
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "could not read quality_gate_facade.rs at {}: {error}",
            path.display()
        )
    })
}

#[test]
fn facade_does_not_re_own_engine_publish_steps() {
    let source = facade_source();

    for token in FORBIDDEN_TOKENS {
        assert!(
            !source.contains(token),
            "quality_gate_facade.rs must not contain `{token}`: the engine's \
             `publish` step is the single owner of the output pipeline. \
             Re-introducing a facade-side {token} would re-split the \
             ownership the #15290 refactor consolidated."
        );
    }
}

#[test]
fn facade_does_not_use_tempfile_for_evaluation_workspace() {
    let source = facade_source();
    assert!(
        !source.contains("tempfile"),
        "quality_gate_facade.rs must not depend on `tempfile`: the engine \
         evaluates against the caller-supplied policy directly. A temp \
         workspace would re-introduce the path-remapping layer #15290 \
         collapsed."
    );
}

#[test]
fn facade_does_not_re_export_publish_helpers() {
    let source = facade_source();
    for helper in [
        "pub fn render_json",
        "pub fn render_markdown",
        "pub fn assert_current",
        "pub fn write_text",
        "pub fn publish",
    ] {
        assert!(
            !source.contains(helper),
            "quality_gate_facade.rs must not re-expose `{helper}`: the \
             facade is an adapter that hands its typed overlay to \
             `implementation::run_with_overlay`; re-publishing helpers \
             would create a second owner of the output pipeline."
        );
    }
}
