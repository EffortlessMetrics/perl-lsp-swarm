use super::super::model::ConfigurationState;
use super::super::summary::render_policy_summary;
use super::{deferred_lint, empty_debt, ledger_with, lint_entry, planned_lint};

#[test]
fn summary_is_insertion_order_independent() {
    let mut first = ledger_with(vec![
        lint_entry("clippy::panic", "active"),
        lint_entry("clippy::indexing_slicing", "tracked"),
    ]);
    first.planned.push(planned_lint("clippy::manual_pop_if", "1.96"));
    first.deferred_due.push(deferred_lint("clippy::manual_checked_ops", "1.95"));

    let mut second = ledger_with(vec![
        lint_entry("clippy::indexing_slicing", "tracked"),
        lint_entry("clippy::panic", "active"),
    ]);
    second.planned.push(planned_lint("clippy::manual_pop_if", "1.96"));
    second.deferred_due.push(deferred_lint("clippy::manual_checked_ops", "1.95"));

    assert_eq!(
        render_policy_summary(&first, &empty_debt(), 0),
        render_policy_summary(&second, &empty_debt(), 0)
    );
}

#[test]
fn summary_surfaces_empty_by_design_configuration() {
    let mut lint = lint_entry("clippy::disallowed_fields", "active");
    lint.configuration_state = Some(ConfigurationState::EmptyByDesign);
    let summary = render_policy_summary(&ledger_with(vec![lint]), &empty_debt(), 0);

    assert!(summary.contains("configuration-empty-by-design (1): clippy::disallowed_fields"));
    assert!(summary.contains("configured selector denominator: 0"));
    assert!(summary.contains("protected architecture-seam denominator: 0"));
}
