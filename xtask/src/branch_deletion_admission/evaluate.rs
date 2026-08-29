//! The deletion-admission decision (#12885).
//!
//! One pure function over a graph snapshot. It reads no network, runs no git,
//! and deletes nothing — callers route its typed outcome.

use super::model::{
    AdmissionOutcome, AdmissionRequest, BRANCH_DELETION_ADMISSION_POLICY_VERSION,
    BRANCH_DELETION_ADMISSION_SCHEMA_VERSION, DeletionAdmission, GraphCompleteness, Mergeability,
    NextOwner, ObservedPullRequest, ParentTerminality, PullRequestState, RetainedChild,
    WorktreeOwnership,
};

/// Decide whether the parent's head branch may be deleted.
///
/// An open PR that names the branch as its base is a live dependency on that
/// branch — including when the child is a draft, blocked, conflicting or
/// scheduled for reconstruction. Deleting the base closes such a child, which
/// is how #7810 and #7819 were lost during the August 15 convergence.
///
/// Every path except a fully proven, childless, unmoved, unowned branch
/// retains. Missing, truncated, or contradictory evidence retains; it is never
/// read as absence of children.
///
/// Precedence, when more than one reason applies — the branch is retained
/// identically in every case, so this only chooses which reason is *reported*,
/// most actionable first:
///
/// 1. `RETAIN_PARENT_NOT_TERMINAL` — nothing downstream is meaningful yet.
/// 2. `RETAIN_GRAPH_NOT_PROVEN` — the child query was never completed, so
///    "no children" was never established.
/// 3. `RETAIN_OPEN_CHILDREN` — names work another authority must reconcile.
/// 4. `RETAIN_BRANCH_MOVED` — the branch is no longer the reviewed subject.
/// 5. `RETAIN_GRAPH_NOT_PROVEN` — local worktree/writer ownership (#3957).
/// 6. `SAFE_TO_DELETE`.
pub fn evaluate(request: &AdmissionRequest) -> AdmissionOutcome {
    let parent = &request.parent;
    let outcome = |admission: DeletionAdmission,
                   detail: String,
                   retained_children: Vec<RetainedChild>| AdmissionOutcome {
        schema_version: BRANCH_DELETION_ADMISSION_SCHEMA_VERSION.to_string(),
        policy_version: BRANCH_DELETION_ADMISSION_POLICY_VERSION.to_string(),
        repository: parent.repository.render(),
        parent_number: parent.number,
        branch: parent.head_ref.clone(),
        admission,
        detail,
        retained_children,
        remote: request.remote.clone(),
        push_endpoint: request.push_endpoint.clone(),
        admitted_sha: None,
    };

    // 0. The request is caller-supplied JSON. A malformed or incomplete one
    //    must not be able to reach SAFE_TO_DELETE: an absent SHA, a zero PR
    //    number, or an empty repository identity is missing evidence, not
    //    permission.
    if let Some(problem) = request.structural_problem() {
        return outcome(
            DeletionAdmission::RetainGraphNotProven,
            format!("admission request is not well formed: {problem}"),
            Vec::new(),
        );
    }

    // 1. The parent must have reached the terminal state this route expects.
    //    A merged parent is not on its own evidence that cleanup is safe —
    //    that inference is what the remaining steps exist to refuse.
    match parent.terminality {
        ParentTerminality::Merged => {}
        ParentTerminality::Open => {
            return outcome(
                DeletionAdmission::RetainParentNotTerminal,
                format!("pull request #{} is still open", parent.number),
                Vec::new(),
            );
        }
        ParentTerminality::ClosedUnmerged => {
            return outcome(
                DeletionAdmission::RetainParentNotTerminal,
                format!(
                    "pull request #{} closed without merging; the branch may hold the only copy of unsalvaged work",
                    parent.number
                ),
                Vec::new(),
            );
        }
        ParentTerminality::NotProven => {
            return outcome(
                DeletionAdmission::RetainParentNotTerminal,
                format!("terminal state of pull request #{} is not proven", parent.number),
                Vec::new(),
            );
        }
    }

    // 1b. The branch this route would delete must be the parent's actual head
    //     branch. For a cross-repository (fork) parent, `head_ref` names a
    //     branch in the fork; a same-named branch here is a different branch,
    //     and deleting it would destroy something the merge never touched.
    if !parent.head_in_admitted_repository {
        return outcome(
            DeletionAdmission::RetainBranchMoved,
            format!(
                "pull request #{} merged from a fork: {} names a branch in the head repository, not in {}",
                parent.number,
                parent.head_ref,
                parent.repository.render()
            ),
            Vec::new(),
        );
    }

    // 2. "No open children" is only a fact if the query actually finished.
    match &request.graph.completeness {
        GraphCompleteness::Complete => {}
        GraphCompleteness::Truncated { detail } => {
            return outcome(
                DeletionAdmission::RetainGraphNotProven,
                format!("open-child query was truncated: {detail}"),
                Vec::new(),
            );
        }
        GraphCompleteness::Unavailable { detail } => {
            return outcome(
                DeletionAdmission::RetainGraphNotProven,
                format!("open-child query was unavailable: {detail}"),
                Vec::new(),
            );
        }
    }

    // 3. Same-repository open PRs that merge *into* this branch.
    //
    //    Repository identity is compared exactly: an identically named branch
    //    in a fork or sibling repository is a different branch, and neither
    //    creates nor erases a dependency here.
    let children: Vec<&ObservedPullRequest> = request
        .graph
        .pull_requests
        .iter()
        .filter(|pull_request| {
            pull_request.repository == parent.repository
                && pull_request.base_ref == parent.head_ref
                && pull_request.state == PullRequestState::Open
        })
        .collect();

    if !children.is_empty() {
        let retained: Vec<RetainedChild> = children.iter().map(|child| retain(child)).collect();
        let numbers = retained
            .iter()
            .map(|child| format!("#{}", child.number))
            .collect::<Vec<_>>()
            .join(", ");
        return outcome(
            DeletionAdmission::RetainOpenChildren,
            format!(
                "{} open pull request(s) use {} as their base: {numbers}",
                retained.len(),
                parent.head_ref
            ),
            retained,
        );
    }

    // 4. The branch must still point at the subject that was reviewed. An
    //    unreadable tip is movement, not agreement.
    match request.branch.current_sha.as_deref() {
        Some(current) if current == parent.reviewed_head_sha => {}
        Some(current) => {
            return outcome(
                DeletionAdmission::RetainBranchMoved,
                format!(
                    "{} now points at {current}, not the reviewed subject {}",
                    parent.head_ref, parent.reviewed_head_sha
                ),
                Vec::new(),
            );
        }
        None => {
            return outcome(
                DeletionAdmission::RetainBranchMoved,
                format!("current tip of {} could not be read", parent.head_ref),
                Vec::new(),
            );
        }
    }

    // 5. Local worktree/writer ownership is the #3957 authority's to report.
    match &request.worktree_ownership {
        WorktreeOwnership::Clear => {}
        WorktreeOwnership::ActiveWriter { detail } => {
            return outcome(
                DeletionAdmission::RetainGraphNotProven,
                format!("a local writer still owns {}: {detail}", parent.head_ref),
                Vec::new(),
            );
        }
        WorktreeOwnership::NotProven { detail } => {
            return outcome(
                DeletionAdmission::RetainGraphNotProven,
                format!("local worktree ownership of {} is not proven: {detail}", parent.head_ref),
                Vec::new(),
            );
        }
    }

    // Step 4 proved the tip equals the reviewed subject, so that is the
    // admitted SHA the deletion will be leased against.
    let mut admitted = outcome(
        DeletionAdmission::SafeToDelete,
        format!(
            "#{} is merged, no open pull request uses {} as a base, the branch still points at {}, and no local writer owns it",
            parent.number, parent.head_ref, parent.reviewed_head_sha
        ),
        Vec::new(),
    );
    admitted.admitted_sha = Some(parent.reviewed_head_sha.clone());
    admitted
}

/// Build the retained-child packet, proposing a next owner from what the graph
/// actually reported.
///
/// `CLOSE_OR_SUPERSEDE` is never proposed: whether a child is genuinely
/// superseded is a judgment this check has no evidence for, and #12885 forbids
/// automatic child closure. Unknown mergeability proposes `HOLD` rather than
/// guessing.
fn retain(child: &ObservedPullRequest) -> RetainedChild {
    let next_owner = match child.mergeable {
        Mergeability::Clean => NextOwner::Retarget,
        Mergeability::Conflicting | Mergeability::Blocked => NextOwner::Reconstruct,
        Mergeability::NotProven => NextOwner::Hold,
    };

    RetainedChild {
        number: child.number,
        head_ref: child.head_ref.clone(),
        base_ref: child.base_ref.clone(),
        draft: child.draft,
        state: child.state,
        mergeable: child.mergeable,
        mergeability_changed_by_parent_merge: child.mergeability_changed_by_parent_merge,
        next_owner,
    }
}
