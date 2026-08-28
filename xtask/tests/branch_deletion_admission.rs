//! Falsifiers for branch-deletion admission (#12885).
//!
//! Every test drives a fixture graph. Nothing here creates, retargets, closes
//! or deletes a live branch or pull request, and nothing reads the network.

use xtask::branch_deletion_admission::{
    AdmissionRequest, BranchSubject, DeletionAdmission, GraphCompleteness, Mergeability, NextOwner,
    ObservedPullRequest, OpenChildGraph, ParentSubject, ParentTerminality, PullRequestState,
    RepositoryId, WorktreeOwnership, branch_deletion_command, evaluate, merge_command,
    remote_verification_command, render_disposition,
};

const PARENT_BRANCH: &str = "agent/vim-activation-root-7762";
const REVIEWED_SHA: &str = "1111111111111111111111111111111111111111";
const OTHER_SHA: &str = "2222222222222222222222222222222222222222";
/// A realistic object id containing letters, so case sensitivity is testable.
const LETTERED_SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";

fn repository() -> RepositoryId {
    RepositoryId::new("EffortlessMetrics", "perl-lsp-swarm")
}

/// A merged parent whose branch is otherwise unencumbered: the only shape
/// that may ever reach `SAFE_TO_DELETE`. Each falsifier perturbs exactly one
/// field, so a passing test names the field that mattered.
fn admissible_request() -> AdmissionRequest {
    AdmissionRequest {
        push_endpoint: Some("https://github.com/EffortlessMetrics/perl-lsp-swarm.git".to_string()),
        parent: ParentSubject {
            repository: repository(),
            number: 7799,
            head_ref: PARENT_BRANCH.to_string(),
            reviewed_head_sha: REVIEWED_SHA.to_string(),
            terminality: ParentTerminality::Merged,
            head_in_admitted_repository: true,
        },
        branch: BranchSubject { current_sha: Some(REVIEWED_SHA.to_string()) },
        graph: OpenChildGraph {
            completeness: GraphCompleteness::Complete,
            pull_requests: Vec::new(),
        },
        worktree_ownership: WorktreeOwnership::Clear,
        remote: "origin".to_string(),
    }
}

fn child(number: u64, draft: bool, mergeable: Mergeability) -> ObservedPullRequest {
    ObservedPullRequest {
        repository: repository(),
        number,
        head_ref: format!("agent/child-{number}"),
        base_ref: PARENT_BRANCH.to_string(),
        state: PullRequestState::Open,
        draft,
        mergeable,
        mergeability_changed_by_parent_merge: None,
    }
}

/// The control. Without it, every falsifier below could pass for the trivial
/// reason that nothing is ever admitted.
#[test]
fn a_merged_unencumbered_branch_with_a_proven_empty_graph_is_admitted() {
    let outcome = evaluate(&admissible_request());
    assert_eq!(outcome.admission, DeletionAdmission::SafeToDelete, "{}", outcome.detail);
    assert!(outcome.retained_children.is_empty());
    assert_eq!(outcome.admitted_sha.as_deref(), Some(REVIEWED_SHA));
    assert_eq!(
        branch_deletion_command(&outcome),
        Some(vec![
            "git".to_string(),
            "push".to_string(),
            // The bound push URL, not the remote name: a name is mutable config
            // git would re-resolve at push time, after every check has passed.
            "https://github.com/EffortlessMetrics/perl-lsp-swarm.git".to_string(),
            format!("--force-with-lease=refs/heads/{PARENT_BRANCH}:{REVIEWED_SHA}"),
            "--delete".to_string(),
            PARENT_BRANCH.to_string(),
        ]),
        "deletion must be leased on the admitted tip",
    );
}

/// The deletion must be leased on the admitted tip, not merely issued.
///
/// `evaluate` reads the branch SHA at snapshot time; a writer can advance the
/// branch before the command runs. Verified against real git 2.43.0 in a
/// scratch repository: with the branch advanced past the admitted SHA, the
/// leased form is rejected as `! [rejected] (delete) -> feature (stale info)`
/// and the branch survives, while a plain `git push origin --delete` deletes
/// the advanced tip and exits 0. Without the lease, `RETAIN_BRANCH_MOVED` is
/// enforced only at evaluation and unsalvaged work can still be destroyed.
#[test]
fn deletion_is_leased_on_the_admitted_tip() {
    let outcome = evaluate(&admissible_request());
    let command = branch_deletion_command(&outcome).unwrap_or_default();

    let lease = command
        .iter()
        .find(|argument| argument.starts_with("--force-with-lease="))
        .map(String::as_str)
        .unwrap_or_default();
    assert_eq!(
        lease,
        format!("--force-with-lease=refs/heads/{PARENT_BRANCH}:{REVIEWED_SHA}"),
        "the lease must name the branch ref and the exact admitted SHA",
    );
    assert!(
        command.iter().any(|argument| argument == "--delete"),
        "the command must still be a deletion: {command:?}",
    );
}

/// Fail closed on a `SAFE_TO_DELETE` carrying no admitted tip: there is
/// nothing to lease against, so no command is produced rather than an
/// unleased deletion being emitted.
#[test]
fn an_admission_without_an_admitted_tip_yields_no_command() {
    let mut outcome = evaluate(&admissible_request());
    assert_eq!(outcome.admission, DeletionAdmission::SafeToDelete);
    outcome.admitted_sha = None;
    assert_eq!(
        branch_deletion_command(&outcome),
        None,
        "an unleasable admission must not produce a deletion command",
    );
}

/// No retaining outcome carries an admitted tip — the lease target exists
/// only where deletion was actually admitted.
#[test]
fn retaining_outcomes_carry_no_admitted_tip() {
    let mut retained = admissible_request();
    retained.graph.pull_requests = vec![child(7810, false, Mergeability::Clean)];
    assert_eq!(evaluate(&retained).admitted_sha, None);

    let mut moved = admissible_request();
    moved.branch.current_sha = Some(OTHER_SHA.to_string());
    assert_eq!(evaluate(&moved).admitted_sha, None);
}

/// A cross-repository (fork) parent retains: its `head_ref` names a branch in
/// the fork, so a same-named branch here is a different branch and deleting it
/// would destroy something the merge never touched.
///
/// This fails closed in most shapes anyway — the fork's branch usually does
/// not exist here, so the tip read comes back empty — but not when this
/// repository happens to hold the same name at the same SHA. That case is the
/// one this gate exists for.
#[test]
fn a_fork_parent_retains_even_when_the_name_resolves_here() {
    let mut request = admissible_request();
    request.parent.head_in_admitted_repository = false;
    // Deliberately everything else admissible: same name, same SHA, no
    // children, clean ownership. Only the fork flag stands between this and
    // SAFE_TO_DELETE.
    let outcome = evaluate(&request);
    assert_eq!(outcome.admission, DeletionAdmission::RetainBranchMoved, "{}", outcome.detail);
    assert!(outcome.detail.contains("fork"), "the detail must name the reason: {}", outcome.detail);
    assert_eq!(branch_deletion_command(&outcome), None);
}

/// Falsifier 1 — parent with two open children: the generated merge command
/// cannot contain `--delete-branch`.
#[test]
fn the_merge_command_never_carries_delete_branch_with_open_children() {
    let mut request = admissible_request();
    request.parent.terminality = ParentTerminality::Open;
    request.graph.pull_requests =
        vec![child(7810, false, Mergeability::Clean), child(7819, false, Mergeability::Clean)];

    let command = merge_command(request.parent.number, REVIEWED_SHA);
    assert!(
        !command.iter().any(|argument| argument == "--delete-branch"),
        "merge command must not delete the base branch: {command:?}",
    );
    assert_eq!(
        command,
        vec![
            "gh".to_string(),
            "pr".to_string(),
            "merge".to_string(),
            "7799".to_string(),
            "--squash".to_string(),
            "--match-head-commit".to_string(),
            REVIEWED_SHA.to_string(),
        ],
        "the canonical PLSP-SPEC-0006 form must be preserved exactly",
    );

    // Stronger than the falsifier asks: pre-merge the parent is not terminal,
    // so no admission at merge time can produce a deletion at all.
    let outcome = evaluate(&request);
    assert_eq!(outcome.admission, DeletionAdmission::RetainParentNotTerminal);
    assert_eq!(branch_deletion_command(&outcome), None);
}

/// Falsifier 2 — a draft child is still a live dependency.
#[test]
fn a_draft_child_retains_the_branch() {
    let mut request = admissible_request();
    request.graph.pull_requests = vec![child(7810, true, Mergeability::Clean)];

    let outcome = evaluate(&request);
    assert_eq!(outcome.admission, DeletionAdmission::RetainOpenChildren, "{}", outcome.detail);
    assert_eq!(outcome.retained_children.len(), 1);
    let retained = &outcome.retained_children[0];
    assert!(retained.draft, "the packet must carry the draft disposition");
    assert_eq!(retained.number, 7810);
    assert_eq!(retained.base_ref, PARENT_BRANCH);
    assert_eq!(branch_deletion_command(&outcome), None);
}

/// Falsifier 3 — a blocked or conflicting child is still a live dependency,
/// and is proposed for reconstruction rather than a plain retarget.
#[test]
fn a_blocked_or_conflicting_child_retains_the_branch() {
    for mergeable in [Mergeability::Conflicting, Mergeability::Blocked] {
        let mut request = admissible_request();
        request.graph.pull_requests = vec![child(7819, false, mergeable)];

        let outcome = evaluate(&request);
        assert_eq!(
            outcome.admission,
            DeletionAdmission::RetainOpenChildren,
            "{mergeable:?} must retain: {}",
            outcome.detail,
        );
        assert_eq!(outcome.retained_children[0].next_owner, NextOwner::Reconstruct);
    }
}

/// Falsifier 4 — retargeting a child after the parent merged does not admit
/// deletion until a live re-read actually observes zero children.
#[test]
fn deletion_stays_blocked_until_a_re_read_observes_zero_children() {
    let mut before = admissible_request();
    before.graph.pull_requests = vec![child(7810, false, Mergeability::Clean)];
    assert_eq!(evaluate(&before).admission, DeletionAdmission::RetainOpenChildren);

    // The child moved to a durable base. Re-reading the graph is the only
    // thing that changes the answer — the earlier outcome cannot be reused.
    let mut after = admissible_request();
    let mut retargeted = child(7810, false, Mergeability::Clean);
    retargeted.base_ref = "main".to_string();
    after.graph.pull_requests = vec![retargeted];

    let outcome = evaluate(&after);
    assert_eq!(outcome.admission, DeletionAdmission::SafeToDelete, "{}", outcome.detail);
}

/// Falsifier 5 — closed children do not block once the open graph is proven
/// complete.
#[test]
fn closed_children_do_not_retain_the_branch() {
    let mut request = admissible_request();
    for state in [PullRequestState::Closed, PullRequestState::Merged] {
        let mut settled = child(7810, false, Mergeability::Clean);
        settled.state = state;
        request.graph.pull_requests = vec![settled];

        let outcome = evaluate(&request);
        assert_eq!(
            outcome.admission,
            DeletionAdmission::SafeToDelete,
            "{state:?} child must not retain: {}",
            outcome.detail,
        );
    }
}

/// Falsifier 6 — a truncated or unavailable child query retains. "No children
/// returned" is not "no children exist".
#[test]
fn an_unproven_child_graph_retains_the_branch() {
    for completeness in [
        GraphCompleteness::Truncated { detail: "stopped after page 1 of 3".to_string() },
        GraphCompleteness::Unavailable { detail: "secondary rate limit".to_string() },
    ] {
        let mut request = admissible_request();
        request.graph.completeness = completeness.clone();
        // Deliberately empty: an empty page from an unfinished query is the
        // exact shape that would tempt a caller to infer safety.
        request.graph.pull_requests = Vec::new();

        let outcome = evaluate(&request);
        assert_eq!(
            outcome.admission,
            DeletionAdmission::RetainGraphNotProven,
            "{completeness:?} must retain: {}",
            outcome.detail,
        );
        assert_eq!(branch_deletion_command(&outcome), None);
    }
}

/// Falsifier 7 — a branch that advanced past the reviewed subject is refused,
/// and an unreadable tip is treated as movement rather than agreement.
#[test]
fn a_moved_or_unreadable_branch_is_refused() {
    let mut advanced = admissible_request();
    advanced.branch.current_sha = Some("2222222222222222222222222222222222222222".to_string());
    let outcome = evaluate(&advanced);
    assert_eq!(outcome.admission, DeletionAdmission::RetainBranchMoved, "{}", outcome.detail);

    let mut unreadable = admissible_request();
    unreadable.branch.current_sha = None;
    let outcome = evaluate(&unreadable);
    assert_eq!(outcome.admission, DeletionAdmission::RetainBranchMoved, "{}", outcome.detail);
}

/// Falsifier 8 — the same branch name in another repository or a fork neither
/// creates nor erases a same-repository dependency.
#[test]
fn a_same_named_branch_in_another_repository_is_not_a_dependency() {
    // A foreign child on an identically named base must not retain.
    let mut foreign = admissible_request();
    let mut elsewhere = child(7810, false, Mergeability::Clean);
    elsewhere.repository = RepositoryId::new("SomeFork", "perl-lsp-swarm");
    foreign.graph.pull_requests = vec![elsewhere];
    assert_eq!(
        evaluate(&foreign).admission,
        DeletionAdmission::SafeToDelete,
        "a fork's child must not retain this repository's branch",
    );

    // ...and must not mask a genuine same-repository child either.
    let mut mixed = admissible_request();
    let mut elsewhere = child(7810, false, Mergeability::Clean);
    elsewhere.repository = RepositoryId::new("SomeFork", "perl-lsp-swarm");
    mixed.graph.pull_requests = vec![elsewhere, child(7819, false, Mergeability::Clean)];
    let outcome = evaluate(&mixed);
    assert_eq!(outcome.admission, DeletionAdmission::RetainOpenChildren);
    assert_eq!(outcome.retained_children.len(), 1, "only the same-repository child is retained");
    assert_eq!(outcome.retained_children[0].number, 7819);
}

/// Falsifier 9 — a cleanup helper cannot infer safety from the parent merely
/// being merged. Every other evidence gate still has to pass.
#[test]
fn a_merged_parent_alone_does_not_admit_deletion() {
    let ownership_cases = [
        WorktreeOwnership::ActiveWriter { detail: "worktree wt-7762 is checked out".to_string() },
        WorktreeOwnership::NotProven { detail: "worktree inventory unavailable".to_string() },
    ];
    for ownership in ownership_cases {
        let mut request = admissible_request();
        request.worktree_ownership = ownership.clone();
        let outcome = evaluate(&request);
        assert_eq!(
            outcome.admission,
            DeletionAdmission::RetainGraphNotProven,
            "{ownership:?} must retain even though the parent merged: {}",
            outcome.detail,
        );
    }

    // A parent that closed without merging is not terminal-as-expected: its
    // branch may hold the only copy of unsalvaged work.
    let mut closed = admissible_request();
    closed.parent.terminality = ParentTerminality::ClosedUnmerged;
    assert_eq!(evaluate(&closed).admission, DeletionAdmission::RetainParentNotTerminal);

    let mut unproven = admissible_request();
    unproven.parent.terminality = ParentTerminality::NotProven;
    assert_eq!(evaluate(&unproven).admission, DeletionAdmission::RetainParentNotTerminal);
}

/// No `RETAIN_*` outcome can be routed into a deletion, and no outcome at all
/// can put `--delete-branch` on a merge command. This is the property the
/// August 15 incident violated.
#[test]
fn no_retaining_outcome_can_be_routed_into_a_deletion() {
    let retaining = [
        {
            let mut request = admissible_request();
            request.parent.terminality = ParentTerminality::Open;
            request
        },
        {
            let mut request = admissible_request();
            request.graph.completeness =
                GraphCompleteness::Unavailable { detail: "transport error".to_string() };
            request
        },
        {
            let mut request = admissible_request();
            request.graph.pull_requests = vec![child(7810, true, Mergeability::NotProven)];
            request
        },
        {
            let mut request = admissible_request();
            request.branch.current_sha = None;
            request
        },
    ];

    for request in retaining {
        let outcome = evaluate(&request);
        assert!(
            !outcome.admission.admits_deletion(),
            "{:?} must not admit deletion",
            outcome.admission,
        );
        assert_eq!(branch_deletion_command(&outcome), None);
        assert!(
            !merge_command(outcome.parent_number, REVIEWED_SHA)
                .iter()
                .any(|argument| argument == "--delete-branch"),
        );
    }
}

/// Unknown mergeability proposes `HOLD`, and `CLOSE_OR_SUPERSEDE` is never
/// proposed automatically — #12885 forbids automatic child closure.
#[test]
fn unknown_mergeability_holds_and_closure_is_never_proposed() {
    let mut request = admissible_request();
    request.graph.pull_requests = vec![
        child(7810, false, Mergeability::NotProven),
        child(7819, false, Mergeability::Clean),
        child(7820, true, Mergeability::Conflicting),
    ];

    let outcome = evaluate(&request);
    let owners: Vec<NextOwner> =
        outcome.retained_children.iter().map(|child| child.next_owner).collect();
    assert_eq!(owners, vec![NextOwner::Hold, NextOwner::Retarget, NextOwner::Reconstruct]);
    assert!(
        !owners.contains(&NextOwner::CloseOrSupersede),
        "closure must never be proposed automatically",
    );
}

/// The retained packet must carry everything #6188/#11773 needs to reconcile a
/// child without re-querying the graph.
#[test]
fn the_retained_packet_renders_every_field_a_reconciler_needs() {
    let mut request = admissible_request();
    let mut affected = child(7810, true, Mergeability::Conflicting);
    affected.mergeability_changed_by_parent_merge = Some(true);
    request.graph.pull_requests = vec![affected];

    let outcome = evaluate(&request);
    let rendered = render_disposition(&outcome);
    for expected in [
        "RETAIN_OPEN_CHILDREN",
        "child #7810",
        "head=agent/child-7810",
        &format!("base={PARENT_BRANCH}"),
        "draft=true",
        "mergeable=CONFLICTING",
        "mergeability_changed_by_parent_merge=true",
        "next_owner=RECONSTRUCT",
    ] {
        assert!(rendered.contains(expected), "missing {expected:?} in:\n{rendered}");
    }

    // An unreported mergeability change renders as NOT_PROVEN, never as false.
    let mut silent = admissible_request();
    silent.graph.pull_requests = vec![child(7811, false, Mergeability::Clean)];
    let rendered = render_disposition(&evaluate(&silent));
    assert!(
        rendered.contains("mergeability_changed_by_parent_merge=NOT_PROVEN"),
        "an absent field must not render as a negative answer:\n{rendered}",
    );
}

/// Caller-supplied JSON must not be able to reach `SAFE_TO_DELETE` by being
/// incomplete. Each case perturbs exactly one field of the admissible fixture
/// into a shape a hand-written or truncated request could plausibly carry.
#[test]
fn a_malformed_request_cannot_reach_safe_to_delete() {
    type RequestMutation = Box<dyn Fn(&mut AdmissionRequest)>;
    let cases: Vec<(&str, RequestMutation)> = vec![
        (
            "empty reviewed sha",
            Box::new(|r: &mut AdmissionRequest| {
                r.parent.reviewed_head_sha = String::new();
                r.branch.current_sha = Some(String::new());
            }),
        ),
        (
            "abbreviated sha",
            Box::new(|r: &mut AdmissionRequest| {
                r.parent.reviewed_head_sha = "1111111".to_string();
                r.branch.current_sha = Some("1111111".to_string());
            }),
        ),
        (
            // Must carry letters: an all-digit sha is unchanged by
            // to_uppercase, so REVIEWED_SHA here would pass without
            // exercising the check at all.
            "uppercase sha",
            Box::new(|r: &mut AdmissionRequest| {
                r.parent.reviewed_head_sha = LETTERED_SHA.to_uppercase();
                r.branch.current_sha = Some(LETTERED_SHA.to_uppercase());
            }),
        ),
        (
            "non-hex sha",
            Box::new(|r: &mut AdmissionRequest| {
                let bogus = "z".repeat(40);
                r.parent.reviewed_head_sha = bogus.clone();
                r.branch.current_sha = Some(bogus);
            }),
        ),
        ("zero parent number", Box::new(|r: &mut AdmissionRequest| r.parent.number = 0)),
        ("empty head ref", Box::new(|r: &mut AdmissionRequest| r.parent.head_ref = String::new())),
        (
            "empty repository owner",
            Box::new(|r: &mut AdmissionRequest| {
                r.parent.repository.owner = String::new();
            }),
        ),
        ("empty remote", Box::new(|r: &mut AdmissionRequest| r.remote = String::new())),
        (
            "child with zero number",
            Box::new(|r: &mut AdmissionRequest| {
                let mut broken = child(7810, false, Mergeability::Clean);
                broken.number = 0;
                r.graph.pull_requests = vec![broken];
            }),
        ),
        (
            "child with empty base ref",
            Box::new(|r: &mut AdmissionRequest| {
                let mut broken = child(7810, false, Mergeability::Clean);
                broken.base_ref = String::new();
                r.graph.pull_requests = vec![broken];
            }),
        ),
    ];

    for (label, perturb) in cases {
        let mut request = admissible_request();
        perturb(&mut request);
        let outcome = evaluate(&request);
        assert_eq!(
            outcome.admission,
            DeletionAdmission::RetainGraphNotProven,
            "{label} must retain, got {:?} ({})",
            outcome.admission,
            outcome.detail,
        );
        assert_eq!(branch_deletion_command(&outcome), None, "{label} must yield no command");
    }

    // Positive control. Without it, a validator that rejected every request
    // would satisfy every case above.
    let mut well_formed = admissible_request();
    well_formed.parent.reviewed_head_sha = LETTERED_SHA.to_string();
    well_formed.branch.current_sha = Some(LETTERED_SHA.to_string());
    let outcome = evaluate(&well_formed);
    assert_eq!(
        outcome.admission,
        DeletionAdmission::SafeToDelete,
        "a well-formed lowercase sha must still be admitted ({})",
        outcome.detail,
    );
    assert_eq!(outcome.admitted_sha.as_deref(), Some(LETTERED_SHA));
}

/// The plan pairs the deletion with a verification naming the admitted
/// repository, and the deletion itself targets the VERIFIED URL rather than the
/// remote name. A name alone says nothing about which repository it resolves to,
/// and it stays mutable after the verification runs — so the argv must not
/// contain it at all.
#[test]
fn the_deletion_plan_binds_the_remote_to_the_admitted_repository() {
    let mut request = admissible_request();
    request.remote = "upstream".to_string();
    request.push_endpoint =
        Some("https://github.com/EffortlessMetrics/upstream-clone.git".to_string());
    let outcome = evaluate(&request);

    let (verification, expected) =
        remote_verification_command(&outcome).unwrap_or_else(|| (Vec::new(), String::new()));
    assert_eq!(
        verification,
        vec![
            "git".to_string(),
            "remote".to_string(),
            "get-url".to_string(),
            "upstream".to_string(),
        ],
    );
    assert_eq!(expected, "EffortlessMetrics/perl-lsp-swarm");

    // The verification names the remote; the deletion targets the URL that
    // verification bound. Neither the checked name nor a fallback may appear in
    // the mutating argv, or git would resolve remote config a second time.
    let command = branch_deletion_command(&outcome).unwrap_or_default();
    assert!(
        command.contains(&"https://github.com/EffortlessMetrics/upstream-clone.git".to_string()),
        "the deletion must target the bound URL: {command:?}",
    );
    assert!(!command.contains(&"upstream".to_string()), "must not push to a name: {command:?}");
    assert!(!command.contains(&"origin".to_string()), "must not fall back to origin: {command:?}");

    // A retaining outcome gets neither.
    let mut retained = admissible_request();
    retained.graph.pull_requests = vec![child(7810, false, Mergeability::Clean)];
    let retained = evaluate(&retained);
    assert!(remote_verification_command(&retained).is_none());
    assert!(branch_deletion_command(&retained).is_none());
}

// ── Recurrence check (falsifier 10) ───────────────────────────────────────────

/// Executable surfaces scanned for an unguarded merge-and-delete. Prose is
/// deliberately excluded: a historical runbook recording what was run in 2026
/// is not a path anything executes today. Only these directories can actually
/// delete a branch on a real repository.
const EXECUTABLE_SURFACES: &[&str] = &["scripts", ".github/workflows", "hooks", "xtask/src"];

fn repository_root() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    Ok(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

/// Join shell line-continuations so a flag parked on the next line is still
/// seen as part of the same command.
fn logical_lines(source: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut pending = String::new();
    for raw in source.lines() {
        let trimmed = raw.trim_end();
        if let Some(head) = trimmed.strip_suffix('\\') {
            pending.push_str(head);
            pending.push(' ');
            continue;
        }
        pending.push_str(trimmed);
        lines.push(std::mem::take(&mut pending));
    }
    if !pending.is_empty() {
        lines.push(pending);
    }
    lines
}

/// Falsifier 10 — a future direct `gh pr merge --delete-branch` anywhere on an
/// executable path is rejected.
///
/// This is the recurrence guard for the August 15 incident: #7799 was merged
/// with `--delete-branch` while #7810 and #7819 still named its head branch as
/// their base, and GitHub closed both. Deletion must go through an admission
/// that can return `RETAIN_OPEN_CHILDREN`, never ride along with the merge.
#[test]
fn no_executable_path_merges_with_delete_branch() -> Result<(), Box<dyn std::error::Error>> {
    let root = repository_root()?;
    let mut offenders = Vec::new();
    let mut scanned_files = 0usize;

    for surface in EXECUTABLE_SURFACES {
        let directory = root.join(surface);
        if !directory.exists() {
            return Err(format!("executable surface {surface} is missing; update the scan").into());
        }
        for entry in walkdir::WalkDir::new(&directory) {
            // A guard that silently skips what it cannot read is a guard that
            // fails open: an unreadable file is unscanned, not clean.
            let entry = entry.map_err(|error| format!("traversing {surface}: {error}"))?;
            if !entry.file_type().is_file() {
                continue;
            }
            let source = match std::fs::read_to_string(entry.path()) {
                Ok(source) => source,
                Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                    // Genuinely binary (fixtures, archives): cannot contain a
                    // shell invocation, so this is a real exemption rather
                    // than a swallowed failure.
                    continue;
                }
                Err(error) => {
                    return Err(format!("reading {}: {error}", entry.path().display()).into());
                }
            };
            scanned_files += 1;
            for (index, line) in logical_lines(&source).iter().enumerate() {
                // A commented mention explaining why the flag is absent is not
                // an invocation.
                if line.trim_start().starts_with('#') || line.trim_start().starts_with("//") {
                    continue;
                }
                // Any literal occurrence, not only one on the same logical
                // line as `gh pr merge`. A flag assembled into a variable, or
                // passed through a wrapper, deletes exactly as effectively as
                // an inline one, and pattern-matching only the shape this PR
                // happened to repair would leave those forms unguarded.
                if line.contains("--delete-branch")
                    || (line.contains("delete-branch:") && line.contains("true"))
                {
                    offenders.push(format!(
                        "{}:{}: {}",
                        entry.path().strip_prefix(&root).unwrap_or(entry.path()).display(),
                        index + 1,
                        line.trim(),
                    ));
                }
            }
        }
    }

    assert!(scanned_files > 0, "the recurrence scan read no files; it would pass vacuously");
    assert!(
        offenders.is_empty(),
        "merge-and-delete is not an admissible integration path (#12885). \
         Merge without --delete-branch and do not hand a deletion input to an \
         action; route branch cleanup through `branch-deletion-admission \
         admit`, which retains while any open PR names the branch as its \
         base. Offending sites:\n  {}",
        offenders.join("\n  "),
    );
    Ok(())
}

/// The scan must be able to fail. A guard that cannot see a planted violation
/// is not a guard, and this one has a real false-negative surface: the
/// comment filter and the continuation joining.
#[test]
fn the_recurrence_scan_detects_a_planted_violation() {
    let planted = "#!/usr/bin/env bash\ngh pr merge \"$PR\" --squash --delete-branch\n";
    let found = logical_lines(planted)
        .iter()
        .any(|line| line.contains("gh pr merge") && line.contains("--delete-branch"));
    assert!(found, "the scan must detect a direct merge-and-delete");

    // Split across a line continuation — the shape that would slip past a
    // naive per-line grep.
    let continued = "gh pr merge \"$PR\" --squash \\\n  --delete-branch\n";
    let found = logical_lines(continued)
        .iter()
        .any(|line| line.contains("gh pr merge") && line.contains("--delete-branch"));
    assert!(found, "the scan must join line continuations before matching");

    // Variable-built and wrapper forms: the flag never appears next to
    // `gh pr merge`, but reaches it just the same.
    for indirect in [
        "FLAGS=\"--delete-branch\"\ngh pr merge \"$PR\" --squash $FLAGS\n",
        "merge_pr() { gh pr merge \"$1\" --squash \"${@:2}\"; }\nmerge_pr 42 --delete-branch\n",
        "gh pr merge $PR --squash --delete-branch\n",
    ] {
        let found = logical_lines(indirect)
            .iter()
            .any(|line| !line.trim_start().starts_with('#') && line.contains("--delete-branch"));
        assert!(found, "the scan must detect an indirect deletion flag in:\n{indirect}");
    }

    // The action-input form: `delete-branch: true` handed to
    // peter-evans/create-pull-request deletes the generated head branch and is
    // just as unguarded as the CLI flag.
    let workflow_input = "        with:\n          delete-branch: true\n";
    let found = logical_lines(workflow_input)
        .iter()
        .any(|line| line.contains("delete-branch:") && line.contains("true"));
    assert!(found, "the scan must detect an action deletion input");

    // A comment explaining the absent flag must not be reported.
    let documented = "# No --delete-branch here: see #12885\ngh pr merge \"$PR\" --squash\n";
    let flagged = logical_lines(documented).iter().any(|line| {
        !line.trim_start().starts_with('#')
            && line.contains("gh pr merge")
            && line.contains("--delete-branch")
    });
    assert!(!flagged, "a comment must not count as an invocation");
}
