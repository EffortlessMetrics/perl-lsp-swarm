//! Contract for the Windows reparse proof cache save boundary.

use std::{fs, path::PathBuf};

use anyhow::{Result, anyhow, ensure};
use serde_yaml_ng::Value;

const TRUSTED_SAVE_IF: &str =
    "${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}";

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn validate_cache_boundary(source: &str) -> Result<()> {
    let workflow: Value = serde_yaml_ng::from_str(source)?;
    let events = workflow
        .get("on")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("the workflow must declare a mapping of events"))?;
    ensure!(
        events.get("pull_request").is_some_and(Value::is_mapping),
        "the workflow must have a structured pull_request trigger"
    );

    let jobs = workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("the workflow must declare jobs"))?;
    let job = jobs
        .get("windows-reparse-proof")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("the workflow must declare the Windows reparse proof job"))?;
    ensure!(
        job.get("runs-on").and_then(Value::as_str) == Some("windows-2022"),
        "the Windows reparse proof must run on windows-2022"
    );

    let permissions = workflow
        .get("permissions")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("the workflow must declare permissions"))?;
    ensure!(
        permissions.get("contents").and_then(Value::as_str) == Some("read"),
        "the workflow must grant contents read permission"
    );

    let steps = job
        .get("steps")
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("the Windows reparse proof job must declare steps"))?;
    let cache_steps: Vec<_> = steps
        .iter()
        .filter(|step| {
            step.get("uses")
                .and_then(Value::as_str)
                .is_some_and(|uses| uses.starts_with("Swatinem/rust-cache@"))
        })
        .collect();
    ensure!(
        cache_steps.len() == 1,
        "the Windows reparse proof job must have exactly one rust-cache step"
    );
    ensure!(
        steps.iter().any(|step| {
            step.get("name").and_then(Value::as_str)
                == Some("Run non-skipping Windows reparse proof")
                && step.get("run").and_then(Value::as_str).is_some()
        }),
        "the Windows reparse proof job must reach its non-skipping proof"
    );

    let cache_with = cache_steps[0]
        .get("with")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("the rust-cache step must declare inputs"))?;
    ensure!(
        cache_with.get("save-if").and_then(Value::as_str) == Some(TRUSTED_SAVE_IF),
        "rust-cache must save only for the canonical main/master refs"
    );
    Ok(())
}

fn fixture(save_if: &str) -> String {
    format!(
        "on:\n  pull_request:\n    branches: [main]\npermissions:\n  contents: read\njobs:\n  windows-reparse-proof:\n    runs-on: windows-2022\n    steps:\n      - uses: Swatinem/rust-cache@pinned\n        with:\n          save-if: '{save_if}'\n      - name: Run non-skipping Windows reparse proof\n        run: cargo test --locked\n"
    )
}

#[test]
fn corpus_windows_reparse_workflow_has_restore_only_pr_cache_boundary() -> Result<()> {
    let path = project_root().join(".github/workflows/corpus-windows-reparse-proof.yml");
    validate_cache_boundary(&fs::read_to_string(path)?)
}

#[test]
fn cache_contract_rejects_missing_save_boundary() -> Result<()> {
    let source = fixture("true").replace("save-if: true", "");
    ensure!(
        validate_cache_boundary(&source).is_err(),
        "a cache writer without save-if must be rejected"
    );
    Ok(())
}

#[test]
fn cache_contract_rejects_malformed_pull_request_triggers() -> Result<()> {
    for trigger in ["null", "false", "true", "pull-request"] {
        let source = fixture(TRUSTED_SAVE_IF).replacen(
            "pull_request:\n    branches: [main]",
            &format!("pull_request: {trigger}"),
            1,
        );
        ensure!(
            validate_cache_boundary(&source).is_err(),
            "malformed pull_request trigger must be rejected: {trigger}"
        );
    }
    Ok(())
}

#[test]
fn cache_contract_rejects_cache_step_outside_windows_proof_job() -> Result<()> {
    let source = fixture(TRUSTED_SAVE_IF)
        .replace(
            "      - uses: Swatinem/rust-cache@pinned\n        with:\n          save-if: '${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}'\n",
            "",
        )
        .replace(
            "jobs:\n  windows-reparse-proof:",
            "jobs:\n  decoy:\n    runs-on: ubuntu-24.04\n    steps:\n      - uses: Swatinem/rust-cache@pinned\n        with:\n          save-if: '${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}'\n  windows-reparse-proof:",
        );
    ensure!(
        validate_cache_boundary(&source).is_err(),
        "a cache step outside the Windows proof job must be rejected"
    );
    Ok(())
}

#[test]
fn cache_contract_rejects_unconditional_save() -> Result<()> {
    ensure!(validate_cache_boundary(&fixture("true")).is_err(), "save-if: true must be rejected");
    Ok(())
}

#[test]
fn cache_contract_rejects_pr_and_noncanonical_refs() -> Result<()> {
    for save_if in [
        "${{ github.ref == 'refs/pull/1/merge' }}",
        "${{ github.ref == 'refs/heads/feature/cache' }}",
        "${{ github.ref == 'refs/heads/main' || github.ref == 'refs/tags/v1' }}",
    ] {
        ensure!(
            validate_cache_boundary(&fixture(save_if)).is_err(),
            "non-canonical cache save ref must be rejected: {save_if}"
        );
    }
    Ok(())
}
