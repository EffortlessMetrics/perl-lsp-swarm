//! `cargo xtask ux cases discover` — emit `ux_case_inventory.v1` (#9890).
//!
//! This module owns command orchestration only. Case identity, the inventory
//! schema, Cargo/libtest parsing, and every failure classification live in
//! `perl_lsp_ux_tests::case_inventory`, which is the single UX control-plane
//! authority those types belong to. Nothing here re-implements them.
//!
//! Concretely, this file supplies:
//!
//! - the real [`UxDiscoveryCommands`] implementation (Cargo, libtest, sha256);
//! - the subject facts discovery cannot observe for itself (repository SHA and
//!   dirty state, `Cargo.lock` and manifest digests, toolchain, host target);
//! - CLI plumbing and deterministic output.

use color_eyre::eyre::{Result, eyre};
use perl_lsp_ux_tests::case_inventory::{
    self, UxCaseInventory, UxDirtyState, UxDiscoveryCommands, UxDiscoveryFailure,
    UxDiscoveryRequest, sha256_hex,
};
use perl_lsp_ux_tests::taxonomy::UxCiTier;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::utils;

/// Default location for the emitted inventory.
pub const DEFAULT_OUT: &str = "target/receipts/editor-ux/ux-case-inventory.json";

/// Maximum bytes of failing command output retained in a failure.
const DETAIL_LIMIT: usize = 2000;

/// The production [`UxDiscoveryCommands`] implementation.
///
/// Every method is read-only: it compiles test targets, asks executables to
/// list themselves, and digests files. It never runs a test case.
struct SystemDiscoveryCommands {
    workspace_root: PathBuf,
}

fn truncate(text: &str) -> String {
    if text.len() <= DETAIL_LIMIT {
        return text.to_string();
    }
    let mut end = DETAIL_LIMIT;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… (truncated)", &text[..end])
}

impl UxDiscoveryCommands for SystemDiscoveryCommands {
    fn compile_test_targets(&self, argv: &[String]) -> Result<String, UxDiscoveryFailure> {
        let (program, args) = argv.split_first().ok_or_else(|| {
            UxDiscoveryFailure::InstrumentFailure { reason: "empty compile argv".to_string() }
        })?;
        let output =
            Command::new(program).args(args).current_dir(&self.workspace_root).output().map_err(
                |error| UxDiscoveryFailure::CargoInvocationFailed {
                    argv: argv.to_vec(),
                    status: None,
                    detail: error.to_string(),
                },
            )?;
        if !output.status.success() {
            return Err(UxDiscoveryFailure::CargoInvocationFailed {
                argv: argv.to_vec(),
                status: output.status.code(),
                detail: truncate(&String::from_utf8_lossy(&output.stderr)),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn list_cases(
        &self,
        target_identity: &str,
        executable: &Path,
        argv: &[String],
    ) -> Result<String, UxDiscoveryFailure> {
        let output = Command::new(executable)
            .args(argv)
            .current_dir(&self.workspace_root)
            .output()
            .map_err(|error| UxDiscoveryFailure::ListCommandFailed {
                target: target_identity.to_string(),
                argv: argv.to_vec(),
                status: None,
                detail: error.to_string(),
            })?;
        if !output.status.success() {
            return Err(UxDiscoveryFailure::ListCommandFailed {
                target: target_identity.to_string(),
                argv: argv.to_vec(),
                status: output.status.code(),
                detail: truncate(&String::from_utf8_lossy(&output.stderr)),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn executable_digest(
        &self,
        target_identity: &str,
        executable: &Path,
    ) -> Result<String, UxDiscoveryFailure> {
        let bytes =
            fs::read(executable).map_err(|error| UxDiscoveryFailure::DigestUnavailable {
                target: target_identity.to_string(),
                reason: error.to_string(),
            })?;
        Ok(sha256_hex(&bytes))
    }

    fn executable_exists(&self, executable: &Path) -> bool {
        executable.is_file()
    }
}

/// Capture a command's stdout, returning `None` when the probe cannot run.
///
/// A failed probe becomes an explicit unknown subject fact rather than a
/// fabricated default.
fn probe(root: &Path, program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).current_dir(root).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn file_digest(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let hex: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    Some(format!("sha256:{hex}"))
}

/// Extract `key: value` from `rustc -vV` output.
fn rustc_field(verbose: &str, key: &str) -> Option<String> {
    verbose.lines().find_map(|line| {
        line.strip_prefix(key).and_then(|rest| rest.strip_prefix(": ")).map(str::to_string)
    })
}

fn build_request(root: &Path, tier: UxCiTier, include_local_execution: bool) -> UxDiscoveryRequest {
    let mut request = UxDiscoveryRequest::new(tier, root.to_path_buf());

    request.repository_sha = probe(root, "git", &["rev-parse", "HEAD"])
        .map(|sha| sha.trim().to_string())
        .filter(|sha| !sha.is_empty());
    request.repository_dirty_state = match probe(root, "git", &["status", "--porcelain"]) {
        Some(status) if status.trim().is_empty() => UxDirtyState::Clean,
        Some(_) => UxDirtyState::Dirty,
        None => UxDirtyState::Unknown,
    };
    request.cargo_lock_digest = file_digest(&root.join("Cargo.lock"));
    request.package_manifest_digest = file_digest(
        &root.join("crates").join(case_inventory::UX_INVENTORY_PACKAGE).join("Cargo.toml"),
    );

    let verbose = probe(root, "rustc", &["-vV"]).unwrap_or_default();
    request.rust_toolchain = rustc_field(&verbose, "release")
        .map_or_else(|| "unknown".to_string(), |release| format!("rustc {release}"));
    request.host_target = rustc_field(&verbose, "host").unwrap_or_else(|| "unknown".to_string());
    // Cargo builds test executables under the `test` profile.
    request.cargo_profile = "test".to_string();

    request.include_local_execution = include_local_execution;
    request.generated_at = include_local_execution.then(|| chrono::Utc::now().to_rfc3339());
    request
}

/// Serialize an inventory deterministically, with a trailing newline.
fn render(inventory: &UxCaseInventory) -> Result<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(inventory)?))
}

fn write_inventory(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, body)?;
    Ok(())
}

/// Run `ux cases discover`.
///
/// # Errors
///
/// Returns the discovery failure verbatim rather than emitting a smaller
/// denominator: a missing binary, a malformed listing, a stale wrong-profile
/// artifact, or a colliding identity is never rendered as a complete inventory.
pub fn run_discover(
    profile: &str,
    out: Option<PathBuf>,
    local_execution: bool,
    stdout_json: bool,
) -> Result<()> {
    let tier = case_inventory::parse_profile(profile).map_err(|failure| eyre!("{failure}"))?;
    let root = utils::project_root()?;

    let commands = SystemDiscoveryCommands { workspace_root: root.clone() };
    let request = build_request(&root, tier, local_execution);
    let inventory = case_inventory::discover_cases(&commands, &request)
        .map_err(|failure| eyre!("{failure}"))?;
    inventory.verify_digest().map_err(|failure| eyre!("{failure}"))?;

    let body = render(&inventory)?;
    let out = out.unwrap_or_else(|| root.join(DEFAULT_OUT));
    write_inventory(&out, &body)?;

    if stdout_json {
        print!("{body}");
    } else {
        println!(
            "ux_case_inventory.v1 profile={} targets={} cases={} zero-case-targets={}",
            inventory.subject.operational_profile,
            inventory.totals.target_count,
            inventory.totals.case_count,
            inventory.totals.zero_case_target_count
        );
        println!("subject   {}", inventory.subject.subject_digest);
        println!("inventory {}", inventory.inventory_digest);
        println!("written   {}", out.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn rustc_fields_parse_from_verbose_output() -> TestResult {
        let verbose = "rustc 1.95.0 (59807616e 2026-04-14)\nbinary: rustc\nrelease: 1.95.0\nhost: x86_64-unknown-linux-gnu\n";
        assert_eq!(rustc_field(verbose, "release").as_deref(), Some("1.95.0"));
        assert_eq!(rustc_field(verbose, "host").as_deref(), Some("x86_64-unknown-linux-gnu"));
        assert_eq!(rustc_field(verbose, "absent"), None);
        Ok(())
    }

    #[test]
    fn unknown_profiles_are_rejected_before_any_command_runs() {
        let failure = run_discover("staging", None, false, false)
            .expect_err("an unknown profile must be rejected");
        assert!(failure.to_string().contains("unknown discovery profile"), "{failure}");
    }

    #[test]
    fn detail_truncation_respects_character_boundaries() -> TestResult {
        let long = "é".repeat(DETAIL_LIMIT);
        let truncated = truncate(&long);
        assert!(truncated.ends_with("… (truncated)"));
        assert!(truncated.len() < long.len() + 20);
        assert_eq!(truncate("short"), "short");
        Ok(())
    }

    #[test]
    fn the_default_output_path_is_under_the_ux_receipt_root() {
        assert!(DEFAULT_OUT.starts_with("target/receipts/editor-ux/"));
        assert!(DEFAULT_OUT.ends_with(".json"));
    }
}
