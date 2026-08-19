//! Integration tests for `cargo xtask non-rust inventory` (PR 3 of the
//! file-policy rollout, issue #8174).
//!
//! Unit tests for the classifier logic live inline in
//! `xtask/src/tasks/file_policy.rs` as `#[cfg(test)]` modules.

use assert_cmd::Command;
use color_eyre::eyre::{Result, eyre};
use serde_yaml_ng::Value;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, MutexGuard};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn project_root() -> Result<PathBuf> {
    // CARGO_MANIFEST_DIR is xtask/ — go up one level.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().map(PathBuf::from).ok_or_else(|| eyre!("xtask must be in a subdirectory"))
}

static INVENTORY_OUTPUT_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn inventory_output_lock() -> Result<MutexGuard<'static, ()>> {
    INVENTORY_OUTPUT_LOCK.lock().map_err(|_| eyre!("inventory output lock poisoned"))
}

// ---------------------------------------------------------------------------
// CLI smoke tests
// ---------------------------------------------------------------------------

#[test]
fn non_rust_inventory_subcommand_help_exits_zero() -> Result<()> {
    let output = Command::cargo_bin("xtask")?.args(["non-rust", "--help"]).output()?;
    assert!(output.status.success(), "non-rust --help should exit 0");
    Ok(())
}

#[test]
fn non_rust_inventory_inventory_help_exits_zero() -> Result<()> {
    let output = Command::cargo_bin("xtask")?.args(["non-rust", "inventory", "--help"]).output()?;
    assert!(output.status.success(), "non-rust inventory --help should exit 0");
    Ok(())
}

/// End-to-end test: runs on the actual repo and exits 0.
#[test]
fn non_rust_inventory_command_exits_zero() -> Result<()> {
    let _guard = inventory_output_lock()?;
    Command::cargo_bin("xtask")?
        .args(["non-rust", "inventory"])
        .current_dir(project_root()?)
        .assert()
        .success();
    Ok(())
}

/// Read-only end-to-end check against the tracked-file inventory scan.
#[test]
fn non_rust_inventory_check_command_exits_zero() -> Result<()> {
    let _guard = inventory_output_lock()?;
    Command::cargo_bin("xtask")?
        .args(["non-rust", "inventory", "--check"])
        .current_dir(project_root()?)
        .assert()
        .success();
    Ok(())
}

/// Verify that the expected output files are created under `target/`.
///
/// `non-rust inventory` (without `--write`) must not modify any tracked file;
/// output goes to `target/policy/` only.
#[test]
fn non_rust_inventory_creates_output_files() -> Result<()> {
    let _guard = inventory_output_lock()?;
    Command::cargo_bin("xtask")?
        .args(["non-rust", "inventory"])
        .current_dir(project_root()?)
        .assert()
        .success();

    let root = project_root()?;
    assert!(
        root.join("target/policy/non-rust-inventory.md").exists(),
        "target/policy/non-rust-inventory.md should exist after the command"
    );
    assert!(
        root.join("target/policy/non-rust-inventory.json").exists(),
        "target/policy/non-rust-inventory.json should exist after the command"
    );
    // docs/policy/NON_RUST_INVENTORY.md must NOT be rewritten by the
    // non-`--write` path; the committed snapshot is updated only by
    // `cargo xtask non-rust inventory --write`.
    Ok(())
}

/// Verify that the JSON output is valid and contains expected fields.
#[test]
fn non_rust_inventory_json_is_valid() -> Result<()> {
    let _guard = inventory_output_lock()?;
    Command::cargo_bin("xtask")?
        .args(["non-rust", "inventory"])
        .current_dir(project_root()?)
        .assert()
        .success();

    let root = project_root()?;
    let json_path = root.join("target/policy/non-rust-inventory.json");
    let content = std::fs::read_to_string(&json_path)?;
    let value: serde_json::Value = serde_json::from_str(&content)?;

    assert!(value.is_array(), "inventory JSON must be an array");
    let arr = value.as_array().ok_or_else(|| eyre!("inventory JSON must be an array"))?;
    assert!(!arr.is_empty(), "inventory must contain at least one record");

    for (index, record) in arr.iter().enumerate() {
        assert!(record.get("path").is_some(), "record {index} must have `path`");
        assert!(record.get("extension").is_some(), "record {index} must have `extension`");
        assert!(record.get("category").is_some(), "record {index} must have `category`");
        assert!(record.get("allowlisted").is_some(), "record {index} must have `allowlisted`");
    }

    Ok(())
}

/// Verify that the markdown output starts with the expected header.
#[test]
fn non_rust_inventory_markdown_has_header() -> Result<()> {
    let _guard = inventory_output_lock()?;
    Command::cargo_bin("xtask")?
        .args(["non-rust", "inventory"])
        .current_dir(project_root()?)
        .assert()
        .success();

    let root = project_root()?;
    let md_path = root.join("target/policy/non-rust-inventory.md");
    let content = std::fs::read_to_string(&md_path)?;

    assert!(
        content.starts_with("# Non-Rust File Inventory"),
        "markdown must start with the expected heading"
    );
    assert!(
        content.contains("Generated by `cargo xtask non-rust inventory`"),
        "markdown must include the generator notice"
    );
    Ok(())
}

/// Assert that `docs/policy/NON_RUST_INVENTORY.md` matches what the current
/// tree generates.
///
/// When this test fails, the committed snapshot is stale.  Refresh it with:
///
/// ```text
/// cargo xtask non-rust inventory --write
/// ```
///
/// then commit the updated file.  The test deliberately fails with a diff so
/// that the stale content is visible without opening a separate file — compare
/// the "left" (committed) with the "right" (generated) in the panic output.
#[test]
fn non_rust_inventory_docs_are_current() -> Result<()> {
    let _guard = inventory_output_lock()?;
    let root = project_root()?;

    // Generate fresh output to target/ — no tracked file is touched.
    Command::cargo_bin("xtask")?
        .args(["non-rust", "inventory"])
        .current_dir(&root)
        .assert()
        .success();

    let generated_path = root.join("target/policy/non-rust-inventory.md");
    let committed_path = root.join("docs/policy/NON_RUST_INVENTORY.md");

    let generated = std::fs::read_to_string(&generated_path).map_err(|e| {
        eyre!("could not read generated inventory at {}: {e}", generated_path.display())
    })?;
    let committed = std::fs::read_to_string(&committed_path).map_err(|e| {
        eyre!("could not read committed inventory at {}: {e}", committed_path.display())
    })?;

    // Normalise line endings so CRLF/LF differences do not cause spurious failures.
    let normalize = |s: &str| s.replace("\r\n", "\n");

    assert_eq!(
        normalize(&committed),
        normalize(&generated),
        "\n\ndocs/policy/NON_RUST_INVENTORY.md is stale.\n\
         Run `cargo xtask non-rust inventory --write` and commit the result.\n"
    );

    Ok(())
}

/// The generated inventory check is only useful when the existing policy
/// shard actually invokes it. Keep the source policy and workflow matrix
/// wired to the same direct, read-only command.
#[test]
fn non_rust_inventory_check_is_wired_to_policy_shard() -> Result<()> {
    let root = project_root()?;
    let policy: Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(root.join(".ci/gate-policy.yaml"))?)?;
    let gate = policy
        .get("gates")
        .and_then(Value::as_sequence)
        .and_then(|gates| {
            gates.iter().find(|gate| {
                gate.get("name").and_then(Value::as_str) == Some("non_rust_inventory_check")
            })
        })
        .ok_or_else(|| eyre!("non_rust_inventory_check is missing from gate policy"))?;

    assert_eq!(gate.get("tier").and_then(Value::as_str), Some("merge_gate"));
    assert_eq!(gate.get("required").and_then(Value::as_bool), Some(true));
    assert_eq!(
        gate.get("command").and_then(Value::as_str),
        Some("cargo xtask non-rust inventory --check")
    );
    assert_eq!(gate.get("timeout_seconds").and_then(Value::as_u64), Some(300));
    assert_eq!(
        gate.get("budgets")
            .and_then(|budgets| budgets.get("max_duration_ms"))
            .and_then(Value::as_u64),
        Some(240_000)
    );

    let mapped = policy
        .get("workflow_integration")
        .and_then(|integration| integration.get("job_mapping"))
        .and_then(|mapping| mapping.get("ci-gate"))
        .and_then(|ci_gate| ci_gate.get("gates"))
        .and_then(Value::as_sequence)
        .map(|gates| {
            gates
                .iter()
                .filter_map(Value::as_str)
                .filter(|name| *name == "non_rust_inventory_check")
                .count()
        })
        .unwrap_or_default();
    assert_eq!(mapped, 1, "gate must be mapped exactly once in the policy shard");

    let workflow = std::fs::read_to_string(root.join(".github/workflows/ci.yml"))?;
    assert!(
        workflow.contains("docs_build adr_link_check non_rust_inventory_check v2_bundle_sync"),
        "the live policy matrix must execute the inventory scan gate"
    );
    Ok(())
}
