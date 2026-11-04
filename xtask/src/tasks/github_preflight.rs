//! Read-only protected-merge preflight composed from the factual GitHub slices.

use super::github::{self, CandidateFacts, RequiredContext};
use super::github_review::{self, ReviewSnapshot};
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::BTreeSet;

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
    id: u64,
    name: String,
    status: String,
    conclusion: Option<String>,
    head_sha: Option<String>,
    started_at: Option<String>,
    app: Option<CheckRunApp>,
}

#[derive(Debug, Deserialize)]
struct CheckRunApp {
    id: u64,
}

pub fn run_preflight(pr: u64, json_only: bool) -> Result<()> {
    let mut errors = Vec::new();
    let initial_candidate = match github::candidate_facts(pr) {
        Ok(candidate) => Some(candidate),
        Err(error) => {
            errors.push(format!("failed to collect initial candidate facts: {error}"));
            None
        }
    };
    let initial_repository = initial_candidate
        .as_ref()
        .map(|candidate| candidate.repository.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let review = match github_review::review_snapshot(pr) {
        Ok(review) => review,
        Err(error) => not_proven_review(&initial_repository, pr, error.to_string()),
    };
    let repository = if initial_repository == "unknown" {
        review.repository.clone()
    } else {
        initial_repository
    };
    let mut candidate_identity_error = false;
    let mut candidate_refresh_error = false;
    let mut candidate = match initial_candidate.as_ref() {
        Some(initial) => match github::candidate_facts(pr) {
            Ok(candidate) => {
                if initial.head_sha != candidate.head_sha {
                    candidate_identity_error = true;
                    errors.push(
                        "candidate head moved while composing the preflight snapshot".to_string(),
                    );
                }
                candidate
            }
            Err(error) => {
                candidate_refresh_error = true;
                errors.push(format!("failed to collect candidate facts: {error}"));
                not_proven_candidate(&repository, pr)
            }
        },
        None => not_proven_candidate(&repository, pr),
    };
    errors.extend(review.errors.clone());
    if review.head_sha != candidate.head_sha {
        errors.push("candidate and review snapshots describe different heads".to_string());
    }
    let required_checks = if candidate.head_sha.is_empty() {
        Vec::new()
    } else {
        match collect_required_checks(&candidate) {
            Ok(checks) => checks,
            Err(error) => {
                errors.push(error.to_string());
                Vec::new()
            }
        }
    };
    if initial_candidate.is_some() {
        match github::candidate_facts(pr) {
            Ok(final_candidate) => {
                if candidate_refresh_error {
                    candidate = final_candidate;
                    candidate.identity_result = "NOT_PROVEN".to_string();
                    errors.push(
                        "candidate facts were unavailable during snapshot composition; head stability is NOT_PROVEN"
                            .to_string(),
                    );
                } else if candidate.head_sha.is_empty() {
                    candidate.identity_result = "NOT_PROVEN".to_string();
                    errors.push(
                        "candidate facts were incomplete during snapshot composition; head stability is NOT_PROVEN"
                            .to_string(),
                    );
                } else {
                    let head_changed = final_candidate.head_sha != candidate.head_sha;
                    let integration_changed =
                        !candidate_integration_facts_match(&final_candidate, &candidate);
                    if head_changed {
                        errors.push("candidate head moved before preflight completion".to_string());
                    } else if integration_changed && !candidate_identity_error {
                        errors.push(
                            "candidate base, merge, or required-policy facts changed before preflight completion"
                                .to_string(),
                        );
                    }
                    if head_changed || integration_changed || candidate_identity_error {
                        candidate.identity_result = "NOT_PROVEN".to_string();
                    } else {
                        candidate.identity_result = "current".to_string();
                    }
                }
            }
            Err(error) => {
                candidate.identity_result = "NOT_PROVEN".to_string();
                errors.push(format!("failed to revalidate candidate head: {error}"));
            }
        }
    }
    candidate.required_contexts_result = summarize_required_checks(&required_checks, &errors);
    let protected_merge = ProtectedMergeFacts {
        base_ref: candidate.base_ref.clone(),
        policy_source: candidate
            .required_contexts
            .iter()
            .map(|context| context.source.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join("+"),
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

fn candidate_integration_facts_match(left: &CandidateFacts, right: &CandidateFacts) -> bool {
    left.state == right.state
        && left.draft == right.draft
        && left.head_sha == right.head_sha
        && left.base_ref == right.base_ref
        && left.base_sha == right.base_sha
        && left.mergeability == right.mergeability
        && left.merge_state == right.merge_state
        && left.required_contexts == right.required_contexts
}

fn not_proven_candidate(repository: &str, pr: u64) -> CandidateFacts {
    CandidateFacts {
        repository: repository.to_string(),
        pr,
        state: "UNKNOWN".to_string(),
        draft: false,
        head_ref: String::new(),
        head_sha: String::new(),
        base_ref: String::new(),
        base_sha: String::new(),
        mergeability: "UNKNOWN".to_string(),
        merge_state: "UNKNOWN".to_string(),
        required_contexts: Vec::new(),
        required_contexts_result: "NOT_PROVEN".to_string(),
        identity_result: "NOT_PROVEN".to_string(),
    }
}

fn not_proven_review(repository: &str, pr: u64, error: String) -> ReviewSnapshot {
    ReviewSnapshot {
        repository: repository.to_string(),
        pr,
        head_sha: String::new(),
        result: "NOT_PROVEN".to_string(),
        converged: false,
        submitted_reviews: Vec::new(),
        pending_reviewers: Vec::new(),
        unresolved_active: Vec::new(),
        unresolved_outdated: Vec::new(),
        resolved_without_disposition: Vec::new(),
        currentness_basis: vec!["review facts unavailable".to_string()],
        errors: vec![error],
    }
}

fn collect_required_checks(candidate: &CandidateFacts) -> Result<Vec<RequiredCheckFact>> {
    let all_check_runs = paginate_github_items(
        &candidate.repository,
        &candidate.head_sha,
        "check-runs",
        "check runs",
        |payload: CheckRunsPayload| payload.check_runs,
    )?;
    let all_statuses = paginate_github_items(
        &candidate.repository,
        &candidate.head_sha,
        "status",
        "commit statuses",
        |payload: CommitStatusPayload| payload.statuses,
    )?;
    Ok(candidate
        .required_contexts
        .iter()
        .map(|required| {
            resolve_required_check(required, &all_check_runs, &all_statuses, &candidate.head_sha)
        })
        .collect())
}

fn paginate_github_items<T, P, F>(
    repository: &str,
    head_sha: &str,
    endpoint_kind: &str,
    label: &str,
    extract: F,
) -> Result<Vec<T>>
where
    P: DeserializeOwned,
    F: Fn(P) -> Vec<T>,
{
    let mut items = Vec::new();
    let mut page = 1;
    loop {
        let endpoint = format!(
            "repos/{repository}/commits/{head_sha}/{endpoint_kind}?per_page=100&page={page}"
        );
        let raw = github::command_text("gh", &["api", &endpoint])?;
        let payload: P = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {label} for the captured candidate head"))?;
        let page_items = extract(payload);
        let count = page_items.len();
        items.extend(page_items);
        if count < 100 {
            return Ok(items);
        }
        if page >= 100 {
            bail!("{label} pagination exceeded the 100-page safety cap");
        }
        page += 1;
    }
}

fn resolve_required_check(
    required: &RequiredContext,
    check_runs: &[CheckRun],
    statuses: &[CommitStatus],
    candidate_head_sha: &str,
) -> RequiredCheckFact {
    let matching = latest_check_run(check_runs, required);
    let status = required
        .app_id
        .is_none()
        .then(|| statuses.iter().find(|status| status.context == required.name))
        .flatten();
    let run_result = matching.map(|run| classify_check(run, candidate_head_sha));
    let status_result = status.map(classify_status);
    let result = match (run_result.as_deref(), status_result.as_deref()) {
        (Some(run), Some(status)) => combine_required_results(run, status),
        (Some(run), None) => run.to_string(),
        (None, Some(status)) => status.to_string(),
        (None, None) => "MISSING".to_string(),
    };
    RequiredCheckFact {
        name: required.name.clone(),
        result,
        evaluated_head_sha: matching
            .and_then(|run| run.head_sha.clone())
            .or_else(|| status.map(|_| candidate_head_sha.to_string())),
    }
}

fn combine_required_results(run: &str, status: &str) -> String {
    for result in ["INSTRUMENT_FAILURE", "STALE", "FAILURE", "CANCELLED", "PENDING", "SKIPPED"] {
        if run == result || status == result {
            return result.to_string();
        }
    }
    if run == "SUCCESS" && status == "SUCCESS" {
        "SUCCESS".to_string()
    } else {
        "INSTRUMENT_FAILURE".to_string()
    }
}

fn latest_check_run<'a>(runs: &'a [CheckRun], required: &RequiredContext) -> Option<&'a CheckRun> {
    runs.iter()
        .filter(|run| run.name == required.name)
        .filter(|run| {
            required
                .app_id
                .is_none_or(|app_id| run.app.as_ref().is_some_and(|app| app.id == app_id))
        })
        .max_by(|left, right| {
            left.started_at
                .as_deref()
                .unwrap_or("")
                .cmp(right.started_at.as_deref().unwrap_or(""))
                .then_with(|| left.id.cmp(&right.id))
        })
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
        // Keep SKIPPED distinct. A required check that GitHub reports as
        // skipped remains blocked here; only an explicitly successful check
        // is sufficient for this factual preflight.
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
    if !checks.is_empty() && checks.iter().all(|check| check.result == "SUCCESS") {
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
    if candidate.state != "OPEN" {
        findings.push(format!("candidate state is {}", candidate.state));
    }
    if candidate.draft {
        findings.push("candidate is a draft".to_string());
    }
    if candidate.mergeability == "UNKNOWN" || candidate.merge_state == "UNKNOWN" {
        findings.push(format!(
            "native merge state is NOT_PROVEN: {} / {}",
            candidate.mergeability, candidate.merge_state
        ));
        return ("NOT_PROVEN".to_string(), findings);
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
    if checks.is_empty() {
        findings.push("no required checks were discovered".to_string());
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
                app_id: None,
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
        assert!(findings.is_empty(), "ready preflight must have no findings");
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
        assert!(
            findings.iter().any(|finding| finding.contains("MISSING")),
            "missing required checks must be reported"
        );
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
        assert!(!findings.is_empty(), "partial data must produce a finding");
    }

    #[test]
    fn check_classification_preserves_stale_and_incomplete_states() {
        let stale = CheckRun {
            id: 1,
            name: "required".to_string(),
            status: "COMPLETED".to_string(),
            conclusion: Some("SUCCESS".to_string()),
            head_sha: Some("old".to_string()),
            started_at: Some("2026-08-01T00:00:00Z".to_string()),
            app: None,
        };
        let pending = CheckRun {
            id: 2,
            name: "required".to_string(),
            status: "IN_PROGRESS".to_string(),
            conclusion: None,
            head_sha: Some("head".to_string()),
            started_at: Some("2026-08-02T00:00:00Z".to_string()),
            app: None,
        };
        assert_eq!(classify_check(&stale, "head"), "STALE");
        assert_eq!(classify_check(&pending, "head"), "PENDING");
    }

    #[test]
    fn legacy_status_contexts_are_classified_without_false_missing() {
        let status = CommitStatus { context: "required".to_string(), state: "success".to_string() };
        assert_eq!(classify_status(&status), "SUCCESS");
    }

    #[test]
    fn latest_check_attempt_controls_required_context_result() {
        let runs = vec![
            CheckRun {
                id: 10,
                name: "required".to_string(),
                status: "COMPLETED".to_string(),
                conclusion: Some("SUCCESS".to_string()),
                head_sha: Some("head".to_string()),
                started_at: Some("2026-08-02T00:00:00Z".to_string()),
                app: None,
            },
            CheckRun {
                id: 11,
                name: "required".to_string(),
                status: "COMPLETED".to_string(),
                conclusion: Some("FAILURE".to_string()),
                head_sha: Some("head".to_string()),
                started_at: Some("2026-08-02T00:01:00Z".to_string()),
                app: None,
            },
        ];
        let required = RequiredContext {
            name: "required".to_string(),
            source: "checks".to_string(),
            app_id: None,
        };
        let fact = resolve_required_check(&required, &runs, &[], "head");
        assert_eq!(fact.result, "FAILURE", "latest check attempt must control the verdict");
    }

    #[test]
    fn empty_required_check_set_is_blocked() {
        assert_eq!(summarize_required_checks(&[], &[]), "BLOCKED");
        let (result, findings) =
            derive_preflight_result(&candidate(), &review("CURRENT", true), &[], &[]);
        assert_eq!(result, "BLOCKED");
        assert!(findings.iter().any(|finding| finding.contains("no required checks")));
    }

    #[test]
    fn required_app_identity_selects_only_the_matching_check_run() {
        let runs = vec![
            CheckRun {
                id: 1,
                name: "required".to_string(),
                status: "COMPLETED".to_string(),
                conclusion: Some("SUCCESS".to_string()),
                head_sha: Some("head".to_string()),
                started_at: Some("2026-08-02T00:00:00Z".to_string()),
                app: Some(CheckRunApp { id: 1 }),
            },
            CheckRun {
                id: 2,
                name: "required".to_string(),
                status: "COMPLETED".to_string(),
                conclusion: Some("FAILURE".to_string()),
                head_sha: Some("head".to_string()),
                started_at: Some("2026-08-02T00:01:00Z".to_string()),
                app: Some(CheckRunApp { id: 2 }),
            },
        ];
        let required = RequiredContext {
            name: "required".to_string(),
            source: "ruleset".to_string(),
            app_id: Some(1),
        };
        assert_eq!(resolve_required_check(&required, &runs, &[], "head").result, "SUCCESS");
    }

    #[test]
    fn app_neutral_required_context_requires_both_check_sources() {
        let run = CheckRun {
            id: 1,
            name: "required".to_string(),
            status: "COMPLETED".to_string(),
            conclusion: Some("SUCCESS".to_string()),
            head_sha: Some("head".to_string()),
            started_at: Some("2026-08-02T00:00:00Z".to_string()),
            app: None,
        };
        let status = CommitStatus { context: "required".to_string(), state: "failure".to_string() };
        let required = RequiredContext {
            name: "required".to_string(),
            source: "branch_protection".to_string(),
            app_id: None,
        };
        assert_eq!(resolve_required_check(&required, &[run], &[status], "head").result, "FAILURE");
    }

    #[test]
    fn unknown_mergeability_is_not_proven() {
        let mut candidate = candidate();
        candidate.mergeability = "UNKNOWN".to_string();
        let (result, findings) = derive_preflight_result(
            &candidate,
            &review("CURRENT", true),
            &[RequiredCheckFact {
                name: "required".to_string(),
                result: "SUCCESS".to_string(),
                evaluated_head_sha: Some("head".to_string()),
            }],
            &[],
        );
        assert_eq!(result, "NOT_PROVEN");
        assert!(findings.iter().any(|finding| finding.contains("NOT_PROVEN")));
    }
}
