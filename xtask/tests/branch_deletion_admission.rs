//! Falsifiers for branch-deletion admission (#12885).
//!
//! Every test drives a fixture graph. Nothing here creates, retargets, closes
//! or deletes a live branch or pull request, and nothing reads the network.

use xtask::branch_deletion_admission::{
    AdmissionRequest, BranchSubject, DeletionAdmission, GraphCompleteness, Mergeability, NextOwner,
    ObservedPullRequest, OpenChildGraph, ParentSubject, ParentTerminality, PullRequestState,
    RepositoryId, WorktreeOwnership, branch_deletion_command, evaluate, merge_command,
    render_disposition,
};

const PARENT_BRANCH: &str = "agent/vim-activation-root-7762";
const REVIEWED_SHA: &str = "1111111111111111111111111111111111111111";

fn repository() -> RepositoryId {
    RepositoryId::new("EffortlessMetrics", "perl-lsp-swarm")
}

/// A merged parent whose branch is otherwise unencumbered: the only shape
/// that may ever reach `SAFE_TO_DELETE`. Each falsifier perturbs exactly one
/// field, so a passing test names the field that mattered.
fn admissible_request() -> AdmissionRequest {
    AdmissionRequest {
        parent: ParentSubject {
            repository: repository(),
            number: 7799,
            head_ref: PARENT_BRANCH.to_string(),
            reviewed_head_sha: REVIEWED_SHA.to_string(),
            terminality: ParentTerminality::Merged,
        },
        branch: BranchSubject { current_sha: Some(REVIEWED_SHA.to_string()) },
        graph: OpenChildGraph {
            completeness: GraphCompleteness::Complete,
            pull_requests: Vec::new(),
        },
        worktree_ownership: WorktreeOwnership::Clear,
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
            "origin".to_string(),
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
    moved.branch.current_sha = Some("2222222222222222222222222222222222222222".to_string());
    assert_eq!(evaluate(&moved).admitted_sha, None);
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
        for entry in walkdir::WalkDir::new(&directory).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(entry.path()) else {
                continue; // binary or non-UTF-8 file: nothing to scan
            };
            scanned_files += 1;
            for (index, line) in logical_lines(&source).iter().enumerate() {
                // A commented mention explaining why the flag is absent is not
                // an invocation.
                if line.trim_start().starts_with('#') || line.trim_start().starts_with("//") {
                    continue;
                }
                if line.contains("gh pr merge") && line.contains("--delete-branch") {
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

    assert!(scanned_files > 0, "the recurrence scan read no files; it would pass vacuously",);
    assert!(
        offenders.is_empty(),
        "merge-and-delete is not an admissible integration path (#12885). \
         Merge without --delete-branch, then route branch cleanup through \
         `branch-deletion-admission admit`, which retains while any open PR \
         names the branch as its base. Offending sites:\n  {}",
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

    // A comment explaining the absent flag must not be reported.
    let documented = "# No --delete-branch here: see #12885\ngh pr merge \"$PR\" --squash\n";
    let flagged = logical_lines(documented).iter().any(|line| {
        !line.trim_start().starts_with('#')
            && line.contains("gh pr merge")
            && line.contains("--delete-branch")
    });
    assert!(!flagged, "a comment must not count as an invocation");
}
