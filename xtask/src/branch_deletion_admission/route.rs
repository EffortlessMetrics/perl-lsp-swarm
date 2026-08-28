//! Routing the admission outcome into the commands the integration path runs.

use super::model::AdmissionOutcome;

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
/// outcome yields `None`, and so does a `SAFE_TO_DELETE` carrying no admitted
/// SHA — there would be nothing to lease against.
///
/// The deletion is **leased** on the admitted tip. `evaluate` reads the branch
/// SHA at snapshot time, but a writer can advance the branch between that read
/// and this command running; a plain `git push origin --delete` would then
/// delete the *new* tip, silently defeating `RETAIN_BRANCH_MOVED` and
/// destroying unsalvaged work. `--force-with-lease=<refname>:<expect>` makes
/// git reject the deletion as `stale info` instead, so branch movement fails
/// closed at execution rather than only at evaluation.
///
/// # Residual
///
/// The lease binds the *branch tip*, not the child graph. A pull request opened
/// against this branch after `evaluate` read the graph and before this command
/// runs is still auto-closed by the deletion, because opening a PR does not
/// move the tip and GitHub offers no lock that prevents a new dependency edge.
/// That window cannot be closed here; it is narrowed by re-reading the graph
/// immediately before deleting (#12885's contract step 5) and is recorded as a
/// known residual rather than claimed away.
pub fn branch_deletion_command(outcome: &AdmissionOutcome) -> Option<Vec<String>> {
    if !outcome.admission.admits_deletion() {
        return None;
    }

    // Fail closed: an admission with no admitted tip cannot be leased, so it
    // does not get a command.
    let admitted_sha = outcome.admitted_sha.as_deref()?;

    // Push to the exact verified URL, not the remote NAME. Verifying that a
    // name resolves to the admitted endpoint and then pushing to that name
    // leaves a window in which `git remote set-url --push` redirects the
    // deletion after every check has passed; the name is mutable config
    // re-resolved by git at mutation time. An outcome with no bound endpoint
    // gets no command — a snapshot is not an authorization.
    let push_endpoint = outcome.push_endpoint.as_deref()?;

    Some(vec![
        "git".to_string(),
        "push".to_string(),
        push_endpoint.to_string(),
        format!("--force-with-lease=refs/heads/{}:{admitted_sha}", outcome.branch),
        "--delete".to_string(),
        outcome.branch.clone(),
    ])
}

/// The command that binds the remote *name* to the admitted repository.
///
/// `branch_deletion_command` pushes to a remote name, and a name alone says
/// nothing about which repository it resolves to — the same-repository child
/// check in `evaluate` is snapshot-local, so a caller pointed at a different
/// remote would delete a branch no child check ever covered. The caller must
/// run this first and confirm the output identifies
/// [`AdmissionOutcome::repository`]; the expected identity is returned
/// alongside so there is nothing to look up.
///
/// Returns `None` for any outcome that does not admit deletion.
pub fn remote_verification_command(outcome: &AdmissionOutcome) -> Option<(Vec<String>, String)> {
    if !outcome.admission.admits_deletion() {
        return None;
    }

    Some((
        vec![
            "git".to_string(),
            "remote".to_string(),
            "get-url".to_string(),
            outcome.remote.clone(),
        ],
        outcome.repository.clone(),
    ))
}

/// Disposition only, with no runnable command attached.
///
/// Used where the outcome was computed from a *caller-supplied* snapshot
/// rather than live collection. Such an outcome can be structurally valid and
/// still describe a world that does not exist, so it must not hand anyone
/// something to run: authorization comes only from the live paths, which read
/// the subjects themselves.
pub fn render_snapshot_disposition(outcome: &AdmissionOutcome) -> String {
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

    rendered
}

/// Human-readable disposition *with* the commands a live caller should run.
///
/// For an admitted outcome this prints the identity check and the exact leased
/// deletion. Emitting them is the point: a caller who hand-rolls
/// `git push origin --delete` reintroduces the unleased deletion this module
/// exists to prevent, so the safe form is the one placed in front of them.
///
/// Only ever used for outcomes derived from live collection. Snapshot
/// evaluation uses [`render_snapshot_disposition`], which attaches nothing
/// runnable.
pub fn render_disposition(outcome: &AdmissionOutcome) -> String {
    let mut rendered = render_snapshot_disposition(outcome);

    if let Some((verification, expected_repository)) = remote_verification_command(outcome) {
        rendered
            .push_str(&format!("\n  verify: {} == {expected_repository}", verification.join(" ")));
    }
    if let Some(command) = branch_deletion_command(outcome) {
        rendered.push_str(&format!("\n  run: {}", command.join(" ")));
    }

    rendered
}
