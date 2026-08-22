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
    pub ux_implemented: usize,
    pub ux_total: usize,
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

    Ok(LspCoverage {
        ux_implemented: ux_implemented.len(),
        ux_total: ux_trackable.len(),
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

    let mut by_area: BTreeMap<String, (usize, usize)> = BTreeMap::new(); // (declared ga/production/preview, total)

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

    // #6731 containment: these are declaration counts from catalog maturity
    // labels, not behavior evidence. The table publishes raw counts as
    // navigation data only — no percentage column and no coverage claim.
    let mut lines: Vec<String> = Vec::new();
    lines.push("| Area | Declared ga/preview rows | Total rows |".to_string());
    lines.push("|------|---------------------------|------------|".to_string());

    let mut total_impl: usize = 0;
    let mut total_all: usize = 0;

    for (area, (impl_count, total)) in &by_area {
        lines.push(format!("| {area} | {impl_count} | {total} |"));
        total_impl += impl_count;
        total_all += total;
    }

    lines.push(format!("| **Overall** | **{total_impl}** | **{total_all}** |"));
    lines.push(String::new());
    lines.push(
        "Counts are navigation only (#6731): maturity labels are declarations without per-row \
         behavior-evidence ownership."
            .to_string(),
    );

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
    // #6731 containment leaf: catalog maturity labels are declarations, not
    // behavior evidence. Until every promoted row names an exact current
    // behavior-evidence owner, this projection must refuse to publish an
    // aggregate percentage or a passing verdict. Derived counts remain as
    // navigation data only; the evidence state renders `not_proven`.
    let coverage_row = "| **LSP Coverage** | not_proven — no exact current behavior-evidence \
                        owner (#6731); catalog counts below are navigation only |"
        .to_string();

    let lsp_coverage_bullet = format!(
        "- **Advertised ga/production rows**: {} of {} coverage-tracked advertised rows declare \
         ga/production (navigation count from `features.toml`)",
        cov.ux_implemented, cov.ux_total
    );
    let protocol_compliance_bullet = format!(
        "- **Protocol surface labels**: {} of {} declared rows carry ga/production/preview labels \
         (navigation only)",
        cov.protocol_implemented, cov.protocol_total
    );
    let evidence_bullet =
        "- **Evidence state**: not_proven — cells without an exact current behavior-evidence owner \
         render `not_proven`, never inherited green"
            .to_string();
    let lsp_target =
        "**Target**: every promoted cell names exact current behavior and subject evidence (#6731)"
            .to_string();

    let bullets_content = [
        lsp_coverage_bullet.as_str(),
        protocol_compliance_bullet.as_str(),
        evidence_bullet.as_str(),
        "",
        lsp_target.as_str(),
    ]
    .join("\n");

    let mut text = original.to_string();
    text = replace_block(
        &text,
        "<!-- BEGIN: LSP_COVERAGE -->",
        "<!-- END: LSP_COVERAGE -->",
        &coverage_row,
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
        Ok(())
    }

    /// The advertised-rows bullet and the compliance-table Overall row make the
    /// same declaration-count claim, so they must be computed from the same
    /// numerator and denominator.
    ///
    /// Regression for #6909, adapted by #6731: the claim is now navigation-only
    /// counts, but one denominator per claim still holds.
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
            .filter_map(|cell| cell.trim().trim_matches('*').parse().ok())
            .collect();
        let [table_implemented, table_total] = cells[..] else {
            panic!("could not read declared/total counts from Overall row: {overall}");
        };

        assert_eq!(
            (cov.protocol_implemented, cov.protocol_total),
            (table_implemented, table_total),
            "headline reports {}/{} but the table reports {}/{}; \
             one protocol-surface claim must not render two denominators (#6909)",
            cov.protocol_implemented,
            cov.protocol_total,
            table_implemented,
            table_total,
        );
        Ok(())
    }

    /// A feature the project has acknowledged but not implemented must stay in
    /// the denominator, so a planned gap cannot be counted away.
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
        Ok(())
    }

    /// #6731 containment recurrence control: the generated status must render
    /// `not_proven` evidence state and navigation-labeled counts, and must never
    /// reintroduce an aggregate percentage or a passing verdict derived from
    /// maturity declarations.
    #[test]
    fn generated_lsp_status_renders_not_proven_and_never_a_passing_verdict() -> Result<()> {
        let root = project_root()?;
        let cov = count_lsp_coverage(&root)?;
        let table = compute_compliance_table(&root)?;
        let original = std::fs::read_to_string(root.join("docs/project/status/lsp.md"))?;

        let generated = generate_lsp_status(&cov, &table, &original)?;

        let coverage_block = block(&generated, "LSP_COVERAGE")?;
        assert!(
            coverage_block.contains("not_proven"),
            "LSP Coverage row must render not_proven without an evidence owner: {coverage_block}"
        );

        let bullets_block = block(&generated, "LSP_METRICS_BULLETS")?;
        assert!(
            bullets_block.contains("not_proven"),
            "metrics bullets must state the not_proven evidence state: {bullets_block}"
        );
        assert!(
            bullets_block.contains("navigation"),
            "derived counts must be labeled navigation only: {bullets_block}"
        );

        // Scan every generated region: aggregate percentages and passing
        // verdicts are declaration-count claims, not behavior proof (#6731).
        let generated_region = [
            block(&generated, "LSP_COVERAGE")?,
            block(&generated, "LSP_METRICS_BULLETS")?,
            block(&generated, "COMPLIANCE_TABLE")?,
        ]
        .join("\n");
        for forbidden in ["PASS", "%"] {
            assert!(
                !generated_region.contains(forbidden),
                "generated LSP status must not contain {forbidden:?} (#6731)"
            );
        }
        Ok(())
    }

    /// Mutation control: even with a synthetic catalog that reports full
    /// coverage, the projection must keep refusing a percentage or PASS. This
    /// fails if anyone reattaches verdict logic to the coverage counters.
    #[test]
    fn full_declaration_coverage_still_renders_not_proven() -> Result<()> {
        let cov = LspCoverage {
            ux_implemented: 60,
            ux_total: 60,
            protocol_implemented: 125,
            protocol_total: 125,
        };
        let table = "| Area | Declared ga/preview rows | Total rows |\n\
                     |------|---------------------------|------------|\n\
                     | **Overall** | **125** | **125** |";
        let original = "<!-- BEGIN: LSP_COVERAGE -->\nold\n<!-- END: LSP_COVERAGE -->\n\
                        <!-- BEGIN: LSP_METRICS_BULLETS -->\nold\n<!-- END: LSP_METRICS_BULLETS -->\n\
                        <!-- BEGIN: COMPLIANCE_TABLE -->\nold\n<!-- END: COMPLIANCE_TABLE -->\n";

        let generated = generate_lsp_status(&cov, table, original)?;

        assert!(block(&generated, "LSP_COVERAGE")?.contains("not_proven"));
        assert!(!generated.contains("PASS"), "PASS must never derive from declarations");
        assert!(
            !generated.contains('%'),
            "no percentage may be derived from declarations, even at full coverage"
        );
        Ok(())
    }

    fn block(text: &str, tag: &str) -> Result<String> {
        let begin = format!("<!-- BEGIN: {tag} -->");
        let end = format!("<!-- END: {tag} -->");
        let start = text
            .find(&begin)
            .ok_or_else(|| eyre!("generated LSP status is missing begin marker for {tag}"))?;
        let region = &text[start..];
        let stop = region
            .find(&end)
            .ok_or_else(|| eyre!("generated LSP status is missing end marker for {tag}"))?;
        Ok(region[..stop].to_string())
    }
}
