//! Routing the admission outcome into the commands the integration path runs.

use super::model::{AdmissionOutcome, DeletionAdmission};

/// The canonical protected-integration merge command.
///
/// PLSP-SPEC-0006 fixes the form as
/// `gh pr merge <n> --squash --match-head-commit <current-head-sha>`, where the
/// head SHA is compare-and-swap protection against a merge race.
///
/// It carries no `--delete-branch`, and no admission outcome can add one:
/// parent merge and parent-branch deletion are separate decisions (#12885).
/// A pre-merge parent is by construction not terminal, so the pre-merge
/// admission is always `RETAIN_PARENT_NOT_TERMINAL` — the flag is not merely
/// omitted here, it is unreachable at merge time. Deletion is admitted
/// afterwards, against a freshly re-read graph, through
/// [`branch_deletion_command`].
pub fn merge_command(pull_number: u64, head_sha: &str) -> Vec<String> {
    vec![
        "gh".to_string(),
        "pr".to_string(),
        "merge".to_string(),
        pull_number.to_string(),
        "--squash".to_string(),
        "--match-head-commit".to_string(),
        head_sha.to_string(),
    ]
}

/// The branch-deletion command for an outcome, or `None` when the outcome
/// retains.
///
/// This is the only sanctioned way to turn an admission into a deletion: a
/// caller that cannot get a command back has nothing to run. Every `RETAIN_*`
/// outcome yields `None`.
pub fn branch_deletion_command(outcome: &AdmissionOutcome) -> Option<Vec<String>> {
    if !outcome.admission.admits_deletion() {
        return None;
    }

    Some(vec![
        "git".to_string(),
        "push".to_string(),
        "origin".to_string(),
        "--delete".to_string(),
        outcome.branch.clone(),
    ])
}

/// Human-readable one-line disposition, for logs and PR comments.
pub fn render_disposition(outcome: &AdmissionOutcome) -> String {
    let mut rendered = format!(
        "{} {} (#{}) — {}",
        outcome.admission.as_str(),
        outcome.branch,
        outcome.parent_number,
        outcome.detail
    );

    for child in &outcome.retained_children {
        rendered.push_str(&format!(
            "\n  child #{} head={} base={} draft={} state={} mergeable={} \
             mergeability_changed_by_parent_merge={} next_owner={}",
            child.number,
            child.head_ref,
            child.base_ref,
            child.draft,
            child.state.as_str(),
            child.mergeable.as_str(),
            match child.mergeability_changed_by_parent_merge {
                Some(changed) => changed.to_string(),
                None => "NOT_PROVEN".to_string(),
            },
            child.next_owner.as_str(),
        ));
    }

    debug_assert!(
        matches!(outcome.admission, DeletionAdmission::RetainOpenChildren)
            || outcome.retained_children.is_empty(),
        "only RETAIN_OPEN_CHILDREN carries retained children",
    );

    rendered
}
