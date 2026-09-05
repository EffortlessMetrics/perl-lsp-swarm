//! Read-only module live frontier: `module_train_live.v1` (#11627, node C03).
//!
//! This slice joins the checked module train (`.spec/11625-module-train-graph`,
//! #11625) and the offline current-tree projection (#11626, consumed through
//! `LoadedManifest::node_statuses()`) to one bounded, immutable, read-only
//! observation of live collaboration state, and classifies the safest current
//! action per node/writer-conflict surface.
//!
//! Boundaries (issue law):
//!
//! * only `live refresh` (network mode) touches the network or any external
//!   state, strictly through read-only observation commands; every other
//!   subcommand is pure/offline/deterministic from the immutable snapshot;
//! * no assignment, lease, scheduling, branch/worktree/issue/PR creation,
//!   comment, review, repair, push, merge, close, release, publication,
//!   support promotion, or any other external mutation is performed by any
//!   code path (all subprocesses route through one allowlisted choke point
//!   asserted read-only by tests);
//! * missing permission, rate limit, truncation, timeout, malformed response,
//!   or adapter failure is `not_proven`/`instrument_failed`, never "no
//!   candidate", "no review", or pass;
//! * candidate ownership requires the explicit machine-checkable identity
//!   block plus manifest agreement; title/branch/author/labels/age/CI colour
//!   are diagnostics only;
//! * candidate existence, binding agreement, review decision, checks, merge
//!   probing, dirty/unpushed unique work and instrument health stay
//!   independent state flags, never one collapsed signal;
//! * GitHub/live state can never manufacture implementation, behavior,
//!   profile, support or release truth: review-head currency, review threads
//!   and behavior receipts are not observable with this slice's bounded
//!   fields, so they remain typed blockers that keep
//!   `MERGE_READY_RECOMMENDATION` unreachable from live observation.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::module_train::{
    LoadedManifest, NodeStaticFact, ProbeOutcome, canonical_digest, load_manifest,
};

#[cfg(test)]
#[path = "module_train_live_tests.rs"]
mod tests;

/// Snapshot schema identity produced by this tool.
pub const LIVE_SCHEMA_NAME: &str = "module_train_live.v1";
/// Snapshot schema version.
pub const LIVE_SCHEMA_VERSION: u64 = 1;
/// Raw observation schema identity (adapter output / fixture input).
pub const RAW_SCHEMA_NAME: &str = "module_train_live_raw.v1";
/// The module train authority issue referenced by the identity block.
pub const TRAIN_AUTHORITY_ISSUE: u64 = 11625;
/// Expected base branch while every manifest `stack_relation` is `none`.
pub const EXPECTED_BASE_MAIN: &str = "main";

/// Bounded observation limits (ceilings; hitting one marks the instrument
/// `truncated`, which is `not_proven` for binding decisions, never "complete").
pub const OPEN_PR_LIMIT: usize = 100;
pub const MERGED_PR_WINDOW: usize = 100;
pub const MAX_DETAIL_VIEWS: usize = 40;
pub const MAX_CHECK_NAMES_PER_BUCKET: usize = 50;
pub const MAX_DIRTY_SAMPLE: usize = 50;
pub const MAX_STORED_TITLE: usize = 160;

// ---------------------------------------------------------------------------
// Read-only subprocess choke point. Every external invocation in this module
// routes through `run_observation` / `run_git_ancestry`; the allowlist below
// is the single inventory of observation commands and is asserted read-only by
// tests (issue shift-left falsifier 18).
// ---------------------------------------------------------------------------

/// Allowed first words for `git` observation subcommands.
const GIT_READ_ONLY_FIRST: [&str; 7] =
    ["rev-parse", "status", "for-each-ref", "ls-remote", "merge-base", "worktree", "remote"];

/// Returns true when the exact git argument list is a read-only observation.
fn git_args_read_only(args: &[&str]) -> bool {
    let Some(first) = args.first() else {
        return false;
    };
    if !GIT_READ_ONLY_FIRST.contains(first) {
        return false;
    }
    match *first {
        "worktree" => matches!(args.get(1), Some(&"list")),
        "remote" => matches!(args.get(1), Some(&"get-url")),
        // rev-parse, status, for-each-ref, ls-remote and merge-base are
        // read-only in every argument shape; status must never carry
        // mutation flags (it has none) and merge-base never writes.
        _ => true,
    }
}

/// Returns true when the exact `gh` argument list is a read-only observation.
fn gh_args_read_only(args: &[&str]) -> bool {
    matches!(args.first(), Some(&"pr")) && matches!(args.get(1), Some(&"list") | Some(&"view"))
}

fn args_read_only(program: &str, args: &[&str]) -> bool {
    match program {
        "git" => git_args_read_only(args),
        "gh" => gh_args_read_only(args),
        _ => false,
    }
}

/// The single inventory of observation command shapes (test surface for the
/// read-only law). Each entry is `program args...` up to the discriminating
/// prefix. Consumed by tests; kept out of dead-code analysis for the bin
/// target.
#[allow(dead_code)]
pub fn observation_command_inventory() -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    for verb in GIT_READ_ONLY_FIRST {
        match verb {
            "worktree" => entries.push("git worktree list".to_string()),
            "remote" => entries.push("git remote get-url <remote>".to_string()),
            other => entries.push(format!("git {other} …")),
        }
    }
    entries.push("gh pr list --json … --limit <n>".to_string());
    entries.push("gh pr view <n> --json …".to_string());
    entries
}

struct ObservationFailure {
    program: String,
    args: Vec<String>,
    stderr: String,
}

impl std::fmt::Display for ObservationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} failed: {}", self.program, self.args.join(" "), self.stderr)
    }
}

/// Run one allowlisted observation command. Any non-allowlisted invocation
/// fails closed before spawning: mutation is structurally unreachable.
fn run_observation(
    root: Option<&Path>,
    program: &str,
    args: &[&str],
) -> std::result::Result<String, ObservationFailure> {
    if !args_read_only(program, args) {
        return Err(ObservationFailure {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            stderr: format!(
                "rejected non-read-only observation candidate: {program} {}",
                args.join(" ")
            ),
        });
    }
    let mut command = std::process::Command::new(program);
    command.args(args);
    if let Some(dir) = root {
        command.current_dir(dir);
    }
    let output = command.output().map_err(|error| ObservationFailure {
        program: program.to_string(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        stderr: format!("failed to spawn: {error}"),
    })?;
    if !output.status.success() {
        return Err(ObservationFailure {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    String::from_utf8(output.stdout).map(|text| text.trim().to_string()).map_err(|error| {
        ObservationFailure {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            stderr: format!("non-UTF-8 output: {error}"),
        }
    })
}

/// Tri-state result of `git merge-base --is-ancestor <a> HEAD`: exit 0 = yes,
/// exit 1 = no, anything else = the probe failed (unknown commit, corrupt
/// object store, …) and must never be read as a definite "no".
#[derive(Debug)]
enum Ancestry {
    Yes,
    No,
    ProbeFailed(String),
}

/// Ancestry probe. This is the one place besides `run_observation` that
/// spawns a subprocess (it needs exit-code-1 as data, which the string
/// adapter treats as failure), so it validates its exact argument list against
/// the same read-only allowlist and the probed oid's shape (git never
/// shell-interprets arguments, but a malformed oid has no business reaching
/// the object-store probe at all). No ungated command path exists here.
fn run_git_ancestry(root: &Path, oid: &str) -> Ancestry {
    let args = ["merge-base", "--is-ancestor", oid, "HEAD"];
    if !args_read_only("git", &args) {
        return Ancestry::ProbeFailed(format!(
            "rejected non-read-only ancestry candidate: git {}",
            args.join(" ")
        ));
    }
    if oid.is_empty() || !oid.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Ancestry::ProbeFailed(format!(
            "rejected non-hex ancestry oid (never spawned): {oid:?}"
        ));
    }
    let output = std::process::Command::new("git").args(args).current_dir(root).output();
    match output {
        Ok(output) if output.status.success() => Ancestry::Yes,
        // Documented git contract: exit 1 is the definite "not an ancestor"
        // answer for commits present in the object store.
        Ok(output) if output.status.code() == Some(1) => Ancestry::No,
        Ok(output) => Ancestry::ProbeFailed(format!(
            "git merge-base --is-ancestor {oid} HEAD exited {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => Ancestry::ProbeFailed(format!("failed to spawn git merge-base: {error}")),
    }
}

// ---------------------------------------------------------------------------
// Instrument model.
// ---------------------------------------------------------------------------

/// Typed instrument state. Anything other than `Ok` is a failure of
/// observation, never evidence of absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentState {
    Ok,
    Failed,
    RateLimited,
    PermissionDenied,
    Truncated,
    Unavailable,
}

impl InstrumentState {
    fn as_str(self) -> &'static str {
        match self {
            InstrumentState::Ok => "ok",
            InstrumentState::Failed => "failed",
            InstrumentState::RateLimited => "rate_limited",
            InstrumentState::PermissionDenied => "permission_denied",
            InstrumentState::Truncated => "truncated",
            InstrumentState::Unavailable => "unavailable",
        }
    }

    fn is_ok(self) -> bool {
        self == InstrumentState::Ok
    }

    /// Classify a `gh` failure payload. The detail string is always preserved
    /// verbatim alongside the classification.
    fn from_failure_text(text: &str) -> InstrumentState {
        let lowered = text.to_ascii_lowercase();
        if lowered.contains("rate limit") {
            InstrumentState::RateLimited
        } else if lowered.contains("http 403")
            || lowered.contains("http 401")
            || lowered.contains("permission")
        {
            InstrumentState::PermissionDenied
        } else if lowered.contains("http 404")
            || lowered.contains("could not resolve")
            || lowered.contains("no such host")
        {
            InstrumentState::Unavailable
        } else {
            InstrumentState::Failed
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentRecord {
    source: String,
    state: InstrumentState,
    #[serde(default)]
    detail: String,
}

// ---------------------------------------------------------------------------
// Raw observation model (adapter output and fixture input share this shape).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct RawObservation {
    pub schema: String,
    pub observed_at: String,
    #[serde(default)]
    pub repository: RawRepository,
    #[serde(default)]
    pub git_local: RawGitLocal,
    #[serde(default)]
    pub git_remote: RawGitRemote,
    #[serde(default)]
    pub github: RawGithub,
    #[serde(default)]
    pub instruments: RawInstruments,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawRepository {
    pub owner: Option<String>,
    pub name: Option<String>,
    pub default_branch: Option<String>,
    pub observed_main_sha: Option<String>,
    pub observed_main_source: Option<String>,
    pub local_origin_main_sha: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawGitLocal {
    pub head: Option<String>,
    pub branch: Option<String>,
    #[serde(default)]
    pub dirty_paths: Vec<String>,
    #[serde(default)]
    pub manifest_dirty: bool,
    #[serde(default)]
    pub branches: Vec<RawBranch>,
    #[serde(default)]
    pub worktrees: Vec<RawWorktree>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawBranch {
    pub name: String,
    pub sha: String,
    pub upstream: Option<String>,
    pub ahead: Option<u64>,
    pub behind: Option<u64>,
    /// The upstream remote ref was deleted: local-only commits may be unique
    /// work, so the branch counts as unpushed (never disposable).
    #[serde(default)]
    pub upstream_gone: bool,
    pub worktree_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawWorktree {
    pub path: String,
    pub head: String,
    pub branch: Option<String>,
    pub detached: bool,
    pub locked: bool,
    pub dirty_paths: usize,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawGitRemote {
    #[serde(default)]
    pub refs: Vec<RawRemoteRef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawRemoteRef {
    pub name: String,
    pub sha: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawGithub {
    /// Module-associated PR records only (the adapter drops unrelated PRs;
    /// normalization re-checks association so fixtures cannot smuggle data).
    #[serde(default)]
    pub prs: Vec<RawPr>,
    #[serde(default)]
    pub open_observed: usize,
    #[serde(default)]
    pub merged_observed: usize,
    /// The OPEN window hit its limit: absence of a viable (open) candidate is
    /// not provable, which gates every candidate-absence decision.
    #[serde(default)]
    pub open_truncated: bool,
    /// The MERGED window hit its limit: merged-candidate facts beyond the
    /// window are not provable (recorded limitation); open-candidate facts
    /// and viability are unaffected.
    #[serde(default)]
    pub merged_truncated: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawPr {
    pub number: u64,
    pub state: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author_login: String,
    #[serde(default)]
    pub base_ref: String,
    #[serde(default)]
    pub head_ref: String,
    #[serde(default)]
    pub head_oid: String,
    #[serde(default)]
    pub mergeable: String,
    #[serde(default)]
    pub merged_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub review_decision: Option<String>,
    #[serde(default)]
    pub reviews: Option<Vec<RawReview>>,
    #[serde(default)]
    pub checks: Option<Vec<RawCheck>>,
    #[serde(default)]
    pub merge_commit_oid: Option<String>,
    #[serde(default)]
    pub merge_commit_in_local_head: Option<bool>,
    /// Transient: consumed for the identity block, then dropped. Never stored
    /// in the snapshot (privacy/bounded payload law).
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawReview {
    #[serde(default)]
    pub author_login: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawCheck {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub conclusion: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawInstruments {
    pub git_local: Option<InstrumentRecord>,
    pub git_remote: Option<InstrumentRecord>,
    pub github_prs: Option<InstrumentRecord>,
}

// ---------------------------------------------------------------------------
// Snapshot model (immutable, deterministic; `observed_at` stays outside the
// semantic digest).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveSnapshot {
    pub schema: String,
    pub schema_version: u64,
    pub observed_at: String,
    pub semantic: Semantic,
    pub semantic_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Semantic {
    pub repository: RepositoryFacts,
    pub train: TrainFacts,
    pub instruments: InstrumentsFacts,
    pub git: GitFacts,
    pub github: GithubFacts,
    pub nodes: Vec<NodeLive>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryFacts {
    pub owner: Option<String>,
    pub name: Option<String>,
    pub default_branch: Option<String>,
    pub observed_main_sha: Option<String>,
    pub observed_main_source: Option<String>,
    pub local_origin_main_sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainFacts {
    pub manifest_digest: String,
    pub c02_nodes: BTreeMap<String, C02NodeSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct C02NodeSummary {
    pub state: String,
    pub implementation_presence: String,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentsFacts {
    pub git_local: InstrumentRecord,
    pub git_remote: InstrumentRecord,
    pub github_prs: InstrumentRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFacts {
    pub head: Option<String>,
    pub branch: Option<String>,
    pub dirty_paths: usize,
    #[serde(default)]
    pub dirty_sample: Vec<String>,
    pub manifest_dirty: bool,
    pub branches: Vec<BranchFacts>,
    pub worktrees: Vec<WorktreeFacts>,
    pub remote_refs: Vec<RemoteRefFacts>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchFacts {
    pub name: String,
    pub sha: String,
    pub upstream: Option<String>,
    pub ahead: Option<u64>,
    pub behind: Option<u64>,
    pub worktree_path: Option<String>,
    /// Diagnostic association only: never ownership authority.
    pub associated_nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeFacts {
    pub path: String,
    pub head: String,
    pub branch: Option<String>,
    pub detached: bool,
    pub locked: bool,
    pub dirty_paths: usize,
    pub associated_nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteRefFacts {
    pub name: String,
    pub sha: String,
    pub associated_nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubFacts {
    /// Bounded records for module-associated PRs, sorted by number. Bodies are
    /// never stored.
    pub prs: Vec<PrFacts>,
    /// PRs carrying module-train trailers that failed binding agreement.
    pub misbound_prs: Vec<MisboundPr>,
    pub open_observed: usize,
    pub merged_observed: usize,
    /// Open-window truncation impairs absence-of-candidate proof (global
    /// not_proven gate). Merged-window truncation only degrades merged facts
    /// beyond the window (per-node limitation).
    #[serde(default)]
    pub open_truncated: bool,
    #[serde(default)]
    pub merged_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrFacts {
    pub number: u64,
    pub state: String,
    pub draft: bool,
    pub title: String,
    pub author_login: String,
    pub base_ref: String,
    pub head_ref: String,
    pub head_oid: String,
    pub mergeable: String,
    pub merged_at: Option<String>,
    pub updated_at: Option<String>,
    pub review_decision: Option<String>,
    pub latest_reviews: Vec<ReviewFacts>,
    pub checks: ChecksFacts,
    pub merge_commit_oid: Option<String>,
    pub merge_commit_in_local_head: Option<bool>,
    pub binding: BindingFacts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFacts {
    pub author_login: String,
    pub state: String,
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksFacts {
    pub success: usize,
    pub failed: usize,
    pub pending: usize,
    pub other: usize,
    /// Cancelled runs carry no verdict (superseded executions): neither a
    /// failure nor a wait signal, surfaced as a limitation.
    #[serde(default)]
    pub cancelled: usize,
    #[serde(default)]
    pub failed_names: Vec<String>,
    #[serde(default)]
    pub pending_names: Vec<String>,
    pub names_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingFacts {
    pub bound: bool,
    pub node_id: Option<String>,
    pub node_issue: Option<u64>,
    pub controller_issue: Option<u64>,
    /// Typed agreement verdicts for the identity block.
    pub train_authority_ok: bool,
    pub node_known: bool,
    pub controller_ok: bool,
    pub base_ok: bool,
    pub role_implementation_capable: bool,
    #[serde(default)]
    pub misbound_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MisboundPr {
    pub number: u64,
    pub state: String,
    pub head_ref: String,
    /// The node the identity block named, when known (agreement failed).
    pub node_id: Option<String>,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceFacts {
    pub kind: String,
    pub name: String,
    pub sha: String,
    pub worktree_path: Option<String>,
    pub dirty: bool,
    pub unpushed: bool,
    /// Association is name-diagnostic only and never ownership authority.
    pub association: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLive {
    pub node_id: String,
    pub issue: u64,
    pub role: String,
    pub lane: String,
    pub conflict_key: String,
    pub parallel_group: String,
    pub c02_state: String,
    #[serde(default)]
    pub c02_reasons: Vec<String>,
    /// Independent candidate-state flags (closed vocabulary).
    #[serde(default)]
    pub candidate_flags: Vec<String>,
    /// Bound candidates (canonical/multiple), sorted by number.
    #[serde(default)]
    pub candidates: Vec<PrFacts>,
    /// Name-associated local/remote surfaces.
    #[serde(default)]
    pub surfaces: Vec<SurfaceFacts>,
    pub action: String,
    #[serde(default)]
    pub action_reasons: Vec<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

// ---------------------------------------------------------------------------
// Action + candidate-state vocabularies.
// ---------------------------------------------------------------------------

/// The closed safe-action vocabulary (exactly one per node / writer-conflict
/// surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Resume,
    Repair,
    Restack,
    Review,
    Wait,
    MergeReadyRecommendation,
    SupersedeRecommended,
    Reconcile,
    ReturnToIssue,
    Blocked,
    NotProven,
    Stop,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Start => "START",
            Action::Resume => "RESUME",
            Action::Repair => "REPAIR",
            Action::Restack => "RESTACK",
            Action::Review => "REVIEW",
            Action::Wait => "WAIT",
            Action::MergeReadyRecommendation => "MERGE_READY_RECOMMENDATION",
            Action::SupersedeRecommended => "SUPERSEDE_RECOMMENDED",
            Action::Reconcile => "RECONCILE",
            Action::ReturnToIssue => "RETURN_TO_ISSUE",
            Action::Blocked => "BLOCKED",
            Action::NotProven => "NOT_PROVEN",
            Action::Stop => "STOP",
        }
    }

    fn from_str(text: &str) -> Option<Action> {
        Some(match text {
            "START" => Action::Start,
            "RESUME" => Action::Resume,
            "REPAIR" => Action::Repair,
            "RESTACK" => Action::Restack,
            "REVIEW" => Action::Review,
            "WAIT" => Action::Wait,
            "MERGE_READY_RECOMMENDATION" => Action::MergeReadyRecommendation,
            "SUPERSEDE_RECOMMENDED" => Action::SupersedeRecommended,
            "RECONCILE" => Action::Reconcile,
            "RETURN_TO_ISSUE" => Action::ReturnToIssue,
            "BLOCKED" => Action::Blocked,
            "NOT_PROVEN" => Action::NotProven,
            "STOP" => Action::Stop,
            _ => return None,
        })
    }
}

/// Closed candidate-state vocabulary (issue law; flags stay independent).
/// Every flag emitted anywhere in this module must appear here.
pub const CANDIDATE_STATES: [&str; 28] = [
    "absent",
    "local_worktree",
    "remote_branch",
    "canonical_candidate",
    "explicit_stack_member",
    "salvage_source",
    "multiple_candidates",
    "duplicate_candidate",
    "misbound_candidate",
    "controller_candidate",
    "conflict_key_collision",
    "stale_base",
    "wrong_dependency_or_stack_relation",
    "head_moved_after_proof",
    "head_moved_after_review",
    "checks_pending",
    "checks_failed",
    "review_pending",
    "review_changes_requested",
    "review_threads_open",
    "behavior_receipt_missing_or_noncurrent",
    "merged_candidate_pending_current_tree_probe",
    "merged_current_tree",
    "closed_without_merge",
    "superseded",
    "dirty_or_unpushed_unique_work",
    "instrument_failed",
    "not_proven",
];
// `explicit_stack_member`, `conflict_key_collision`, `head_moved_after_proof`,
// `review_threads_open`, `behavior_receipt_missing_or_noncurrent` and
// `superseded` are reserved vocabulary: this slice has no instrument that can
// observe them, so they are never emitted and never guessed.

/// Roles that can never own an implementation candidate.
const ROLE_NEVER_IMPLEMENTATION: [&str; 4] = ["controller", "fan_in", "external_gate", "claim"];

fn role_never_implementation(role: &str) -> bool {
    ROLE_NEVER_IMPLEMENTATION.contains(&role)
}

// ---------------------------------------------------------------------------
// Identity-block parsing (the only ownership authority).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct IdentityBlock {
    train_issue: u64,
    node_issue: u64,
    controller_issue: u64,
}

/// Extract the machine-checkable identity block from a PR body. Accepts the
/// exact three keys with optional leading markdown bullets/bold and inline
/// code spans; values must be explicit `#<digits>` issue references. Nothing
/// else binds (no title matching, no branch guessing).
fn parse_identity_block(body: &str) -> Option<IdentityBlock> {
    let mut train: Option<u64> = None;
    let mut node: Option<u64> = None;
    let mut controller: Option<u64> = None;
    for raw_line in body.lines() {
        let line = raw_line.trim().trim_start_matches(['-', '*', ' ']).trim();
        let line = line.trim_start_matches("**").trim_end_matches("**").trim();
        let value = |key: &str| -> Option<u64> {
            let (name, rest) = line.split_once(':')?;
            if name.trim() != key {
                return None;
            }
            let token = rest
                .trim()
                .trim_start_matches(['`', '*', ' '])
                .trim_end_matches(['`', '*', ' '])
                .trim();
            let digits = token.strip_prefix('#')?.trim();
            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            digits.parse::<u64>().ok()
        };
        if let Some(issue) = value("Module train") {
            train = Some(issue);
        } else if let Some(issue) = value("Module node") {
            node = Some(issue);
        } else if let Some(issue) = value("Parent/controller") {
            controller = Some(issue);
        }
    }
    Some(IdentityBlock { train_issue: train?, node_issue: node?, controller_issue: controller? })
}

/// Extract only the `Module node: #<issue>` trailer. Used when the full
/// identity block is incomplete: the named node must still see the claim
/// (misbound, needs a bounded ownership decision) even though the block as a
/// whole cannot bind.
fn parse_node_issue_trailer(body: &str) -> Option<u64> {
    for raw_line in body.lines() {
        let line = raw_line.trim().trim_start_matches(['-', '*', ' ']).trim();
        let line = line.trim_start_matches("**").trim_end_matches("**").trim();
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if name.trim() != "Module node" {
            continue;
        }
        let token = rest
            .trim()
            .trim_start_matches(['`', '*', ' '])
            .trim_end_matches(['`', '*', ' '])
            .trim();
        let digits = token.strip_prefix('#')?.trim();
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            return digits.parse::<u64>().ok();
        }
        return None;
    }
    None
}

// ---------------------------------------------------------------------------
// Name association (diagnostics only; never ownership).
// ---------------------------------------------------------------------------

fn normalize_token(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    out
}

fn association_tokens(fact: &NodeStaticFact) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    tokens.insert(normalize_token(&fact.node_id));
    tokens.insert(fact.issue.to_string());
    let conflict_tail =
        fact.conflict_key.rsplit('.').next().map(normalize_token).unwrap_or_default();
    if !conflict_tail.is_empty() {
        tokens.insert(conflict_tail);
    }
    for alias in &fact.aliases {
        let normalized = normalize_token(alias);
        if !normalized.is_empty() {
            tokens.insert(normalized);
        }
    }
    tokens
}

/// All nodes whose name tokens associate with a surface name. Purely a
/// diagnostic: association never establishes candidate ownership.
fn surface_associations(surface: &str, tokens: &BTreeMap<String, BTreeSet<String>>) -> Vec<String> {
    let normalized_surface = normalize_token(surface);
    tokens
        .iter()
        .filter(|(_, node_tokens)| {
            node_tokens
                .iter()
                .any(|token| !token.is_empty() && normalized_surface.contains(token.as_str()))
        })
        .map(|(node_id, _)| node_id.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Network adapters (refresh network mode only; all read-only).
// ---------------------------------------------------------------------------

fn instrument_ok(source: &str) -> InstrumentRecord {
    InstrumentRecord {
        source: source.to_string(),
        state: InstrumentState::Ok,
        detail: String::new(),
    }
}

fn instrument_from_failure(source: &str, failure: &ObservationFailure) -> InstrumentRecord {
    InstrumentRecord {
        source: source.to_string(),
        state: InstrumentState::from_failure_text(&failure.stderr),
        detail: failure.to_string(),
    }
}

fn parse_tracking(text: &str) -> (Option<u64>, Option<u64>, bool) {
    // `%(upstream:track)` renders like "ahead 2", "behind 3",
    // "ahead 2, behind 1", or "gone" (upstream ref deleted).
    let mut ahead: Option<u64> = None;
    let mut behind: Option<u64> = None;
    let mut gone = false;
    for part in text.split(',') {
        let part = part.trim();
        if let Some(number) = part.strip_prefix("ahead ") {
            ahead = number.trim().parse::<u64>().ok();
        } else if let Some(number) = part.strip_prefix("behind ") {
            behind = number.trim().parse::<u64>().ok();
        } else if part == "gone" {
            gone = true;
        }
    }
    (ahead, behind, gone)
}

fn observe_git_local(root: &Path) -> (RawGitLocal, InstrumentRecord, Option<String>) {
    let source = "git rev-parse/status/for-each-ref/worktree list (local, read-only)";
    let fail = |failure: ObservationFailure| {
        (RawGitLocal::default(), instrument_from_failure(source, &failure), None)
    };
    let head = match run_observation(Some(root), "git", &["rev-parse", "HEAD"]) {
        Ok(text) => text,
        Err(failure) => return fail(failure),
    };
    let branch = match run_observation(Some(root), "git", &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(text) => Some(text),
        Err(failure) => return fail(failure),
    };
    let status = match run_observation(Some(root), "git", &["status", "--porcelain"]) {
        Ok(text) => text,
        Err(failure) => return fail(failure),
    };
    let dirty_paths: Vec<String> =
        status.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect();
    let manifest_dirty = match run_observation(
        Some(root),
        "git",
        &["status", "--porcelain", "--", super::module_train::MANIFEST_RELATIVE_PATH],
    ) {
        Ok(text) => !text.trim().is_empty(),
        Err(failure) => return fail(failure),
    };
    let refs = match run_observation(
        Some(root),
        "git",
        &[
            "for-each-ref",
            "--format=%(refname:short)|%(objectname)|%(upstream:short)|%(upstream:track)",
            "refs/heads/",
        ],
    ) {
        Ok(text) => text,
        Err(failure) => return fail(failure),
    };
    let mut branches: Vec<RawBranch> = Vec::new();
    for line in refs.lines() {
        let mut parts = line.splitn(4, '|');
        let (Some(name), Some(sha)) = (parts.next(), parts.next()) else {
            continue;
        };
        let upstream_raw = parts.next().unwrap_or("").trim();
        let track_raw = parts.next().unwrap_or("").trim();
        let (ahead, behind, upstream_gone) = parse_tracking(track_raw);
        branches.push(RawBranch {
            name: name.to_string(),
            sha: sha.to_string(),
            upstream: (!upstream_raw.is_empty()).then(|| upstream_raw.to_string()),
            ahead,
            behind,
            upstream_gone,
            worktree_path: None,
        });
    }
    let worktree_list =
        match run_observation(Some(root), "git", &["worktree", "list", "--porcelain"]) {
            Ok(text) => text,
            Err(failure) => return fail(failure),
        };
    let mut worktrees: Vec<RawWorktree> = Vec::new();
    let mut current: Option<(String, String, Option<String>, bool, bool)> = None;
    for line in worktree_list.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some((path, head, branch, detached, locked)) = current.take() {
                worktrees.push(worktree_record(&path, &head, branch, detached, locked));
            }
            current = Some((path.to_string(), String::new(), None, false, false));
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };
        if let Some(value) = line.strip_prefix("HEAD ") {
            entry.1 = value.to_string();
        } else if let Some(value) = line.strip_prefix("branch ") {
            entry.2 = Some(value.trim_start_matches("refs/heads/").to_string());
        } else if line == "detached" {
            entry.3 = true;
        } else if line == "locked" {
            entry.4 = true;
        }
    }
    if let Some((path, head, branch, detached, locked)) = current.take() {
        worktrees.push(worktree_record(&path, &head, branch, detached, locked));
    }
    for worktree in &worktrees {
        if let Some(branch) = &worktree.branch {
            for raw in &mut branches {
                if raw.name == *branch {
                    raw.worktree_path = Some(worktree.path.clone());
                }
            }
        }
    }
    let origin_main = run_observation(Some(root), "git", &["rev-parse", "origin/main"]).ok();
    (
        RawGitLocal { head: Some(head), branch, dirty_paths, manifest_dirty, branches, worktrees },
        instrument_ok(source),
        origin_main,
    )
}

fn worktree_record(
    path: &str,
    head: &str,
    branch: Option<String>,
    detached: bool,
    locked: bool,
) -> RawWorktree {
    let dirty = run_observation(Some(Path::new(path)), "git", &["status", "--porcelain"])
        .map(|text| text.lines().filter(|line| !line.trim().is_empty()).count())
        .unwrap_or(usize::MAX);
    RawWorktree {
        path: path.to_string(),
        head: head.to_string(),
        branch,
        detached,
        locked,
        dirty_paths: dirty,
    }
}

fn observe_git_remote(
    root: &Path,
) -> (RawGitRemote, InstrumentRecord, Option<String>, Option<String>) {
    let source = "git ls-remote origin refs/heads/* (network, read-only)";
    match run_observation(Some(root), "git", &["ls-remote", "origin", "refs/heads/*"]) {
        Ok(text) => {
            let mut refs: Vec<RawRemoteRef> = Vec::new();
            let mut main_sha: Option<String> = None;
            for line in text.lines() {
                let mut parts = line.splitn(2, '\t');
                let (Some(sha), Some(reference)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let name = reference.trim().trim_start_matches("refs/heads/").to_string();
                if name == EXPECTED_BASE_MAIN {
                    main_sha = Some(sha.to_string());
                }
                refs.push(RawRemoteRef { name, sha: sha.to_string() });
            }
            refs.sort_by(|a, b| a.name.cmp(&b.name));
            (RawGitRemote { refs }, instrument_ok(source), main_sha, None)
        }
        Err(failure) => {
            let record = instrument_from_failure(source, &failure);
            (RawGitRemote::default(), record, None, None)
        }
    }
}

fn parse_owner_name(url: &str) -> Option<(String, String)> {
    let path = if let Some(rest) = url.strip_prefix("git@github.com:") {
        rest.to_string()
    } else if let Some(rest) = url.split_once("github.com/").map(|(_, rest)| rest) {
        rest.to_string()
    } else {
        return None;
    };
    let path = path.trim_end_matches(".git");
    let mut parts = path.split('/');
    let owner = parts.next()?.to_string();
    let name = parts.next()?.to_string();
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        return None;
    }
    Some((owner, name))
}

const GH_LIST_FIELDS: &str = "number,title,state,isDraft,headRefName,baseRefName,headRefOid,author,mergeable,mergedAt,updatedAt,body";
const GH_VIEW_FIELDS: &str = "number,reviewDecision,latestReviews,statusCheckRollup,mergeCommit";

/// The `owner/name` repository selector for every `gh` query. Always the
/// checkout's own origin remote, never the ambient `GH_REPO` environment: an
/// unqualified `gh pr list` would silently observe a foreign repository while
/// the git facts still come from this checkout, which would corrupt binding.
fn gh_repo_selector(root: &Path) -> Option<String> {
    run_observation(Some(root), "git", &["remote", "get-url", "origin"])
        .ok()
        .and_then(|url| parse_owner_name(&url))
        .map(|(owner, name)| format!("{owner}/{name}"))
}

/// Argument lists for the gh observation commands, separated for tests: the
/// repository selector must appear in every list.
fn gh_list_args(state: &str, limit: usize, repo: &str) -> Vec<String> {
    vec![
        "pr".to_string(),
        "list".to_string(),
        "--repo".to_string(),
        repo.to_string(),
        "--state".to_string(),
        state.to_string(),
        "--limit".to_string(),
        limit.to_string(),
        "--json".to_string(),
        GH_LIST_FIELDS.to_string(),
    ]
}

fn gh_view_args(number: u64, repo: &str) -> Vec<String> {
    vec![
        "pr".to_string(),
        "view".to_string(),
        number.to_string(),
        "--repo".to_string(),
        repo.to_string(),
        "--json".to_string(),
        GH_VIEW_FIELDS.to_string(),
    ]
}

fn run_gh(root: &Path, args: &[String]) -> std::result::Result<String, ObservationFailure> {
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_observation(Some(root), "gh", &borrowed)
}

fn gh_list(
    root: &Path,
    state: &str,
    limit: usize,
    repo: &str,
) -> std::result::Result<Vec<Value>, ObservationFailure> {
    let text = run_gh(root, &gh_list_args(state, limit, repo))?;
    let value: Value = serde_json::from_str(&text).map_err(|error| ObservationFailure {
        program: "gh".into(),
        args: vec![format!("pr list --state {state} (malformed response)")],
        stderr: format!("malformed JSON response: {error}"),
    })?;
    Ok(value.as_array().cloned().unwrap_or_default())
}

fn gh_view(root: &Path, number: u64, repo: &str) -> std::result::Result<Value, ObservationFailure> {
    let text = run_gh(root, &gh_view_args(number, repo))?;
    serde_json::from_str(&text).map_err(|error| ObservationFailure {
        program: "gh".into(),
        args: vec![format!("pr view {number} (malformed response)")],
        stderr: format!("malformed JSON response: {error}"),
    })
}

fn string_field(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn opt_string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn raw_pr_from_list(value: &Value) -> RawPr {
    RawPr {
        number: value.get("number").and_then(Value::as_u64).unwrap_or_default(),
        state: string_field(value, "state"),
        draft: value.get("isDraft").and_then(Value::as_bool).unwrap_or_default(),
        title: string_field(value, "title"),
        author_login: value
            .get("author")
            .and_then(|author| author.get("login"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        base_ref: string_field(value, "baseRefName"),
        head_ref: string_field(value, "headRefName"),
        head_oid: string_field(value, "headRefOid"),
        mergeable: string_field(value, "mergeable"),
        merged_at: opt_string_field(value, "mergedAt"),
        updated_at: opt_string_field(value, "updatedAt"),
        review_decision: None,
        reviews: None,
        checks: None,
        merge_commit_oid: None,
        merge_commit_in_local_head: None,
        body: opt_string_field(value, "body"),
    }
}

fn observe_github(root: &Path) -> (RawGithub, InstrumentRecord) {
    let source = format!(
        "gh pr list --state open --limit {OPEN_PR_LIMIT} + --state merged --limit {MERGED_PR_WINDOW} + bounded gh pr view"
    );
    let Some(repo) = gh_repo_selector(root) else {
        return (
            RawGithub::default(),
            InstrumentRecord {
                source,
                state: InstrumentState::Failed,
                detail: "origin remote did not resolve to owner/name; refusing \
                         unqualified gh queries that GH_REPO could redirect to a foreign repository"
                    .to_string(),
            },
        );
    };
    let open_values = match gh_list(root, "open", OPEN_PR_LIMIT, &repo) {
        Ok(values) => values,
        Err(failure) => return (RawGithub::default(), instrument_from_failure(&source, &failure)),
    };
    let merged_values = match gh_list(root, "merged", MERGED_PR_WINDOW, &repo) {
        Ok(values) => values,
        Err(failure) => return (RawGithub::default(), instrument_from_failure(&source, &failure)),
    };
    let mut state = InstrumentState::Ok;
    let mut detail = String::new();
    let open_truncated = open_values.len() >= OPEN_PR_LIMIT;
    let merged_truncated = merged_values.len() >= MERGED_PR_WINDOW;
    if open_truncated {
        // Absence of a viable (open) candidate is not provable: global gate.
        state = InstrumentState::Truncated;
        let _ = write!(detail, "open PR window hit its limit ({OPEN_PR_LIMIT}); ");
    }
    if merged_truncated {
        // Bounded history: merged facts beyond the window degrade to a
        // recorded limitation, never to "no merged candidate".
        let _ = write!(detail, "merged PR window hit its limit ({MERGED_PR_WINDOW}); ");
    }
    let mut prs: Vec<RawPr> = Vec::new();
    let mut detail_views = 0usize;
    for value in open_values.iter().chain(merged_values.iter()) {
        let mut raw = raw_pr_from_list(value);
        let Some(body) = raw.body.clone() else {
            continue;
        };
        if !body.contains("Module train:") && !body.contains("Module node:") {
            // Unrelated PR: never stored (bounded/private-safe payload law).
            continue;
        }
        if detail_views < MAX_DETAIL_VIEWS {
            match gh_view(root, raw.number, &repo) {
                Ok(view) => {
                    raw.review_decision = opt_string_field(&view, "reviewDecision");
                    raw.reviews =
                        view.get("latestReviews").and_then(Value::as_array).map(|reviews| {
                            reviews
                                .iter()
                                .map(|review| RawReview {
                                    author_login: review
                                        .get("author")
                                        .and_then(|author| author.get("login"))
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string(),
                                    state: string_field(review, "state"),
                                    submitted_at: opt_string_field(review, "submittedAt"),
                                })
                                .collect()
                        });
                    raw.checks =
                        view.get("statusCheckRollup").and_then(Value::as_array).map(|checks| {
                            checks
                                .iter()
                                .map(|check| RawCheck {
                                    name: string_field(check, "name"),
                                    status: string_field(check, "status"),
                                    conclusion: string_field(check, "conclusion"),
                                })
                                .collect()
                        });
                    raw.merge_commit_oid = view
                        .get("mergeCommit")
                        .and_then(|commit| commit.get("oid"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
                // A failed detail read (rate limit, permission, malformed
                // response) must never silently downgrade to "no reviews, no
                // checks": the instrument fails and the whole projection
                // gates to NOT_PROVEN instead of misclassifying.
                Err(failure) => {
                    state = InstrumentState::from_failure_text(&failure.stderr);
                    let _ = write!(detail, "pr view {} failed: {failure}; ", raw.number);
                }
            }
            detail_views += 1;
        } else {
            state = InstrumentState::Truncated;
            let _ =
                write!(detail, "module-associated PR detail views capped at {MAX_DETAIL_VIEWS}; ");
        }
        if raw.state == "MERGED" {
            match &raw.merge_commit_oid {
                Some(oid) => match run_git_ancestry(root, oid) {
                    Ancestry::Yes => raw.merge_commit_in_local_head = Some(true),
                    Ancestry::No => raw.merge_commit_in_local_head = Some(false),
                    Ancestry::ProbeFailed(reason) => {
                        raw.merge_commit_in_local_head = None;
                        let _ =
                            write!(detail, "PR {} ancestry probe failed: {reason}; ", raw.number);
                    }
                },
                None => raw.merge_commit_in_local_head = None,
            }
        }
        prs.push(raw);
    }
    prs.sort_by_key(|pr| pr.number);
    (
        RawGithub {
            prs,
            open_observed: open_values.len(),
            merged_observed: merged_values.len(),
            open_truncated,
            merged_truncated,
        },
        InstrumentRecord { source, state, detail },
    )
}

fn now_rfc3339() -> String {
    // Observation provenance only: this value stays outside the semantic
    // digest, so no clock precision is load-bearing for determinism.
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| format!("unix:{}", since.as_secs()))
        .unwrap_or_else(|error| format!("unix:unavailable:{error}"))
}

/// Observe live state (network mode). Every instrument failure is captured in
/// the raw observation; nothing here mutates repository or GitHub state.
pub fn observe_live(root: &Path) -> RawObservation {
    let (git_local, git_local_instrument, local_origin_main) = observe_git_local(root);
    let (git_remote, git_remote_instrument, remote_main, _) = observe_git_remote(root);
    let (github, github_instrument) = observe_github(root);
    let (owner, name) = run_observation(Some(root), "git", &["remote", "get-url", "origin"])
        .ok()
        .and_then(|url| parse_owner_name(&url))
        .map(|(owner, name)| (Some(owner), Some(name)))
        .unwrap_or((None, None));
    RawObservation {
        schema: RAW_SCHEMA_NAME.to_string(),
        observed_at: now_rfc3339(),
        repository: RawRepository {
            owner,
            name,
            default_branch: Some(EXPECTED_BASE_MAIN.to_string()),
            observed_main_sha: remote_main,
            observed_main_source: Some("git ls-remote origin refs/heads/main".to_string()),
            local_origin_main_sha: local_origin_main,
        },
        git_local,
        git_remote,
        github,
        instruments: RawInstruments {
            git_local: Some(git_local_instrument),
            git_remote: Some(git_remote_instrument),
            github_prs: Some(github_instrument),
        },
    }
}

// ---------------------------------------------------------------------------
// Normalization: raw observation + manifest -> immutable snapshot. Pure and
// deterministic (same raw bytes in, same snapshot bytes out).
// ---------------------------------------------------------------------------

fn checks_facts(raw: &RawPr) -> ChecksFacts {
    let mut facts = ChecksFacts {
        success: 0,
        failed: 0,
        pending: 0,
        other: 0,
        cancelled: 0,
        failed_names: Vec::new(),
        pending_names: Vec::new(),
        names_truncated: false,
    };
    let Some(checks) = &raw.checks else {
        return facts;
    };
    for check in checks {
        let name = check.name.trim();
        match (check.status.as_str(), check.conclusion.as_str()) {
            ("COMPLETED", "SUCCESS") => facts.success += 1,
            // CANCELLED carries no verdict (a run superseded by a newer
            // execution): it must not direct the node to REPAIR. It lands in
            // the no-verdict bucket with a visible marker.
            ("COMPLETED", "CANCELLED") => facts.cancelled += 1,
            ("COMPLETED", "FAILURE" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE") => {
                facts.failed += 1;
                if facts.failed_names.len() < MAX_CHECK_NAMES_PER_BUCKET {
                    facts.failed_names.push(name.to_string());
                } else {
                    facts.names_truncated = true;
                }
            }
            ("QUEUED", _)
            | ("IN_PROGRESS", _)
            | ("WAITING", _)
            | ("PENDING", _)
            | ("REQUESTED", _) => {
                facts.pending += 1;
                if facts.pending_names.len() < MAX_CHECK_NAMES_PER_BUCKET {
                    facts.pending_names.push(name.to_string());
                } else {
                    facts.names_truncated = true;
                }
            }
            _ => facts.other += 1,
        }
    }
    facts
}

fn binding_facts(
    raw: &RawPr,
    loaded: &LoadedManifest,
    static_by_issue: &BTreeMap<u64, NodeStaticFact>,
) -> (Option<String>, BindingFacts) {
    let mut binding = BindingFacts {
        bound: false,
        node_id: None,
        node_issue: None,
        controller_issue: None,
        train_authority_ok: false,
        node_known: false,
        controller_ok: false,
        base_ok: false,
        role_implementation_capable: false,
        misbound_reasons: Vec::new(),
    };
    let body = raw.body.as_deref().unwrap_or_default();
    let Some(block) = parse_identity_block(body) else {
        binding.misbound_reasons.push("identity_block_missing_or_malformed".to_string());
        // An incomplete block still names a node in most cases: retain the
        // association so the node surfaces the claim (RECONCILE) instead of
        // silently STARTing duplicate work. No association is invented when
        // even the node trailer is absent or names an unknown issue.
        if let Some(node_issue) = parse_node_issue_trailer(body) {
            binding.node_issue = Some(node_issue);
            if let Some(fact) = static_by_issue.get(&node_issue) {
                binding.node_known = true;
                binding.node_id = Some(fact.node_id.clone());
            }
        }
        return (binding.node_id.clone(), binding);
    };
    binding.node_issue = Some(block.node_issue);
    binding.controller_issue = Some(block.controller_issue);
    binding.train_authority_ok = block.train_issue == TRAIN_AUTHORITY_ISSUE;
    if !binding.train_authority_ok {
        binding.misbound_reasons.push(format!("train_authority_mismatch:{}", block.train_issue));
    }
    let Some(fact) = static_by_issue.get(&block.node_issue) else {
        binding.misbound_reasons.push(format!("node_issue_unknown:#{}", block.node_issue));
        return (None, binding);
    };
    binding.node_known = true;
    binding.node_id = Some(fact.node_id.clone());
    let expected_controller = loaded.controller_issue(&fact.chain_controller);
    binding.controller_ok = expected_controller == Some(block.controller_issue);
    if !binding.controller_ok {
        binding.misbound_reasons.push(format!("controller_mismatch:#{}", block.controller_issue));
    }
    binding.role_implementation_capable = !role_never_implementation(&fact.role);
    if !binding.role_implementation_capable {
        binding.misbound_reasons.push(format!("role_not_implementation_capable:{}", fact.role));
    }
    if fact.stack_relation == "none" {
        binding.base_ok = raw.base_ref == EXPECTED_BASE_MAIN;
        if !binding.base_ok {
            binding
                .misbound_reasons
                .push(format!("wrong_dependency_or_stack_relation:base={}", raw.base_ref));
        }
    } else {
        // Reserved vocabulary: explicit stack parsing is not defined in this
        // slice; a non-`none` stack relation fails closed rather than guessing.
        binding.base_ok = false;
        binding.misbound_reasons.push(format!("stack_relation_unparsed:{}", fact.stack_relation));
    }
    binding.bound = binding.train_authority_ok
        && binding.node_known
        && binding.controller_ok
        && binding.base_ok;
    (binding.node_id.clone(), binding)
}

/// The pure classification input for one node.
#[derive(Debug, Clone, Default)]
pub struct NodeFacts {
    pub role: String,
    pub buildable: bool,
    pub c02_state: String,
    pub c02_reasons: Vec<String>,
    /// Bound open candidates, sorted by number.
    pub open_bound: Vec<CandidateView>,
    /// Bound merged candidates, sorted by number.
    pub merged_bound: Vec<CandidateView>,
    /// Bound closed-without-merge candidates, sorted by number.
    pub closed_bound: Vec<CandidateView>,
    /// Module-trailer PRs whose identity block names this node but failed
    /// binding agreement (wrong base, controller mismatch, …). They can never
    /// be silently ignored while the node STARTs a duplicate.
    pub misbound_refs: Vec<MisboundRef>,
    /// Name-associated local/remote surfaces (diagnostics only).
    pub surfaces: Vec<SurfaceView>,
    /// Hard-dependency node ids that currently own a nonterminal bound
    /// candidate (a live fact the offline C02 projection cannot see).
    pub hard_dep_nonterminal: Vec<String>,
    pub git_local_ok: bool,
    pub github_ok: bool,
    pub git_remote_ok: bool,
    /// Merged-window truncation: merged-candidate facts beyond the bounded
    /// window stay not_proven (limitation), without gating viability.
    pub merged_window_truncated: bool,
}

/// A misbound trailer PR referencing this node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MisboundRef {
    pub number: u64,
    pub reasons: Vec<String>,
}

/// Bounded view of one bound candidate used by the classifier.
#[derive(Debug, Clone, Default)]
pub struct CandidateView {
    pub number: u64,
    pub draft: bool,
    pub mergeable: String,
    pub review_decision: String,
    pub has_reviews: bool,
    pub checks_failed: bool,
    pub checks_pending: bool,
    /// A cancelled check run exists: no verdict, surfaced as a limitation so
    /// the recommendation never reads it as green or failed.
    pub checks_cancelled: bool,
    /// `Some(true)` = merge commit is an ancestor of the observed local HEAD;
    /// `Some(false)` = definitively not; `None` = probe unavailable.
    pub merged_in_local_head: Option<bool>,
    pub head_oid: String,
    // Synthetic merge-ready inputs (unobservable live; unit-test surface).
    pub review_on_head: Option<bool>,
    pub threads_resolved: Option<bool>,
    pub core_receipt_pass: Option<bool>,
    pub edit_profile_pass: Option<bool>,
    pub exact_process_receipt_pass: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct SurfaceView {
    pub kind: String,
    pub name: String,
    pub dirty: bool,
    pub unpushed: bool,
}

/// The classified output for one node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedNode {
    pub action: Action,
    pub reasons: Vec<String>,
    pub limitations: Vec<String>,
    pub flags: Vec<String>,
}

/// The pure, deterministic action law. Exactly one action per node; every
/// branch records typed reason codes; unavailable observation is a typed
/// blocker, never absence and never pass.
pub fn classify(facts: &NodeFacts) -> ClassifiedNode {
    let mut reasons: BTreeSet<String> = BTreeSet::new();
    let mut limitations: BTreeSet<String> = BTreeSet::new();
    let mut flags: BTreeSet<String> = BTreeSet::new();

    // 0. Instrument health is the outermost gate: a failed observation
    //    instrument can never support "no candidate" or any pass-shaped
    //    action.
    if !facts.git_local_ok {
        flags.insert("instrument_failed".to_string());
        flags.insert("not_proven".to_string());
        reasons.insert("instrument_git_local_failed".to_string());
        return ClassifiedNode {
            action: Action::NotProven,
            reasons: reasons.into_iter().collect(),
            limitations: limitations.into_iter().collect(),
            flags: flags.into_iter().collect(),
        };
    }
    if !facts.github_ok {
        flags.insert("instrument_failed".to_string());
        flags.insert("not_proven".to_string());
        reasons.insert("instrument_github_failed".to_string());
        return ClassifiedNode {
            action: Action::NotProven,
            reasons: reasons.into_iter().collect(),
            limitations: limitations.into_iter().collect(),
            flags: flags.into_iter().collect(),
        };
    }
    if !facts.git_remote_ok {
        limitations.insert("git_remote_observation_failed_remote_facts_not_proven".to_string());
    }
    if facts.merged_window_truncated {
        limitations.insert("merged_window_truncated_merged_facts_not_proven".to_string());
    }

    let never_implementation = role_never_implementation(&facts.role);
    let bound_total = facts.open_bound.len() + facts.merged_bound.len() + facts.closed_bound.len();

    // 1. A controller/fan-in/gate/claim bound (or named by a trailer PR while
    //    violating agreement) as an implementation candidate is STOP,
    //    regardless of anything else it looks like.
    if never_implementation && (bound_total > 0 || !facts.misbound_refs.is_empty()) {
        flags.insert("controller_candidate".to_string());
        reasons.insert("controller_selected_as_implementation".to_string());
        return ClassifiedNode {
            action: Action::Stop,
            reasons: reasons.into_iter().collect(),
            limitations: limitations.into_iter().collect(),
            flags: flags.into_iter().collect(),
        };
    }

    // 2. Candidate multiplicity is RECONCILE; candidates are never ranked by
    //    recency, author, model, CI colour or diff size.
    if bound_total > 1 {
        flags.insert("multiple_candidates".to_string());
        reasons.insert("multiple_bound_candidates_need_bounded_ownership_decision".to_string());
        let mut heads: BTreeSet<&str> = BTreeSet::new();
        let mut duplicate = false;
        for candidate in
            facts.open_bound.iter().chain(facts.merged_bound.iter()).chain(&facts.closed_bound)
        {
            if !heads.insert(candidate.head_oid.as_str()) {
                duplicate = true;
            }
        }
        if duplicate {
            flags.insert("duplicate_candidate".to_string());
            reasons.insert("same_head_bound_more_than_once".to_string());
        }
        let mut numbers: Vec<String> = facts
            .open_bound
            .iter()
            .chain(facts.merged_bound.iter())
            .chain(&facts.closed_bound)
            .map(|candidate| format!("#{}", candidate.number))
            .collect();
        numbers.sort();
        reasons.insert(format!("bound_candidates:{}", numbers.join(",")));
        return ClassifiedNode {
            action: Action::Reconcile,
            reasons: reasons.into_iter().collect(),
            limitations: limitations.into_iter().collect(),
            flags: flags.into_iter().collect(),
        };
    }

    // 3. A trailer PR that names this node but fails binding agreement can
    //    never be silently ignored while the node STARTs fresh duplicate work.
    if !facts.misbound_refs.is_empty() {
        flags.insert("misbound_candidate".to_string());
        for reference in &facts.misbound_refs {
            for reason in &reference.reasons {
                if reason.starts_with("wrong_dependency_or_stack_relation") {
                    flags.insert("wrong_dependency_or_stack_relation".to_string());
                }
            }
            reasons.insert(format!("misbound_candidate_pr:#{}", reference.number));
        }
        reasons.insert("binding_agreement_failed_needs_bounded_ownership_decision".to_string());
        return ClassifiedNode {
            action: Action::Reconcile,
            reasons: reasons.into_iter().collect(),
            limitations: limitations.into_iter().collect(),
            flags: flags.into_iter().collect(),
        };
    }

    if bound_total == 1 {
        // Exactly one bound candidate: classify it, never duplicate-START it.
        if let Some(candidate) = facts.open_bound.first() {
            flags.insert("canonical_candidate".to_string());
            if candidate.checks_pending {
                flags.insert("checks_pending".to_string());
            }
            if candidate.checks_failed {
                flags.insert("checks_failed".to_string());
            }
            if candidate.review_decision == "CHANGES_REQUESTED" {
                flags.insert("review_changes_requested".to_string());
            } else if candidate.review_decision.is_empty() || !candidate.has_reviews {
                flags.insert("review_pending".to_string());
            }
            if candidate.mergeable == "CONFLICTING" {
                flags.insert("stale_base".to_string());
            }
            // Ordered action law for the single open candidate.
            if candidate.mergeable == "CONFLICTING" {
                reasons.insert("base_conflict_with_current_base".to_string());
                return finish(Action::Restack, reasons, limitations, flags);
            }
            if candidate.review_decision == "CHANGES_REQUESTED" {
                reasons.insert("review_changes_requested".to_string());
                return finish(Action::Repair, reasons, limitations, flags);
            }
            if candidate.checks_failed {
                reasons.insert("checks_failed".to_string());
                return finish(Action::Repair, reasons, limitations, flags);
            }
            if candidate.checks_pending {
                reasons.insert("checks_pending_external_event".to_string());
                return finish(Action::Wait, reasons, limitations, flags);
            }
            if candidate.checks_cancelled {
                limitations.insert("checks_cancelled_no_verdict_recorded".to_string());
            }
            if candidate.draft {
                reasons.insert("candidate_in_draft".to_string());
                return finish(Action::Resume, reasons, limitations, flags);
            }
            if candidate.review_decision == "APPROVED" {
                // Merge-ready requires complete current facts. Review-head
                // currency, thread resolution and receipts are unobservable
                // live: typed blockers, never a green-light.
                limitations.insert("review_head_currency_not_observable".to_string());
                limitations.insert("review_threads_not_observable".to_string());
                limitations.insert("behavior_receipts_not_observable".to_string());
                match (
                    candidate.review_on_head,
                    candidate.threads_resolved,
                    candidate.core_receipt_pass,
                    candidate.edit_profile_pass,
                ) {
                    (Some(true), Some(true), Some(true), Some(true)) => {
                        reasons.insert("merge_ready_facts_complete".to_string());
                        return finish(
                            Action::MergeReadyRecommendation,
                            reasons,
                            limitations,
                            flags,
                        );
                    }
                    _ => {
                        flags.insert("not_proven".to_string());
                        reasons.insert("merge_ready_facts_incomplete".to_string());
                        if candidate.review_on_head == Some(false) {
                            flags.insert("head_moved_after_review".to_string());
                            reasons.insert("review_not_on_current_head".to_string());
                        }
                        if candidate.threads_resolved == Some(false) {
                            reasons.insert("review_threads_unresolved".to_string());
                        }
                        if candidate.core_receipt_pass == Some(true)
                            && candidate.edit_profile_pass != Some(true)
                        {
                            reasons.insert(
                                "core_receipt_cannot_hide_edit_profile_non_pass".to_string(),
                            );
                        }
                        if candidate.exact_process_receipt_pass == Some(true)
                            && candidate.edit_profile_pass != Some(true)
                        {
                            reasons.insert(
                                "exact_process_receipt_is_not_broader_support_truth".to_string(),
                            );
                        }
                        return finish(Action::NotProven, reasons, limitations, flags);
                    }
                }
            }
            // No current blocking finding: review is the independent next
            // step. Proof currentness is the writer's local fact; this
            // recommendation records that limitation honestly.
            if candidate.has_reviews {
                reasons.insert("review_head_currency_not_proven".to_string());
                limitations.insert("review_head_currency_not_observable".to_string());
                limitations.insert("review_threads_not_observable".to_string());
            } else {
                reasons.insert("review_pending".to_string());
            }
            limitations.insert("proof_currentness_unobserved".to_string());
            return finish(Action::Review, reasons, limitations, flags);
        }
        if let Some(candidate) = facts.merged_bound.first() {
            match candidate.merged_in_local_head {
                Some(true) => {
                    flags.insert("merged_current_tree".to_string());
                    reasons.insert("landed_current_tree_no_writer_action".to_string());
                    return finish(Action::Wait, reasons, limitations, flags);
                }
                Some(false) => {
                    flags.insert("merged_candidate_pending_current_tree_probe".to_string());
                    reasons.insert("merge_commit_not_ancestor_of_observed_head".to_string());
                    return finish(Action::Wait, reasons, limitations, flags);
                }
                None => {
                    flags.insert("merged_candidate_pending_current_tree_probe".to_string());
                    flags.insert("not_proven".to_string());
                    reasons.insert("merge_ancestry_probe_unavailable".to_string());
                    return finish(Action::Wait, reasons, limitations, flags);
                }
            }
        }
        if let Some(_candidate) = facts.closed_bound.first() {
            flags.insert("closed_without_merge".to_string());
            flags.insert("salvage_source".to_string());
            reasons.insert("closed_candidate_unique_work_needs_salvage_decision".to_string());
            return finish(Action::Reconcile, reasons, limitations, flags);
        }
    }

    // 3. No bound candidate. Fan-in surfaces carry an unobservable child
    // receipt obligation and fall through to their own gate below; every
    // other grouping/authorization surface has no implementation start.
    if (never_implementation && facts.role != "fan_in") || !facts.buildable {
        reasons.insert("grouping_or_authorization_surface_no_implementation_start".to_string());
        return finish(Action::Wait, reasons, limitations, flags);
    }
    match facts.c02_state.as_str() {
        "blocked_hard" | "blocked_evidence" | "blocked_external_or_authorization" => {
            flags.insert("absent".to_string());
            for reason in &facts.c02_reasons {
                reasons.insert(reason.clone());
            }
            finish(Action::Blocked, reasons, limitations, flags)
        }
        "ready" => {
            // Fan-in starts require child behavior receipts, which this slice
            // cannot observe: fail closed instead of guessing.
            if facts.role == "fan_in" {
                flags.insert("not_proven".to_string());
                limitations.insert("behavior_receipts_not_observable".to_string());
                reasons.insert("child_receipts_not_observable".to_string());
                return finish(Action::NotProven, reasons, limitations, flags);
            }
            // A hard dependency with a live nonterminal candidate owns the
            // surface.
            if !facts.hard_dep_nonterminal.is_empty() {
                for dep in &facts.hard_dep_nonterminal {
                    reasons.insert(format!("hard_dep_candidate_nonterminal:{dep}"));
                }
                return finish(Action::Wait, reasons, limitations, flags);
            }
            // Dirty/unpushed/unique local work is never disposable.
            let unique_surfaces: Vec<&SurfaceView> =
                facts.surfaces.iter().filter(|surface| surface.dirty || surface.unpushed).collect();
            if !unique_surfaces.is_empty() {
                flags.insert("dirty_or_unpushed_unique_work".to_string());
                for surface in &unique_surfaces {
                    reasons
                        .insert(format!("unique_work_surface:{}:{}", surface.kind, surface.name));
                }
                return finish(Action::Reconcile, reasons, limitations, flags);
            }
            if !facts.surfaces.is_empty() {
                for surface in &facts.surfaces {
                    flags.insert(if surface.kind == "remote_branch" {
                        "remote_branch".to_string()
                    } else {
                        "local_worktree".to_string()
                    });
                    reasons.insert(format!(
                        "unbound_associated_surface:{}:{}",
                        surface.kind, surface.name
                    ));
                }
                return finish(Action::Reconcile, reasons, limitations, flags);
            }
            flags.insert("absent".to_string());
            reasons.insert("c02_ready_no_viable_candidate_surface_available".to_string());
            finish(Action::Start, reasons, limitations, flags)
        }
        "landed_current_tree" => {
            reasons.insert("landed_current_tree_no_writer_action".to_string());
            finish(Action::Wait, reasons, limitations, flags)
        }
        _ => {
            flags.insert("not_proven".to_string());
            if facts.role == "fan_in" {
                // The projection's own state vocabulary keeps role-blocked
                // fan-in nodes unactionable; the receipt gate is stated too.
                limitations.insert("behavior_receipts_not_observable".to_string());
                reasons.insert("child_receipts_not_observable".to_string());
            }
            reasons.insert(format!("c02_state_not_actionable:{}", facts.c02_state));
            finish(Action::NotProven, reasons, limitations, flags)
        }
    }
}

fn finish(
    action: Action,
    reasons: BTreeSet<String>,
    limitations: BTreeSet<String>,
    flags: BTreeSet<String>,
) -> ClassifiedNode {
    ClassifiedNode {
        action,
        reasons: reasons.into_iter().collect(),
        limitations: limitations.into_iter().collect(),
        flags: flags.into_iter().collect(),
    }
}

fn candidate_view(pr: &PrFacts) -> CandidateView {
    CandidateView {
        number: pr.number,
        draft: pr.draft,
        mergeable: pr.mergeable.clone(),
        review_decision: pr.review_decision.clone().unwrap_or_default(),
        has_reviews: !pr.latest_reviews.is_empty(),
        checks_failed: pr.checks.failed > 0,
        checks_pending: pr.checks.pending > 0,
        checks_cancelled: pr.checks.cancelled > 0,
        merged_in_local_head: pr.merge_commit_in_local_head,
        head_oid: pr.head_oid.clone(),
        review_on_head: None,
        threads_resolved: None,
        core_receipt_pass: None,
        edit_profile_pass: None,
        exact_process_receipt_pass: None,
    }
}

/// Normalize a raw observation into the immutable deterministic snapshot.
pub fn normalize(raw: &RawObservation, loaded: &LoadedManifest) -> Result<LiveSnapshot> {
    if raw.schema != RAW_SCHEMA_NAME {
        bail!("raw observation schema mismatch: expected {RAW_SCHEMA_NAME}, found {}", raw.schema);
    }
    let statuses = loaded.node_statuses()?;
    let static_facts = loaded.node_static_facts();
    let static_by_issue: BTreeMap<u64, NodeStaticFact> =
        static_facts.iter().map(|fact| (fact.issue, fact.clone())).collect();
    let tokens: BTreeMap<String, BTreeSet<String>> =
        static_facts.iter().map(|fact| (fact.node_id.clone(), association_tokens(fact))).collect();

    // GitHub records: bounded, association-checked, sorted.
    let mut prs: Vec<PrFacts> = Vec::new();
    let mut misbound: Vec<MisboundPr> = Vec::new();
    for raw_pr in &raw.github.prs {
        let body = raw_pr.body.as_deref().unwrap_or_default();
        if !body.contains("Module train:") && !body.contains("Module node:") {
            // Fixtures cannot smuggle unrelated PRs into node classification.
            continue;
        }
        let (node_id, binding) = binding_facts(raw_pr, loaded, &static_by_issue);
        let pr = PrFacts {
            number: raw_pr.number,
            state: raw_pr.state.clone(),
            draft: raw_pr.draft,
            title: {
                let mut title = raw_pr.title.clone();
                if title.chars().count() > MAX_STORED_TITLE {
                    title = title.chars().take(MAX_STORED_TITLE).collect();
                }
                title
            },
            author_login: raw_pr.author_login.clone(),
            base_ref: raw_pr.base_ref.clone(),
            head_ref: raw_pr.head_ref.clone(),
            head_oid: raw_pr.head_oid.clone(),
            mergeable: raw_pr.mergeable.clone(),
            merged_at: raw_pr.merged_at.clone(),
            updated_at: raw_pr.updated_at.clone(),
            review_decision: raw_pr.review_decision.clone(),
            latest_reviews: raw_pr
                .reviews
                .as_ref()
                .map(|reviews| {
                    reviews
                        .iter()
                        .map(|review| ReviewFacts {
                            author_login: review.author_login.clone(),
                            state: review.state.clone(),
                            submitted_at: review.submitted_at.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            checks: checks_facts(raw_pr),
            merge_commit_oid: raw_pr.merge_commit_oid.clone(),
            merge_commit_in_local_head: raw_pr.merge_commit_in_local_head,
            binding,
        };
        match node_id.filter(|_| pr.binding.bound) {
            Some(_) => prs.push(pr),
            None => misbound.push(MisboundPr {
                number: pr.number,
                state: pr.state.clone(),
                head_ref: pr.head_ref.clone(),
                node_id: pr.binding.node_id.clone(),
                reasons: pr.binding.misbound_reasons.clone(),
            }),
        }
    }
    prs.sort_by_key(|pr| pr.number);
    misbound.sort_by_key(|pr| pr.number);

    // Bound candidates per node id, and misbound trailer PRs per named node.
    let mut bound_by_node: BTreeMap<String, Vec<&PrFacts>> = BTreeMap::new();
    for pr in &prs {
        if let Some(node_id) = &pr.binding.node_id {
            bound_by_node.entry(node_id.clone()).or_default().push(pr);
        }
    }
    let mut misbound_by_node: BTreeMap<String, Vec<MisboundRef>> = BTreeMap::new();
    for pr in &misbound {
        // node_id is set whenever the identity block named a known node, even
        // when agreement failed: the PR still claims that node.
        if let Some(node_id) = &pr.node_id {
            misbound_by_node
                .entry(node_id.clone())
                .or_default()
                .push(MisboundRef { number: pr.number, reasons: pr.reasons.clone() });
        }
    }
    // Nonterminal (open) bound candidates per node id, for dependents.
    let open_by_node: BTreeSet<String> = bound_by_node
        .iter()
        .filter(|(_, list)| list.iter().any(|pr| pr.state == "OPEN"))
        .map(|(node_id, _)| node_id.clone())
        .collect();

    let git_local_ok =
        raw.instruments.git_local.as_ref().map(|record| record.state.is_ok()).unwrap_or(false);
    let git_remote_ok =
        raw.instruments.git_remote.as_ref().map(|record| record.state.is_ok()).unwrap_or(false);
    let github_ok =
        raw.instruments.github_prs.as_ref().map(|record| record.state.is_ok()).unwrap_or(false)
            && !raw.github.open_truncated;

    let mut nodes: Vec<NodeLive> = Vec::with_capacity(static_facts.len());
    for fact in &static_facts {
        let status = statuses
            .iter()
            .find(|status| status.node_id == fact.node_id)
            .ok_or_else(|| color_eyre::eyre::eyre!("C02 projection lost node {}", fact.node_id))?;
        let bound = bound_by_node.get(&fact.node_id).cloned().unwrap_or_default();
        let open_bound: Vec<&PrFacts> =
            bound.iter().filter(|pr| pr.state == "OPEN").copied().collect();
        let merged_bound: Vec<&PrFacts> =
            bound.iter().filter(|pr| pr.state == "MERGED").copied().collect();
        let closed_bound: Vec<&PrFacts> =
            bound.iter().filter(|pr| pr.state == "CLOSED").copied().collect();

        let mut surfaces: Vec<SurfaceFacts> = Vec::new();
        for branch in &raw.git_local.branches {
            if !surface_associations(&branch.name, &tokens).contains(&fact.node_id) {
                continue;
            }
            let unpushed =
                branch.upstream.is_none() || branch.upstream_gone || branch.ahead.unwrap_or(0) > 0;
            surfaces.push(SurfaceFacts {
                kind: "local_branch".to_string(),
                name: branch.name.clone(),
                sha: branch.sha.clone(),
                worktree_path: branch.worktree_path.clone(),
                dirty: branch.worktree_path.is_some()
                    && raw.git_local.worktrees.iter().any(|worktree| {
                        Some(worktree.path.as_str()) == branch.worktree_path.as_deref()
                            && worktree.dirty_paths > 0
                    }),
                unpushed,
                association: "name_diagnostic_only".to_string(),
            });
        }
        for worktree in &raw.git_local.worktrees {
            let surface_name = worktree.branch.clone().unwrap_or_else(|| worktree.path.clone());
            if !surface_associations(&surface_name, &tokens).contains(&fact.node_id) {
                continue;
            }
            surfaces.push(SurfaceFacts {
                kind: "worktree".to_string(),
                name: surface_name,
                sha: worktree.head.clone(),
                worktree_path: Some(worktree.path.clone()),
                dirty: worktree.dirty_paths > 0,
                unpushed: false,
                association: "name_diagnostic_only".to_string(),
            });
        }
        if git_remote_ok {
            for reference in &raw.git_remote.refs {
                if !surface_associations(&reference.name, &tokens).contains(&fact.node_id) {
                    continue;
                }
                surfaces.push(SurfaceFacts {
                    kind: "remote_branch".to_string(),
                    name: reference.name.clone(),
                    sha: reference.sha.clone(),
                    worktree_path: None,
                    dirty: false,
                    unpushed: false,
                    association: "name_diagnostic_only".to_string(),
                });
            }
        }
        surfaces.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));

        let hard_dep_nonterminal: Vec<String> = fact
            .dependencies
            .iter()
            .filter(|(_, class)| class == "hard")
            .map(|(target, _)| target.clone())
            .filter(|target| open_by_node.contains(target))
            .collect();
        let misbound_refs = misbound_by_node.get(&fact.node_id).cloned().unwrap_or_default();

        let node_facts = NodeFacts {
            role: fact.role.clone(),
            buildable: fact.buildable,
            c02_state: status.state.as_str().to_string(),
            c02_reasons: status.reasons.clone(),
            open_bound: open_bound.iter().map(|pr| candidate_view(pr)).collect(),
            merged_bound: merged_bound.iter().map(|pr| candidate_view(pr)).collect(),
            closed_bound: closed_bound.iter().map(|pr| candidate_view(pr)).collect(),
            misbound_refs,
            surfaces: surfaces
                .iter()
                .map(|surface| SurfaceView {
                    kind: surface.kind.clone(),
                    name: surface.name.clone(),
                    dirty: surface.dirty,
                    unpushed: surface.unpushed,
                })
                .collect(),
            hard_dep_nonterminal,
            git_local_ok,
            github_ok,
            git_remote_ok,
            merged_window_truncated: raw.github.merged_truncated,
        };
        let classified = classify(&node_facts);
        nodes.push(NodeLive {
            node_id: fact.node_id.clone(),
            issue: fact.issue,
            role: fact.role.clone(),
            lane: fact.lane.clone(),
            conflict_key: fact.conflict_key.clone(),
            parallel_group: fact.parallel_group.clone(),
            c02_state: status.state.as_str().to_string(),
            c02_reasons: status.reasons.clone(),
            candidate_flags: classified.flags,
            candidates: bound.into_iter().cloned().collect(),
            surfaces,
            action: classified.action.as_str().to_string(),
            action_reasons: classified.reasons,
            limitations: classified.limitations,
        });
    }
    nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));

    // At most one action per writer/conflict surface (manifest law: conflict
    // keys are unique per node; asserted here so a future revision cannot
    // silently double-allocate).
    let mut seen_keys: BTreeSet<&str> = BTreeSet::new();
    for node in &nodes {
        if !seen_keys.insert(node.conflict_key.as_str()) {
            bail!(
                "conflict key {} allocated by more than one node; refusing to project an ambiguous surface",
                node.conflict_key
            );
        }
    }

    let semantic = Semantic {
        repository: RepositoryFacts {
            owner: raw.repository.owner.clone(),
            name: raw.repository.name.clone(),
            default_branch: raw.repository.default_branch.clone(),
            observed_main_sha: raw.repository.observed_main_sha.clone(),
            observed_main_source: raw.repository.observed_main_source.clone(),
            local_origin_main_sha: raw.repository.local_origin_main_sha.clone(),
        },
        train: TrainFacts {
            manifest_digest: crate::tasks::module_train::PINNED_CANONICAL_DIGEST.to_string(),
            c02_nodes: statuses
                .iter()
                .map(|status| {
                    (
                        status.node_id.clone(),
                        C02NodeSummary {
                            state: status.state.as_str().to_string(),
                            implementation_presence: match status.implementation_presence {
                                ProbeOutcome::Pass => "probe:pass".to_string(),
                                ProbeOutcome::Absent => "not_proven".to_string(),
                            },
                            reasons: status.reasons.clone(),
                        },
                    )
                })
                .collect(),
        },
        instruments: InstrumentsFacts {
            git_local: raw.instruments.git_local.clone().unwrap_or_else(|| InstrumentRecord {
                source: "git local observation".to_string(),
                state: InstrumentState::Unavailable,
                detail: "raw observation carries no git-local instrument record".to_string(),
            }),
            git_remote: raw.instruments.git_remote.clone().unwrap_or_else(|| InstrumentRecord {
                source: "git remote observation".to_string(),
                state: InstrumentState::Unavailable,
                detail: "raw observation carries no git-remote instrument record".to_string(),
            }),
            github_prs: raw.instruments.github_prs.clone().unwrap_or_else(|| InstrumentRecord {
                source: "github observation".to_string(),
                state: InstrumentState::Unavailable,
                detail: "raw observation carries no github instrument record".to_string(),
            }),
        },
        git: GitFacts {
            head: raw.git_local.head.clone(),
            branch: raw.git_local.branch.clone(),
            dirty_paths: raw.git_local.dirty_paths.len(),
            dirty_sample: raw
                .git_local
                .dirty_paths
                .iter()
                .take(MAX_DIRTY_SAMPLE)
                .cloned()
                .collect(),
            manifest_dirty: raw.git_local.manifest_dirty,
            branches: raw
                .git_local
                .branches
                .iter()
                .map(|branch| BranchFacts {
                    name: branch.name.clone(),
                    sha: branch.sha.clone(),
                    upstream: branch.upstream.clone(),
                    ahead: branch.ahead,
                    behind: branch.behind,
                    worktree_path: branch.worktree_path.clone(),
                    associated_nodes: surface_associations(&branch.name, &tokens),
                })
                .collect(),
            worktrees: raw
                .git_local
                .worktrees
                .iter()
                .map(|worktree| WorktreeFacts {
                    path: worktree.path.clone(),
                    head: worktree.head.clone(),
                    branch: worktree.branch.clone(),
                    detached: worktree.detached,
                    locked: worktree.locked,
                    dirty_paths: worktree.dirty_paths,
                    associated_nodes: surface_associations(
                        &worktree.branch.clone().unwrap_or_else(|| worktree.path.clone()),
                        &tokens,
                    ),
                })
                .collect(),
            remote_refs: if git_remote_ok {
                raw.git_remote
                    .refs
                    .iter()
                    .map(|reference| RemoteRefFacts {
                        name: reference.name.clone(),
                        sha: reference.sha.clone(),
                        associated_nodes: surface_associations(&reference.name, &tokens),
                    })
                    .collect()
            } else {
                Vec::new()
            },
        },
        github: GithubFacts {
            prs,
            misbound_prs: misbound,
            open_observed: raw.github.open_observed,
            merged_observed: raw.github.merged_observed,
            open_truncated: raw.github.open_truncated,
            merged_truncated: raw.github.merged_truncated,
        },
        nodes,
    };
    let semantic_value = serde_json::to_value(&semantic)
        .with_context(|| "failed to serialize the live snapshot semantic section")?;
    let semantic_digest = canonical_digest(&semantic_value)
        .with_context(|| "failed to digest the live snapshot semantic section")?;
    Ok(LiveSnapshot {
        schema: LIVE_SCHEMA_NAME.to_string(),
        schema_version: LIVE_SCHEMA_VERSION,
        observed_at: raw.observed_at.clone(),
        semantic,
        semantic_digest,
    })
}

// ---------------------------------------------------------------------------
// Snapshot loading + validation (check/next/explain offline path).
// ---------------------------------------------------------------------------

fn load_snapshot(path: &Path) -> Result<LiveSnapshot> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read live snapshot at {}", path.display()))?;
    let snapshot: LiveSnapshot = serde_json::from_slice(&bytes).with_context(|| {
        format!("live snapshot at {} violates the strict schema", path.display())
    })?;
    if snapshot.schema != LIVE_SCHEMA_NAME {
        bail!(
            "snapshot schema mismatch at {}: expected {LIVE_SCHEMA_NAME}, found {}",
            path.display(),
            snapshot.schema
        );
    }
    if snapshot.schema_version != LIVE_SCHEMA_VERSION {
        bail!(
            "snapshot schema_version mismatch at {}: expected {LIVE_SCHEMA_VERSION}, found {}",
            path.display(),
            snapshot.schema_version
        );
    }
    let semantic_value = serde_json::to_value(&snapshot.semantic)
        .with_context(|| "failed to re-serialize the semantic section for digest verification")?;
    let digest = canonical_digest(&semantic_value)?;
    if digest != snapshot.semantic_digest {
        bail!(
            "snapshot semantic digest drift at {}: computed {digest} but stored {}",
            path.display(),
            snapshot.semantic_digest
        );
    }
    Ok(snapshot)
}

/// Full offline validation: schema, digest, vocabularies, the one-action law,
/// and consistency between the stored classification and a re-derivation from
/// the snapshot's own fact sections (tamper evidence).
pub fn validate_snapshot(snapshot: &LiveSnapshot, loaded: &LoadedManifest) -> Result<Vec<String>> {
    let mut report: Vec<String> = Vec::new();
    // The snapshot must be bound to exactly the current pinned manifest
    // revision: a foreign or stale train cannot validate here even with a
    // self-consistent digest.
    if snapshot.semantic.train.manifest_digest
        != crate::tasks::module_train::PINNED_CANONICAL_DIGEST
    {
        bail!(
            "snapshot manifest digest {} does not match the pinned module_train.v1 revision {};              re-observe against the current train",
            snapshot.semantic.train.manifest_digest,
            crate::tasks::module_train::PINNED_CANONICAL_DIGEST
        );
    }
    let static_facts = loaded.node_static_facts();
    let manifest_node_ids: BTreeSet<&str> =
        static_facts.iter().map(|fact| fact.node_id.as_str()).collect();
    let snapshot_node_ids: BTreeSet<&str> =
        snapshot.semantic.nodes.iter().map(|node| node.node_id.as_str()).collect();
    if manifest_node_ids != snapshot_node_ids {
        let missing: Vec<&str> =
            manifest_node_ids.difference(&snapshot_node_ids).copied().collect();
        let extra: Vec<&str> = snapshot_node_ids.difference(&manifest_node_ids).copied().collect();
        bail!(
            "snapshot node set disagrees with the manifest (missing: [{}], unknown: [{}]);              re-observe against the current train",
            missing.join(","),
            extra.join(",")
        );
    }
    for node in &snapshot.semantic.nodes {
        if Action::from_str(&node.action).is_none() {
            bail!("node {} carries unknown action {}", node.node_id, node.action);
        }
        if node.action_reasons.is_empty() {
            bail!("node {} carries an action without reason codes", node.node_id);
        }
        for flag in &node.candidate_flags {
            if !CANDIDATE_STATES.contains(&flag.as_str()) {
                bail!("node {} carries unknown candidate flag {}", node.node_id, flag);
            }
        }
        let mut reason_set: BTreeSet<&str> = BTreeSet::new();
        for reason in &node.action_reasons {
            if !reason_set.insert(reason.as_str()) {
                bail!("node {} carries duplicated reason {reason}", node.node_id);
            }
        }
    }
    let mut keys: BTreeSet<&str> = BTreeSet::new();
    for node in &snapshot.semantic.nodes {
        if !keys.insert(node.conflict_key.as_str()) {
            bail!("conflict key {} carries more than one action", node.conflict_key);
        }
    }

    // Rebuild classification inputs from the snapshot's fact sections and
    // compare with the stored actions.
    let static_facts = loaded.node_static_facts();
    let static_by_id: BTreeMap<String, NodeStaticFact> =
        static_facts.iter().map(|fact| (fact.node_id.clone(), fact.clone())).collect();
    let mut bound_by_node: BTreeMap<String, Vec<&PrFacts>> = BTreeMap::new();
    for pr in &snapshot.semantic.github.prs {
        if let Some(node_id) = pr.binding.node_id.as_ref().filter(|_| pr.binding.bound) {
            bound_by_node.entry(node_id.clone()).or_default().push(pr);
        }
    }
    let open_by_node: BTreeSet<String> = bound_by_node
        .iter()
        .filter(|(_, list)| list.iter().any(|pr| pr.state == "OPEN"))
        .map(|(node_id, _)| node_id.clone())
        .collect();
    for node in &snapshot.semantic.nodes {
        let Some(fact) = static_by_id.get(&node.node_id) else {
            bail!("snapshot node {} is absent from the pinned manifest", node.node_id);
        };
        if fact.conflict_key != node.conflict_key {
            bail!(
                "snapshot node {} disagrees with the manifest conflict key ({} vs {})",
                node.node_id,
                node.conflict_key,
                fact.conflict_key
            );
        }
        let bound = bound_by_node.get(&node.node_id).cloned().unwrap_or_default();
        let facts = NodeFacts {
            role: fact.role.clone(),
            buildable: fact.buildable,
            c02_state: node.c02_state.clone(),
            c02_reasons: node.c02_reasons.clone(),
            // Misbound refs already surfaced on the stored node's flags; the
            // re-derivation reads them back from the snapshot's own record.
            misbound_refs: snapshot
                .semantic
                .github
                .misbound_prs
                .iter()
                .filter(|pr| pr.node_id.as_deref() == Some(node.node_id.as_str()))
                .map(|pr| MisboundRef { number: pr.number, reasons: pr.reasons.clone() })
                .collect(),
            open_bound: bound
                .iter()
                .filter(|pr| pr.state == "OPEN")
                .map(|pr| candidate_view(pr))
                .collect(),
            merged_bound: bound
                .iter()
                .filter(|pr| pr.state == "MERGED")
                .map(|pr| candidate_view(pr))
                .collect(),
            closed_bound: bound
                .iter()
                .filter(|pr| pr.state == "CLOSED")
                .map(|pr| candidate_view(pr))
                .collect(),
            surfaces: node
                .surfaces
                .iter()
                .map(|surface| SurfaceView {
                    kind: surface.kind.clone(),
                    name: surface.name.clone(),
                    dirty: surface.dirty,
                    unpushed: surface.unpushed,
                })
                .collect(),
            hard_dep_nonterminal: hard_dep_dep_nonterminal(
                &fact.node_id,
                &open_by_node,
                &static_by_id,
            ),
            git_local_ok: snapshot.semantic.instruments.git_local.state.is_ok(),
            github_ok: snapshot.semantic.instruments.github_prs.state.is_ok()
                && !snapshot.semantic.github.open_truncated,
            git_remote_ok: snapshot.semantic.instruments.git_remote.state.is_ok(),
            merged_window_truncated: snapshot.semantic.github.merged_truncated,
        };
        let classified = classify(&facts);
        if classified.action.as_str() != node.action {
            bail!(
                "node {} stored action {} disagrees with re-derived {} (snapshot tampering or stale classifier)",
                node.node_id,
                node.action,
                classified.action.as_str()
            );
        }
        report.push(format!(
            "node {} action {} consistent ({} reasons)",
            node.node_id,
            node.action,
            node.action_reasons.len()
        ));
    }
    Ok(report)
}

fn hard_dep_dep_nonterminal(
    node_id: &str,
    open_by_node: &BTreeSet<String>,
    static_by_id: &BTreeMap<String, NodeStaticFact>,
) -> Vec<String> {
    let Some(fact) = static_by_id.get(node_id) else {
        return Vec::new();
    };
    fact.dependencies
        .iter()
        .filter(|(_, class)| class == "hard")
        .map(|(target, _)| target.clone())
        .filter(|target| open_by_node.contains(target))
        .collect()
}

// ---------------------------------------------------------------------------
// Rendering (deterministic; no timestamps, no ambient paths beyond given).
// ---------------------------------------------------------------------------

fn render_instruments(snapshot: &LiveSnapshot) -> String {
    let mut out = String::new();
    for (name, record) in [
        ("git_local", &snapshot.semantic.instruments.git_local),
        ("git_remote", &snapshot.semantic.instruments.git_remote),
        ("github_prs", &snapshot.semantic.instruments.github_prs),
    ] {
        let _ = writeln!(
            out,
            "instrument {name}: {} detail={}",
            record.state.as_str(),
            if record.detail.trim().is_empty() { "-" } else { record.detail.trim() }
        );
    }
    out
}

pub fn render_check(snapshot: &LiveSnapshot, validation: &[String]) -> String {
    let mut out = String::new();
    out.push_str("module-train live check (offline snapshot validation)\n");
    let _ = writeln!(out, "schema: {} (version {})", snapshot.schema, snapshot.schema_version);
    let _ = writeln!(out, "semantic_digest: {}", snapshot.semantic_digest);
    let _ = writeln!(out, "observed_at: {} (outside semantic digest)", snapshot.observed_at);
    let _ = writeln!(out, "manifest_digest: {}", snapshot.semantic.train.manifest_digest);
    out.push_str(&render_instruments(snapshot));
    let _ = writeln!(
        out,
        "nodes: {} prs_bound: {} prs_misbound: {}",
        snapshot.semantic.nodes.len(),
        snapshot.semantic.github.prs.len(),
        snapshot.semantic.github.misbound_prs.len()
    );
    let mut actions: BTreeMap<&str, usize> = BTreeMap::new();
    for node in &snapshot.semantic.nodes {
        *actions.entry(node.action.as_str()).or_default() += 1;
    }
    for (action, count) in &actions {
        let _ = writeln!(out, "action {action}: {count}");
    }
    out.push_str("consistency: stored actions match re-derivation from snapshot facts\n");
    for line in validation.iter().take(5) {
        let _ = writeln!(out, "  {line}");
    }
    if validation.len() > 5 {
        let _ = writeln!(out, "  … {} more consistent nodes", validation.len() - 5);
    }
    out.push_str(
        "law: instrument failure is not_proven, never absence; at most one action per conflict key\n",
    );
    out
}

pub fn render_next(snapshot: &LiveSnapshot) -> String {
    let mut out = String::new();
    out.push_str("module-train live next (read-only live frontier projection)\n");
    let _ = writeln!(out, "schema: {} (version {})", snapshot.schema, snapshot.schema_version);
    let _ = writeln!(out, "semantic_digest: {}", snapshot.semantic_digest);
    let _ = writeln!(out, "observed_at: {}", snapshot.observed_at);
    let _ = writeln!(out, "manifest_digest: {}", snapshot.semantic.train.manifest_digest);
    if let Some(main) = &snapshot.semantic.repository.observed_main_sha {
        let _ = writeln!(
            out,
            "observed_main: {main} ({})",
            snapshot.semantic.repository.observed_main_source.as_deref().unwrap_or("unrecorded")
        );
    } else {
        out.push_str("observed_main: not_proven (remote observation unavailable)\n");
    }
    out.push_str(&render_instruments(snapshot));
    let mut by_action: BTreeMap<&str, Vec<&NodeLive>> = BTreeMap::new();
    for node in &snapshot.semantic.nodes {
        by_action.entry(node.action.as_str()).or_default().push(node);
    }
    let order = [
        "START",
        "RESUME",
        "REPAIR",
        "RESTACK",
        "REVIEW",
        "MERGE_READY_RECOMMENDATION",
        "WAIT",
        "RECONCILE",
        "STOP",
        "BLOCKED",
        "SUPERSEDE_RECOMMENDED",
        "RETURN_TO_ISSUE",
        "NOT_PROVEN",
    ];
    for action in order {
        let Some(nodes) = by_action.get(action) else {
            continue;
        };
        let _ = writeln!(out, "\n{action} ({})", nodes.len());
        for node in nodes {
            let _ = writeln!(
                out,
                "  {} #{:<6} role={:<14} writer_class={} conflict_key={}",
                node.node_id, node.issue, node.role, node.parallel_group, node.conflict_key
            );
            if !node.candidate_flags.is_empty() {
                let _ = writeln!(out, "    flags: {}", node.candidate_flags.join(","));
            }
            if !node.action_reasons.is_empty() {
                let _ = writeln!(out, "    reasons: {}", node.action_reasons.join(","));
            }
            if !node.limitations.is_empty() {
                let _ = writeln!(out, "    limitations: {}", node.limitations.join(","));
            }
            for candidate in &node.candidates {
                let _ = writeln!(
                    out,
                    "    candidate #{} {} base={} head={} draft={} mergeable={} review={:?}",
                    candidate.number,
                    candidate.state,
                    candidate.base_ref,
                    &candidate.head_oid[..candidate.head_oid.len().min(8)],
                    candidate.draft,
                    candidate.mergeable,
                    candidate.review_decision
                );
            }
        }
    }
    if !snapshot.semantic.github.misbound_prs.is_empty() {
        out.push_str("\nmisbound module-trailer PRs (diagnostics; never ownership)\n");
        for pr in &snapshot.semantic.github.misbound_prs {
            let _ = writeln!(
                out,
                "  #{} {} {} [{}]",
                pr.number,
                pr.state,
                pr.head_ref,
                pr.reasons.join(",")
            );
        }
    }
    out.push_str(
        "\nlaw: exactly one action per node; at most one action per writer/conflict surface; \
         writer classes are ceilings, never quotas; observation is read-only and performs no \
         mutation; unavailable facts are typed blockers, never absence or pass\n",
    );
    out
}

pub fn render_explain(
    snapshot: &LiveSnapshot,
    loaded: &LoadedManifest,
    node_id: &str,
) -> Result<String> {
    let static_facts = loaded.node_static_facts();
    let fact = static_facts
        .iter()
        .find(|fact| fact.node_id == node_id)
        .ok_or_else(|| color_eyre::eyre::eyre!("unknown module-train node {node_id}"))?;
    let node = snapshot
        .semantic
        .nodes
        .iter()
        .find(|node| node.node_id == node_id)
        .ok_or_else(|| color_eyre::eyre::eyre!("node {node_id} missing from snapshot"))?;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "module-train live explain {node_id} (#{}) — static packet + live addendum",
        fact.issue
    );
    let _ = writeln!(
        out,
        "identity: role={} lane={} chain={}/{}",
        fact.role, fact.lane, fact.chain_home, fact.chain_controller
    );
    let _ = writeln!(out, "one_pr_outcome: {}", fact.one_pr_outcome);
    let _ = writeln!(out, "claim_ceiling: {}", fact.claim_ceiling);
    let _ = writeln!(out, "first_falsifier: {}", fact.first_falsifier);
    let _ = writeln!(out, "rollback_stop: {}", fact.rollback_stop);
    let _ = writeln!(
        out,
        "writer: conflict_key={} parallel_group={} stack_relation={}",
        fact.conflict_key, fact.parallel_group, fact.stack_relation
    );
    let deps: Vec<String> =
        fact.dependencies.iter().map(|(target, class)| format!("{target}:{class}")).collect();
    let _ = writeln!(out, "dependencies: {}", deps.join(" "));
    let _ = writeln!(out, "c02_state: {} reasons={}", node.c02_state, node.c02_reasons.join(","));
    let _ = writeln!(
        out,
        "\nlive addendum (observed_at {}; snapshot {})",
        snapshot.observed_at, snapshot.semantic_digest
    );
    if let Some(main) = &snapshot.semantic.repository.observed_main_sha {
        let _ = writeln!(out, "observed_main: {main}");
    } else {
        let _ = writeln!(out, "observed_main: not_proven (remote observation unavailable)");
    }
    for candidate in &node.candidates {
        let _ = writeln!(
            out,
            "candidate #{} {} draft={} base={} head={} mergeable={} merge_commit_in_head={:?}",
            candidate.number,
            candidate.state,
            candidate.draft,
            candidate.base_ref,
            &candidate.head_oid[..candidate.head_oid.len().min(8)],
            candidate.mergeable,
            candidate.merge_commit_in_local_head
        );
        if !candidate.latest_reviews.is_empty() {
            let reviews: Vec<String> = candidate
                .latest_reviews
                .iter()
                .map(|review| format!("{}:{}", review.author_login, review.state))
                .collect();
            let _ = writeln!(out, "  latest_reviews: {}", reviews.join(","));
        }
        let _ = writeln!(
            out,
            "  checks: success={} failed={} pending={} other={}",
            candidate.checks.success,
            candidate.checks.failed,
            candidate.checks.pending,
            candidate.checks.other
        );
    }
    for surface in &node.surfaces {
        let _ = writeln!(
            out,
            "surface {} {} dirty={} unpushed={}",
            surface.kind, surface.name, surface.dirty, surface.unpushed
        );
    }
    let _ = writeln!(out, "action: {} (why now)", node.action);
    for reason in &node.action_reasons {
        let _ = writeln!(out, "  reason: {reason}");
    }
    for flag in &node.candidate_flags {
        let _ = writeln!(out, "  state: {flag}");
    }
    for limitation in &node.limitations {
        let _ = writeln!(out, "  limitation: {limitation}");
    }
    if node.limitations.is_empty() {
        out.push_str("  limitation: none recorded\n");
    }
    out.push_str("facts unavailable and their consequence:\n");
    out.push_str("  review-head currency, review threads, behavior receipts: not observable in this slice -> merge-ready recommendations stay NOT_PROVEN, never guessed\n");
    let _ = writeln!(
        out,
        "next bounded action: {} — {}",
        node.action,
        node.action_reasons.first().map(String::as_str).unwrap_or("see reasons")
    );
    if node.candidate_flags.iter().any(|flag| flag == "merged_current_tree") {
        out.push_str("closeout route: landed on the observed tree; issue closeout may proceed under its own authority\n");
    } else {
        out.push_str("closeout route: not current; follow the action above first\n");
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// CLI entry points.
// ---------------------------------------------------------------------------

fn project_root() -> Result<PathBuf> {
    crate::utils::project_root()
}

pub fn run_refresh(output: &Path, from_fixture: Option<&Path>) -> Result<()> {
    let raw = match from_fixture {
        Some(fixture) => {
            let bytes = std::fs::read(fixture)
                .with_context(|| format!("failed to read raw fixture at {}", fixture.display()))?;
            serde_json::from_slice(&bytes).with_context(|| {
                format!("raw fixture at {} violates the raw observation schema", fixture.display())
            })?
        }
        None => {
            let root = project_root()?;
            observe_live(&root)
        }
    };
    let loaded = load_manifest()?;
    let snapshot = normalize(&raw, &loaded)?;
    if let Some(parent) = output.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create snapshot output directory {}", parent.display())
        })?;
    }
    let bytes = serde_json::to_vec_pretty(&snapshot)
        .with_context(|| "failed to serialize the live snapshot")?;
    std::fs::write(output, &bytes)
        .with_context(|| format!("failed to write live snapshot to {}", output.display()))?;
    let mut summary = String::new();
    let _ = writeln!(summary, "live snapshot written: {}", output.display());
    let _ = writeln!(summary, "schema: {} (version {})", snapshot.schema, snapshot.schema_version);
    let _ = writeln!(summary, "semantic_digest: {}", snapshot.semantic_digest);
    let _ = writeln!(summary, "observed_at: {} (outside semantic digest)", snapshot.observed_at);
    summary.push_str(&render_instruments(&snapshot));
    let _ = writeln!(
        summary,
        "nodes: {} bound_prs: {} misbound_prs: {}",
        snapshot.semantic.nodes.len(),
        snapshot.semantic.github.prs.len(),
        snapshot.semantic.github.misbound_prs.len()
    );
    print!("{summary}");
    Ok(())
}

pub fn run_check(snapshot_path: &Path) -> Result<()> {
    let snapshot = load_snapshot(snapshot_path)?;
    let loaded = load_manifest()?;
    let report = validate_snapshot(&snapshot, &loaded)?;
    print!("{}", render_check(&snapshot, &report));
    Ok(())
}

pub fn run_next(snapshot_path: &Path) -> Result<()> {
    let snapshot = load_snapshot(snapshot_path)?;
    let loaded = load_manifest()?;
    validate_snapshot(&snapshot, &loaded)?;
    print!("{}", render_next(&snapshot));
    Ok(())
}

pub fn run_explain(node: &str, snapshot_path: &Path) -> Result<()> {
    let snapshot = load_snapshot(snapshot_path)?;
    let loaded = load_manifest()?;
    validate_snapshot(&snapshot, &loaded)?;
    print!("{}", render_explain(&snapshot, &loaded, node)?);
    Ok(())
}
