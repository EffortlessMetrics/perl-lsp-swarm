//! CLI round trips for `compiler-profile unsupported` (#4770 slice 2,
//! SPEC-0038 X1-X2).

use anyhow::{Context, Result};
use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;
use std::path::PathBuf;

fn workspace_root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|path| path.to_path_buf())
        .context("xtask manifest dir should have a parent")
}

fn seed_catalog_arg() -> Result<String> {
    let path = workspace_root()?.join("policy/compiler/unsupported-facts.v1.toml");
    path.to_str().map(str::to_string).context("seed catalog path should be UTF-8")
}

fn duplicate_fixture_arg() -> Result<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/unsupported-facts/duplicate-id.toml");
    path.to_str().map(str::to_string).context("fixture path should be UTF-8")
}

#[test]
fn unsupported_check_accepts_seed_catalog() -> Result<()> {
    let catalog = seed_catalog_arg()?;
    cargo_bin_cmd!("xtask")
        .args(["compiler-profile", "unsupported", "--check", catalog.as_str()])
        .assert()
        .success()
        .stdout("unsupported_fact_catalog.v1: 14 classes, 0 violations\n");
    Ok(())
}

#[test]
fn unsupported_check_defaults_to_landed_catalog() -> Result<()> {
    cargo_bin_cmd!("xtask")
        .args(["compiler-profile", "unsupported", "--check"])
        .assert()
        .success()
        .stdout("unsupported_fact_catalog.v1: 14 classes, 0 violations\n");
    Ok(())
}

#[test]
fn unsupported_check_rejects_dangling_nearest_neighbor() -> Result<()> {
    let seed =
        fs::read_to_string(workspace_root()?.join("policy/compiler/unsupported-facts.v1.toml"))
            .context("read seed catalog for dangling-neighbor CLI fixture")?;
    let dangling = seed.replacen(
        "nearest_neighbor = \"generated_no_source\"",
        "nearest_neighbor = \"not_a_catalog_class\"",
        1,
    );
    assert_ne!(dangling, seed, "seed catalog must contain autoload's generated_no_source neighbor");
    let dir = tempfile::tempdir().context("create dangling-neighbor fixture dir")?;
    let path = dir.path().join("dangling-neighbor.toml");
    fs::write(&path, dangling).context("write dangling-neighbor fixture")?;
    let fixture = path.to_str().context("dangling-neighbor fixture path should be UTF-8")?;
    let output = cargo_bin_cmd!("xtask")
        .args(["compiler-profile", "unsupported", "--check", fixture])
        .output()?;
    assert!(!output.status.success(), "dangling nearest_neighbor must fail --check");
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("nearest_neighbor")
            && stderr.contains("not_a_catalog_class")
            && stderr.contains("autoload"),
        "rejection must name the dangling neighbor: {stderr}"
    );
    Ok(())
}

#[test]
fn unsupported_check_rejects_duplicate_id_fixture() -> Result<()> {
    let fixture = duplicate_fixture_arg()?;
    let output = cargo_bin_cmd!("xtask")
        .args(["compiler-profile", "unsupported", "--check", fixture.as_str()])
        .output()?;
    assert!(!output.status.success(), "duplicate IDs must fail --check");
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("duplicate class_id 'ambient_module_resolution'"),
        "rejection must name the duplicate ID: {stderr}"
    );
    Ok(())
}

#[test]
fn unsupported_report_is_deterministic() -> Result<()> {
    let catalog = seed_catalog_arg()?;
    let first = cargo_bin_cmd!("xtask")
        .args(["compiler-profile", "unsupported", "--report", catalog.as_str()])
        .output()?;
    assert!(first.status.success(), "report on the seed catalog must succeed");
    let second = cargo_bin_cmd!("xtask")
        .args(["compiler-profile", "unsupported", "--report", catalog.as_str()])
        .output()?;
    assert!(second.status.success(), "report on the seed catalog must succeed");
    assert_eq!(first.stdout, second.stdout, "report bytes must be deterministic");
    let stdout = String::from_utf8(first.stdout)?;
    let mut lines = stdout.lines();
    assert_eq!(
        lines.next(),
        Some(
            "unsupported_fact_catalog.v1 classes=14 compile_blocks=3 downstream_blocks=14 layers=hir:4,parser:3,semantic:7"
        )
    );
    let ids: Vec<&str> =
        lines.map(|line| line.split_whitespace().next().unwrap_or_default()).collect();
    assert_eq!(ids.len(), 14, "report must carry one line per class");
    assert_eq!(ids.first(), Some(&"ambient_module_resolution"));
    assert_eq!(ids.last(), Some(&"xs_native_dependency"));
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "report IDs must be sorted");
    assert!(!stdout.contains(".toml"), "report must not leak catalog paths: {stdout}");
    Ok(())
}

#[test]
fn unsupported_requires_check_or_report() -> Result<()> {
    let output = cargo_bin_cmd!("xtask").args(["compiler-profile", "unsupported"]).output()?;
    assert!(!output.status.success(), "bare unsupported must fail closed");
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("requires --check and/or --report"),
        "bare unsupported must print usage: {stderr}"
    );
    Ok(())
}
