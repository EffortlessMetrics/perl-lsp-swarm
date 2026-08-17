//! LSP subsystem status generator.
//!
//! Owns LSP coverage counts, protocol compliance table, lsp.md generation,
//! and ROADMAP.md compliance table sync.

use std::collections::BTreeMap;
use std::path::Path;

use color_eyre::eyre::{Context, Result};

use super::replace_block;

// ---------------------------------------------------------------------------
// LSP coverage struct
// ---------------------------------------------------------------------------

pub(super) struct LspCoverage {
    pub ux_percent: usize,
    pub ux_implemented: usize,
    pub ux_total: usize,
    pub protocol_percent: usize,
    pub protocol_implemented: usize,
    pub protocol_total: usize,
}

pub(super) fn count_lsp_coverage(root: &Path) -> Result<LspCoverage> {
    let features_path = root.join("features.toml");
    let catalog = perl_lsp_rs_core::feature_catalog::read_catalog(&features_path)
        .with_context(|| format!("loading {}", features_path.display()))?;

    // UX Coverage: advertised=true, counts_in_coverage!=false, maturity!=planned
    let ux_trackable: Vec<_> = catalog
        .feature
        .iter()
        .filter(|f| {
            f.maturity != perl_lsp_rs_core::feature_catalog::Maturity::Planned
                && f.counts_in_coverage
                && f.advertised
        })
        .collect();

    let ux_implemented: Vec<_> = ux_trackable
        .iter()
        .filter(|f| {
            matches!(
                f.maturity,
                perl_lsp_rs_core::feature_catalog::Maturity::Ga
                    | perl_lsp_rs_core::feature_catalog::Maturity::Production
            )
        })
        .collect();

    let ux_percent = if ux_trackable.is_empty() {
        0
    } else {
        ((ux_implemented.len() as f64 / ux_trackable.len() as f64) * 100.0).round() as usize
    };

    // Protocol Compliance: every catalog feature, regardless of
    // `counts_in_coverage` and regardless of maturity.
    //
    // `Planned` features stay in this denominator on purpose. They are protocol
    // surface the project has acknowledged and not yet implemented, so dropping
    // them would let the headline reach 100% by planning the gap away — and it
    // would disagree with `compute_compliance_table`, which counts every
    // feature. That disagreement is the #6909 defect: two denominators for one
    // "Protocol Compliance" claim rendered into the same document, where the
    // headline reported `123/123 — 100%` beside a table reporting
    // `123/125 — 98%`. One denominator, shared with the table, is the fix.
    let protocol_trackable: Vec<_> = catalog.feature.iter().collect();

    let protocol_implemented: Vec<_> = protocol_trackable
        .iter()
        .filter(|f| {
            matches!(
                f.maturity,
                perl_lsp_rs_core::feature_catalog::Maturity::Ga
                    | perl_lsp_rs_core::feature_catalog::Maturity::Production
                    | perl_lsp_rs_core::feature_catalog::Maturity::Preview
            )
        })
        .collect();

    let protocol_percent = if protocol_trackable.is_empty() {
        0
    } else {
        ((protocol_implemented.len() as f64 / protocol_trackable.len() as f64) * 100.0).round()
            as usize
    };

    Ok(LspCoverage {
        ux_percent,
        ux_implemented: ux_implemented.len(),
        ux_total: ux_trackable.len(),
        protocol_percent,
        protocol_implemented: protocol_implemented.len(),
        protocol_total: protocol_trackable.len(),
    })
}

// ---------------------------------------------------------------------------
// Compliance table for ROADMAP.md and lsp.md
// ---------------------------------------------------------------------------

pub(super) fn compute_compliance_table(root: &Path) -> Result<String> {
    let features_path = root.join("features.toml");
    let catalog = perl_lsp_rs_core::feature_catalog::read_catalog(&features_path)
        .with_context(|| format!("loading {}", features_path.display()))?;

    let mut by_area: BTreeMap<String, (usize, usize)> = BTreeMap::new(); // (implemented, total)

    for f in &catalog.feature {
        let entry = by_area.entry(f.area.clone()).or_insert((0, 0));
        entry.1 += 1;
        if matches!(
            f.maturity,
            perl_lsp_rs_core::feature_catalog::Maturity::Ga
                | perl_lsp_rs_core::feature_catalog::Maturity::Production
                | perl_lsp_rs_core::feature_catalog::Maturity::Preview
        ) {
            entry.0 += 1;
        }
    }

    let mut lines: Vec<String> = Vec::new();
    lines.push("| Area | Implemented | Total | Coverage |".to_string());
    lines.push("|------|-------------|-------|----------|".to_string());

    let mut total_impl: usize = 0;
    let mut total_all: usize = 0;

    for (area, (impl_count, total)) in &by_area {
        let pct = if *total == 0 {
            0
        } else {
            ((*impl_count as f64 / *total as f64) * 100.0).round() as usize
        };
        lines.push(format!("| {area} | {impl_count} | {total} | {pct}% |"));
        total_impl += impl_count;
        total_all += total;
    }

    let overall_pct = if total_all == 0 {
        0
    } else {
        ((total_impl as f64 / total_all as f64) * 100.0).round() as usize
    };
    lines
        .push(format!("| **Overall** | **{total_impl}** | **{total_all}** | **{overall_pct}%** |"));

    Ok(lines.join("\n"))
}

// ---------------------------------------------------------------------------
// Generator
// ---------------------------------------------------------------------------

pub(super) fn generate_lsp_status(
    cov: &LspCoverage,
    compliance_table: &str,
    original: &str,
) -> Result<String> {
    let lsp_target_pct: usize = 100;
    let lsp_status = if cov.ux_percent >= lsp_target_pct { "PASS" } else { "In progress" };
    let lsp_table_row = format!(
        "| **LSP Coverage** | {}% ({}/{} advertised features, `features.toml`) | {}% | {} |",
        cov.ux_percent, cov.ux_implemented, cov.ux_total, lsp_target_pct, lsp_status
    );

    let lsp_coverage_bullet = format!(
        "- **LSP Coverage**: {}% user-visible feature coverage ({}/{} advertised features from `features.toml`)",
        cov.ux_percent, cov.ux_implemented, cov.ux_total
    );
    let protocol_compliance_bullet = format!(
        "- **Protocol Compliance**: {}% overall LSP protocol support ({}/{} including plumbing)",
        cov.protocol_percent, cov.protocol_implemented, cov.protocol_total
    );

    let lsp_target = if cov.ux_percent >= lsp_target_pct {
        "**Target**: maintain 100% LSP coverage (no regressions)".to_string()
    } else {
        format!("**Target**: 100% LSP coverage (from current {}%)", cov.ux_percent)
    };

    let bullets_content = [
        lsp_coverage_bullet.as_str(),
        protocol_compliance_bullet.as_str(),
        "",
        lsp_target.as_str(),
    ]
    .join("\n");

    let mut text = original.to_string();
    text = replace_block(
        &text,
        "<!-- BEGIN: LSP_COVERAGE -->",
        "<!-- END: LSP_COVERAGE -->",
        &lsp_table_row,
    )?;
    text = replace_block(
        &text,
        "<!-- BEGIN: LSP_METRICS_BULLETS -->",
        "<!-- END: LSP_METRICS_BULLETS -->",
        &bullets_content,
    )?;
    text = replace_block(
        &text,
        "<!-- BEGIN: COMPLIANCE_TABLE -->",
        "<!-- END: COMPLIANCE_TABLE -->",
        compliance_table,
    )?;
    Ok(text)
}

// ---------------------------------------------------------------------------
// ROADMAP.md update (keeps compliance table in sync)
// ---------------------------------------------------------------------------

pub(super) fn update_roadmap(root: &Path, original: &str) -> Result<String> {
    let compliance_table = compute_compliance_table(root)?;
    replace_block(
        original,
        "<!-- BEGIN: COMPLIANCE_TABLE -->",
        "<!-- END: COMPLIANCE_TABLE -->",
        &compliance_table,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::project_root;
    use color_eyre::eyre::eyre;

    #[test]
    fn test_lsp_coverage_from_catalog() -> Result<()> {
        let root = project_root()?;
        let cov = count_lsp_coverage(&root)?;
        assert!(cov.ux_total > 0, "expected non-zero ux_total");
        assert!(cov.protocol_total > 0, "expected non-zero protocol_total");
        assert!(cov.ux_percent <= 100, "ux_percent should be <= 100, got {}", cov.ux_percent);
        Ok(())
    }

    /// The protocol-compliance headline and the compliance table are rendered
    /// into the same document and make the same claim, so they must be computed
    /// from the same numerator and denominator.
    ///
    /// Regression for #6909: the headline previously dropped `Planned` features
    /// from its denominator only, so it published `123/123 — 100%` directly
    /// above a table publishing `123/125 — 98%`.
    #[test]
    fn protocol_headline_and_compliance_table_share_one_denominator() -> Result<()> {
        let root = project_root()?;
        let cov = count_lsp_coverage(&root)?;
        let table = compute_compliance_table(&root)?;

        let overall = table
            .lines()
            .find(|line| line.contains("**Overall**"))
            .ok_or_else(|| eyre!("compliance table is missing its Overall row"))?;
        let cells: Vec<usize> = overall
            .split('|')
            .filter_map(|cell| cell.trim().trim_matches('*').trim_end_matches('%').parse().ok())
            .collect();
        let [table_implemented, table_total, table_percent] = cells[..] else {
            panic!("could not read numerator/denominator/percent from Overall row: {overall}");
        };

        assert_eq!(
            (cov.protocol_implemented, cov.protocol_total, cov.protocol_percent),
            (table_implemented, table_total, table_percent),
            "headline reports {}/{} = {}% but the table reports {}/{} = {}%; \
             one Protocol Compliance claim must not render two denominators (#6909)",
            cov.protocol_implemented,
            cov.protocol_total,
            cov.protocol_percent,
            table_implemented,
            table_total,
            table_percent,
        );
        Ok(())
    }

    /// A feature the project has acknowledged but not implemented must stay in
    /// the denominator, so the headline cannot reach 100% by planning a gap away.
    #[test]
    fn planned_features_remain_in_the_protocol_denominator() -> Result<()> {
        let root = project_root()?;
        let catalog = perl_lsp_rs_core::feature_catalog::read_catalog(&root.join("features.toml"))?;
        let cov = count_lsp_coverage(&root)?;

        assert_eq!(
            cov.protocol_total,
            catalog.feature.len(),
            "every catalog feature belongs in the protocol denominator",
        );

        let planned = catalog
            .feature
            .iter()
            .filter(|f| f.maturity == perl_lsp_rs_core::feature_catalog::Maturity::Planned)
            .count();
        if planned > 0 {
            assert!(
                cov.protocol_percent < 100,
                "{planned} planned feature(s) are unimplemented, so protocol compliance \
                 must not render as 100%",
            );
        }
        Ok(())
    }
}
