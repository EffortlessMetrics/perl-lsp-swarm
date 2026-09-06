//! Integration tests for `cargo xtask non-rust inventory` (PR 3 of the
//! file-policy rollout, issue #8174).
//!
//! Unit tests for the classifier logic live inline in
//! `xtask/src/tasks/file_policy.rs` as `#[cfg(test)]` modules.

use assert_cmd::Command;
use color_eyre::eyre::{Result, ensure, eyre};
use serde_yaml_ng::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
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

fn lock_or_recover<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn inventory_output_lock() -> MutexGuard<'static, ()> {
    lock_or_recover(&INVENTORY_OUTPUT_LOCK)
}

#[cfg(test)]
mod lock_tests {
    use super::lock_or_recover;
    use color_eyre::eyre::{Result, ensure};
    use std::sync::{Mutex, TryLockError};

    #[test]
    fn healthy_inventory_lock_retains_exclusion() -> Result<()> {
        let lock = Mutex::new(());
        let guard = lock_or_recover(&lock);
        ensure!(
            matches!(lock.try_lock(), Err(TryLockError::WouldBlock)),
            "the returned guard must hold the supplied mutex"
        );
        ensure!(!lock.is_poisoned(), "normal acquisition must not poison the mutex");
        drop(guard);
        ensure!(lock.try_lock().is_ok(), "dropping the guard must release the mutex");
        Ok(())
    }

    #[test]
    fn poisoned_inventory_lock_is_recovered() -> Result<()> {
        let lock = Mutex::new(());
        let join_result = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _guard = match lock.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    panic!("fixture failure must poison the lock for this regression");
                })
                .join()
        });

        ensure!(join_result.is_err(), "the fixture must terminate by unwinding");
        ensure!(lock.is_poisoned(), "the fixture must actually poison the mutex");
        let guard = lock_or_recover(&lock);
        ensure!(lock.is_poisoned(), "recovery preserves the poison marker for later diagnostics");
        ensure!(
            matches!(lock.try_lock(), Err(TryLockError::WouldBlock)),
            "recovery must still exclude another inventory writer"
        );
        drop(guard);
        ensure!(
            matches!(lock.try_lock(), Err(TryLockError::Poisoned(_))),
            "dropping the recovered guard must release the mutex without clearing poison"
        );

        let _guard = lock_or_recover(&lock);
        ensure!(
            matches!(lock.try_lock(), Err(TryLockError::WouldBlock)),
            "a later inventory writer must reacquire the same poisoned mutex"
        );
        ensure!(lock.is_poisoned(), "repeated recovery must not erase the original failure");
        Ok(())
    }
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
fn non_rust_inventory_inventory_help_describes_fail_closed_snapshot_check() -> Result<()> {
    let output = Command::cargo_bin("xtask")?.args(["non-rust", "inventory", "--help"]).output()?;
    assert!(output.status.success(), "non-rust inventory --help should exit 0");
    let help = String::from_utf8(output.stdout)?;
    ensure!(
        help.contains("Require the generated Markdown snapshot")
            && help.contains("after line-ending normalization"),
        "inventory --help must describe the fail-closed normalized snapshot check"
    );
    Ok(())
}

/// End-to-end test: runs on the actual repo and exits 0.
#[test]
fn non_rust_inventory_command_exits_zero() -> Result<()> {
    let _guard = inventory_output_lock();
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
    let _guard = inventory_output_lock();
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
    let _guard = inventory_output_lock();
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
    let _guard = inventory_output_lock();
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
    let _guard = inventory_output_lock();
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
    let _guard = inventory_output_lock();
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
    ensure!(
        gate.get("description").and_then(Value::as_str)
            == Some(
                "Scan and classify tracked non-Rust files and require the normalized committed snapshot to match"
            ),
        "the required gate description must promise exact normalized snapshot parity"
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

    let job = merge_gate_shards_job(&root)?;
    let shard_gates = policy_shard_gates(&job)?;

    // Negative control for the parse, asserted before membership. A membership
    // test over an empty or mis-navigated list is silently false rather than
    // loud, so first prove the resolved row is the real one: every name it
    // declares must be a gate `.ci/gate-policy.yaml` actually defines. A parse
    // that landed on the wrong matrix row or a default empty sequence cannot
    // satisfy this, and a gate name that exists in neither file is drift the
    // shard should not be allowed to carry.
    let defined = defined_gate_names(&policy)?;
    let undefined: Vec<&str> =
        shard_gates.iter().map(String::as_str).filter(|&name| !defined.contains(name)).collect();
    ensure!(
        undefined.is_empty(),
        "the `policy` shard names gates that .ci/gate-policy.yaml does not define: {undefined:?}"
    );

    ensure!(
        shard_gates.iter().filter(|name| *name == "non_rust_inventory_check").count() == 1,
        "the live policy matrix must execute the inventory scan gate exactly once; \
         the `policy` shard declares {shard_gates:?}"
    );

    // Being listed in the matrix is not the same as being run. The step that
    // consumes `matrix.gates` must stay unconditional: a step-level `if:` that
    // excludes the policy shard drops every gate in it while GitHub still
    // reports the check green, because a skipped step reports success. That is
    // the same "contract that cannot go red" failure this suite exists to catch
    // (#14585), so assert the execution seam and not only the declaration.
    let runner = shard_runner_step(&job)?;
    ensure!(
        runner.get("if").is_none(),
        "the `merge-gate-shards` step consuming `matrix.gates` must run for every \
         shard; an `if:` on it can drop the policy shard's gates entirely while \
         the check still reports success"
    );

    Ok(())
}

/// The `merge-gate-shards` job, parsed once for both the matrix row and the
/// step that consumes it.
fn merge_gate_shards_job(root: &Path) -> Result<Value> {
    let workflow: Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(root.join(".github/workflows/ci.yml"))?)?;
    workflow
        .get("jobs")
        .and_then(|jobs| jobs.get("merge-gate-shards"))
        .cloned()
        .ok_or_else(|| eyre!("ci.yml no longer defines a `merge-gate-shards` job"))
}

/// Gate names the `policy` merge-gate shard actually declares, in matrix order.
///
/// This resolves the workflow structurally — job, strategy, matrix row — rather
/// than matching a literal run of adjacent gate names. The literal form went
/// stale the moment `must_context_check` was inserted mid-list (#14585), which
/// turned a wiring contract into an ordering assertion. Membership survives
/// insertion, removal, and reordering of neighbouring gates; only actually
/// dropping the gate from the shard fails it.
fn policy_shard_gates(job: &Value) -> Result<Vec<String>> {
    let rows: Vec<&Value> = job
        .get("strategy")
        .and_then(|strategy| strategy.get("matrix"))
        .and_then(|matrix| matrix.get("include"))
        .and_then(Value::as_sequence)
        .map(|shards| {
            shards
                .iter()
                .filter(|shard| shard.get("name").and_then(Value::as_str) == Some("policy"))
                .collect()
        })
        .unwrap_or_default();

    // Exactly one row, not merely the first match. `include` may legally repeat
    // a name and every matching entry becomes its own job, so taking the first
    // would let a second `policy` row execute a different gate list unobserved
    // — and would equally fail the assertion for a duplicate that is correctly
    // wired. Requiring uniqueness makes the subject of every assertion below
    // unambiguous instead.
    ensure!(
        rows.len() == 1,
        "expected exactly one `policy` row under \
         jobs.merge-gate-shards.strategy.matrix.include, found {}",
        rows.len()
    );
    let row = rows
        .first()
        .ok_or_else(|| eyre!("ci.yml no longer declares a `policy` merge-gate shard row"))?;

    let gates = row.get("gates").and_then(Value::as_str).ok_or_else(|| {
        eyre!(
            "the `policy` matrix row's `gates` is missing or is not a whitespace-separated scalar"
        )
    })?;
    Ok(gates.split_whitespace().map(str::to_owned).collect())
}

/// The step that actually executes a shard's gate list, located by the
/// `SHARD_GATES` environment binding rather than by step name, so renaming the
/// step does not quietly bypass the conditional check above.
fn shard_runner_step(job: &Value) -> Result<Value> {
    job.get("steps")
        .and_then(Value::as_sequence)
        .and_then(|steps| {
            steps.iter().find(|step| {
                step.get("env")
                    .and_then(|env| env.get("SHARD_GATES"))
                    .and_then(Value::as_str)
                    .is_some_and(binds_matrix_gates)
            })
        })
        .cloned()
        .ok_or_else(|| {
            eyre!(
                "no `merge-gate-shards` step binds SHARD_GATES to `matrix.gates`; \
                 the shard's gate list is no longer executed where this test looks"
            )
        })
}

/// True only when the binding resolves exactly `matrix.gates`.
///
/// Substring matching would also accept `matrix.gates_disabled` or
/// `matrix.gates_legacy`, which would leave the declared gate list unexecuted
/// while this check stayed green. Comparing the unwrapped expression instead
/// tolerates whitespace inside `${{ }}` without tolerating a different key.
fn binds_matrix_gates(binding: &str) -> bool {
    binding
        .trim()
        .strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .is_some_and(|expression| expression.trim() == "matrix.gates")
}

/// Every gate name defined in `.ci/gate-policy.yaml`.
fn defined_gate_names(policy: &Value) -> Result<BTreeSet<String>> {
    let gates = policy
        .get("gates")
        .and_then(Value::as_sequence)
        .ok_or_else(|| eyre!(".ci/gate-policy.yaml no longer defines a `gates` sequence"))?;
    Ok(gates
        .iter()
        .filter_map(|gate| gate.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect())
}
