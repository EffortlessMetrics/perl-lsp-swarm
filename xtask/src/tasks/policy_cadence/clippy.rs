use super::RawObligation;
use crate::tasks::check_lint_policy;
use color_eyre::eyre::Result;
use std::path::Path;

pub(super) fn obligations(root: &Path) -> Result<Vec<RawObligation>> {
    Ok(check_lint_policy::cadence_rows(root)?
        .into_iter()
        .map(|entry| RawObligation {
            record_id: entry.record_id,
            source_kind: entry.source_kind.to_string(),
            source_path: entry.source_path.to_string(),
            owner: entry.owner,
            owner_issue: entry.owner_issue,
            review_after: Some(entry.review_after),
            expires: None,
            evidence_identity: Some(entry.evidence_identity),
            expected_debt_class: Some(entry.expected_debt_class),
            required_decision: entry.required_decision.to_string(),
            reproduce: Some("cargo xtask check-lint-policy".to_string()),
            disposition: None,
            falsifier: None,
            invalid_reason: None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::{CadenceState, classify, parse_date};
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn debt_and_deferred_rows_keep_overdue_owner_work_visible() -> Result<()> {
        let root = tempdir()?;
        let policy = root.path().join("policy");
        fs::create_dir_all(policy.join("clippy-lints.d"))?;
        fs::write(
            policy.join("clippy-lints.toml"),
            r#"schema = 2
msrv = "1.95"

[policy]
panic_free_tests = true
allow_test_carveouts = false
suppression_style = "expect-with-reason"
blanket_categories = false

[[deferred_due]]
name = "clippy::manual_checked_ops"
level = "warn"
activate_when_msrv = "1.95"
class = "numeric"
owner = "#9869"
reason = "fixture deferral"
review_after = "2026-09-01"
next_status = "active"
"#,
        )?;
        fs::write(
            policy.join("clippy-lints.d/00-fixture.toml"),
            r#"schema = 1

[[lint]]
name = "clippy::collapsible_if"
level = "deny"
status = "debt"
class = "reviewability"
reason = "fixture catalog row"
"#,
        )?;
        fs::write(
            policy.join("clippy-debt.toml"),
            r#"schema = 2

[[debt]]
lint = "clippy::collapsible_if"
level = "deny"
path = "Cargo.toml"
owner = "#6305"
reason = "fixture debt"
review_after = "2026-09-02"
"#,
        )?;

        let as_of = parse_date("2026-09-10", "fixture")?;
        let mut items = obligations(root.path())?
            .into_iter()
            .map(|item| classify(item, as_of))
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.source_kind.cmp(&right.source_kind));

        assert_eq!(items.len(), 2);
        let debt = items
            .iter()
            .find(|item| item.source_kind == "clippy_debt")
            .expect("debt row");
        assert_eq!(debt.state, CadenceState::ReviewOverdue);
        assert_eq!(debt.owner, "#6305");
        assert_eq!(debt.owner_issue.as_deref(), Some("#6305"));
        assert_eq!(debt.source_path, "policy/clippy-debt.toml");
        assert_eq!(debt.record_id, "clippy::collapsible_if:Cargo.toml");

        let deferred = items
            .iter()
            .find(|item| item.source_kind == "clippy_deferred_due")
            .expect("deferred row");
        assert_eq!(deferred.state, CadenceState::ReviewOverdue);
        assert_eq!(deferred.owner, "#9869");
        assert_eq!(deferred.owner_issue.as_deref(), Some("#9869"));
        assert_eq!(deferred.source_path, "policy/clippy-lints.toml");
        assert_eq!(deferred.record_id, "clippy::manual_checked_ops");
        Ok(())
    }
}
