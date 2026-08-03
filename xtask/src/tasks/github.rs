//! GitHub repository maintenance tasks delegated to the `gh` CLI.

use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequiredContext {
    pub name: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CandidateFacts {
    pub repository: String,
    pub pr: u64,
    pub state: String,
    pub draft: bool,
    pub head_ref: String,
    pub head_sha: String,
    pub base_ref: String,
    pub base_sha: String,
    pub mergeability: String,
    pub merge_state: String,
    pub required_contexts: Vec<RequiredContext>,
    pub required_contexts_result: String,
    pub identity_result: String,
}

#[derive(Debug, Deserialize)]
struct RequiredStatusChecksPayload {
    #[serde(default)]
    contexts: Vec<String>,
    #[serde(default)]
    checks: Vec<RequiredStatusCheck>,
}

#[derive(Debug, Deserialize)]
struct RequiredStatusCheck {
    context: String,
}

pub fn run_labels() -> Result<()> {
    let root = crate::utils::project_root()?;
    let script = root.join("scripts").join("gh").join("ensure-labels.sh");
    run_script(&script, &[])
}

pub fn run_issues_needing_triage(limit: usize) -> Result<()> {
    let root = crate::utils::project_root()?;
    let script = root.join("scripts").join("gh").join("issues-needing-triage.sh");
    let limit = limit.to_string();
    run_script(&script, &[limit.as_str()])
}

pub fn run_backfill_prefixed_labels(apply: bool) -> Result<()> {
    let root = crate::utils::project_root()?;
    let script = root.join("scripts").join("gh").join("backfill-prefixed-labels.sh");
    if apply { run_script(&script, &["--apply"]) } else { run_script(&script, &[]) }
}

pub fn run_candidate(
    pr: u64,
    expected_head: Option<String>,
    fixture: Option<PathBuf>,
    json_only: bool,
) -> Result<()> {
    let mut facts = if let Some(path) = fixture {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read candidate fixture {}", path.display()))?;
        serde_json::from_str::<CandidateFacts>(&raw)
            .with_context(|| format!("failed to parse candidate fixture {}", path.display()))?
    } else {
        collect_candidate(pr)?
    };

    if facts.pr != pr {
        bail!("candidate fact PR #{} does not match requested PR #{}", facts.pr, pr);
    }

    facts.identity_result = identity_result(expected_head.as_deref(), &facts.head_sha).to_string();

    if !json_only {
        println!("candidate PR #{}: {}", facts.pr, facts.identity_result);
        println!("  repository: {}", facts.repository);
        println!("  head: {} ({})", facts.head_ref, facts.head_sha);
        println!("  base: {} ({})", facts.base_ref, facts.base_sha);
        println!("  state: {}, draft: {}", facts.state, facts.draft);
        println!("  mergeability: {} ({})", facts.mergeability, facts.merge_state);
        println!(
            "  required contexts: {} ({})",
            facts
                .required_contexts
                .iter()
                .map(|context| context.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            facts.required_contexts_result
        );
    }

    if json_only {
        println!("{}", serde_json::to_string_pretty(&facts)?);
    }

    if facts.identity_result != "current" {
        bail!("candidate facts are NOT_PROVEN for PR #{}", facts.pr);
    }

    Ok(())
}

/// Collect candidate facts for composition by another factual instrument.
pub fn candidate_facts(pr: u64) -> Result<CandidateFacts> {
    collect_candidate(pr)
}

fn collect_candidate(pr: u64) -> Result<CandidateFacts> {
    let repository =
        command_text("gh", &["repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"])?
            .trim()
            .to_string();
    let pr_text = command_text(
        "gh",
        &[
            "pr",
            "view",
            &pr.to_string(),
            "--json",
            "number,state,isDraft,headRefName,headRefOid,baseRefName,baseRefOid,mergeable,mergeStateStatus",
        ],
    )?;
    let pr_value: Value =
        serde_json::from_str(&pr_text).context("failed to parse gh pr view JSON")?;

    let base_ref = required_string(&pr_value, "baseRefName")?;
    let required_contexts = collect_required_contexts(&repository, &base_ref)?;

    Ok(CandidateFacts {
        repository,
        pr: required_u64(&pr_value, "number")?,
        state: required_string(&pr_value, "state")?,
        draft: required_bool(&pr_value, "isDraft")?,
        head_ref: required_string(&pr_value, "headRefName")?,
        head_sha: required_string(&pr_value, "headRefOid")?,
        base_ref,
        base_sha: required_string(&pr_value, "baseRefOid")?,
        // `gh pr view --json mergeable` exposes GitHub's enum as a string,
        // unlike the REST pull-request payload's boolean field.
        mergeability: required_string(&pr_value, "mergeable")?,
        merge_state: required_string(&pr_value, "mergeStateStatus")?,
        required_contexts,
        // A1 discovers the policy contexts only. A2 owns evaluating their
        // results against this candidate head, so currentness is not proven
        // by this snapshot.
        required_contexts_result: "NOT_PROVEN".to_string(),
        identity_result: "NOT_PROVEN".to_string(),
    })
}

fn collect_required_contexts(repository: &str, base_ref: &str) -> Result<Vec<RequiredContext>> {
    let encoded_base_ref = encode_path_segment(base_ref);
    let branch_endpoint =
        format!("repos/{repository}/branches/{encoded_base_ref}/protection/required_status_checks");
    let branch_raw = command_text("gh", &["api", &branch_endpoint])?;
    let branch_names = required_context_names(&branch_raw)?;

    let rules_endpoint = format!("repos/{repository}/rules/branches/{encoded_base_ref}");
    let rules_raw = command_text("gh", &["api", &rules_endpoint])?;
    let rules: Value = serde_json::from_str(&rules_raw).context("failed to parse branch rules")?;

    Ok(merge_required_contexts(&branch_names, &rules))
}

fn required_context_names(raw: &str) -> Result<Vec<String>> {
    if let Ok(payload) = serde_json::from_str::<RequiredStatusChecksPayload>(raw) {
        let mut names = BTreeSet::new();
        names.extend(payload.contexts);
        names.extend(payload.checks.into_iter().map(|check| check.context));
        return Ok(names.into_iter().collect());
    }
    serde_json::from_str::<Vec<String>>(raw)
        .context("failed to parse required branch-protection checks")
}

fn merge_required_contexts(branch_names: &[String], rules: &Value) -> Vec<RequiredContext> {
    let mut sources = BTreeMap::<String, BTreeSet<String>>::new();
    for name in branch_names {
        sources.entry(name.clone()).or_default().insert("branch_protection".to_string());
    }
    for rule in rules.as_array().into_iter().flatten() {
        if rule.get("type").and_then(Value::as_str) != Some("required_status_checks") {
            continue;
        }
        for check in rule
            .pointer("/parameters/required_status_checks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(name) = check.get("context").and_then(Value::as_str) {
                sources.entry(name.to_string()).or_default().insert("ruleset".to_string());
            }
        }
    }

    sources
        .into_iter()
        .map(|(name, source)| RequiredContext {
            name,
            source: source.into_iter().collect::<Vec<_>>().join("+"),
        })
        .collect()
}

fn encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for &byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
    }
    encoded
}

fn hex_digit(value: u8) -> char {
    b"0123456789ABCDEF"[value as usize] as char
}

pub(crate) fn command_text(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute {program}"))?;
    if !output.status.success() {
        bail!(
            "{program} failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn required_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| color_eyre::eyre::eyre!("gh pr view omitted string field {key}"))
}

fn required_u64(value: &Value, key: &str) -> Result<u64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| color_eyre::eyre::eyre!("gh pr view omitted numeric field {key}"))
}

fn required_bool(value: &Value, key: &str) -> Result<bool> {
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| color_eyre::eyre::eyre!("gh pr view omitted boolean field {key}"))
}

fn identity_result(expected_head: Option<&str>, actual_head: &str) -> &'static str {
    match expected_head {
        Some(expected) if expected == actual_head => "current",
        Some(_) => "moved",
        None => "NOT_PROVEN",
    }
}

fn run_script(script: &Path, args: &[&str]) -> Result<()> {
    let mut command = Command::new("bash");
    command.arg(script);
    for arg in args {
        command.arg(arg);
    }

    let status =
        command.status().with_context(|| format!("failed to execute {}", script.display()))?;

    if status.success() {
        Ok(())
    } else {
        bail!("github maintenance script failed: {}", script.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> CandidateFacts {
        CandidateFacts {
            repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
            pr: 5501,
            state: "OPEN".to_string(),
            draft: false,
            head_ref: "codex/example".to_string(),
            head_sha: "abc123".to_string(),
            base_ref: "main".to_string(),
            base_sha: "def456".to_string(),
            mergeability: "MERGEABLE".to_string(),
            merge_state: "CLEAN".to_string(),
            required_contexts: vec![RequiredContext {
                name: "methodology-gate".to_string(),
                source: "branch_protection".to_string(),
            }],
            required_contexts_result: "NOT_PROVEN".to_string(),
            identity_result: "current".to_string(),
        }
    }

    #[test]
    fn expected_head_comparison_is_explicit() {
        assert_eq!(identity_result(Some("abc123"), "abc123"), "current");
        assert_eq!(identity_result(Some("old456"), "abc123"), "moved");
        assert_eq!(identity_result(None, "abc123"), "NOT_PROVEN");
    }

    #[test]
    fn normalized_facts_round_trip_as_json() -> Result<()> {
        let facts = fixture();
        let encoded = serde_json::to_string(&facts)?;
        let decoded: CandidateFacts = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, facts);
        Ok(())
    }

    #[test]
    fn a1_does_not_claim_required_context_results_are_current() -> Result<()> {
        let facts = fixture();
        assert_eq!(facts.required_contexts_result, "NOT_PROVEN");
        Ok(())
    }

    #[test]
    fn committed_candidate_fixture_reports_current_identity_fields() -> Result<()> {
        let facts: CandidateFacts = serde_json::from_str(include_str!(
            "../../tests/fixtures/github/candidate-current.json"
        ))?;
        assert_eq!(facts.repository, "EffortlessMetrics/perl-lsp-swarm");
        assert_eq!(facts.pr, 5609);
        assert_eq!(facts.state, "OPEN");
        assert!(!facts.draft);
        assert_eq!(facts.base_ref, "main");
        assert_eq!(facts.mergeability, "MERGEABLE");
        assert_eq!(facts.merge_state, "CLEAN");
        assert_eq!(facts.required_contexts.len(), 5);
        assert!(facts.required_contexts.iter().any(|context| {
            context.name == "Compile All Targets (bit-rot guard)" && context.source == "ruleset"
        }));
        assert_eq!(facts.identity_result, "current");
        assert_eq!(facts.required_contexts_result, "NOT_PROVEN");
        assert!(!facts.head_sha.is_empty());
        assert!(!facts.base_sha.is_empty());
        Ok(())
    }

    #[test]
    fn branch_names_are_encoded_as_single_api_path_segments() {
        assert_eq!(encode_path_segment("main"), "main");
        assert_eq!(encode_path_segment("release/#123"), "release%2F%23123");
    }

    #[test]
    fn required_contexts_merge_classic_and_ruleset_sources() -> Result<()> {
        let branch_names = vec!["Shared check".to_string(), "Classic only".to_string()];
        let rules: Value = serde_json::json!([
            {
                "type": "required_status_checks",
                "parameters": {
                    "required_status_checks": [
                        { "context": "Shared check" },
                        { "context": "Ruleset only" }
                    ]
                }
            }
        ]);
        let contexts = merge_required_contexts(&branch_names, &rules);
        assert_eq!(contexts.len(), 3);
        assert_eq!(
            contexts
                .iter()
                .find(|context| context.name == "Shared check")
                .map(|context| context.source.as_str()),
            Some("branch_protection+ruleset")
        );
        assert!(contexts.iter().any(|context| context.name == "Ruleset only"));
        Ok(())
    }

    #[test]
    fn required_context_names_accepts_native_checks_and_legacy_contexts() -> Result<()> {
        let names =
            required_context_names(r#"{"contexts":["legacy"],"checks":[{"context":"native"}]}"#)?;
        assert_eq!(names, vec!["legacy".to_string(), "native".to_string()]);
        Ok(())
    }
}
