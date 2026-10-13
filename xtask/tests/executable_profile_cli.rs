use anyhow::Result;
use assert_cmd::cargo::cargo_bin_cmd;
use std::io::Write;

const VALID_MANIFEST: &str = r#"schema_version: executable_profile.v1
profile_id: narrow_eval_v1
profile_version: v1
purpose: Minimal executable-profile fixture for S2 CLI proof.
supported_perl_versions:
  - "5.38"
supported_constructs:
  - scalar_assignment
supported_builtins: []
supported_regex_subset: bounded-literals-only
module_policy: Static modules only.
filesystem_policy: No filesystem access.
io_policy: No I/O.
environment_policy: No environment reads.
network_policy: No network access.
xs_policy: No XS.
source_filter_policy: No source filters.
dynamic_boundary_behavior: Stop and report; never guess.
warnings_behavior: Collected and reported.
exceptions_behavior: Unwind to the boundary and report.
tap_behavior: Real totals only.
budgets:
  max_steps: 1000
  max_time_ms: 1000
  max_memory_bytes: 1048576
  max_recursion_depth: 16
  max_output_bytes: 65536
determinism_guarantees: Deterministic for same inputs and version.
oracle_agreement_thresholds: Exact agreement on the profiled subset.
unsupported_boundary_ids: []
owner_issue: '#4773'
change_reason: S2 CLI fixture.
claim_boundary: Fixture only; admits no evaluation.
"#;

fn write_fixture(
    dir: &tempfile::TempDir,
    name: &str,
    contents: &str,
) -> Result<std::path::PathBuf> {
    let path = dir.path().join(name);
    let mut file = std::fs::File::create(&path)?;
    file.write_all(contents.as_bytes())?;
    Ok(path)
}

#[test]
fn executable_profile_check_reports_stable_identity() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let manifest = write_fixture(&dir, "profile.yaml", VALID_MANIFEST)?;

    let output =
        cargo_bin_cmd!("xtask").arg("executable-profile").arg("check").arg(&manifest).output()?;
    assert!(output.status.success(), "check must accept the valid manifest");

    let stdout = String::from_utf8(output.stdout)?;
    let parts = stdout.split_whitespace().collect::<Vec<_>>();
    assert_eq!(parts.len(), 3, "check must print id, version, and hash: {stdout}");
    assert_eq!(parts[0], "narrow_eval_v1");
    assert_eq!(parts[1], "v1");
    assert_eq!(parts[2].len(), 64, "identity must be a sha256 hex digest: {stdout}");
    assert!(parts[2].chars().all(|char| char.is_ascii_hexdigit()));
    Ok(())
}

#[test]
fn executable_profile_report_summarizes_the_admitted_subset() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let manifest = write_fixture(&dir, "profile.yaml", VALID_MANIFEST)?;

    let output =
        cargo_bin_cmd!("xtask").arg("executable-profile").arg("report").arg(&manifest).output()?;
    assert!(output.status.success(), "report must accept the valid manifest");

    let stdout = String::from_utf8(output.stdout)?;
    for line in [
        "profile: narrow_eval_v1 v1",
        "perl_versions: 1",
        "constructs: 1",
        "builtins: 0",
        "regex_subset: bounded-literals-only",
        "boundaries: 0",
        "claim_boundary: Fixture only; admits no evaluation.",
    ] {
        assert!(stdout.contains(line), "report must contain {line:?}: {stdout}");
    }
    assert!(stdout.ends_with('\n'), "report must end with a newline");
    Ok(())
}

#[test]
fn executable_profile_check_rejects_premature_boundary_wiring() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let wired = VALID_MANIFEST.replace(
        "unsupported_boundary_ids: []",
        "unsupported_boundary_ids:\n  - outside_profile_eval",
    );
    let manifest = write_fixture(&dir, "wired.yaml", &wired)?;

    let output =
        cargo_bin_cmd!("xtask").arg("executable-profile").arg("check").arg(&manifest).output()?;
    assert!(!output.status.success(), "check must reject premature boundary wiring");
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("S4") && stderr.contains("#4770"),
        "rejection must name the S4 gate: {stderr}"
    );
    Ok(())
}

#[test]
fn executable_profile_check_rejects_whitespace_identity() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let spaced =
        VALID_MANIFEST.replace("profile_id: narrow_eval_v1", "profile_id: 'narrow eval_v1'");
    let manifest = write_fixture(&dir, "spaced.yaml", &spaced)?;

    let output =
        cargo_bin_cmd!("xtask").arg("executable-profile").arg("check").arg(&manifest).output()?;
    assert!(
        !output.status.success(),
        "check must reject a whitespace profile id that would break its three-field output"
    );
    Ok(())
}

#[test]
fn executable_profile_is_registered_in_list_commands() -> Result<()> {
    let output = cargo_bin_cmd!("xtask").arg("list-commands").output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(
        stdout.lines().any(|line| line == "executable-profile"),
        "list-commands must register the new root: {stdout}"
    );
    Ok(())
}
