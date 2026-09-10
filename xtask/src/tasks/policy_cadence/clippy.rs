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
            invalid_reason: entry.invalid_reason,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::{CadenceState, classify, parse_date};
    use super::*;
    use color_eyre::eyre::eyre;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn debt_and_deferred_rows_keep_overdue_owner_work_visible() -> Result<()> {
        let root = tempdir()?;
        write_fixture(root.path(), 2, "deny")?;

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
            .ok_or_else(|| eyre!("missing Clippy debt cadence row"))?;
        assert_eq!(debt.state, CadenceState::ReviewOverdue);
        assert_eq!(debt.owner, "#6305");
        assert_eq!(debt.owner_issue.as_deref(), Some("#6305"));
        assert_eq!(debt.source_path, "policy/clippy-debt.toml");
        assert_eq!(debt.record_id, "clippy::collapsible_if:Cargo.toml");
        assert!(debt.not_proven_reason.is_none());

        let deferred = items
            .iter()
            .find(|item| item.source_kind == "clippy_deferred_due")
            .ok_or_else(|| eyre!("missing Clippy deferred cadence row"))?;
        assert_eq!(deferred.state, CadenceState::ReviewOverdue);
        assert_eq!(deferred.owner, "#9869");
        assert_eq!(deferred.owner_issue.as_deref(), Some("#9869"));
        assert_eq!(deferred.source_path, "policy/clippy-lints.toml");
        assert_eq!(deferred.record_id, "clippy::manual_checked_ops");
        assert!(deferred.not_proven_reason.is_none());
        Ok(())
    }

    #[test]
    fn invalid_policy_header_cannot_appear_current_in_cadence() -> Result<()> {
        let root = tempdir()?;
        write_fixture(root.path(), 1, "deny")?;

        let as_of = parse_date("2026-08-01", "fixture")?;
        let items = obligations(root.path())?
            .into_iter()
            .map(|item| classify(item, as_of))
            .collect::<Vec<_>>();

        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| item.state == CadenceState::Invalid));
        assert!(items.iter().all(|item| {
            item.not_proven_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("policy/clippy-lints.toml schema must be 2"))
        }));
        Ok(())
    }

    #[test]
    fn invalid_debt_row_cannot_appear_current_in_cadence() -> Result<()> {
        let root = tempdir()?;
        write_fixture(root.path(), 2, "sometimes")?;

        let as_of = parse_date("2026-08-01", "fixture")?;
        let items = obligations(root.path())?
            .into_iter()
            .map(|item| classify(item, as_of))
            .collect::<Vec<_>>();

        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| item.state == CadenceState::Invalid));
        assert!(items.iter().all(|item| {
            item.not_proven_reason.as_deref().is_some_and(|reason| {
                reason.contains("lint clippy::collapsible_if has unsupported level sometimes")
            })
        }));
        Ok(())
    }

    #[test]
    fn misplaced_configuration_state_marker_cannot_appear_current_in_cadence() -> Result<()> {
        let root = tempdir()?;
        write_fixture(root.path(), 2, "deny")?;
        fs::write(
            root.path().join("policy/clippy-lints.d/00-fixture.toml"),
            r##"schema = 1

[[lint]]
name = "clippy::collapsible_if"
level = "deny"
status = "debt"
class = "reviewability"
reason = "fixture catalog row"
configuration_state = "empty-by-design"
"##,
        )?;

        let as_of = parse_date("2026-08-01", "fixture")?;
        let items = obligations(root.path())?
            .into_iter()
            .map(|item| classify(item, as_of))
            .collect::<Vec<_>>();

        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| item.state == CadenceState::Invalid));
        assert!(items.iter().all(|item| {
            item.not_proven_reason.as_deref().is_some_and(|reason| {
                reason.contains("sets configuration_state, but only config-backed lints may do so")
            })
        }));
        Ok(())
    }

    fn write_fixture(root: &Path, lint_schema: u64, debt_level: &str) -> Result<()> {
        let policy = root.join("policy");
        fs::create_dir_all(policy.join("clippy-lints.d"))?;
        fs::write(root.join("Cargo.toml"), "[workspace]\n")?;
        fs::write(
            policy.join("clippy-lints.toml"),
            format!(
                r##"schema = {lint_schema}
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
"##
            ),
        )?;
        fs::write(
            policy.join("clippy-lints.d/00-fixture.toml"),
            r##"schema = 1

[[lint]]
name = "clippy::collapsible_if"
level = "deny"
status = "debt"
class = "reviewability"
reason = "fixture catalog row"
"##,
        )?;
        fs::write(
            policy.join("clippy-debt.toml"),
            format!(
                r##"schema = 2

[[debt]]
lint = "clippy::collapsible_if"
level = "{debt_level}"
path = "Cargo.toml"
owner = "#6305"
reason = "fixture debt"
review_after = "2026-09-02"
"##
            ),
        )?;
        Ok(())
    }
}
