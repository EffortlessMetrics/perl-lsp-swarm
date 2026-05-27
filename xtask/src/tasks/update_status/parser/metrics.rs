use std::fs;
use std::ops::Deref;
use std::path::Path;
use std::time::Duration;

use regex::Regex;
use serde::Deserialize;

use super::super::token::TokenHealthMetrics;
use super::accuracy::{ParserAccuracyArtifactSummary, read_parser_accuracy_artifact};

pub(in crate::tasks::update_status) struct ParserMetrics {
    pub(in crate::tasks::update_status::parser) syntax_sections: usize,
    pub(in crate::tasks::update_status::parser) system_receipt: Option<ParserSweepReceipt>,
    pub(in crate::tasks::update_status::parser) cpan_receipt: Option<ParserSweepReceipt>,
    pub(in crate::tasks::update_status::parser) project_corpus:
        Option<super::super::super::corpus_audit::StatusSummary>,
    /// Receipt from `just common-corpus-check` - the strict-clean pinned-module gate.
    pub(in crate::tasks::update_status::parser) common_corpus_receipt: Option<ParserSweepReceipt>,
    /// Number of pinned modules in `.ci/common-corpus-manifest.txt`.
    pub(in crate::tasks::update_status::parser) common_corpus_pinned: usize,
    pub(in crate::tasks::update_status::parser) performance_scorecard:
        Option<ParserPerformanceScorecard>,
    pub(in crate::tasks::update_status::parser) parser_accuracy:
        Option<ParserAccuracyArtifactSummary>,
    pub(in crate::tasks::update_status::parser) token_metrics: TokenHealthMetrics,
}

#[derive(Debug, Clone)]
pub(in crate::tasks::update_status::parser) struct ParserSweepReceipt {
    report: super::super::super::parser_corpus_sweep::SweepReport,
    pub(in crate::tasks::update_status::parser) has_recovery_shape: bool,
}

impl ParserSweepReceipt {
    #[cfg(test)]
    pub(in crate::tasks::update_status::parser) fn with_recovery_shape(
        report: super::super::super::parser_corpus_sweep::SweepReport,
    ) -> Self {
        Self { report, has_recovery_shape: true }
    }

    #[cfg(test)]
    pub(in crate::tasks::update_status::parser) fn without_recovery_shape(
        report: super::super::super::parser_corpus_sweep::SweepReport,
    ) -> Self {
        Self { report, has_recovery_shape: false }
    }
}

impl Deref for ParserSweepReceipt {
    type Target = super::super::super::parser_corpus_sweep::SweepReport;

    fn deref(&self) -> &Self::Target {
        &self.report
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::tasks::update_status::parser) struct ParserPerformanceScorecard {
    pub(in crate::tasks::update_status::parser) generated_at_epoch_s: u64,
    pub(in crate::tasks::update_status::parser) metrics:
        std::collections::BTreeMap<String, ParserPerfMetric>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::tasks::update_status::parser) struct ParserPerfMetric {
    pub(in crate::tasks::update_status::parser) iterations: usize,
    pub(in crate::tasks::update_status::parser) median_ns: u128,
    pub(in crate::tasks::update_status::parser) p95_ns: u128,
    pub(in crate::tasks::update_status::parser) mean_ns: u128,
}

pub(in crate::tasks::update_status) fn collect_parser_metrics(root: &Path) -> ParserMetrics {
    let common_corpus_receipt =
        read_sweep_report(&root.join("target/receipts/common-corpus-sweep.json"));
    let common_corpus_pinned = count_common_corpus_pinned(root);
    ParserMetrics {
        syntax_sections: count_corpus_sections(root),
        system_receipt: read_sweep_report(&root.join(".ci/parser-corpus-baseline.json")),
        cpan_receipt: read_sweep_report(&root.join(".ci/cpan-corpus-baseline.json")),
        project_corpus: super::super::super::corpus_audit::compute_status_summary(
            root,
            Duration::from_secs(5),
        )
        .ok(),
        common_corpus_receipt,
        common_corpus_pinned,
        performance_scorecard: read_parser_performance_scorecard(root),
        parser_accuracy: read_parser_accuracy_artifact(root),
        token_metrics: super::super::token::collect_token_health_metrics(root),
    }
}

/// Count the non-comment, non-blank lines in `.ci/common-corpus-manifest.txt`.
pub(in crate::tasks::update_status::parser) fn count_common_corpus_pinned(root: &Path) -> usize {
    let path = root.join(".ci/common-corpus-manifest.txt");
    let Ok(raw) = fs::read_to_string(path) else {
        return 0;
    };
    raw.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).count()
}

pub(in crate::tasks::update_status::parser) fn read_sweep_report(
    path: &Path,
) -> Option<ParserSweepReceipt> {
    let raw = fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    let has_recovery_shape = value.get("files_with_structured_recovery_only").is_some()
        && value.get("files_with_error_nodes").is_some()
        && value.get("files_with_catastrophic_parse_failure").is_some()
        && value.get("total_dirty_files").is_some();
    let report = serde_json::from_value(value).ok()?;
    Some(ParserSweepReceipt { report, has_recovery_shape })
}

fn read_parser_performance_scorecard(root: &Path) -> Option<ParserPerformanceScorecard> {
    let path = root.join("docs/project/status/parser_performance_scorecard.json");
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub(in crate::tasks::update_status::parser) fn count_corpus_sections(root: &Path) -> usize {
    let corpus_dir = root.join("tree-sitter-perl/test/corpus");
    let marker = Regex::new(r"^=+\s*$").ok();
    let mut total: usize = 0;

    let walker =
        walkdir::WalkDir::new(&corpus_dir).into_iter().filter_map(|e| e.ok()).filter(|e| {
            e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "txt")
        });

    for entry in walker {
        if let Ok(content) = fs::read_to_string(entry.path())
            && let Some(ref re) = marker
        {
            total += content.lines().filter(|line| re.is_match(line)).count();
        }
    }
    total
}
