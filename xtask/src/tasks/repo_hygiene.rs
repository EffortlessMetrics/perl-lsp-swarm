//! Exact-head, changed-file repository hygiene for issue #4149.
//!
//! This task owns only Taplo and typos admission. It composes the shared
//! change-set resolver so local and CI callers classify the same paths.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use color_eyre::eyre::{Context, Result, bail};
use serde::Serialize;

use crate::tasks::change_set::{self, ArtifactIdentity};
use crate::utils::project_root;

const SCHEMA_VERSION: &str = "repo-hygiene.v1";
const CLAIM_BOUNDARY: &str = "Changed-file Taplo formatting/syntax checks and typos checks for the exact resolved range; not whole-repository historical cleanliness, semantic policy validation, or release readiness";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResultClass {
    Pass,
    PolicyFinding,
    NotProven,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolResult {
    pub result: ResultClass,
    pub command: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct RepoHygieneReceipt {
    pub schema_version: &'static str,
    pub base_sha: String,
    pub head_sha: String,
    pub changed_files: Vec<String>,
    pub taplo_files: Vec<String>,
    pub typos_files: Vec<String>,
    pub taplo: Vec<ToolResult>,
    pub typos: Option<ToolResult>,
    pub status: ResultClass,
    pub claim_boundary: &'static str,
}

pub struct RepoHygieneConfig {
    pub base: String,
    pub head: String,
    pub receipt: PathBuf,
    pub summary: PathBuf,
}

pub fn run(config: RepoHygieneConfig) -> Result<()> {
    let root = project_root()?;
    let resolved = change_set::resolve_change_set(
        ArtifactIdentity::CommitRange { base: config.base, head: config.head },
        &root,
    )?;
    let (base_sha, head_sha) = match (resolved.base_sha, resolved.head_sha) {
        (Some(base), Some(head)) => (base, head),
        _ => bail!("repo-hygiene requires a resolved commit range"),
    };

    let taplo_files = resolved
        .changed_paths
        .iter()
        .filter(|path| is_toml_path(path))
        .cloned()
        .collect::<Vec<_>>();
    let typos_files = resolved
        .changed_paths
        .iter()
        .filter(|path| is_typos_path(path))
        .cloned()
        .collect::<Vec<_>>();

    let taplo = run_taplo(&root, &taplo_files);
    let typos = run_typos(&root, &typos_files);
    let status = overall_status(&taplo, typos.as_ref());
    let receipt = RepoHygieneReceipt {
        schema_version: SCHEMA_VERSION,
        base_sha: base_sha.clone(),
        head_sha: head_sha.clone(),
        changed_files: resolved.changed_paths,
        taplo_files,
        typos_files,
        taplo,
        typos,
        status,
        claim_boundary: CLAIM_BOUNDARY,
    };

    write_receipt(&config.receipt, &receipt)?;
    write_summary(&config.summary, &receipt)?;
    println!("repo hygiene: {:?} ({})", receipt.status, config.receipt.display());

    if matches!(receipt.status, ResultClass::Pass | ResultClass::NotApplicable) {
        Ok(())
    } else {
        bail!("repo-hygiene status is {:?}", receipt.status)
    }
}

fn run_taplo(root: &Path, files: &[String]) -> Vec<ToolResult> {
    if files.is_empty() {
        return vec![ToolResult {
            result: ResultClass::NotApplicable,
            command: "aqua exec -- taplo (no changed TOML files)".to_string(),
            detail: "no changed TOML files were resolved".to_string(),
        }];
    }

    let available = run_aqua(root, "taplo", &["--version"]);
    if available.result != ResultClass::Pass {
        return vec![available];
    }

    let mut format_args = vec!["fmt", "--check"];
    format_args.extend(files.iter().map(String::as_str));
    let format = run_aqua(root, "taplo", &format_args);

    let mut check_args = vec!["check"];
    check_args.extend(files.iter().map(String::as_str));
    let check = run_aqua(root, "taplo", &check_args);
    vec![format, check]
}

fn run_typos(root: &Path, files: &[String]) -> Option<ToolResult> {
    if files.is_empty() {
        return None;
    }

    let available = run_aqua(root, "typos", &["--version"]);
    if available.result != ResultClass::Pass {
        return Some(available);
    }

    let mut args = Vec::with_capacity(files.len());
    args.extend(files.iter().map(String::as_str));
    Some(run_aqua(root, "typos", &args))
}

fn run_aqua(root: &Path, tool: &str, args: &[&str]) -> ToolResult {
    let mut command = Command::new("aqua");
    command.current_dir(root).args(["exec", "--", tool]).args(args);
    let rendered =
        format_command("aqua", &["exec".to_string(), "--".to_string(), tool.to_string()]);
    let rendered =
        if args.is_empty() { rendered } else { format!("{rendered} {}", args.join(" ")) };

    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            return ToolResult {
                result: ResultClass::NotProven,
                command: rendered,
                detail: format!("could not start Aqua: {error}"),
            };
        }
    };
    let detail = command_detail(&output.stdout, &output.stderr);
    let result = if output.status.success() {
        ResultClass::Pass
    } else if args == ["--version"] {
        ResultClass::NotProven
    } else {
        ResultClass::PolicyFinding
    };
    ToolResult { result, command: rendered, detail }
}

fn overall_status(taplo: &[ToolResult], typos: Option<&ToolResult>) -> ResultClass {
    let results = taplo.iter().chain(typos);
    let mut applicable = false;
    let mut finding = false;
    for result in results {
        if result.result != ResultClass::NotApplicable {
            applicable = true;
        }
        match result.result {
            ResultClass::NotProven => return ResultClass::NotProven,
            ResultClass::PolicyFinding => finding = true,
            ResultClass::Pass | ResultClass::NotApplicable => {}
        }
    }
    if finding {
        ResultClass::PolicyFinding
    } else if applicable {
        ResultClass::Pass
    } else {
        ResultClass::NotApplicable
    }
}

pub fn is_toml_path(path: &str) -> bool {
    path.ends_with(".toml") && !path.starts_with("target/")
}

pub fn is_typos_path(path: &str) -> bool {
    if path.starts_with("target/") || path.ends_with("/Cargo.lock") || path == "Cargo.lock" {
        return false;
    }
    let lower = path.to_ascii_lowercase();
    if is_toml_path(path)
        || [
            "readme",
            "changelog",
            "license",
            "dockerfile",
            "makefile",
            ".gitignore",
            ".gitattributes",
            ".gitmodules",
            ".editorconfig",
        ]
        .iter()
        .any(|name| lower == *name || lower.starts_with(&format!("{name}.")))
    {
        return true;
    }
    Path::new(path).extension().and_then(|extension| extension.to_str()).is_some_and(|extension| {
        matches!(
            extension.to_ascii_lowercase().as_str(),
            "c" | "cc"
                | "cpp"
                | "h"
                | "hpp"
                | "html"
                | "js"
                | "json"
                | "jsx"
                | "lua"
                | "md"
                | "perl"
                | "pl"
                | "pm"
                | "ps1"
                | "py"
                | "rs"
                | "sass"
                | "scss"
                | "sh"
                | "sql"
                | "svg"
                | "ts"
                | "tsx"
                | "txt"
                | "xml"
                | "yaml"
                | "yml"
        )
    })
}

fn command_detail(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    let detail = format!("stdout: {stdout}\nstderr: {stderr}").trim().to_string();
    if detail.len() > 4000 {
        let bounded = detail.chars().take(4000).collect::<String>();
        format!("{bounded}...")
    } else {
        detail
    }
}

fn format_command(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ")
}

fn write_receipt(path: &Path, receipt: &RepoHygieneReceipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(receipt).context("serializing repo hygiene receipt")?;
    fs::write(path, format!("{json}\n")).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn write_summary(path: &Path, receipt: &RepoHygieneReceipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut summary = format!(
        "# Repository hygiene\n\n- Status: `{:?}`\n- Base: `{}`\n- Head: `{}`\n- Taplo files: {}\n- Typos files: {}\n\n",
        receipt.status,
        receipt.base_sha,
        receipt.head_sha,
        receipt.taplo_files.len(),
        receipt.typos_files.len(),
    );
    summary
        .push_str("Rerun the exact-head proof with:\n\n```text\ncargo xtask repo-hygiene --base ");
    summary.push_str(&receipt.base_sha);
    summary.push_str(" --head ");
    summary.push_str(&receipt.head_sha);
    summary.push_str("\n```\n\n");
    summary.push_str(receipt.claim_boundary);
    summary.push('\n');
    fs::write(path, summary).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::{Result, ensure};

    #[test]
    fn classifies_only_changed_toml_for_taplo() -> Result<()> {
        ensure!(is_toml_path("policy/ci-budget.toml"));
        ensure!(!is_toml_path("Cargo.lock"));
        ensure!(!is_toml_path("target/generated.toml"));
        ensure!(!is_toml_path("docs/README.md"));
        Ok(())
    }

    #[test]
    fn classifies_project_text_and_source_for_typos() -> Result<()> {
        for path in ["README.md", "crates/perl-parser/src/lib.rs", ".github/workflows/ci.yml"] {
            ensure!(is_typos_path(path), "expected {path} to be checked");
        }
        ensure!(!is_typos_path("Cargo.lock"));
        ensure!(!is_typos_path("target/generated.txt"));
        ensure!(!is_typos_path("assets/logo.png"));
        Ok(())
    }

    #[test]
    fn missing_tool_result_is_not_proven() -> Result<()> {
        let result = ToolResult {
            result: ResultClass::NotProven,
            command: "aqua exec -- taplo --version".to_string(),
            detail: "could not start Aqua".to_string(),
        };
        ensure!(result.result == ResultClass::NotProven);
        ensure!(overall_status(std::slice::from_ref(&result), None) == ResultClass::NotProven);
        Ok(())
    }
}
