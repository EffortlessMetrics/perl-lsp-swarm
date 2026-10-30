//! PR-driven release orchestration wrapper.
//!
//! This task delegates the existing shell flow in
//! `scripts/release-turnkey-pr.sh` while exposing the command through
//! `cargo xtask release-turnkey`.

use color_eyre::eyre::{Context, Result, bail};
use std::process::Command;

use crate::utils::project_root;

/// Configuration for the release turn-key wrapper task.
pub struct ReleaseTurnkeyConfig {
    pub version: Option<String>,
    pub positional_version: Option<String>,
    pub prerelease: bool,
    pub dry_run: bool,
    pub skip_crates: bool,
    pub skip_extension: bool,
    pub skip_docker: bool,
    pub base_branch: Option<String>,
    pub no_auto_merge: bool,
    pub no_wait_pr_merge: bool,
    pub no_wait_release: bool,
    pub transaction: Option<String>,
    pub workflow_timeout: Option<u64>,
}

impl ReleaseTurnkeyConfig {
    fn resolve_version(&self) -> Result<String> {
        match (self.version.as_deref(), self.positional_version.as_deref()) {
            (None, None) => bail!("release version is required"),
            (Some(v), None) => Ok(v.to_string()),
            (None, Some(v)) => Ok(v.to_string()),
            (Some(v), Some(p)) if v == p => Ok(v.to_string()),
            (Some(v), Some(p)) => {
                bail!("version mismatch: --version {v} does not match positional {p}")
            }
        }
    }

    fn build_args(&self, version: &str) -> Vec<String> {
        let mut args = vec!["--version".to_string(), version.to_string()];

        if self.prerelease {
            args.push("--prerelease".to_string());
        }
        if self.dry_run {
            args.push("--dry-run".to_string());
        }
        if self.skip_crates {
            args.push("--skip-crates".to_string());
        }
        if self.skip_extension {
            args.push("--skip-extension".to_string());
        }
        if self.skip_docker {
            args.push("--skip-docker".to_string());
        }
        if let Some(base_branch) = &self.base_branch {
            args.push("--base-branch".to_string());
            args.push(base_branch.clone());
        }
        if self.no_auto_merge {
            args.push("--no-auto-merge".to_string());
        }
        if self.no_wait_pr_merge {
            args.push("--no-wait-pr-merge".to_string());
        }
        if self.no_wait_release {
            args.push("--no-wait-release".to_string());
        }
        if let Some(transaction) = &self.transaction {
            args.push("--transaction".to_string());
            args.push(transaction.clone());
        }
        if let Some(workflow_timeout) = self.workflow_timeout {
            args.push("--workflow-timeout".to_string());
            args.push(workflow_timeout.to_string());
        }

        args
    }
}

/// Execute the shell release driver via the canonical script path.
pub fn run(config: ReleaseTurnkeyConfig) -> Result<()> {
    let root = project_root()?;
    let script = root.join("scripts").join("release-turnkey-pr.sh");

    let version = config.resolve_version()?;
    let args = config.build_args(&version);

    let status = Command::new("bash")
        .arg(&script)
        .args(&args)
        .current_dir(&root)
        .status()
        .with_context(|| format!("failed to run {}", script.display()))?;

    match status.code() {
        Some(0) => Ok(()),
        Some(2) => {
            // Typed `manual_merge_required` handoff (#16798): not a command
            // failure and not release-orchestration success. The shell driver
            // remaps accidental subprocess 2/4 to 1 before we observe them.
            std::process::exit(2);
        }
        Some(4) => {
            // Typed `merge_requested_waiting_for_landing` handoff (#16798).
            std::process::exit(4);
        }
        _ => bail!("release-turnkey command failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_args_forwards_transaction_path() {
        let config = ReleaseTurnkeyConfig {
            version: Some("0.9.2".to_string()),
            positional_version: None,
            prerelease: false,
            dry_run: false,
            skip_crates: false,
            skip_extension: false,
            skip_docker: false,
            base_branch: None,
            no_auto_merge: true,
            no_wait_pr_merge: false,
            no_wait_release: false,
            transaction: Some("/tmp/tx.json".to_string()),
            workflow_timeout: None,
        };
        let args = config.build_args("0.9.2");
        assert!(
            args.windows(2).any(|pair| pair == ["--transaction", "/tmp/tx.json"]),
            "expected --transaction /tmp/tx.json in {args:?}"
        );
        assert!(args.iter().any(|arg| arg == "--no-auto-merge"));
    }
}
