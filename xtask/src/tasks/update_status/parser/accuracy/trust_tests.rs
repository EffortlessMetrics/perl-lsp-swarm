//! Fail-closed controls for the legacy-population trust contract.
//!
//! Split from `accuracy.rs` for the 400-line gate in `update_status::mod_tests`;
//! row and artifact builders are shared with the sibling `tests` module.

use super::{ParserAccuracyArtifactSummary, trust_disposition_is_fail_closed};

/// The denominator every fixture here carries. Its values are irrelevant to the
/// trust contract, but the reader models all thirteen schema fields strictly, so
/// a missing one would fail deserialization for an unrelated reason.
pub(super) fn denominator_json() -> serde_json::Value {
    serde_json::json!({
        "fixture_count": 4,
        "fixture_family_count": 1,
        "scored_line_count": 4,
        "scored_symbol_count": 2,
        "fully_labeled_region_count": 1,
        "partial_labeled_region_count": 1,
        "unknown_region_count": 1,
        "negative_region_count": 1,
        "dynamic_boundary_case_count": 0,
        "unsupported_construct_case_count": 0,
        "real_project_file_count": 0,
        "generated_fixture_count": 0,
        "hand_labeled_fixture_count": 4,
    })
}

#[test]
fn fail_closed_reader_rejects_missing_or_stale_population_evidence() {
    // The base artifact is deliberately VALID and asserted so below. It
    // previously carried no aggregate row at all, so every case in this test
    // failed whether or not its own mutation was present.
    let artifact_json = |legacy_population: serde_json::Value,
                         extra_metrics: Vec<serde_json::Value>| {
        let mut metrics = vec![
            serde_json::json!({
                "state": "insufficient_data",
                "metric": "line_construct_f1",
                "reason": "line-level gold scorer is not wired yet",
                "sample_count": 0,
                "confidence": "low",
            }),
            serde_json::json!({
                "state": "investigation_only",
                "metric": "whitespace_invariance_rate",
                "value": 0.5,
                // Tracks the population block, so a control that mutates only
                // the counts is not also failing the applied-count check and
                // thereby proving nothing about its own subject.
                "sample_count": legacy_population
                    .get("population_applied_count")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(2),
                "transformation_profile": "trailing_horizontal_whitespace.legacy.v1",
                "evidence_class": "investigation_only",
                "terminal_disposition": "not_proven",
                "reason": "legacy_hash_oracle_untrusted",
                "packet_policy": "none",
                "floor_eligible": false,
            }),
        ];
        metrics.extend(extra_metrics);
        serde_json::json!({
            "schema_version": 1,
            "subsystem": "parser_accuracy",
            "cadence": "pr",
            "denominator": denominator_json(),
            "families": [{ "family": "packages", "fixture_count": 4 }],
            "metrics": metrics,
            "legacy_population": legacy_population,
            "failure_packets": [],
            "gold_drift": {},
            "metric_runtime": {},
        })
        .to_string()
    };
    let population_json = |overrides: serde_json::Value| {
        let mut base = serde_json::json!({
            "transformation_profile": "trailing_horizontal_whitespace.legacy.v1",
            "population_identity": format!("sha256:{}", "a".repeat(64)),
            "aggregate_metric": "whitespace_invariance_rate",
            "quarantined_metrics": ["whitespace_invariance_rate"],
            "population_total_count": 4,
            "population_applied_count": 2,
            "population_unclassified_count": 2,
            "manifest_schema_version": 1,
        });
        if let (Some(fields), serde_json::Value::Object(overrides)) =
            (base.as_object_mut(), overrides)
        {
            for (key, value) in overrides {
                fields.insert(key, value);
            }
        }
        base
    };
    let parse = |raw: &str| -> ParserAccuracyArtifactSummary {
        serde_json::from_str(raw).expect("artifact with a well-shaped population must deserialize")
    };

    // Positive control. Without this every case below could pass against a base
    // that was already failing for an unrelated reason.
    let valid = artifact_json(population_json(serde_json::json!({})), Vec::new());
    assert!(
        trust_disposition_is_fail_closed(&parse(&valid)),
        "the base artifact must be valid, or the negative controls prove nothing"
    );

    // A missing population block must not deserialize to a readable artifact.
    let missing = artifact_json(serde_json::json!({}), Vec::new());
    assert!(
        serde_json::from_str::<ParserAccuracyArtifactSummary>(&missing).is_err(),
        "artifact without retained population evidence must fail closed"
    );

    let population_cases: [(&str, serde_json::Value); 6] = [
        (
            "counts that do not close over retained rows",
            serde_json::json!({ "population_unclassified_count": 1 }),
        ),
        (
            "an identity that is not a sha256-tagged digest",
            serde_json::json!({ "population_identity": "legacy-digest" }),
        ),
        // The schema pins `^sha256:[0-9a-f]{64}$`, so an `is_ascii_hexdigit`
        // check would admit an artifact the schema rejects.
        (
            "an all-uppercase digest",
            serde_json::json!({ "population_identity": format!("sha256:{}", "A".repeat(64)) }),
        ),
        (
            "a single uppercase digit in the digest",
            serde_json::json!({
                "population_identity": format!("sha256:A{}", "a".repeat(63)),
            }),
        ),
        // A *forged* population: `u64::MAX + 5` wraps to the declared total of
        // 4, and the aggregate row's sample count tracks the applied count, so
        // every other invariant holds and only checked addition can reject it.
        (
            "counts forged to close by wrapping",
            serde_json::json!({
                "population_total_count": 4,
                "population_applied_count": u64::MAX,
                "population_unclassified_count": 5,
            }),
        ),
        (
            "an empty retained population",
            serde_json::json!({
                "population_total_count": 0,
                "population_applied_count": 0,
                "population_unclassified_count": 0,
            }),
        ),
    ];
    for (label, overrides) in population_cases {
        let raw = artifact_json(population_json(overrides), Vec::new());
        assert!(!trust_disposition_is_fail_closed(&parse(&raw)), "{label} must fail closed");
    }

    // Two aggregate rows, each individually well formed, leave the reported
    // value to array order.
    let duplicate = artifact_json(
        population_json(serde_json::json!({})),
        vec![serde_json::json!({
            "state": "investigation_only",
            "metric": "whitespace_invariance_rate",
            "value": 0.9,
            "sample_count": 2,
            "transformation_profile": "trailing_horizontal_whitespace.legacy.v1",
            "evidence_class": "investigation_only",
            "terminal_disposition": "not_proven",
            "reason": "legacy_hash_oracle_untrusted",
            "packet_policy": "none",
            "floor_eligible": false,
        })],
    );
    assert!(
        !trust_disposition_is_fail_closed(&parse(&duplicate)),
        "a duplicated aggregate row must fail closed"
    );

    // The schema forbids extra properties here. `confidence` is legal on
    // measured and insufficient rows but not on this variant.
    let stray_field = artifact_json(
        population_json(serde_json::json!({ "aggregate_metric": "comment_invariance_rate" })),
        vec![serde_json::json!({
            "state": "investigation_only",
            "metric": "comment_invariance_rate",
            "value": 0.5,
            "sample_count": 2,
            "transformation_profile": "trailing_horizontal_whitespace.legacy.v1",
            "evidence_class": "investigation_only",
            "terminal_disposition": "not_proven",
            "reason": "legacy_hash_oracle_untrusted",
            "packet_policy": "none",
            "floor_eligible": false,
            "confidence": "high",
        })],
    );
    assert!(
        !trust_disposition_is_fail_closed(&parse(&stray_field)),
        "an investigation row carrying a schema-forbidden field must fail closed"
    );
}

#[test]
fn zero_applied_population_reports_instead_of_failing() {
    // A population whose fixtures are all excluded by the legacy whitespace
    // heuristic has nothing to observe, so the aggregate is honestly
    // `insufficient_data`. Rejecting that fails a valid custom `--manifest`.
    let artifact = |aggregate_row: serde_json::Value| {
        serde_json::json!({
            "schema_version": 1,
            "subsystem": "parser_accuracy",
            "cadence": "pr",
            "denominator": denominator_json(),
            "families": [{ "family": "packages", "fixture_count": 4 }],
            "metrics": [aggregate_row],
            "legacy_population": {
                "transformation_profile": "trailing_horizontal_whitespace.legacy.v1",
                "population_identity": format!("sha256:{}", "a".repeat(64)),
                "aggregate_metric": "whitespace_invariance_rate",
                "quarantined_metrics": ["whitespace_invariance_rate"],
                "population_total_count": 4,
                "population_applied_count": 0,
                "population_unclassified_count": 4,
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

    let insufficient = artifact(serde_json::json!({
        "state": "insufficient_data",
        "metric": "whitespace_invariance_rate",
        "reason": "no retained fixture matched the legacy whitespace profile",
        "sample_count": 0,
        "confidence": "low",
    }));
    assert!(
        trust_disposition_is_fail_closed(&parse(&insufficient)),
        "a zero-applied population must report, not fail the artifact"
    );

    // Opposite-direction control: with nothing applied there is nothing to
    // have investigated, so an investigation row is a claim about evidence
    // that does not exist.
    let investigated = artifact(serde_json::json!({
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
    }));
    assert!(
        !trust_disposition_is_fail_closed(&parse(&investigated)),
        "a zero-applied population must not carry investigation evidence"
    );

    // And a measured aggregate stays trusted accuracy by another name at
    // any applied count.
    let measured = artifact(serde_json::json!({
        "state": "measured",
        "metric": "whitespace_invariance_rate",
        "value": 1.0,
        "sample_count": 4,
    }));
    assert!(
        !trust_disposition_is_fail_closed(&parse(&measured)),
        "a measured aggregate must fail closed at any applied count"
    );
}
