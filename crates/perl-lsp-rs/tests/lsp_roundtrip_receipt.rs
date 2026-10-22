//! Deterministic contract checks for the custom receipt, without spawning an LSP.

#[path = "../benches/support/roundtrip_receipt.rs"]
mod roundtrip_receipt;

use roundtrip_receipt::{build_receipt, duration_micros, format_lane_summary, summarise};
use serde_json::{Value, json};
use std::time::Duration;

#[test]
fn duration_conversion_retains_sub_millisecond_samples() {
    assert_eq!(duration_micros(Duration::from_micros(125)), 125);
    assert_eq!(duration_micros(Duration::from_micros(999)), 999);
    assert_eq!(duration_micros(Duration::from_micros(1_001)), 1_001);
    assert_eq!(duration_micros(Duration::from_nanos(125_999)), 125);
    assert_eq!(duration_micros(Duration::ZERO), 0);
    assert_eq!(duration_micros(Duration::MAX), u64::MAX);
}

#[test]
fn percentiles_keep_the_existing_rounded_index_convention() {
    // For ten ordered values, round(p * (n - 1)) chooses indices 5, 9, 9.
    // This differs from floor, interpolation, and nearest-rank at p50.
    let samples = [900, 100, 700, 300, 500, 200, 800, 400, 1_000, 600]
        .into_iter()
        .map(|us| duration_micros(Duration::from_micros(us)))
        .collect();
    assert_eq!(
        summarise(samples).to_json(),
        json!({"p50_us": 600, "p95_us": 1_000, "p99_us": 1_000, "samples": 10})
    );
    // Preserve the warm-request sample count's percentile values.
    assert_eq!(
        summarise((1..=50).rev().collect()).to_json(),
        json!({"p50_us": 26, "p95_us": 48, "p99_us": 50, "samples": 50})
    );
    // At the didOpen sample count, p95 rounds index 10.45 down to 10;
    // a ceiling index would incorrectly select value 12 instead of 11.
    assert_eq!(
        summarise((1..=12).rev().collect()).to_json(),
        json!({"p50_us": 7, "p95_us": 11, "p99_us": 12, "samples": 12})
    );
    assert_eq!(
        summarise(vec![]).to_json(),
        json!({"p50_us": 0, "p95_us": 0, "p99_us": 0, "samples": 0})
    );
    assert_eq!(
        summarise(vec![125]).to_json(),
        json!({"p50_us": 125, "p95_us": 125, "p99_us": 125, "samples": 1})
    );
}

#[test]
fn receipt_serialization_versions_microseconds_for_every_lane()
-> Result<(), Box<dyn std::error::Error>> {
    // Synthetic inputs exercise the production serializer; no benchmark receipt is written.
    let summary = summarise(vec![125, 750, 999]).to_json();
    let lanes: Vec<_> = [
        "initialize",
        "did_open_to_diagnostics",
        "hover",
        "completion",
        "definition",
        "workspace_symbol",
    ]
    .into_iter()
    .map(|lane| (lane, summary.clone()))
    .collect();
    let receipt = build_receipt(
        "synthetic-test-time".into(),
        "synthetic-test-commit".into(),
        json!({"fixture": "synthetic"}),
        json!({"index_ready": true}),
        json!({"request_samples_per_lane": 3}),
        &lanes,
    );
    let serialized = serde_json::to_string_pretty(&receipt)?;
    let decoded: Value = serde_json::from_str(&serialized)?;
    assert_eq!(
        decoded,
        json!({
            "schema_version": 2,
            "kind": "lsp_stdio_roundtrip",
            "measured_at": "synthetic-test-time",
            "commit": "synthetic-test-commit",
            "corpus": {"fixture": "synthetic"},
            "runtime": {"index_ready": true},
            "sampling": {"request_samples_per_lane": 3},
            "lanes": lanes.iter().map(|(name, _)| ((*name).to_string(), json!({
                "p50_us": 750, "p95_us": 999, "p99_us": 999, "samples": 3,
            }))).collect::<serde_json::Map<String, Value>>(),
            "timing_policy": "client-side wallclock send→response over stdio on a warm session; advisory baseline only — no wallclock assertion runs in cargo test (ux_latency_raw_rpc.rs timing policy, #17159)",
        })
    );
    assert_eq!(
        format_lane_summary("hover", &summary),
        "hover                      p50=  750us  p95=  999us  p99=  999us  samples=3"
    );
    Ok(())
}
