//! Strict decoding of the existing v1 payload, not another receipt authority.

use serde::Deserialize;

use super::{
    NATIVE_PRODUCT_KIND, NATIVE_PRODUCT_ROWS, NATIVE_PRODUCT_SCHEMA_VERSION,
    NativeProductReadiness, NativeProductRow, NativeProductVerdict, verdict_for,
};

/// Untrusted wire shape. Only the checked conversion constructs a parsed payload.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NativeProductReadinessWire {
    kind: String,
    schema_version: u32,
    verdict: NativeProductVerdict,
    rows: Vec<NativeProductRow>,
}

impl TryFrom<NativeProductReadinessWire> for NativeProductReadiness {
    type Error = String;

    fn try_from(wire: NativeProductReadinessWire) -> Result<Self, Self::Error> {
        if wire.kind != NATIVE_PRODUCT_KIND {
            return Err("invalid native-product payload kind".to_string());
        }
        if wire.schema_version != NATIVE_PRODUCT_SCHEMA_VERSION {
            return Err("unsupported native-product schema version".to_string());
        }
        // Validate before verdict_for: an empty slice otherwise looks all-pass.
        // Unlike from_rows, decoding must not repair a malformed evidence packet.
        if wire.rows.len() != NATIVE_PRODUCT_ROWS.len() {
            return Err(format!(
                "native-product row count must be {}, received {}",
                NATIVE_PRODUCT_ROWS.len(),
                wire.rows.len()
            ));
        }
        for (index, (row, expected)) in wire.rows.iter().zip(NATIVE_PRODUCT_ROWS).enumerate() {
            if row.id != expected {
                return Err(format!("native-product catalog row mismatch at index {index}"));
            }
        }
        if wire.verdict != verdict_for(&wire.rows) {
            return Err("native-product verdict disagrees with mandatory rows".to_string());
        }
        Ok(Self {
            kind: wire.kind,
            schema_version: wire.schema_version,
            verdict: wire.verdict,
            rows: wire.rows,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn packet() -> Value {
        json!({
            "kind": "native_product_readiness",
            "schema_version": 1,
            "verdict": "ready",
            "rows": NATIVE_PRODUCT_ROWS.map(|id| json!({
                "id": id,
                "status": "pass",
                "evidence": [{ "kind": "receipt", "value": "fixtures/native-default.json" }],
                "remediation": null,
                "limitation": "Posture only; no installed acceptance."
            }))
        })
    }

    fn rejects(value: Value, reason: &str) {
        let parsed = serde_json::from_value::<NativeProductReadiness>(value);
        assert!(
            matches!(&parsed, Err(error) if error.to_string().contains(reason)),
            "expected rejection containing {reason:?}, received {parsed:?}"
        );
    }

    #[test]
    fn valid_packet_preserves_rows_evidence_and_serialized_bytes() -> Result<(), serde_json::Error>
    {
        let value = packet();
        let parsed: NativeProductReadiness = serde_json::from_value(value.clone())?;
        assert_eq!(serde_json::to_value(&parsed)?, value);
        let serialized = parsed.to_json()?;
        let reparsed: NativeProductReadiness = serde_json::from_str(&serialized)?;
        assert_eq!(reparsed, parsed);
        assert_eq!(reparsed.to_json()?, serialized);
        Ok(())
    }

    #[test]
    fn wrong_kind_or_schema_cannot_enter_the_v1_domain() {
        for kind in ["", "native_product_readiness.v2", "perl_kwalitee"] {
            let mut value = packet();
            value["kind"] = json!(kind);
            rejects(value, "payload kind");
        }
        for version in [0, 2, u32::MAX] {
            let mut value = packet();
            value["schema_version"] = json!(version);
            rejects(value, "schema version");
        }
    }

    #[test]
    fn empty_or_incomplete_or_extra_rows_are_rejected_not_repaired() {
        for count in [0, 1, 2, 3, 5] {
            let mut value = packet();
            value["rows"] = json!((0..count).map(|_| value["rows"][0].clone()).collect::<Vec<_>>());
            rejects(value, "row count");
        }
        // Even a conservative verdict cannot turn malformed input into a valid packet.
        let mut value = packet();
        value["rows"] = json!([]);
        value["verdict"] = json!("not_proven");
        rejects(value, "row count");
    }

    #[test]
    fn exact_catalog_order_rejects_unknown_duplicate_and_swapped_rows() {
        for index in 0..NATIVE_PRODUCT_ROWS.len() {
            let mut unknown = packet();
            unknown["rows"][index]["id"] = json!("native_product.unknown");
            rejects(unknown, "catalog row mismatch");

            let mut duplicate = packet();
            duplicate["rows"][index]["id"] = json!(NATIVE_PRODUCT_ROWS[(index + 1) % 4]);
            rejects(duplicate, "catalog row mismatch");
        }
        for index in 0..3 {
            let mut value = packet();
            let first = value["rows"][index].clone();
            value["rows"][index] = value["rows"][index + 1].clone();
            value["rows"][index + 1] = first;
            rejects(value, "catalog row mismatch");
        }
    }

    #[test]
    fn all_status_combinations_accept_only_the_earned_verdict() -> Result<(), serde_json::Error> {
        // Literal wire oracle, independent of verdict_for and the Rust enum names.
        // Four statuses across four mandatory rows: 256 accepted, 768 rejected.
        assert_eq!(NATIVE_PRODUCT_ROWS.len(), 4, "v1 denominator changed");
        let statuses = ["pass", "warn", "unverified", "fail"];
        let verdicts = ["ready", "limited", "not_proven", "blocked"];
        for encoded in 0..256_usize {
            let codes = [encoded % 4, (encoded / 4) % 4, (encoded / 16) % 4, encoded / 64];
            let expected = if codes.contains(&3) {
                "blocked"
            } else if codes.contains(&2) {
                "not_proven"
            } else if codes.contains(&1) {
                "limited"
            } else {
                "ready"
            };
            for verdict in verdicts {
                let mut value = packet();
                for (index, code) in codes.iter().enumerate() {
                    value["rows"][index]["status"] = json!(statuses[*code]);
                }
                value["verdict"] = json!(verdict);
                if verdict == expected {
                    let parsed: NativeProductReadiness = serde_json::from_value(value.clone())?;
                    assert_eq!(serde_json::to_value(parsed)?, value);
                } else {
                    rejects(value, "verdict disagrees");
                }
            }
        }
        Ok(())
    }

    #[test]
    fn unknown_fields_remain_rejected_at_every_payload_layer() {
        let mut outer = packet();
        outer["unexpected"] = json!(true);
        rejects(outer, "unknown field");
        let mut row = packet();
        row["rows"][0]["unexpected"] = json!(true);
        rejects(row, "unknown field");
        let mut evidence = packet();
        evidence["rows"][0]["evidence"][0]["unexpected"] = json!(true);
        rejects(evidence, "unknown field");
    }

    #[test]
    fn duplicate_json_keys_cannot_overwrite_an_earlier_verdict() -> Result<(), serde_json::Error> {
        let encoded = serde_json::to_string(&packet())?;
        let duplicate = encoded.replacen('{', "{\"verdict\":\"blocked\",", 1);
        let parsed = serde_json::from_str::<NativeProductReadiness>(&duplicate);
        assert!(
            matches!(&parsed, Err(error) if error.to_string().contains("duplicate field")),
            "duplicate verdict must be rejected: {parsed:?}"
        );
        Ok(())
    }

    #[test]
    fn assembled_missing_rows_remain_valid_not_proven() -> Result<(), serde_json::Error> {
        let assembled = NativeProductReadiness::from_rows(Vec::new());
        let decoded: NativeProductReadiness = serde_json::from_str(&assembled.to_json()?)?;
        assert_eq!(decoded.verdict, NativeProductVerdict::NotProven);
        assert_eq!(decoded, assembled);
        Ok(())
    }
}
