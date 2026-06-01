use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use super::coverage::CoverageProofPackReceipt;
use super::model::{CiRouteReceipt, ProofPackReceipt};

pub(super) fn render_summary(
    receipt_path: &Path,
    summary_path: &Path,
    receipt: &CiRouteReceipt,
) -> String {
    let mut markdown = String::new();
    writeln!(markdown, "# CI Route Proof Packet").ok();
    writeln!(markdown).ok();
    writeln!(markdown, "- decision: `advisory`").ok();
    writeln!(markdown, "- provider_action: `{}`", receipt.provider_action).ok();
    writeln!(markdown, "- claim_boundary: {}", receipt.claim_boundary).ok();
    writeln!(markdown, "- base: `{}`", receipt.base).ok();
    writeln!(markdown, "- head: `{}`", receipt.head).ok();
    writeln!(markdown, "- receipt: `{}`", receipt_path.display()).ok();
    writeln!(markdown, "- summary: `{}`", summary_path.display()).ok();
    writeln!(markdown, "- estimated_lem: `{}`", receipt.estimated_lem).ok();
    writeln!(markdown).ok();

    markdown_list(&mut markdown, "Changed Files", &receipt.changed_files);
    markdown_list(&mut markdown, "Changed Surfaces", &receipt.changed_surfaces);
    markdown_skips(&mut markdown, &receipt.skipped_by_policy);
    markdown_proof_packs(&mut markdown, &receipt.required_proof_packs);
    markdown_coverage_packs(&mut markdown, &receipt.coverage_proof_packs);

    writeln!(markdown, "## Refresh Command").ok();
    writeln!(markdown).ok();
    writeln!(markdown, "```bash").ok();
    writeln!(markdown, "{}", refresh_command(receipt_path, summary_path, receipt)).ok();
    writeln!(markdown, "```").ok();
    markdown
}

fn refresh_command(receipt_path: &Path, summary_path: &Path, receipt: &CiRouteReceipt) -> String {
    let mut command = format!(
        "rtk cargo xtask ci route --base {} --head {} --receipt {} --summary {}",
        shell_quote(&receipt.base),
        shell_quote(&receipt.head),
        shell_quote(&receipt_path.display().to_string()),
        shell_quote(&summary_path.display().to_string())
    );
    for file in &receipt.changed_files {
        write!(command, " --changed-file {}", shell_quote(file)).ok();
    }
    command
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '\\' | '-' | '_' | '.' | ':'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn markdown_list(markdown: &mut String, heading: &str, values: &[String]) {
    writeln!(markdown, "## {heading}").ok();
    writeln!(markdown).ok();
    if values.is_empty() {
        writeln!(markdown, "- none").ok();
    } else {
        for value in values {
            writeln!(markdown, "- `{value}`").ok();
        }
    }
    writeln!(markdown).ok();
}

fn markdown_skips(markdown: &mut String, skipped_by_policy: &BTreeMap<String, String>) {
    writeln!(markdown, "## Skipped By Policy").ok();
    writeln!(markdown).ok();
    if skipped_by_policy.is_empty() {
        writeln!(markdown, "- none").ok();
    } else {
        for (pack, reason) in skipped_by_policy {
            writeln!(markdown, "- `{pack}`: {reason}").ok();
        }
    }
    writeln!(markdown).ok();
}

fn markdown_proof_packs(markdown: &mut String, proof_packs: &[ProofPackReceipt]) {
    writeln!(markdown, "## Required Proof Packs").ok();
    writeln!(markdown).ok();
    for pack in proof_packs {
        writeln!(markdown, "### `{}`", pack.id).ok();
        writeln!(markdown).ok();
        for command in &pack.commands {
            writeln!(markdown, "- `{command}`").ok();
        }
        writeln!(markdown).ok();
    }
}

fn markdown_coverage_packs(markdown: &mut String, coverage_packs: &[CoverageProofPackReceipt]) {
    writeln!(markdown, "## Coverage Proof Packs").ok();
    writeln!(markdown).ok();
    if coverage_packs.is_empty() {
        writeln!(markdown, "- none").ok();
        writeln!(markdown).ok();
        return;
    }

    for pack in coverage_packs {
        writeln!(markdown, "### `{}`", pack.id).ok();
        writeln!(markdown).ok();
        writeln!(markdown, "Files:").ok();
        for file in &pack.files {
            writeln!(markdown, "- `{file}`").ok();
        }
        writeln!(markdown).ok();
        writeln!(markdown, "Coverage filters:").ok();
        for filter in &pack.coverage_filters {
            writeln!(markdown, "- `{filter}`").ok();
        }
        writeln!(markdown).ok();
        writeln!(markdown, "Commands:").ok();
        for command in &pack.commands {
            writeln!(markdown, "- `{command}`").ok();
        }
        writeln!(markdown).ok();
    }
}
