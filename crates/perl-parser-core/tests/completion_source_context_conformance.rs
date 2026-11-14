//! Canonical source-region layer for the #13246 completion source-context matrix.

use perl_parser_core::{SourceRegionIndex, SourceRegionKind};
use serde_json::Value;
use std::collections::BTreeSet;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const MATRIX: &str = include_str!("../../../.ci/completion-source-context-conformance/cases.json");

fn matrix() -> Result<Value, serde_json::Error> {
    serde_json::from_str(MATRIX)
}

fn probe_offset(source: &str, marker: &str) -> Result<usize, String> {
    let start = source.find(marker).ok_or_else(|| format!("probe marker `{marker}` not found"))?;
    Ok(start + marker.len())
}

fn parse_region_kind(name: &str) -> Result<SourceRegionKind, String> {
    match name {
        "Code" => Ok(SourceRegionKind::Code),
        "Pod" => Ok(SourceRegionKind::Pod),
        "LineComment" => Ok(SourceRegionKind::LineComment),
        "StringLiteral" => Ok(SourceRegionKind::StringLiteral),
        "QuoteLike" => Ok(SourceRegionKind::QuoteLike),
        "RegexLike" => Ok(SourceRegionKind::RegexLike),
        "Heredoc" => Ok(SourceRegionKind::Heredoc),
        "DataSection" => Ok(SourceRegionKind::DataSection),
        other => Err(format!("unknown region_kind `{other}`")),
    }
}

fn row_has_layer(row: &Value, layer: &str) -> bool {
    row["layers"]
        .as_array()
        .is_some_and(|layers| layers.iter().any(|entry| entry.as_str() == Some(layer)))
}

#[test]
fn matrix_schema_and_denominator_are_well_formed() -> TestResult {
    let doc = matrix()?;
    if doc["schema"].as_str() != Some("completion-source-context-conformance/v1") {
        return Err("wrong matrix schema".into());
    }
    let rows = doc["rows"].as_array().ok_or("missing rows")?;
    let ids: BTreeSet<_> = rows.iter().filter_map(|row| row["id"].as_str()).collect();
    if ids.len() != rows.len() {
        return Err("duplicate or missing row ids".into());
    }
    let declared: BTreeSet<_> = doc["denominator"]["row_ids"]
        .as_array()
        .ok_or("missing denominator.row_ids")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    if declared != ids {
        return Err("denominator row_ids drift".into());
    }
    for row in rows {
        if !row["canonical"]["region_kind"].is_string() {
            return Err(format!("{} missing canonical.region_kind", row["id"]).into());
        }
        if let Some(opposite) = row.get("opposite").and_then(Value::as_str) {
            if !ids.contains(opposite) {
                return Err(format!("{} opposite `{opposite}` missing", row["id"]).into());
            }
        }
    }
    Ok(())
}

#[test]
fn canonical_layer_matches_authored_expectations_for_green_rows() -> TestResult {
    let doc = matrix()?;
    let rows = doc["rows"].as_array().ok_or("missing rows")?;
    let mut executed = 0usize;
    for row in rows {
        if !row_has_layer(row, "canonical") {
            continue;
        }
        if row.get("canonical_mismatch_owner").and_then(|v| v.as_u64()).is_some() {
            continue;
        }
        let source = row["source"].as_str().ok_or("missing source")?;
        let marker = row["probe_marker"].as_str().ok_or("missing probe_marker")?;
        let offset = probe_offset(source, marker)?;
        let expected =
            row["canonical"]["region_kind"].as_str().ok_or("missing canonical.region_kind")?;
        let expected_kind = parse_region_kind(expected)?;
        let actual = SourceRegionIndex::build(source).kind_at_offset(offset);
        if actual != expected_kind {
            return Err(format!(
                "{}: expected canonical {expected_kind:?} at offset {offset}, observed {actual:?}",
                row["id"]
            )
            .into());
        }
        executed += 1;
    }
    if executed == 0 {
        return Err("canonical layer selected zero rows (NOT_PROVEN)".into());
    }
    Ok(())
}

#[test]
fn canonical_mismatch_rows_remain_authored_and_named() -> TestResult {
    let doc = matrix()?;
    let rows = doc["rows"].as_array().ok_or("missing rows")?;
    let mut owners = 0usize;
    for row in rows {
        if row.get("canonical_mismatch_owner").and_then(|v| v.as_u64()).is_some() {
            owners += 1;
            if row["canonical_mismatch_owner"].as_u64() != Some(13241) {
                return Err(format!("{} unexpected canonical_mismatch_owner", row["id"]).into());
            }
        }
    }
    if owners == 0 {
        return Err("expected at least one canonical mismatch owner row".into());
    }
    Ok(())
}
