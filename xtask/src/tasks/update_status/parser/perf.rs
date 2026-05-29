use std::fs;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub(in crate::tasks::update_status) struct ParserPerformanceScorecard {
    pub(super) generated_at_epoch_s: u64,
    pub(super) metrics: std::collections::BTreeMap<String, ParserPerfMetric>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::tasks::update_status::parser) struct ParserPerfMetric {
    pub(super) iterations: usize,
    pub(super) median_ns: u128,
    pub(super) p95_ns: u128,
    pub(super) mean_ns: u128,
}

pub(super) fn read_parser_performance_scorecard(root: &Path) -> Option<ParserPerformanceScorecard> {
    let path = root.join("docs/project/status/parser_performance_scorecard.json");
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn ns_to_ms(ns: u128) -> f64 {
    ns as f64 / 1_000_000.0
}

pub(super) fn format_perf_metric_row(name: &str, metric: Option<&ParserPerfMetric>) -> String {
    metric.map_or_else(
        || format!("| **{name}** | UNVERIFIED | benchmark receipt missing | `docs/project/status/parser_performance_scorecard.json` |"),
        |m| {
            format!(
                "| **{name}** | p50 {:.3} ms / p95 {:.3} ms | mean {:.3} ms over {} samples | `docs/project/status/parser_performance_scorecard.json` |",
                ns_to_ms(m.median_ns),
                ns_to_ms(m.p95_ns),
                ns_to_ms(m.mean_ns),
                m.iterations,
            )
        },
    )
}
