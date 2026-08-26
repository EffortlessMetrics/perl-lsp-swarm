//! Deterministic human catalog reference.

use super::error::CatalogError;
use super::types::DistributionKwaliteeCatalog;

/// Render the checked-in human catalog reference.
pub fn render_distribution_kwalitee_catalog_markdown(
    catalog: &DistributionKwaliteeCatalog,
) -> Result<String, CatalogError> {
    let core = catalog.metric.iter().filter(|metric| metric.participates_in_core_score).count();
    let mut out = String::from(
        "# Native CPANTS-compatible catalog v1\n\n\
         > Generated from `crates/perl-kwalitee/distribution_kwalitee_catalog.v1.toml`.\n\
         > Do not edit this table independently.\n\n\
         ## Contract\n\n",
    );
    out.push_str(&format!("- kind: `{}`\n", catalog.kind));
    out.push_str(&format!("- catalog version: `{}`\n", catalog.catalog_version));
    out.push_str(&format!("- schema version: `{}`\n", catalog.schema_version));
    out.push_str(&format!("- status: `{}`\n", catalog.status));
    out.push_str("- scoring: `compatible_core_score = passed applicable cpants_offline_core / applicable cpants_offline_core`\n");
    out.push_str("- extra, experimental, site analogue, native extension, and deferred rows never enter the compatible core score\n");
    out.push_str("- invalid input has no ordinary score\n");
    out.push_str("- authoring trees are not staged input and have no ordinary score\n");
    out.push_str("- unverified required core evidence stays in the denominator; strict staged evaluation is incomplete\n");
    out.push_str(
        "- a NotApplicable observation cannot drop an applicable core row from the denominator\n",
    );
    out.push_str(&format!("- production runtime: `{}`\n", catalog.production_runtime));
    out.push_str(&format!("- oracle role: `{}`\n", catalog.oracle_role));
    out.push_str(&format!("- Module::CPANTS::Analyse: `{}`\n", catalog.cpants_analyse_version));
    out.push_str(&format!("- SiteKwalitee: `{}`\n", catalog.cpants_site_kwalitee_ref));
    out.push_str(&format!("- metrics: {} ({} compatible-core)\n\n", catalog.metric.len(), core));
    out.push_str(
        "## Metrics\n\n\
         | ID | Alias | Class | Score | Relationship | Source | Owner | Fixtures |\n\
         |---|---|---|---|---|---|---:|---|\n",
    );
    for metric in &catalog.metric {
        out.push_str(&format!(
            "| `{}` | `{}` | `{}` | {} | `{}` | `{}` | #{} | {} |\n",
            metric.id,
            metric.alias,
            metric.class.as_str(),
            if metric.participates_in_core_score { "core" } else { "no" },
            metric.relationship.as_str(),
            sanitize_cell(&metric.source_module),
            metric.implementation_owner,
            metric.fixture_ids.iter().map(|id| format!("`{id}`")).collect::<Vec<_>>().join(", "),
        ));
    }
    out.push_str(
        "\n## Interpretation\n\n\
         - `cpants_offline_core` is the only class that participates in `compatible_core_score`.\n\
         - Site analogues keep a narrower local claim and cannot masquerade as compatible core.\n\
         - Fixture identities are frozen here; reserved trees are filled by #8433/#9220.\n\
         - This catalog does not implement indicators, load archives, or invoke CPANTS.\n",
    );
    Ok(out)
}

fn sanitize_cell(value: &str) -> String {
    value.replace(['\r', '\n'], " ").replace('|', "\\|").replace('`', "'")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]
    use super::*;
    use crate::distribution_kwalitee::catalog::load_distribution_kwalitee_catalog;

    const GENERATED: &str =
        include_str!("../../../../docs/reference/DISTRIBUTION_KWALITEE_CATALOG.md");

    #[test]
    fn generated_catalog_reference_is_current() {
        let catalog = load_distribution_kwalitee_catalog().expect("catalog");
        let rendered = render_distribution_kwalitee_catalog_markdown(&catalog).expect("render");
        assert_eq!(rendered, GENERATED);
    }
}
