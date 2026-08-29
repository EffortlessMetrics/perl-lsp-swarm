use super::model::{ConfigurationState, DebtLedger, LintLedger};

pub(super) fn render_policy_summary(
    ledger: &LintLedger,
    debt: &DebtLedger,
    configured_selector_count: usize,
) -> String {
    let mut output = format!(
        "lint policy ok: {} dispositions, {} debt rows\n",
        ledger.lint.len() + ledger.planned.len() + ledger.deferred_due.len(),
        debt.debt.len()
    );

    for status in ["active", "debt", "tracked"] {
        let names = ledger
            .lint
            .iter()
            .filter(|lint| lint.status == status)
            .map(|lint| lint.name.clone())
            .collect();
        append_summary_group(&mut output, status, names);
    }
    append_summary_group(
        &mut output,
        "configuration-empty-by-design",
        ledger
            .lint
            .iter()
            .filter(|lint| lint.configuration_state == Some(ConfigurationState::EmptyByDesign))
            .map(|lint| lint.name.clone())
            .collect(),
    );
    // Phase 1 validation admits only the empty selector set, so the configured
    // selector denominator is also the protected architecture-seam denominator.
    output.push_str(&format!("  configured selector denominator: {configured_selector_count}\n"));
    output.push_str(&format!(
        "  protected architecture-seam denominator: {configured_selector_count}\n"
    ));
    append_summary_group(
        &mut output,
        "future-planned",
        ledger.planned.iter().map(|lint| lint.name.clone()).collect(),
    );
    append_summary_group(
        &mut output,
        "due-deferred",
        ledger.deferred_due.iter().map(|lint| lint.name.clone()).collect(),
    );

    output
}

fn append_summary_group(output: &mut String, label: &str, mut names: Vec<String>) {
    names.sort();
    output.push_str(&format!("  {label} ({}):", names.len()));
    if names.is_empty() {
        output.push_str(" none\n");
        return;
    }
    output.push(' ');
    output.push_str(&names.join(", "));
    output.push('\n');
}
