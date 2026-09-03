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
use std::fs;
use std::io::Write;
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
        // `argv` on the failure is contractually the exact invoked command, so
        // it must name the executable rather than only its arguments.
        let invoked: Vec<String> = std::iter::once(executable.to_string_lossy().into_owned())
            .chain(argv.iter().cloned())
            .collect();
        let output = Command::new(executable)
            .args(argv)
            .current_dir(&self.workspace_root)
            .output()
            .map_err(|error| UxDiscoveryFailure::ListCommandFailed {
                target: target_identity.to_string(),
                argv: invoked.clone(),
                status: None,
                detail: error.to_string(),
            })?;
        if !output.status.success() {
            return Err(UxDiscoveryFailure::ListCommandFailed {
                target: target_identity.to_string(),
                argv: invoked,
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
    let label = format!("{program} {}", args.join(" "));
    match Command::new(program).args(args).current_dir(root).output() {
        Ok(output) if output.status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        // The subject fact still becomes `unknown`; this only makes the reason
        // recoverable during triage, where "missing binary" and "non-zero exit"
        // are otherwise indistinguishable.
        Ok(output) => {
            report_probe_failure(
                &label,
                &format!(
                    "exit {:?}: {}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            );
            None
        }
        Err(error) => {
            report_probe_failure(&label, &error.to_string());
            None
        }
    }
}

/// Record why a subject probe could not answer, without changing the subject.
fn report_probe_failure(label: &str, detail: &str) {
    eprintln!(
        "ux cases discover: subject probe `{label}` unavailable ({}) — the affected subject field is recorded as unknown and declared as a limitation",
        truncate(detail)
    );
}

fn file_digest(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(sha256_hex(&bytes))
}

/// Deterministic description of any environment-declared compiler wrapper.
///
/// `RUSTC_WRAPPER` and `RUSTC_WORKSPACE_WRAPPER` sit between Cargo and the
/// compiler, so two otherwise identical toolchains with different wrappers are
/// different build environments and must not share a subject digest.
///
/// This reads the **environment only**. A wrapper declared as
/// `build.rustc-wrapper` in a Cargo `config.toml` — which this repository
/// documents in `.cargo/config.local.toml.example` — is not seen here, and two
/// builds differing only by such a wrapper share a subject digest.
///
/// That is deliberate rather than overlooked. Cargo resolves the setting across
/// the workspace file, every parent directory, and `$CARGO_HOME`, with the
/// environment taking precedence; reading just the workspace file would still
/// miss `$CARGO_HOME/config.toml`, where a global `rustc-wrapper` usually
/// lives, while making the subject look complete. The gap is therefore declared
/// as the `cargo_config_wrapper_not_resolved` limitation instead of being
/// half-closed. Full resolution is tracked separately.
fn compiler_wrappers() -> Option<String> {
    let mut declared: Vec<String> = Vec::new();
    for key in ["RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"] {
        if let Ok(value) = std::env::var(key)
            && !value.trim().is_empty()
        {
            declared.push(format!("{key}: {}", value.trim()));
        }
    }
    (!declared.is_empty()).then(|| declared.join(" | "))
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
/// `cargo metadata` for `root`, or `None` when it could not be read.
///
/// Always run in `root`, not the launch directory: resolving against another
/// workspace would classify executables under the wrong target directory.
fn cargo_metadata_value(root: &Path) -> Option<serde_json::Value> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        report_probe_failure("cargo metadata", &String::from_utf8_lossy(&output.stderr));
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

/// Both readers take the metadata document rather than fetching their own, so
/// one discovery sees one consistent view of the workspace and spawns one
/// `cargo metadata` rather than two.
fn cargo_target_root(root: &Path, value: &serde_json::Value) -> Option<PathBuf> {
    let directory = value.get("target_directory")?.as_str()?;
    let directory = PathBuf::from(directory);
    if directory.is_absolute() { Some(directory) } else { Some(root.join(directory)) }
}

/// Manifest path for the UX package, as `cargo metadata` resolves it.
fn package_manifest_path(value: &serde_json::Value) -> Option<PathBuf> {
    let packages = value.get("packages")?.as_array()?;
    packages
        .iter()
        .find(|package| {
            package.get("name").and_then(serde_json::Value::as_str)
                == Some(case_inventory::UX_INVENTORY_PACKAGE)
        })
        .and_then(|package| package.get("manifest_path"))
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
}

fn build_request(root: &Path, tier: UxCiTier, include_local_execution: bool) -> UxDiscoveryRequest {
    let mut request = UxDiscoveryRequest::new(tier, root.to_path_buf());
    let metadata = cargo_metadata_value(root);
    request.cargo_target_root = metadata.as_ref().and_then(|value| cargo_target_root(root, value));

    request.repository_sha = probe(root, "git", &["rev-parse", "HEAD"])
        .map(|sha| sha.trim().to_string())
        .filter(|sha| !sha.is_empty());
    request.repository_dirty_state = match probe(root, "git", &["status", "--porcelain"]) {
        Some(status) if status.trim().is_empty() => UxDirtyState::Clean,
        Some(_) => UxDirtyState::Dirty,
        None => UxDirtyState::Unknown,
    };
    request.cargo_lock_digest = file_digest(&root.join("Cargo.lock"));
    // Resolved through `cargo metadata` so a package rename or a workspace
    // relayout cannot silently drop the digest; the hardcoded layout is only a
    // fallback, and a miss is a declared limitation either way.
    request.package_manifest_digest = metadata
        .as_ref()
        .and_then(package_manifest_path)
        .as_deref()
        .and_then(file_digest)
        .or_else(|| {
            file_digest(
                &root.join("crates").join(case_inventory::UX_INVENTORY_PACKAGE).join("Cargo.toml"),
            )
        });

    // Cargo compiles with `$RUSTC` when it is set, so probing the PATH `rustc`
    // would record a compiler that never touched these executables.
    let compiler = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let verbose = probe(root, &compiler, &["-vV"]).unwrap_or_default();
    // A wrapper sits between Cargo and rustc and can change what is built, so
    // it belongs in the subject even though `rustc -vV` cannot see it.
    let wrappers = compiler_wrappers();
    // The whole `rustc -vV` block, not just `release`: two builds of the same
    // release with different commit hashes are different discovery
    // environments and must not share one subject digest.
    request.rust_toolchain = match wrappers {
        Some(wrappers) => format!("{} | {wrappers}", normalize_rustc_identity(&verbose)),
        None => normalize_rustc_identity(&verbose),
    };
    request.host_target = rustc_field(&verbose, "host").unwrap_or_else(|| "unknown".to_string());
    // Cargo builds test executables under the `test` profile.
    request.cargo_profile = "test".to_string();

    request.include_local_execution = include_local_execution;
    // Every probe above can fail. `UxDiscoveryRequest` records `None`/`unknown`
    // for those, and `discover_cases` turns each into a declared limitation, so
    // a subject assembled from partial evidence never reads as fully known.
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
/// Directory that receives the staged file and the post-rename durability sync.
///
/// `Path::parent` of a bare file name is `Some("")`, and `create_dir_all("")`
/// fails — so `--out inventory.json` would never write anything. Resolving the
/// empty case to `.` also keeps the durability step applicable to a bare output
/// rather than silently skipped.
///
/// Separated from [`write_atomic`] so the rule is provable without a process
/// working directory: `set_current_dir` is process-wide, and a test that moved
/// it could race any sibling test that reads it.
fn output_parent(path: &Path) -> &Path {
    path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."))
}

fn write_atomic(path: &Path, body: &str) -> Result<()> {
    let parent = output_parent(path);
    fs::create_dir_all(parent)?;
    let unique = format!(
        "{}.{}.{}.tmp",
        path.file_name().map_or_else(|| "inventory".into(), |name| name.to_string_lossy()),
        std::process::id(),
        STAGING_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    // Same directory, so the rename stays on one filesystem and is atomic.
    let staging = path.with_file_name(unique);
    if let Err(error) = write_and_sync(&staging, body) {
        let _ = fs::remove_file(&staging);
        return Err(error);
    }
    if let Err(error) = fs::rename(&staging, path) {
        let _ = fs::remove_file(&staging);
        return Err(error.into());
    }
    sync_directory(parent)?;
    Ok(())
}

/// Flush a directory entry so a rename survives a crash.
///
/// Without this, `write_atomic` can report success before the new name is
/// durable and a crash loses an inventory a downstream gate already believes
/// exists. Failures propagate rather than being swallowed: a write reported as
/// successful must actually be durable.
#[cfg(unix)]
fn sync_directory(parent: &Path) -> Result<()> {
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

/// Windows has no directory handle to fsync; `MoveFileEx` ordering is the
/// platform's durability contract for the rename itself.
#[cfg(not(unix))]
fn sync_directory(_parent: &Path) -> Result<()> {
    Ok(())
}

/// Write `body` to `path` and flush it to stable storage before returning.
fn write_and_sync(path: &Path, body: &str) -> Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(body.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

/// Invalidate the canonical path before discovery starts.
///
/// The tombstone is the preferred outcome because it distinguishes "a refresh
/// is running" from "nothing ever ran". When it cannot be written the previous
/// document must still stop being consumable, so the stale file is removed as a
/// last resort — `unlink` needs no free space, so this recovers the realistic
/// disk-full case where the write failed but the old inventory is still sitting
/// there looking current. That specific branch is not unit-tested: simulating a
/// full filesystem is not available here, and running as root defeats
/// permission-based simulation. The branch where removal also fails *is*
/// covered, and both branches report which happened.
///
/// Note the boundary this does *not* cross: it protects against a refresh that
/// started and failed, not against one that was never invoked. No file
/// operation can express "the command never ran"; that is what the subject
/// digest and repository SHA are for.
///
/// # Errors
///
/// Returns the tombstone-write failure, noting whether the stale document was
/// removed or is still present.
fn invalidate_before_discovery(out: &Path, tier: UxCiTier) -> Result<()> {
    let Err(write_error) = write_tombstone(out, &UxCaseInventoryInvalid::in_progress(tier)) else {
        return Ok(());
    };
    if !out.exists() {
        return Err(eyre!(
            "could not write the in-progress tombstone to `{}`: {write_error}",
            out.display()
        ));
    }
    match fs::remove_file(out) {
        Ok(()) => Err(eyre!(
            "could not write the in-progress tombstone to `{}` ({write_error}); the previous inventory was removed so it cannot be read as current",
            out.display()
        )),
        Err(remove_error) => Err(eyre!(
            "could not write the in-progress tombstone to `{}` ({write_error}) and the previous inventory could not be removed ({remove_error}); it may still be readable as current",
            out.display()
        )),
    }
}

/// Retire the in-progress tombstone for a failed discovery, preserving the cause.
///
/// The tombstone write is best effort. If it fails, the discovery failure is
/// still the error worth returning — it is the reason the run ended — and the
/// write failure is attached as secondary context rather than replacing it.
/// Returning the write error instead would hide the actual cause behind an I/O
/// message and leave the caller unable to say why discovery stopped.
fn retire_with(out: &Path, tier: UxCiTier, failure: &UxDiscoveryFailure) -> color_eyre::Report {
    match write_tombstone(out, &UxCaseInventoryInvalid::failed(tier, failure)) {
        Ok(()) => eyre!("{failure}"),
        Err(write_error) => eyre!(
            "{failure} (the failure tombstone could not be written, so `{}` may still hold an in-progress marker: {write_error})",
            out.display()
        ),
    }
}

/// Publish `inventory` to `out`, retiring the in-progress tombstone on failure.
///
/// Split out so the publication path is directly testable: a rename or render
/// failure here must leave a `discovery_failed` document rather than a stale
/// `discovery_in_progress` one.
///
/// # Errors
///
/// Returns the rendering or publication failure. The original error is
/// preserved even when the tombstone write also fails.
fn publish_or_retire(out: &Path, tier: UxCiTier, inventory: &UxCaseInventory) -> Result<()> {
    match render(inventory).and_then(|body| write_atomic(out, &body)) {
        Ok(()) => Ok(()),
        Err(error) => {
            let tombstone = UxCaseInventoryInvalid::failed(
                tier,
                &UxDiscoveryFailure::InstrumentFailure { reason: error.to_string() },
            );
            match write_tombstone(out, &tombstone) {
                Ok(()) => Err(error),
                Err(write_error) => Err(eyre!(
                    "{error} (the failure tombstone could not be written, so `{}` may still hold an in-progress marker: {write_error})",
                    out.display()
                )),
            }
        }
    }
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
    invalidate_before_discovery(out, tier)?;

    let inventory = match case_inventory::discover_cases(commands, request)
        .and_then(|inventory| inventory.verify_digest().map(|()| inventory))
    {
        Ok(inventory) => inventory,
        Err(failure) => return Err(retire_with(out, tier, &failure)),
    };

    // Rendering and publication are both fallible, and a failure in either must
    // retire the in-progress tombstone. Leaving it in place would tell a
    // consumer a refresh is still running when it has already ended.
    publish_or_retire(out, tier, &inventory)?;
    Ok(inventory)
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
    fn a_failed_discovery_retires_the_in_progress_tombstone() -> TestResult {
        let dir = tempfile::tempdir()?;
        let out = dir.path().join("ux-case-inventory.json");
        fs::write(&out, "seed")?;

        let request = UxDiscoveryRequest::new(UxCiTier::Pr, dir.path().to_path_buf());
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

    /// A minimal real inventory, produced through the ordinary discovery path.
    fn sample_inventory() -> Result<UxCaseInventory> {
        struct OneCase;
        const EXE: &str = "/w/target/debug/deps/t-1";

        impl UxDiscoveryCommands for OneCase {
            fn compile_test_targets(&self, _argv: &[String]) -> Result<String, UxDiscoveryFailure> {
                Ok(format!(
                    r#"{{"reason":"compiler-artifact","package_id":"path+file:///w/crates/perl-lsp-ux-tests#0.1.0","target":{{"kind":["test"],"name":"t","src_path":"/w/tests/t.rs"}},"profile":{{"test":true}},"features":[],"executable":"{EXE}"}}"#
                ))
            }
            fn list_cases(
                &self,
                _target: &str,
                _executable: &Path,
                _argv: &[String],
            ) -> Result<String, UxDiscoveryFailure> {
                Ok("a: test\n\n1 test, 0 benchmarks\n".to_string())
            }
            fn executable_digest(
                &self,
                _target: &str,
                _executable: &Path,
            ) -> Result<String, UxDiscoveryFailure> {
                Ok(sha256_hex(b"stable"))
            }
            fn executable_exists(&self, _executable: &Path) -> bool {
                true
            }
        }

        let request = UxDiscoveryRequest::new(UxCiTier::Pr, PathBuf::from("/w"));
        case_inventory::discover_cases(&OneCase, &request).map_err(|failure| eyre!("{failure}"))
    }

    #[test]
    fn a_failed_publication_surfaces_rather_than_reporting_success() -> TestResult {
        // Exercises the publication path itself, not the discovery path: a
        // directory sitting at the output path makes the final rename fail
        // after discovery has already succeeded.
        let dir = tempfile::tempdir()?;
        let out = dir.path().join("occupied");
        fs::create_dir(&out)?;

        let error = publish_or_retire(&out, UxCiTier::Pr, &sample_inventory()?)
            .expect_err("renaming onto a directory must fail");
        assert!(!out.is_file(), "a failed publication must not leave a document claiming success");
        assert!(!error.to_string().is_empty());
        Ok(())
    }

    #[test]
    fn a_failed_tombstone_write_does_not_mask_the_discovery_failure() -> TestResult {
        // Both the discovery failure and the tombstone write fail. The cause of
        // the run ending is the discovery failure; returning the I/O error
        // instead would leave the caller unable to say why discovery stopped.
        let dir = tempfile::tempdir()?;
        let out = dir.path().join("occupied");
        fs::create_dir(&out)?;

        let failure = UxDiscoveryFailure::NoTestArtifacts { package: "perl-lsp-ux-tests".into() };
        let report = retire_with(&out, UxCiTier::Pr, &failure);
        let rendered = report.to_string();

        assert!(
            rendered.contains("no test artifacts"),
            "the discovery failure must survive as the primary cause: {rendered}"
        );
        assert!(
            rendered.contains("in-progress marker"),
            "the tombstone write failure must be attached as context: {rendered}"
        );

        // The happy path returns the discovery failure unadorned.
        let writable = dir.path().join("ux-case-inventory.json");
        let clean = retire_with(&writable, UxCiTier::Pr, &failure).to_string();
        assert!(clean.contains("no test artifacts"));
        assert!(!clean.contains("in-progress marker"));
        Ok(())
    }

    #[test]
    fn a_failed_initial_invalidation_reports_whether_the_stale_document_survived() -> TestResult {
        // A directory sitting at the output path makes the tombstone rename fail
        // and also makes the fallback removal fail, which is the branch where the
        // previous document can still be read. The error must say so rather than
        // implying a clean invalidation.
        let dir = tempfile::tempdir()?;
        let occupied = dir.path().join("ux-case-inventory.json");
        fs::create_dir(&occupied)?;
        fs::write(occupied.join("keep"), "non-empty")?;

        let error = invalidate_before_discovery(&occupied, UxCiTier::Pr)
            .expect_err("an unwritable destination must fail closed");
        let rendered = error.to_string();
        assert!(
            rendered.contains("may still be readable as current"),
            "the error must admit the stale document survived: {rendered}"
        );

        // The ordinary path leaves a tombstone, not the previous inventory.
        let out = dir.path().join("fresh.json");
        fs::write(&out, r#"{"schema":"ux_case_inventory.v1","totals":{"case_count":349}}"#)?;
        invalidate_before_discovery(&out, UxCiTier::Pr)?;
        let after: serde_json::Value = serde_json::from_str(&fs::read_to_string(&out)?)?;
        assert_eq!(after["schema"], UX_CASE_INVENTORY_INVALID_SCHEMA);
        assert!(!fs::read_to_string(&out)?.contains("349"));
        Ok(())
    }

    #[test]
    fn a_bare_output_file_name_resolves_to_the_working_directory() {
        // `Path::parent` of a bare name is `Some("")`; `create_dir_all("")`
        // fails, so a bare `--out` used to write nothing at all.
        //
        // Proven through `output_parent` rather than by moving the process into
        // a temporary directory: `std::env::set_current_dir` is process-wide, so
        // such a test races every sibling test that reads the working directory
        // and can fail this suite nondeterministically. A lock held by one test
        // cannot fix that, because the racing readers do not take it.
        assert_eq!(output_parent(Path::new("ux-case-inventory.json")), Path::new("."));
        assert_eq!(output_parent(Path::new("receipts/ux.json")), Path::new("receipts"));
        assert_eq!(output_parent(Path::new("/tmp/receipts/ux.json")), Path::new("/tmp/receipts"));
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
    fn a_failed_listing_records_the_executable_it_invoked() -> TestResult {
        let dir = tempfile::tempdir()?;
        let missing = dir.path().join("not-a-real-test-binary");
        let commands = SystemDiscoveryCommands { workspace_root: dir.path().to_path_buf() };

        let failure = commands
            .list_cases("pkg::test::t", &missing, &["--list".to_string()])
            .expect_err("spawning a missing executable must fail");
        match failure {
            UxDiscoveryFailure::ListCommandFailed { argv, .. } => {
                assert_eq!(
                    argv.first().map(String::as_str),
                    Some(missing.to_string_lossy().as_ref()),
                    "argv must be the exact invoked command, executable first"
                );
                assert_eq!(argv.get(1).map(String::as_str), Some("--list"));
            }
            other => return Err(format!("unexpected failure: {other}").into()),
        }
        Ok(())
    }

    #[test]
    fn the_default_output_path_is_under_the_ux_receipt_root() {
        assert!(DEFAULT_OUT.starts_with("target/receipts/editor-ux/"));
        assert!(DEFAULT_OUT.ends_with(".json"));
    }
}
