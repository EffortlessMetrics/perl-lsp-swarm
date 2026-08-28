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

/// A remote's full identity: the network endpoint as well as `owner/name`.
///
/// The endpoint is scheme + host + port, not host alone.
/// `github.com/Owner/Repo` and `evil.example.com/Owner/Repo` share an
/// `owner/name`; `github.com/O/R` and `github.com:8443/O/R` share a host; and
/// `https://github.com/O/R` and `git://github.com/O/R` share both host and
/// port-less appearance while speaking to different services over different
/// transports. Any of those differences is a different endpoint, so a deletion
/// leased against one must not be redeemed against another.
///
/// Both sides of a comparison are produced by parsing the output of
/// `git remote get-url <remote>` at collection and again at verification, so
/// strict equality is exactly the intended check: it detects the remote being
/// repointed in between.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteIdentity {
    pub scheme: String,
    pub host: String,
    /// Effective port: explicit when the URL carries one, otherwise the
    /// scheme's default. `None` only for a scheme with no known default and no
    /// explicit port, where identical inputs still compare equal.
    pub port: Option<u16>,
    pub repository: RepositoryId,
}

impl RemoteIdentity {
    /// Canonical `scheme://host[:port]/owner/name`, compared exactly.
    pub fn render(&self) -> String {
        match self.port {
            Some(port) => {
                format!("{}://{}:{}/{}", self.scheme, self.host, port, self.repository.render())
            }
            None => format!("{}://{}/{}", self.scheme, self.host, self.repository.render()),
        }
    }
}

/// The default port for a git-capable scheme, so an explicit `:443` and an
/// implicit HTTPS compare equal instead of reading as two endpoints.
fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "https" => Some(443),
        "http" => Some(80),
        "ssh" => Some(22),
        "git" => Some(9418),
        "ftps" => Some(990),
        "ftp" => Some(21),
        _ => None,
    }
}

/// Parse a git remote URL into its endpoint and `owner/name`.
///
/// Handles the `scheme://[user@]host[:port]/owner/name(.git)` and the scp-like
/// `[user@]host:owner/name(.git)` forms. The scp-like form has no scheme or
/// port of its own — git speaks SSH over it — so it normalizes to `ssh` on 22.
/// Anything else is unparseable and must not be guessed at.
pub fn parse_remote_identity(url: &str) -> Option<RemoteIdentity> {
    let trimmed = url.trim().trim_end_matches('/');
    let without_suffix = trimmed.strip_suffix(".git").unwrap_or(trimmed);

    let (scheme, authority, path) = match without_suffix.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.split_once('/')?;
            (scheme.to_ascii_lowercase(), authority, path)
        }
        // scp-like: the text after ':' is a path, never a port. `host:1234/x`
        // is the path `1234/x`, which is how git itself reads it.
        None => {
            let (authority, path) = without_suffix.split_once(':')?;
            ("ssh".to_string(), authority, path)
        }
    };
    if scheme.is_empty() {
        return None;
    }

    // Drop any `user@` prefix; credentials are not identity. Compare hosts
    // case-insensitively, as DNS does.
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let (host, explicit_port) = match authority.rsplit_once(':') {
        Some((host, port_text)) => {
            // A non-numeric tail is not a port; treat the whole thing as host
            // rather than silently discarding it.
            match port_text.parse::<u16>() {
                Ok(port) => (host, Some(port)),
                Err(_) => (authority, None),
            }
        }
        None => (authority, None),
    };
    let host = host.to_ascii_lowercase();

    let (owner, name) = path.rsplit_once('/')?;
    let owner = owner.rsplit('/').next().unwrap_or(owner);
    if host.is_empty() || owner.is_empty() || name.is_empty() {
        return None;
    }

    Some(RemoteIdentity {
        port: explicit_port.or_else(|| default_port(&scheme)),
        scheme,
        host,
        repository: RepositoryId::new(owner, name),
    })
}

/// Parse just `owner/name` out of a git remote URL.
///
/// Used where the host is not the discriminator — the child graph comes from a
/// single host by construction. Remote *binding* must use
/// [`parse_remote_identity`] instead.
pub fn repository_from_remote_url(url: &str) -> Option<RepositoryId> {
    parse_remote_identity(url).map(|identity| identity.repository)
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
    /// True when the head branch lives in a fork rather than this repository.
    #[serde(rename = "isCrossRepository", default)]
    is_cross_repository: bool,
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
) -> Result<LiveCollection> {
    // Repository identity comes from the remote itself, so the admission is
    // bound to the repository the deletion would actually target rather than
    // to a caller-supplied label.
    // Both the fetch and push URLs are read here: the deletion travels over the
    // push endpoint, so admitting against the fetch endpoint alone would bind
    // the wrong thing. A divergence refuses collection outright.
    let (remote_identity, push_endpoint) = resolve_remote_endpoint(commands, remote)?;
    let repository = remote_identity.repository.clone();

    let parent_json = commands.capture(
        "gh",
        &[
            "pr",
            "view",
            &parent_number.to_string(),
            "--repo",
            &repository.render(),
            "--json",
            "number,state,merged,headRefName,headRefOid,isCrossRepository",
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
        head_in_admitted_repository: !parent.is_cross_repository,
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
                let mut unreadable: Vec<String> = Vec::new();
                let mut pull_requests: Vec<ObservedPullRequest> = Vec::new();
                for row in rows {
                    // A state this build does not recognise is not "not a
                    // child" — it is a child whose state could not be read.
                    // Dropping the row would shrink the graph and let a
                    // malformed or newer listing look complete, which is the
                    // exact permissive read #12885 forbids.
                    let Some(state) = child_state(&row.state) else {
                        unreadable.push(format!("#{} state `{}`", row.number, row.state));
                        continue;
                    };
                    pull_requests.push(ObservedPullRequest {
                        repository: repository.clone(),
                        number: row.number,
                        head_ref: row.head_ref_name,
                        base_ref: row.base_ref_name,
                        state,
                        draft: row.is_draft,
                        mergeable: child_mergeability(&row.mergeable),
                        // The host does not report this; absent is not "no".
                        mergeability_changed_by_parent_merge: None,
                    });
                }
                let completeness = if !unreadable.is_empty() {
                    GraphCompleteness::Unavailable {
                        detail: format!(
                            "open-child listing carried unreadable states: {}",
                            unreadable.join(", ")
                        ),
                    }
                } else if truncated {
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

    Ok(LiveCollection {
        request: AdmissionRequest {
            branch: collect_branch(commands, remote, &parent.head_ref_name),
            worktree_ownership: collect_worktree_ownership(commands, &parent.head_ref_name),
            parent: parent_subject,
            graph,
            remote: remote.to_string(),
            push_endpoint: Some(push_endpoint.clone()),
        },
        remote_identity,
    })
}

/// What one live collection observed.
///
/// `remote_identity` is deliberately *not* part of [`AdmissionRequest`]: the
/// request is serialisable and therefore forgeable, while the identity a
/// deletion is bound to must come from the process that actually read the
/// remote.
#[derive(Debug, Clone)]
pub struct LiveCollection {
    pub request: AdmissionRequest,
    pub remote_identity: RemoteIdentity,
}

/// Confirm the remote resolves to the repository the admission was granted
/// against.
///
/// `remote_verification_command` only *names* this check; this runs it. A
/// mismatch or an unreadable remote is an error, never a pass.
/// Resolve a remote to one endpoint, requiring its fetch and push URLs to agree.
///
/// `git remote get-url <remote>` reads the **fetch** URL, but `git push` honors
/// `remote.<name>.pushurl` when it is configured. Verified against real git
/// 2.43.0: a remote can report `github.com/Owner/Repo` for fetch and an
/// entirely different endpoint for push. Binding only the fetch URL would let
/// collection, the child graph, the branch tip and identity all verify against
/// endpoint A while the leased deletion is delivered to endpoint B.
///
/// So both are read and required to be the same endpoint. A divergence is
/// refused rather than resolved in favour of either: the caller's intent is
/// unknowable and only one of the two was ever admitted.
fn resolve_remote_endpoint(
    commands: &dyn ReadOnlyCommands,
    remote: &str,
) -> Result<(RemoteIdentity, String)> {
    let fetch_url = commands
        .capture("git", &["remote", "get-url", remote])
        .map_err(|error| eyre!("reading the fetch URL of remote {remote}: {error}"))?;
    let fetch = parse_remote_identity(&fetch_url).ok_or_else(|| {
        eyre!(
            "remote {remote} fetch URL {:?} is not an endpoint/owner/name shape",
            fetch_url.trim()
        )
    })?;

    // `--all`, because git permits MULTIPLE `remote.<name>.pushurl` entries and
    // `git push <remote>` delivers to every one of them. Without `--all`,
    // `get-url --push` reports only the first. Verified against real git 2.43.0
    // with two bare destinations: one push created the ref in both, while the
    // single-URL read named only one — so an admitted endpoint could coexist
    // with an entirely unexamined deletion endpoint.
    let push_urls = commands
        .capture("git", &["remote", "get-url", "--push", "--all", remote])
        .map_err(|error| eyre!("reading the push URLs of remote {remote}: {error}"))?;
    let push_lines: Vec<&str> =
        push_urls.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    let [push_url] = push_lines.as_slice() else {
        // Zero is unreadable; more than one is a fan-out this admission cannot
        // cover, and picking any single one of them would be a guess.
        return Err(eyre!(
            "remote {remote} has {} push URLs ({}); a deletion would be delivered to every one, and only a single admitted endpoint can be verified",
            push_lines.len(),
            push_lines.join(", ")
        ));
    };
    let push = parse_remote_identity(push_url).ok_or_else(|| {
        eyre!("remote {remote} push URL {push_url:?} is not an endpoint/owner/name shape")
    })?;

    if fetch != push {
        return Err(eyre!(
            "remote {remote} fetches from {} but pushes to {}; a deletion would be delivered to an endpoint the admission never covered",
            fetch.render(),
            push.render()
        ));
    }
    // Return the RAW verified URL as well: the deletion is executed against
    // this exact string, never against the mutable remote name.
    Ok((push, (*push_url).to_string()))
}

pub fn verify_remote_identity(
    commands: &dyn ReadOnlyCommands,
    remote: &str,
    expected: &RemoteIdentity,
) -> Result<()> {
    let (observed, _verified_url) = resolve_remote_endpoint(commands, remote)?;
    if &observed != expected {
        return Err(eyre!(
            "remote {remote} resolves to {} but the admission was granted against {}",
            observed.render(),
            expected.render()
        ));
    }
    Ok(())
}

/// The one mutating capability in this module, kept deliberately separate from
/// [`ReadOnlyCommands`] so the collection surface stays provably read-only.
///
/// Implementations run an argv vector directly — never through a shell — so a
/// branch name containing shell metacharacters cannot become a command.
pub trait DeletionExecutor {
    /// Run `argv` and return `Ok(())` only if it exited successfully.
    fn execute(&self, argv: &[String]) -> Result<()>;
}

/// Runs the deletion for real, via `Command` argv. No shell is involved.
pub struct SystemDeletion;

impl DeletionExecutor for SystemDeletion {
    fn execute(&self, argv: &[String]) -> Result<()> {
        let (program, rest) = argv.split_first().ok_or_else(|| eyre!("empty deletion command"))?;
        let status = Command::new(program)
            .args(rest)
            .status()
            .map_err(|error| eyre!("running {program}: {error}"))?;
        if !status.success() {
            return Err(eyre!("{program} exited {:?}", status.code()));
        }
        Ok(())
    }
}

/// Perform an admitted branch deletion, or refuse.
///
/// This is the only path that deletes anything, and it refuses unless *all* of
/// the following hold on the outcome it was handed:
///
/// 1. the admission is `SAFE_TO_DELETE`;
/// 2. the remote still resolves to the repository the admission was granted
///    against — re-checked here, immediately before the deletion, not merely
///    named for a human to run;
/// 3. a leased deletion command exists (an admission with no admitted tip
///    produces none).
///
/// The command is passed as argv, so shell quoting never enters the picture:
/// there is no shell. This replaces an earlier design that parsed the rendered
/// plan and ran it through `eval`, which would have executed a branch name
/// containing shell metacharacters.
pub fn execute_admitted_deletion(
    reads: &dyn ReadOnlyCommands,
    deleter: &dyn DeletionExecutor,
    outcome: &super::model::AdmissionOutcome,
    expected_remote: &RemoteIdentity,
) -> Result<()> {
    if !outcome.admission.admits_deletion() {
        return Err(eyre!(
            "refusing to delete {}: admission is {}",
            outcome.branch,
            outcome.admission.as_str()
        ));
    }

    // Bound to what collection observed — host included — not to a label that
    // travelled through the serialisable request.
    verify_remote_identity(reads, &outcome.remote, expected_remote)?;

    let argv = super::route::branch_deletion_command(outcome).ok_or_else(|| {
        eyre!("admission for {} produced no leased deletion command", outcome.branch)
    })?;
    deleter.execute(&argv)
}
