//! Source-bound `--check` authority for corpus audit reports (#6718 / #7067).
//!
//! Check mode must not trust a pre-existing artifact without proving it matches a
//! complete fresh execution over the current inputs. Informational metadata such
//! as wall-clock timestamps and duration are excluded from the semantic digest.

use color_eyre::eyre::{Context, Result, bail};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

use super::report::AuditReport;

/// Stable error prefix for a report missing at the expected output path.
pub const MISSING_REPORT_PREFIX: &str = "corpus audit check: missing report at";

/// Stable error prefix when the stored artifact's semantic payload differs.
pub const STALE_REPORT_PREFIX: &str = "corpus audit check: stale report";

/// Compute a deterministic digest of the report semantic payload.
///
/// `generated_at` and `duration_secs` are informational only and are omitted.
pub fn audit_report_semantic_digest(report: &AuditReport) -> Result<String> {
    stable_json_digest(&semantic_projection(report)?)
}

/// Compare a stored artifact against a freshly generated report.
pub fn compare_stored_report_for_check(stored: &AuditReport, fresh: &AuditReport) -> Result<()> {
    let stored_digest = audit_report_semantic_digest(stored)?;
    let fresh_digest = audit_report_semantic_digest(fresh)?;
    if stored_digest == fresh_digest {
        return Ok(());
    }
    bail!(
        "{STALE_REPORT_PREFIX}: semantic digest mismatch (stored {stored_digest}, current {fresh_digest})"
    );
}

fn semantic_projection(report: &AuditReport) -> Result<Value> {
    let mut value =
        serde_json::to_value(report).context("serializing corpus audit report for check digest")?;
    let metadata = value
        .get_mut("metadata")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| color_eyre::eyre::eyre!("corpus audit report is missing metadata object"))?;
    metadata.remove("generated_at");
    metadata.remove("duration_secs");
    Ok(value)
}

fn stable_json_digest(value: &Value) -> Result<String> {
    let mut canonical = String::new();
    write_canonical_json(value, &mut canonical)?;
    let digest = Sha256::digest(canonical.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02X}");
    }
    Ok(hex)
}

fn write_canonical_json(value: &Value, out: &mut String) -> Result<()> {
    match value {
        Value::Null => out.push_str("n;"),
        Value::Bool(flag) => {
            let _ = write!(out, "b:{};", if *flag { "True" } else { "False" });
        }
        Value::Number(number) => {
            if let Some(signed) = number.as_i64() {
                let _ = write!(out, "i:{signed};");
            } else if let Some(unsigned) = number.as_u64() {
                let _ = write!(out, "i:{unsigned};");
            } else if let Some(float) = number.as_f64() {
                let _ = write!(out, "f:{float:.17};");
            } else {
                bail!("corpus audit canonicalization cannot encode number {number}");
            }
        }
        Value::String(text) => {
            out.push_str("s:");
            for ch in text.chars() {
                match ch {
                    '\\' => out.push_str("\\\\"),
                    ';' => out.push_str("\\;"),
                    _ => out.push(ch),
                }
            }
            out.push(';');
        }
        Value::Array(items) => {
            out.push('[');
            for item in items {
                write_canonical_json(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for key in keys {
                out.push_str(key);
                out.push('=');
                write_canonical_json(&map[key], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::corpus_audit::corpus;
    use crate::tasks::corpus_audit::ga_alignment;
    use crate::tasks::corpus_audit::nodekind_analysis;
    use crate::tasks::corpus_audit::report;
    use std::collections::HashMap;

    fn minimal_report(generated_at: &str, duration_secs: u64, ok_count: usize) -> AuditReport {
        AuditReport {
            metadata: report::ReportMetadata {
                generated_at: generated_at.to_string(),
                version: "0.1.0".to_string(),
                duration_secs,
            },
            inventory: corpus::CorpusInventory {
                total_files: ok_count,
                files_by_layer: Vec::new(),
                total_size_bytes: 0,
                total_line_count: 0,
            },
            parse_outcomes: report::ParseOutcomesSummary {
                total: ok_count,
                ok: ok_count,
                error: 0,
                timeout: 0,
                panic: 0,
                error_by_category: HashMap::new(),
                failing_files: Vec::new(),
            },
            nodekind_coverage: nodekind_analysis::NodeKindStats {
                total_count: 1,
                covered_count: 1,
                coverage_percentage: 100.0,
                never_seen: Vec::new(),
                allowlisted_never_seen: Vec::new(),
                actionable_never_seen: Vec::new(),
                at_risk: Vec::new(),
                frequency: HashMap::new(),
            },
            ga_coverage: ga_alignment::GAFeatureCoverage {
                total_count: 1,
                covered_count: 1,
                coverage_percentage: 100.0,
                features: Vec::new(),
                uncovered_critical: Vec::new(),
                uncovered_partial: Vec::new(),
            },
            timeout_risks: Vec::new(),
        }
    }

    #[test]
    fn informational_metadata_does_not_change_semantic_digest() {
        let a = minimal_report("2026-01-01T00:00:00Z", 10, 3);
        let b = minimal_report("2026-11-02T12:34:56Z", 999, 3);
        assert_eq!(
            audit_report_semantic_digest(&a).expect("digest a"),
            audit_report_semantic_digest(&b).expect("digest b")
        );
    }

    #[test]
    fn semantic_digest_changes_when_payload_changes() {
        let stored = minimal_report("2026-01-01T00:00:00Z", 1, 3);
        let fresh = minimal_report("2026-01-01T00:00:00Z", 1, 4);
        let err = compare_stored_report_for_check(&stored, &fresh).expect_err("must reject stale");
        assert!(err.to_string().contains(STALE_REPORT_PREFIX), "unexpected error: {err}");
    }

    #[test]
    fn matching_semantic_payload_passes_compare() {
        let stored = minimal_report("2026-01-01T00:00:00Z", 1, 2);
        let fresh = minimal_report("2099-01-01T00:00:00Z", 500, 2);
        compare_stored_report_for_check(&stored, &fresh).expect("same semantic payload");
    }
}
