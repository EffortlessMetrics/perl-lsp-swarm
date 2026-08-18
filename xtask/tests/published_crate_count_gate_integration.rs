//! Integration contract for the one active published-crate-count gate.
//!
//! The transition-era duplicate `published_crate_count` merge-gate row was
//! quarantined while the workspace still carried 81 publishable crates. The
//! collapse is complete and `published_crate_count_pr_fast` now owns the same
//! xtask predicate at the current exact baseline.

use serde_yaml_ng::Value;
use std::fs;
use std::path::{Path, PathBuf};

const ACTIVE_GATE: &str = "published_crate_count_pr_fast";
const OBSOLETE_GATE: &str = "published_crate_count";

fn workspace_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask must be a direct workspace member".to_string())
}

fn read_workspace_file(relative: &str) -> Result<String, String> {
    let path = workspace_root()?.join(relative);
    fs::read_to_string(&path).map_err(|error| format!("failed to read {path:?}: {error}"))
}

fn load_gate_policy_yaml() -> Result<Value, String> {
    serde_yaml_ng::from_str(&read_workspace_file(".ci/gate-policy.yaml")?)
        .map_err(|error| format!("gate-policy.yaml must be valid YAML: {error}"))
}

fn gates(policy: &Value) -> Result<&[Value], String> {
    policy
        .get("gates")
        .and_then(Value::as_sequence)
        .map(Vec::as_slice)
        .ok_or_else(|| "gate-policy.yaml must contain a gates sequence".to_string())
}

fn gate_named<'a>(policy: &'a Value, name: &str) -> Result<&'a Value, String> {
    gates(policy)?
        .iter()
        .find(|gate| gate.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| format!("gate {name:?} must exist"))
}

fn string_field<'a>(gate: &'a Value, field: &str) -> Result<&'a str, String> {
    gate.get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("gate field {field:?} must be a string"))
}

fn bool_field(gate: &Value, field: &str) -> Result<bool, String> {
    gate.get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("gate field {field:?} must be a boolean"))
}

#[test]
fn active_pr_fast_gate_owns_the_published_crate_count_predicate() -> Result<(), String> {
    let policy = load_gate_policy_yaml()?;
    let active_count = gates(&policy)?
        .iter()
        .filter(|gate| gate.get("name").and_then(Value::as_str) == Some(ACTIVE_GATE))
        .count();
    if active_count != 1 {
        return Err(format!("{ACTIVE_GATE} must exist exactly once; found {active_count}"));
    }

    let gate = gate_named(&policy, ACTIVE_GATE)?;
    if string_field(gate, "tier")? != "pr_fast" {
        return Err("active crate-count gate must remain in pr_fast".to_string());
    }
    if !bool_field(gate, "required")? {
        return Err("active crate-count gate must remain required".to_string());
    }
    if bool_field(gate, "quarantine")? {
        return Err("active crate-count gate must remain non-quarantined".to_string());
    }
    if string_field(gate, "command")? != "just ci-published-crate-count" {
        return Err("active gate must keep the canonical just recipe".to_string());
    }
    Ok(())
}

#[test]
fn obsolete_quarantined_merge_gate_does_not_return() -> Result<(), String> {
    let policy = load_gate_policy_yaml()?;
    if gates(&policy)?
        .iter()
        .any(|gate| gate.get("name").and_then(Value::as_str) == Some(OBSOLETE_GATE))
    {
        return Err("the duplicate quarantined published_crate_count merge gate must stay retired"
            .to_string());
    }
    Ok(())
}

#[test]
fn obsolete_gate_is_not_selected_by_a_workflow_matrix() -> Result<(), String> {
    let workflow = read_workspace_file(".github/workflows/ci.yml")?;
    for line in workflow.lines() {
        let trimmed = line.trim_start();
        let matrix = trimmed.strip_prefix("gates: ").or_else(|| trimmed.strip_prefix("- gates: "));
        if matrix.is_some_and(|value| value.split_whitespace().any(|token| token == OBSOLETE_GATE))
        {
            return Err(format!(
                "workflow matrix still selects obsolete gate {OBSOLETE_GATE:?}: {trimmed}"
            ));
        }
    }
    Ok(())
}

#[test]
fn just_recipe_still_delegates_to_the_xtask_ratchet() -> Result<(), String> {
    let justfile = read_workspace_file("justfile")?;
    let marker = "ci-published-crate-count:";
    let body = justfile
        .split_once(marker)
        .map(|(_, body)| body)
        .ok_or_else(|| "ci-published-crate-count recipe must exist".to_string())?;
    let recipe = body.split_once("\n\n").map_or(body, |(recipe, _)| recipe);
    if !recipe.lines().any(|line| {
        matches!(
            line.trim(),
            "cargo xtask published-crate-count" | "@cargo xtask published-crate-count"
        )
    }) {
        return Err("ci-published-crate-count must delegate to cargo xtask published-crate-count"
            .to_string());
    }
    Ok(())
}
