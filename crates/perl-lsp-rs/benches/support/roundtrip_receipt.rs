//! Pure custom round-trip receipt logic, shared with deterministic contract tests.

use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub(super) struct LatencySummary {
    p50_us: u64,
    p95_us: u64,
    p99_us: u64,
    samples: usize,
}

impl LatencySummary {
    pub(super) fn to_json(self) -> Value {
        json!({
            "p50_us": self.p50_us,
            "p95_us": self.p95_us,
            "p99_us": self.p99_us,
            "samples": self.samples,
        })
    }
}

/// Percentile convention shared with `tests/real_project_latency.rs`.
fn percentile(sorted: &[u64], pct: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((f64::from(pct as u8) / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

pub(super) fn summarise(mut samples: Vec<u64>) -> LatencySummary {
    let count = samples.len();
    samples.sort_unstable();
    LatencySummary {
        p50_us: percentile(&samples, 50),
        p95_us: percentile(&samples, 95),
        p99_us: percentile(&samples, 99),
        samples: count,
    }
}

/// Whole microseconds, saturating only when the duration cannot fit in u64.
/// Sub-microsecond fractions are truncated, as with Duration::as_micros.
pub(super) fn duration_micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

/// Construct the custom receipt; Criterion's independently measured history is separate.
pub(super) fn build_receipt(
    measured_at: String,
    commit: String,
    corpus: Value,
    runtime: Value,
    sampling: Value,
    lanes: &[(&str, Value)],
) -> Value {
    let mut lanes_map = serde_json::Map::new();
    for (lane, summary) in lanes {
        lanes_map.insert((*lane).to_string(), summary.clone());
    }

    json!({
        "schema_version": 2,
        "kind": "lsp_stdio_roundtrip",
        "measured_at": measured_at,
        "commit": commit,
        "corpus": corpus,
        "runtime": runtime,
        "sampling": sampling,
        "lanes": Value::Object(lanes_map),
        "timing_policy": "client-side wallclock send→response over stdio on a warm session; \
          advisory baseline only — no wallclock assertion runs in cargo test \
          (ux_latency_raw_rpc.rs timing policy, #17159)",
    })
}

pub(super) fn format_lane_summary(lane: &str, summary: &Value) -> String {
    format!(
        "{lane:<26} p50={:>5}us  p95={:>5}us  p99={:>5}us  samples={}",
        summary["p50_us"].as_u64().unwrap_or_default(),
        summary["p95_us"].as_u64().unwrap_or_default(),
        summary["p99_us"].as_u64().unwrap_or_default(),
        summary["samples"].as_u64().unwrap_or_default(),
    )
}
