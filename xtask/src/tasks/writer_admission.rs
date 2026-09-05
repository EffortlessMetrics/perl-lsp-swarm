//! Writer admission — read-only pre-admission diagnostic (#3957 W1).
//!
//! Produces one typed `AdmissionVerdict` (`PASS` / `BLOCK` / `NOT_PROVEN`)
//! for "is it safe to open a writer worktree/branch here?", with a
//! per-check breakdown. **Read-only**: this module never mutates git state,
//! the filesystem, or GitHub — it only gathers signals and reports.
//!
//! It composes the semantics of the existing report-only tooling rather
//! than reimplementing them:
//! - `scripts/swarm-doctor` — worktree inventory / dirty / disk / divergence
//!   shape (`--json` mode).
//! - `scripts/swarm-clean::branch_pr_status` — the tri-state (`open` /
//!   `none` / `unknown`) PR-candidate pattern. This is candidate-existence
//!   evidence for reuse/resume guidance, not writer-liveness evidence.
//! - `scripts/clean-worktrees.sh` — the `FLOOR_GB=200` / `FLOOR_PCT=5` disk
//!   floor convention (reused verbatim, not reinvented).
//!
//! Checks (each contributes one `CheckResult` to the verdict):
//! 1. `canonical-base` — the live `refs/remotes/origin/<base>` SHA vs an
//!    optional caller-supplied expected SHA.
//! 2. `shadow-ref` — a reserved `refs/heads/origin/*` ref shadowing a
//!    remote-tracking ref (the cited real incident).
//! 3. `symbolic-head` — a dangling symbolic HEAD (points at a ref that does
//!    not resolve).
//! 4. `branch-worktree-mapping` — either the root checkout has drifted onto
//!    a feature branch (production-mutation risk), or the target branch is
//!    checked out in more than one worktree.
//! 5. `dirty-unpushed` — an abnormally large staged/dirty change set
//!    (possible synthetic mass-staged additions).
//! 6. `disk-capacity` — free disk below the `clean-worktrees.sh` floor.
//! 7. `remote-branch-identity` — distinguishes a known remote branch,
//!    known absence, and an instrument failure that leaves CREATE/RESUME
//!    selection `NOT_PROVEN`.
//! 8. `candidate-presence` — surfaces an existing open PR for reuse/resume;
//!    it never treats PR existence or lookup failure as a live writer.
//!
//! An instrument failure in a safety/identity check reports `NOT_PROVEN`,
//! never a false `PASS` — see `docs/reference/ISSUE_PLAN_DOCTRINE.md`-style
//! report-only doctrine. PR lookup is deliberately advisory: GitHub can say
//! whether a candidate exists, not whether another session is alive.
//!
//! Advisory-first: `run` always returns `Ok(())`. The verdict is
//! informational; nothing is blocked or mutated by W1 itself. Consuming the
//! verdict to gate a real writer-worktree creation is #3982's job, not
//! this module's.
//!
//! ## Resume/reuse guidance (#3957 W2)
//!
//! Alongside `checks[]`, the report carries an informational `guidance`
//! object (`AdmissionGuidance`) so a consumer (`/start-work`, #3982/#4103)
//! can distinguish "admit a brand-new branch/worktree" from "resume an
//! existing remote branch" or "reuse an existing worktree", rather than
//! double-creating either. `guidance` is additive metadata computed from
//! signals already gathered for the checks above. Remote-branch lookup
//! failure is also a typed `remote-branch-identity` check so guidance and
//! aggregate verdict cannot disagree. The protected object stays the
//! branch/worktree/local repo state, never a per-agent lease (#3957's
//! explicit non-goal).

use color_eyre::eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::tasks::git_context::git_stdout_with_worktree_fallback;

// ---- CLI config -------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AdmissionConfig {
    pub branch: Option<String>,
    pub base: String,
    pub worktree: Option<PathBuf>,
    pub expected_base_sha: Option<String>,
    pub repo: Option<String>,
    pub fixture: Option<PathBuf>,
    pub json: bool,
    pub floor_gb: f64,
    pub floor_pct: f64,
    pub large_staged_threshold: u32,
}

// ---- Snapshot (fixture schema / live-gathered signal bundle) ---------------

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct HeadInfo {
    #[serde(default)]
    pub symbolic_ref: Option<String>,
    #[serde(default)]
    pub resolved_sha: Option<String>,
    #[serde(default)]
    pub dangling: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ShadowRefInfo {
    #[serde(default)]
    pub refs: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CanonicalBaseInfo {
    #[serde(default)]
    pub remote_sha: Option<String>,
    #[serde(default)]
    pub selected_sha: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct WorktreeEntry {
    pub path: String,
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct WorktreeMappingInfo {
    #[serde(default)]
    pub entries: Vec<WorktreeEntry>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DirtyInfo {
    #[serde(default)]
    pub status_count: u32,
    #[serde(default)]
    pub unpushed_commits: u32,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DiskInfo {
    #[serde(default)]
    pub avail_gb: Option<f64>,
    #[serde(default)]
    pub total_gb: Option<f64>,
    #[serde(default)]
    pub worktree_count: Option<u32>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Mirrors `scripts/swarm-clean::branch_pr_status`'s tri-state exactly.
/// The state says whether an existing PR candidate was observed. It does
/// not identify a live writer, and `Unknown` must not be converted into a
/// collision merely because the query was unavailable.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PrStatus {
    Open,
    None,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PrOwnershipInfo {
    #[serde(default)]
    pub status: PrStatus,
    #[serde(default)]
    pub pr_number: Option<u64>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Resolves `refs/remotes/origin/<target_branch>` — does the target branch
/// already exist on the remote, and if so, at what SHA? Feeds
/// `AdmissionGuidance::remote_branch_sha` (the W2 RESUME signal): an
/// existing remote branch must be resumed from its actual head, never
/// recreated fresh off the requested base.
///
/// A non-existent remote branch is a legitimate absence (mirrors
/// `gather_head_info`'s `symbolic-ref -q` handling), not an instrument
/// failure — `error` is reserved for a genuine spawn failure.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct RemoteBranchInfo {
    #[serde(default)]
    pub sha: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WriterAdmissionSnapshot {
    pub target_branch: String,
    #[serde(default = "default_base")]
    pub requested_base: String,
    #[serde(default)]
    pub is_root_checkout: bool,
    #[serde(default)]
    pub head: HeadInfo,
    #[serde(default)]
    pub shadow_refs: ShadowRefInfo,
    #[serde(default)]
    pub canonical_base: CanonicalBaseInfo,
    #[serde(default)]
    pub worktree_mapping: WorktreeMappingInfo,
    #[serde(default)]
    pub dirty: DirtyInfo,
    #[serde(default)]
    pub disk: DiskInfo,
    #[serde(default)]
    pub pr_ownership: PrOwnershipInfo,
    #[serde(default)]
    pub remote_branch: RemoteBranchInfo,
}

fn default_base() -> String {
    "origin/main".to_string()
}

// ---- Verdict model -----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckStatus {
    Pass,
    Block,
    NotProven,
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            CheckStatus::Pass => "PASS",
            CheckStatus::Block => "BLOCK",
            CheckStatus::NotProven => "NOT_PROVEN",
        };
        write!(f, "{text}")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub status: CheckStatus,
    pub reason: String,
}

/// Distinct from `Posture` (`xtask/src/tasks/commit_checks.rs`) on purpose:
/// `Posture` is defined as never-blocking. `AdmissionVerdict::Block` is
/// explicitly a blocking signal for whatever consumes it (e.g. #3982's
/// `/start-work`) — this module must not extend `Posture` and silently
/// violate its never-blocks invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdmissionVerdict {
    Pass,
    Block,
    NotProven,
}

impl std::fmt::Display for AdmissionVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            AdmissionVerdict::Pass => "PASS",
            AdmissionVerdict::Block => "BLOCK",
            AdmissionVerdict::NotProven => "NOT_PROVEN",
        };
        write!(f, "{text}")
    }
}

/// Informational resume/reuse guidance (#3957 W2) — never a `CheckResult`,
/// never contributes to `aggregate_verdict`. A consumer (`/start-work`)
/// reads this to decide RESUME/REUSE/ADMIT once deterministic local
/// safety/capacity blockers are ruled out.
#[derive(Debug, Clone, Serialize, Default)]
pub struct AdmissionGuidance {
    /// Path of the single existing worktree already checked out on the
    /// target branch, when exactly one exists. `None` when no worktree
    /// maps to the branch (nothing to reuse), more than one does (already
    /// ambiguous — `branch-worktree-mapping` reports that separately; never
    /// suggest reuse when it's unclear which of several to reuse), or this
    /// invocation's own checkout is the root (the root is never a valid
    /// REUSE target — see `compute_guidance`'s doc comment).
    pub existing_worktree_path: Option<String>,
    /// The resolved SHA of `refs/remotes/origin/<target_branch>` when the
    /// target branch already exists on the remote. `None` for a genuinely
    /// new branch **or** when the lookup itself failed — check
    /// `remote_branch_lookup_error` to tell those two apart before treating
    /// a `None` here as "safe to ADMIT a fresh branch".
    pub remote_branch_sha: Option<String>,
    /// Set when the `refs/remotes/origin/<target_branch>` lookup itself
    /// failed (a genuine `git` instrument failure, e.g. not a git
    /// repository, not a spawnable `git`), as opposed to a legitimate
    /// "branch doesn't exist yet" absence. A consumer must treat a non-null
    /// value here as `NOT_PROVEN` for the RESUME decision; the aggregate
    /// report carries the same fact through `remote-branch-identity`.
    pub remote_branch_lookup_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdmissionReport {
    pub schema_version: String,
    pub mode: String,
    pub target_branch: String,
    pub verdict: AdmissionVerdict,
    pub checks: Vec<CheckResult>,
    pub guidance: AdmissionGuidance,
}

/// Worst-status-wins: any `Block` makes the whole verdict `Block`; absent
/// that, any `NotProven` makes it `NotProven`; only an all-`Pass` check set
/// is `Pass`.
pub fn aggregate_verdict(checks: &[CheckResult]) -> AdmissionVerdict {
    if checks.iter().any(|c| c.status == CheckStatus::Block) {
        AdmissionVerdict::Block
    } else if checks.iter().any(|c| c.status == CheckStatus::NotProven) {
        AdmissionVerdict::NotProven
    } else {
        AdmissionVerdict::Pass
    }
}

// ---- Entry point --------------------------------------------------------------

pub fn run(config: AdmissionConfig) -> Result<()> {
    let snapshot = load_snapshot(&config)?;
    let checks = run_checks(&snapshot, &config);
    let verdict = aggregate_verdict(&checks);
    let guidance = compute_guidance(&snapshot);
    let report = AdmissionReport {
        schema_version: "1".to_string(),
        mode: "advisory".to_string(),
        target_branch: snapshot.target_branch.clone(),
        verdict,
        checks,
        guidance,
    };
    print_report(&config, &report)?;
    // Advisory-first: W1 produces a verdict, it never blocks the command
    // itself. A consumer (e.g. #3982's /start-work) decides what to do
    // with AdmissionVerdict::Block.
    Ok(())
}

fn load_snapshot(config: &AdmissionConfig) -> Result<WriterAdmissionSnapshot> {
    if let Some(path) = &config.fixture {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read fixture {}", path.display()))?;
        let mut snapshot: WriterAdmissionSnapshot = serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse fixture {}", path.display()))?;
        if let Some(branch) = &config.branch {
            snapshot.target_branch = branch.clone();
        }
        return Ok(snapshot);
    }
    Ok(gather_live_snapshot(config))
}

// ---- Checks --------------------------------------------------------------------

pub fn run_checks(
    snapshot: &WriterAdmissionSnapshot,
    config: &AdmissionConfig,
) -> Vec<CheckResult> {
    vec![
        check_canonical_base(snapshot),
        check_shadow_ref(snapshot),
        check_symbolic_head(snapshot),
        check_branch_worktree_mapping(snapshot),
        check_dirty_unpushed(snapshot, config),
        check_disk_capacity(snapshot, config),
        check_remote_branch_identity(snapshot),
        check_candidate_presence(snapshot),
    ]
}

fn check_canonical_base(snapshot: &WriterAdmissionSnapshot) -> CheckResult {
    let name = "canonical-base".to_string();
    let info = &snapshot.canonical_base;
    if let Some(err) = &info.error {
        return CheckResult {
            name,
            status: CheckStatus::NotProven,
            reason: format!("could not resolve `{}`: {err}", snapshot.requested_base),
        };
    }
    let Some(remote_sha) = &info.remote_sha else {
        return CheckResult {
            name,
            status: CheckStatus::NotProven,
            reason: format!("no SHA resolved for `{}`", snapshot.requested_base),
        };
    };
    match &info.selected_sha {
        // No expectation was supplied — nothing to compare against.
        None => CheckResult {
            name,
            status: CheckStatus::Pass,
            reason: format!("`{}` resolves to {remote_sha}", snapshot.requested_base),
        },
        Some(selected_sha) if sha_matches(remote_sha, selected_sha) => CheckResult {
            name,
            status: CheckStatus::Pass,
            reason: format!("`{}` matches expected {remote_sha}", snapshot.requested_base),
        },
        Some(selected_sha) => CheckResult {
            name,
            status: CheckStatus::Block,
            reason: format!(
                "base ref mismatch: `{}` is {remote_sha} live, expected {selected_sha}",
                snapshot.requested_base
            ),
        },
    }
}

/// A caller-supplied `--expected-base-sha` is commonly a short/abbreviated
/// SHA (this is how commits are conventionally referenced in issues, PR
/// descriptions, and plan-review comments — e.g. "currently 6fade008c"),
/// not necessarily the full 40-hex-character SHA. Exact string equality
/// would false-BLOCK every short-SHA caller even when the base is exactly
/// correct. Accept a full match OR a case-insensitive hex-prefix match, with
/// a minimum prefix length (git's own historical default abbreviation is 7
/// hex characters) to avoid a degenerate short prefix matching too loosely.
fn sha_matches(remote_sha: &str, selected_sha: &str) -> bool {
    const MIN_PREFIX_LEN: usize = 4;
    if selected_sha.eq_ignore_ascii_case(remote_sha) {
        return true;
    }
    selected_sha.len() >= MIN_PREFIX_LEN
        && selected_sha.len() <= remote_sha.len()
        && selected_sha.chars().all(|c| c.is_ascii_hexdigit())
        && remote_sha[..selected_sha.len()].eq_ignore_ascii_case(selected_sha)
}

fn check_shadow_ref(snapshot: &WriterAdmissionSnapshot) -> CheckResult {
    let name = "shadow-ref".to_string();
    let info = &snapshot.shadow_refs;
    if let Some(err) = &info.error {
        return CheckResult {
            name,
            status: CheckStatus::NotProven,
            reason: format!("could not scan `refs/heads/origin/*`: {err}"),
        };
    }
    if info.refs.is_empty() {
        return CheckResult {
            name,
            status: CheckStatus::Pass,
            reason: "no reserved `refs/heads/origin/*` shadow refs found".to_string(),
        };
    }
    CheckResult {
        name,
        status: CheckStatus::Block,
        reason: format!(
            "reserved `refs/heads/origin/*` shadow ref(s) found, shadowing remote-tracking refs: {}",
            info.refs.join(", ")
        ),
    }
}

fn check_symbolic_head(snapshot: &WriterAdmissionSnapshot) -> CheckResult {
    let name = "symbolic-head".to_string();
    let info = &snapshot.head;
    if let Some(err) = &info.error {
        return CheckResult {
            name,
            status: CheckStatus::NotProven,
            reason: format!("could not resolve HEAD: {err}"),
        };
    }
    if info.dangling {
        return CheckResult {
            name,
            status: CheckStatus::Block,
            reason: format!(
                "HEAD is dangling (symbolic_ref={:?}, resolved_sha={:?})",
                info.symbolic_ref, info.resolved_sha
            ),
        };
    }
    CheckResult {
        name,
        status: CheckStatus::Pass,
        reason: match &info.symbolic_ref {
            Some(sym) => format!("HEAD -> {sym}, resolved"),
            None => "HEAD is detached but resolves cleanly".to_string(),
        },
    }
}

fn canonical_main_name(base: &str) -> &str {
    base.rsplit('/').next().unwrap_or(base)
}

fn check_branch_worktree_mapping(snapshot: &WriterAdmissionSnapshot) -> CheckResult {
    let name = "branch-worktree-mapping".to_string();
    let info = &snapshot.worktree_mapping;
    if let Some(err) = &info.error {
        return CheckResult {
            name,
            status: CheckStatus::NotProven,
            reason: format!("worktree inventory unavailable: {err}"),
        };
    }

    // Root-checkout health: production writes must never mutate the root
    // checkout in place — it must stay on the canonical base (or detached
    // at it), never drift onto a feature branch (#3957's "root checkout on
    // a feature branch" negative case).
    if snapshot.is_root_checkout {
        if let Some(sym) = &snapshot.head.symbolic_ref {
            let current_branch = sym.strip_prefix("refs/heads/").unwrap_or(sym);
            let canonical = canonical_main_name(&snapshot.requested_base);
            if current_branch != canonical && current_branch != "master" {
                return CheckResult {
                    name,
                    status: CheckStatus::Block,
                    reason: format!(
                        "root checkout is on feature branch `{current_branch}`, expected the \
                         canonical base `{canonical}` (production writes belong in a worktree)"
                    ),
                };
            }
        }
        return CheckResult {
            name,
            status: CheckStatus::Pass,
            reason: "root checkout is on the canonical base (or detached at it)".to_string(),
        };
    }

    // Linked worktree: the target branch must not already be checked out
    // elsewhere (git itself refuses this, but a stale/corrupted admin dir
    // entry could still surface it — detect rather than assume).
    let matches = worktrees_matching_target_branch(snapshot);
    if matches.len() > 1 {
        return CheckResult {
            name,
            status: CheckStatus::Block,
            reason: format!(
                "branch `{}` is checked out in {} worktrees: {}",
                snapshot.target_branch,
                matches.len(),
                matches.join(", ")
            ),
        };
    }

    CheckResult {
        name,
        status: CheckStatus::Pass,
        reason: "branch/worktree mapping is unambiguous".to_string(),
    }
}

fn check_dirty_unpushed(
    snapshot: &WriterAdmissionSnapshot,
    config: &AdmissionConfig,
) -> CheckResult {
    let name = "dirty-unpushed".to_string();
    let info = &snapshot.dirty;
    if let Some(err) = &info.error {
        return CheckResult {
            name,
            status: CheckStatus::NotProven,
            reason: format!("could not read working-tree status: {err}"),
        };
    }
    if info.status_count > config.large_staged_threshold {
        return CheckResult {
            name,
            status: CheckStatus::Block,
            reason: format!(
                "abnormally large change set: {} file(s) changed (threshold {}) — possible \
                 synthetic mass-staged additions",
                info.status_count, config.large_staged_threshold
            ),
        };
    }
    CheckResult {
        name,
        status: CheckStatus::Pass,
        reason: format!(
            "{} file(s) changed, {} unpushed commit(s) (informational)",
            info.status_count, info.unpushed_commits
        ),
    }
}

fn check_disk_capacity(
    snapshot: &WriterAdmissionSnapshot,
    config: &AdmissionConfig,
) -> CheckResult {
    let name = "disk-capacity".to_string();
    let info = &snapshot.disk;
    if let Some(err) = &info.error {
        return CheckResult {
            name,
            status: CheckStatus::NotProven,
            reason: format!("could not read disk usage: {err}"),
        };
    }
    let Some(avail_gb) = info.avail_gb else {
        return CheckResult {
            name,
            status: CheckStatus::NotProven,
            reason: "no disk-availability data reported".to_string(),
        };
    };
    // Matches scripts/clean-worktrees.sh's floor convention verbatim:
    // max(FLOOR_GB, FLOOR_PCT% of total volume size).
    let pct_floor = info.total_gb.map(|total| total * config.floor_pct / 100.0).unwrap_or(0.0);
    let floor = config.floor_gb.max(pct_floor);
    let worktree_note =
        info.worktree_count.map(|n| format!(", {n} worktree(s) present")).unwrap_or_default();
    if avail_gb < floor {
        return CheckResult {
            name,
            status: CheckStatus::Block,
            reason: format!(
                "disk headroom {avail_gb:.1}G is below the floor {floor:.1}G \
                 (max(FLOOR_GB={}, FLOOR_PCT={}%)){worktree_note}",
                config.floor_gb, config.floor_pct
            ),
        };
    }
    CheckResult {
        name,
        status: CheckStatus::Pass,
        reason: format!(
            "disk headroom {avail_gb:.1}G is above the floor {floor:.1}G{worktree_note}"
        ),
    }
}

/// Prove enough remote-branch identity to choose CREATE versus RESUME.
///
/// A known absence is safe and means CREATE remains available. A known SHA
/// means RESUME from that exact branch head. An instrument failure is
/// different: it leaves branch identity unknown, so writer admission is
/// `NOT_PROVEN` without inferring anything about another session's liveness.
fn check_remote_branch_identity(snapshot: &WriterAdmissionSnapshot) -> CheckResult {
    let name = "remote-branch-identity".to_string();
    if snapshot.target_branch == "(detached)" {
        return CheckResult {
            name,
            status: CheckStatus::Pass,
            reason: "detached checkout has no target branch identity to resolve".to_string(),
        };
    }
    let info = &snapshot.remote_branch;
    if let Some(err) = &info.error {
        return CheckResult {
            name,
            status: CheckStatus::NotProven,
            reason: format!(
                "could not resolve `refs/remotes/origin/{}`: {err}; CREATE versus RESUME is not proven",
                snapshot.target_branch
            ),
        };
    }
    match &info.sha {
        Some(sha) => CheckResult {
            name,
            status: CheckStatus::Pass,
            reason: format!(
                "remote branch `{}` resolves to {sha}; RESUME from that observed head",
                snapshot.target_branch
            ),
        },
        None => CheckResult {
            name,
            status: CheckStatus::Pass,
            reason: format!(
                "no remote branch observed for `{}`; CREATE remains available",
                snapshot.target_branch
            ),
        },
    }
}

/// Surface an existing PR candidate without inventing writer liveness.
///
/// #3957 says open PRs are surfaced while two *writers* on one branch are
/// the collision. #3982 says an existing open PR should be continued and
/// reused. GitHub PR existence therefore cannot by itself BLOCK writer
/// admission, and a failed PR lookup cannot prove that a writer exists.
fn check_candidate_presence(snapshot: &WriterAdmissionSnapshot) -> CheckResult {
    let name = "candidate-presence".to_string();
    let info = &snapshot.pr_ownership;
    if let Some(err) = &info.error {
        return CheckResult {
            name,
            status: CheckStatus::Pass,
            reason: format!(
                "PR lookup unavailable ({err}); candidate presence is not proven, and no writer \
                 collision is inferred from that absence of evidence"
            ),
        };
    }
    match info.status {
        PrStatus::Open => CheckResult {
            name,
            status: CheckStatus::Pass,
            reason: match info.pr_number {
                Some(n) => format!(
                    "open PR #{n} already exists for branch `{}` — reuse/resume that candidate; \
                     PR existence is not live-writer evidence",
                    snapshot.target_branch
                ),
                None => format!(
                    "an open PR already exists for branch `{}` — reuse/resume that candidate; \
                     PR existence is not live-writer evidence",
                    snapshot.target_branch
                ),
            },
        },
        PrStatus::None => CheckResult {
            name,
            status: CheckStatus::Pass,
            reason: format!("no open PR observed for branch `{}`", snapshot.target_branch),
        },
        PrStatus::Unknown => CheckResult {
            name,
            status: CheckStatus::Pass,
            reason: "PR candidate lookup unavailable; do not infer either candidate absence or a \
                     live writer from this signal"
                .to_string(),
        },
    }
}

/// Every worktree-inventory entry whose branch is exactly the target
/// branch. Shared by `check_branch_worktree_mapping` (BLOCK when more than
/// one) and `compute_guidance` (a REUSE candidate when exactly one) so the
/// two never drift into disagreeing definitions of "matches".
fn worktrees_matching_target_branch(snapshot: &WriterAdmissionSnapshot) -> Vec<&str> {
    snapshot
        .worktree_mapping
        .entries
        .iter()
        .filter(|e| e.branch.as_deref() == Some(snapshot.target_branch.as_str()))
        .map(|e| e.path.as_str())
        .collect()
}

/// Computes the informational RESUME/REUSE guidance (#3957 W2) from signals
/// already gathered for the checks above. Never itself a `CheckResult` and
/// never consulted by `aggregate_verdict` — a consumer applies this only
/// after ruling out deterministic local safety/capacity blockers.
///
/// The root checkout is never a valid REUSE target — WORKTREE_PROTOCOL.md is
/// explicit that production writes must never land in the root checkout,
/// only in a worktree. `git worktree list --porcelain` always lists the
/// main/root worktree alongside every linked one, so when this invocation's
/// own checkout IS the root (`snapshot.is_root_checkout`) and the root
/// happens to sit on the target branch (the exact drift scenario #3957's
/// problem statement opens with), `worktrees_matching_target_branch` would
/// otherwise return the root's own entry as if it were an ordinary linked
/// worktree available for reuse. `check_branch_worktree_mapping` already
/// reports that same condition as its own advisory-class `BLOCK` ("root
/// checkout is on feature branch ... production writes belong in a
/// worktree") — `compute_guidance` must never independently contradict that
/// by offering the same root path as a safe REUSE candidate, which would
/// let Step 6c's REUSE outcome route an operator straight back into the
/// root. Short-circuit unconditionally on `is_root_checkout`, not only when
/// it's the sole match — being invoked from the root is never itself a
/// reusable-worktree signal, regardless of how many entries matched.
pub fn compute_guidance(snapshot: &WriterAdmissionSnapshot) -> AdmissionGuidance {
    let existing_worktree_path = if snapshot.is_root_checkout {
        None
    } else {
        let matches = worktrees_matching_target_branch(snapshot);
        if matches.len() == 1 { Some(matches[0].to_string()) } else { None }
    };
    AdmissionGuidance {
        existing_worktree_path,
        remote_branch_sha: snapshot.remote_branch.sha.clone(),
        remote_branch_lookup_error: snapshot.remote_branch.error.clone(),
    }
}

// ---- Live gathering --------------------------------------------------------------

/// Runs a read-only `git` query and returns its non-empty stdout lines.
///
/// A non-zero exit is always treated as a genuine instrument failure
/// (`Err`), never silently folded into "no results". This matters for
/// callers like `for-each-ref`: it exits **0** with empty stdout when
/// nothing matches a pattern — a real, legitimate absence — so a non-zero
/// exit from it means something actually went wrong (corrupt ref store,
/// invalid pattern, not a git repository at all), not "zero matches".
/// Conflating the two would let a genuine failure silently report as a
/// clean/empty result — the exact instrument-failure-must-never-be-PASS
/// invariant this tool exists to uphold. Callers that DO have a
/// documented, command-specific "non-zero exit legitimately means
/// absence" case (e.g. `git symbolic-ref -q HEAD` on a detached HEAD)
/// must not route through this helper — they inspect `Command::output()`
/// directly, as `gather_head_info` does.
fn git_lines(root: &Path, args: &[&str]) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("failed to spawn `git {}`: {e}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("git {} failed with status {}", args.join(" "), output.status)
        } else {
            format!("git {} failed: {stderr}", args.join(" "))
        });
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.lines().filter(|l| !l.is_empty()).map(str::to_string).collect())
}

/// Builds the early-return `HeadInfo` for a `git` spawn failure encountered
/// while gathering HEAD info. Carries forward whatever fields an earlier
/// `git` call already resolved (e.g. `symbolic_ref`) so a later spawn error
/// can never silently discard already-known state via `..Default::default()`.
fn head_info_spawn_error(symbolic_ref: Option<String>, message: String) -> HeadInfo {
    HeadInfo { symbolic_ref, error: Some(message), ..Default::default() }
}

fn gather_head_info(root: &Path) -> HeadInfo {
    let symbolic =
        Command::new("git").args(["symbolic-ref", "-q", "HEAD"]).current_dir(root).output();
    let symbolic_ref = match symbolic {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if text.is_empty() { None } else { Some(text) }
        }
        // Non-zero exit from `symbolic-ref -q` legitimately means detached
        // HEAD, not an error.
        Ok(_) => None,
        Err(e) => {
            return head_info_spawn_error(None, format!("failed to spawn git symbolic-ref: {e}"));
        }
    };

    let resolved = Command::new("git")
        .args(["rev-parse", "-q", "--verify", "HEAD"])
        .current_dir(root)
        .output();
    let resolved_sha = match resolved {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if text.is_empty() { None } else { Some(text) }
        }
        Ok(_) => None,
        Err(e) => {
            return head_info_spawn_error(
                symbolic_ref,
                format!("failed to spawn git rev-parse: {e}"),
            );
        }
    };

    // Dangling: HEAD does not resolve to a commit at all (broken symbolic
    // target, or a genuinely empty/corrupt ref).
    let dangling = resolved_sha.is_none();

    HeadInfo { symbolic_ref, resolved_sha, dangling, error: None }
}

fn gather_shadow_refs(root: &Path) -> ShadowRefInfo {
    match git_lines(root, &["for-each-ref", "--format=%(refname)", "refs/heads/origin/"]) {
        Ok(refs) => ShadowRefInfo { refs, error: None },
        Err(err) => ShadowRefInfo { refs: Vec::new(), error: Some(err) },
    }
}

fn gather_canonical_base(root: &Path, config: &AdmissionConfig) -> CanonicalBaseInfo {
    let output = Command::new("git")
        .args(["rev-parse", "-q", "--verify", &config.base])
        .current_dir(root)
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
            CanonicalBaseInfo {
                remote_sha: Some(sha),
                selected_sha: config.expected_base_sha.clone(),
                error: None,
            }
        }
        Ok(out) => CanonicalBaseInfo {
            remote_sha: None,
            selected_sha: config.expected_base_sha.clone(),
            error: Some(format!(
                "git rev-parse --verify {} failed: {}",
                config.base,
                String::from_utf8_lossy(&out.stderr).trim()
            )),
        },
        Err(e) => CanonicalBaseInfo {
            remote_sha: None,
            selected_sha: config.expected_base_sha.clone(),
            error: Some(format!("failed to spawn git rev-parse: {e}")),
        },
    }
}

fn gather_worktree_mapping(root: &Path) -> WorktreeMappingInfo {
    let output =
        Command::new("git").args(["worktree", "list", "--porcelain"]).current_dir(root).output();
    let text = match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        Ok(out) => {
            return WorktreeMappingInfo {
                entries: Vec::new(),
                error: Some(format!(
                    "git worktree list failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                )),
            };
        }
        Err(e) => {
            return WorktreeMappingInfo {
                entries: Vec::new(),
                error: Some(format!("failed to spawn git worktree list: {e}")),
            };
        }
    };

    let mut entries = Vec::new();
    let mut path: Option<String> = None;
    let mut branch: Option<String> = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            if let Some(prev_path) = path.take() {
                entries.push(WorktreeEntry { path: prev_path, branch: branch.take() });
            }
            path = Some(p.to_string());
        } else if let Some(b) = line.strip_prefix("branch ") {
            branch = Some(b.trim_start_matches("refs/heads/").to_string());
        } else if line == "detached" {
            branch = None;
        }
    }
    if let Some(prev_path) = path.take() {
        entries.push(WorktreeEntry { path: prev_path, branch: branch.take() });
    }

    WorktreeMappingInfo { entries, error: None }
}

fn gather_dirty_info(worktree_root: &Path) -> DirtyInfo {
    let status =
        Command::new("git").args(["status", "--porcelain"]).current_dir(worktree_root).output();
    let status_count = match status {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.is_empty()).count() as u32
        }
        Ok(out) => {
            return DirtyInfo {
                status_count: 0,
                unpushed_commits: 0,
                error: Some(format!(
                    "git status failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                )),
            };
        }
        Err(e) => {
            return DirtyInfo {
                status_count: 0,
                unpushed_commits: 0,
                error: Some(format!("failed to spawn git status: {e}")),
            };
        }
    };

    // No upstream configured is a normal state for a brand-new branch, not
    // an instrument failure — treat as zero unpushed commits.
    let unpushed_commits = Command::new("git")
        .args(["rev-list", "@{u}..HEAD", "--count"])
        .current_dir(worktree_root)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8_lossy(&out.stdout).trim().parse::<u32>().ok())
        .unwrap_or(0);

    DirtyInfo { status_count, unpushed_commits, error: None }
}

/// Disposition note (reviewed, not changed): dividing 1024-byte blocks by
/// `1024*1024` yields **binary** gigabytes (GiB), not decimal GB, even
/// though the fields/flags are named `_gb`/`FLOOR_GB`. This is intentional
/// bug-for-bug compatibility with `scripts/clean-worktrees.sh`, which the
/// #3957 W1 spec requires reusing verbatim (`avail_gb=$((avail_kb / 1024 /
/// 1024))`, displayed as `${avail_gb}G`) rather than inventing a new
/// convention. The numerator and denominator always use the same units
/// here, so the floor comparison itself is correct; only the "GB" naming
/// is imprecise. Not relabeled to avoid diverging the CLI flag names
/// (`--floor-gb`, matching `FLOOR_GB`) from the reused source.
fn gather_disk_info(root: &Path, worktree_count: u32) -> DiskInfo {
    let output = Command::new("df").args(["-Pk", "."]).current_dir(root).output();
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let Some(fields_line) = text.lines().nth(1) else {
                return DiskInfo {
                    avail_gb: None,
                    total_gb: None,
                    worktree_count: Some(worktree_count),
                    error: Some("df produced no data line".to_string()),
                };
            };
            let fields: Vec<&str> = fields_line.split_whitespace().collect();
            // POSIX df -Pk: Filesystem 1024-blocks Used Available Capacity Mounted-on
            let total_kb = fields.get(1).and_then(|s| s.parse::<f64>().ok());
            let avail_kb = fields.get(3).and_then(|s| s.parse::<f64>().ok());
            DiskInfo {
                avail_gb: avail_kb.map(|kb| kb / (1024.0 * 1024.0)),
                total_gb: total_kb.map(|kb| kb / (1024.0 * 1024.0)),
                worktree_count: Some(worktree_count),
                error: None,
            }
        }
        Ok(out) => DiskInfo {
            avail_gb: None,
            total_gb: None,
            worktree_count: Some(worktree_count),
            error: Some(format!("df failed: {}", String::from_utf8_lossy(&out.stderr).trim())),
        },
        Err(e) => DiskInfo {
            avail_gb: None,
            total_gb: None,
            worktree_count: Some(worktree_count),
            error: Some(format!("failed to spawn df: {e}")),
        },
    }
}

/// Tri-state candidate lookup — mirrors
/// `scripts/swarm-clean::branch_pr_status` exactly. An open result means an
/// existing PR should be reused/resumed; it does not mean a writer is live.
fn gather_pr_ownership(branch: &str, repo: Option<&str>) -> PrOwnershipInfo {
    if branch.is_empty() {
        // An empty `--head` filter is not "no filter" from gh's point of
        // view in every code path — it must never be sent, or the query
        // can match an unrelated PR and misattribute candidate presence.
        return PrOwnershipInfo { status: PrStatus::Unknown, pr_number: None, error: None };
    }
    if which_gh().is_none() {
        return PrOwnershipInfo { status: PrStatus::Unknown, pr_number: None, error: None };
    }
    let mut cmd = Command::new("gh");
    cmd.args(["pr", "list", "--head", branch, "--state", "open", "--json", "number"]);
    if let Some(repo) = repo {
        cmd.args(["--repo", repo]);
    }
    let output = cmd.output();
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            match serde_json::from_str::<Vec<serde_json::Value>>(&text) {
                Ok(items) if items.is_empty() => {
                    PrOwnershipInfo { status: PrStatus::None, pr_number: None, error: None }
                }
                Ok(items) => {
                    let pr_number =
                        items.first().and_then(|v| v.get("number")).and_then(|n| n.as_u64());
                    PrOwnershipInfo { status: PrStatus::Open, pr_number, error: None }
                }
                Err(e) => PrOwnershipInfo {
                    status: PrStatus::Unknown,
                    pr_number: None,
                    error: Some(format!("could not parse gh pr list output: {e}")),
                },
            }
        }
        Ok(_) | Err(_) => {
            PrOwnershipInfo { status: PrStatus::Unknown, pr_number: None, error: None }
        }
    }
}

/// Resolves `refs/remotes/origin/<branch>` — the W2 RESUME signal. A
/// non-zero exit from `rev-parse -q --verify` with **empty** stderr
/// legitimately means the branch doesn't exist on the remote yet (`-q`
/// suppresses git's "no such ref" message, mirrors `gather_head_info`'s
/// `symbolic-ref -q` handling); a non-zero exit that DID print to stderr
/// (e.g. "fatal: not a git repository...") is a genuine instrument
/// failure and must not be folded into that same silent absence — `-q`
/// only suppresses the ref-not-found message, not earlier repository-
/// level failures.
fn gather_remote_branch_info(root: &Path, branch: &str) -> RemoteBranchInfo {
    let output = Command::new("git")
        .args(["rev-parse", "-q", "--verify", &format!("refs/remotes/origin/{branch}")])
        .current_dir(root)
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
            RemoteBranchInfo { sha: if sha.is_empty() { None } else { Some(sha) }, error: None }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if stderr.is_empty() {
                RemoteBranchInfo { sha: None, error: None }
            } else {
                RemoteBranchInfo {
                    sha: None,
                    error: Some(format!("git rev-parse --verify failed: {stderr}")),
                }
            }
        }
        Err(e) => RemoteBranchInfo {
            sha: None,
            error: Some(format!("failed to spawn git rev-parse: {e}")),
        },
    }
}

fn which_gh() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let exe_name = if cfg!(windows) { "gh.exe" } else { "gh" };
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(exe_name))
        .find(|candidate| candidate.is_file())
}

fn gather_live_snapshot(config: &AdmissionConfig) -> WriterAdmissionSnapshot {
    let root = config.worktree.clone().unwrap_or_else(|| PathBuf::from("."));

    let head = gather_head_info(&root);

    let is_root_checkout = {
        let git_dir = git_stdout_with_worktree_fallback(&root, &["rev-parse", "--git-dir"]).ok();
        let common_dir =
            git_stdout_with_worktree_fallback(&root, &["rev-parse", "--git-common-dir"]).ok();
        match (git_dir, common_dir) {
            (Some(a), Some(b)) => normalize_dir(&root, &a) == normalize_dir(&root, &b),
            _ => false,
        }
    };

    let branch = config.branch.clone().or_else(|| {
        head.symbolic_ref.as_ref().map(|s| s.strip_prefix("refs/heads/").unwrap_or(s).to_string())
    });
    let target_branch = branch.unwrap_or_else(|| "(detached)".to_string());

    let worktree_mapping = gather_worktree_mapping(&root);
    let worktree_count = worktree_mapping.entries.len() as u32;

    // Use the *resolved* target branch (never the raw, possibly-absent
    // `--branch` flag) for the existing-candidate query. Querying `gh pr
    // list --head ""` would silently drop the filter and return an
    // unrelated open PR. A detached checkout has no branch identity to
    // query, so that case is simply not applicable.
    let pr_ownership = if target_branch == "(detached)" {
        PrOwnershipInfo { status: PrStatus::None, pr_number: None, error: None }
    } else {
        gather_pr_ownership(&target_branch, config.repo.as_deref())
    };

    // Same "no branch identity, nothing to resolve" carve-out as
    // pr_ownership above — a detached checkout has no target branch for a
    // remote-branch lookup to make sense against.
    let remote_branch = if target_branch == "(detached)" {
        RemoteBranchInfo::default()
    } else {
        gather_remote_branch_info(&root, &target_branch)
    };

    WriterAdmissionSnapshot {
        target_branch,
        requested_base: config.base.clone(),
        is_root_checkout,
        head,
        shadow_refs: gather_shadow_refs(&root),
        canonical_base: gather_canonical_base(&root, config),
        worktree_mapping,
        dirty: gather_dirty_info(&root),
        disk: gather_disk_info(&root, worktree_count),
        pr_ownership,
        remote_branch,
    }
}

fn normalize_dir(root: &Path, raw: &str) -> PathBuf {
    let candidate = Path::new(raw);
    let joined =
        if candidate.is_absolute() { candidate.to_path_buf() } else { root.join(candidate) };
    fs::canonicalize(&joined).unwrap_or(joined)
}

// ---- Output --------------------------------------------------------------------

fn print_report(config: &AdmissionConfig, report: &AdmissionReport) -> Result<()> {
    if config.json {
        let rendered = serde_json::to_string_pretty(report)?;
        println!("{rendered}");
        return Ok(());
    }
    println!("Writer Admission [{}]: {} — {}", report.mode, report.target_branch, report.verdict);
    for check in &report.checks {
        println!("  [{}] {}: {}", check.status, check.name, check.reason);
    }
    if let Some(path) = &report.guidance.existing_worktree_path {
        println!("  [GUIDANCE] existing worktree at {path} — REUSE, do not double-create");
    }
    if let Some(sha) = &report.guidance.remote_branch_sha {
        println!(
            "  [GUIDANCE] branch `{}` already exists on the remote at {sha} — RESUME from \
             there, do not recreate off the requested base",
            report.target_branch
        );
    }
    if let Some(err) = &report.guidance.remote_branch_lookup_error {
        println!(
            "  [GUIDANCE] NOT_PROVEN: could not resolve whether branch `{}` exists on the \
             remote: {err} — do not treat this as \"safe to ADMIT a fresh branch\"",
            report.target_branch
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_snapshot() -> WriterAdmissionSnapshot {
        WriterAdmissionSnapshot {
            target_branch: "impl/1234-feature".to_string(),
            requested_base: "origin/main".to_string(),
            is_root_checkout: false,
            head: HeadInfo {
                symbolic_ref: Some("refs/heads/impl/1234-feature".to_string()),
                resolved_sha: Some("abc123".to_string()),
                dangling: false,
                error: None,
            },
            shadow_refs: ShadowRefInfo { refs: Vec::new(), error: None },
            canonical_base: CanonicalBaseInfo {
                remote_sha: Some("deadbeef".to_string()),
                selected_sha: None,
                error: None,
            },
            worktree_mapping: WorktreeMappingInfo {
                entries: vec![WorktreeEntry {
                    path: "/repo/.claude/worktrees/agent-1".to_string(),
                    branch: Some("impl/1234-feature".to_string()),
                }],
                error: None,
            },
            dirty: DirtyInfo { status_count: 0, unpushed_commits: 1, error: None },
            disk: DiskInfo {
                avail_gb: Some(500.0),
                total_gb: Some(2000.0),
                worktree_count: Some(3),
                error: None,
            },
            pr_ownership: PrOwnershipInfo { status: PrStatus::None, pr_number: None, error: None },
            remote_branch: RemoteBranchInfo::default(),
        }
    }

    fn default_config() -> AdmissionConfig {
        AdmissionConfig {
            branch: None,
            base: "origin/main".to_string(),
            worktree: None,
            expected_base_sha: None,
            repo: None,
            fixture: None,
            json: false,
            floor_gb: 200.0,
            floor_pct: 5.0,
            large_staged_threshold: 1000,
        }
    }

    #[test]
    fn healthy_snapshot_is_pass() {
        let snapshot = base_snapshot();
        let checks = run_checks(&snapshot, &default_config());
        assert!(checks.iter().all(|c| c.status == CheckStatus::Pass), "{checks:?}");
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::Pass);
    }

    #[test]
    fn shadow_ref_blocks() {
        let mut snapshot = base_snapshot();
        snapshot.shadow_refs.refs = vec!["refs/heads/origin/main".to_string()];
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::Block);
        assert!(
            checks.iter().any(|c| c.name == "shadow-ref" && c.status == CheckStatus::Block),
            "expected shadow-ref check present and blocking: {checks:?}"
        );
    }

    #[test]
    fn dangling_head_blocks_without_mutating_anything() {
        let mut snapshot = base_snapshot();
        snapshot.head.dangling = true;
        snapshot.head.resolved_sha = None;
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::Block);
        assert!(
            checks.iter().any(|c| c.name == "symbolic-head" && c.status == CheckStatus::Block),
            "expected symbolic-head check present and blocking: {checks:?}"
        );
    }

    #[test]
    fn detached_root_checkout_at_canonical_base_is_pass() {
        let mut snapshot = base_snapshot();
        snapshot.is_root_checkout = true;
        snapshot.target_branch = "(detached)".to_string();
        snapshot.head.symbolic_ref = None;
        snapshot.worktree_mapping.entries =
            vec![WorktreeEntry { path: "/repo".to_string(), branch: None }];
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::Pass, "{checks:?}");
    }

    #[test]
    fn root_checkout_on_feature_branch_blocks() {
        let mut snapshot = base_snapshot();
        snapshot.is_root_checkout = true;
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::Block);
        assert!(
            checks
                .iter()
                .any(|c| c.name == "branch-worktree-mapping" && c.status == CheckStatus::Block),
            "expected branch-worktree-mapping check present and blocking: {checks:?}"
        );
    }

    #[test]
    fn duplicate_worktree_mapping_blocks() {
        let mut snapshot = base_snapshot();
        snapshot.worktree_mapping.entries.push(WorktreeEntry {
            path: "/repo/.claude/worktrees/agent-2".to_string(),
            branch: Some("impl/1234-feature".to_string()),
        });
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::Block);
    }

    #[test]
    fn base_mismatch_blocks() {
        let mut snapshot = base_snapshot();
        snapshot.canonical_base.selected_sha = Some("stale-sha".to_string());
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::Block);
    }

    #[test]
    fn short_sha_prefix_of_the_correct_base_is_not_a_false_block() {
        // --expected-base-sha is commonly a short SHA (e.g. "currently
        // 6fade008c" is how commits get cited in issues/plan-reviews).
        // Exact-string equality would false-BLOCK this every time even
        // though the base is exactly correct.
        let mut snapshot = base_snapshot();
        // base_snapshot()'s remote_sha is "deadbeef".
        snapshot.canonical_base.selected_sha = Some("dead".to_string());
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::Pass, "{checks:?}");
    }

    #[test]
    fn short_sha_that_is_genuinely_a_different_commit_still_blocks() {
        let mut snapshot = base_snapshot();
        // Not a prefix of "deadbeef" at all — a real mismatch, short or not.
        snapshot.canonical_base.selected_sha = Some("beef".to_string());
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::Block, "{checks:?}");
    }

    #[test]
    fn sha_matches_accepts_exact_and_prefix_rejects_short_and_wrong() {
        assert!(sha_matches("deadbeef", "deadbeef"));
        assert!(sha_matches("deadbeef", "DEAD"), "case-insensitive prefix should match");
        assert!(!sha_matches("deadbeef", "dea"), "below the minimum prefix length must not match");
        assert!(!sha_matches("deadbeef", "beef"), "a non-prefix substring must not match");
        assert!(
            !sha_matches("deadbeef", "deadbeefff"),
            "longer than the remote SHA must not match"
        );
        assert!(!sha_matches("deadbeef", "dead-bad"), "non-hex characters must not match");
    }

    #[test]
    fn low_disk_blocks() {
        let mut snapshot = base_snapshot();
        snapshot.disk.avail_gb = Some(10.0);
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::Block);
    }

    #[test]
    fn large_staged_change_set_blocks() {
        let mut snapshot = base_snapshot();
        snapshot.dirty.status_count = 5000;
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::Block);
    }

    #[test]
    fn open_pr_is_candidate_presence_not_writer_collision() {
        // #3957 surfaces an existing PR so callers can reuse the current
        // candidate. It does not say that PR existence proves a live writer.
        // This is the exact regression boundary: the old implementation
        // converted `PrStatus::Open` into a writer-collision BLOCK.
        let mut snapshot = base_snapshot();
        snapshot.pr_ownership =
            PrOwnershipInfo { status: PrStatus::Open, pr_number: Some(42), error: None };
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::Pass, "{checks:?}");
        assert!(
            checks.iter().any(|c| {
                c.name == "candidate-presence"
                    && c.status == CheckStatus::Pass
                    && c.reason.contains("reuse/resume")
                    && c.reason.contains("not live-writer evidence")
            }),
            "expected open PR to be surfaced as candidate presence, not a collision: {checks:?}"
        );
        assert!(
            !checks.iter().any(|c| c.name == "writer-collision"),
            "writer-collision must not be synthesized from PR existence: {checks:?}"
        );
    }

    #[test]
    fn gh_unavailable_does_not_invent_writer_collision() {
        // PR lookup can establish an existing candidate for reuse, but it
        // cannot establish whether another session is alive. Losing that
        // advisory lookup therefore must not become a liveness blocker.
        let mut snapshot = base_snapshot();
        snapshot.pr_ownership =
            PrOwnershipInfo { status: PrStatus::Unknown, pr_number: None, error: None };
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::Pass, "{checks:?}");
        assert!(
            checks.iter().any(|c| {
                c.name == "candidate-presence"
                    && c.status == CheckStatus::Pass
                    && c.reason.contains("do not infer")
            }),
            "unavailable PR lookup must remain candidate uncertainty, not writer collision: {checks:?}"
        );
    }

    #[test]
    fn remote_branch_lookup_error_is_not_proven_without_inventing_liveness() {
        // Unlike PR lookup, the remote-ref lookup owns a real identity
        // decision: CREATE versus RESUME. A tool failure here cannot be
        // folded into "branch absent" without risking recreation from the
        // wrong base, but it still says nothing about writer liveness.
        let mut snapshot = base_snapshot();
        snapshot.remote_branch = RemoteBranchInfo {
            sha: None,
            error: Some("git rev-parse --verify failed: fatal: not a git repository".to_string()),
        };
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::NotProven, "{checks:?}");
        assert!(
            checks.iter().any(|c| {
                c.name == "remote-branch-identity"
                    && c.status == CheckStatus::NotProven
                    && c.reason.contains("CREATE versus RESUME is not proven")
            }),
            "remote identity failure must be typed NOT_PROVEN: {checks:?}"
        );
        assert!(
            !checks.iter().any(|c| c.name == "writer-collision"),
            "remote identity failure must not be converted into writer liveness: {checks:?}"
        );
    }

    #[test]
    fn tool_error_on_any_safety_check_is_not_proven_never_pass() {
        let mut snapshot = base_snapshot();
        snapshot.disk.error = Some("df: command not found".to_string());
        snapshot.disk.avail_gb = None;
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::NotProven);
    }

    #[test]
    fn aggregate_prefers_block_over_not_proven() {
        let checks = vec![
            CheckResult {
                name: "a".to_string(),
                status: CheckStatus::NotProven,
                reason: String::new(),
            },
            CheckResult {
                name: "b".to_string(),
                status: CheckStatus::Block,
                reason: String::new(),
            },
        ];
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::Block);
    }

    #[test]
    fn fixture_deserializes_with_minimal_fields() -> Result<()> {
        let json = r#"{"target_branch": "impl/1-x"}"#;
        let snapshot: WriterAdmissionSnapshot = serde_json::from_str(json)?;
        assert_eq!(snapshot.target_branch, "impl/1-x");
        assert_eq!(snapshot.requested_base, "origin/main");
        assert!(!snapshot.is_root_checkout);
        assert_eq!(snapshot.pr_ownership.status, PrStatus::Unknown);
        // A fixture predating #3957 W2 has no `remote_branch` key at all —
        // it must default cleanly, never fail to parse.
        assert_eq!(snapshot.remote_branch.sha, None);
        Ok(())
    }

    #[test]
    fn empty_branch_never_reaches_the_pr_candidate_query() {
        // Regression test for a real live-smoke bug: when no --branch is
        // supplied and HEAD cannot resolve to a name, an empty `--head`
        // filter can be dropped by gh and match an unrelated PR. The object
        // is now candidate presence rather than writer liveness, but the
        // attribution bug is unchanged: empty branch must map straight to
        // Unknown without spawning gh at all.
        let info = gather_pr_ownership("", None);
        assert_eq!(info.status, PrStatus::Unknown);
        assert!(info.pr_number.is_none());
    }

    #[test]
    fn shadow_ref_check_with_error_is_not_proven_never_pass() {
        // Regression guard on the check-level contract: an empty `refs`
        // list is legitimate ("no matches", e.g. `for-each-ref` exiting 0
        // with nothing to report) and must PASS, but the SAME empty list
        // paired with a genuine tool error must never be folded into that
        // same PASS — it must be NOT_PROVEN. This is the invariant the
        // live-gathering bug below violated: a real `git for-each-ref`
        // failure was indistinguishable from a legitimate empty match.
        let mut snapshot = base_snapshot();
        snapshot.shadow_refs = ShadowRefInfo {
            refs: Vec::new(),
            error: Some("git for-each-ref failed: fatal: not a git repository".to_string()),
        };
        let checks = run_checks(&snapshot, &default_config());
        assert_eq!(aggregate_verdict(&checks), AdmissionVerdict::NotProven, "{checks:?}");
        assert!(
            checks.iter().any(|c| c.name == "shadow-ref" && c.status == CheckStatus::NotProven),
            "expected shadow-ref to be NOT_PROVEN, never PASS, on a tool error: {checks:?}"
        );
    }

    #[test]
    fn gather_shadow_refs_on_a_genuine_git_failure_reports_error_not_empty() -> Result<()> {
        // `git for-each-ref` exits 0 with empty stdout when nothing matches,
        // so a non-zero exit is a genuine instrument failure. Before the
        // fix, `git_lines` swallowed that failure into `Ok(vec![])`, making
        // "not a repository" indistinguishable from "no shadow refs" and
        // silently manufacturing PASS.
        let dir = tempfile::tempdir()?;
        // dir.path() is deliberately NOT a git repository.
        let info = gather_shadow_refs(dir.path());
        assert!(
            info.error.is_some(),
            "expected a genuine git failure (not a git repository) to be surfaced as an error, \
             not silently folded into an empty match list: {info:?}"
        );
        assert!(info.refs.is_empty());
        Ok(())
    }

    #[test]
    fn head_info_spawn_error_preserves_already_gathered_symbolic_ref() -> Result<()> {
        use perl_tdd_support::must_some;
        // Regression for the writer-admission fast-follow (#3957 W1): the
        // `git rev-parse` spawn-error arm of `gather_head_info` used to
        // rebuild `HeadInfo` with `..Default::default()`, resetting a
        // `symbolic_ref` that the earlier `git symbolic-ref` call had
        // already proved. Pin the helper at the unit where that loss lived.
        let info = head_info_spawn_error(
            Some("refs/heads/impl/1234-feature".to_string()),
            "failed to spawn git rev-parse: boom".to_string(),
        );
        let symbolic_ref = must_some(info.symbolic_ref.clone());
        assert_eq!(symbolic_ref, "refs/heads/impl/1234-feature");
        let error = must_some(info.error.clone());
        assert_eq!(error, "failed to spawn git rev-parse: boom");
        assert_eq!(info.resolved_sha, None);
        assert!(!info.dangling);
        Ok(())
    }

    #[test]
    fn head_info_spawn_error_with_no_prior_symbolic_ref_stays_none() -> Result<()> {
        use perl_tdd_support::must_some;
        // Symmetric control: if the first symbolic-ref call itself cannot
        // spawn, there is no prior ref to preserve and None is correct.
        let info =
            head_info_spawn_error(None, "failed to spawn git symbolic-ref: boom".to_string());
        assert_eq!(info.symbolic_ref, None);
        let error = must_some(info.error.clone());
        assert_eq!(error, "failed to spawn git symbolic-ref: boom");
        Ok(())
    }

    // ---- #3957 W2: resume/reuse guidance -----------------------------------

    #[test]
    fn guidance_reports_reuse_candidate_for_exactly_one_matching_worktree() -> Result<()> {
        use perl_tdd_support::must_some;
        // base_snapshot() already has exactly one worktree entry mapped to
        // the target branch — a clean REUSE candidate.
        let snapshot = base_snapshot();
        let guidance = compute_guidance(&snapshot);
        let path = must_some(guidance.existing_worktree_path);
        assert_eq!(path, "/repo/.claude/worktrees/agent-1");
        assert_eq!(guidance.remote_branch_sha, None, "no remote_branch.sha set on this fixture");
        Ok(())
    }

    #[test]
    fn guidance_reports_no_reuse_candidate_when_no_worktree_matches() {
        // Negative control: this must not be hardcoded to "always Some".
        // Zero matching worktree entries means there is nothing to reuse.
        let mut snapshot = base_snapshot();
        snapshot.worktree_mapping.entries =
            vec![WorktreeEntry { path: "/repo".to_string(), branch: Some("main".to_string()) }];
        let guidance = compute_guidance(&snapshot);
        assert_eq!(
            guidance.existing_worktree_path, None,
            "no worktree maps to the target branch — nothing to reuse"
        );
    }

    #[test]
    fn guidance_stays_none_when_worktree_mapping_is_ambiguous() {
        // Mutation check: falling back to "first match" would suggest one
        // of two worktrees even though branch-worktree-mapping correctly
        // marks that topology ambiguous and unsafe.
        let mut snapshot = base_snapshot();
        snapshot.worktree_mapping.entries.push(WorktreeEntry {
            path: "/repo/.claude/worktrees/agent-2".to_string(),
            branch: Some(snapshot.target_branch.clone()),
        });
        let guidance = compute_guidance(&snapshot);
        assert_eq!(
            guidance.existing_worktree_path, None,
            "ambiguous branch/worktree mapping must never suggest a reuse candidate: \
             {guidance:?}"
        );
    }

    #[test]
    fn guidance_never_offers_the_root_checkout_as_a_reuse_candidate() {
        // Regression for a P1 caught by independent execution review of
        // #3957 W2. `git worktree list --porcelain` includes the main/root
        // worktree. If this invocation is itself the root and the root has
        // drifted onto the target feature branch, there can be exactly one
        // matching entry — the root. A naive "exactly one => REUSE" rule
        // would contradict branch-worktree-mapping's BLOCK and route writes
        // straight back into the coordination checkout.
        let mut snapshot = base_snapshot();
        snapshot.is_root_checkout = true;
        // Exactly the reviewer's repro: the sole matching path is root.
        snapshot.worktree_mapping.entries = vec![WorktreeEntry {
            path: "/repo".to_string(),
            branch: Some(snapshot.target_branch.clone()),
        }];
        let guidance = compute_guidance(&snapshot);
        assert_eq!(
            guidance.existing_worktree_path, None,
            "REUSE must never be offered for the root checkout, even when it is the sole \
             matching worktree entry: {guidance:?}"
        );
    }

    #[test]
    fn guidance_reports_resume_candidate_from_remote_branch_sha() -> Result<()> {
        use perl_tdd_support::must_some;
        let mut snapshot = base_snapshot();
        // No local worktree is on the branch, but the remote branch already
        // exists. This is RESUME from the observed head, not fresh CREATE.
        snapshot.worktree_mapping.entries =
            vec![WorktreeEntry { path: "/repo".to_string(), branch: Some("main".to_string()) }];
        snapshot.remote_branch =
            RemoteBranchInfo { sha: Some("f00dcafe".to_string()), error: None };
        let guidance = compute_guidance(&snapshot);
        let sha = must_some(guidance.remote_branch_sha);
        assert_eq!(sha, "f00dcafe");
        assert_eq!(guidance.existing_worktree_path, None);
        assert_eq!(
            guidance.remote_branch_lookup_error, None,
            "a successful lookup must not also carry a lookup error"
        );
        Ok(())
    }

    #[test]
    fn guidance_propagates_a_genuine_remote_branch_lookup_failure() -> Result<()> {
        use perl_tdd_support::must_some;
        // Regression for #3957 W2: lookup failure and legitimate branch
        // absence both used to collapse to `remote_branch_sha: None`. That
        // can turn an instrument failure into fresh CREATE from the base.
        // Keep the error in guidance while remote-branch-identity carries
        // the same fact into the aggregate NOT_PROVEN verdict.
        let mut snapshot = base_snapshot();
        snapshot.remote_branch =
            RemoteBranchInfo { sha: None, error: Some("fatal: not a git repository".to_string()) };
        let guidance = compute_guidance(&snapshot);
        assert_eq!(guidance.remote_branch_sha, None, "no SHA was resolved — this must stay None");
        let error = must_some(guidance.remote_branch_lookup_error);
        assert_eq!(error, "fatal: not a git repository");
        Ok(())
    }

    #[test]
    fn guidance_is_carried_through_the_full_report() -> Result<()> {
        // End-to-end wiring control: guidance must reach the serialized
        // AdmissionReport, not exist only in compute_guidance's unit tests.
        let snapshot = base_snapshot();
        let checks = run_checks(&snapshot, &default_config());
        let verdict = aggregate_verdict(&checks);
        let guidance = compute_guidance(&snapshot);
        let report = AdmissionReport {
            schema_version: "1".to_string(),
            mode: "advisory".to_string(),
            target_branch: snapshot.target_branch.clone(),
            verdict,
            checks,
            guidance,
        };
        let rendered = serde_json::to_string(&report)?;
        assert!(
            rendered.contains("\"existing_worktree_path\":\"/repo/.claude/worktrees/agent-1\""),
            "expected guidance to be serialized into the report JSON: {rendered}"
        );
        Ok(())
    }

    #[test]
    fn gather_remote_branch_info_on_a_nonexistent_branch_is_none_not_an_error() -> Result<()> {
        // `-q --verify` exiting non-zero for a ref that simply does not
        // exist is a legitimate brand-new-branch absence. It must remain
        // distinct from a repository/tool failure so CREATE stays valid.
        let dir = tempfile::tempdir()?;
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .context("failed to spawn git init")?;
        assert!(init.success(), "git init must succeed in the temp dir");
        let info = gather_remote_branch_info(dir.path(), "impl/9999-does-not-exist");
        assert_eq!(info.sha, None);
        assert_eq!(info.error, None, "a legitimate absence must never be reported as an error");
        Ok(())
    }

    #[test]
    fn gather_remote_branch_info_on_a_genuine_spawn_failure_reports_error() -> Result<()> {
        // Not a git repository at all: rev-parse cannot establish remote-ref
        // identity, so this must surface as error rather than silently look
        // like "branch does not exist".
        let dir = tempfile::tempdir()?;
        let info = gather_remote_branch_info(dir.path(), "impl/1234-feature");
        assert!(
            info.error.is_some(),
            "expected a genuine git failure (not a git repository) to be surfaced as an \
             error: {info:?}"
        );
        assert_eq!(info.sha, None);
        Ok(())
    }
}
