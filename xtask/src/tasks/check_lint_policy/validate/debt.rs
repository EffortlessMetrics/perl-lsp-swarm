use super::super::model::{DebtEntry, DebtLedger, LintLedger};
use super::common::{parse_review_date, validate_level, validate_lint_name, validate_nonempty};
use super::disposition::validate_unique_dispositions;
use chrono::NaiveDate;
use color_eyre::eyre::{Result, bail};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(crate) fn validate_debt_ledger(
    root: &Path,
    lint_ledger: &LintLedger,
    debt_ledger: &DebtLedger,
    today: NaiveDate,
) -> Result<()> {
    if debt_ledger.schema != 2 {
        bail!("policy/clippy-debt.toml schema must be 2");
    }
    validate_unique_dispositions(lint_ledger)?;

    let lint_by_name: BTreeMap<_, _> =
        lint_ledger.lint.iter().map(|lint| (lint.name.as_str(), lint)).collect();
    let debt_lints: BTreeSet<_> = lint_ledger
        .lint
        .iter()
        .filter(|lint| lint.status == "debt")
        .map(|lint| lint.name.as_str())
        .collect();
    let mut counts = BTreeMap::<&str, usize>::new();
    let mut identities = BTreeSet::new();

    for entry in &debt_ledger.debt {
        validate_debt_entry(root, entry, today)?;
        let Some(lint) = lint_by_name.get(entry.lint.as_str()) else {
            bail!("debt entry names ungoverned lint {}", entry.lint);
        };
        if lint.status != "debt" {
            bail!(
                "debt entry for {} requires ledger status debt, found {}",
                entry.lint,
                lint.status
            );
        }
        if lint.level != entry.level {
            bail!(
                "debt entry for {} has level {}, but ledger has {}",
                entry.lint,
                entry.level,
                lint.level
            );
        }
        if !identities.insert((entry.lint.as_str(), entry.path.as_str())) {
            bail!("duplicate debt identity for {} at {}", entry.lint, entry.path);
        }
        *counts.entry(entry.lint.as_str()).or_default() += 1;
    }

    for lint in debt_lints {
        if counts.get(lint).copied().unwrap_or_default() == 0 {
            bail!("debt lint {lint} has no current debt rows");
        }
    }

    Ok(())
}

fn validate_debt_entry(root: &Path, entry: &DebtEntry, today: NaiveDate) -> Result<()> {
    validate_lint_name(&entry.lint)?;
    validate_level(&entry.lint, &entry.level, false)?;
    validate_nonempty(&entry.lint, "path", &entry.path)?;
    let relative_path = Path::new(&entry.path);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        bail!("debt entry for {} path must be repository-relative: {}", entry.lint, entry.path);
    }
    let repository_root = root.canonicalize().map_err(|error| {
        color_eyre::eyre::eyre!("cannot resolve repository root {}: {error}", root.display())
    })?;
    let current_path = root.join(relative_path).canonicalize().map_err(|error| {
        color_eyre::eyre::eyre!(
            "debt entry for {} path is not a current repository file {}: {error}",
            entry.lint,
            entry.path
        )
    })?;
    if !current_path.starts_with(&repository_root) || !current_path.is_file() {
        bail!(
            "debt entry for {} path is not a current repository file: {}",
            entry.lint,
            entry.path
        );
    }
    validate_nonempty(&entry.lint, "owner", &entry.owner)?;
    validate_nonempty(&entry.lint, "reason", &entry.reason)?;
    let review_after = parse_review_date(&entry.lint, &entry.review_after)?;
    if review_after < today {
        bail!("debt entry for {} review date expired on {review_after}", entry.lint);
    }
    Ok(())
}
