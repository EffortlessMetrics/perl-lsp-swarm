use super::model::{DebtLedger, LintLedger};

pub(super) fn render_policy_summary(ledger: &LintLedger, debt: &DebtLedger) -> String {
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
        "future-planned",
        ledger.planned.iter().map(|lint| lint.name.clone()).collect(),
    );
    append_summary_group(
        &mut output,
        "due-deferred",
        ledger
            .deferred_due
            .iter()
            .map(|lint| lint.name.clone())
            .collect(),
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
