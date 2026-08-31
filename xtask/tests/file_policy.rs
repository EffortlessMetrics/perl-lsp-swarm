//! Integration tests for `cargo xtask non-rust inventory` (PR 3 of the
//! file-policy rollout, issue #8174).
//!
//! Unit tests for the classifier logic live inline in
//! `xtask/src/tasks/file_policy.rs` as `#[cfg(test)]` modules.

use assert_cmd::Command;
use color_eyre::eyre::{Result, ensure, eyre};
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
fn non_rust_inventory_inventory_help_describes_current_tree_check() -> Result<()> {
    let output = Command::cargo_bin("xtask")?.args(["non-rust", "inventory", "--help"]).output()?;
    assert!(output.status.success(), "non-rust inventory --help should exit 0");
    let help = String::from_utf8(output.stdout)?;
    ensure!(
        help.contains("Validate current-tree policy")
            && help.contains("target/policy/non-rust-inventory"),
        "inventory --help must describe current-tree validation and generated outputs"
    );
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

/// The inventory check must have an independently attributable, exact-tree CI
/// result instead of being hidden inside the aggregate policy shard.
#[test]
fn non_rust_inventory_check_is_wired_to_exact_tree_job() -> Result<()> {
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
    ensure!(
        gate.get("description").and_then(Value::as_str)
            == Some(
                "Validate current-tree non-Rust classification and emit reviewable inventory artifacts"
            ),
        "the required gate description must promise current-tree policy validation"
    );
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
        .and_then(|mapping| mapping.get("non-rust-policy"))
        .and_then(|job| job.get("gates"))
        .and_then(Value::as_sequence)
        .map(|gates| {
            gates
                .iter()
                .filter_map(Value::as_str)
                .filter(|name| *name == "non_rust_inventory_check")
                .count()
        })
        .unwrap_or_default();
    assert_eq!(mapped, 1, "gate must be mapped exactly once in workflow integration");

    let workflow_text = std::fs::read_to_string(root.join(".github/workflows/ci.yml"))?;
    let workflow: Value = serde_yaml_ng::from_str(&workflow_text)?;
    let job = workflow
        .get("jobs")
        .and_then(|jobs| jobs.get("non-rust-policy"))
        .ok_or_else(|| eyre!("dedicated non-rust-policy job is missing"))?;
    ensure!(
        job.get("name").and_then(Value::as_str) == Some("Non-Rust policy exact-tree"),
        "dedicated result must be attributable"
    );
    let env = job.get("env").ok_or_else(|| eyre!("job env is missing"))?;
    ensure!(
        env.get("SUBJECT_SHA").and_then(Value::as_str).is_some_and(|value| {
            value.contains("github.event.pull_request.head.sha")
                && value.contains("github.event.merge_group.head_sha")
                && value.contains("inputs.head_sha")
        }),
        "job must select the exact PR, merge-group, or dispatch subject"
    );
    ensure!(
        env.get("BASE_SHA").and_then(Value::as_str).is_some_and(|value| {
            value.contains("github.event.pull_request.base.sha")
                && value.contains("github.event.merge_group.base_sha")
                && value.contains("inputs.base_sha")
        }),
        "job must select the explicit event comparison base"
    );
    let steps = job
        .get("steps")
        .and_then(Value::as_sequence)
        .ok_or_else(|| eyre!("job steps are missing"))?;
    let step_named = |name: &str| {
        steps.iter().find(|step| step.get("name").and_then(Value::as_str) == Some(name))
    };
    let checkout = step_named("Checkout exact non-Rust policy subject")
        .ok_or_else(|| eyre!("exact-subject checkout is missing"))?;
    ensure!(
        checkout.get("with").and_then(|with| with.get("ref")).and_then(Value::as_str)
            == Some("${{ env.SUBJECT_SHA }}"),
        "checkout must use the selected exact subject"
    );
    let binding = step_named("Bind policy evidence to the checked-out tree")
        .and_then(|step| step.get("run"))
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("candidate-binding step is missing"))?;
    ensure!(
        binding.contains("test \"$actual_sha\" = \"$SUBJECT_SHA\"")
            && binding.contains("test \"$SUBJECT_SHA\" = \"$GITHUB_SHA\"")
            && binding.contains("git rev-parse --verify \"$BASE_SHA^{commit}\"")
            && binding.contains("SUBJECT_TREE_SHA="),
        "job must verify the selected commit, base, and tree"
    );
    let policy_step = step_named("Validate exact-tree non-Rust policy")
        .ok_or_else(|| eyre!("policy execution step is missing"))?;
    ensure!(
        policy_step.get("run").and_then(Value::as_str)
            == Some("cargo xtask gates --gate non_rust_inventory_check"),
        "dedicated job must execute the governed gate"
    );
    ensure!(
        policy_step.get("env").and_then(|env| env.get("CI_SCOPE_BASE")).and_then(Value::as_str)
            == Some("${{ env.BASE_SHA }}"),
        "policy execution must receive the explicit comparison base"
    );
    ensure!(
        !workflow_text.contains("adr_link_check non_rust_inventory_check lint_policy"),
        "aggregate policy shard must not duplicate the dedicated result"
    );
    Ok(())
}
