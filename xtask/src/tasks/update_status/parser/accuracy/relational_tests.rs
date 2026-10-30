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
use xtask::parser_accuracy_legacy_population::LEGACY_QUARANTINED_METRICS;

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
                "quarantined_metrics": ["whitespace_invariance_rate", "comment_invariance_rate", "newline_style_invariance_rate"],
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

#[test]
fn quarantined_legacy_metrics_cannot_reappear_as_measured() {
    // The generator quarantines three legacy observations —
    // whitespace_invariance_rate, comment_invariance_rate and
    // newline_style_invariance_rate — but `legacy_population` names only the
    // whitespace one as its aggregate. The reader rejected a measured row only
    // when it matched that single name, so a measured comment or newline row
    // passed and rendered as trusted accuracy.
    let artifact = |extra: serde_json::Value| {
        serde_json::json!({
            "schema_version": 1,
            "subsystem": "parser_accuracy",
            "cadence": "pr",
            "denominator": denominator_json(),
            "families": [{ "family": "packages", "fixture_count": 4 }],
            "metrics": [
                {
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
                },
                extra,
            ],
            "legacy_population": {
                "transformation_profile": "trailing_horizontal_whitespace.legacy.v1",
                "population_identity": format!("sha256:{}", "a".repeat(64)),
                "aggregate_metric": "whitespace_invariance_rate",
                "quarantined_metrics": ["whitespace_invariance_rate", "comment_invariance_rate", "newline_style_invariance_rate"],
                "quarantined_metrics": [
                    "whitespace_invariance_rate",
                    "comment_invariance_rate",
                    "newline_style_invariance_rate",
                ],
                "population_total_count": 4,
                "population_applied_count": 2,
                "population_unclassified_count": 2,
                "manifest_schema_version": 1,
            },
            "failure_packets": [],
            "gold_drift": {},
            "metric_runtime": {},
        })
        .to_string()
    };
    let parse = |raw: &str| -> ParserAccuracyArtifactSummary {
        serde_json::from_str(raw).expect("well-shaped artifact must deserialize")
    };

    for metric in ["comment_invariance_rate", "newline_style_invariance_rate"] {
        let raw = artifact(serde_json::json!({
            "state": "measured",
            "metric": metric,
            "value": 1.0,
            "sample_count": 47,
        }));
        assert!(
            !trust_disposition_is_fail_closed(&parse(&raw)),
            "a measured {metric} must fail closed: it is quarantined evidence"
        );
    }

    // A *partial* declaration must be refused rather than obeyed. Otherwise the
    // fix above is only as good as the artifact's honesty: dropping a name from
    // `quarantined_metrics` would restore exactly the trust it removes.
    for omitted in ["comment_invariance_rate", "newline_style_invariance_rate"] {
        let declared: Vec<&str> =
            LEGACY_QUARANTINED_METRICS.iter().copied().filter(|m| *m != omitted).collect();
        let raw = artifact(serde_json::json!({
            "state": "measured",
            "metric": omitted,
            "value": 1.0,
            "sample_count": 47,
        }))
        .replace(
            &serde_json::to_string(&LEGACY_QUARANTINED_METRICS).unwrap_or_default(),
            &serde_json::to_string(&declared).unwrap_or_default(),
        );
        assert!(
            !trust_disposition_is_fail_closed(&parse(&raw)),
            "a declaration omitting {omitted} must be refused, not honoured"
        );
    }

    // Positive control: a metric outside the quarantine is ordinary trusted
    // accuracy and must still pass, so this is a declaration check rather than
    // the name classifier this contract retired. A near-miss name is the point:
    // resemblance must not downgrade anything.
    let near_miss = artifact(serde_json::json!({
        "state": "measured",
        "metric": "whitespace_invariance_rate_v2",
        "value": 1.0,
        "sample_count": 47,
    }));
    assert!(
        trust_disposition_is_fail_closed(&parse(&near_miss)),
        "a name merely resembling a quarantined metric must stay trusted"
    );

    let trusted = artifact(serde_json::json!({
        "state": "measured",
        "metric": "line_construct_f1",
        "value": 1.0,
        "sample_count": 125,
    }));
    assert!(
        trust_disposition_is_fail_closed(&parse(&trusted)),
        "a non-quarantined measured metric must still be trusted"
    );
}
