//! Fail-closed CPAN SweepReport summary reader pins (#15315).

use std::fs;

use anyhow::{Result, ensure};

const WORKFLOW: &str = "post-merge-corpus-ratchet.yml";
const READER: &str = ".ci/scripts/read-cpan-corpus-sweep-summary.py";

fn project_root() -> std::path::PathBuf {
    let mut root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn workflow_raw() -> Result<String> {
    Ok(fs::read_to_string(project_root().join(".github/workflows").join(WORKFLOW))?)
}

#[test]
fn cpan_corpus_summary_consumers_use_fail_closed_reader() -> Result<()> {
    let raw = workflow_raw()?;
    ensure!(raw.contains(READER), "workflow must delegate summary rendering to {READER}");
    ensure!(
        !raw.contains("report.get('total_files', 0)"),
        "inline default-counter summary must not remain in {WORKFLOW}"
    );
    Ok(())
}

#[test]
fn cpan_corpus_summary_reader_tests_pass() -> Result<()> {
    let test_script = project_root().join(".ci/scripts/test_read_cpan_corpus_sweep_summary.py");
    let output = std::process::Command::new("python3").arg(&test_script).output()?;
    ensure!(
        output.status.success(),
        "python summary reader tests failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
