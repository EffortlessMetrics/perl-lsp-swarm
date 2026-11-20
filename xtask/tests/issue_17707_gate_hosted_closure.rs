//! #17707 — required `policy` shard gates must have hosted closure and a
//! documented merge order for the four in-flight gate PRs.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use anyhow::{Result, anyhow, ensure};
use perl_tdd_support::must;
use serde_yaml_ng::Value;

#[test]
fn issue_17707_gate_stack_merge_order_doc_lists_four_prs() -> Result<()> {
    let root = repo_root();
    let doc = must(fs::read_to_string(root.join("docs/ci/gate-stack-17405-merge-order.md")));
    for marker in [
        "#17411",
        "#17432",
        "#17433",
        "#17434",
        "test_deletion_gate",
        "secret_scan",
        "change_scope_check",
    ] {
        ensure!(
            doc.contains(marker),
            "gate-stack merge-order doc must mention `{marker}` for #17707 coordination"
        );
    }
    Ok(())
}

#[test]
fn issue_17707_policy_shard_required_gates_have_hosted_closure() -> Result<()> {
    let root = repo_root();
    let policy_text = must(fs::read_to_string(root.join(".ci/gate-policy.yaml")));
    let policy: Value = serde_yaml_ng::from_str(&policy_text)?;
    let workflow: Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root.join(".github/workflows/ci.yml"))?)?;
    let execution = must(fs::read_to_string(root.join(".ci/gate-shard-execution.json")));
    let lane_map = must(fs::read_to_string(root.join("scripts/ci/validate_gate_lane_mapping.py")));

    let shard_gates = policy_shard_gates(&workflow)?;

    for gate_name in &shard_gates {
        let block = gate_block(&policy_text, gate_name)?;
        if !block.contains("required: true") || !block.contains("tier: merge_gate") {
            continue;
        }
        hosted_closure_for_gate(gate_name, block, &policy, &execution, &lane_map)?;
    }

    Ok(())
}

fn hosted_closure_for_gate(
    gate_name: &str,
    gate_block: &str,
    policy: &Value,
    execution_json: &str,
    lane_map_py: &str,
) -> Result<()> {
    for needle in ["tier: merge_gate", "required: true", "quarantine: false"] {
        ensure!(
            gate_block.contains(needle),
            "gate `{gate_name}` on the policy shard must contain `{needle}`; got:\n{gate_block}"
        );
    }

    let needle = format!("\"{gate_name}\"");
    ensure!(
        execution_json.contains(&needle),
        "gate-shard-execution.json must carry a dependency row for `{gate_name}`"
    );
    ensure!(
        lane_map_py.contains(&needle),
        "validate_gate_lane_mapping.py GATE_TO_LANE_MAP must account for `{gate_name}`"
    );

    let defined = defined_gate_names(policy)?;
    ensure!(defined.contains(gate_name), "`.ci/gate-policy.yaml` must define `{gate_name}`");

    Ok(())
}

fn gate_block<'a>(policy_text: &'a str, gate_name: &str) -> Result<&'a str> {
    let marker = format!("  - name: {gate_name}");
    let start =
        policy_text.find(&marker).ok_or_else(|| anyhow!("gate policy missing `{gate_name}`"))?;
    let tail = &policy_text[start..];
    let end = tail.find("\n  - name:").unwrap_or(tail.len());
    Ok(&tail[..end])
}

fn policy_shard_gates(workflow: &Value) -> Result<Vec<String>> {
    let job = workflow
        .get("jobs")
        .and_then(|jobs| jobs.get("merge-gate-shards"))
        .ok_or_else(|| anyhow!("ci.yml missing jobs.merge-gate-shards"))?;
    let rows: Vec<&Value> = job
        .get("strategy")
        .and_then(|s| s.get("matrix"))
        .and_then(|m| m.get("include"))
        .and_then(Value::as_sequence)
        .map(|shards| {
            shards
                .iter()
                .filter(|shard| shard.get("name").and_then(Value::as_str) == Some("policy"))
                .collect()
        })
        .unwrap_or_default();
    ensure!(rows.len() == 1, "expected exactly one policy shard row, found {}", rows.len());
    let gates = rows[0]
        .get("gates")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("policy shard row missing gates scalar"))?;
    Ok(gates.split_whitespace().map(str::to_owned).collect())
}

fn defined_gate_names(policy: &Value) -> Result<BTreeSet<String>> {
    let gates = policy
        .get("gates")
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("gate policy missing gates sequence"))?;
    Ok(gates
        .iter()
        .filter_map(|gate| gate.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}
