//! Relational controls: rows and packets against the retained population.
//!
//! Split from `trust_tests.rs` for the 400-line gate in
//! `update_status::mod_tests`. These cases are about how one row or packet
//! relates to another and to the population block, rather than about the
//! population's own shape.

use super::tests::{
    artifact_with_rows, investigation_row, investigation_row_with_disposition, valid_population,
};
use super::trust_tests::denominator_json;
use super::{
    ParserAccuracyArtifactSummary, ParserAccuracyLegacyPopulation, ParserAccuracyMetricSummary,
    trust_disposition_is_fail_closed,
};

#[test]
fn fail_closed_reader_rejects_untyped_aggregates_and_stale_counts() {
    let population = valid_population();

    // The aggregate serialized as trusted measured evidence is rejected.
    let trusted_aggregate = artifact_with_rows(
        vec![ParserAccuracyMetricSummary::Measured {
            metric: "whitespace_invariance_rate".to_string(),
            value: 0.4,
            sample_count: 2,
            unmodeled_fields: serde_json::Map::new(),
        }],
        population.clone(),
    );
    assert!(
        !trust_disposition_is_fail_closed(&trusted_aggregate),
        "a legacy aggregate serialized as trusted measured evidence must fail closed"
    );

    // A retained population without any typed aggregate row is rejected.
    let missing_aggregate = artifact_with_rows(vec![], population.clone());
    assert!(
        !trust_disposition_is_fail_closed(&missing_aggregate),
        "a population without a typed aggregate row must fail closed"
    );

    // A stale aggregate count (from an older population) is rejected even
    // though the row itself is typed investigation evidence.
    let stale_aggregate = artifact_with_rows(
        vec![investigation_row("whitespace_invariance_rate", 0.4, 3)],
        population.clone(),
    );
    assert!(
        !trust_disposition_is_fail_closed(&stale_aggregate),
        "an aggregate from an older population must fail closed"
    );

    // An aggregate bound to a different profile than the retained
    // population is rejected.
    let mismatched_profile = artifact_with_rows(
        vec![investigation_row("whitespace_invariance_rate", 0.4, 2)],
        ParserAccuracyLegacyPopulation {
            transformation_profile: "newline_style.legacy.v1".to_string(),
            ..population.clone()
        },
    );
    assert!(
        !trust_disposition_is_fail_closed(&mismatched_profile),
        "an aggregate observed under another profile must fail closed"
    );
}

#[test]
fn fail_closed_reader_rejects_unknown_dispositions_and_floor_admission() {
    let population = valid_population();

    for (label, row) in [
        (
            "unknown evidence class",
            investigation_row_with_disposition(
                "whitespace_invariance_rate",
                0.4,
                2,
                "trusted",
                "not_proven",
                "none",
                false,
            ),
        ),
        (
            "unknown terminal disposition",
            investigation_row_with_disposition(
                "whitespace_invariance_rate",
                0.4,
                2,
                "investigation_only",
                "pass",
                "none",
                false,
            ),
        ),
        (
            "non-none packet policy",
            investigation_row_with_disposition(
                "whitespace_invariance_rate",
                0.4,
                2,
                "investigation_only",
                "not_proven",
                "defect",
                false,
            ),
        ),
        (
            "floor-admitted investigation row",
            investigation_row_with_disposition(
                "whitespace_invariance_rate",
                0.4,
                2,
                "investigation_only",
                "not_proven",
                "none",
                true,
            ),
        ),
    ] {
        let artifact = artifact_with_rows(vec![row], population.clone());
        assert!(!trust_disposition_is_fail_closed(&artifact), "{label} must fail closed");
    }
}

#[test]
fn projected_rows_and_failure_packets_cannot_carry_investigation_claims() {
    // Two relational invariants the generator already enforces and the reader
    // did not, so an artifact the generator would refuse still rendered.
    let artifact = |rows: serde_json::Value, packets: serde_json::Value| {
        serde_json::json!({
            "schema_version": 1,
            "subsystem": "parser_accuracy",
            "cadence": "pr",
            "denominator": denominator_json(),
            "families": [{ "family": "packages", "fixture_count": 4 }],
            "metrics": rows,
            "legacy_population": {
                "transformation_profile": "trailing_horizontal_whitespace.legacy.v1",
                "population_identity": format!("sha256:{}", "a".repeat(64)),
                "aggregate_metric": "whitespace_invariance_rate",
                "population_total_count": 4,
                "population_applied_count": 2,
                "population_unclassified_count": 2,
                "manifest_schema_version": 1,
            },
            "failure_packets": packets,
            "gold_drift": {},
            "metric_runtime": {},
        })
        .to_string()
    };
    let aggregate = serde_json::json!({
        "state": "investigation_only",
        "metric": "whitespace_invariance_rate",
        "value": 0.5,
        "sample_count": 2,
        "transformation_profile": "trailing_horizontal_whitespace.legacy.v1",
        "evidence_class": "investigation_only",
        "terminal_disposition": "not_proven",
        "reason": "legacy_hash_oracle_untrusted",
        "packet_policy": "none",
        "floor_eligible": false,
    });
    let parse = |raw: &str| -> ParserAccuracyArtifactSummary {
        serde_json::from_str(raw).expect("well-shaped artifact must deserialize")
    };

    // Positive control, and it also pins the projection: `confidence`, `delta`
    // and `floor` are legal on a measured row and this reader does not model
    // them, so they must pass. Rejecting every unknown key here would break
    // every real artifact.
    let projected = artifact(
        serde_json::json!([
            aggregate,
            {
                "state": "measured",
                "metric": "line_construct_f1",
                "value": 1.0,
                "sample_count": 125,
                "confidence": "high",
                "delta": 0.01,
                "floor": 0.9,
            },
        ]),
        serde_json::json!([]),
    );
    assert!(
        trust_disposition_is_fail_closed(&parse(&projected)),
        "unmodeled but schema-legal fields must not reject a valid artifact"
    );

    // The schema allows these five only on an investigation row, so each is a
    // contradictory trust claim on a measured row.
    for field in [
        "evidence_class",
        "terminal_disposition",
        "packet_policy",
        "floor_eligible",
        "transformation_profile",
    ] {
        let mut row = serde_json::json!({
            "state": "measured",
            "metric": "line_construct_f1",
            "value": 1.0,
            "sample_count": 125,
        });
        if let Some(object) = row.as_object_mut() {
            object.insert((*field).to_string(), serde_json::json!("not_proven"));
        }
        let raw = artifact(serde_json::json!([aggregate, row]), serde_json::json!([]));
        assert!(
            !trust_disposition_is_fail_closed(&parse(&raw)),
            "a measured row carrying {field} must fail closed"
        );
    }

    // An investigation row declares `packet_policy: none`, so a failure packet
    // naming one reports an active parser defect against evidence that emits
    // none. The generator already refuses this.
    let packeted = artifact(
        serde_json::json!([aggregate]),
        serde_json::json!([{ "metric": "whitespace_invariance_rate", "fixture_id": "f" }]),
    );
    assert!(
        !trust_disposition_is_fail_closed(&parse(&packeted)),
        "a failure packet naming an investigation metric must fail closed"
    );

    // A packet against a trusted metric, or carrying no metric at all, is
    // ordinary and must still pass.
    let unrelated = artifact(
        serde_json::json!([aggregate]),
        serde_json::json!([
            { "metric": "line_construct_f1", "fixture_id": "f" },
            { "fixture_id": "g" },
        ]),
    );
    assert!(
        trust_disposition_is_fail_closed(&parse(&unrelated)),
        "packets against trusted or unnamed metrics must not reject the artifact"
    );
}
