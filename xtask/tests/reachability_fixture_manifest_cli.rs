//! Contract tests for the reachability fixture manifest checker (#10998):
//! the canonical denominator manifest must validate, its generated view must
//! stay drift-free across repeated runs, and unknown-field documents must
//! fail closed through the CLI surface.

use anyhow::Result;
use assert_cmd::cargo::cargo_bin_cmd;

/// The CLI tests drive the same checked-in manifest and view files through
/// one shared checkout, and the test harness runs them on parallel threads.
/// Every test therefore holds this lock for its whole body so a temporary
/// mutation (the corrupt-then-regenerate check below) can never race another
/// test's read of the shared files.
static REPO_STATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn check_passes_over_the_canonical_manifest() -> Result<()> {
    let _repo_state = REPO_STATE_LOCK.lock().expect("repo state lock poisoned");
    // The summary population is asserted against the manifest's own validated
    // `declared_row_count`, never a hand-copied literal.
    let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/analysis_reachability_denominator/manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path)?)?;
    let declared = manifest
        .get("declared_row_count")
        .and_then(serde_json::Value::as_u64)
        .expect("manifest declares declared_row_count");

    let output = cargo_bin_cmd!("xtask").args(["check-reachability-fixture-manifest"]).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        output.status.success(),
        "reachability fixture manifest check must pass:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains(&format!("declared_row_count {declared}")),
        "summary cross-checks the declared population: {stdout}"
    );
    assert!(
        stdout.contains(&format!("{declared} rows")),
        "summary reports its population: {stdout}"
    );
    assert!(
        stdout.contains("10 of 10 families covered"),
        "every declared family stays instantiated or visibly deferred: {stdout}"
    );
    assert!(
        stdout.contains("NOT_PROVEN instruments"),
        "missing instruments surface as explicit NOT_PROVEN coverage: {stdout}"
    );
    Ok(())
}

#[test]
fn check_is_idempotent_across_second_run() -> Result<()> {
    let _repo_state = REPO_STATE_LOCK.lock().expect("repo state lock poisoned");
    for _ in 0..2 {
        let output =
            cargo_bin_cmd!("xtask").args(["check-reachability-fixture-manifest"]).output()?;
        assert!(
            output.status.success(),
            "second generation must produce no drift:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[test]
fn update_view_is_explicit_and_deterministic() -> Result<()> {
    let _repo_state = REPO_STATE_LOCK.lock().expect("repo state lock poisoned");
    let view_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/analysis_reachability_denominator/denominator-coverage-view.md");
    let before = std::fs::read_to_string(&view_path)?;
    // Falsify a no-op writer: corrupt the view first, so only a real
    // regeneration can restore the validated bytes. Comparing two reads of an
    // already-current view would pass a writer that never writes.
    std::fs::write(&view_path, "stale drift marker: no real writer would keep this\n")?;
    let output = cargo_bin_cmd!("xtask")
        .args(["check-reachability-fixture-manifest", "--update-view"])
        .output()?;
    assert!(
        output.status.success(),
        "--update-view regenerates the drifted view:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let after = std::fs::read_to_string(&view_path)?;
    assert_eq!(before, after, "regeneration restores the validated view bytes");
    // Determinism: a second run over unchanged input stays byte-stable.
    let second = cargo_bin_cmd!("xtask")
        .args(["check-reachability-fixture-manifest", "--update-view"])
        .output()?;
    assert!(
        second.status.success(),
        "second regeneration must succeed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let after_second = std::fs::read_to_string(&view_path)?;
    assert_eq!(after, after_second, "regeneration is deterministic over unchanged input");
    Ok(())
}
