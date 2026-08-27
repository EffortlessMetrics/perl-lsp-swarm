//! Live subject collection for branch-deletion admission (#12885).
//!
//! [`evaluate`](super::evaluate) decides over a snapshot. This module *builds*
//! that snapshot from the real repository and the real pull-request graph, so
//! the admission is reachable from production rather than only from fixtures.
//!
//! Every read is fail-closed: a command that cannot run, exits non-zero, or
//! returns output this module cannot parse yields the `NOT_PROVEN` shape for
//! whatever it was reading. Nothing here mutates anything — no push, no
//! deletion, no retarget, no PR mutation. The only commands issued are
//! `git ls-remote`, `git remote get-url`, `git worktree list`, and `gh` reads.

use super::model::{
    AdmissionRequest, BranchSubject, GraphCompleteness, Mergeability, ObservedPullRequest,
    OpenChildGraph, ParentSubject, ParentTerminality, PullRequestState, RepositoryId,
    WorktreeOwnership, is_full_object_id,
};
use color_eyre::eyre::{Result, eyre};
use std::process::Command;

/// A bounded command surface, so collection can be proven without a network.
///
/// The real implementation shells out; tests supply canned output. Only
/// read-only commands are ever issued through it.
pub trait ReadOnlyCommands {
    /// Run `program` with `args`, returning stdout on success.
    ///
    /// `Err` means the command could not be run or exited non-zero — the
    /// caller turns that into a `NOT_PROVEN` shape rather than a default.
    fn capture(&self, program: &str, args: &[&str]) -> Result<String>;
}

/// Shells out for real. Used by the CLI; never by tests.
pub struct SystemCommands;

impl ReadOnlyCommands for SystemCommands {
    fn capture(&self, program: &str, args: &[&str]) -> Result<String> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|error| eyre!("running {program} {}: {error}", args.join(" ")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(eyre!(
                "{program} {} exited {:?}: {}",
                args.join(" "),
                output.status.code(),
                stderr.trim()
            ));
        }
        String::from_utf8(output.stdout)
            .map_err(|error| eyre!("{program} produced non-UTF-8 output: {error}"))
    }
}

/// Parse `owner/name` out of a git remote URL.
///
/// Handles the `https://host/owner/name(.git)` and `git@host:owner/name(.git)`
/// forms. Anything else is unparseable and must not be guessed at.
pub fn repository_from_remote_url(url: &str) -> Option<RepositoryId> {
    let trimmed = url.trim().trim_end_matches('/');
    let without_suffix = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let path = match without_suffix.split_once("://") {
        // https://host/owner/name → host/owner/name → drop the host
        Some((_scheme, rest)) => rest.split_once('/').map(|(_host, path)| path)?,
        // git@host:owner/name
        None => without_suffix.split_once(':').map(|(_host, path)| path)?,
    };
    let (owner, name) = path.rsplit_once('/')?;
    let owner = owner.rsplit('/').next().unwrap_or(owner);
    if owner.is_empty() || name.is_empty() {
        return None;
    }
    Some(RepositoryId::new(owner, name))
}

fn parent_terminality(state: &str, merged: bool) -> ParentTerminality {
    if merged {
        return ParentTerminality::Merged;
    }
    match state {
        "OPEN" => ParentTerminality::Open,
        "CLOSED" => ParentTerminality::ClosedUnmerged,
        _ => ParentTerminality::NotProven,
    }
}

fn child_state(state: &str) -> Option<PullRequestState> {
    match state {
        "OPEN" => Some(PullRequestState::Open),
        "CLOSED" => Some(PullRequestState::Closed),
        "MERGED" => Some(PullRequestState::Merged),
        _ => None,
    }
}

fn child_mergeability(mergeable: &str) -> Mergeability {
    match mergeable {
        "MERGEABLE" => Mergeability::Clean,
        "CONFLICTING" => Mergeability::Conflicting,
        // UNKNOWN means the host has not finished computing it. That is not
        // "fine" — it is unknown, and the packet must say so.
        _ => Mergeability::NotProven,
    }
}

/// Read the branch tip from the remote.
///
/// An unreadable tip, an absent ref, or a value that is not a full object id
/// all yield `None`, which `evaluate` treats as movement rather than agreement.
fn collect_branch(commands: &dyn ReadOnlyCommands, remote: &str, branch: &str) -> BranchSubject {
    let reference = format!("refs/heads/{branch}");
    let Ok(output) = commands.capture("git", &["ls-remote", remote, &reference]) else {
        return BranchSubject { current_sha: None };
    };
    let sha = output
        .lines()
        .find_map(|line| line.split_whitespace().next())
        .filter(|sha| is_full_object_id(sha))
        .map(str::to_string);
    BranchSubject { current_sha: sha }
}

/// Report whether any registered local worktree has `branch` checked out.
///
/// An unreadable worktree list is `NOT_PROVEN`, never `Clear`: #3957 owns this
/// signal and absence of evidence is not evidence of absence.
fn collect_worktree_ownership(commands: &dyn ReadOnlyCommands, branch: &str) -> WorktreeOwnership {
    let output = match commands.capture("git", &["worktree", "list", "--porcelain"]) {
        Ok(output) => output,
        Err(error) => {
            return WorktreeOwnership::NotProven { detail: error.to_string() };
        }
    };

    let wanted = format!("refs/heads/{branch}");
    let mut current_path: Option<String> = None;
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.to_string());
        }
        if let Some(reference) = line.strip_prefix("branch ")
            && reference.trim() == wanted
        {
            let detail = current_path.clone().unwrap_or_else(|| "an unnamed worktree".to_string());
            return WorktreeOwnership::ActiveWriter {
                detail: format!("{detail} has {branch} checked out"),
            };
        }
    }
    WorktreeOwnership::Clear
}

/// One row of `gh pr view --json`, kept minimal on purpose.
#[derive(serde::Deserialize)]
struct GhParent {
    number: u64,
    state: String,
    merged: bool,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    #[serde(rename = "headRefOid")]
    head_ref_oid: String,
}

#[derive(serde::Deserialize)]
struct GhChild {
    number: u64,
    state: String,
    #[serde(rename = "isDraft")]
    is_draft: bool,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    #[serde(rename = "baseRefName")]
    base_ref_name: String,
    #[serde(default)]
    mergeable: String,
}

/// The number of open children a single `gh pr list` page is asked for.
///
/// If the host returns exactly this many rows the listing may have been
/// truncated, and completeness is reported as such rather than assumed.
const CHILD_PAGE_LIMIT: usize = 100;

/// Build a live [`AdmissionRequest`] for `parent_number` on `remote`.
///
/// Fail-closed throughout. The returned request always evaluates to a
/// `RETAIN_*` outcome unless every subject was actually read: parent identity
/// and terminality, a complete open-child listing, the remote branch tip, and
/// local worktree ownership.
///
/// The caller is expected to feed the result straight to
/// [`evaluate`](super::evaluate); this function never decides anything itself.
pub fn collect_request(
    commands: &dyn ReadOnlyCommands,
    parent_number: u64,
    remote: &str,
) -> Result<AdmissionRequest> {
    // Repository identity comes from the remote itself, so the admission is
    // bound to the repository the deletion would actually target rather than
    // to a caller-supplied label.
    let remote_url = commands
        .capture("git", &["remote", "get-url", remote])
        .map_err(|error| eyre!("reading the URL of remote {remote}: {error}"))?;
    let repository = repository_from_remote_url(&remote_url).ok_or_else(|| {
        eyre!("remote {remote} URL {:?} is not owner/name shaped", remote_url.trim())
    })?;

    let parent_json = commands.capture(
        "gh",
        &[
            "pr",
            "view",
            &parent_number.to_string(),
            "--repo",
            &repository.render(),
            "--json",
            "number,state,merged,headRefName,headRefOid",
        ],
    )?;
    let parent: GhParent = serde_json::from_str(&parent_json)
        .map_err(|error| eyre!("parsing pull request #{parent_number}: {error}"))?;

    let parent_subject = ParentSubject {
        repository: repository.clone(),
        number: parent.number,
        head_ref: parent.head_ref_name.clone(),
        reviewed_head_sha: parent.head_ref_oid.clone(),
        terminality: parent_terminality(&parent.state, parent.merged),
    };

    // Open children: every PR whose base is the parent's head branch, in this
    // repository. A listing that comes back at the page limit may be
    // truncated, so completeness is reported rather than assumed.
    let graph = match commands.capture(
        "gh",
        &[
            "pr",
            "list",
            "--repo",
            &repository.render(),
            "--base",
            &parent.head_ref_name,
            "--state",
            "open",
            "--limit",
            &CHILD_PAGE_LIMIT.to_string(),
            "--json",
            "number,state,isDraft,headRefName,baseRefName,mergeable",
        ],
    ) {
        Ok(listing) => match serde_json::from_str::<Vec<GhChild>>(&listing) {
            Ok(rows) => {
                let truncated = rows.len() >= CHILD_PAGE_LIMIT;
                let pull_requests = rows
                    .into_iter()
                    .filter_map(|row| {
                        child_state(&row.state).map(|state| ObservedPullRequest {
                            repository: repository.clone(),
                            number: row.number,
                            head_ref: row.head_ref_name,
                            base_ref: row.base_ref_name,
                            state,
                            draft: row.is_draft,
                            mergeable: child_mergeability(&row.mergeable),
                            // The host does not report this; absent is not "no".
                            mergeability_changed_by_parent_merge: None,
                        })
                    })
                    .collect();
                let completeness = if truncated {
                    GraphCompleteness::Truncated {
                        detail: format!(
                            "listing returned the {CHILD_PAGE_LIMIT}-row page limit; more children may exist"
                        ),
                    }
                } else {
                    GraphCompleteness::Complete
                };
                OpenChildGraph { completeness, pull_requests }
            }
            Err(error) => OpenChildGraph {
                completeness: GraphCompleteness::Unavailable {
                    detail: format!("open-child listing did not parse: {error}"),
                },
                pull_requests: Vec::new(),
            },
        },
        Err(error) => OpenChildGraph {
            completeness: GraphCompleteness::Unavailable { detail: error.to_string() },
            pull_requests: Vec::new(),
        },
    };

    Ok(AdmissionRequest {
        branch: collect_branch(commands, remote, &parent.head_ref_name),
        worktree_ownership: collect_worktree_ownership(commands, &parent.head_ref_name),
        parent: parent_subject,
        graph,
        remote: remote.to_string(),
    })
}

/// Confirm the remote resolves to the repository the admission was granted
/// against.
///
/// `remote_verification_command` only *names* this check; this runs it. A
/// mismatch or an unreadable remote is an error, never a pass.
pub fn verify_remote_identity(
    commands: &dyn ReadOnlyCommands,
    remote: &str,
    expected: &str,
) -> Result<()> {
    let url = commands.capture("git", &["remote", "get-url", remote])?;
    let observed = repository_from_remote_url(&url)
        .ok_or_else(|| eyre!("remote {remote} URL {:?} is not owner/name shaped", url.trim()))?;
    if observed.render() != expected {
        return Err(eyre!(
            "remote {remote} resolves to {} but the admission was granted against {expected}",
            observed.render()
        ));
    }
    Ok(())
}
