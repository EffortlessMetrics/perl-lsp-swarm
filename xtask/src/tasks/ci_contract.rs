//! Thin current-head repository contract receipt for issue #3987.
//!
//! This module owns only the C1 remote contract boundary. It composes the
//! shared change-set resolver and `ci_scope` classifier, then runs a small set
//! of deterministic repository checks selected from the changed surface. It
//! deliberately does not run behavioral proof, RIPR, review, or merge logic.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::{Deserialize, Serialize};

use crate::tasks::change_set::{self, ArtifactIdentity};
use crate::tasks::ci_scope::{self, ScopeOutput};
use crate::tasks::repo_hygiene;
use crate::utils::project_root;

const SCHEMA_VERSION: &str = "ci-contract.v1";
const CHECK_TIMEOUT: Duration = Duration::from_mins(2);
const CLAIM_BOUNDARY: &str =
    "Advisory exact-head repository contracts; no behavioral proof, review, or merge authorization";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContractResultClass {
    Success,
    PolicyFinding,
    NotProven,
    NotApplicable,
    Stale,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContractStatus {
    Success,
    PolicyFinding,
    NotProven,
    NotApplicable,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContractCheck {
    pub id: String,
    pub reason: String,
    pub command: String,
    pub result: ContractResultClass,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractReceipt {
    pub schema_version: &'static str,
    pub provider_action: &'static str,
    pub repository: String,
    pub base_sha: String,
    pub head_sha: String,
    pub changed_files: Vec<String>,
    pub changed_surfaces: Vec<String>,
    pub scope: ScopeOutput,
    pub checks: Vec<ContractCheck>,
    pub status: ContractStatus,
    pub claim_boundary: &'static str,
}

pub struct CiContractConfig {
    pub base: String,
    pub head: String,
    pub receipt: PathBuf,
    pub summary: PathBuf,
}

#[derive(Debug, Clone)]
struct CheckSpec {
    id: &'static str,
    reason: String,
    program: &'static str,
    args: Vec<String>,
}

pub fn run(config: CiContractConfig) -> Result<()> {
    let root = project_root()?;
    let resolved = change_set::resolve_change_set(
        ArtifactIdentity::CommitRange { base: config.base, head: config.head },
        &root,
    )?;
    if !matches!(resolved.identity, ArtifactIdentity::CommitRange { .. }) {
        bail!("ci-contract requires a commit range");
    }
    let (base_sha, head_sha) = validate_resolved_identity(resolved.base_sha, resolved.head_sha)?;

    let metadata = ci_scope::load_metadata(&root)?;
    let workspace_root = root.to_string_lossy().replace('\\', "/");
    let mut scope = ci_scope::classify_files(&resolved.changed_paths, &metadata, &workspace_root)?;
    scope.base = base_sha.clone();
    scope.head_sha = head_sha.clone();
    let repository = repository_identity(&root)?;
    let changed_files_path = root.join("target/ci-contract/changed-files.txt");
    write_changed_files(&changed_files_path, &resolved.changed_paths)?;

    let checkout_head = resolve_sha(&root, "HEAD")?;
    let mut checks = Vec::new();
    if checkout_head != head_sha {
        checks.push(head_identity_check(&head_sha, &checkout_head));
    } else {
        let specs =
            select_checks(&resolved.changed_paths, &base_sha, &head_sha, &changed_files_path);
        checks.reserve(specs.len() + 1);
        for spec in specs {
            checks.push(run_check(&root, &spec));
        }
        let current_head = resolve_sha(&root, "HEAD")?;
        if current_head != head_sha {
            checks.push(head_identity_check(&head_sha, &current_head));
        }
    }

    let receipt = ContractReceipt {
        schema_version: SCHEMA_VERSION,
        provider_action: "repository_contract",
        repository,
        base_sha,
        head_sha,
        changed_surfaces: changed_surfaces(&resolved.changed_paths),
        changed_files: resolved.changed_paths,
        scope,
        status: status_from_checks(&checks),
        checks,
        claim_boundary: CLAIM_BOUNDARY,
    };
    write_receipt(&config.receipt, &receipt)?;
    write_summary(&config.summary, &receipt)?;
    println!("repository contract: {:?} ({})", receipt.status, config.receipt.display());

    if receipt.status != ContractStatus::Success && receipt.status != ContractStatus::NotApplicable
    {
        bail!("repository contract status is {:?}", receipt.status);
    }
    Ok(())
}

fn select_checks(
    changed_files: &[String],
    base: &str,
    head: &str,
    changed_files_path: &Path,
) -> Vec<CheckSpec> {
    if changed_files.is_empty() {
        return Vec::new();
    }

    let mut checks = vec![CheckSpec {
        id: "diff_check",
        reason: "every exact-head candidate must have a clean binary diff".to_string(),
        program: "git",
        args: vec!["diff".to_string(), "--check".to_string(), format!("{base}..{head}")],
    }];

    if changed_files.iter().any(|file| file.ends_with(".rs") || file.ends_with("Cargo.toml")) {
        checks.push(CheckSpec {
            id: "rust_format",
            reason: "Rust or Cargo source changed".to_string(),
            program: "cargo",
            args: vec!["xtask".to_string(), "fmt".to_string(), "--check".to_string()],
        });
    }

    if changed_files.iter().any(|file| is_workflow_or_shell(file)) {
        checks.push(CheckSpec {
            id: "workflow_contract",
            reason: "workflow or shell surface changed".to_string(),
            program: "cargo",
            args: vec![
                "xtask".to_string(),
                "workflows".to_string(),
                "check".to_string(),
                "--self-test".to_string(),
                "--base".to_string(),
                base.to_string(),
            ],
        });
    }

    if changed_files.iter().any(|file| is_workflow(file)) {
        checks.push(CheckSpec {
            id: "workflow_trigger_policy",
            reason: "GitHub workflow trigger surface changed".to_string(),
            program: "cargo",
            args: vec![
                "xtask".to_string(),
                "workflow-trigger-lint".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ],
        });
    }

    if changed_files.iter().any(|file| is_policy(file)) {
        checks.push(CheckSpec {
            id: "gate_policy",
            reason: "repository policy surface changed".to_string(),
            program: "cargo",
            args: vec!["xtask".to_string(), "gate-policy".to_string(), "check".to_string()],
        });
    }

    if changed_files.iter().any(|file| is_changelog(file)) {
        checks.push(CheckSpec {
            id: "changelog_disposition",
            reason: "changelog or release-note surface changed".to_string(),
            program: "cargo",
            args: vec![
                "xtask".to_string(),
                "changelog".to_string(),
                "check".to_string(),
                "--base".to_string(),
                base.to_string(),
                "--changed-files".to_string(),
                changed_files_path.display().to_string(),
            ],
        });
    }

    if changed_files
        .iter()
        .any(|file| repo_hygiene::is_toml_path(file) || repo_hygiene::is_typos_path(file))
    {
        checks.push(CheckSpec {
            id: "repo_hygiene",
            reason: "changed TOML or text/config/source surface changed".to_string(),
            program: "cargo",
            args: vec![
                "xtask".to_string(),
                "repo-hygiene".to_string(),
                "--base".to_string(),
                base.to_string(),
                "--head".to_string(),
                head.to_string(),
                "--receipt".to_string(),
                "target/receipts/repo-hygiene.json".to_string(),
                "--summary".to_string(),
                "target/receipts/repo-hygiene.md".to_string(),
            ],
        });
    }

    checks
}

fn run_check(root: &Path, spec: &CheckSpec) -> ContractCheck {
    let command = format_command(spec.program, &spec.args);
    if spec.id == "repo_hygiene"
        && let Err(error) = clear_repo_hygiene_receipt(root, &spec.args) {
            return ContractCheck {
                id: spec.id.to_string(),
                reason: spec.reason.clone(),
                command,
                result: ContractResultClass::NotProven,
                detail: format!("could not prepare repo-hygiene receipt: {error}"),
            };
        }
    match execute_check(root, spec) {
        Ok(output) => {
            let (result, detail) = if spec.id == "repo_hygiene" {
                classify_repo_hygiene_output(root, &spec.args, &output.stdout, &output.stderr)
            } else {
                classify_check_output(spec.id, output.status.code(), &output.stdout, &output.stderr)
            };
            ContractCheck {
                id: spec.id.to_string(),
                reason: spec.reason.clone(),
                command,
                result,
                detail,
            }
        }
        Err(error) => ContractCheck {
            id: spec.id.to_string(),
            reason: spec.reason.clone(),
            command,
            result: ContractResultClass::NotProven,
            detail: format!("failed to start check: {error}"),
        },
    }
}

#[derive(Debug, Deserialize)]
struct RepoHygieneStatusReceipt {
    status: repo_hygiene::ResultClass,
}

fn clear_repo_hygiene_receipt(root: &Path, args: &[String]) -> Result<()> {
    for flag in ["--receipt", "--summary"] {
        let path = repo_hygiene_output_path(root, args, flag)?;
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("removing {}", path.display()));
            }
        }
    }
    Ok(())
}

fn repo_hygiene_receipt_path(root: &Path, args: &[String]) -> Result<PathBuf> {
    repo_hygiene_output_path(root, args, "--receipt")
}

fn repo_hygiene_output_path(root: &Path, args: &[String], flag: &str) -> Result<PathBuf> {
    let path = args
        .windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| root.join(&pair[1]))
        .ok_or_else(|| eyre!("repo-hygiene check did not declare a {flag} path"))?;
    Ok(path)
}

fn classify_repo_hygiene_output(
    root: &Path,
    args: &[String],
    stdout: &[u8],
    stderr: &[u8],
) -> (ContractResultClass, String) {
    let detail = bounded_output(&command_output(stdout, stderr));
    let path = match repo_hygiene_receipt_path(root, args) {
        Ok(path) => path,
        Err(error) => return (ContractResultClass::NotProven, format!("{detail}; {error}")),
    };
    let status = match fs::read(&path)
        .with_context(|| format!("reading {}", path.display()))
        .and_then(|bytes| {
            serde_json::from_slice::<RepoHygieneStatusReceipt>(&bytes)
                .with_context(|| format!("parsing {}", path.display()))
        }) {
        Ok(receipt) => receipt.status,
        Err(error) => {
            return (
                ContractResultClass::NotProven,
                format!("{detail}; repo-hygiene receipt is invalid: {error}"),
            );
        }
    };
    let result = match status {
        repo_hygiene::ResultClass::Pass => ContractResultClass::Success,
        repo_hygiene::ResultClass::PolicyFinding => ContractResultClass::PolicyFinding,
        repo_hygiene::ResultClass::NotProven => ContractResultClass::NotProven,
        repo_hygiene::ResultClass::NotApplicable => ContractResultClass::NotApplicable,
    };
    (result, detail)
}

fn execute_check(root: &Path, spec: &CheckSpec) -> std::io::Result<Output> {
    let mut command =
        if spec.program == "cargo" && spec.args.first().is_some_and(|arg| arg == "xtask") {
            // Reuse the current executable so inherited CARGO_*/RUSTFLAGS settings remain
            // intact and Cargo does not try to replace the running xtask binary on Windows.
            let executable = std::env::current_exe()?;
            let mut command = Command::new(executable);
            command.args(spec.args.iter().skip(1));
            command
        } else {
            let mut command = Command::new(spec.program);
            command.args(&spec.args);
            command
        };
    command.current_dir(root).stdout(Stdio::piped()).stderr(Stdio::piped());
    run_with_timeout(command)
}

fn run_with_timeout(mut command: Command) -> io::Result<Output> {
    let mut child = command.spawn()?;
    let mut stdout_handle = child.stdout.take().map(|mut stream| {
        thread::spawn(move || {
            let mut buffer = Vec::new();
            stream.read_to_end(&mut buffer).map(|_| buffer)
        })
    });
    let mut stderr_handle = child.stderr.take().map(|mut stream| {
        thread::spawn(move || {
            let mut buffer = Vec::new();
            stream.read_to_end(&mut buffer).map(|_| buffer)
        })
    });
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let stdout = join_output(stdout_handle.take(), "stdout")?;
            let stderr = join_output(stderr_handle.take(), "stderr")?;
            return Ok(Output { status, stdout, stderr });
        }
        if started.elapsed() >= CHECK_TIMEOUT {
            terminate_process_tree(&mut child);
            let _ = child.wait();
            let _ = join_output(stdout_handle.take(), "stdout");
            let _ = join_output(stderr_handle.take(), "stderr");
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("check exceeded {} seconds", CHECK_TIMEOUT.as_secs()),
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn terminate_process_tree(child: &mut std::process::Child) {
    let pid = child.id().to_string();
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill").args(["/PID", &pid, "/T", "/F"]).status();
    }
    #[cfg(unix)]
    {
        let descendants = unix_descendants(&pid);
        for descendant in descendants.iter().rev() {
            let _ = Command::new("kill").args(["-TERM", descendant]).status();
        }
        for descendant in descendants.iter().rev() {
            let _ = Command::new("kill").args(["-KILL", descendant]).status();
        }
    }
    let _ = child.kill();
}

#[cfg(unix)]
fn unix_descendants(root: &str) -> Vec<String> {
    let mut descendants = Vec::new();
    let mut pending = vec![root.to_string()];
    while let Some(parent) = pending.pop() {
        let Ok(output) = Command::new("pgrep").args(["-P", &parent]).output() else { continue };
        for child in String::from_utf8_lossy(&output.stdout).lines().map(str::trim) {
            if child.is_empty() || descendants.iter().any(|pid| pid == child) {
                continue;
            }
            let child = child.to_string();
            pending.push(child.clone());
            descendants.push(child);
        }
    }
    descendants
}

fn join_output(
    handle: Option<thread::JoinHandle<io::Result<Vec<u8>>>>,
    stream: &str,
) -> io::Result<Vec<u8>> {
    let Some(handle) = handle else { return Ok(Vec::new()) };
    handle.join().map_err(|_| io::Error::other(format!("{stream} reader panicked")))?
}

fn result_for_exit(_check_id: &str, code: Option<i32>, detail: &str) -> ContractResultClass {
    match code {
        Some(0) if has_policy_finding(detail) => ContractResultClass::PolicyFinding,
        Some(0) => ContractResultClass::Success,
        Some(1) => ContractResultClass::PolicyFinding,
        _ => ContractResultClass::NotProven,
    }
}

fn has_policy_finding(detail: &str) -> bool {
    detail.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("WARN ") || line.starts_with("[WARN]")
    })
}

fn classify_check_output(
    check_id: &str,
    code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> (ContractResultClass, String) {
    let raw_detail = command_output(stdout, stderr);
    let detail = if code.is_none() {
        bounded_output_with_prefix("process terminated without an exit code; ", &raw_detail)
    } else {
        bounded_output(&raw_detail)
    };
    (result_for_exit(check_id, code, &raw_detail), detail)
}

fn head_identity_check(expected: &str, current: &str) -> ContractCheck {
    ContractCheck {
        id: "head_identity".to_string(),
        reason: "the checkout does not match the evaluated head".to_string(),
        command: "git rev-parse --verify HEAD^{commit}".to_string(),
        result: ContractResultClass::Stale,
        detail: format!("evaluated={expected}, current={current}"),
    }
}

fn status_from_checks(checks: &[ContractCheck]) -> ContractStatus {
    for (class, status) in [
        (ContractResultClass::Stale, ContractStatus::Stale),
        (ContractResultClass::NotProven, ContractStatus::NotProven),
        (ContractResultClass::PolicyFinding, ContractStatus::PolicyFinding),
        (ContractResultClass::Success, ContractStatus::Success),
        (ContractResultClass::NotApplicable, ContractStatus::NotApplicable),
    ] {
        if checks.iter().any(|check| check.result == class) {
            return status;
        }
    }
    ContractStatus::NotApplicable
}

fn changed_surfaces(files: &[String]) -> Vec<String> {
    let mut surfaces = BTreeSet::new();
    for file in files {
        if is_workflow_or_shell(file) {
            surfaces.insert("workflow_or_shell".to_string());
        }
        if file.ends_with(".rs") || file.ends_with("Cargo.toml") {
            surfaces.insert("rust".to_string());
        }
        if is_policy(file) {
            surfaces.insert("policy".to_string());
        }
        if is_changelog(file) {
            surfaces.insert("changelog".to_string());
        }
    }
    if surfaces.is_empty() {
        surfaces.insert("docs_or_other".to_string());
    }
    surfaces.into_iter().collect()
}

fn is_workflow_or_shell(file: &str) -> bool {
    is_workflow(file)
        || file.starts_with(".github/actions/")
        || file.starts_with("scripts/")
        || file.starts_with("hooks/")
        || file.ends_with(".sh")
        || file == "justfile"
}

fn is_workflow(file: &str) -> bool {
    file.starts_with(".github/workflows/")
}

fn is_policy(file: &str) -> bool {
    file.starts_with(".ci/") || file.starts_with("policy/")
}

fn is_changelog(file: &str) -> bool {
    file.starts_with(".changes/") || file == "CHANGELOG.md" || file.ends_with("/CHANGELOG.md")
}

fn repository_identity(root: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(root)
        .output()
        .context("failed to read origin URL")?;
    if !output.status.success() {
        bail!("origin URL is not configured");
    }
    let raw = String::from_utf8(output.stdout).context("origin URL was not UTF-8")?;
    let value = raw.trim().trim_end_matches(".git");
    let repository = value
        .strip_prefix("git@github.com:")
        .or_else(|| value.strip_prefix("https://github.com/"))
        .or_else(|| value.strip_prefix("http://github.com/"))
        .ok_or_else(|| eyre!("origin URL is not a supported GitHub repository: {value}"))?;
    if repository.matches('/').count() != 1 || repository.is_empty() {
        bail!("origin URL did not resolve to owner/name: {value}");
    }
    Ok(repository.to_string())
}

fn resolve_sha(root: &Path, reference: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", &format!("{reference}^{{commit}}")])
        .current_dir(root)
        .output()
        .with_context(|| format!("failed to resolve {reference}"))?;
    if !output.status.success() {
        bail!("could not resolve {reference}: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let sha =
        String::from_utf8(output.stdout).context("resolved SHA was not UTF-8")?.trim().to_string();
    validate_object_id(&sha, reference)?;
    Ok(sha)
}

fn validate_object_id(value: &str, label: &str) -> Result<()> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{label} must be a full 40-character hexadecimal object ID");
    }
    Ok(())
}

fn validate_resolved_identity(
    base_sha: Option<String>,
    head_sha: Option<String>,
) -> Result<(String, String)> {
    let base_sha = base_sha.ok_or_else(|| eyre!("base SHA was not resolved"))?;
    let head_sha = head_sha.ok_or_else(|| eyre!("head SHA was not resolved"))?;
    validate_object_id(&base_sha, "base SHA")?;
    validate_object_id(&head_sha, "head SHA")?;
    Ok((base_sha, head_sha))
}

fn write_changed_files(path: &Path, files: &[String]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, files.join("\n") + if files.is_empty() { "" } else { "\n" })
        .with_context(|| format!("failed to write {}", path.display()))
}

fn write_receipt(path: &Path, receipt: &ContractReceipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let json =
        serde_json::to_vec_pretty(receipt).context("failed to serialize contract receipt")?;
    fs::write(path, json).with_context(|| format!("failed to write {}", path.display()))
}

fn write_summary(path: &Path, receipt: &ContractReceipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut summary = format!(
        "# Repository Contract\n\n- status: `{:?}`\n- repository: `{}`\n- base: `{}`\n- head: `{}`\n- claim boundary: {}\n\n## Checks\n\n",
        receipt.status,
        receipt.repository,
        receipt.base_sha,
        receipt.head_sha,
        receipt.claim_boundary
    );
    for check in &receipt.checks {
        summary.push_str(&format!("- `{}` — `{:?}` — {}\n", check.id, check.result, check.reason));
    }
    fs::write(path, summary).with_context(|| format!("failed to write {}", path.display()))
}

fn format_command(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ")
}

fn command_output(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => "no output".to_string(),
        (false, true) => stdout,
        (true, false) => stderr,
        (false, false) => format!("stdout: {stdout}; stderr: {stderr}"),
    }
}

fn bounded_output(detail: &str) -> String {
    detail.chars().take(2000).collect()
}

fn bounded_output_with_prefix(prefix: &str, detail: &str) -> String {
    let remaining = 2000usize.saturating_sub(prefix.chars().count());
    let mut bounded = prefix.chars().take(2000).collect::<String>();
    bounded.extend(detail.chars().take(remaining));
    bounded
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::{Result, ensure, eyre};

    fn check(result: ContractResultClass) -> ContractCheck {
        ContractCheck {
            id: "fixture".to_string(),
            reason: "fixture".to_string(),
            command: "fixture".to_string(),
            result,
            detail: "fixture".to_string(),
        }
    }

    #[test]
    fn status_precedence_is_stale_then_not_proven_then_policy() -> Result<()> {
        ensure!(
            status_from_checks(&[
                check(ContractResultClass::PolicyFinding),
                check(ContractResultClass::NotProven)
            ]) == ContractStatus::NotProven,
            "not-proven must outrank policy findings"
        );
        ensure!(
            status_from_checks(&[
                check(ContractResultClass::PolicyFinding),
                check(ContractResultClass::Stale)
            ]) == ContractStatus::Stale,
            "stale must outrank policy findings"
        );
        Ok(())
    }

    #[test]
    fn docs_only_keeps_exact_diff_contract_without_rust_checks() -> Result<()> {
        let checks =
            select_checks(&["docs/guide.md".to_string()], "base", "head", Path::new("files.txt"));
        let ids = checks.iter().map(|check| check.id).collect::<Vec<_>>();
        ensure!(ids == vec!["diff_check", "repo_hygiene"], "docs-only selection was {ids:?}");
        let diff_check = checks.first().ok_or_else(|| eyre!("docs-only selection was empty"))?;
        ensure!(
            diff_check.args == vec!["diff", "--check", "base..head"],
            "diff check must use the exact requested head"
        );
        Ok(())
    }

    #[test]
    fn rust_selection_adds_format_check() -> Result<()> {
        let checks = select_checks(
            &["crates/perl-parser/src/lib.rs".to_string()],
            "base",
            "head",
            Path::new("files.txt"),
        );
        ensure!(
            checks.iter().any(|check| check.id == "rust_format"),
            "Rust selection must include formatting"
        );
        Ok(())
    }

    #[test]
    fn workflow_and_policy_selection_is_deterministic() -> Result<()> {
        let files =
            vec![".github/workflows/ci.yml".to_string(), ".ci/gate-policy.yaml".to_string()];
        let checks = select_checks(&files, "base", "head", Path::new("files.txt"));
        let ids = checks.iter().map(|check| check.id).collect::<Vec<_>>();
        ensure!(
            ids == vec![
                "diff_check",
                "workflow_contract",
                "workflow_trigger_policy",
                "gate_policy",
                "repo_hygiene"
            ],
            "workflow/policy selection was {ids:?}"
        );
        Ok(())
    }

    #[test]
    fn shell_selection_does_not_claim_trigger_policy_linting() -> Result<()> {
        let checks = select_checks(
            &["scripts/check-contract.sh".to_string()],
            "base",
            "head",
            Path::new("files.txt"),
        );
        let ids = checks.iter().map(|check| check.id).collect::<Vec<_>>();
        ensure!(
            ids == vec!["diff_check", "workflow_contract", "repo_hygiene"],
            "shell selection was {ids:?}"
        );
        Ok(())
    }

    #[test]
    fn command_results_map_to_documented_classes() -> Result<()> {
        ensure!(
            result_for_exit("generic", Some(0), "ok") == ContractResultClass::Success,
            "zero exit must be success"
        );
        ensure!(
            result_for_exit("generic", Some(0), "WARN existing advisory baseline")
                == ContractResultClass::PolicyFinding,
            "explicit advisory findings must remain visible"
        );
        ensure!(
            result_for_exit("generic", Some(1), "policy finding")
                == ContractResultClass::PolicyFinding,
            "one exit must be a policy finding"
        );
        ensure!(
            result_for_exit("generic", Some(2), "tool failed") == ContractResultClass::NotProven,
            "other exits must be not-proven"
        );
        ensure!(
            result_for_exit("generic", None, "process terminated")
                == ContractResultClass::NotProven,
            "missing exit code must be not-proven"
        );
        ensure!(
            result_for_exit("generic", Some(2), "instrument failure")
                == ContractResultClass::NotProven,
            "instrument exit status must be not-proven"
        );
        ensure!(
            result_for_exit("generic", Some(1), "policy finding: failed to read expected file")
                == ContractResultClass::PolicyFinding,
            "policy output must not be downgraded by incidental wording"
        );
        let directory = tempfile::tempdir()?;
        let receipt_path = directory.path().join("repo-hygiene.json");
        let args = vec!["--receipt".to_string(), "repo-hygiene.json".to_string()];
        fs::write(&receipt_path, br#"{"status":"NOT_APPLICABLE"}"#)?;
        let (result, _) = classify_repo_hygiene_output(
            directory.path(),
            &args,
            b"repo hygiene: PolicyFinding",
            &[],
        );
        ensure!(
            result == ContractResultClass::NotApplicable,
            "repo-hygiene receipt status must outrank diagnostic output"
        );
        fs::write(&receipt_path, b"not json")?;
        let (result, _) = classify_repo_hygiene_output(
            directory.path(),
            &args,
            b"repo hygiene: PolicyFinding",
            &[],
        );
        ensure!(
            result == ContractResultClass::NotProven,
            "invalid repo-hygiene receipts must be not-proven"
        );
        Ok(())
    }

    #[test]
    fn clearing_repo_hygiene_outputs_removes_receipt_and_summary() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let receipt = directory.path().join("repo-hygiene.json");
        let summary = directory.path().join("repo-hygiene.md");
        fs::write(&receipt, b"stale")?;
        fs::write(&summary, b"stale")?;
        let args = vec![
            "--receipt".to_string(),
            "repo-hygiene.json".to_string(),
            "--summary".to_string(),
            "repo-hygiene.md".to_string(),
        ];
        clear_repo_hygiene_receipt(directory.path(), &args)?;
        ensure!(!receipt.exists(), "stale receipt must be removed");
        ensure!(!summary.exists(), "stale summary must be removed");
        Ok(())
    }

    #[test]
    fn empty_change_range_is_not_applicable() -> Result<()> {
        ensure!(
            select_checks(&[], "base", "head", Path::new("files.txt")).is_empty(),
            "empty ranges must not select checks"
        );
        ensure!(
            status_from_checks(&[]) == ContractStatus::NotApplicable,
            "empty ranges must be not-applicable"
        );
        Ok(())
    }

    #[test]
    fn object_ids_require_full_hex_values() -> Result<()> {
        ensure!(validate_object_id(&"a".repeat(40), "head").is_ok(), "full hex should pass");
        ensure!(validate_object_id("abc", "head").is_err(), "short ID should fail");
        ensure!(
            validate_object_id(&format!("{}z", "a".repeat(39)), "head").is_err(),
            "non-hex ID should fail"
        );
        Ok(())
    }

    #[test]
    fn resolved_identity_rejects_missing_or_malformed_values() -> Result<()> {
        ensure!(
            validate_resolved_identity(None, Some("a".repeat(40))).is_err(),
            "missing base identity must fail closed"
        );
        ensure!(
            validate_resolved_identity(Some("a".repeat(40)), None).is_err(),
            "missing head identity must fail closed"
        );
        ensure!(
            validate_resolved_identity(Some("bad".to_string()), Some("a".repeat(40))).is_err(),
            "malformed base identity must fail closed"
        );
        ensure!(
            validate_resolved_identity(Some("a".repeat(40)), Some("bad".to_string())).is_err(),
            "malformed head identity must fail closed"
        );
        Ok(())
    }

    #[test]
    fn shell_output_is_bounded_and_preserves_streams() -> Result<()> {
        let (result, detail) = classify_check_output("generic", Some(0), b"out", b"err");
        ensure!(result == ContractResultClass::Success, "ordinary output was not successful");
        ensure!(detail == "stdout: out; stderr: err", "combined output was {detail:?}");
        ensure!(
            bounded_output(&command_output(&vec![b'x'; 3000], &[])).len() == 2000,
            "output was not bounded"
        );
        let late_warning = format!("{}\nWARN late advisory", "x".repeat(2000));
        let (late_result, late_detail) =
            classify_check_output("generic", Some(0), late_warning.as_bytes(), &[]);
        ensure!(
            late_result == ContractResultClass::PolicyFinding,
            "late advisory output must be classified before receipt truncation"
        );
        ensure!(late_detail.len() == 2000, "late advisory detail was not bounded");
        let (_, terminated_detail) = classify_check_output("generic", None, &vec![b'x'; 2000], &[]);
        ensure!(
            terminated_detail.len() == 2000,
            "termination detail was not bounded after adding its prefix"
        );
        Ok(())
    }

    #[test]
    fn receipts_preserve_full_identity_in_json_and_markdown() -> Result<()> {
        let base = "0123456789abcdef0123456789abcdef01234567";
        let head = "89abcdef0123456789abcdef0123456789abcdef";
        let receipt = ContractReceipt {
            schema_version: SCHEMA_VERSION,
            provider_action: "repository_contract",
            repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
            base_sha: base.to_string(),
            head_sha: head.to_string(),
            changed_files: vec!["docs/guide.md".to_string()],
            changed_surfaces: vec!["docs_or_other".to_string()],
            scope: ScopeOutput {
                schema_version: 2,
                base: base.to_string(),
                head_sha: head.to_string(),
                changed_files: vec!["docs/guide.md".to_string()],
                diff_class: "prose".to_string(),
                direct_crates: vec![],
                reverse_dep_closure: vec![],
                architecture_wideners: vec![],
                risk_tags: vec![],
                platform_overrides: Default::default(),
                selected_lanes: vec![],
                selected_heavy_lanes: vec![],
                lanes: Default::default(),
                explanations: Default::default(),
            },
            checks: vec![check(ContractResultClass::Success)],
            status: ContractStatus::Success,
            claim_boundary: CLAIM_BOUNDARY,
        };
        let directory = tempfile::tempdir()?;
        let json_path = directory.path().join("receipt.json");
        let summary_path = directory.path().join("summary.md");
        write_receipt(&json_path, &receipt)?;
        write_summary(&summary_path, &receipt)?;

        let json: serde_json::Value = serde_json::from_slice(&fs::read(&json_path)?)?;
        ensure!(
            json.get("base_sha").and_then(serde_json::Value::as_str) == Some(base),
            "JSON base identity was not preserved"
        );
        ensure!(
            json.get("head_sha").and_then(serde_json::Value::as_str) == Some(head),
            "JSON head identity was not preserved"
        );
        let scope = json.get("scope").ok_or_else(|| eyre!("JSON scope was missing"))?;
        ensure!(
            scope.get("base").and_then(serde_json::Value::as_str) == Some(base),
            "scope base identity was not preserved"
        );
        ensure!(
            scope.get("head_sha").and_then(serde_json::Value::as_str) == Some(head),
            "scope head identity was not preserved"
        );
        let summary = fs::read_to_string(&summary_path)?;
        ensure!(summary.contains(base), "Markdown base identity was not preserved");
        ensure!(summary.contains(head), "Markdown head identity was not preserved");
        Ok(())
    }
}
