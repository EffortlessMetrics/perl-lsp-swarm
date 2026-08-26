//! Timed workspace Clippy scope-cost measurement (#11736).
//!
//! Deterministic instrument that answers the cost question [#11736 decision 1]
//! before any gate-flag change: how much wall time does workspace
//! `--all-targets` Clippy cost compared to library scope, under controlled
//! cache states?
//!
//! Measured pairs, in fixed order:
//!
//! 1. `warm` / `lib`
//! 2. `warm` / `all-targets`
//! 3. `members-cold` / `lib`      (member fingerprints invalidated first)
//! 4. `members-cold` / `all-targets` (invalidated again, canonically)
//!
//! `members-cold` removes only workspace-member fingerprint directories from
//! the target dir, so dependencies stay cached while every member unit is
//! re-checked — the representative steady state for CI and for kernel-cohort
//! admission runs. The invalidation is repeated before *each* cold pass so
//! both cold measurements start from the identical canonical state instead of
//! the second one inheriting warmth produced by the first.
//!
//! Every pass records argv, wall duration, exit code, and Clippy finding
//! counts parsed from `--message-format=json` stdout. Non-zero Clippy exits
//! are recorded, not treated as instrument failure, only when they carry the
//! known lint-debt shape: current-main `--all-targets` carries known
//! deny-level tranche debt (#11736 census), so the measurement must survive
//! it to observe the cost. A non-zero exit whose stderr shows a non-lint
//! command failure (`--locked` lock-file conflicts, build-script failures,
//! manifest load failures, rustc internal compiler errors) aborts loudly
//! before any receipt is written instead of recording a partial runtime as
//! a completed measurement.
//!
//! Each warm pass is preceded by an unmeasured priming pass of the same
//! scope, so both warm timings measure steady-state re-check cost of their
//! own scope from self-consistent cache states instead of the second scope
//! inheriting compilation performed by the first.
//!
//! Failures are loud and typed: missing `cargo`/`cargo-clippy`, unusable
//! `git`, failed `cargo metadata`, spawn errors, and watchdog timeouts all
//! abort with an error before any receipt is written.
//!
//! Receipt: `target/receipts/clippy-cost-measurement.json` (override with
//! `--receipt`), raw per-pass streams under `target/receipts/logs/clippy-cost/`.

use chrono::Utc;
use clap::ValueEnum;
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::utils::project_root;

/// Target-kind scopes the instrument can time.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ClippyScope {
    /// `cargo clippy --workspace --lib`
    Lib,
    /// `cargo clippy --workspace --all-targets --keep-going`
    AllTargets,
}

impl ClippyScope {
    pub const fn label(self) -> &'static str {
        match self {
            ClippyScope::Lib => "lib",
            ClippyScope::AllTargets => "all-targets",
        }
    }

    fn cargo_flag(self) -> &'static str {
        match self {
            ClippyScope::Lib => "--lib",
            ClippyScope::AllTargets => "--all-targets",
        }
    }
}

/// Dependency-cache states the instrument measures under.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ClippyCacheState {
    /// Run with whatever warmth the target dir already has.
    Warm,
    /// Invalidate workspace-member fingerprints first; dependencies stay cached.
    MembersCold,
}

impl ClippyCacheState {
    pub const fn label(self) -> &'static str {
        match self {
            ClippyCacheState::Warm => "warm",
            ClippyCacheState::MembersCold => "members-cold",
        }
    }
}

/// CLI arguments for `cargo xtask clippy-cost-measure`.
pub struct ClippyCostMeasureArgs {
    pub receipt: PathBuf,
    pub scopes: Vec<ClippyScope>,
    pub states: Vec<ClippyCacheState>,
    pub timeout_secs: u64,
}

/// Fixed canonical ordering: warm passes first, then each cold pass behind a
/// fresh canonical invalidation.
fn ordered_scopes(selected: &[ClippyScope]) -> Vec<ClippyScope> {
    let canonical = [ClippyScope::Lib, ClippyScope::AllTargets];
    canonical.into_iter().filter(|s| selected.contains(s)).collect()
}

fn ordered_states(selected: &[ClippyCacheState]) -> Vec<ClippyCacheState> {
    let canonical = [ClippyCacheState::Warm, ClippyCacheState::MembersCold];
    canonical.into_iter().filter(|s| selected.contains(s)).collect()
}

// ---------------------------------------------------------------------------
// Receipt model
// ---------------------------------------------------------------------------
// Receipt model
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ClippyCostReceipt {
    schema_version: u32,
    issue: &'static str,
    generated_at: String,
    git_head: String,
    toolchain: ToolchainInfo,
    host: HostInfo,
    workspace_members: Vec<String>,
    runs: Vec<RunReceipt>,
    summary: Vec<StateSummaryRow>,
    notes: Vec<&'static str>,
}

#[derive(Serialize)]
struct ToolchainInfo {
    cargo_clippy_version: String,
}

#[derive(Serialize)]
struct HostInfo {
    os: &'static str,
    arch: &'static str,
    family: &'static str,
    logical_cpus: Option<u32>,
}

#[derive(Serialize)]
struct RunReceipt {
    label: String,
    scope: &'static str,
    cache_state: &'static str,
    argv: Vec<String>,
    started_at: String,
    finished_at: String,
    duration_ms: u128,
    exit_code: i32,
    invalidated_fingerprint_dirs: Option<u32>,
    counts: MessageCounts,
    stdout_log: String,
    stderr_log: String,
}

#[derive(Serialize, Default)]
struct MessageCounts {
    compiler_messages: u64,
    clippy_findings_total: u64,
    unparsed_lines: u64,
    by_lint: BTreeMap<String, u64>,
}

#[derive(Serialize)]
struct StateSummaryRow {
    cache_state: &'static str,
    lib_duration_ms: Option<u128>,
    all_targets_duration_ms: Option<u128>,
    all_targets_over_lib: Option<f64>,
}

fn duration_for(runs: &[RunReceipt], state: &str, scope: &str) -> Option<u128> {
    runs.iter().find(|r| r.cache_state == state && r.scope == scope).map(|r| r.duration_ms)
}

fn build_summary(runs: &[RunReceipt], states: &[ClippyCacheState]) -> Vec<StateSummaryRow> {
    ordered_states(states)
        .into_iter()
        .map(|state| {
            let lib = duration_for(runs, state.label(), "lib");
            let all = duration_for(runs, state.label(), "all-targets");
            let ratio = match (lib, all) {
                (Some(l), Some(a)) if l > 0 => Some((a as f64 / l as f64 * 100.0).round() / 100.0),
                _ => None,
            };
            StateSummaryRow {
                cache_state: state.label(),
                lib_duration_ms: lib,
                all_targets_duration_ms: all,
                all_targets_over_lib: ratio,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Environment discovery (loud failures)
// ---------------------------------------------------------------------------

fn cargo_program() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn probe_toolchain(cargo: &str) -> Result<ToolchainInfo> {
    let output = Command::new(cargo).args(["clippy", "--version"]).output().map_err(|e| {
        eyre!("cannot execute '{cargo} clippy --version': {e} — is cargo installed and on PATH?")
    })?;
    if !output.status.success() {
        bail!(
            "'{cargo} clippy --version' exited with {} — the clippy driver is unavailable",
            output.status.code().unwrap_or(-1)
        );
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        bail!("'{cargo} clippy --version' produced no version text");
    }
    Ok(ToolchainInfo { cargo_clippy_version: text })
}

fn git_head(root: &Path) -> Result<String> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(root)
        .output()
        .map_err(|e| eyre!("cannot execute 'git rev-parse HEAD': {e}"))?;
    if !output.status.success() {
        bail!("'git rev-parse HEAD' failed:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if head.is_empty() {
        bail!("'git rev-parse HEAD' returned empty output");
    }
    Ok(head)
}

fn workspace_members_and_target(root: &Path) -> Result<(Vec<String>, PathBuf)> {
    let bytes = crate::utils::run_cargo_metadata(true)
        .with_context(|| format!("cargo metadata failed under {}", root.display()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| eyre!("cargo metadata JSON parse failed: {e}"))?;
    let mut members: Vec<String> = value["packages"]
        .as_array()
        .ok_or_else(|| eyre!("cargo metadata missing 'packages' array"))?
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_string))
        .collect();
    members.sort();
    if members.is_empty() {
        bail!("cargo metadata reported zero workspace members");
    }
    let target_dir = value["target_directory"]
        .as_str()
        .ok_or_else(|| eyre!("cargo metadata missing 'target_directory'"))?;
    Ok((members, PathBuf::from(target_dir)))
}

// ---------------------------------------------------------------------------
// Member fingerprint invalidation
// ---------------------------------------------------------------------------

/// True when `dir_name` is a fingerprint directory belonging to `member`:
/// cargo always writes these as `{package}-{hash}`, so the match requires the
/// `-` separator plus an alphanumeric suffix of at least eight characters
/// (observed hashes are 16 lowercase hex digits). The suffix shape is what
/// keeps sibling member names distinct — `perl-core-harness-types-<hash>`
/// must never be claimed by prefix `perl-core-harness`.
fn is_member_fingerprint_dir(dir_name: &str, member: &str) -> bool {
    match dir_name.strip_prefix(member).and_then(|rest| rest.strip_prefix('-')) {
        Some(hash) => {
            hash.len() >= 8 && !hash.is_empty() && hash.chars().all(|c| c.is_ascii_alphanumeric())
        }
        None => false,
    }
}

/// All fingerprint roots that can hold relevant member units: the base
/// `target/debug/.fingerprint` plus one root per platform subdirectory
/// (`target/<triple>/debug/.fingerprint`). Cargo writes cross-compilation
/// artifacts under `CARGO_BUILD_TARGET` / `[build] target` triples while
/// `cargo metadata` still reports the base target directory, so invalidating
/// only the base would miss every configured-triple unit and silently label
/// a warm run `members-cold`. A directory qualifies as a platform
/// subdirectory structurally — an immediate child of the target dir holding
/// its own `debug/.fingerprint` — which covers env-var and config-file
/// configuration without parsing cargo configuration.
fn fingerprint_roots(target_dir: &Path) -> Vec<PathBuf> {
    let mut roots = vec![target_dir.join("debug").join(".fingerprint")];
    let mut platform_dirs: Vec<PathBuf> = match fs::read_dir(target_dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.is_dir() && path.join("debug").join(".fingerprint").is_dir())
            .collect(),
        Err(_) => Vec::new(),
    };
    platform_dirs.sort();
    roots.extend(platform_dirs.into_iter().map(|dir| dir.join("debug").join(".fingerprint")));
    roots
}

fn invalidate_member_fingerprints(target_dir: &Path, members: &[String]) -> Result<u32> {
    let mut removed = 0u32;
    for fp_dir in fingerprint_roots(target_dir) {
        let entries = match fs::read_dir(&fp_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| format!("reading {}", fp_dir.display())),
        };
        for entry in entries {
            let entry = entry.with_context(|| format!("reading entry in {}", fp_dir.display()))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let owned = members.iter().any(|m| is_member_fingerprint_dir(&name, m));
            if owned {
                fs::remove_dir_all(entry.path()).with_context(|| {
                    format!("removing fingerprint directory {}", entry.path().display())
                })?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

// ---------------------------------------------------------------------------
// Timed command execution
// ---------------------------------------------------------------------------

fn clippy_argv(scope: ClippyScope) -> Vec<String> {
    let mut argv = vec![
        "clippy".to_string(),
        "--workspace".to_string(),
        scope.cargo_flag().to_string(),
        "--locked".to_string(),
    ];
    if scope == ClippyScope::AllTargets {
        // Census-method requirement (#11736): without keep-going, the first
        // failing crate masks downstream findings and corrupts the timing of
        // everything behind it.
        argv.push("--keep-going".to_string());
    }
    argv.push("--message-format=json".to_string());
    argv
}

/// Spawn `program` with piped-to-file stdout/stderr, enforce a watchdog, and
/// return the exit code. Spawn failures and timeouts are typed errors.
fn run_with_watchdog(
    program: &str,
    args: &[String],
    current_dir: &Path,
    stdout_log: &Path,
    stderr_log: &Path,
    timeout: Duration,
) -> Result<i32> {
    let stdout_file = fs::File::create(stdout_log)
        .with_context(|| format!("creating {}", stdout_log.display()))?;
    let stderr_file = fs::File::create(stderr_log)
        .with_context(|| format!("creating {}", stderr_log.display()))?;

    let mut child: Child = Command::new(program)
        .args(args)
        .current_dir(current_dir)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .map_err(|e| eyre!("failed to spawn '{program}' (missing binary?): {e}"))?;

    let started = Instant::now();
    loop {
        match child.try_wait().with_context(|| format!("waiting on '{program}'"))? {
            Some(status) => return Ok(status.code().unwrap_or(-1)),
            None => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("'{program}' exceeded the {:?} watchdog and was killed", timeout);
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Finding-count parsing
// ---------------------------------------------------------------------------

/// Classification of one stdout line from a `--message-format=json` run.
enum JsonLine {
    CompilerMessage { code: Option<String> },
    Ignored,
    NonJson,
}

fn classify_line(line: &str) -> JsonLine {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return JsonLine::Ignored;
    }
    let value: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return JsonLine::NonJson,
    };
    if value["reason"].as_str() != Some("compiler-message") {
        return JsonLine::Ignored;
    }
    JsonLine::CompilerMessage {
        code: value["message"]["code"]["code"].as_str().map(str::to_string),
    }
}

fn fold_counts(stdout_jsonl: &str) -> MessageCounts {
    let mut counts = MessageCounts::default();
    for line in stdout_jsonl.lines() {
        match classify_line(line) {
            JsonLine::CompilerMessage { code } => {
                counts.compiler_messages += 1;
                // Only `clippy::`-coded diagnostics are Clippy findings.
                // Plain rustc lint codes (`unused_variables`) and compiler
                // error codes would otherwise mislabel the observed
                // diagnostic population; the broader compiler-message count
                // above retains them separately.
                if let Some(clippy_code) = code.filter(|c| c.starts_with("clippy::")) {
                    *counts.by_lint.entry(clippy_code).or_insert(0) += 1;
                    counts.clippy_findings_total += 1;
                }
            }
            JsonLine::Ignored => {}
            JsonLine::NonJson => counts.unparsed_lines += 1,
        }
    }
    counts
}

fn read_log(path: &Path) -> Result<String> {
    let mut buf = String::new();
    fs::File::open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .read_to_string(&mut buf)
        .with_context(|| format!("reading {}", path.display()))?;
    Ok(buf)
}

// ---------------------------------------------------------------------------
// Non-lint command-failure discrimination
// ---------------------------------------------------------------------------

/// Cargo/rustc top-level failure texts that identify a non-lint command
/// failure: `--locked` lock-file conflicts, custom-build-command failures,
/// workspace manifest load failures, rustc internal compiler errors, and the
/// generic cargo orchestration form. A non-zero clippy exit carrying one of
/// these is NOT lint debt; recording it would present a partial runtime as a
/// completed measurement, so the driver aborts instead.
const HARD_FAILURE_SIGNATURES: [&str; 5] = [
    "the lock file",
    "failed to run custom build command",
    "failed to load manifest",
    "internal compiler error",
    "error: failed to",
];

/// First hard-failure signature present in `stderr_text`, if any.
fn hard_command_failure(stderr_text: &str) -> Option<&'static str> {
    HARD_FAILURE_SIGNATURES.into_iter().find(|signature| stderr_text.contains(signature))
}

/// Warm passes need an unmeasured same-scope priming pass so their timed run
/// measures steady-state re-check cost of its own scope; cold passes already
/// share one canonical starting state through fresh invalidation.
fn pass_is_primed(state: ClippyCacheState) -> bool {
    state == ClippyCacheState::Warm
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

pub fn run(args: ClippyCostMeasureArgs) -> Result<()> {
    let root = project_root()?;
    let cargo = cargo_program();

    let toolchain = probe_toolchain(&cargo)?;
    let git_head = git_head(&root)?;
    let (members, target_dir) = workspace_members_and_target(&root)?;

    let receipt_path =
        if args.receipt.is_absolute() { args.receipt.clone() } else { root.join(&args.receipt) };
    let logs_dir = root.join("target").join("receipts").join("logs").join("clippy-cost");
    fs::create_dir_all(&logs_dir).with_context(|| format!("creating {}", logs_dir.display()))?;
    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let logical_cpus = std::thread::available_parallelism().ok().map(|n| n.get() as u32);
    let timeout = Duration::from_secs(args.timeout_secs);

    let mut runs: Vec<RunReceipt> = Vec::new();
    for state in ordered_states(&args.states) {
        for scope in ordered_scopes(&args.scopes) {
            let invalidated = match state {
                ClippyCacheState::Warm => None,
                ClippyCacheState::MembersCold => {
                    let n = invalidate_member_fingerprints(&target_dir, &members)?;
                    println!(
                        "==> invalidated {n} member fingerprint director{} under {}",
                        if n == 1 { "y" } else { "ies" },
                        target_dir.join("debug").join(".fingerprint").display()
                    );
                    Some(n)
                }
            };

            let label = format!("{}/{}", state.label(), scope.label());

            // Warm passes are primed with an unmeasured run of the same
            // scope first: without it, warm/all-targets inherits compilation
            // performed by the earlier warm/lib pass and the two timings do
            // not start from comparable cache states. Cold passes already
            // share one canonical starting state via the fresh invalidation
            // above, so priming them would only double their cost.
            if pass_is_primed(state) {
                println!("==> priming [{label}] (unmeasured)");
                let prime_argv = clippy_argv(scope);
                let prime_stdout =
                    logs_dir.join(format!("{}.prime.stdout.jsonl", label.replace('/', "_")));
                let prime_stderr =
                    logs_dir.join(format!("{}.prime.stderr.log", label.replace('/', "_")));
                run_with_watchdog(
                    &cargo,
                    &prime_argv,
                    &root,
                    &prime_stdout,
                    &prime_stderr,
                    timeout,
                )
                .with_context(|| format!("priming pass [{label}] failed"))?;
            }

            println!("==> measuring [{label}]");
            let argv = clippy_argv(scope);
            let stdout_log = logs_dir.join(format!("{}.stdout.jsonl", label.replace('/', "_")));
            let stderr_log = logs_dir.join(format!("{}.stderr.log", label.replace('/', "_")));

            let started_at = Utc::now();
            let start = Instant::now();
            let exit_code =
                run_with_watchdog(&cargo, &argv, &root, &stdout_log, &stderr_log, timeout)
                    .with_context(|| format!("measurement pass [{label}] failed"))?;
            let duration_ms = start.elapsed().as_millis();
            let finished_at = Utc::now();

            if exit_code != 0 {
                let stderr_text = read_log(&stderr_log)?;
                if let Some(signature) = hard_command_failure(&stderr_text) {
                    bail!(
                        "measurement pass [{label}] exited {exit_code} but stderr reports a \
                         non-lint command failure ({signature:?}); refusing to record a partial \
                         runtime as a completed measurement — see {}",
                        stderr_log.display()
                    );
                }
            }

            let counts = fold_counts(&read_log(&stdout_log)?);
            println!(
                "    [{label}] exit={exit_code} dur={}ms compiler_msgs={} clippy_findings={}",
                duration_ms, counts.compiler_messages, counts.clippy_findings_total
            );

            runs.push(RunReceipt {
                label: label.clone(),
                scope: scope.label(),
                cache_state: state.label(),
                argv,
                started_at: started_at.to_rfc3339(),
                finished_at: finished_at.to_rfc3339(),
                duration_ms,
                exit_code,
                invalidated_fingerprint_dirs: invalidated,
                counts,
                stdout_log: stdout_log.display().to_string(),
                stderr_log: stderr_log.display().to_string(),
            });
        }
    }

    let summary = build_summary(&runs, &args.states);
    for row in &summary {
        match (row.lib_duration_ms, row.all_targets_duration_ms) {
            (Some(lib), Some(all)) => println!(
                "    [{}] lib={}ms all-targets={}ms ratio={}x",
                row.cache_state,
                lib,
                all,
                row.all_targets_over_lib.map(|r| r.to_string()).unwrap_or_else(|| "n/a".into()),
            ),
            _ => println!("    [{}] incomplete pair", row.cache_state),
        }
    }

    let receipt = ClippyCostReceipt {
        schema_version: 1,
        issue: "11736",
        generated_at: Utc::now().to_rfc3339(),
        git_head,
        toolchain,
        host: HostInfo {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            family: std::env::consts::FAMILY,
            logical_cpus,
        },
        workspace_members: members,
        runs,
        summary,
        notes: vec![
            "Instrument for the #11736 decision-1 prerequisite: time workspace clippy by target-kind scope before any gate-flag change.",
            "members-cold removes workspace-member fingerprint dirs only; dependency artifacts stay cached (base and configured-triple roots both invalidated).",
            "Each members-cold pass is preceded by a fresh invalidation so cold measurements share one canonical starting state.",
            "Each warm pass is preceded by an unmeasured same-scope priming pass so both warm timings start from comparable steady-state cache states.",
            "--keep-going is mandatory on all-targets so a failing crate cannot mask downstream timing (#11736 census method).",
            "Non-zero clippy exits are recorded as lint debt only when stderr carries no non-lint command-failure signature; lock-file conflicts, build-script failures, manifest load failures, and internal compiler errors abort without a receipt.",
            "clippy_findings_total and by_lint count only clippy::-coded diagnostics; plain rustc lint/error codes are retained separately in compiler_messages.",
            "Finding counts are contextual observability, not the census method (governed lints are not downgraded here).",
        ],
    };

    let rendered = serde_json::to_string_pretty(&receipt)
        .map_err(|e| eyre!("receipt serialization failed: {e}"))?;
    fs::write(&receipt_path, rendered.as_bytes())
        .with_context(|| format!("writing {}", receipt_path.display()))?;
    println!("receipt: {}", receipt_path.display());

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str) -> String {
        name.to_string()
    }

    #[test]
    fn fingerprint_matcher_requires_hash_suffix_and_rejects_bare_names() {
        let m = member("perl-token");
        assert!(is_member_fingerprint_dir("perl-token-1a2b3c4d", &m));
        assert!(is_member_fingerprint_dir("perl-token-deadbeefdeadbeef", &m));
        // Bare member names never appear as fingerprint directories on
        // current cargo; requiring the hash suffix is what keeps sibling
        // member claims unambiguous.
        assert!(!is_member_fingerprint_dir("perl-token", &m));
        // Too-short alnum suffixes are not cargo hashes.
        assert!(!is_member_fingerprint_dir("perl-token-types", &m));
    }

    #[test]
    fn fingerprint_matcher_rejects_sibling_member_names_and_unrelated() {
        let harness = member("perl-core-harness");
        // Sibling member whose name extends the prefix must not match.
        assert!(!is_member_fingerprint_dir("perl-core-harness-types-1a2b3c4d", &harness));
        assert!(!is_member_fingerprint_dir("perl-core-harness-types", &harness));
        assert!(!is_member_fingerprint_dir("perl-core-harness-types-abc", &harness));
        // Unrelated directories never match.
        assert!(!is_member_fingerprint_dir("serde-1a2b3c4d", &harness));
        assert!(!is_member_fingerprint_dir("", &harness));
        // Hash position must be alnum-only artifact suffixes.
        assert!(!is_member_fingerprint_dir("perl-token-with-dash", &member("perl-token")));
    }

    #[test]
    fn fingerprint_matcher_matches_each_member_independently() {
        let members = [member("perl-core-harness"), member("perl-core-harness-types")];
        assert!(is_member_fingerprint_dir("perl-core-harness-154660bf6260e38a", &members[0]));
        assert!(is_member_fingerprint_dir("perl-core-harness-types-99ff00aa11223344", &members[1]));
        // The types-sibling hash directory belongs only to the longer member.
        assert!(!is_member_fingerprint_dir(
            "perl-core-harness-types-99ff00aa11223344",
            &members[0]
        ));
        // The shorter member's hash directory must not be claimed by the
        // longer member either.
        assert!(!is_member_fingerprint_dir("perl-core-harness-154660bf6260e38a", &members[1]));
    }

    #[test]
    fn clippy_argv_matches_documented_commands() {
        assert_eq!(
            clippy_argv(ClippyScope::Lib),
            vec![
                "clippy".to_string(),
                "--workspace".to_string(),
                "--lib".to_string(),
                "--locked".to_string(),
                "--message-format=json".to_string(),
            ]
        );
        assert_eq!(
            clippy_argv(ClippyScope::AllTargets),
            vec![
                "clippy".to_string(),
                "--workspace".to_string(),
                "--all-targets".to_string(),
                "--locked".to_string(),
                "--keep-going".to_string(),
                "--message-format=json".to_string(),
            ]
        );
    }

    #[test]
    fn classifier_separates_messages_other_json_and_noise() {
        let msg_with_code = r#"{"reason":"compiler-message","message":{"level":"warning","code":{"code":"clippy::print_stdout"}}}"#;
        let msg_no_code = r#"{"reason":"compiler-message","message":{"level":"error"}}"#;
        let build_finished = r#"{"reason":"build-finished","success":false}"#;

        assert!(matches!(classify_line(msg_with_code), JsonLine::CompilerMessage { .. }));
        match classify_line(msg_with_code) {
            JsonLine::CompilerMessage { code } => {
                assert_eq!(code.as_deref(), Some("clippy::print_stdout"))
            }
            _ => {}
        }
        assert!(matches!(classify_line(msg_no_code), JsonLine::CompilerMessage { .. }));
        match classify_line(msg_no_code) {
            JsonLine::CompilerMessage { code } => assert!(code.is_none()),
            _ => {}
        }
        assert!(matches!(classify_line(build_finished), JsonLine::Ignored));
        assert!(matches!(classify_line("not json"), JsonLine::NonJson));
        assert!(matches!(classify_line(""), JsonLine::Ignored));
    }

    #[test]
    fn folder_accumulates_totals_and_per_lint_counts() {
        let input = concat!(
            "{\"reason\":\"compiler-message\",\"message\":{\"level\":\"warning\",\"code\":{\"code\":\"clippy::print_stdout\"}}}\n",
            "{\"reason\":\"compiler-message\",\"message\":{\"level\":\"warning\",\"code\":{\"code\":\"clippy::print_stdout\"}}}\n",
            "{\"reason\":\"compiler-message\",\"message\":{\"level\":\"error\",\"code\":{\"code\":\"clippy::expect_used\"}}}\n",
            "{\"reason\":\"compiler-message\",\"message\":{\"level\":\"warning\",\"code\":{\"code\":\"unused_variables\"}}}\n",
            "{\"reason\":\"compiler-message\",\"message\":{\"level\":\"error\",\"code\":{\"code\":\"E0382\"}}}\n",
            "{\"reason\":\"compiler-message\",\"message\":{\"level\":\"error\"}}\n",
            "{\"reason\":\"build-finished\",\"success\":true}\n",
            "garbage\n",
            "\n"
        );
        let counts = fold_counts(input);
        // Every compiler message is retained in the broad count...
        assert_eq!(counts.compiler_messages, 6);
        // ...but only clippy::-coded diagnostics are Clippy findings: plain
        // rustc lint codes (unused_variables) and compiler error codes
        // (E0382) must not mislabel the finding population.
        assert_eq!(counts.clippy_findings_total, 3);
        assert_eq!(counts.unparsed_lines, 1);
        assert_eq!(counts.by_lint.get("clippy::print_stdout"), Some(&2));
        assert_eq!(counts.by_lint.get("clippy::expect_used"), Some(&1));
        assert_eq!(counts.by_lint.len(), 2);
        assert!(!counts.by_lint.contains_key("unused_variables"));
        assert!(!counts.by_lint.contains_key("E0382"));
    }

    #[test]
    fn hard_failure_signatures_reject_non_lint_command_failures() {
        let lockfile_conflict = "error: the lock file E:\\repo\\Cargo.lock needs to be updated \
                                 but --locked was passed to prevent updating it";
        let build_script = "error: failed to run custom build command for `tree-sitter-perl-rs`";
        let manifest_load = "error: failed to load manifest for workspace member `xtask`";
        let ice = "error: internal compiler error: compiler/rustc_middle/src/thir.rs:99: not yet implemented";
        assert_eq!(hard_command_failure(lockfile_conflict), Some("the lock file"));
        assert_eq!(hard_command_failure(build_script), Some("failed to run custom build command"));
        assert_eq!(hard_command_failure(manifest_load), Some("failed to load manifest"));
        assert_eq!(hard_command_failure(ice), Some("internal compiler error"));

        // Deny-level lint debt keeps its recorded-not-fatal treatment.
        let lint_debt = concat!(
            "error: use of `expect` is not allowed\n",
            "  --> xtask\\src\\main.rs:12:10\n",
            "error: could not compile `perl-lsp-rs-core` (lib) due to 5 previous errors\n"
        );
        assert_eq!(hard_command_failure(lint_debt), None);
        assert_eq!(hard_command_failure(""), None);
    }

    #[test]
    fn only_warm_passes_are_primed() {
        assert!(pass_is_primed(ClippyCacheState::Warm));
        assert!(!pass_is_primed(ClippyCacheState::MembersCold));
    }

    #[test]
    fn summary_computes_ratio_and_guards_zero() {
        let mk_run = |scope: &'static str, ms: u128| RunReceipt {
            label: format!("x/{scope}"),
            scope,
            cache_state: "warm",
            argv: vec![],
            started_at: String::new(),
            finished_at: String::new(),
            duration_ms: ms,
            exit_code: 0,
            invalidated_fingerprint_dirs: None,
            counts: MessageCounts::default(),
            stdout_log: String::new(),
            stderr_log: String::new(),
        };
        let runs = vec![mk_run("lib", 100), mk_run("all-targets", 350)];
        let summary = build_summary(&runs, &[ClippyCacheState::Warm]);
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0].lib_duration_ms, Some(100));
        assert_eq!(summary[0].all_targets_duration_ms, Some(350));
        assert_eq!(summary[0].all_targets_over_lib, Some(3.5));

        // Zero-duration lib must yield no ratio rather than inf/NaN.
        let degenerate = vec![mk_run("lib", 0), mk_run("all-targets", 10)];
        let degenerate_summary = build_summary(&degenerate, &[ClippyCacheState::Warm]);
        assert_eq!(degenerate_summary[0].all_targets_over_lib, None);

        // Missing half of the pair yields nulls, not fabricated numbers.
        let half = vec![mk_run("lib", 50)];
        let half_summary = build_summary(&half, &[ClippyCacheState::Warm]);
        assert_eq!(half_summary[0].all_targets_duration_ms, None);
        assert_eq!(half_summary[0].all_targets_over_lib, None);
    }

    #[test]
    fn ordering_is_canonical_regardless_of_selection_order() {
        let ordered = ordered_scopes(&[ClippyScope::AllTargets, ClippyScope::Lib]);
        assert_eq!(ordered, vec![ClippyScope::Lib, ClippyScope::AllTargets]);

        // A subset selection keeps only canonical order and drops the rest.
        let subset = ordered_states(&[ClippyCacheState::MembersCold, ClippyCacheState::Warm]);
        assert_eq!(subset, vec![ClippyCacheState::Warm, ClippyCacheState::MembersCold]);
        let single = ordered_scopes(&[ClippyScope::AllTargets]);
        assert_eq!(single, vec![ClippyScope::AllTargets]);
    }

    #[test]
    fn missing_binary_fails_loudly_not_silently() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let stdout_log = dir.path().join("out.jsonl");
        let stderr_log = dir.path().join("err.log");
        let result = run_with_watchdog(
            "plsw-definitely-not-a-real-binary-9f3a7c",
            &[],
            dir.path(),
            &stdout_log,
            &stderr_log,
            Duration::from_secs(5),
        );
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn watchdog_kills_hanging_process_and_errors() -> Result<()> {
        // `ping -n 60` blocks for ~60s wall time regardless of stdin state
        // (unlike `cmd /C pause`, which returns immediately under a closed
        // test-harness stdin); the watchdog must convert the hang into a
        // typed error well before the bound elapses.
        if std::env::consts::OS != "windows" {
            return Ok(());
        }
        let dir = tempfile::tempdir()?;
        let result = run_with_watchdog(
            "ping",
            &["-n".to_string(), "60".to_string(), "127.0.0.1".to_string()],
            dir.path(),
            &dir.path().join("o"),
            &dir.path().join("e"),
            Duration::from_millis(500),
        );
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn receipt_serialization_carries_core_fields() -> Result<()> {
        let receipt = ClippyCostReceipt {
            schema_version: 1,
            issue: "11736",
            generated_at: "2026-08-24T00:00:00Z".to_string(),
            git_head: "abc123".to_string(),
            toolchain: ToolchainInfo { cargo_clippy_version: "clippy 0.1.0".to_string() },
            host: HostInfo {
                os: "windows",
                arch: "x86_64",
                family: "windows",
                logical_cpus: Some(8),
            },
            workspace_members: vec!["xtask".to_string()],
            runs: vec![],
            summary: vec![],
            notes: vec!["note"],
        };
        let rendered = serde_json::to_string_pretty(&receipt)?;
        let value: serde_json::Value = serde_json::from_str(&rendered)?;
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["issue"], "11736");
        assert_eq!(value["host"]["logical_cpus"], 8);
        assert_eq!(value["notes"][0], "note");
        assert_eq!(value["runs"].as_array().map(Vec::len), Some(0));
        Ok(())
    }

    #[test]
    fn invalidation_removes_only_owned_directories() -> Result<()> {
        let target = tempfile::tempdir()?;
        let fp = target.path().join("debug").join(".fingerprint");
        for name in [
            "perl-token-1a2b3c4d",
            "perl-token-deadbeefdeadbeef",
            "perl-core-harness-types-99ff00aa",
            "serde-1a2b3c4d",
        ] {
            fs::create_dir_all(fp.join(name))?;
        }
        let members = [member("perl-token")];
        let removed = invalidate_member_fingerprints(target.path(), &members)?;
        assert_eq!(removed, 2);
        assert!(!fp.join("perl-token-1a2b3c4d").exists());
        assert!(!fp.join("perl-token-deadbeefdeadbeef").exists());
        assert!(fp.join("perl-core-harness-types-99ff00aa").exists());
        assert!(fp.join("serde-1a2b3c4d").exists());
        Ok(())
    }

    #[test]
    fn invalidation_handles_missing_fingerprint_root() -> Result<()> {
        let target = tempfile::tempdir()?;
        let members = [member("perl-token")];
        let removed = invalidate_member_fingerprints(target.path(), &members)?;
        assert_eq!(removed, 0);
        Ok(())
    }

    #[test]
    fn invalidation_covers_configured_triple_roots() -> Result<()> {
        let target = tempfile::tempdir()?;
        let base_fp = target.path().join("debug").join(".fingerprint");
        let triple_fp =
            target.path().join("x86_64-unknown-linux-gnu").join("debug").join(".fingerprint");
        for root in [&base_fp, &triple_fp] {
            fs::create_dir_all(root)?;
        }
        // Member units under both the base and the configured-triple roots.
        fs::create_dir_all(base_fp.join("perl-token-1a2b3c4d"))?;
        fs::create_dir_all(triple_fp.join("perl-token-99ff00aa11223344"))?;
        // A dependency unit under the triple must survive.
        fs::create_dir_all(triple_fp.join("serde-deadbeefdeadbeef"))?;

        let members = [member("perl-token")];
        let removed = invalidate_member_fingerprints(target.path(), &members)?;
        assert_eq!(removed, 2);
        assert!(!base_fp.join("perl-token-1a2b3c4d").exists());
        assert!(!triple_fp.join("perl-token-99ff00aa11223344").exists());
        assert!(triple_fp.join("serde-deadbeefdeadbeef").exists());
        Ok(())
    }

    #[test]
    fn fingerprint_roots_list_base_then_structural_platform_dirs() -> Result<()> {
        let target = tempfile::tempdir()?;
        // Without any platform subdirectory there is exactly the base root.
        assert_eq!(
            fingerprint_roots(target.path()),
            vec![target.path().join("debug").join(".fingerprint")]
        );

        // Structural qualification: an immediate child holding its own
        // debug/.fingerprint counts as a platform dir; plain dirs do not.
        let linux = target.path().join("aarch64-apple-darwin");
        fs::create_dir_all(linux.join("debug").join(".fingerprint"))?;
        fs::create_dir_all(target.path().join("not-a-triple"))?;
        let mut roots = fingerprint_roots(target.path());
        roots.sort();
        let mut expected = vec![
            target.path().join("debug").join(".fingerprint"),
            linux.join("debug").join(".fingerprint"),
        ];
        expected.sort();
        assert_eq!(roots, expected);
        Ok(())
    }
}
