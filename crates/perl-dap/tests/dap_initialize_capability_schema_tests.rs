//! #9534 initialize capability schema projection — edge cases and drift guards.

use perl_dap::initialize_capability_schema::{
    extension_initialize_capability_candidates, standard_initialize_capability_rows,
};
use std::collections::{BTreeMap, BTreeSet};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn standard_row_set_matches_pinned_upstream_capabilities_properties() -> TestResult {
    let rows = standard_initialize_capability_rows()?;
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../.ci/dap/upstream-debugAdapterProtocol.json"))?;
    let properties = schema["definitions"]["Capabilities"]["properties"]
        .as_object()
        .ok_or("upstream Capabilities.properties missing")?;
    let upstream: BTreeSet<String> = properties.keys().cloned().collect();
    let projected: BTreeSet<String> = rows.iter().map(|row| row.wire_name.clone()).collect();
    assert_eq!(
        projected, upstream,
        "standard projection must cover every upstream field exactly once"
    );
    Ok(())
}

#[test]
fn every_standard_row_links_to_a_nullability_inventory_row() -> TestResult {
    let nullability: serde_json::Value =
        serde_json::from_str(include_str!("../../../.ci/dap/nullability-inventory.v1.json"))?;
    let indexed: BTreeMap<(String, String), &serde_json::Value> = nullability["rows"]
        .as_array()
        .ok_or("nullability rows missing")?
        .iter()
        .filter_map(|row| {
            let definition = row.get("definition")?.as_str()?.to_owned();
            let field = row.get("field")?.as_str()?.to_owned();
            Some(((definition, field), row))
        })
        .collect();
    for row in standard_initialize_capability_rows()? {
        let key = (row.nullability_row.definition.clone(), row.nullability_row.field.clone());
        let null_row =
            indexed.get(&key).ok_or_else(|| format!("missing nullability row for {key:?}"))?;
        assert_eq!(
            null_row.get("class").and_then(|value| value.as_str()),
            Some(row.nullability_row.class.as_str()),
            "nullability class must stay referenced, not duplicated"
        );
    }
    Ok(())
}

#[test]
fn extension_candidates_remain_outside_the_standard_projection() -> TestResult {
    let standard: BTreeSet<String> =
        standard_initialize_capability_rows()?.iter().map(|row| row.wire_name.clone()).collect();
    let extensions = extension_initialize_capability_candidates()?;
    assert_eq!(extensions.len(), 1);
    assert_eq!(extensions[0].wire_name, "supportsInlineValues");
    assert!(
        !standard.contains("supportsInlineValues"),
        "custom extension must not enter the standard projection"
    );
    Ok(())
}

#[test]
fn array_capability_shapes_expose_item_identities() -> TestResult {
    let rows = standard_initialize_capability_rows()?;
    let filters = rows
        .iter()
        .find(|row| row.wire_name == "exceptionBreakpointFilters")
        .ok_or("missing exceptionBreakpointFilters row")?;
    let items = filters
        .wire_shape
        .items
        .as_ref()
        .ok_or("exceptionBreakpointFilters must declare array items")?;
    assert_eq!(items.kind, "definition");
    assert_eq!(items.definition.as_deref(), Some("ExceptionBreakpointsFilter"));

    let completion = rows
        .iter()
        .find(|row| row.wire_name == "completionTriggerCharacters")
        .ok_or("missing completionTriggerCharacters row")?;
    let completion_items = completion
        .wire_shape
        .items
        .as_ref()
        .ok_or("completionTriggerCharacters must declare array items")?;
    assert_eq!(completion_items.kind, "string");
    Ok(())
}

#[test]
fn request_families_are_preserved_for_named_capabilities() -> TestResult {
    let rows = standard_initialize_capability_rows()?;
    let cancel = rows
        .iter()
        .find(|row| row.wire_name == "supportsCancelRequest")
        .ok_or("missing supportsCancelRequest row")?;
    assert_eq!(cancel.related_requests, vec!["cancel".to_owned()]);
    Ok(())
}
