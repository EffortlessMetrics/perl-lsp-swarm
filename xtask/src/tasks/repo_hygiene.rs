//! Exact-head, changed-file repository hygiene for issue #4149.
//!
//! This task owns only Taplo and typos admission. It composes the shared
//! change-set resolver so local and CI callers classify the same paths.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use color_eyre::eyre::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::tasks::change_set::{self, ArtifactIdentity};
use crate::utils::project_root;

const SCHEMA_VERSION: &str = "repo-hygiene.v1";
const CLAIM_BOUNDARY: &str = "Changed-file Taplo formatting/syntax checks and typos checks for the exact resolved range; not whole-repository historical cleanliness, semantic policy validation, or release readiness";
const TOOL_CONFIG_FILES: &[&str] = &["aqua.yaml", "taplo.toml", ".typos.toml"];
const AQUA_VERIFICATION_ENV: &[&str] = &[
    "AQUA_GLOBAL_CONFIG",
    "AQUA_DISABLE_COSIGN",
    "AQUA_DISABLE_SLSA",
    "AQUA_DISABLE_GITHUB_ARTIFACT_ATTESTATION",
];
const TAPLO_CONFIG_ENV: &str = "TAPLO_CONFIG";

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
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
    ensure_exact_head(&root, &head_sha)?;

    let changed_files = resolved.changed_paths;
    let mut head_existing_files = Vec::with_capacity(changed_files.len());
    for path in &changed_files {
        if path_exists_at_head(&root, &head_sha, path)? {
            head_existing_files.push(path.clone());
        }
    }
    let mut proof_input_files = head_existing_files.clone();
    for path in TOOL_CONFIG_FILES {
        if path_exists_at_head(&root, &head_sha, path)?
            && !proof_input_files.iter().any(|file| file == path)
        {
            proof_input_files.push((*path).to_string());
        }
    }
    ensure_selected_paths_clean(&root, &proof_input_files)?;

    let taplo_files =
        head_existing_files.iter().filter(|path| is_toml_path(path)).cloned().collect::<Vec<_>>();
    let typos_files =
        head_existing_files.iter().filter(|path| is_typos_path(path)).cloned().collect::<Vec<_>>();

    let taplo = run_taplo(&root, &taplo_files);
    let typos = run_typos(&root, &typos_files);
    let status = overall_status(&taplo, typos.as_ref());
    let receipt = RepoHygieneReceipt {
        schema_version: SCHEMA_VERSION,
        base_sha: base_sha.clone(),
        head_sha: head_sha.clone(),
        changed_files,
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
    if let Some(policy) = taplo_schema_policy(root, files) {
        return vec![policy];
    }

    let available = run_aqua(root, "taplo", &["--version".to_string()]);
    if available.result != ResultClass::Pass {
        return vec![available];
    }

    let format_args = tool_file_args(&["fmt", "--check"], files);
    let format = run_aqua(root, "taplo", &format_args);

    let check_args = tool_file_args(&["lint"], files);
    let check = run_aqua(root, "taplo", &check_args);
    vec![format, check]
}

fn run_typos(root: &Path, files: &[String]) -> Option<ToolResult> {
    if files.is_empty() {
        return None;
    }

    let available = run_aqua(root, "typos", &["--version".to_string()]);
    if available.result != ResultClass::Pass {
        return Some(available);
    }

    let args = tool_file_args(&[], files);
    Some(run_aqua(root, "typos", &args))
}

fn tool_file_args(prefix: &[&str], files: &[String]) -> Vec<String> {
    let mut args = prefix.iter().map(|arg| (*arg).to_string()).collect::<Vec<_>>();
    args.push("--".to_string());
    args.extend(files.iter().cloned());
    args
}

fn taplo_schema_policy(root: &Path, files: &[String]) -> Option<ToolResult> {
    let mut inputs = files.to_vec();
    for config in ["taplo.toml", ".taplo.toml"] {
        if root.join(config).is_file() && !inputs.iter().any(|path| path == config) {
            inputs.push(config.to_string());
        }
    }

    for path in inputs {
        let content = match fs::read_to_string(root.join(&path)) {
            Ok(content) => content,
            Err(error) => {
                return Some(ToolResult {
                    result: ResultClass::NotProven,
                    command: "taplo lint --schema-policy".to_string(),
                    detail: format!("could not read Taplo input {path}: {error}"),
                });
            }
        };
        if is_remote_schema_reference(&content) {
            return Some(ToolResult {
                result: ResultClass::PolicyFinding,
                command: "taplo lint --schema-policy".to_string(),
                detail: format!(
                    "external schema sources are not allowed for changed-file hygiene: {path}"
                ),
            });
        }
        for (line_number, line) in content.lines().enumerate() {
            if is_remote_schema_reference(line) && line.contains("#:schema") {
                return Some(ToolResult {
                    result: ResultClass::PolicyFinding,
                    command: "taplo lint --schema-policy".to_string(),
                    detail: format!(
                        "external schema sources are not allowed for changed-file hygiene: {path}:{}",
                        line_number + 1
                    ),
                });
            }
        }
    }
    None
}

fn is_remote_schema_reference(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    if lower.contains("#:schema") && has_remote_schema_uri(&lower) {
        return true;
    }
    toml::from_str::<toml::Value>(line)
        .is_ok_and(|value| toml_value_has_remote_schema(&value, false))
}

fn has_remote_schema_uri(value: &str) -> bool {
    ["http://", "https://", "file://", "taplo://"].iter().any(|scheme| value.contains(scheme))
}

fn toml_value_has_remote_schema(value: &toml::Value, schema_context: bool) -> bool {
    match value {
        toml::Value::Table(table) => table.iter().any(|(key, value)| {
            let key_is_schema = key.to_ascii_lowercase().contains("schema");
            toml_value_has_remote_schema(value, schema_context || key_is_schema)
        }),
        toml::Value::Array(values) => {
            values.iter().any(|value| toml_value_has_remote_schema(value, schema_context))
        }
        toml::Value::String(value) => {
            schema_context && has_remote_schema_uri(&value.to_ascii_lowercase())
        }
        _ => false,
    }
}

fn run_aqua(root: &Path, tool: &str, args: &[String]) -> ToolResult {
    let mut command = Command::new("aqua");
    command
        .current_dir(root)
        .env("AQUA_CONFIG", root.join("aqua.yaml"))
        .args(["exec", "--", tool])
        .args(args);
    for name in AQUA_VERIFICATION_ENV {
        command.env_remove(name);
    }
    command.env_remove(TAPLO_CONFIG_ENV);
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
    } else if args.len() == 1 && args[0] == "--version" {
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
    let basename =
        Path::new(path).file_name().and_then(|name| name.to_str()).map(str::to_ascii_lowercase);
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
        .any(|name| {
            basename.as_deref().is_some_and(|basename| {
                basename == *name || basename.starts_with(&format!("{name}."))
            })
        })
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

fn ensure_exact_head(root: &Path, expected_head: &str) -> Result<()> {
    let actual_head = resolve_current_head(root)?;
    ensure!(
        actual_head == expected_head,
        "repo-hygiene requires a clean checkout at head {expected_head}; current HEAD is {actual_head}"
    );
    Ok(())
}

fn resolve_current_head(root: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "HEAD^{commit}"])
        .current_dir(root)
        .output()
        .context("resolving the checked-out HEAD")?;
    if !output.status.success() {
        bail!(
            "could not resolve the checked-out HEAD: {}",
            command_detail(&output.stdout, &output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn path_exists_at_head(root: &Path, head_sha: &str, path: &str) -> Result<bool> {
    let entry = Command::new("git")
        .args(["ls-tree", "-z", "--full-tree", head_sha, "--", path])
        .current_dir(root)
        .output()
        .with_context(|| format!("checking whether {path} exists at {head_sha}"))?;
    if !entry.status.success() {
        return Ok(false);
    }
    Ok(entry.stdout.split(|byte| *byte == 0).any(is_regular_tree_entry))
}

fn is_regular_tree_entry(entry: &[u8]) -> bool {
    let Some(tab) = entry.iter().position(|byte| *byte == b'\t') else { return false };
    let header = &entry[..tab];
    let mut fields = header.split(|byte| *byte == b' ');
    let Some(mode) = fields.next() else { return false };
    let Some(object_type) = fields.next() else { return false };
    matches!(mode, b"100644" | b"100755") && object_type == b"blob"
}

fn ensure_selected_paths_clean(root: &Path, paths: &[String]) -> Result<()> {
    for path in paths {
        for args in [["diff", "--quiet", "--"], ["diff", "--cached", "--"]] {
            let output = Command::new("git")
                .args(args)
                .arg(path)
                .current_dir(root)
                .output()
                .with_context(|| format!("checking whether {path} is clean"))?;
            if output.status.code() == Some(1) {
                bail!("repo-hygiene cannot prove {path}: the checked-out file is dirty");
            }
            if !output.status.success() {
                bail!(
                    "could not check whether {path} is clean: {}",
                    command_detail(&output.stdout, &output.stderr)
                );
            }
        }
    }
    Ok(())
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
        for path in [
            "README.md",
            "crates/perl-parser/src/lib.rs",
            ".github/workflows/ci.yml",
            ".docker/perl-lsp/Dockerfile",
            "tree-sitter-perl/Makefile",
            "tree-sitter-perl/LICENSE",
        ] {
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

    #[test]
    fn tool_file_args_terminate_options_for_literal_paths() -> Result<()> {
        let taplo = tool_file_args(
            &["lint"],
            &["--config=outside.toml".to_string(), "--file-list=paths.txt".to_string()],
        );
        ensure!(
            taplo == vec!["lint", "--", "--config=outside.toml", "--file-list=paths.txt"],
            "Taplo paths must be literal arguments: {taplo:?}"
        );

        let typos = tool_file_args(&[], &["--files".to_string()]);
        ensure!(typos == vec!["--", "--files"], "typos paths must be literal arguments: {typos:?}");
        Ok(())
    }

    #[test]
    fn remote_schema_references_are_rejected_before_taplo_runs() -> Result<()> {
        ensure!(is_remote_schema_reference("#:schema https://example.test/schema.json"));
        ensure!(is_remote_schema_reference("[schema]\npath = \"file:///tmp/schema.json\""));
        ensure!(is_remote_schema_reference(
            "[rule.schema]\npath = \"\"\"\nhttps://example.test/schema.json\n\"\"\""
        ));
        ensure!(!is_remote_schema_reference("[schema]\npath = \"schemas/local.json\""));
        ensure!(!is_remote_schema_reference("[tool]\npath = \"https://example.test/tool\""));
        Ok(())
    }

    #[test]
    fn taplo_config_environment_override_is_cleared() -> Result<()> {
        ensure!(TAPLO_CONFIG_ENV == "TAPLO_CONFIG");
        Ok(())
    }

    #[test]
    fn only_regular_blob_tree_entries_are_proof_inputs() -> Result<()> {
        ensure!(is_regular_tree_entry(b"100644 blob abc\tfile.toml\0"));
        ensure!(is_regular_tree_entry(b"100755 blob abc\tscript.sh\0"));
        ensure!(!is_regular_tree_entry(b"120000 blob abc\tlink.toml\0"));
        ensure!(!is_regular_tree_entry(b"160000 commit abc\tsubmodule\0"));
        Ok(())
    }
}
