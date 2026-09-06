//! v0.18 text/position envelope decision packet (#8129).

use serde_json::Value;
use std::path::PathBuf;

fn packet_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.spec/8129-v0-18-text-sync-envelope/v0_18_text_sync_envelope.v1.json")
}

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/v0_18_text_sync_envelope.v1.schema.json")
}

#[test]
fn v0_18_text_sync_envelope_packet_matches_schema_required_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let packet: Value = serde_json::from_str(&std::fs::read_to_string(packet_path())?)?;
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(schema_path())?)?;

    assert_eq!(
        packet.get("schema_version").and_then(Value::as_str),
        Some("v0_18_text_sync_envelope.v1")
    );
    assert_eq!(
        schema.pointer("/properties/schema_version/const").and_then(Value::as_str),
        Some("v0_18_text_sync_envelope.v1")
    );
    assert_eq!(packet.get("release").and_then(Value::as_str), Some("0.18.0"));
    assert_eq!(packet.get("decision").and_then(Value::as_str), Some("full_document_utf16"));
    assert_eq!(packet.get("text_sync_kind").and_then(Value::as_str), Some("full"));

    let encodings =
        packet.get("active_encoding_set").and_then(Value::as_array).ok_or("active_encoding_set")?;
    assert_eq!(encodings, &vec![Value::String("utf-16".into())]);

    let sha = packet.get("subject_sha").and_then(Value::as_str).ok_or("subject_sha")?;
    assert_eq!(
        sha, "494faa8e9ab47e8b7b97035488fb5e4bd0aa5a6d",
        "subject_sha must stay the origin/main SHA against which A vs B was judged"
    );

    let limitations = packet
        .get("limitations")
        .and_then(Value::as_array)
        .ok_or("limitations")?
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        limitations.contains("subject_sha is origin/main"),
        "packet must state that subject_sha is the judged main, not the candidate: {limitations}"
    );

    let owners = packet
        .get("required_owner_issues")
        .and_then(Value::as_array)
        .ok_or("required_owner_issues")?;
    assert_eq!(owners, &vec![Value::from(8129)]);

    for field in [
        "production_authorities",
        "removed_or_disabled_authorities",
        "exact_process_receipts",
        "editor_receipts",
        "limitations",
    ] {
        let values = packet.get(field).and_then(Value::as_array).ok_or(field)?;
        assert!(!values.is_empty(), "{field} must be populated");
    }

    let required = schema.get("required").and_then(Value::as_array).ok_or("schema required")?;
    let properties =
        schema.get("properties").and_then(Value::as_object).ok_or("schema properties")?;
    let packet_obj = packet.as_object().ok_or("packet object")?;
    for key in packet_obj.keys() {
        assert!(
            properties.contains_key(key),
            "packet key {key:?} is not in schema properties (additionalProperties: false)"
        );
    }
    for field in required {
        let field = field.as_str().ok_or("required field name")?;
        assert!(packet_obj.contains_key(field), "packet missing required {field}");
    }

    let ceiling = packet.get("public_claim_ceiling").and_then(Value::as_str).ok_or("ceiling")?;
    assert!(ceiling.contains("full-document"), "{ceiling}");
    assert!(ceiling.contains("UTF-16"), "{ceiling}");
    assert!(!ceiling.to_ascii_lowercase().contains("incremental text-sync is claimed"));
    Ok(())
}

#[test]
fn v0_18_envelope_does_not_close_long_term_incremental_owners()
-> Result<(), Box<dyn std::error::Error>> {
    let packet: Value = serde_json::from_str(&std::fs::read_to_string(packet_path())?)?;
    let owners = packet
        .get("required_owner_issues")
        .and_then(Value::as_array)
        .ok_or("required_owner_issues")?;
    for forbidden in [1690_u64, 7409, 7417] {
        assert!(
            !owners.iter().any(|value| value.as_u64() == Some(forbidden)),
            "full-document decision must not claim to close #{forbidden}"
        );
    }
    let limitations = packet
        .get("limitations")
        .and_then(Value::as_array)
        .ok_or("limitations")?
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(limitations.contains("#1690/#7409/#7417"), "{limitations}");
    Ok(())
}
