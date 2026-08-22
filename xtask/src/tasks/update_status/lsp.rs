//! LSP subsystem status generator.
//!
//! Owns the ROADMAP.md compliance-table sync. The generated claim-status
//! surface itself (`docs/project/status/lsp.md`) is owned by
//! `cargo xtask catalog-authority sync-status`, which renders from the same
//! evidence model in `perl-lsp-rs-core::feature_evidence`.
//!
//! Historical note (#6731): this module previously rendered two declaration-
//! derived denominators into `lsp.md` — a "UX coverage" percentage whose
//! denominator excluded `counts_in_coverage = false` rows (60/60 = 100%) and
//! a "protocol compliance" percentage over every row (123/125 = 98%). Both
//! turned declarations into proof. Percentages no longer appear on any LSP
//! status surface; documented claim counts replace them.

use std::path::Path;

use color_eyre::eyre::{Context, Result};

use super::replace_block;

// ---------------------------------------------------------------------------
// ROADMAP.md sync (single writer for the COMPLIANCE_TABLE fence)
// ---------------------------------------------------------------------------

/// Regenerate the ROADMAP.md `COMPLIANCE_TABLE` fence from the catalog
/// authority and the GA evidence policy.
///
/// The fence keeps its historical name for link stability; its content is now
/// the claim-status table (documented counts per claim status), not a coverage
/// percentage.
pub(super) fn update_roadmap(root: &Path, original: &str) -> Result<String> {
    let features_path = root.join("features.toml");
    let catalog = perl_lsp_rs_core::feature_catalog::read_catalog(&features_path)
        .with_context(|| format!("loading {}", features_path.display()))?;
    let policy = perl_lsp_rs_core::feature_evidence::GaEvidencePolicy::load(
        &root.join("policy/ga-evidence-policy.toml"),
    )
    .map_err(color_eyre::Report::msg)
    .with_context("loading GA evidence policy")?;
    let table =
        perl_lsp_rs_core::feature_evidence::render_claim_status_table(&catalog, &policy)
            .map_err(color_eyre::Report::msg)?;

    replace_block(
        original,
        "<!-- BEGIN: COMPLIANCE_TABLE -->",
        "<!-- END: COMPLIANCE_TABLE -->",
        &table,
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
    use perl_lsp_rs_core::feature_evidence::{
        GaEvidencePolicy, claim_counts_by_area, parse_claim_table_overall,
    };

    /// The ROADMAP fence must render the same claim-status table the
    /// standalone authority bin renders for `lsp.md`: one denominator, one
    /// renderer, two surfaces.
    #[test]
    fn roadmap_table_matches_the_authority_renderer() -> Result<()> {
        let root = project_root()?;
        let features_path = root.join("features.toml");
        let catalog = perl_lsp_rs_core::feature_catalog::read_catalog(&features_path)?;
        let policy = GaEvidencePolicy::load(&root.join("policy/ga-evidence-policy.toml"))?;
        let expected = perl_lsp_rs_core::feature_evidence::render_claim_status_table(
            &catalog, &policy,
        )
        .map_err(|e| eyre!("{e}"))?;

        let roadmap = std::fs::read_to_string(root.join("docs/project/ROADMAP.md"))?;
        let updated = update_roadmap(&root, &roadmap)?;
        let begin = "<!-- BEGIN: COMPLIANCE_TABLE -->";
        let end = "<!-- END: COMPLIANCE_TABLE -->";
        let start = updated
            .find(begin)
            .ok_or_else(|| eyre!("COMPLIANCE_TABLE begin marker missing after render"))?
            + begin.len();
        let stop = updated[start..]
            .find(end)
            .ok_or_else(|| eyre!("COMPLIANCE_TABLE end marker missing after render"))?;
        let rendered_block = updated[start..start + stop].trim().to_string();

        assert_eq!(rendered_block, expected);
        Ok(())
    }

    /// The rendered Overall row must partition the full catalog denominator:
    /// every catalog row appears in exactly one claim status.
    #[test]
    fn rendered_overall_row_partitions_the_catalog_denominator() -> Result<()> {
        let root = project_root()?;
        let catalog = perl_lsp_rs_core::feature_catalog::read_catalog(&root.join("features.toml"))?;
        let policy = GaEvidencePolicy::load(&root.join("policy/ga-evidence-policy.toml"))?;

        let areas = claim_counts_by_area(&catalog, &policy);
        let mut totals = [0usize; 6];
        for counts in areas.values() {
            totals[0] += counts.proven;
            totals[1] += counts.preview;
            totals[2] += counts.planned;
            totals[3] += counts.not_proven;
            totals[4] += counts.unsupported;
            totals[5] += counts.total;
        }
        assert_eq!(totals[5], catalog.feature.len(), "area totals cover the whole denominator");

        let table = perl_lsp_rs_core::feature_evidence::render_claim_status_table(&catalog, &policy)
            .map_err(|e| eyre!("{e}"))?;
        let overall = parse_claim_table_overall(&table)
            .ok_or_else(|| eyre!("rendered table lost its Overall row"))?;
        assert_eq!(
            overall[0] + overall[1] + overall[2] + overall[3] + overall[4],
            overall[5],
            "statuses partition the denominator"
        );
        assert_eq!(&overall, &totals, "Overall row aggregates the per-area counts");
        Ok(())
    }

    /// A planned feature must keep the headline honest: with any planned row
    /// present, `proven` cannot equal the denominator.
    #[test]
    fn planned_rows_prevent_a_fully_proven_render() -> Result<()> {
        let root = project_root()?;
        let catalog = perl_lsp_rs_core::feature_catalog::read_catalog(&root.join("features.toml"))?;
        let policy = GaEvidencePolicy::load(&root.join("policy/ga-evidence-policy.toml"))?;

        let planned = catalog
            .features()
            .iter()
            .filter(|f| f.maturity == perl_lsp_rs_core::feature_catalog::Maturity::Planned)
            .count();
        if planned > 0 {
            let table =
                perl_lsp_rs_core::feature_evidence::render_claim_status_table(&catalog, &policy)
                    .map_err(|e| eyre!("{e}"))?;
            let overall = parse_claim_table_overall(&table)
                .ok_or_else(|| eyre!("missing Overall row"))?;
            assert!(
                overall[0] < overall[5],
                "{planned} planned row(s) exist, so proven ({}) cannot equal the denominator ({})",
                overall[0],
                overall[5]
            );
        }
        Ok(())
    }

    /// No percentage may reappear on a claim-status surface (#6731).
    #[test]
    fn rendered_tables_carry_no_percentage_claims() -> Result<()> {
        let root = project_root()?;
        let catalog = perl_lsp_rs_core::feature_catalog::read_catalog(&root.join("features.toml"))?;
        let policy = GaEvidencePolicy::load(&root.join("policy/ga-evidence-policy.toml"))?;
        let table = perl_lsp_rs_core::feature_evidence::render_claim_status_table(&catalog, &policy)
            .map_err(|e| eyre!("{e}"))?;
        assert!(!table.contains('%'), "claim tables must not publish percentages");
        Ok(())
    }
}
