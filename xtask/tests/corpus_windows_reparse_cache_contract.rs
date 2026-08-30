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
    ensure!(
        workflow.get("on").and_then(|events| events.get("pull_request")).is_some(),
        "the workflow must remain PR-capable"
    );

    let jobs = workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("the workflow must declare jobs"))?;
    let cache_steps: Vec<_> = jobs
        .values()
        .filter_map(Value::as_mapping)
        .filter_map(|job| job.get("steps"))
        .filter_map(Value::as_sequence)
        .flat_map(|steps| steps.iter())
        .filter(|step| {
            step.get("uses")
                .and_then(Value::as_str)
                .is_some_and(|uses| uses.starts_with("Swatinem/rust-cache@"))
        })
        .collect();
    ensure!(cache_steps.len() == 1, "the workflow must have exactly one canonical rust-cache step");

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
        "on:\n  pull_request:\njobs:\n  proof:\n    steps:\n      - uses: Swatinem/rust-cache@pinned\n        with:\n          save-if: '{save_if}'\n"
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
