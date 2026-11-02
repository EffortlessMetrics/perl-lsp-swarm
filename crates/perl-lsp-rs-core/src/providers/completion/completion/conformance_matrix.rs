//! Completion policy layer for the #13246 matrix (same crate as lexical classifiers).

use super::*;
use serde_json::Value;
use std::collections::BTreeSet;

const MATRIX: &str =
    include_str!("../../../../../../.ci/completion-source-context-conformance/cases.json");

fn matrix() -> Result<Value, serde_json::Error> {
    serde_json::from_str(MATRIX)
}

fn probe_offset(source: &str, marker: &str) -> Result<usize, String> {
    let start = source.find(marker).ok_or_else(|| format!("probe marker `{marker}` not found"))?;
    Ok(start + marker.len())
}

fn row_has_layer(row: &Value, layer: &str) -> bool {
    row["layers"]
        .as_array()
        .is_some_and(|layers| layers.iter().any(|entry| entry.as_str() == Some(layer)))
}

fn observe_broad_policy(source: &str, position: usize) -> &'static str {
    if invalid_string_position(source, position) {
        return "invalid_or_out_of_bounds";
    }
    if is_in_comment(source, position) {
        return "suppress";
    }
    if is_in_pod(source, position) || is_in_heredoc(source, position) {
        return "suppress";
    }
    if is_in_string(source, position) {
        return "suppress";
    }
    if is_in_regex(source, position) {
        return "suppress";
    }
    "allow"
}

fn observe_specialized_context(source: &str, position: usize) -> &'static str {
    if is_in_regex(source, position) && !is_in_string(source, position) {
        return "regex";
    }
    "none"
}

#[test]
fn completion_source_context_policy_matrix_matches_authored_dispositions()
-> Result<(), Box<dyn std::error::Error>> {
    let doc = matrix()?;
    let rows = doc["rows"].as_array().ok_or("missing rows")?;
    let ids: BTreeSet<_> = rows.iter().filter_map(|row| row["id"].as_str()).collect();
    if ids.len() != rows.len() {
        return Err("duplicate row ids".into());
    }
    let mut executed = 0usize;
    for row in rows {
        if !row_has_layer(row, "completion_policy") {
            continue;
        }
        if row.get("completion_policy_mismatch_owner").and_then(|v| v.as_u64()).is_some() {
            continue;
        }
        let source = row["source"].as_str().ok_or("missing source")?;
        let marker = row["probe_marker"].as_str().ok_or("missing probe_marker")?;
        let offset = probe_offset(source, marker)?;
        let expected_broad =
            row["completion_policy"]["broad"].as_str().ok_or("missing completion_policy.broad")?;
        let expected_specialized = row["completion_policy"]["specialized"]
            .as_str()
            .ok_or("missing completion_policy.specialized")?;
        let actual_broad = observe_broad_policy(source, offset);
        let actual_specialized = observe_specialized_context(source, offset);
        if actual_broad != expected_broad {
            return Err(format!(
                "{}: expected broad policy {expected_broad}, observed {actual_broad} at offset {offset}",
                row["id"]
            )
            .into());
        }
        if actual_specialized != expected_specialized {
            return Err(format!(
                "{}: expected specialized {expected_specialized}, observed {actual_specialized}",
                row["id"]
            )
            .into());
        }
        executed += 1;
    }
    if executed == 0 {
        return Err("completion policy layer selected zero rows (NOT_PROVEN)".into());
    }
    Ok(())
}

#[test]
fn completion_source_context_policy_mutations_fail_named_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let doc = matrix()?;
    let rows = doc["rows"].as_array().ok_or("missing rows")?;
    let row_by_id = |id: &str| -> Result<&Value, String> {
        rows.iter()
            .find(|row| row["id"].as_str() == Some(id))
            .ok_or_else(|| format!("missing row `{id}`"))
    };
    for mutation in doc["mutations"].as_array().ok_or("missing mutations")? {
        let row_id = mutation["row"].as_str().ok_or("missing mutation.row")?;
        let wrong = mutation["field"].as_str().ok_or("missing mutation.field")?;
        let wrong_value = mutation["wrong"].as_str().ok_or("missing mutation.wrong")?;
        let row = row_by_id(row_id)?;
        let source = row["source"].as_str().ok_or("missing source")?;
        let marker = row["probe_marker"].as_str().ok_or("missing probe_marker")?;
        let offset = probe_offset(source, marker)?;
        let expected_broad =
            row["completion_policy"]["broad"].as_str().ok_or("missing completion_policy.broad")?;
        if wrong != "completion_policy.broad" {
            return Err(format!("unsupported mutation field `{wrong}`").into());
        }
        if observe_broad_policy(source, offset) != expected_broad {
            return Err(format!("row `{row_id}` already fails before mutation planting").into());
        }
        if wrong_value == expected_broad {
            return Err(format!("mutation `{}` does not alter disposition", mutation["id"]).into());
        }
    }
    Ok(())
}
