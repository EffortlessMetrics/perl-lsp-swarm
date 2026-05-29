use super::ParserSweepReceipt;

pub(super) fn format_clean_rate(clean_files: usize, total_files: usize) -> String {
    let clean_pct = 100.0 * clean_files as f64 / total_files.max(1) as f64;
    format!("{clean_pct:.1}% clean (`{clean_files}/{total_files}`)")
}

pub(super) fn format_salvage_rate(salvage_rate: Option<f64>) -> String {
    match salvage_rate {
        Some(rate) => format!("{:.1}% salvage", rate * 100.0),
        None => "insufficient_data salvage".to_string(),
    }
}

pub(super) fn format_recovery_shape_note(receipt: &ParserSweepReceipt) -> String {
    if receipt.has_recovery_shape {
        format!(
            "`{}` unreadable, `{}` recovery-only, `{}` ERROR-node files, `{}` catastrophic",
            receipt.files_unreadable,
            receipt.files_with_structured_recovery_only,
            receipt.files_with_error_nodes,
            receipt.files_with_catastrophic_parse_failure,
        )
    } else {
        format!(
            "`{}` unreadable, `insufficient_data` recovery-only, `insufficient_data` ERROR-node files, `insufficient_data` catastrophic",
            receipt.files_unreadable,
        )
    }
}

pub(super) fn short_day(timestamp: &str) -> &str {
    timestamp.get(..10).unwrap_or(timestamp)
}

pub(super) fn format_failure_receipt_note(receipt: &ParserSweepReceipt) -> String {
    format!(
        "Receipt snapshot: profile `{}`, commit `{}`, generated `{}`, Perl `{}`, `{}` resolved roots. Raw bucket counts are point-in-time compatibility data; before starting a parser-fix lane from a bucket, rerun `cargo xtask parser-corpus-sweep --baseline .ci/parser-corpus-baseline.json --enforce --receipt` on Linux or add a focused fixture when system roots are unavailable.",
        receipt.corpus_profile,
        receipt.commit,
        short_day(&receipt.timestamp),
        receipt.perl_version,
        receipt.resolved_roots_count,
    )
}

pub(super) fn format_nodekind_gap_note(
    summary: &super::super::super::corpus_audit::StatusSummary,
) -> String {
    match (
        summary.nodekind_actionable_never_seen,
        summary.nodekind_allowlisted_never_seen,
        summary.nodekind_never_seen,
    ) {
        (0, 0, 0) => "0 never-seen node kinds".to_string(),
        (0, allowlisted, _) => {
            format!("0 actionable never-seen; {allowlisted} recovery-only allowlisted")
        }
        (actionable, 0, total) => {
            format!("{actionable} actionable never-seen; {total} total never-seen")
        }
        (actionable, allowlisted, total) => {
            format!(
                "{actionable} actionable never-seen; {allowlisted} recovery-only allowlisted; {total} total never-seen"
            )
        }
    }
}
