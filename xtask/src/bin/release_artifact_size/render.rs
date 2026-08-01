use color_eyre::eyre::{Context, Result};
use std::fs;
use std::path::Path;

use super::measure::{display_path, resolve_path};
use super::model::Receipt;

pub(crate) fn write_json(root: &Path, output: &Path, receipt: &Receipt) -> Result<()> {
    let output = resolve_path(root, output);
    ensure_parent(&output)?;
    let rendered = serde_json::to_string_pretty(receipt).context("serializing size receipt")?;
    fs::write(&output, format!("{rendered}\n"))
        .with_context(|| format!("writing {}", output.display()))
}

pub(crate) fn write_markdown(root: &Path, output: &Path, receipt: &Receipt) -> Result<()> {
    let output = resolve_path(root, output);
    ensure_parent(&output)?;

    let mut markdown = String::new();
    markdown.push_str("# Safe ICF artifact comparison\n\n");
    markdown.push_str(&format!("- **Target:** `{}`\n", receipt.subject.target));
    markdown.push_str(&format!("- **Git SHA:** `{}`\n", receipt.subject.git_sha));
    markdown.push_str(&format!("- **Recommendation:** `{}`\n", receipt.recommendation.as_str()));
    markdown.push_str(&format!("- **Status:** `{}`\n\n", receipt.status));
    markdown.push_str(
        "| Artifact | Baseline bytes | Candidate bytes | Reduction bytes | Reduction bp |\n",
    );
    markdown.push_str("|---|---:|---:|---:|---:|\n");
    for (name, delta) in &receipt.comparison.binaries {
        markdown.push_str(&format!(
            "| `{name}` | {} | {} | {} | {} |\n",
            delta.baseline_bytes,
            delta.candidate_bytes,
            delta.reduction_bytes,
            delta.reduction_basis_points
        ));
    }
    let combined = &receipt.comparison.combined;
    markdown.push_str(&format!(
        "| **combined** | **{}** | **{}** | **{}** | **{}** |\n",
        combined.baseline_bytes,
        combined.candidate_bytes,
        combined.reduction_bytes,
        combined.reduction_basis_points
    ));
    let archive = &receipt.comparison.archive;
    markdown.push_str(&format!(
        "| archive | {} | {} | {} | {} |\n\n",
        archive.baseline_bytes,
        archive.candidate_bytes,
        archive.reduction_bytes,
        archive.reduction_basis_points
    ));
    markdown.push_str(&format!(
        concat!(
            "- Structural parity: `{}`\n",
            "- Target architecture match: `{}`\n",
            "- Baseline archive identity: `{}`\n",
            "- Candidate archive identity: `{}`\n",
            "- Baseline smokes: `{}`\n",
            "- Candidate smokes: `{}`\n",
            "- Baseline smoke identity: `{}`\n",
            "- Candidate smoke identity: `{}`\n",
            "- Source identity bound: `{}`\n",
            "- Material reduction: `{}`\n",
            "- Component growth within policy: `{}`\n",
            "- Repeat confirmed: `{}`\n",
            "- Repeat requirement satisfied: `{}`\n"
        ),
        receipt.comparison.structural_parity,
        receipt.comparison.target_architecture_match,
        receipt.comparison.baseline_archive_identity,
        receipt.comparison.candidate_archive_identity,
        receipt.comparison.baseline_smokes_pass,
        receipt.comparison.candidate_smokes_pass,
        receipt.baseline.lsp_smoke.binary_matches && receipt.baseline.dap_smoke.binary_matches,
        receipt.candidate.lsp_smoke.binary_matches && receipt.candidate.dap_smoke.binary_matches,
        receipt.comparison.source_identity_bound,
        receipt.comparison.material_reduction,
        receipt.comparison.component_growth_within_policy,
        receipt.comparison.repeat_confirmed,
        receipt.comparison.repeat_requirement_satisfied
    ));
    if !receipt.limitations.is_empty() {
        markdown.push_str("\n## Limitations\n\n");
        for limitation in &receipt.limitations {
            markdown.push_str(&format!("- {limitation}\n"));
        }
    }
    markdown.push_str("\n## Claim boundary\n\n");
    markdown.push_str(receipt.claim_boundary);
    markdown.push('\n');

    fs::write(&output, markdown)
        .with_context(|| format!("writing Markdown receipt {}", display_path(root, &output)))
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    Ok(())
}
