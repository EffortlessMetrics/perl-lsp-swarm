use super::model::{ObservationState, WorktreeCleanupPlan, WorktreeClassification};

pub fn render_human(plan: &WorktreeCleanupPlan) -> String {
    let mut output = String::new();
    let subject = &plan.subject;

    push_line(&mut output, "worktree cleanup inspection");
    push_line(
        &mut output,
        format!("  repository: {}", subject.repository_root.display()),
    );
    push_line(
        &mut output,
        format!("  common dir: {}", subject.common_dir.display()),
    );
    push_line(&mut output, format!("  observed: {}", plan.observed_at));
    push_line(
        &mut output,
        format!("  plan digest: {}", plan.plan_digest),
    );
    push_line(
        &mut output,
        format!(
            "  aggregate: {}",
            plan.aggregate_classification.as_str()
        ),
    );
    push_line(&mut output, "");

    for entry in &plan.entries {
        push_line(
            &mut output,
            format!(
                "{:<12} {} [{}]",
                entry.classification.as_str(),
                entry.path.display(),
                entry.entry_id
            ),
        );
        if let Some(branch) = &entry.branch {
            push_line(&mut output, format!("  branch: {branch}"));
        }
        if let Some(head) = &entry.head {
            push_line(&mut output, format!("  head: {head}"));
        }
        if !entry.reason_tokens.is_empty() {
            push_line(
                &mut output,
                format!("  reasons: {}", entry.reason_tokens.join(", ")),
            );
        }
        if let Some(action) = &entry.proposed_action {
            push_line(
                &mut output,
                format!(
                    "  proposed: {} {} ({})",
                    action.kind.as_str(),
                    action.target.display(),
                    if action.targetable {
                        "targetable"
                    } else {
                        "review only"
                    }
                ),
            );
        }
        if entry.classification == WorktreeClassification::NotProven {
            render_not_proven_details(&mut output, entry);
        }
    }

    let summary = &plan.summary;
    push_line(&mut output, "");
    push_line(
        &mut output,
        format!(
            "summary: keep={} cache_only={} salvage={} review={} not_proven={} \
             targetable_actions={}",
            summary.keep,
            summary.cache_only,
            summary.salvage,
            summary.review,
            summary.not_proven,
            summary.targetable_actions
        ),
    );
    output
}

fn render_not_proven_details(output: &mut String, entry: &super::model::WorktreePlanEntry) {
    let observations = [
        (&entry.facts.path_exists.state, entry.facts.path_exists.detail.as_deref()),
        (
            &entry.facts.administrative_path.state,
            entry.facts.administrative_path.detail.as_deref(),
        ),
        (&entry.facts.dirty.state, entry.facts.dirty.detail.as_deref()),
        (
            &entry.facts.untracked.state,
            entry.facts.untracked.detail.as_deref(),
        ),
        (
            &entry.facts.open_pr.state,
            entry.facts.open_pr.detail.as_deref(),
        ),
        (
            &entry.facts.merged_pr.state,
            entry.facts.merged_pr.detail.as_deref(),
        ),
        (
            &entry.facts.unpushed_commits.state,
            entry.facts.unpushed_commits.detail.as_deref(),
        ),
    ];

    for (state, detail) in observations {
        if *state == ObservationState::NotProven {
            if let Some(detail) = detail {
                push_line(output, format!("  not proven: {detail}"));
            }
        }
    }
}

fn push_line(output: &mut String, line: impl AsRef<str>) {
    output.push_str(line.as_ref());
    output.push('\n');
}
