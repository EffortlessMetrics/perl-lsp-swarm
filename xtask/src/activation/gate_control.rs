//! Gate activation truth for inventory rows (#9210).
//!
//! Consumes existing CI control-plane authorities (gate policy, workflow
//! reachability, tier enforcement) without re-parsing workflow prose as a
//! second route authority. Live GitHub enforcement is not claimed before the
//! #9194-#9197 merge-proof transition completes (#9197).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::model::ActivationError;

const GATE_POLICY_YAML: &str = ".ci/gate-policy.yaml";
const WORKFLOW_GLOB: &str = ".github/workflows";

/// Workflow-scoped tiers whose required gates must have a statically visible
/// producer path (same scope as `validate_gate_lane_mapping.py`).
const WORKFLOW_SCOPED_TIERS: &[&str] = &["pr_fast", "merge_gate", "release"];

/// Merge-proof contexts remain checked-in advisory until #9197 promotes live
/// protection; activation must not treat them as live required enforcement.
const MERGE_PROOF_CHECKED_IN_ADVISORY: &[&str] =
    &["Merge Proof Finalizer", "merge-proof-finalizer", "merge_proof_finalizer"];

#[derive(Debug, Clone)]
struct GateSpec {
    tier: String,
    required: bool,
    command: String,
    quarantine: bool,
    tier_enforcement: String,
}

#[derive(Debug, Clone)]
pub struct GateControlSnapshot {
    gates: BTreeMap<String, GateSpec>,
    reachable: BTreeSet<String>,
}

impl GateControlSnapshot {
    pub fn load(root: &Path) -> Result<Self, ActivationError> {
        let policy_path = root.join(GATE_POLICY_YAML);
        let policy_text = read_text(&policy_path)?;
        let policy: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&policy_text).map_err(|error| {
                ActivationError::new(format!("{GATE_POLICY_YAML}: invalid YAML: {error}"))
            })?;

        let tier_enforcement = tier_enforcement_map(&policy)?;
        let gates = gate_specs(&policy, &tier_enforcement)?;
        let reachable = workflow_reachable_gates(root, &gates)?;
        Ok(Self { gates, reachable })
    }

    pub fn evaluate_gate_id(
        &self,
        gate_id: &str,
        registration_detail: Option<&str>,
    ) -> Vec<String> {
        let Some(spec) = self.gates.get(gate_id) else {
            return Vec::new();
        };

        let mut reasons = Vec::new();

        if spec.command.trim().is_empty() {
            reasons.push(format!("gate `{gate_id}` has no command in {GATE_POLICY_YAML}"));
        }

        if registration_detail_claims_required(registration_detail) && !spec.required {
            reasons.push(format!(
                "gate `{gate_id}` registration detail claims required enforcement but policy declares advisory"
            ));
        }

        if spec.required && registration_detail_claims_advisory_only(registration_detail) {
            reasons.push(format!(
                "gate `{gate_id}` registration detail claims advisory-only but policy declares required"
            ));
        }

        if spec.quarantine && spec.required {
            reasons.push(format!(
                "required gate `{gate_id}` is quarantined in {GATE_POLICY_YAML}; lifecycle is not active"
            ));
        }

        if WORKFLOW_SCOPED_TIERS.contains(&spec.tier.as_str()) {
            if spec.required && !self.reachable.contains(gate_id) {
                reasons.push(format!(
                    "required gate `{gate_id}` has no statically visible workflow producer for tier `{}`",
                    spec.tier
                ));
            }
            if spec.required && spec.tier_enforcement == "local" {
                reasons.push(format!(
                    "required gate `{gate_id}` tier `{}` is local-only enforcement and cannot be active on CI integration paths",
                    spec.tier
                ));
            }
        }

        reasons
    }
}

pub fn gate_id_from_surface(surface_id: &str) -> Option<&str> {
    surface_id.strip_prefix("gate:")
}

fn read_text(path: &Path) -> Result<String, ActivationError> {
    fs::read_to_string(path)
        .map_err(|error| ActivationError::new(format!("{}: cannot read: {error}", path.display())))
}

fn tier_enforcement_map(
    policy: &serde_yaml_ng::Value,
) -> Result<BTreeMap<String, String>, ActivationError> {
    let tiers =
        policy.get("tiers").and_then(serde_yaml_ng::Value::as_mapping).ok_or_else(|| {
            ActivationError::new(format!("{GATE_POLICY_YAML}: missing `tiers` mapping"))
        })?;
    let mut out = BTreeMap::new();
    for (key, value) in tiers {
        let Some(name) = key.as_str() else {
            continue;
        };
        let enforcement = value
            .get("enforcement")
            .and_then(serde_yaml_ng::Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        out.insert(name.to_string(), enforcement);
    }
    Ok(out)
}

fn gate_specs(
    policy: &serde_yaml_ng::Value,
    tier_enforcement: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, GateSpec>, ActivationError> {
    let gates =
        policy.get("gates").and_then(serde_yaml_ng::Value::as_sequence).ok_or_else(|| {
            ActivationError::new(format!("{GATE_POLICY_YAML}: missing `gates` sequence"))
        })?;
    let mut out = BTreeMap::new();
    for gate in gates {
        let Some(mapping) = gate.as_mapping() else {
            continue;
        };
        let name = mapping
            .get(serde_yaml_ng::Value::from("name"))
            .and_then(serde_yaml_ng::Value::as_str)
            .ok_or_else(|| {
                ActivationError::new(format!("{GATE_POLICY_YAML}: gate row missing name"))
            })?;
        let tier = mapping
            .get(serde_yaml_ng::Value::from("tier"))
            .and_then(serde_yaml_ng::Value::as_str)
            .ok_or_else(|| {
                ActivationError::new(format!("{GATE_POLICY_YAML}: gate `{name}` missing tier"))
            })?
            .to_string();
        let required = match mapping.get(serde_yaml_ng::Value::from("required")) {
            None => true,
            Some(value) => value.as_bool().ok_or_else(|| {
                ActivationError::new(format!(
                    "{GATE_POLICY_YAML}: gate `{name}` has non-boolean `required`"
                ))
            })?,
        };
        let command = mapping
            .get(serde_yaml_ng::Value::from("command"))
            .and_then(serde_yaml_ng::Value::as_str)
            .unwrap_or("")
            .to_string();
        let quarantine = mapping
            .get(serde_yaml_ng::Value::from("quarantine"))
            .and_then(serde_yaml_ng::Value::as_bool)
            .unwrap_or(false);
        let tier_enforcement =
            tier_enforcement.get(&tier).cloned().unwrap_or_else(|| "unknown".to_string());
        out.insert(
            name.to_string(),
            GateSpec { tier, required, command, quarantine, tier_enforcement },
        );
    }
    Ok(out)
}

fn registration_detail_claims_required(detail: Option<&str>) -> bool {
    let Some(detail) = detail else {
        return false;
    };
    detail.contains("required = true") || detail.contains("required=true")
}

fn registration_detail_claims_advisory_only(detail: Option<&str>) -> bool {
    let Some(detail) = detail else {
        return false;
    };
    detail.contains("required = false") || detail.contains("required=false")
}

fn workflow_reachable_gates(
    root: &Path,
    gates: &BTreeMap<String, GateSpec>,
) -> Result<BTreeSet<String>, ActivationError> {
    let known: BTreeSet<String> = gates.keys().cloned().collect();
    let mut reachable = BTreeSet::new();
    let workflows_dir = root.join(WORKFLOW_GLOB);
    let entries = fs::read_dir(&workflows_dir)
        .map_err(|error| ActivationError::new(format!("{WORKFLOW_GLOB}: cannot list: {error}")))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            ActivationError::new(format!("{WORKFLOW_GLOB}: cannot read entry: {error}"))
        })?;
        let path = entry.path();
        let ext = path.extension().and_then(|value| value.to_str()).unwrap_or("");
        if ext != "yml" && ext != "yaml" {
            continue;
        }
        let text = read_text(&path)?;
        scan_workflow_text(&text, gates, &known, &mut reachable);
    }
    Ok(reachable)
}

fn scan_workflow_text(
    text: &str,
    gates: &BTreeMap<String, GateSpec>,
    known: &BTreeSet<String>,
    reachable: &mut BTreeSet<String>,
) {
    let mut in_run_block = false;
    let mut run_block_indent = -1;
    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or("");
        if line.trim().is_empty() {
            continue;
        }
        let indent = line_indent(line);
        if in_run_block && indent <= run_block_indent {
            in_run_block = false;
        }
        let stripped = line.trim_start();
        if let Some(block) = stripped.strip_prefix("run:") {
            let rest = block.trim();
            if rest == "|" || rest == ">" || rest.starts_with("|-") || rest.starts_with(">-") {
                in_run_block = true;
                run_block_indent = indent;
                continue;
            }
            record_gate_matches(rest, gates, known, reachable);
            continue;
        }
        if in_run_block && indent > run_block_indent {
            record_gate_matches(stripped, gates, known, reachable);
            continue;
        }
        if workflow_line_is_metadata(line) {
            continue;
        }
        record_gate_matches(stripped, gates, known, reachable);
    }
}

fn line_indent(line: &str) -> i32 {
    line.len() as i32 - line.trim_start().len() as i32
}

fn workflow_line_is_metadata(line: &str) -> bool {
    let stripped = line.trim_start();
    if stripped.starts_with("- gates:") || stripped.starts_with("gates:") {
        return false;
    }
    if line.contains("run:") || stripped.starts_with("run ") {
        return false;
    }
    true
}

fn record_gate_matches(
    line: &str,
    gates: &BTreeMap<String, GateSpec>,
    known: &BTreeSet<String>,
    reachable: &mut BTreeSet<String>,
) {
    if let Some(tier) = extract_tier_invocation(line) {
        reachable.extend(
            gates.iter().filter(|(_, spec)| spec.tier == tier).map(|(name, _)| name.clone()),
        );
    }
    if let Some(list) = extract_gates_matrix(line) {
        for token in list {
            if known.contains(&token) {
                reachable.insert(token);
            }
        }
    }
    if let Some(gate) = extract_single_gate(line) {
        if known.contains(&gate) {
            reachable.insert(gate);
        }
    }
}

fn extract_tier_invocation(line: &str) -> Option<String> {
    let marker = "gates --tier ";
    let start = line.find(marker)? + marker.len();
    let tail = &line[start..];
    let tier: String = tail
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect();
    if tier.is_empty() {
        return None;
    }
    Some(tier.replace('-', "_"))
}

fn extract_gates_matrix(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("- gates:").or_else(|| trimmed.strip_prefix("gates:"))?;
    let tokens: Vec<String> = rest
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .filter(|token| !token.is_empty() && token.starts_with(|c: char| c.is_ascii_lowercase()))
        .map(str::to_string)
        .collect();
    if tokens.is_empty() { None } else { Some(tokens) }
}

fn extract_single_gate(line: &str) -> Option<String> {
    let marker = "gates --gate ";
    let start = line.find(marker)? + marker.len();
    let tail = &line[start..];
    let gate: String =
        tail.chars().take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_').collect();
    if gate.is_empty() { None } else { Some(gate) }
}

/// Returns a failure reason when a surface claims live merge-proof enforcement
/// before the staged promotion issue (#9197) lands.
pub fn merge_proof_live_enforcement_claim(
    notes: Option<&str>,
    proof_ids: &[String],
) -> Option<String> {
    let haystacks: Vec<String> =
        notes.into_iter().map(str::to_string).chain(proof_ids.iter().cloned()).collect();
    for haystack in haystacks {
        let lower = haystack.to_ascii_lowercase();
        if MERGE_PROOF_CHECKED_IN_ADVISORY
            .iter()
            .any(|token| lower.contains(&token.to_ascii_lowercase()))
            && (lower.contains("live enforcement")
                || lower.contains("github-ruleset")
                || lower.contains("branch protection"))
        {
            return Some(
                "merge-proof transition (#9194-#9197) is checked-in advisory until #9197; live enforcement cannot be claimed"
                    .to_string(),
            );
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".."))
    }

    #[test]
    fn required_unreachable_gate_is_not_active() {
        let mut gates = BTreeMap::new();
        gates.insert(
            "dead_required".to_string(),
            GateSpec {
                tier: "merge_gate".to_string(),
                required: true,
                command: "cargo xtask gates --gate dead_required".to_string(),
                quarantine: false,
                tier_enforcement: "required".to_string(),
            },
        );
        let snapshot = GateControlSnapshot { gates, reachable: BTreeSet::new() };
        let reasons = snapshot
            .evaluate_gate_id("dead_required", Some("tier = \"merge_gate\"; required = true"));
        assert!(
            reasons.iter().any(|reason| reason.contains("no statically visible workflow producer")),
            "{reasons:?}"
        );
    }

    #[test]
    fn advisory_registration_cannot_claim_required_enforcement() {
        let mut gates = BTreeMap::new();
        gates.insert(
            "advisory_only".to_string(),
            GateSpec {
                tier: "pr_fast".to_string(),
                required: false,
                command: "true".to_string(),
                quarantine: false,
                tier_enforcement: "required".to_string(),
            },
        );
        let snapshot =
            GateControlSnapshot { gates, reachable: BTreeSet::from(["advisory_only".to_string()]) };
        let reasons =
            snapshot.evaluate_gate_id("advisory_only", Some("tier = \"pr_fast\"; required = true"));
        assert!(
            reasons.iter().any(|reason| reason.contains("claims required enforcement")),
            "{reasons:?}"
        );
    }

    #[test]
    fn quarantined_required_gate_is_not_active() {
        let mut gates = BTreeMap::new();
        gates.insert(
            "required_quarantined".to_string(),
            GateSpec {
                tier: "merge_gate".to_string(),
                required: true,
                command: "cargo xtask gates --gate required_quarantined".to_string(),
                quarantine: true,
                tier_enforcement: "required".to_string(),
            },
        );
        let snapshot = GateControlSnapshot {
            gates,
            reachable: BTreeSet::from(["required_quarantined".to_string()]),
        };
        let reasons = snapshot.evaluate_gate_id(
            "required_quarantined",
            Some("tier = \"merge_gate\"; required = true"),
        );
        assert!(reasons.iter().any(|reason| reason.contains("quarantined")), "{reasons:?}");
    }

    #[test]
    fn live_merge_proof_enforcement_claim_is_rejected() {
        let reason = merge_proof_live_enforcement_claim(
            Some("Merge Proof Finalizer has live enforcement on main"),
            &[],
        );
        assert!(reason.is_some(), "{reason:?}");
    }

    #[test]
    fn main_tree_required_gates_are_reachable() {
        let root = repo_root();
        let snapshot = GateControlSnapshot::load(&root).expect("load gate control snapshot");
        let mut failures = Vec::new();
        for (name, spec) in &snapshot.gates {
            if !spec.required {
                continue;
            }
            if !WORKFLOW_SCOPED_TIERS.contains(&spec.tier.as_str()) {
                continue;
            }
            if !snapshot.reachable.contains(name) {
                failures.push(name.clone());
            }
        }
        assert!(failures.is_empty(), "required unreachable gates: {failures:?}");
    }
}
