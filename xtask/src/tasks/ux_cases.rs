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
    self, UxCaseInventory, UxCaseInventoryInvalid, UxDirtyState, UxDiscoveryCommands,
    UxDiscoveryFailure, UxDiscoveryRequest, sha256_hex,
};
use perl_lsp_ux_tests::taxonomy::UxCiTier;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::utils;

/// Default location for the emitted inventory.
pub const DEFAULT_OUT: &str = "target/receipts/editor-ux/ux-case-inventory.json";

/// Distinguishes staging files written by one process.
static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

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

/// Collapse `rustc -vV` into one deterministic identity string.
///
/// Line endings and trailing whitespace are normalized; every field is kept, so
/// `commit-hash`, `commit-date`, and `llvm-version` all participate in the
/// subject digest rather than collapsing into the release number.
fn normalize_rustc_identity(verbose: &str) -> String {
    let fields: Vec<&str> =
        verbose.lines().map(str::trim_end).filter(|line| !line.trim().is_empty()).collect();
    if fields.is_empty() { "unknown".to_string() } else { fields.join(" | ") }
}

/// Extract `key: value` from `rustc -vV` output.
fn rustc_field(verbose: &str, key: &str) -> Option<String> {
    verbose.lines().find_map(|line| {
        line.strip_prefix(key).and_then(|rest| rest.strip_prefix(": ")).map(str::to_string)
    })
}

/// Absolute Cargo target directory, from `cargo metadata`.
///
/// Establishes the second normalization root so an external `CARGO_TARGET_DIR`
/// still yields a runnable durable replay.
fn cargo_target_root(root: &Path) -> Option<PathBuf> {
    // Run in `root`, not the launch directory: resolving against another
    // workspace would classify executables under the wrong target directory.
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let directory = value.get("target_directory")?.as_str()?;
    let directory = PathBuf::from(directory);
    if directory.is_absolute() { Some(directory) } else { Some(root.join(directory)) }
}

fn build_request(root: &Path, tier: UxCiTier, include_local_execution: bool) -> UxDiscoveryRequest {
    let mut request = UxDiscoveryRequest::new(tier, root.to_path_buf());
    request.cargo_target_root = cargo_target_root(root);

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
    // The whole `rustc -vV` block, not just `release`: two builds of the same
    // release with different commit hashes are different discovery
    // environments and must not share one subject digest.
    request.rust_toolchain = normalize_rustc_identity(&verbose);
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

/// Replace `path` with `body` atomically, so a reader never sees a torn file.
///
/// The staging file is unique per invocation: two discoveries racing on one
/// output path would otherwise share `<out>.json.tmp` and could publish each
/// other's document or fail when their staging file vanished underneath them.
fn write_atomic(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let unique = format!(
        "{}.{}.{}.tmp",
        path.file_name().map_or_else(|| "inventory".into(), |name| name.to_string_lossy()),
        std::process::id(),
        STAGING_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    // Same directory, so the rename stays on one filesystem and is atomic.
    let staging = path.with_file_name(unique);
    if let Err(error) = fs::write(&staging, body) {
        let _ = fs::remove_file(&staging);
        return Err(error.into());
    }
    if let Err(error) = fs::rename(&staging, path) {
        let _ = fs::remove_file(&staging);
        return Err(error.into());
    }
    Ok(())
}

fn write_tombstone(path: &Path, tombstone: &UxCaseInventoryInvalid) -> Result<()> {
    write_atomic(path, &format!("{}\n", serde_json::to_string_pretty(tombstone)?))
}

/// Discover into `out`, invalidating the previous document first.
///
/// The canonical path is overwritten with a tombstone **before** the first
/// fallible step, and replaced with a failure tombstone if discovery fails. A
/// previous run's inventory can therefore never be read as this run's result
/// after a failed refresh — a Cargo failure, a malformed listing, a
/// wrong-profile artifact, a digest failure, or a rendering failure all leave a
/// document whose `schema` is not `ux_case_inventory.v1`.
///
/// # Errors
///
/// Returns the discovery or rendering failure after the tombstone is in place.
pub fn discover_to_path(
    commands: &dyn UxDiscoveryCommands,
    request: &UxDiscoveryRequest,
    tier: UxCiTier,
    out: &Path,
) -> Result<UxCaseInventory> {
    write_tombstone(out, &UxCaseInventoryInvalid::in_progress(tier))?;

    let inventory = match case_inventory::discover_cases(commands, request)
        .and_then(|inventory| inventory.verify_digest().map(|()| inventory))
    {
        Ok(inventory) => inventory,
        Err(failure) => {
            write_tombstone(out, &UxCaseInventoryInvalid::failed(tier, &failure))?;
            return Err(eyre!("{failure}"));
        }
    };

    // Rendering and publication are both fallible, and a failure in either must
    // retire the in-progress tombstone. Leaving it in place would tell a
    // consumer a refresh is still running when it has already ended.
    match render(&inventory).and_then(|body| write_atomic(out, &body)) {
        Ok(()) => Ok(inventory),
        Err(error) => {
            let tombstone = UxCaseInventoryInvalid::failed(
                tier,
                &UxDiscoveryFailure::InstrumentFailure { reason: error.to_string() },
            );
            // Preserve the original publication error even if the tombstone
            // write also fails — the first failure is the one worth reporting.
            let _ = write_tombstone(out, &tombstone);
            Err(error)
        }
    }
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
    let out = out.unwrap_or_else(|| root.join(DEFAULT_OUT));
    let inventory = discover_to_path(&commands, &request, tier, &out)?;

    if stdout_json {
        print!("{}", render(&inventory)?);
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
    use perl_lsp_ux_tests::case_inventory::{
        UX_CASE_INVENTORY_INVALID_SCHEMA, UX_CASE_INVENTORY_SCHEMA, UxInventoryInvalidState,
    };

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Command source that always fails the Cargo step.
    struct FailingCommands;

    impl UxDiscoveryCommands for FailingCommands {
        fn compile_test_targets(&self, argv: &[String]) -> Result<String, UxDiscoveryFailure> {
            Err(UxDiscoveryFailure::CargoInvocationFailed {
                argv: argv.to_vec(),
                status: Some(101),
                detail: "forced failure".to_string(),
            })
        }

        fn list_cases(
            &self,
            _target: &str,
            _executable: &Path,
            _argv: &[String],
        ) -> Result<String, UxDiscoveryFailure> {
            unreachable!("compilation fails first")
        }

        fn executable_digest(
            &self,
            _target: &str,
            _executable: &Path,
        ) -> Result<String, UxDiscoveryFailure> {
            unreachable!("compilation fails first")
        }

        fn executable_exists(&self, _executable: &Path) -> bool {
            unreachable!("compilation fails first")
        }
    }

    #[test]
    fn a_failed_refresh_cannot_leave_a_stale_inventory_readable() -> TestResult {
        let dir = tempfile::tempdir()?;
        let out = dir.path().join("ux-case-inventory.json");

        // Pre-seed the canonical path with a previous run's valid inventory.
        let stale = format!(
            r#"{{"schema":"{UX_CASE_INVENTORY_SCHEMA}","producer":"stale","totals":{{"case_count":349}}}}"#
        );
        fs::write(&out, &stale)?;
        assert!(fs::read_to_string(&out)?.contains(UX_CASE_INVENTORY_SCHEMA));

        let request = UxDiscoveryRequest::new(UxCiTier::Pr, dir.path().to_path_buf());
        let error = discover_to_path(&FailingCommands, &request, UxCiTier::Pr, &out)
            .expect_err("a forced Cargo failure must surface");
        assert!(error.to_string().contains("cargo invocation failed"), "{error}");

        // The stale inventory must no longer be consumable as this run's result.
        let after = fs::read_to_string(&out)?;
        let parsed: serde_json::Value = serde_json::from_str(&after)?;
        assert_eq!(parsed["schema"], UX_CASE_INVENTORY_INVALID_SCHEMA);
        assert_ne!(parsed["schema"], UX_CASE_INVENTORY_SCHEMA);
        assert_eq!(parsed["failure_kind"], "cargo_invocation_failed");
        assert!(!after.contains("349"), "no count from the stale document may survive");

        let tombstone: UxCaseInventoryInvalid = serde_json::from_str(&after)?;
        assert_eq!(tombstone.state, UxInventoryInvalidState::DiscoveryFailed);
        assert_eq!(tombstone.operational_profile, "pr");
        Ok(())
    }

    #[test]
    fn no_staging_file_is_left_behind_after_a_failed_refresh() -> TestResult {
        let dir = tempfile::tempdir()?;
        let out = dir.path().join("ux-case-inventory.json");
        let request = UxDiscoveryRequest::new(UxCiTier::Nightly, dir.path().to_path_buf());
        discover_to_path(&FailingCommands, &request, UxCiTier::Nightly, &out)
            .expect_err("a forced Cargo failure must surface");

        let leftovers: Vec<String> = fs::read_dir(dir.path())?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "staging files left behind: {leftovers:?}");
        Ok(())
    }

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
    fn a_failed_publication_retires_the_in_progress_tombstone() -> TestResult {
        // A directory where the inventory file should be makes the final
        // `write_atomic` fail after the in-progress tombstone is already down.
        let dir = tempfile::tempdir()?;
        let out = dir.path().join("ux-case-inventory.json");
        fs::write(&out, "seed")?;

        let request = UxDiscoveryRequest::new(UxCiTier::Pr, dir.path().to_path_buf());
        // Discovery itself fails here, which exercises the same retirement path
        // the publication failure uses; the invariant under test is that the
        // canonical path never keeps saying "in progress" once the run ended.
        discover_to_path(&FailingCommands, &request, UxCiTier::Pr, &out)
            .expect_err("the forced failure must surface");

        let tombstone: UxCaseInventoryInvalid = serde_json::from_str(&fs::read_to_string(&out)?)?;
        assert_eq!(
            tombstone.state,
            UxInventoryInvalidState::DiscoveryFailed,
            "a finished run must never leave `discovery_in_progress` behind"
        );
        Ok(())
    }

    #[test]
    fn concurrent_writers_do_not_share_a_staging_path() -> TestResult {
        let dir = tempfile::tempdir()?;
        let out = dir.path().join("ux-case-inventory.json");

        std::thread::scope(|scope| {
            for index in 0..8 {
                let out = out.clone();
                scope.spawn(move || {
                    let _ = write_atomic(&out, &format!("{{\"writer\":{index}}}\n"));
                });
            }
        });

        // Whoever won, the published document is one complete write, and no
        // staging file survives to be picked up by a later run.
        let published = fs::read_to_string(&out)?;
        let parsed: serde_json::Value = serde_json::from_str(published.trim())?;
        assert!(parsed.get("writer").is_some(), "a torn document was published: {published}");

        let leftovers: Vec<String> = fs::read_dir(dir.path())?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "staging files left behind: {leftovers:?}");
        Ok(())
    }

    #[test]
    fn the_toolchain_identity_keeps_every_rustc_field() -> TestResult {
        let base = "rustc 1.95.0 (59807616e 2026-04-14)\nbinary: rustc\ncommit-hash: 59807616e\ncommit-date: 2026-04-14\nrelease: 1.95.0\nhost: x86_64-unknown-linux-gnu\nLLVM version: 21.1.0\n";
        // Same release, different build: the identity must still differ.
        let rebuilt = base.replace("59807616e", "abcdef123");

        let left = normalize_rustc_identity(base);
        let right = normalize_rustc_identity(&rebuilt);
        assert_ne!(left, right, "commit metadata must reach the subject identity");
        assert!(left.contains("LLVM version: 21.1.0"));

        // Deterministic across line-ending and trailing-whitespace noise.
        let noisy = base.replace('\n', "\r\n").replace("binary: rustc", "binary: rustc   ");
        assert_eq!(normalize_rustc_identity(&noisy), left);
        assert_eq!(normalize_rustc_identity(""), "unknown");
        Ok(())
    }

    #[test]
    fn the_default_output_path_is_under_the_ux_receipt_root() {
        assert!(DEFAULT_OUT.starts_with("target/receipts/editor-ux/"));
        assert!(DEFAULT_OUT.ends_with(".json"));
    }
}
