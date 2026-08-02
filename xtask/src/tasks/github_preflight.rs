//! Read-only protected-merge preflight composed from the factual GitHub slices.

use super::github::{self, CandidateFacts};
use super::github_review::{self, ReviewSnapshot};
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequiredCheckFact {
    pub name: String,
    pub result: String,
    pub evaluated_head_sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtectedMergeFacts {
    pub base_ref: String,
    pub policy_source: String,
    pub required_contexts: Vec<String>,
    pub mergeability: String,
    pub merge_state: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PreflightSnapshot {
    pub schema_version: &'static str,
    pub repository: String,
    pub pr: u64,
    pub head_sha: String,
    pub candidate: CandidateFacts,
    pub review: ReviewSnapshot,
    pub required_checks: Vec<RequiredCheckFact>,
    pub protected_merge: ProtectedMergeFacts,
    pub result: String,
    pub findings: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CheckRunsPayload {
    check_runs: Vec<CheckRun>,
}

#[derive(Debug, Deserialize)]
struct CommitStatusPayload {
    statuses: Vec<CommitStatus>,
}

#[derive(Debug, Deserialize)]
struct CommitStatus {
    context: String,
    state: String,
}

#[derive(Debug, Deserialize)]
struct CheckRun {
    name: String,
    status: String,
    conclusion: Option<String>,
    head_sha: Option<String>,
}

pub fn run_preflight(pr: u64, json_only: bool) -> Result<()> {
    let initial_candidate = github::candidate_facts(pr)?;
    let repository = initial_candidate.repository.clone();
    let review = github_review::review_snapshot(pr)?;
    let mut candidate = github::candidate_facts(pr)?;
    let mut errors = review.errors.clone();
    if initial_candidate.head_sha != candidate.head_sha {
        errors.push("candidate head moved while composing the preflight snapshot".to_string());
    }
    if review.head_sha != candidate.head_sha {
        errors.push("candidate and review snapshots describe different heads".to_string());
    }
    let required_checks = match collect_required_checks(&candidate) {
        Ok(checks) => checks,
        Err(error) => {
            errors.push(error.to_string());
            Vec::new()
        }
    };
    match github::candidate_facts(pr) {
        Ok(final_candidate) if final_candidate.head_sha == candidate.head_sha => {}
        Ok(_) => errors.push("candidate head moved before preflight completion".to_string()),
        Err(error) => errors.push(format!("failed to revalidate candidate head: {error}")),
    }
    candidate.required_contexts_result = summarize_required_checks(&required_checks, &errors);
    let protected_merge = ProtectedMergeFacts {
        base_ref: candidate.base_ref.clone(),
        policy_source: "branch_protection".to_string(),
        required_contexts: candidate
            .required_contexts
            .iter()
            .map(|context| context.name.clone())
            .collect(),
        mergeability: candidate.mergeability.clone(),
        merge_state: candidate.merge_state.clone(),
    };
    let (result, findings) =
        derive_preflight_result(&candidate, &review, &required_checks, &errors);
    let snapshot = PreflightSnapshot {
        schema_version: "github-preflight.v1",
        repository,
        pr,
        head_sha: candidate.head_sha.clone(),
        candidate,
        review,
        required_checks,
        protected_merge,
        result,
        findings,
        errors,
    };
    if !json_only {
        println!("protected merge preflight PR #{}: {}", snapshot.pr, snapshot.result);
    }
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    if snapshot.result == "NOT_PROVEN" {
        bail!("protected merge preflight is NOT_PROVEN for PR #{}", pr);
    }
    Ok(())
}

fn collect_required_checks(candidate: &CandidateFacts) -> Result<Vec<RequiredCheckFact>> {
    let mut all_check_runs = Vec::new();
    let mut page = 1;
    loop {
        let endpoint = format!(
            "repos/{}/commits/{}/check-runs?per_page=100&page={page}",
            candidate.repository, candidate.head_sha
        );
        let raw = command_text("gh", &["api", &endpoint])?;
        let payload: CheckRunsPayload = serde_json::from_str(&raw)
            .context("failed to parse check runs for the captured candidate head")?;
        let count = payload.check_runs.len();
        all_check_runs.extend(payload.check_runs);
        if count < 100 {
            break;
        }
        page += 1;
    }
    let mut all_statuses = Vec::new();
    let mut page = 1;
    loop {
        let endpoint = format!(
            "repos/{}/commits/{}/status?per_page=100&page={page}",
            candidate.repository, candidate.head_sha
        );
        let raw = command_text("gh", &["api", &endpoint])?;
        let payload: CommitStatusPayload = serde_json::from_str(&raw)
            .context("failed to parse commit statuses for the captured candidate head")?;
        let count = payload.statuses.len();
        all_statuses.extend(payload.statuses);
        if count < 100 {
            break;
        }
        page += 1;
    }
    Ok(candidate
        .required_contexts
        .iter()
        .map(|required| {
            let matching = all_check_runs.iter().find(|run| run.name == required.name);
            let status = all_statuses.iter().find(|status| status.context == required.name);
            match matching {
                Some(run) => RequiredCheckFact {
                    name: required.name.clone(),
                    result: classify_check(run, &candidate.head_sha),
                    evaluated_head_sha: run.head_sha.clone(),
                },
                None => match status {
                    Some(status) => RequiredCheckFact {
                        name: required.name.clone(),
                        result: classify_status(status),
                        evaluated_head_sha: Some(candidate.head_sha.clone()),
                    },
                    None => RequiredCheckFact {
                        name: required.name.clone(),
                        result: "MISSING".to_string(),
                        evaluated_head_sha: None,
                    },
                },
            }
        })
        .collect())
}

fn classify_check(run: &CheckRun, candidate_head_sha: &str) -> String {
    if run.head_sha.as_deref() != Some(candidate_head_sha) {
        return "STALE".to_string();
    }
    if !run.status.eq_ignore_ascii_case("COMPLETED") {
        return "PENDING".to_string();
    }
    match run.conclusion.as_deref().map(str::to_ascii_uppercase).as_deref() {
        Some("SUCCESS") | Some("NEUTRAL") => "SUCCESS",
        Some("SKIPPED") => "SKIPPED",
        Some("CANCELLED") => "CANCELLED",
        Some("TIMED_OUT") | Some("FAILURE") | Some("ACTION_REQUIRED") => "FAILURE",
        Some("STALE") => "STALE",
        _ => "INSTRUMENT_FAILURE",
    }
    .to_string()
}

fn classify_status(status: &CommitStatus) -> String {
    match status.state.to_ascii_uppercase().as_str() {
        "SUCCESS" => "SUCCESS",
        "PENDING" => "PENDING",
        "ERROR" | "FAILURE" => "FAILURE",
        _ => "INSTRUMENT_FAILURE",
    }
    .to_string()
}

fn summarize_required_checks(checks: &[RequiredCheckFact], errors: &[String]) -> String {
    if !errors.is_empty() {
        return "NOT_PROVEN".to_string();
    }
    if checks.iter().all(|check| check.result == "SUCCESS") {
        "SUCCESS".to_string()
    } else {
        "BLOCKED".to_string()
    }
}

fn derive_preflight_result(
    candidate: &CandidateFacts,
    review: &ReviewSnapshot,
    checks: &[RequiredCheckFact],
    errors: &[String],
) -> (String, Vec<String>) {
    let mut findings = Vec::new();
    if !errors.is_empty() {
        findings.push(
            "candidate, review, or required-check data was partial or instrument-failed"
                .to_string(),
        );
        return ("NOT_PROVEN".to_string(), findings);
    }
    if candidate.identity_result != "current" {
        findings.push("candidate identity is not current".to_string());
    }
    if candidate.head_sha != review.head_sha {
        findings.push("candidate and review snapshots describe different heads".to_string());
    }
    if candidate.state != "OPEN" {
        findings.push(format!("candidate state is {}", candidate.state));
    }
    if candidate.draft {
        findings.push("candidate is a draft".to_string());
    }
    if candidate.mergeability != "MERGEABLE" || candidate.merge_state != "CLEAN" {
        findings.push(format!(
            "native merge state is {} / {}",
            candidate.mergeability, candidate.merge_state
        ));
    }
    if review.result != "CURRENT" || !review.converged {
        findings.push(format!("review convergence is {}", review.result));
    }
    for check in checks {
        if check.result != "SUCCESS" {
            findings.push(format!("required check {} is {}", check.name, check.result));
        }
    }
    if findings.is_empty() {
        ("READY".to_string(), findings)
    } else {
        ("BLOCKED".to_string(), findings)
    }
}

fn command_text(program: &str, args: &[&str]) -> Result<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::github::{CandidateFacts, RequiredContext};

    fn candidate() -> CandidateFacts {
        CandidateFacts {
            repository: "owner/repo".to_string(),
            pr: 7,
            state: "OPEN".to_string(),
            draft: false,
            head_ref: "feature".to_string(),
            head_sha: "head".to_string(),
            base_ref: "main".to_string(),
            base_sha: "base".to_string(),
            mergeability: "MERGEABLE".to_string(),
            merge_state: "CLEAN".to_string(),
            required_contexts: vec![RequiredContext {
                name: "required".to_string(),
                source: "branch_protection".to_string(),
            }],
            required_contexts_result: "SUCCESS".to_string(),
            identity_result: "current".to_string(),
        }
    }

    fn review(result: &str, converged: bool) -> ReviewSnapshot {
        ReviewSnapshot {
            repository: "owner/repo".to_string(),
            pr: 7,
            head_sha: "head".to_string(),
            result: result.to_string(),
            converged,
            submitted_reviews: Vec::new(),
            pending_reviewers: Vec::new(),
            unresolved_active: Vec::new(),
            unresolved_outdated: Vec::new(),
            resolved_without_disposition: Vec::new(),
            currentness_basis: Vec::new(),
            errors: Vec::new(),
        }
    }

    #[test]
    fn ready_requires_all_composed_authorities() {
        let checks = vec![RequiredCheckFact {
            name: "required".to_string(),
            result: "SUCCESS".to_string(),
            evaluated_head_sha: Some("head".to_string()),
        }];
        let (result, findings) =
            derive_preflight_result(&candidate(), &review("CURRENT", true), &checks, &[]);
        assert_eq!(result, "READY");
        assert!(findings.is_empty());
    }

    #[test]
    fn missing_or_skipped_required_check_blocks_without_false_ready() {
        let checks = vec![RequiredCheckFact {
            name: "required".to_string(),
            result: "MISSING".to_string(),
            evaluated_head_sha: None,
        }];
        let (result, findings) =
            derive_preflight_result(&candidate(), &review("CURRENT", true), &checks, &[]);
        assert_eq!(result, "BLOCKED");
        assert!(findings.iter().any(|finding| finding.contains("MISSING")));
    }

    #[test]
    fn partial_data_is_not_proven() {
        let (result, findings) = derive_preflight_result(
            &candidate(),
            &review("CURRENT", true),
            &[],
            &["rate limit".to_string()],
        );
        assert_eq!(result, "NOT_PROVEN");
        assert!(!findings.is_empty());
    }

    #[test]
    fn check_classification_preserves_stale_and_incomplete_states() {
        let stale = CheckRun {
            name: "required".to_string(),
            status: "COMPLETED".to_string(),
            conclusion: Some("SUCCESS".to_string()),
            head_sha: Some("old".to_string()),
        };
        let pending = CheckRun {
            name: "required".to_string(),
            status: "IN_PROGRESS".to_string(),
            conclusion: None,
            head_sha: Some("head".to_string()),
        };
        assert_eq!(classify_check(&stale, "head"), "STALE");
        assert_eq!(classify_check(&pending, "head"), "PENDING");
    }

    #[test]
    fn legacy_status_contexts_are_classified_without_false_missing() {
        let status = CommitStatus { context: "required".to_string(), state: "success".to_string() };
        assert_eq!(classify_status(&status), "SUCCESS");
    }
}
