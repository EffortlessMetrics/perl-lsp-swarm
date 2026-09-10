//! Exact owner-linked dispositions for unresolved tautology findings.
//!
//! Dispositions are smaller than the finding: exact path, exact rule, and an
//! optional exact line. Missing ownership, malformed lifecycle metadata, or an
//! unused row is an instrument failure. Elapsed review and expiry dates remain
//! valid policy and are reported through the advisory policy-cadence surface;
//! crossing midnight does not mutate an unchanged candidate verdict (#15267).

use super::detect::RuleId;
use super::scan::Finding;
use chrono::NaiveDate;
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct DispositionFile {
    schema_version: u32,
    policy: String,
    #[serde(default)]
    disposition: Vec<DispositionRow>,
}

#[derive(Debug, Clone, Deserialize)]
struct DispositionRow {
    id: String,
    rule: String,
    path: String,
    #[serde(default)]
    line: Option<u32>,
    #[serde(default)]
    shape: Option<String>,
    #[serde(default)]
    owner: String,
    #[serde(default)]
    issue: String,
    #[serde(default)]
    reason: String,
    created: String,
    #[serde(default)]
    review_after: Option<String>,
    expires: String,
}

#[derive(Debug, Clone)]
pub struct DispositionLedger {
    rows: Vec<ValidatedDisposition>,
}

#[derive(Debug, Clone)]
struct ValidatedDisposition {
    id: String,
    rule: RuleId,
    rule_name: String,
    path: String,
    line: Option<u32>,
    shape: Option<String>,
    owner: String,
    issue: String,
    reason: String,
    created: String,
    review_after: Option<String>,
    expires: String,
}

/// One structurally valid lifecycle row exposed to `policy cadence`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CadenceRow {
    pub(crate) id: String,
    pub(crate) rule: String,
    pub(crate) path: String,
    pub(crate) line: Option<u32>,
    pub(crate) shape: Option<String>,
    pub(crate) owner: String,
    pub(crate) issue: String,
    pub(crate) reason: String,
    pub(crate) created: String,
    pub(crate) review_after: Option<String>,
    pub(crate) expires: String,
}

impl DispositionLedger {
    pub fn empty() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read disposition ledger {}", path.display()))?;
        Self::parse(&text)
            .with_context(|| format!("invalid disposition ledger {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let parsed: DispositionFile =
            toml::from_str(text).context("disposition TOML parse failed")?;
        if parsed.schema_version != 1 {
            bail!("disposition schema_version must be 1, found {}", parsed.schema_version);
        }
        if parsed.policy != "tautology-dispositions" {
            bail!("disposition policy must be tautology-dispositions");
        }

        let mut ids = BTreeSet::new();
        let mut rows = Vec::new();
        for row in parsed.disposition {
            validate_row(&row, &mut ids)?;
            let rule = RuleId::parse(&row.rule)
                .ok_or_else(|| eyre!("disposition `{}` has unknown rule `{}`", row.id, row.rule))?;
            if row.path.contains('*') || row.path.contains('?') || row.path.ends_with('/') {
                bail!(
                    "disposition `{}` path must be an exact repository-relative file, found `{}`",
                    row.id,
                    row.path
                );
            }
            rows.push(ValidatedDisposition {
                id: row.id,
                rule,
                rule_name: row.rule,
                path: row.path,
                line: row.line,
                shape: row.shape,
                owner: row.owner,
                issue: row.issue,
                reason: row.reason,
                created: row.created,
                review_after: row.review_after,
                expires: row.expires,
            });
        }
        Ok(Self { rows })
    }

    pub fn suppress(&self, finding: &Finding) -> bool {
        self.rows.iter().any(|row| {
            row.path == finding.path
                && row.rule == finding.rule
                && row.line.is_none_or(|line| line == finding.line)
                && row.shape.as_deref().is_none_or(|shape| shape == finding.shape)
        })
    }

    pub fn unused_for(&self, findings: &[Finding]) -> Vec<String> {
        self.rows
            .iter()
            .filter(|row| {
                !findings.iter().any(|finding| {
                    row.path == finding.path
                        && row.rule == finding.rule
                        && row.line.is_none_or(|line| line == finding.line)
                        && row.shape.as_deref().is_none_or(|shape| shape == finding.shape)
                })
            })
            .map(|row| row.id.clone())
            .collect()
    }

    fn cadence_rows(&self) -> Vec<CadenceRow> {
        self.rows
            .iter()
            .map(|row| CadenceRow {
                id: row.id.clone(),
                rule: row.rule_name.clone(),
                path: row.path.clone(),
                line: row.line,
                shape: row.shape.clone(),
                owner: row.owner.clone(),
                issue: row.issue.clone(),
                reason: row.reason.clone(),
                created: row.created.clone(),
                review_after: row.review_after.clone(),
                expires: row.expires.clone(),
            })
            .collect()
    }
}

pub(crate) fn cadence_rows(path: &Path) -> Result<Vec<CadenceRow>> {
    Ok(DispositionLedger::load(path)?.cadence_rows())
}

fn validate_row(row: &DispositionRow, ids: &mut BTreeSet<String>) -> Result<()> {
    if row.id.trim().is_empty() {
        bail!("disposition id must be non-empty");
    }
    if !ids.insert(row.id.clone()) {
        bail!("duplicate disposition id `{}`", row.id);
    }
    if row.owner.trim().is_empty() {
        bail!("disposition `{}` is ownerless", row.id);
    }
    if row.reason.trim().is_empty() {
        bail!("disposition `{}` is missing a reason", row.id);
    }
    if row.issue.trim().is_empty() {
        bail!("disposition `{}` is missing a controlling issue", row.id);
    }
    parse_date(&row.created, "created", &row.id)?;
    if let Some(review_after) = &row.review_after {
        parse_date(review_after, "review_after", &row.id)?;
    }
    parse_date(&row.expires, "expires", &row.id)?;
    Ok(())
}

fn parse_date(value: &str, field: &str, id: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| eyre!("disposition `{id}` has invalid {field} date `{value}`"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::super::detect::RuleId;
    use super::super::scan::Finding;
    use super::DispositionLedger;

    fn valid_row(expires: &str, owner: &str) -> String {
        format!(
            r##"
schema_version = 1
policy = "tautology-dispositions"

[[disposition]]
id = "tautology-demo"
rule = "option-is-some-or-none"
path = "crates/demo/src/lib.rs"
line = 4
owner = "{owner}"
issue = "#14061"
reason = "temporary unresolved product boundary"
created = "2026-08-30"
review_after = "2026-09-30"
expires = "{expires}"
"##
        )
    }

    fn finding() -> Finding {
        Finding {
            path: "crates/demo/src/lib.rs".to_string(),
            line: 4,
            rule: RuleId::OptionSomeOrNone,
            shape: RuleId::OptionSomeOrNone.shape(),
        }
    }

    #[test]
    fn ownerless_disposition_is_an_error() {
        let error = DispositionLedger::parse(&valid_row("2026-11-30", ""))
            .expect_err("ownerless");
        assert!(error.to_string().contains("ownerless"), "{error}");
    }

    #[test]
    fn elapsed_expiry_does_not_change_candidate_policy() {
        let ledger = DispositionLedger::parse(&valid_row("2000-01-01", "parser-core"))
            .expect("elapsed lifecycle date remains structurally valid");
        assert!(ledger.suppress(&finding()));
    }

    #[test]
    fn malformed_expiry_is_an_error() {
        let error = DispositionLedger::parse(&valid_row("not-a-date", "parser-core"))
            .expect_err("malformed expiry");
        assert!(error.to_string().contains("invalid expires date"), "{error}");
    }

    #[test]
    fn exact_disposition_suppresses_only_the_named_finding() {
        let ledger = DispositionLedger::parse(&valid_row("2026-11-30", "parser-core"))
            .expect("valid ledger");
        assert!(ledger.suppress(&finding()));
        let mut other = finding();
        other.line = 9;
        assert!(!ledger.suppress(&other));
        assert!(ledger.unused_for(&[other]).contains(&"tautology-demo".to_string()));
        assert!(ledger.unused_for(&[finding()]).is_empty());
    }
}
