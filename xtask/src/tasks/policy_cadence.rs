//! Read-only inventory of registered time-bound policy obligations.
//!
//! Domain validators remain authoritative. This module only projects their
//! review and expiry dates at an explicit date so owners see work before a
//! governing validator changes state.

use crate::tasks::file_policy;
use chrono::{NaiveDate, Utc};
use color_eyre::eyre::{Context, Result, eyre};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: &str = "policy_cadence.v1";
const REVIEW_WINDOW_DAYS: i64 = 30;
const EXPIRY_WINDOW_DAYS: i64 = 7;

/// Cadence classification at the receipt's explicit `as_of` date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CadenceState {
    /// The record is malformed or lacks required ownership metadata.
    Invalid,
    /// Required evidence is unavailable, so the record cannot appear current.
    NotProven,
    /// Its review date is within the advisory window.
    ReviewDueSoon,
    /// Its review date has passed but its expiry date has not.
    ReviewOverdue,
    /// Its expiry date is within the advisory window.
    Expiring,
    /// Its expiry date has passed. Domain enforcement remains unchanged.
    Expired,
    /// No registered review or expiry obligation is currently due.
    Current,
}

/// One normalized projection from a domain-owned policy record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CadenceObligation {
    pub record_id: String,
    pub source_kind: String,
    pub source_path: String,
    pub owner: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_issue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days_until_review: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days_until_expiry: Option<i64>,
    pub state: CadenceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_identity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_proven_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_debt_class: Option<String>,
    pub required_decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reproduce: Option<String>,
}

#[derive(Debug, Serialize)]
struct CadenceReceipt {
    schema_version: &'static str,
    as_of: String,
    advisory_only: bool,
    obligations: Vec<CadenceObligation>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegisteredCadenceRecord {
    record_id: String,
    source_kind: String,
    source_path: String,
    owner: String,
    owner_issue: Option<String>,
    review_after: Option<String>,
    expires: Option<String>,
    evidence_identity: Option<String>,
    expected_debt_class: Option<String>,
    required_decision: String,
    reproduce: Option<String>,
}

/// CLI configuration for the read-only cadence inventory.
pub struct CadenceArgs {
    pub as_of: Option<String>,
    pub json: PathBuf,
    pub markdown: PathBuf,
}

#[derive(Clone, Debug)]
struct RawObligation {
    record_id: String,
    source_kind: String,
    source_path: String,
    owner: String,
    owner_issue: Option<String>,
    review_after: Option<String>,
    expires: Option<String>,
    evidence_identity: Option<String>,
    expected_debt_class: Option<String>,
    required_decision: String,
    reproduce: Option<String>,
    invalid_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QualityLedger {
    #[serde(default)]
    exception: Vec<QualityException>,
}

#[derive(Debug, Deserialize)]
struct QualityException {
    id: String,
    owner: String,
    issue: Option<String>,
    scope: String,
    evidence: String,
    review_after: String,
    expires: String,
}

#[derive(Debug, Deserialize)]
struct ScenarioManifest {
    #[serde(default)]
    error_waivers: Vec<ScenarioWaiver>,
}

#[derive(Debug, Deserialize)]
struct ScenarioWaiver {
    project: String,
    journey: String,
    expected_error_class: String,
    issue: u64,
    expires_after: String,
}

/// Inventory registered policy surfaces and emit deterministic JSON and Markdown.
pub fn run(root: &Path, args: CadenceArgs) -> Result<()> {
    let as_of = match args.as_of {
        Some(raw) => parse_date(&raw, "--as-of")?,
        None => Utc::now().date_naive(),
    };
    let receipt = build_receipt(root, as_of)?;
    let json = serde_json::to_string_pretty(&receipt)? + "\n";
    let markdown = render_markdown(&receipt);
    write_output(root, &args.json, &json)?;
    write_output(root, &args.markdown, &markdown)?;
    println!(
        "Policy cadence: {} obligation(s) at {}; wrote {} and {}",
        receipt.obligations.len(),
        receipt.as_of,
        args.json.display(),
        args.markdown.display()
    );
    Ok(())
}

fn build_receipt(root: &Path, as_of: NaiveDate) -> Result<CadenceReceipt> {
    let mut raw = Vec::new();
    raw.extend(quality_obligations(root)?);
    raw.extend(scenario_obligations(root)?);
    raw.extend(non_rust_obligations(root)?);
    raw.extend(registered_obligations(root)?);
    let mut obligations = raw.into_iter().map(|item| classify(item, as_of)).collect::<Vec<_>>();
    obligations.sort_by(|left, right| {
        (&left.source_kind, &left.record_id).cmp(&(&right.source_kind, &right.record_id))
    });
    Ok(CadenceReceipt {
        schema_version: SCHEMA_VERSION,
        as_of: as_of.to_string(),
        advisory_only: true,
        obligations,
    })
}

fn registered_obligations(root: &Path) -> Result<Vec<RawObligation>> {
    let relative = "policy/cadence-records.json";
    let path = root.join(relative);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let records: Vec<RegisteredCadenceRecord> =
        serde_json::from_str(&read(root, relative)?).context("parsing cadence records")?;
    Ok(records
        .into_iter()
        .map(|entry| RawObligation {
            record_id: entry.record_id,
            source_kind: entry.source_kind,
            source_path: entry.source_path,
            owner: entry.owner,
            owner_issue: entry.owner_issue,
            review_after: entry.review_after,
            expires: entry.expires,
            evidence_identity: entry.evidence_identity,
            expected_debt_class: entry.expected_debt_class,
            required_decision: entry.required_decision,
            reproduce: entry.reproduce,
            invalid_reason: None,
        })
        .collect())
}

fn quality_obligations(root: &Path) -> Result<Vec<RawObligation>> {
    let relative = "policy/quality-gate-exceptions.toml";
    let text = read(root, relative)?;
    let ledger: QualityLedger = toml::from_str(&text).context("parsing quality exceptions")?;
    Ok(ledger
        .exception
        .into_iter()
        .map(|entry| RawObligation {
            record_id: entry.id,
            source_kind: "quality_gate_exception".to_string(),
            source_path: relative.to_string(),
            owner: entry.owner,
            owner_issue: entry.issue,
            review_after: Some(entry.review_after),
            expires: Some(entry.expires),
            evidence_identity: nonempty(entry.evidence),
            expected_debt_class: Some(entry.scope),
            required_decision: "remove, narrow, or re-justify with fresh evidence".to_string(),
            reproduce: Some("cargo xtask quality-gate --mode transition".to_string()),
            invalid_reason: None,
        })
        .collect())
}

fn scenario_obligations(root: &Path) -> Result<Vec<RawObligation>> {
    let relative = "crates/perl-lsp-ux-tests/fixtures/golden_editor_workload.json";
    let text = read(root, relative)?;
    let manifest: ScenarioManifest =
        serde_json::from_str(&text).context("parsing Scenario 67 manifest")?;
    Ok(manifest
        .error_waivers
        .into_iter()
        .map(|entry| RawObligation {
            record_id: format!("{}:{}", entry.project, entry.journey),
            source_kind: "scenario_67_error_waiver".to_string(),
            source_path: relative.to_string(),
            owner: format!("#{}", entry.issue),
            owner_issue: Some(format!("#{}", entry.issue)),
            review_after: None,
            expires: Some(entry.expires_after),
            evidence_identity: None,
            expected_debt_class: Some(entry.expected_error_class),
            required_decision: "remove after proof, narrow, re-justify with fresh evidence, or intentionally let expire".to_string(),
            reproduce: Some("cargo test -p perl-lsp-ux-tests --test ux_scenario_67_golden_editor_workload --locked".to_string()),
            invalid_reason: None,
        })
        .collect())
}

fn non_rust_obligations(root: &Path) -> Result<Vec<RawObligation>> {
    // Reuse the #2700 file-policy schema and loader instead of creating a
    // competing parser for `[[allow]]` entries.
    let ledger = file_policy::load_allowlist(root)?;
    Ok(ledger
        .allow
        .into_iter()
        .filter(|entry| !entry.retired)
        .map(|entry| {
            let evidence_identity =
                if entry.covered_by.is_empty() { None } else { Some(entry.covered_by.join("; ")) };
            RawObligation {
                record_id: entry.id,
                source_kind: "non_rust_allowlist".to_string(),
                source_path: "policy/non-rust-allowlist.toml".to_string(),
                owner: entry.owner,
                owner_issue: None,
                review_after: Some(entry.review_after),
                expires: entry.expires,
                evidence_identity,
                expected_debt_class: Some(entry.kind),
                required_decision: "retain after review, narrow, replace, or remove".to_string(),
                reproduce: Some("cargo xtask non-rust validate-policy".to_string()),
                invalid_reason: None,
            }
        })
        .collect())
}

fn classify(raw: RawObligation, as_of: NaiveDate) -> CadenceObligation {
    let review = raw.review_after.as_deref().map(|date| parse_date(date, "review_after"));
    let expiry = raw.expires.as_deref().map(|date| parse_date(date, "expires"));
    let invalid_reason = raw
        .invalid_reason
        .or_else(|| {
            review.as_ref().and_then(|result| result.as_ref().err().map(ToString::to_string))
        })
        .or_else(|| {
            expiry.as_ref().and_then(|result| result.as_ref().err().map(ToString::to_string))
        })
        .or_else(|| raw.owner.trim().is_empty().then(|| "owner is empty".to_string()));
    let review_date = review.and_then(Result::ok);
    let expiry_date = expiry.and_then(Result::ok);
    let days_until_review = review_date.map(|date| (date - as_of).num_days());
    let days_until_expiry = expiry_date.map(|date| (date - as_of).num_days());
    let not_proven_reason = if raw.evidence_identity.is_none() {
        Some("current evidence identity is not registered".to_string())
    } else {
        None
    };
    let state = if invalid_reason.is_some() {
        CadenceState::Invalid
    } else if days_until_expiry.is_some_and(|days| days < 0) {
        CadenceState::Expired
    } else if days_until_expiry.is_some_and(|days| days <= EXPIRY_WINDOW_DAYS) {
        CadenceState::Expiring
    } else if days_until_review.is_some_and(|days| {
        days < 0 || (days == 0 && raw.source_kind == "quality_gate_exception")
    }) {
        CadenceState::ReviewOverdue
    } else if days_until_review.is_some_and(|days| days <= REVIEW_WINDOW_DAYS) {
        CadenceState::ReviewDueSoon
    } else if not_proven_reason.is_some() {
        CadenceState::NotProven
    } else {
        CadenceState::Current
    };
    CadenceObligation {
        record_id: raw.record_id,
        source_kind: raw.source_kind,
        source_path: raw.source_path,
        owner: raw.owner,
        owner_issue: raw.owner_issue,
        review_after: raw.review_after,
        expires: raw.expires,
        days_until_review,
        days_until_expiry,
        state,
        evidence_identity: raw.evidence_identity,
        not_proven_reason: invalid_reason.or(not_proven_reason),
        expected_debt_class: raw.expected_debt_class,
        required_decision: raw.required_decision,
        reproduce: raw.reproduce,
    }
}

fn parse_date(raw: &str, field: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| eyre!("{field} must be a real YYYY-MM-DD date: {raw}"))
}

fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn read(root: &Path, relative: &str) -> Result<String> {
    let path = root.join(relative);
    fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
}

fn write_output(root: &Path, path: &Path, contents: &str) -> Result<()> {
    let path = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))
}

fn render_markdown(receipt: &CadenceReceipt) -> String {
    let mut output = format!(
        "# Policy cadence obligations\n\nAs of `{}`. Advisory projection only; domain validators remain authoritative.\n\n",
        receipt.as_of
    );
    output.push_str("| State | Kind | Record | Owner | Review | Expiry | Decision |\n");
    output.push_str("|---|---|---|---|---|---|---|\n");
    for item in &receipt.obligations {
        output.push_str(&format!(
            "| `{:?}` | `{}` | `{}` | `{}` | `{}` | `{}` | {} |\n",
            item.state,
            item.source_kind,
            item.record_id,
            item.owner,
            item.review_after.as_deref().unwrap_or("—"),
            item.expires.as_deref().unwrap_or("—"),
            item.required_decision.replace('|', "\\|")
        ));
    }
    output
}

#[cfg(test)]
fn deterministic_receipt_bytes(mut raw: Vec<RawObligation>, as_of: NaiveDate) -> Result<Vec<u8>> {
    let mut obligations = raw.drain(..).map(|item| classify(item, as_of)).collect::<Vec<_>>();
    obligations.sort_by(|left, right| {
        (&left.source_kind, &left.record_id).cmp(&(&right.source_kind, &right.record_id))
    });
    let receipt = CadenceReceipt {
        schema_version: SCHEMA_VERSION,
        as_of: as_of.to_string(),
        advisory_only: true,
        obligations,
    };
    Ok(serde_json::to_vec_pretty(&receipt)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(review: Option<&str>, expiry: Option<&str>, evidence: bool) -> RawObligation {
        RawObligation {
            record_id: "fixture".to_string(),
            source_kind: "fixture".to_string(),
            source_path: "fixture.toml".to_string(),
            owner: "#7046".to_string(),
            owner_issue: Some("#7046".to_string()),
            review_after: review.map(str::to_string),
            expires: expiry.map(str::to_string),
            evidence_identity: evidence.then(|| "receipt:abc".to_string()),
            expected_debt_class: Some("fixture_debt".to_string()),
            required_decision: "remove or re-justify".to_string(),
            reproduce: Some("cargo test".to_string()),
            invalid_reason: None,
        }
    }

    #[test]
    fn explicit_clock_keeps_review_and_expiry_states_distinct() -> Result<()> {
        let as_of = parse_date("2026-08-12", "fixture")?;
        assert_eq!(
            classify(raw(Some("2026-08-10"), Some("2026-09-01"), true), as_of).state,
            CadenceState::ReviewOverdue
        );
        assert_eq!(
            classify(raw(None, Some("2026-08-19"), true), as_of).state,
            CadenceState::Expiring
        );
        assert_eq!(
            classify(raw(None, Some("2026-08-11"), true), as_of).state,
            CadenceState::Expired
        );
        Ok(())
    }

    #[test]
    fn unavailable_evidence_never_appears_current() -> Result<()> {
        let as_of = parse_date("2026-08-12", "fixture")?;
        let item = classify(raw(Some("2027-01-01"), Some("2027-02-01"), false), as_of);
        assert_eq!(item.state, CadenceState::NotProven);
        assert!(item.not_proven_reason.is_some());
        Ok(())
    }

    #[test]
    fn malformed_dates_are_invalid() -> Result<()> {
        let as_of = parse_date("2026-08-12", "fixture")?;
        assert_eq!(
            classify(raw(None, Some("2026-02-30"), true), as_of).state,
            CadenceState::Invalid
        );
        Ok(())
    }

    #[test]
    fn quality_review_is_due_on_the_governing_date() -> Result<()> {
        let as_of = parse_date("2026-08-12", "fixture")?;
        let mut item = raw(Some("2026-08-12"), Some("2026-09-01"), true);
        item.source_kind = "quality_gate_exception".to_string();
        assert_eq!(classify(item, as_of).state, CadenceState::ReviewOverdue);
        assert_eq!(
            classify(raw(Some("2026-08-12"), Some("2026-09-01"), true), as_of).state,
            CadenceState::ReviewDueSoon
        );
        Ok(())
    }

    #[test]
    fn adapters_accept_governing_optional_and_numeric_issue_fields() -> Result<()> {
        let quality: QualityLedger = toml::from_str(
            r#"
[[exception]]
id = "fixture"
owner = "team"
scope = "fixture"
evidence = "receipt"
review_after = "2026-08-12"
expires = "2026-09-01"
"#,
        )?;
        assert_eq!(quality.exception.len(), 1);
        assert!(quality.exception[0].issue.is_none());

        let scenario: ScenarioManifest = serde_json::from_str(
            r#"{"error_waivers":[{"project":"p","journey":"j","expected_error_class":"e","issue":4050,"expires_after":"2026-09-01"}]}"#,
        )?;
        assert_eq!(scenario.error_waivers[0].issue, 4050);
        Ok(())
    }

    #[test]
    fn input_order_does_not_change_canonical_json() -> Result<()> {
        let as_of = parse_date("2026-08-12", "fixture")?;
        let first = raw(Some("2026-08-15"), None, true);
        let mut second = raw(Some("2026-08-20"), None, true);
        second.record_id = "another-fixture".to_string();
        assert_eq!(
            deterministic_receipt_bytes(vec![first.clone(), second.clone()], as_of)?,
            deterministic_receipt_bytes(vec![second, first], as_of)?
        );
        Ok(())
    }

    #[test]
    fn current_repository_adapters_are_deterministic() -> Result<()> {
        let root = crate::utils::project_root()?;
        let as_of = parse_date("2026-08-12", "fixture")?;
        let first = build_receipt(&root, as_of)?;
        let second = build_receipt(&root, as_of)?;
        assert_eq!(serde_json::to_vec_pretty(&first)?, serde_json::to_vec_pretty(&second)?);
        assert!(first.obligations.iter().any(|item| item.source_kind == "quality_gate_exception"));
        assert!(first.obligations.iter().any(|item| item.source_kind == "non_rust_allowlist"));
        // Current main removed the expired Scenario 67 waivers. The registry
        // retains the four historical obligations without restoring them to
        // the governing fixture or treating their debt as proven current.
        assert_eq!(
            first
                .obligations
                .iter()
                .filter(|item| item.source_kind == "scenario_67_error_waiver")
                .count(),
            4
        );
        Ok(())
    }
}
