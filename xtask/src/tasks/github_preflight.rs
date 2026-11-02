//! Read-only protected-merge preflight composed from the factual GitHub slices.

use super::github::{self, CandidateFacts, RequiredContext};
use super::github_review::{self, ReviewSnapshot};
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, BTreeSet};

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

#[derive(Debug, Clone, Deserialize)]
struct CheckRunsPayload {
    #[serde(default)]
    total_count: u64,
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

#[derive(Debug, Clone, Deserialize)]
struct CheckRun {
    id: u64,
    name: String,
    status: String,
    conclusion: Option<String>,
    head_sha: Option<String>,
    started_at: Option<String>,
    app: Option<CheckRunApp>,
}

#[derive(Debug, Clone, Deserialize)]
struct CheckRunApp {
    id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeadRequiredCheck {
    pub name: String,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryFailure {
    pub name: String,
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeadGateSnapshot {
    pub schema_version: &'static str,
    pub repository: String,
    pub branch: Option<String>,
    pub policy_base_ref: String,
    pub head_sha: String,
    pub policy_source: String,
    pub check_run_total_count: u64,
    pub check_run_pages_read: u32,
    pub required_checks: Vec<HeadRequiredCheck>,
    pub advisory_failures: Vec<AdvisoryFailure>,
    pub overall: String,
}

const KNOWN_ADVISORY_CHECKS: &[&str] = &[
    "CI Gate shard (policy)",
    "CI Gate shard (meta)",
    "Public API Surface (facade PR)",
    "PR Smoke (Fast Feedback, advisory)",
    "Trivy",
    "UX Regression Tests",
];

const PR_SCOPED_REQUIRED_CONTEXTS: &[&str] = &["validate-title"];

pub fn run_preflight_mode(
    pr: Option<u64>,
    sha: Option<String>,
    branch: Option<String>,
    json_only: bool,
) -> Result<()> {
    match (pr, sha, branch) {
        (Some(pr), None, None) => run_preflight(pr, json_only),
        (None, Some(sha), None) => run_head_gate(None, sha, "main", json_only),
        (None, None, Some(branch)) => {
            let repository = github::repository_name_with_owner()?;
            let head_sha = github::resolve_git_ref_head_sha(&repository, &branch)?;
            run_head_gate(Some(branch.clone()), head_sha, &branch, json_only)
        }
        _ => bail!("exactly one of --pr, --sha, or --branch is required"),
    }
}

pub fn run_head_gate(
    branch: Option<String>,
    head_sha: String,
    policy_base_ref: &str,
    json_only: bool,
) -> Result<()> {
    let repository = github::repository_name_with_owner()?;
    let required_contexts = github::required_contexts_for_base(&repository, policy_base_ref)
        .with_context(|| {
            format!("failed to discover required contexts for base ref {policy_base_ref}")
        })?;
    let policy_source = summarize_policy_source(&required_contexts);
    let (all_check_runs, check_run_total_count, check_run_pages_read) =
        paginate_check_runs(&repository, &head_sha)?;
    let all_statuses = paginate_github_items(
        &repository,
        &head_sha,
        "status",
        "commit statuses",
        |payload: CommitStatusPayload| payload.statuses,
    )?;
    let required_checks = required_contexts
        .iter()
        .map(|required| {
            let fact = resolve_required_check(required, &all_check_runs, &all_statuses, &head_sha);
            head_required_check_from_fact(&fact, true)
        })
        .collect::<Vec<_>>();
    let required_names =
        required_contexts.iter().map(|context| context.name.as_str()).collect::<BTreeSet<_>>();
    let advisory_failures = collect_advisory_failures(&all_check_runs, &head_sha, &required_names);
    let overall = derive_head_gate_overall(&required_checks, &[]);
    let snapshot = HeadGateSnapshot {
        schema_version: "github-head-gate.v1",
        repository,
        branch,
        policy_base_ref: policy_base_ref.to_string(),
        head_sha,
        policy_source,
        check_run_total_count,
        check_run_pages_read,
        required_checks,
        advisory_failures,
        overall: overall.clone(),
    };
    if !json_only {
        print_head_gate_human(&snapshot);
    }
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    match overall.as_str() {
        "PASS" => Ok(()),
        "IN_PROGRESS" => bail!("main-head REQUIRED-ONLY gate is IN_PROGRESS"),
        "FAIL" => bail!("main-head REQUIRED-ONLY gate is FAIL"),
        _ => bail!("main-head REQUIRED-ONLY gate is NOT_PROVEN"),
    }
}

fn summarize_policy_source(required_contexts: &[RequiredContext]) -> String {
    required_contexts
        .iter()
        .map(|context| context.source.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("+")
}

fn head_required_check_from_fact(fact: &RequiredCheckFact, branch_head: bool) -> HeadRequiredCheck {
    let note = if branch_head
        && fact.result == "MISSING"
        && PR_SCOPED_REQUIRED_CONTEXTS.contains(&fact.name.as_str())
    {
        Some("PR-scoped; absent on push heads is expected".to_string())
    } else {
        None
    };
    HeadRequiredCheck { name: fact.name.clone(), result: fact.result.clone(), note }
}

fn collect_advisory_failures(
    check_runs: &[CheckRun],
    head_sha: &str,
    required_names: &BTreeSet<&str>,
) -> Vec<AdvisoryFailure> {
    let mut failures = BTreeMap::<String, String>::new();
    for run in check_runs {
        if required_names.contains(run.name.as_str()) {
            continue;
        }
        if !is_known_advisory_check(run.name.as_str()) {
            continue;
        }
        let result = classify_check(run, head_sha);
        if matches!(result.as_str(), "FAILURE" | "CANCELLED" | "INSTRUMENT_FAILURE") {
            failures.insert(run.name.clone(), result);
        }
    }
    failures.into_iter().map(|(name, result)| AdvisoryFailure { name, result }).collect()
}

fn is_known_advisory_check(name: &str) -> bool {
    KNOWN_ADVISORY_CHECKS.contains(&name)
}

fn derive_head_gate_overall(checks: &[HeadRequiredCheck], errors: &[String]) -> String {
    if !errors.is_empty() {
        return "NOT_PROVEN".to_string();
    }
    if checks.is_empty() {
        return "NOT_PROVEN".to_string();
    }
    if checks.iter().any(|check| {
        matches!(
            check.result.as_str(),
            "FAILURE" | "CANCELLED" | "SKIPPED" | "STALE" | "INSTRUMENT_FAILURE"
        )
    }) {
        return "FAIL".to_string();
    }
    if checks.iter().any(|check| check.result == "PENDING") {
        return "IN_PROGRESS".to_string();
    }
    if checks.iter().all(|check| check.result == "SUCCESS") {
        "PASS".to_string()
    } else if checks.iter().any(|check| check.result == "MISSING") {
        "FAIL".to_string()
    } else {
        "NOT_PROVEN".to_string()
    }
}

fn print_head_gate_human(snapshot: &HeadGateSnapshot) {
    println!(
        "main-head REQUIRED-ONLY gate: {} (head {}, check-runs total_count={}, pages={})",
        snapshot.overall,
        snapshot.head_sha,
        snapshot.check_run_total_count,
        snapshot.check_run_pages_read
    );
    println!("  policy: {} (base ref {})", snapshot.policy_source, snapshot.policy_base_ref);
    println!("  required:");
    for check in &snapshot.required_checks {
        if let Some(note) = &check.note {
            println!("    {}: {} ({note})", check.name, check.result);
        } else {
            println!("    {}: {}", check.name, check.result);
        }
    }
    if snapshot.advisory_failures.is_empty() {
        println!("  advisory (does-not-block): none");
    } else {
        println!("  advisory (does-not-block):");
        for failure in &snapshot.advisory_failures {
            println!("    {}: {}", failure.name, failure.result);
        }
    }
}

pub(crate) fn merge_check_run_pages(pages: &[CheckRunsPayload]) -> (Vec<CheckRun>, u64, u32) {
    let check_run_total_count = pages.first().map(|page| page.total_count).unwrap_or(0);
    let check_run_pages_read = u32::try_from(pages.len()).unwrap_or(u32::MAX);
    let check_runs = pages.iter().flat_map(|page| page.check_runs.clone()).collect();
    (check_runs, check_run_total_count, check_run_pages_read)
}

pub(crate) fn merge_check_run_json_pages(page_json: &[&str]) -> Result<(Vec<CheckRun>, u64, u32)> {
    let pages = page_json
        .iter()
        .map(|raw| serde_json::from_str::<CheckRunsPayload>(raw))
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse recorded check-run fixture page")?;
    Ok(merge_check_run_pages(&pages))
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
    let (all_check_runs, _, _) = paginate_check_runs(&candidate.repository, &candidate.head_sha)?;
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

fn paginate_check_runs(repository: &str, head_sha: &str) -> Result<(Vec<CheckRun>, u64, u32)> {
    let mut items = Vec::new();
    let mut total_count = 0u64;
    let mut page = 1u32;
    loop {
        let endpoint =
            format!("repos/{repository}/commits/{head_sha}/check-runs?per_page=100&page={page}");
        let raw = github::command_text("gh", &["api", &endpoint])?;
        let payload: CheckRunsPayload = serde_json::from_str(&raw)
            .context("failed to parse check runs for the captured head")?;
        if page == 1 {
            total_count = payload.total_count;
        }
        let count = payload.check_runs.len();
        items.extend(payload.check_runs);
        if count < 100 {
            return Ok((items, total_count, page));
        }
        if page >= 100 {
            bail!("check runs pagination exceeded the 100-page safety cap");
        }
        page += 1;
    }
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

    fn sample_check_run(id: u64, name: &str, head: &str, conclusion: Option<&str>) -> CheckRun {
        CheckRun {
            id,
            name: name.to_string(),
            status: "COMPLETED".to_string(),
            conclusion: conclusion.map(str::to_string),
            head_sha: Some(head.to_string()),
            started_at: Some(format!("2026-08-02T00:{id:02}:00Z")),
            app: None,
        }
    }

    #[test]
    fn required_join_must_read_beyond_the_first_check_run_page() {
        let head = "6823f7f69";
        let required_name = "Perl LSP Rust Small Result";
        let page1 = CheckRunsPayload {
            total_count: 101,
            check_runs: (0..100)
                .map(|id| sample_check_run(id, &format!("filler-{id}"), head, Some("SUCCESS")))
                .collect(),
        };
        let page2 = CheckRunsPayload {
            total_count: 101,
            check_runs: vec![sample_check_run(1000, required_name, head, Some("FAILURE"))],
        };
        let (page1_only, _, _) = merge_check_run_pages(std::slice::from_ref(&page1));
        let (all_pages, total_count, pages_read) = merge_check_run_pages(&[page1, page2]);
        assert_eq!(total_count, 101);
        assert_eq!(pages_read, 2);
        let required = RequiredContext {
            name: required_name.to_string(),
            source: "ruleset".to_string(),
            app_id: None,
        };
        assert_eq!(
            resolve_required_check(&required, &page1_only, &[], head).result,
            "MISSING",
            "first-page-only reads must not see failures past page 1"
        );
        assert_eq!(
            resolve_required_check(&required, &all_pages, &[], head).result,
            "FAILURE",
            "pagination-safe reads must join all pages before classifying required contexts"
        );
    }

    #[test]
    fn recorded_fixture_pages_merge_with_declared_total_count() -> Result<()> {
        let (runs, total_count, pages_read) = merge_check_run_json_pages(&[
            include_str!("../../tests/fixtures/github/check-runs-page1-17098.json"),
            include_str!("../../tests/fixtures/github/check-runs-page2-17098.json"),
        ])?;
        assert_eq!(total_count, 101);
        assert_eq!(pages_read, 2);
        assert_eq!(runs.len(), 101);
        Ok(())
    }

    #[test]
    fn advisory_reds_do_not_flip_required_only_pass() {
        let head = "head";
        let required = HeadRequiredCheck {
            name: "Perl LSP Rust Small Result".to_string(),
            result: "SUCCESS".to_string(),
            note: None,
        };
        let runs = vec![
            sample_check_run(1, "Perl LSP Rust Small Result", head, Some("SUCCESS")),
            sample_check_run(2, "CI Gate shard (policy)", head, Some("FAILURE")),
        ];
        let required_names = ["Perl LSP Rust Small Result"].into_iter().collect();
        let advisory = collect_advisory_failures(&runs, head, &required_names);
        assert_eq!(advisory.len(), 1);
        assert_eq!(derive_head_gate_overall(&[required], &[]), "PASS");
    }

    #[test]
    fn validate_title_missing_on_branch_heads_is_annotated() {
        let fact = RequiredCheckFact {
            name: "validate-title".to_string(),
            result: "MISSING".to_string(),
            evaluated_head_sha: None,
        };
        let annotated = head_required_check_from_fact(&fact, true);
        assert_eq!(annotated.result, "MISSING");
        assert!(annotated.note.as_deref().is_some_and(|note| note.contains("PR-scoped")));
    }
}
