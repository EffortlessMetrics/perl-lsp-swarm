//! Validate and optionally reconcile declared pull-request candidate sets with live GitHub state.
//!
//! The checked-in policy records explicit dispositions for known same-claim clusters.
//! Live mode verifies that every open cross-referenced PR is represented; it does not
//! mutate, close, retarget, merge, or rebase any pull request.

#![allow(clippy::print_stderr, clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Parser)]
#[command(name = "pr-candidate-set")]
#[command(about = "Validate same-claim PR candidate dispositions")]
struct Args {
    /// Candidate-set policy file.
    #[arg(long, default_value = "policy/pr-candidate-sets.toml")]
    policy: PathBuf,

    /// Override the repository declared by the policy.
    #[arg(long)]
    repository: Option<String>,

    /// Compare each claim with live GitHub issue cross-references through `gh api`.
    #[arg(long)]
    live: bool,

    /// Optional JSON receipt path.
    #[arg(long)]
    receipt: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ClaimState {
    Reconciled,
    Blocked,
    NotProven,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Relationship {
    CurrentCandidate,
    ExplicitStack,
    DistinctSlice,
    SalvageSource,
    Superseded,
    Duplicate,
    Abandoned,
    NotProven,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Candidate {
    pr: u64,
    relationship: Relationship,
    #[serde(default)]
    target_pr: Option<u64>,
    #[serde(default)]
    unique_delta: Vec<String>,
    #[serde(default)]
    open_findings: Vec<String>,
    disposition: String,
    observation: CandidateObservation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CandidateObservation {
    head_sha: String,
    /// Base branch recorded with the observation; live mode fails on drift.
    base_ref: String,
    state: String,
    observed_at: String,
    #[serde(default)]
    acceptance_evidence: Vec<String>,
    review_state: String,
}

/// An open cross-reference that is deliberately not enrolled as a candidate.
/// Membership evidence only; never auto-enrolment.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct ObservedCrossReference {
    pr: u64,
    reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Claim {
    issue: u64,
    claim_id: String,
    state: ClaimState,
    decision: String,
    #[serde(default)]
    required_harvest: Vec<String>,
    #[serde(default)]
    close_or_retarget: Vec<String>,
    #[serde(default)]
    candidates: Vec<Candidate>,
    #[serde(default)]
    observed: Vec<ObservedCrossReference>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Policy {
    schema_version: u32,
    repository: String,
    #[serde(default)]
    claim: Vec<Claim>,
}

#[derive(Clone, Debug, Serialize)]
struct Finding {
    level: &'static str,
    code: &'static str,
    claim_id: Option<String>,
    issue: Option<u64>,
    pr: Option<u64>,
    message: String,
}

#[derive(Clone, Debug, Serialize)]
struct LiveClaimState {
    issue: u64,
    declared_open_prs: Vec<u64>,
    discovered_open_prs: Vec<u64>,
    missing_from_policy: Vec<u64>,
    observed_open_heads: BTreeMap<u64, String>,
}

/// A live PR head observation bound to both commit identity and base branch.
#[derive(Clone, Debug)]
struct LiveHead {
    sha: String,
    base_ref: String,
}

/// Observation age beyond which live evidence is treated as drifted.
const OBSERVATION_MAX_AGE_SECONDS: i64 = 30 * 24 * 60 * 60;

/// Finding codes that make recorded dispositions untrustworthy for a claim.
const DRIFT_FINDING_CODES: [&str; 6] = [
    "STALE_HEAD_OBSERVATION",
    "STALE_BASE_OBSERVATION",
    "STALE_STATE_OBSERVATION",
    "STATE_OBSERVATION_STALE_CLOSED",
    "OBSERVATION_EXPIRED",
    "OBSERVATION_TIME_UNPARSEABLE",
];

#[derive(Debug, Serialize)]
struct Receipt {
    schema_version: &'static str,
    receipt_kind: &'static str,
    repository: String,
    policy_sha256: String,
    queried_issues: Vec<u64>,
    observed_at: String,
    live_checked: bool,
    passed: bool,
    claim_count: usize,
    candidate_count: usize,
    error_count: usize,
    warning_count: usize,
    claims: Vec<Claim>,
    live: Vec<LiveClaimState>,
    findings: Vec<Finding>,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let content = fs::read_to_string(&args.policy)
        .with_context(|| format!("reading {}", args.policy.display()))?;
    let mut policy: Policy =
        toml::from_str(&content).with_context(|| format!("parsing {}", args.policy.display()))?;
    if let Some(repository) = args.repository {
        policy.repository = repository;
    }

    let policy_sha256 =
        Sha256::digest(content.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
    let now = chrono::Utc::now();
    let observed_at = now.to_rfc3339();
    let mut findings = validate_policy(&policy);
    let live = if args.live { check_live_state(&policy, now, &mut findings) } else { Vec::new() };
    apply_drift_downgrades(&mut policy.claim, &findings);
    findings.sort_by(|left, right| {
        (left.level, &left.claim_id, left.issue, left.pr, left.code, &left.message).cmp(&(
            right.level,
            &right.claim_id,
            right.issue,
            right.pr,
            right.code,
            &right.message,
        ))
    });

    let error_count = findings.iter().filter(|finding| finding.level == "error").count();
    let warning_count = findings.iter().filter(|finding| finding.level == "warning").count();
    let candidate_count = policy.claim.iter().map(|claim| claim.candidates.len()).sum();
    let receipt = Receipt {
        schema_version: "pr_candidate_set.v2",
        receipt_kind: "pr_candidate_set",
        repository: policy.repository.clone(),
        policy_sha256,
        queried_issues: policy
            .claim
            .iter()
            .map(|claim| claim.issue)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        observed_at,
        live_checked: args.live,
        passed: error_count == 0,
        claim_count: policy.claim.len(),
        candidate_count,
        error_count,
        warning_count,
        claims: policy.claim,
        live,
        findings,
    };

    for finding in &receipt.findings {
        let command = if finding.level == "error" { "error" } else { "warning" };
        let subject = match (finding.issue, finding.pr) {
            (Some(issue), Some(pr)) => format!("issue #{issue}, PR #{pr}"),
            (Some(issue), None) => format!("issue #{issue}"),
            (None, Some(pr)) => format!("PR #{pr}"),
            (None, None) => "candidate-set policy".to_string(),
        };
        eprintln!("::{command}::[{}] {subject}: {}", finding.code, finding.message);
    }

    if let Some(receipt_path) = args.receipt {
        write_receipt(&receipt_path, &receipt)?;
        println!("Candidate-set receipt written: {}", receipt_path.display());
    }

    if !receipt.passed {
        bail!(
            "candidate-set validation failed with {} error(s) and {} warning(s)",
            receipt.error_count,
            receipt.warning_count
        );
    }

    println!(
        "Candidate-set validation passed ({} claim(s), {} PR(s), live={})",
        receipt.claim_count, receipt.candidate_count, receipt.live_checked
    );
    Ok(())
}

fn validate_policy(policy: &Policy) -> Vec<Finding> {
    let mut findings = Vec::new();
    if policy.schema_version != 2 {
        findings.push(global_error(
            "UNSUPPORTED_SCHEMA",
            format!("schema_version must be 2, got {}", policy.schema_version),
        ));
    }
    if policy.repository.split('/').count() != 2 {
        findings.push(global_error(
            "INVALID_REPOSITORY",
            format!("repository must be owner/name, got '{}'", policy.repository),
        ));
    }
    if policy.claim.is_empty() {
        findings.push(global_error(
            "EMPTY_POLICY",
            "at least one candidate-set claim is required".to_string(),
        ));
    }

    let mut claim_ids = BTreeSet::new();
    let mut global_prs = BTreeSet::new();
    for claim in &policy.claim {
        if claim.claim_id.trim().is_empty() || !claim_ids.insert(claim.claim_id.clone()) {
            findings.push(claim_error(
                claim,
                "INVALID_CLAIM_ID",
                "claim_id must be non-empty and unique".to_string(),
            ));
        }
        validate_claim(claim, &mut global_prs, &mut findings);
    }
    findings
}

fn validate_claim(claim: &Claim, global_prs: &mut BTreeSet<u64>, findings: &mut Vec<Finding>) {
    if claim.decision.trim().is_empty() {
        findings.push(claim_error(
            claim,
            "MISSING_DECISION",
            "decision must explain the current candidate-set conclusion".to_string(),
        ));
    }
    if claim.required_harvest.is_empty() {
        findings.push(claim_error(
            claim,
            "MISSING_HARVEST_PLAN",
            "required_harvest must preserve or explicitly reject unique value".to_string(),
        ));
    }
    if claim.close_or_retarget.is_empty() {
        findings.push(claim_error(
            claim,
            "MISSING_DISPOSITION_PLAN",
            "close_or_retarget must state the next GitHub transitions".to_string(),
        ));
    }
    if claim.candidates.is_empty() {
        findings.push(claim_error(
            claim,
            "EMPTY_CANDIDATE_SET",
            "candidate set must contain at least one PR".to_string(),
        ));
        return;
    }

    let current_count = claim
        .candidates
        .iter()
        .filter(|candidate| candidate.relationship == Relationship::CurrentCandidate)
        .count();
    match claim.state {
        ClaimState::Reconciled if current_count != 1 => findings.push(claim_error(
            claim,
            "RECONCILED_WITHOUT_ONE_CURRENT",
            format!(
                "reconciled claims require exactly one current candidate, found {current_count}"
            ),
        )),
        ClaimState::Blocked | ClaimState::NotProven if current_count != 0 => {
            findings.push(claim_error(
                claim,
                "UNSETTLED_CLAIM_HAS_CURRENT",
                "blocked/not_proven claims cannot silently designate a current implementation"
                    .to_string(),
            ));
        }
        _ => {}
    }

    let local_prs: BTreeSet<u64> = claim.candidates.iter().map(|candidate| candidate.pr).collect();
    if local_prs.len() != claim.candidates.len() {
        findings.push(claim_error(
            claim,
            "DUPLICATE_PR_IN_CLAIM",
            "a PR appears more than once in the same candidate set".to_string(),
        ));
    }

    for candidate in &claim.candidates {
        if !global_prs.insert(candidate.pr) {
            findings.push(candidate_error(
                claim,
                candidate,
                "PR_IN_MULTIPLE_CLAIMS",
                "one PR cannot be dispositioned under multiple semantic claims".to_string(),
            ));
        }
        if candidate.unique_delta.is_empty() {
            findings.push(candidate_error(
                claim,
                candidate,
                "MISSING_UNIQUE_DELTA",
                "unique_delta must record value or explicitly state that no unique value exists"
                    .to_string(),
            ));
        }
        if candidate.disposition.trim().is_empty() {
            findings.push(candidate_error(
                claim,
                candidate,
                "MISSING_CANDIDATE_DISPOSITION",
                "disposition must state what happens to this candidate".to_string(),
            ));
        }
        if candidate.observation.head_sha.len() != 40
            || !candidate.observation.head_sha.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            findings.push(candidate_error(
                claim,
                candidate,
                "INVALID_OBSERVED_HEAD",
                "observation.head_sha must be a full 40-character Git SHA".to_string(),
            ));
        }
        if candidate.observation.observed_at.trim().is_empty()
            || candidate.observation.acceptance_evidence.is_empty()
            || candidate.observation.review_state.trim().is_empty()
        {
            findings.push(candidate_error(
                claim,
                candidate,
                "INCOMPLETE_OBSERVATION",
                "observation must record time, acceptance evidence, and review state".to_string(),
            ));
        }
        if candidate.observation.base_ref.trim().is_empty() {
            findings.push(candidate_error(
                claim,
                candidate,
                "INVALID_OBSERVED_BASE",
                "observation.base_ref must record the base branch seen at observation time"
                    .to_string(),
            ));
        }
        validate_relationship(claim, candidate, &local_prs, findings);
    }

    validate_observed_entries(claim, global_prs, findings);
    if let Some(cycle) = find_relationship_cycle(claim) {
        let path = cycle.iter().map(|pr| format!("#{pr}")).collect::<Vec<_>>().join(" -> ");
        findings.push(claim_error(
            claim,
            "CYCLIC_RELATIONSHIP_GRAPH",
            format!("relationship targets form a cycle: {path}"),
        ));
    }
}

fn validate_observed_entries(
    claim: &Claim,
    global_candidate_prs: &BTreeSet<u64>,
    findings: &mut Vec<Finding>,
) {
    for observed in &claim.observed {
        if observed.reason.trim().is_empty() {
            findings.push(claim_error(
                claim,
                "INVALID_OBSERVED_ENTRY",
                format!(
                    "observed cross-reference #{} must record why it is not a candidate",
                    observed.pr
                ),
            ));
        }
        // Observations are membership evidence and may repeat across issues;
        // they only conflict when the PR is dispositioned as a candidate.
        if global_candidate_prs.contains(&observed.pr) {
            findings.push(claim_error(
                claim,
                "OBSERVED_PR_CONFLICT",
                format!(
                    "PR #{} cannot be both a dispositioned candidate and an observed cross-reference",
                    observed.pr
                ),
            ));
        }
    }
}

/// Follows explicit_stack/superseded/duplicate targets and returns the first
/// target cycle found, as the ordered PR path entering the cycle.
fn find_relationship_cycle(claim: &Claim) -> Option<Vec<u64>> {
    const UNVISITED: u8 = 0;
    const IN_STACK: u8 = 1;
    const DONE: u8 = 2;
    let local_prs: BTreeSet<u64> = claim.candidates.iter().map(|candidate| candidate.pr).collect();
    let edges: BTreeMap<u64, u64> = claim
        .candidates
        .iter()
        .filter(|candidate| {
            matches!(
                candidate.relationship,
                Relationship::ExplicitStack | Relationship::Superseded | Relationship::Duplicate
            )
        })
        .filter_map(|candidate| {
            candidate
                .target_pr
                .filter(|target| *target != candidate.pr && local_prs.contains(target))
                .map(|target| (candidate.pr, target))
        })
        .collect();
    let mut state: BTreeMap<u64, u8> = BTreeMap::new();
    for start in edges.keys().copied().collect::<BTreeSet<_>>() {
        if state.get(&start).copied().unwrap_or(UNVISITED) == DONE {
            continue;
        }
        let mut path: Vec<u64> = Vec::new();
        let mut node = start;
        loop {
            match state.get(&node).copied().unwrap_or(UNVISITED) {
                IN_STACK => {
                    let offset = path.iter().position(|pr| *pr == node)?;
                    return Some(path[offset..].to_vec());
                }
                DONE => break,
                _ => {}
            }
            state.insert(node, IN_STACK);
            path.push(node);
            match edges.get(&node) {
                Some(next) => node = *next,
                None => break,
            }
        }
        for pr in path {
            state.insert(pr, DONE);
        }
    }
    None
}

fn validate_relationship(
    claim: &Claim,
    candidate: &Candidate,
    local_prs: &BTreeSet<u64>,
    findings: &mut Vec<Finding>,
) {
    let requires_target = matches!(
        candidate.relationship,
        Relationship::ExplicitStack | Relationship::Superseded | Relationship::Duplicate
    );
    if requires_target && candidate.target_pr.is_none() {
        findings.push(candidate_error(
            claim,
            candidate,
            "RELATIONSHIP_REQUIRES_TARGET",
            format!("{:?} requires target_pr", candidate.relationship),
        ));
    }
    if matches!(
        candidate.relationship,
        Relationship::CurrentCandidate
            | Relationship::DistinctSlice
            | Relationship::NotProven
            | Relationship::SalvageSource
            | Relationship::Abandoned
    ) && candidate.target_pr.is_some()
    {
        findings.push(candidate_error(
            claim,
            candidate,
            "RELATIONSHIP_FORBIDS_TARGET",
            format!("{:?} must not declare target_pr", candidate.relationship),
        ));
    }
    if let Some(target) = candidate.target_pr {
        if target == candidate.pr {
            findings.push(candidate_error(
                claim,
                candidate,
                "SELF_TARGET",
                "candidate cannot target itself".to_string(),
            ));
        } else if !local_prs.contains(&target) {
            findings.push(candidate_error(
                claim,
                candidate,
                "UNKNOWN_TARGET",
                format!("target PR #{target} is not present in this claim"),
            ));
        } else if let Some(target_candidate) =
            claim.candidates.iter().find(|other| other.pr == target)
        {
            validate_target_relationship(claim, candidate, target_candidate, findings);
        }
    }
}

/// Sink/order rules per relationship kind. Superseded chains are allowed and
/// bounded by cycle detection; duplicates must point at a retained candidate
/// and stacks must point at a mergeable parent.
fn validate_target_relationship(
    claim: &Claim,
    candidate: &Candidate,
    target: &Candidate,
    findings: &mut Vec<Finding>,
) {
    match candidate.relationship {
        Relationship::Duplicate => {
            if !matches!(
                target.relationship,
                Relationship::CurrentCandidate
                    | Relationship::DistinctSlice
                    | Relationship::SalvageSource
            ) {
                findings.push(candidate_error(
                    claim,
                    candidate,
                    "DUPLICATE_TARGET_INVALID",
                    format!(
                        "duplicate must target the retained candidate, not another {:?}",
                        target.relationship
                    ),
                ));
            }
        }
        Relationship::ExplicitStack => {
            if !matches!(
                target.relationship,
                Relationship::CurrentCandidate | Relationship::DistinctSlice
            ) {
                findings.push(candidate_error(
                    claim,
                    candidate,
                    "STACK_TARGET_INVALID",
                    format!(
                        "explicit_stack must target a mergeable current or distinct-slice parent, not {:?}",
                        target.relationship
                    ),
                ));
            }
        }
        _ => {}
    }
}

fn check_live_state(
    policy: &Policy,
    now: chrono::DateTime<chrono::Utc>,
    findings: &mut Vec<Finding>,
) -> Vec<LiveClaimState> {
    if let Err(error) = ensure_gh_available() {
        findings.push(global_error("LIVE_SOURCE_UNAVAILABLE", error.to_string()));
        return Vec::new();
    }
    let mut live = Vec::new();
    let issues: BTreeSet<u64> = policy.claim.iter().map(|claim| claim.issue).collect();
    for issue in issues {
        let discovered = match discover_open_cross_references(&policy.repository, issue) {
            Ok(discovered) => discovered,
            Err(error) => {
                findings.push(Finding {
                    level: "error",
                    code: "LIVE_SOURCE_UNAVAILABLE",
                    claim_id: None,
                    issue: Some(issue),
                    pr: None,
                    message: error.to_string(),
                });
                continue;
            }
        };
        let claims_for_issue: Vec<&Claim> =
            policy.claim.iter().filter(|claim| claim.issue == issue).collect();
        let candidates: Vec<&Candidate> =
            claims_for_issue.iter().flat_map(|claim| &claim.candidates).collect();
        let declared: BTreeSet<u64> = candidates
            .iter()
            .filter(|candidate| candidate.observation.state == "open")
            .map(|candidate| candidate.pr)
            .collect();
        let observed_prs: BTreeSet<u64> =
            claims_for_issue.iter().flat_map(|claim| claim.observed.iter().map(|o| o.pr)).collect();
        let discovered_prs: BTreeSet<u64> = discovered.keys().copied().collect();
        let missing_from_policy: Vec<u64> = discovered_prs
            .difference(&declared)
            .copied()
            .filter(|pr| !observed_prs.contains(pr))
            .collect();
        if !missing_from_policy.is_empty() {
            findings.push(Finding { level: "error", code: "UNASSIGNED_OPEN_CROSS_REFERENCE", claim_id: None, issue: Some(issue), pr: None, message: format!("open cross-references require an explicit policy-owned claim assignment or observed entry: {missing_from_policy:?}") });
        }
        for candidate in &candidates {
            match discovered.get(&candidate.pr) {
                Some(head) => {
                    if observation_head_is_stale(candidate, &head.sha) {
                        findings.push(Finding {
                            level: "error",
                            code: "STALE_HEAD_OBSERVATION",
                            claim_id: None,
                            issue: Some(issue),
                            pr: Some(candidate.pr),
                            message: format!(
                                "recorded head {} differs from live head {}",
                                candidate.observation.head_sha, head.sha
                            ),
                        });
                    }
                    if observation_base_is_stale(candidate, &head.base_ref) {
                        findings.push(Finding {
                            level: "error",
                            code: "STALE_BASE_OBSERVATION",
                            claim_id: None,
                            issue: Some(issue),
                            pr: Some(candidate.pr),
                            message: format!(
                                "recorded base {} differs from live base {}",
                                candidate.observation.base_ref, head.base_ref
                            ),
                        });
                    }
                    if candidate.observation.state != "open" {
                        findings.push(Finding {
                            level: "error",
                            code: "STATE_OBSERVATION_STALE_CLOSED",
                            claim_id: None,
                            issue: Some(issue),
                            pr: Some(candidate.pr),
                            message: format!(
                                "candidate is recorded '{}' but is an open cross-reference",
                                candidate.observation.state
                            ),
                        });
                    }
                }
                None => {
                    if candidate.observation.state == "open" {
                        findings.push(Finding {
                            level: "error",
                            code: "STALE_STATE_OBSERVATION",
                            claim_id: None,
                            issue: Some(issue),
                            pr: Some(candidate.pr),
                            message:
                                "candidate is recorded open but is not an open cross-reference"
                                    .to_string(),
                        });
                    }
                }
            }
            if let Some(code) = observation_time_finding(&candidate.observation.observed_at, now) {
                findings.push(Finding {
                    level: "error",
                    code,
                    claim_id: None,
                    issue: Some(issue),
                    pr: Some(candidate.pr),
                    message: format!(
                        "observation time '{}' does not provide current evidence (max age {}s)",
                        candidate.observation.observed_at, OBSERVATION_MAX_AGE_SECONDS
                    ),
                });
            }
        }
        for claim in &claims_for_issue {
            for observed in &claim.observed {
                if !discovered.contains_key(&observed.pr) {
                    findings.push(Finding {
                        level: "warning",
                        code: "OBSERVED_CROSS_REFERENCE_CLOSED",
                        claim_id: Some(claim.claim_id.clone()),
                        issue: Some(issue),
                        pr: Some(observed.pr),
                        message: "observed cross-reference is no longer open; prune the entry"
                            .to_string(),
                    });
                }
            }
        }
        live.push(LiveClaimState {
            issue,
            declared_open_prs: declared.into_iter().collect(),
            discovered_open_prs: discovered.keys().copied().collect(),
            missing_from_policy,
            observed_open_heads: discovered.into_iter().map(|(pr, head)| (pr, head.sha)).collect(),
        });
    }
    live
}

fn apply_drift_downgrades(claims: &mut [Claim], findings: &[Finding]) {
    for claim in claims {
        let drifted = findings.iter().any(|finding| {
            finding.level == "error"
                && DRIFT_FINDING_CODES.contains(&finding.code)
                && finding.claim_id.as_deref() == Some(claim.claim_id.as_str())
        });
        if drifted && claim.state != ClaimState::NotProven {
            claim.state = ClaimState::NotProven;
        }
    }
}

fn observation_head_is_stale(candidate: &Candidate, live_head: &str) -> bool {
    candidate.observation.head_sha != live_head
}

fn observation_base_is_stale(candidate: &Candidate, live_base: &str) -> bool {
    candidate.observation.base_ref != live_base
}

/// Returns the drift finding for an observation timestamp, if any.
fn observation_time_finding(
    observed_at: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<&'static str> {
    match chrono::DateTime::parse_from_rfc3339(observed_at) {
        Ok(parsed) => {
            let age = now.timestamp().saturating_sub(parsed.timestamp());
            (age > OBSERVATION_MAX_AGE_SECONDS).then_some("OBSERVATION_EXPIRED")
        }
        Err(_) => Some("OBSERVATION_TIME_UNPARSEABLE"),
    }
}

fn ensure_gh_available() -> Result<()> {
    let output = Command::new("gh")
        .arg("--version")
        .output()
        .context("starting gh for live candidate-set validation")?;
    if !output.status.success() {
        return Err(eyre!(
            "gh is unavailable or unhealthy: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn discover_open_cross_references(repository: &str, issue: u64) -> Result<BTreeMap<u64, LiveHead>> {
    let endpoint = format!("repos/{repository}/issues/{issue}/timeline?per_page=100");
    let output = Command::new("gh")
        .args([
            "api",
            "--paginate",
            "--slurp",
            "-H",
            "Accept: application/vnd.github+json",
            &endpoint,
        ])
        .output()
        .with_context(|| format!("reading live timeline for issue #{issue}"))?;
    if !output.status.success() {
        return Err(eyre!(
            "gh api failed for issue #{issue}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let pages: Value = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("decoding live timeline for issue #{issue}"))?;
    let mut prs = BTreeMap::new();
    let Some(page_list) = pages.as_array() else {
        return Err(eyre!("timeline response for issue #{issue} was not an array"));
    };
    for page in page_list {
        let Some(events) = page.as_array() else {
            continue;
        };
        for event in events {
            let source = event.get("source").and_then(|value| value.get("issue"));
            let Some(source_issue) = source else {
                continue;
            };
            if source_issue.get("pull_request").is_none()
                || source_issue.get("state").and_then(Value::as_str) != Some("open")
            {
                continue;
            }
            let same_repository = source_issue
                .get("repository_url")
                .and_then(Value::as_str)
                .is_some_and(|url| url.ends_with(&format!("/repos/{repository}")));
            if !same_repository {
                continue;
            }
            if let Some(number) = source_issue.get("number").and_then(Value::as_u64) {
                let endpoint = format!("repos/{repository}/pulls/{number}");
                let detail = Command::new("gh")
                    .args(["api", "-H", "Accept: application/vnd.github+json", &endpoint])
                    .output()
                    .with_context(|| format!("reading live PR #{number}"))?;
                if !detail.status.success() {
                    return Err(eyre!(
                        "gh api failed for PR #{number}: {}",
                        String::from_utf8_lossy(&detail.stderr).trim()
                    ));
                }
                let detail: Value = serde_json::from_slice(&detail.stdout)
                    .with_context(|| format!("decoding live PR #{number}"))?;
                let sha = detail.pointer("/head/sha").and_then(Value::as_str);
                let base_ref = detail.pointer("/base/ref").and_then(Value::as_str);
                if let (Some(sha), Some(base_ref)) = (sha, base_ref) {
                    prs.insert(
                        number,
                        LiveHead { sha: sha.to_string(), base_ref: base_ref.to_string() },
                    );
                }
            }
        }
    }
    Ok(prs)
}

fn global_error(code: &'static str, message: String) -> Finding {
    Finding { level: "error", code, claim_id: None, issue: None, pr: None, message }
}

fn claim_error(claim: &Claim, code: &'static str, message: String) -> Finding {
    Finding {
        level: "error",
        code,
        claim_id: Some(claim.claim_id.clone()),
        issue: Some(claim.issue),
        pr: None,
        message,
    }
}

fn candidate_error(
    claim: &Claim,
    candidate: &Candidate,
    code: &'static str,
    message: String,
) -> Finding {
    Finding {
        level: "error",
        code,
        claim_id: Some(claim.claim_id.clone()),
        issue: Some(claim.issue),
        pr: Some(candidate.pr),
        message,
    }
}

fn write_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let serialized = serde_json::to_string_pretty(receipt).context("serializing receipt")?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, format!("{serialized}\n"))
        .with_context(|| format!("writing {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(pr: u64, relationship: Relationship, target_pr: Option<u64>) -> Candidate {
        Candidate {
            pr,
            relationship,
            target_pr,
            unique_delta: vec!["reviewed delta".to_string()],
            open_findings: Vec::new(),
            disposition: "reviewed disposition".to_string(),
            observation: CandidateObservation {
                head_sha: format!("{pr:040x}"),
                state: "open".to_string(),
                observed_at: "2026-01-01T00:00:00Z".to_string(),
                base_ref: "main".to_string(),
                acceptance_evidence: vec!["reviewed proof".to_string()],
                review_state: "reviewed".to_string(),
            },
        }
    }

    fn claim(state: ClaimState, candidates: Vec<Candidate>) -> Claim {
        Claim {
            issue: 1,
            claim_id: "test.claim".to_string(),
            state,
            decision: "current decision".to_string(),
            required_harvest: vec!["harvest reviewed delta".to_string()],
            close_or_retarget: vec!["close superseded candidates".to_string()],
            candidates,
            observed: Vec::new(),
        }
    }

    fn policy(claim: Claim) -> Policy {
        Policy { schema_version: 2, repository: "owner/repo".to_string(), claim: vec![claim] }
    }

    #[test]
    fn reconciled_claim_requires_exactly_one_current_candidate() {
        let findings = validate_policy(&policy(claim(
            ClaimState::Reconciled,
            vec![candidate(10, Relationship::SalvageSource, None)],
        )));
        assert!(findings.iter().any(|finding| finding.code == "RECONCILED_WITHOUT_ONE_CURRENT"));
    }

    #[test]
    fn not_proven_claim_allows_no_current_candidate() {
        let findings = validate_policy(&policy(claim(
            ClaimState::NotProven,
            vec![candidate(10, Relationship::SalvageSource, None)],
        )));
        assert!(findings.is_empty());
    }

    #[test]
    fn duplicate_relationship_requires_a_known_target() {
        let findings = validate_policy(&policy(claim(
            ClaimState::NotProven,
            vec![candidate(10, Relationship::Duplicate, Some(99))],
        )));
        assert!(findings.iter().any(|finding| finding.code == "UNKNOWN_TARGET"));
    }

    #[test]
    fn reconciled_claim_with_current_and_superseded_target_is_valid() {
        let findings = validate_policy(&policy(claim(
            ClaimState::Reconciled,
            vec![
                candidate(10, Relationship::CurrentCandidate, None),
                candidate(11, Relationship::Superseded, Some(10)),
            ],
        )));
        assert!(findings.is_empty());
    }

    #[test]
    fn one_issue_can_contain_two_distinct_semantic_claims() {
        let mut second = claim(
            ClaimState::Reconciled,
            vec![candidate(20, Relationship::CurrentCandidate, None)],
        );
        second.claim_id = "test.second_slice".to_string();
        let policy = Policy {
            schema_version: 2,
            repository: "owner/repo".to_string(),
            claim: vec![
                claim(
                    ClaimState::Reconciled,
                    vec![candidate(10, Relationship::CurrentCandidate, None)],
                ),
                second,
            ],
        };
        assert!(validate_policy(&policy).is_empty());
    }

    #[test]
    fn one_pr_cannot_be_assigned_to_two_claims() {
        let mut second = claim(
            ClaimState::Reconciled,
            vec![candidate(10, Relationship::CurrentCandidate, None)],
        );
        second.claim_id = "test.second_slice".to_string();
        let mut policy = policy(claim(
            ClaimState::Reconciled,
            vec![candidate(10, Relationship::CurrentCandidate, None)],
        ));
        policy.claim.push(second);
        assert!(
            validate_policy(&policy).iter().any(|finding| finding.code == "PR_IN_MULTIPLE_CLAIMS")
        );
    }

    #[test]
    fn closed_superseded_candidate_is_valid_history() {
        let mut historical = candidate(11, Relationship::Superseded, Some(10));
        historical.observation.state = "closed".to_string();
        assert!(
            validate_policy(&policy(claim(
                ClaimState::Reconciled,
                vec![candidate(10, Relationship::CurrentCandidate, None), historical],
            )))
            .is_empty()
        );
    }

    #[test]
    fn changed_live_head_makes_the_observation_stale() {
        let candidate = candidate(10, Relationship::CurrentCandidate, None);
        assert!(observation_head_is_stale(&candidate, "ffffffffffffffffffffffffffffffffffffffff"));
        assert!(!observation_base_is_stale(&candidate, "main"));
        assert!(observation_base_is_stale(&candidate, "release"));
    }

    #[test]
    fn workflow_declares_pull_request_lifecycle_routes_and_live_step() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../.github/workflows/pr-candidate-set.yml");
        let content = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("reading workflow contract {}: {error}", path.display())
        });
        for route in [
            "opened",
            "closed",
            "reopened",
            "synchronize",
            "edited",
            "ready_for_review",
            "converted_to_draft",
        ] {
            assert!(
                content.contains(route),
                "workflow must declare pull_request lifecycle type '{route}'"
            );
        }
        assert!(content.contains("schedule:"), "scheduled reconciliation route required");
        assert!(content.contains("workflow_dispatch"), "manual dispatch route required");
        assert!(content.contains("--live"), "live validation step required on every route");
    }

    #[test]
    fn mutual_superseded_pair_is_rejected_as_cycle() {
        let findings = validate_policy(&policy(claim(
            ClaimState::Reconciled,
            vec![
                candidate(10, Relationship::Superseded, Some(11)),
                candidate(11, Relationship::Superseded, Some(10)),
            ],
        )));
        assert!(
            findings.iter().any(|finding| finding.code == "CYCLIC_RELATIONSHIP_GRAPH"),
            "mutual supersession must fail: {findings:?}"
        );
    }

    #[test]
    fn three_way_duplicate_cycle_is_rejected() {
        let findings = validate_policy(&policy(claim(
            ClaimState::NotProven,
            vec![
                candidate(10, Relationship::Duplicate, Some(11)),
                candidate(11, Relationship::Duplicate, Some(12)),
                candidate(12, Relationship::Duplicate, Some(10)),
            ],
        )));
        assert!(
            findings.iter().any(|finding| finding.code == "CYCLIC_RELATIONSHIP_GRAPH")
                && findings.iter().any(|finding| finding.code == "DUPLICATE_TARGET_INVALID")
        );
    }

    #[test]
    fn valid_stack_and_supersession_chains_are_accepted() {
        let findings = validate_policy(&policy(claim(
            ClaimState::Reconciled,
            vec![
                candidate(10, Relationship::CurrentCandidate, None),
                candidate(11, Relationship::ExplicitStack, Some(10)),
                candidate(12, Relationship::Superseded, Some(11)),
                candidate(13, Relationship::Duplicate, Some(10)),
            ],
        )));
        assert!(findings.is_empty(), "legal chains must pass: {findings:?}");
    }

    #[test]
    fn duplicate_targeting_duplicate_is_rejected() {
        let findings = validate_policy(&policy(claim(
            ClaimState::NotProven,
            vec![
                candidate(10, Relationship::SalvageSource, None),
                candidate(11, Relationship::Duplicate, Some(10)),
                candidate(12, Relationship::Duplicate, Some(11)),
            ],
        )));
        assert!(
            findings
                .iter()
                .any(|finding| finding.code == "DUPLICATE_TARGET_INVALID"
                    && finding.pr == Some(12)),
            "duplicate of a duplicate must fail: {findings:?}"
        );
    }

    #[test]
    fn stack_targeting_non_mergeable_parent_is_rejected() {
        let findings = validate_policy(&policy(claim(
            ClaimState::NotProven,
            vec![
                candidate(10, Relationship::SalvageSource, None),
                candidate(11, Relationship::ExplicitStack, Some(10)),
            ],
        )));
        assert!(
            findings
                .iter()
                .any(|finding| finding.code == "STACK_TARGET_INVALID" && finding.pr == Some(11)),
            "stacking onto a salvage source must fail: {findings:?}"
        );
    }

    #[test]
    fn terminal_relationships_must_not_declare_targets() {
        let findings = validate_policy(&policy(claim(
            ClaimState::Reconciled,
            vec![
                candidate(10, Relationship::CurrentCandidate, None),
                candidate(11, Relationship::Abandoned, Some(10)),
                {
                    let mut salvage = candidate(12, Relationship::SalvageSource, None);
                    salvage.target_pr = Some(10);
                    salvage
                },
            ],
        )));
        assert_eq!(
            findings
                .iter()
                .filter(|finding| finding.code == "RELATIONSHIP_FORBIDS_TARGET")
                .map(|finding| finding.pr)
                .collect::<Vec<_>>(),
            vec![Some(11), Some(12)]
        );
    }

    #[test]
    fn observed_entries_require_reasons_and_may_not_shadow_candidates() {
        let mut base = policy(claim(
            ClaimState::NotProven,
            vec![candidate(10, Relationship::SalvageSource, None)],
        ));
        base.claim[0].observed.push(ObservedCrossReference { pr: 6930, reason: String::new() });
        base.claim[0].observed.push(ObservedCrossReference { pr: 10, reason: "shadow".into() });
        let codes: Vec<&str> = validate_policy(&base).iter().map(|finding| finding.code).collect();
        assert!(
            codes.contains(&"INVALID_OBSERVED_ENTRY") && codes.contains(&"OBSERVED_PR_CONFLICT")
        );
    }

    #[test]
    fn expired_or_unparseable_observation_times_are_drift() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-21T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            observation_time_finding("2026-07-01T00:00:00Z", now),
            Some("OBSERVATION_EXPIRED")
        );
        assert_eq!(observation_time_finding("2026-08-20T23:59:00Z", now), None);
        assert_eq!(
            observation_time_finding("not-a-timestamp", now),
            Some("OBSERVATION_TIME_UNPARSEABLE")
        );
    }

    #[test]
    fn drift_downgrades_claim_state_to_not_proven() {
        let mut reconciled = claim(
            ClaimState::Reconciled,
            vec![candidate(10, Relationship::CurrentCandidate, None)],
        );
        let drift = Finding {
            level: "error",
            code: "STALE_BASE_OBSERVATION",
            claim_id: Some(reconciled.claim_id.clone()),
            issue: Some(reconciled.issue),
            pr: Some(10),
            message: "recorded base main differs from live base release".to_string(),
        };
        apply_drift_downgrades(std::slice::from_mut(&mut reconciled), &[drift]);
        assert_eq!(reconciled.state, ClaimState::NotProven);

        let mut unaffected = claim(
            ClaimState::Reconciled,
            vec![candidate(10, Relationship::CurrentCandidate, None)],
        );
        let unrelated = Finding {
            level: "error",
            code: "MISSING_DECISION",
            claim_id: Some(unaffected.claim_id.clone()),
            issue: Some(unaffected.issue),
            pr: None,
            message: "decision must explain the current candidate-set conclusion".to_string(),
        };
        apply_drift_downgrades(std::slice::from_mut(&mut unaffected), &[unrelated]);
        assert_eq!(unaffected.state, ClaimState::Reconciled);
    }
}
