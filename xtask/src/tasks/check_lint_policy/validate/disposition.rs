use super::super::model::{DeferredLint, LintEntry, LintLedger, PlannedLint, RustVersion};
use super::super::read::collect_workspace_lints;
use super::common::{parse_review_date, validate_level, validate_lint_name, validate_nonempty};
use chrono::NaiveDate;
use color_eyre::eyre::{Result, bail, eyre};
use std::collections::BTreeMap;
use toml::Value;

/// Required lint identities, with optional pinned level and pinned ledger status.
///
/// A pinned level alone is not enough to hold a promotion: `[[planned]]` and
/// `[[deferred_due]]` rows also carry `level = "deny"`, so a row demoted out of
/// active enforcement still satisfies a level pin. Pinning the status as well is
/// what makes a promotion non-reversible without an explicit policy change.
const REQUIRED_DISPOSITIONS: &[(&str, Option<&str>, Option<&str>)] = &[
    ("rust::const_item_interior_mutations", None, None),
    ("rust::function_casts_as_integer", None, None),
    ("clippy::same_length_and_capacity", None, None),
    ("clippy::manual_checked_ops", None, None),
    ("clippy::manual_ilog2", Some("deny"), Some("active")),
    ("clippy::manual_take", None, None),
    ("clippy::manual_pop_if", None, None),
];

pub(crate) fn validate_workspace_lints(
    cargo: &Value,
    ledger: &LintLedger,
    today: NaiveDate,
) -> Result<()> {
    validate_unique_dispositions(ledger)?;

    let cargo_lints = collect_workspace_lints(cargo)?;
    let current_msrv = RustVersion::from_text(&ledger.msrv)?;
    let mut lint_by_name = BTreeMap::new();

    for lint in &ledger.lint {
        validate_lint_entry(lint)?;
        if lint_by_name.insert(lint.name.clone(), lint).is_some() {
            bail!("duplicate lint ledger entry for {}", lint.name);
        }
        match lint.status.as_str() {
            "active" | "debt" => {
                let cargo_level = cargo_lints.get(&lint.name).ok_or_else(|| {
                    eyre!("{} lint {} is missing from Cargo.toml", lint.status, lint.name)
                })?;
                if cargo_level != &lint.level {
                    bail!(
                        "lint {} level mismatch: Cargo.toml has {cargo_level}, ledger has {}",
                        lint.name,
                        lint.level
                    );
                }
            }
            "tracked" => {
                if cargo_lints.contains_key(&lint.name) {
                    bail!(
                        "tracked lint {} is already active in Cargo.toml; mark it active or debt",
                        lint.name
                    );
                }
            }
            _ => bail!("lint {} has unsupported status {}", lint.name, lint.status),
        }
    }

    for planned in &ledger.planned {
        validate_planned_lint(planned)?;
        if cargo_lints.contains_key(&planned.name) {
            bail!("future-planned lint {} is already active in Cargo.toml", planned.name);
        }
        let activation = RustVersion::from_text(&planned.activate_when_msrv)?;
        if activation <= current_msrv {
            bail!(
                "planned lint {} is due at MSRV {}; activate it or move it to deferred_due",
                planned.name,
                planned.activate_when_msrv
            );
        }
    }

    for deferred in &ledger.deferred_due {
        validate_deferred_lint(deferred, current_msrv, today)?;
        if cargo_lints.contains_key(&deferred.name) {
            bail!("deferred_due lint {} is already active in Cargo.toml", deferred.name);
        }
    }

    for (name, cargo_level) in &cargo_lints {
        let lint = lint_by_name
            .get(name)
            .ok_or_else(|| eyre!("Cargo.toml activates unledgered lint {name}"))?;
        if !matches!(lint.status.as_str(), "active" | "debt") {
            bail!("Cargo.toml activates lint {name}, but its ledger status is {}", lint.status);
        }
        if &lint.level != cargo_level {
            bail!(
                "lint {name} level mismatch: Cargo.toml has {cargo_level}, ledger has {}",
                lint.level
            );
        }
    }

    Ok(())
}

pub(crate) fn validate_required_dispositions(ledger: &LintLedger) -> Result<()> {
    let mut counts = BTreeMap::<&str, usize>::new();
    for name in ledger
        .lint
        .iter()
        .map(|lint| lint.name.as_str())
        .chain(ledger.planned.iter().map(|lint| lint.name.as_str()))
        .chain(ledger.deferred_due.iter().map(|lint| lint.name.as_str()))
    {
        *counts.entry(name).or_default() += 1;
    }

    for (required, expected_level, expected_status) in REQUIRED_DISPOSITIONS {
        if counts.get(required).copied().unwrap_or_default() != 1 {
            bail!(
                "required lint identity {required} must appear exactly once across the merged disposition model"
            );
        }
        let entry_level = ledger
            .lint
            .iter()
            .find(|lint| lint.name == *required)
            .map(|lint| lint.level.as_str())
            .or_else(|| {
                ledger
                    .planned
                    .iter()
                    .find(|lint| lint.name == *required)
                    .map(|lint| lint.level.as_str())
            })
            .or_else(|| {
                ledger
                    .deferred_due
                    .iter()
                    .find(|lint| lint.name == *required)
                    .map(|lint| lint.level.as_str())
            });
        if let Some(expected_level) = expected_level
            && entry_level != Some(*expected_level)
        {
            bail!(
                "required lint {required} must remain at level {expected_level}, but ledger has {}",
                entry_level.unwrap_or("missing")
            );
        }
        if let Some(expected_status) = expected_status {
            let entry_status = ledger
                .lint
                .iter()
                .find(|lint| lint.name == *required)
                .map(|lint| lint.status.as_str());
            if entry_status != Some(*expected_status) {
                bail!(
                    "required lint {required} must remain an active ledger entry at status {expected_status}, but ledger has {}",
                    entry_status.unwrap_or("no active entry (demoted to planned or deferred_due)")
                );
            }
        }
    }
    Ok(())
}

pub(super) fn validate_unique_dispositions(ledger: &LintLedger) -> Result<()> {
    let mut seen = BTreeMap::new();
    for lint in &ledger.lint {
        insert_disposition(&mut seen, &lint.name, "lint")?;
    }
    for planned in &ledger.planned {
        insert_disposition(&mut seen, &planned.name, "planned")?;
    }
    for deferred in &ledger.deferred_due {
        insert_disposition(&mut seen, &deferred.name, "deferred_due")?;
    }
    Ok(())
}

fn insert_disposition(
    seen: &mut BTreeMap<String, &'static str>,
    name: &str,
    section: &'static str,
) -> Result<()> {
    if let Some(previous) = seen.insert(name.to_owned(), section) {
        bail!("lint {name} has multiple dispositions in {previous} and {section}");
    }
    Ok(())
}

fn validate_lint_entry(lint: &LintEntry) -> Result<()> {
    validate_lint_name(&lint.name)?;
    validate_level(&lint.name, &lint.level, true)?;
    if !matches!(lint.status.as_str(), "active" | "debt" | "tracked") {
        bail!("lint {} must have status active, debt, or tracked", lint.name);
    }
    validate_nonempty(&lint.name, "class", &lint.class)?;
    validate_nonempty(&lint.name, "reason", &lint.reason)?;
    Ok(())
}

fn validate_planned_lint(planned: &PlannedLint) -> Result<()> {
    validate_lint_name(&planned.name)?;
    validate_level(&planned.name, &planned.level, false)?;
    RustVersion::from_text(&planned.activate_when_msrv)?;
    validate_nonempty(&planned.name, "class", &planned.class)?;
    validate_nonempty(&planned.name, "reason", &planned.reason)?;
    Ok(())
}

fn validate_deferred_lint(
    deferred: &DeferredLint,
    current_msrv: RustVersion,
    today: NaiveDate,
) -> Result<()> {
    validate_lint_name(&deferred.name)?;
    validate_level(&deferred.name, &deferred.level, false)?;
    validate_nonempty(&deferred.name, "class", &deferred.class)?;
    validate_nonempty(&deferred.name, "owner", &deferred.owner)?;
    validate_nonempty(&deferred.name, "reason", &deferred.reason)?;
    if !matches!(deferred.next_status.as_str(), "active" | "debt") {
        bail!("deferred_due lint {} next_status must be active or debt", deferred.name);
    }

    let activation = RustVersion::from_text(&deferred.activate_when_msrv)?;
    if activation > current_msrv {
        bail!(
            "deferred_due lint {} is not due until MSRV {}; keep it future-planned",
            deferred.name,
            deferred.activate_when_msrv
        );
    }

    let review_after = parse_review_date(&deferred.name, &deferred.review_after)?;
    if review_after < today {
        bail!("deferred_due lint {} review date expired on {review_after}", deferred.name);
    }

    Ok(())
}
