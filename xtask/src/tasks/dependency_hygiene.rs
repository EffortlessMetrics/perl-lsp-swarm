//! Dependency hygiene analysis for the perl-lsp workspace.
//!
//! Authority: issue #9364.
//!
//! Separates Cargo dependency-unused analysis from Rust source/item liveness.
//! Uses **cargo-machete** as the V1 primary instrument. cargo-udeps is removed
//! from the active hygiene path per #9364; a future PR may re-add it as a
//! scheduled advisory instrument with a discriminating fixture.
//!
//! ## Outcome vocabulary
//! - `SUCCESS`        — instrument ran cleanly, no findings
//! - `POLICY_FINDING` — one or more unused dependencies detected
//! - `NOT_PROVEN`     — instrument absent, crashed, or produced unreadable output
//! - `NOT_APPLICABLE` — reserved: scope selector excluded all targets
//!
//! ## Failure contract
//! A missing executable, unexpected exit code, or unreadable/malformed output
//! yields `NOT_PROVEN` — never zero findings. `check` and `report` never
//! install tools as a side effect.
//!
//! ## Accepted output format
//! The active command uses cargo-machete's structured `--json` output. The
//! parser accepts the documented `{}` clean result and the documented
//! `crates` result shape only; all other successful output is `NOT_PROVEN`.

use std::fs;
use std::path::Path;
use std::process::Command;

use chrono::Utc;
use color_eyre::eyre::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};

// ─── Constants ────────────────────────────────────────────────────────────────

const SCHEMA_VERSION: u32 = 1;
const INSTRUMENT_NAME: &str = "cargo-machete";
const FINDING_CODE_UNUSED_DEP: &str = "UNUSED_DEP";
const COMMAND_IDENTITY: &str = "cargo machete --json --skip-target-dir";
const OUTPUT_SUBDIR: &str = "target/dependency-hygiene";
const NATIVE_OUTPUT_FILENAME: &str = "machete-output.txt";
const REPORT_FILENAME: &str = "report.json";

// ─── Public types ─────────────────────────────────────────────────────────────

/// Subcommands for `cargo xtask dependency-hygiene`.
#[derive(Clone, Debug, clap::ValueEnum)]
pub enum DependencyHygieneMode {
    /// Fail closed on any policy finding (default).
    Check,
    /// Write a machine-readable JSON report; always exits 0.
    Report,
}

/// Typed outcome for a dependency hygiene run.
///
/// Serializes as the repository contract vocabulary:
/// `SUCCESS` | `POLICY_FINDING` | `NOT_PROVEN` | `NOT_APPLICABLE`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DependencyHygieneOutcome {
    /// Instrument ran cleanly; no findings.
    Success,
    /// Instrument ran cleanly; one or more unused dependencies found.
    PolicyFinding,
    /// Instrument unavailable, crashed, or produced unreadable output.
    NotProven,
    /// Reserved: scope selector excluded all targets.
    NotApplicable,
}

impl std::fmt::Display for DependencyHygieneOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success => write!(f, "SUCCESS"),
            Self::PolicyFinding => write!(f, "POLICY_FINDING"),
            Self::NotProven => write!(f, "NOT_PROVEN"),
            Self::NotApplicable => write!(f, "NOT_APPLICABLE"),
        }
    }
}

/// One item-level finding from the primary instrument.
///
/// Carries all identity fields required for de-duplication, triage, and
/// exception matching. `native_output_ref` points to the saved raw output
/// for diagnostic use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyFinding {
    /// Name of the crate whose Cargo.toml declares the dependency.
    pub crate_name: String,
    /// Path to the declaring Cargo.toml (workspace-relative when possible).
    pub manifest_path: String,
    /// Name of the unused dependency as declared in Cargo.toml.
    pub dep_name: String,
    /// Dependency section ([dependencies] / [dev-dependencies] / [build-dependencies]),
    /// when derivable from instrument output. `None` for cargo-machete JSON,
    /// which does not expose this context.
    pub dep_section: Option<String>,
    /// Instrument that produced this finding.
    pub instrument: String,
    /// Version string of the instrument, when obtainable.
    pub instrument_version: Option<String>,
    /// Exact argv used; records the reproducibility identity.
    pub command_identity: String,
    /// Normalized finding code.
    pub finding_code: String,
    /// Path to the saved native output file for human diagnosis.
    pub native_output_ref: String,
    /// Known instrument limitations affecting this finding class.
    pub limitations: Vec<String>,
}

/// Configuration for the dependency-hygiene task.
pub struct DependencyHygieneConfig {
    pub mode: DependencyHygieneMode,
}

/// Complete result of one dependency hygiene run.
#[derive(Debug, Serialize, Deserialize)]
pub struct DependencyHygieneResult {
    pub schema_version: u32,
    pub timestamp: String,
    pub outcome: DependencyHygieneOutcome,
    /// Populated when outcome is NOT_PROVEN or NOT_APPLICABLE.
    pub outcome_detail: Option<String>,
    pub findings: Vec<DependencyFinding>,
    pub instrument_version: Option<String>,
    pub native_output_ref: String,
    pub command_identity: String,
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Entry point called from `xtask` main dispatch.
pub fn run(config: DependencyHygieneConfig) -> Result<()> {
    let root = crate::utils::project_root()?;
    std::env::set_current_dir(&root).context("Failed to change to project root")?;

    println!("[INFO] cargo xtask dependency-hygiene (authority: #9364)");
    println!("[INFO] Primary instrument: {INSTRUMENT_NAME}");
    println!("[INFO] Mode: {:?}", config.mode);
    println!();

    let result = gather_findings(&root);

    println!("[INFO] Outcome: {}", result.outcome);
    if let Some(ref detail) = result.outcome_detail {
        println!("[INFO] Detail: {detail}");
    }
    println!("[INFO] Findings: {}", result.findings.len());
    println!();

    match config.mode {
        DependencyHygieneMode::Check => run_check(result),
        DependencyHygieneMode::Report => run_report(&root, result),
    }
}

// ─── Internal: gather findings ────────────────────────────────────────────────

fn gather_findings(root: &Path) -> DependencyHygieneResult {
    // 1. Probe cargo-machete; fail-closed on absence or probe error.
    let version = match probe_machete_version() {
        ProbeOutcome::Available(v) => v,
        ProbeOutcome::NotInstalled(msg) => {
            return not_proven(root, format!("cargo-machete not installed: {msg}"), None);
        }
        ProbeOutcome::ProbeFailed(msg) => {
            return not_proven(root, format!("cargo-machete probe failed: {msg}"), None);
        }
    };

    // 2. Ensure output directory exists.
    let output_dir = root.join(OUTPUT_SUBDIR);
    if let Err(e) = fs::create_dir_all(&output_dir) {
        return not_proven(
            root,
            format!("cannot create output directory {}: {e}", output_dir.display()),
            Some(version),
        );
    }

    // 3. Record the command identity.
    let command_identity = COMMAND_IDENTITY.to_string();
    let native_path = output_dir.join(NATIVE_OUTPUT_FILENAME);
    let native_ref = native_path.to_string_lossy().into_owned();

    // 4. Run cargo-machete. Never install tools as a side effect.
    let output = match Command::new("cargo")
        .args(["machete", "--json", "--skip-target-dir"])
        .current_dir(root)
        .output()
    {
        Err(e) => {
            return not_proven(
                root,
                format!("failed to execute `{command_identity}`: {e}"),
                Some(version),
            );
        }
        Ok(o) => o,
    };

    // 5. Save native output unconditionally for diagnosis.
    let combined = format!(
        "--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    if let Err(e) = fs::write(&native_path, &combined) {
        return not_proven(
            root,
            format!("cannot write native output to {}: {e}", native_path.display()),
            Some(version),
        );
    }

    // 6. Validate exit code.
    //    cargo-machete contract: 0 = clean, 1 = found unused deps, other = error.
    let exit_code = output.status.code();
    match exit_code {
        Some(0) | Some(1) => {} // expected exit codes
        other => {
            return DependencyHygieneResult {
                schema_version: SCHEMA_VERSION,
                timestamp: Utc::now().to_rfc3339(),
                outcome: DependencyHygieneOutcome::NotProven,
                outcome_detail: Some(format!(
                    "cargo-machete exited with unexpected code {other:?}; \
                     native output saved to {native_ref}"
                )),
                findings: vec![],
                instrument_version: Some(version),
                native_output_ref: native_ref,
                command_identity,
            };
        }
    }

    // 7. Parse structured stdout and require an output shape that matches the
    // selected cargo-machete contract. Stderr is retained above but is not
    // parsed as data because diagnostics are not the JSON result.
    let findings = match classify_machete_output(
        exit_code,
        &String::from_utf8_lossy(&output.stdout),
        &native_ref,
        &version,
        &command_identity,
    ) {
        Ok(f) => f,
        Err(parse_err) => {
            return DependencyHygieneResult {
                schema_version: SCHEMA_VERSION,
                timestamp: Utc::now().to_rfc3339(),
                outcome: DependencyHygieneOutcome::NotProven,
                outcome_detail: Some(format!(
                    "failed to parse cargo-machete output: {parse_err}; \
                 native output saved to {native_ref}"
                )),
                findings: vec![],
                instrument_version: Some(version),
                native_output_ref: native_ref,
                command_identity,
            };
        }
    };

    // 8. Classify only after the parser has established a recognized result.
    if exit_code == Some(1) && findings.is_empty() {
        return DependencyHygieneResult {
            schema_version: SCHEMA_VERSION,
            timestamp: Utc::now().to_rfc3339(),
            outcome: DependencyHygieneOutcome::NotProven,
            outcome_detail: Some(format!(
                "cargo-machete exited 1 but no findings were parsed; \
                 native output saved to {native_ref}"
            )),
            findings: vec![],
            instrument_version: Some(version),
            native_output_ref: native_ref,
            command_identity,
        };
    }

    let outcome = if findings.is_empty() {
        DependencyHygieneOutcome::Success
    } else {
        DependencyHygieneOutcome::PolicyFinding
    };

    DependencyHygieneResult {
        schema_version: SCHEMA_VERSION,
        timestamp: Utc::now().to_rfc3339(),
        outcome,
        outcome_detail: None,
        findings,
        instrument_version: Some(version),
        native_output_ref: native_ref,
        command_identity,
    }
}

/// Construct a NOT_PROVEN result with a reason detail string.
///
/// `native_output_ref` points to the canonical path even if the file may not
/// have been written (e.g., tool was never invoked).
fn not_proven(root: &Path, reason: String, version: Option<String>) -> DependencyHygieneResult {
    DependencyHygieneResult {
        schema_version: SCHEMA_VERSION,
        timestamp: Utc::now().to_rfc3339(),
        outcome: DependencyHygieneOutcome::NotProven,
        outcome_detail: Some(reason),
        findings: vec![],
        instrument_version: version,
        native_output_ref: root
            .join(OUTPUT_SUBDIR)
            .join(NATIVE_OUTPUT_FILENAME)
            .to_string_lossy()
            .into_owned(),
        command_identity: COMMAND_IDENTITY.to_string(),
    }
}

// ─── Mode dispatch ────────────────────────────────────────────────────────────

fn run_check(result: DependencyHygieneResult) -> Result<()> {
    for finding in &result.findings {
        println!(
            "[FINDING] {}: unused dependency `{}` in {}",
            finding.crate_name, finding.dep_name, finding.manifest_path,
        );
    }

    match result.outcome {
        DependencyHygieneOutcome::Success => {
            println!("[SUCCESS] dependency-hygiene: no unused dependencies found");
            Ok(())
        }
        DependencyHygieneOutcome::PolicyFinding => {
            println!(
                "[FAIL] dependency-hygiene: {} unused dependency finding(s); \
                 fix or add a scoped exception with owner and reason",
                result.findings.len()
            );
            bail!("dependency-hygiene: POLICY_FINDING")
        }
        DependencyHygieneOutcome::NotProven => {
            let detail = result.outcome_detail.as_deref().unwrap_or("(no detail)");
            bail!("dependency-hygiene: NOT_PROVEN — {detail}")
        }
        DependencyHygieneOutcome::NotApplicable => {
            println!("[INFO] dependency-hygiene: NOT_APPLICABLE — no targets in scope");
            Ok(())
        }
    }
}

fn run_report(root: &Path, result: DependencyHygieneResult) -> Result<()> {
    let output_dir = root.join(OUTPUT_SUBDIR);
    fs::create_dir_all(&output_dir).context("Failed to create output directory for report")?;

    let report_path = output_dir.join(REPORT_FILENAME);
    let json =
        serde_json::to_string_pretty(&result).context("Failed to serialize hygiene result")?;
    fs::write(&report_path, &json)
        .with_context(|| format!("Failed to write report to {}", report_path.display()))?;

    println!("[INFO] Report: {}", report_path.display());
    println!("{json}");
    Ok(())
}

// ─── cargo-machete probe ──────────────────────────────────────────────────────

/// Result of probing for cargo-machete availability.
#[derive(Debug, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// cargo-machete is available; version string captured.
    Available(String),
    /// cargo is on PATH but the machete subcommand is not registered.
    NotInstalled(String),
    /// Probe command could not be executed (spawn failure or unexpected error).
    ProbeFailed(String),
}

/// Probe for cargo-machete by running `cargo machete --version`.
///
/// Captures the version string from stdout on success.
pub fn probe_machete_version() -> ProbeOutcome {
    probe_machete_with_cargo("cargo")
}

/// Probe using an explicit cargo binary path.
///
/// Public for unit testing with an alternative (or non-existent) binary.
pub fn probe_machete_with_cargo(cargo: &str) -> ProbeOutcome {
    let output = match Command::new(cargo).args(["machete", "--version"]).output() {
        Err(e) => return ProbeOutcome::ProbeFailed(format!("spawn of `{cargo}` failed: {e}")),
        Ok(o) => o,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    classify_probe_output(output.status.code(), stdout.trim(), stderr.trim())
}

/// Classify a cargo probe exit code + captured output into a `ProbeOutcome`.
///
/// Public for unit testing with crafted inputs.
pub fn classify_probe_output(exit_code: Option<i32>, stdout: &str, stderr: &str) -> ProbeOutcome {
    if exit_code == Some(0) {
        let version = if stdout.is_empty() { "unknown".to_string() } else { stdout.to_string() };
        return ProbeOutcome::Available(version);
    }

    // cargo returns exit 101 with "no such command" on missing subcommands.
    if stderr.contains("no such command")
        || stderr.contains("no such subcommand")
        || stderr.contains("Unknown command")
    {
        return ProbeOutcome::NotInstalled(
            "install with: cargo install cargo-machete --locked".to_string(),
        );
    }

    ProbeOutcome::ProbeFailed(stderr.to_string())
}

// ─── Structured cargo-machete output ─────────────────────────────────────────

/// One crate entry from cargo-machete's documented JSON output.
#[derive(Debug, Deserialize)]
struct MacheteJsonCrate {
    package_name: String,
    manifest_path: String,
    #[serde(default)]
    unused: Vec<String>,
    #[serde(default)]
    ignored_used: Vec<String>,
}

/// Parse and classify cargo-machete's documented JSON result.
///
/// The current tool emits `{}` for a clean run and a `crates` array when it
/// has results. `ignored_used` is accepted and deliberately does not create a
/// finding: it is tool-native suppression metadata, not an unused dependency.
/// The parser rejects every other top-level shape so an exit-0 diagnostic or
/// future incompatible format cannot become `SUCCESS` with zero findings.
pub fn classify_machete_output(
    exit_code: Option<i32>,
    stdout: &str,
    native_ref: &str,
    instrument_version: &str,
    command_identity: &str,
) -> Result<Vec<DependencyFinding>, String> {
    let value: serde_json::Value = serde_json::from_str(stdout)
        .map_err(|error| format!("cargo-machete JSON is malformed: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "cargo-machete JSON result is not an object".to_string())?;

    if object.is_empty() {
        return Ok(Vec::new());
    }

    if object.len() != 1 || !object.contains_key("crates") {
        return Err("cargo-machete JSON result must be {} or contain only crates".to_string());
    }

    let crates_value = object
        .get("crates")
        .ok_or_else(|| "cargo-machete JSON result is missing crates".to_string())?;
    if !crates_value.is_array() {
        return Err("cargo-machete JSON crates must be an array".to_string());
    }

    let crates: Vec<MacheteJsonCrate> = serde_json::from_value(crates_value.clone())
        .map_err(|error| format!("cargo-machete JSON crate entry is malformed: {error}"))?;

    let mut findings = Vec::new();
    for crate_result in crates {
        if crate_result.package_name.trim().is_empty() {
            return Err("cargo-machete JSON package_name must not be empty".to_string());
        }
        if crate_result.manifest_path.trim().is_empty() {
            return Err("cargo-machete JSON manifest_path must not be empty".to_string());
        }
        for dependency in crate_result.ignored_used.iter().chain(&crate_result.unused) {
            if !is_valid_dep_name(dependency) {
                return Err(format!(
                    "cargo-machete JSON contains invalid dependency name `{dependency}`"
                ));
            }
        }

        for dependency in crate_result.unused {
            findings.push(DependencyFinding {
                crate_name: crate_result.package_name.clone(),
                manifest_path: crate_result.manifest_path.clone(),
                dep_name: dependency,
                dep_section: None,
                instrument: INSTRUMENT_NAME.to_string(),
                instrument_version: Some(instrument_version.to_string()),
                command_identity: command_identity.to_string(),
                finding_code: FINDING_CODE_UNUSED_DEP.to_string(),
                native_output_ref: native_ref.to_string(),
                limitations: standard_limitations(),
            });
        }
    }

    if exit_code == Some(1) && findings.is_empty() {
        return Err("cargo-machete exited 1 without unused dependencies".to_string());
    }

    Ok(findings)
}

// ─── Legacy text parser ───────────────────────────────────────────────────────

/// Parse cargo-machete human-readable text output into item-level findings.
///
/// ## Expected format
///
/// When unused dependencies are found:
/// ```text
/// Found the following unused dependencies in /abs/path/to/Cargo.toml:
/// dep_name_1
/// dep_name_2
///
/// Found the following unused dependencies in /abs/path/to/other/Cargo.toml:
/// dep_name_3
/// ```
///
/// When no unused dependencies:
/// ```text
/// No unused dependencies found! Nothing to fix.
/// ```
///
/// Returns `Err` only when the output is structurally unrecognizable in a way
/// that indicates tool malfunction rather than simply "no findings found".
///
/// Public for unit testing.
pub fn parse_machete_text(
    text: &str,
    native_ref: &str,
    instrument_version: &str,
    command_identity: &str,
) -> Result<Vec<DependencyFinding>, String> {
    let mut findings: Vec<DependencyFinding> = Vec::new();
    let mut current_manifest: Option<String> = None;

    for raw_line in text.lines() {
        let line = strip_ansi_and_emoji(raw_line).trim().to_string();

        if line.is_empty() {
            // Blank line ends the current manifest block.
            current_manifest = None;
            continue;
        }

        // Detect "Found the following unused dependencies in <path>:" header.
        if let Some(path) = extract_manifest_path_from_line(&line) {
            current_manifest = Some(path);
            continue;
        }

        // Skip known non-dep metadata lines.
        if is_non_finding_line(&line) {
            current_manifest = None;
            continue;
        }

        // If we are inside a manifest block, this line is a dependency name.
        if let Some(ref manifest) = current_manifest
            && is_valid_dep_name(&line)
        {
            findings.push(DependencyFinding {
                crate_name: extract_crate_name(manifest),
                manifest_path: manifest.clone(),
                dep_name: line.clone(),
                dep_section: None,
                instrument: INSTRUMENT_NAME.to_string(),
                instrument_version: Some(instrument_version.to_string()),
                command_identity: command_identity.to_string(),
                finding_code: FINDING_CODE_UNUSED_DEP.to_string(),
                native_output_ref: native_ref.to_string(),
                limitations: standard_limitations(),
            });
        }
    }

    Ok(findings)
}

/// Standard limitation strings for cargo-machete findings.
fn standard_limitations() -> Vec<String> {
    vec![
        "cargo-machete may flag dependencies used only in proc-macros or build scripts".to_string(),
        "cargo-machete JSON does not expose dependency section or target/feature context"
            .to_string(),
    ]
}

/// Strip ANSI escape sequences and high-Unicode codepoints from a string.
///
/// Uses a simple byte-level state machine handling the `ESC[…m` CSI family
/// (sufficient for terminal color codes). Multi-byte UTF-8 sequences
/// (emoji, etc.) are dropped to avoid polluting parsed dependency names.
///
/// Public for unit testing.
pub fn strip_ansi_and_emoji(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_esc = false;

    while i < bytes.len() {
        let b = bytes[i];
        if in_esc {
            // Escape sequence ends on an ASCII letter.
            if b.is_ascii_alphabetic() {
                in_esc = false;
            }
        } else if b == b'\x1b' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // Start of CSI escape sequence.
            in_esc = true;
            i += 1; // skip '[' on next iteration
        } else if b.is_ascii() {
            out.push(b as char);
        }
        // Non-ASCII bytes (multi-byte UTF-8 / emoji) are silently dropped.
        i += 1;
    }

    out
}

/// Extract the manifest path from a "Found the following…" header line.
///
/// Returns the path string without the trailing colon, or `None` if the
/// line does not match the expected header format.
///
/// Public for unit testing.
pub fn extract_manifest_path_from_line(line: &str) -> Option<String> {
    const PREFIX: &str = "Found the following unused dependencies in ";
    if let Some(rest) = line.strip_prefix(PREFIX) {
        let path = rest.trim_end_matches(':').trim();
        if !path.is_empty() {
            return Some(path.to_string());
        }
    }
    None
}

/// Return `true` for lines that are known cargo-machete metadata output
/// and not dependency names.
///
/// Public for unit testing.
pub fn is_non_finding_line(line: &str) -> bool {
    line.starts_with("Found the following")
        || line.starts_with("Searching")
        || line.starts_with("cargo machete")
        || line.contains("No unused dependencies")
        || line.contains("Nothing to fix")
        || line.starts_with("Skipping")
        || line.starts_with("Warning:")
        || line.starts_with("warning:")
        || line.starts_with("error:")
        || line.starts_with("note:")
}

/// Return `true` if the string looks like a valid Cargo dependency name.
///
/// Cargo dep names consist of alphanumeric characters, hyphens, and
/// underscores only.
///
/// Public for unit testing.
pub fn is_valid_dep_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

/// Extract a best-effort crate name from a Cargo.toml manifest path.
///
/// Returns the name of the directory containing the Cargo.toml, or
/// `"unknown"` if the path structure is not navigable.
///
/// Public for unit testing.
pub fn extract_crate_name(manifest_path: &str) -> String {
    Path::new(manifest_path)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const REAL_MACHETE_OUTPUT: &str =
        include_str!("../../tests/fixtures/dependency-hygiene/machete-findings.json");
    const MALFORMED_MACHETE_OUTPUT: &str =
        include_str!("../../tests/fixtures/dependency-hygiene/machete-malformed.json");

    // ── Outcome display ───────────────────────────────────────────────────────

    #[test]
    fn test_outcome_display_success() {
        assert_eq!(DependencyHygieneOutcome::Success.to_string(), "SUCCESS");
    }

    #[test]
    fn test_outcome_display_policy_finding() {
        assert_eq!(DependencyHygieneOutcome::PolicyFinding.to_string(), "POLICY_FINDING");
    }

    #[test]
    fn test_outcome_display_not_proven() {
        assert_eq!(DependencyHygieneOutcome::NotProven.to_string(), "NOT_PROVEN");
    }

    #[test]
    fn test_outcome_display_not_applicable() {
        assert_eq!(DependencyHygieneOutcome::NotApplicable.to_string(), "NOT_APPLICABLE");
    }

    // ── JSON serialization round-trip ─────────────────────────────────────────

    #[test]
    fn test_outcome_serializes_to_screaming_snake_case() {
        assert_eq!(
            serde_json::to_string(&DependencyHygieneOutcome::Success).unwrap(),
            "\"SUCCESS\""
        );
        assert_eq!(
            serde_json::to_string(&DependencyHygieneOutcome::PolicyFinding).unwrap(),
            "\"POLICY_FINDING\""
        );
        assert_eq!(
            serde_json::to_string(&DependencyHygieneOutcome::NotProven).unwrap(),
            "\"NOT_PROVEN\""
        );
        assert_eq!(
            serde_json::to_string(&DependencyHygieneOutcome::NotApplicable).unwrap(),
            "\"NOT_APPLICABLE\""
        );
    }

    #[test]
    fn test_outcome_round_trips_via_json() {
        for outcome in [
            DependencyHygieneOutcome::Success,
            DependencyHygieneOutcome::PolicyFinding,
            DependencyHygieneOutcome::NotProven,
            DependencyHygieneOutcome::NotApplicable,
        ] {
            let json = serde_json::to_string(&outcome).unwrap();
            let decoded: DependencyHygieneOutcome = serde_json::from_str(&json).unwrap();
            assert_eq!(outcome, decoded);
        }
    }

    // ── Probe classification — negative control #1: machete missing ───────────

    #[test]
    fn test_classify_probe_missing_tool_no_such_command() {
        let outcome = classify_probe_output(Some(101), "", "error: no such command: `machete`");
        assert_eq!(
            outcome,
            ProbeOutcome::NotInstalled(
                "install with: cargo install cargo-machete --locked".to_string()
            )
        );
    }

    #[test]
    fn test_classify_probe_missing_tool_no_such_subcommand() {
        let outcome = classify_probe_output(Some(101), "", "error: no such subcommand: `machete`");
        assert!(matches!(outcome, ProbeOutcome::NotInstalled(_)));
    }

    #[test]
    fn test_classify_probe_available() {
        let outcome = classify_probe_output(Some(0), "cargo-machete 0.7.0", "");
        assert_eq!(outcome, ProbeOutcome::Available("cargo-machete 0.7.0".to_string()));
    }

    #[test]
    fn test_classify_probe_available_empty_version() {
        // stdout is empty but exit 0 → version becomes "unknown"
        let outcome = classify_probe_output(Some(0), "", "");
        assert_eq!(outcome, ProbeOutcome::Available("unknown".to_string()));
    }

    #[test]
    fn test_classify_current_json_finding_fixture() {
        let findings = classify_machete_output(
            Some(1),
            REAL_MACHETE_OUTPUT,
            "target/machete-output.txt",
            "cargo-machete 0.9.2",
            COMMAND_IDENTITY,
        )
        .expect("documented cargo-machete JSON should parse");

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].crate_name, "fixture-crate");
        assert_eq!(findings[0].manifest_path, "crates/fixture-crate/Cargo.toml");
        assert_eq!(findings[0].dep_name, "unused-dependency");
        assert_eq!(findings[0].dep_section, None);
        assert_eq!(findings[0].command_identity, COMMAND_IDENTITY);
    }

    #[test]
    fn test_classify_current_json_clean_result() {
        let findings = classify_machete_output(
            Some(0),
            "{}",
            "target/machete-output.txt",
            "cargo-machete 0.9.2",
            COMMAND_IDENTITY,
        )
        .expect("documented clean JSON should parse");

        assert!(findings.is_empty());
    }

    #[test]
    fn test_malformed_exit_zero_json_is_not_proven() {
        let result = classify_machete_output(
            Some(0),
            MALFORMED_MACHETE_OUTPUT,
            "target/machete-output.txt",
            "cargo-machete 0.9.2",
            COMMAND_IDENTITY,
        );

        assert!(result.is_err(), "malformed exit-0 output must fail closed");
    }

    #[test]
    fn test_unrecognized_exit_zero_json_is_not_proven() {
        let result = classify_machete_output(
            Some(0),
            "{\"diagnostic\":\"clean\"}",
            "target/machete-output.txt",
            "cargo-machete 0.9.2",
            COMMAND_IDENTITY,
        );

        assert!(result.is_err(), "unrecognized exit-0 output must fail closed");
    }

    /// Negative control #1 (variant): probe with a non-existent binary → NOT_PROVEN path.
    #[test]
    fn test_probe_with_nonexistent_binary_is_not_available() {
        let outcome = probe_machete_with_cargo("/nonexistent/cargo-binary-that-does-not-exist");
        // spawn fails → ProbeFailed; tool can never be Available.
        assert!(
            !matches!(outcome, ProbeOutcome::Available(_)),
            "a missing binary must not resolve to Available (got {outcome:?})"
        );
    }

    // ── Probe classification — negative control #2: unexpected exit code ──────

    #[test]
    fn test_classify_probe_unexpected_exit_code_two() {
        let outcome = classify_probe_output(Some(2), "", "some unexpected error text");
        assert!(matches!(outcome, ProbeOutcome::ProbeFailed(_)));
    }

    #[test]
    fn test_classify_probe_signal_kill_no_exit_code() {
        let outcome = classify_probe_output(None, "", "killed by signal");
        assert!(matches!(outcome, ProbeOutcome::ProbeFailed(_)));
    }

    // ── Text parser — negative control #3: malformed / unrecognized output ────

    #[test]
    fn test_parse_empty_input_yields_no_findings() {
        let findings = parse_machete_text("", "/tmp/out.txt", "0.7.0", "cargo machete").unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn test_parse_garbage_yields_no_findings() {
        // Output with no recognized header lines → empty findings (not NOT_PROVEN).
        // NOT_PROVEN is only returned at the call site if exit code is inconsistent.
        let text = "!!xyzzy!!\n@@@malformed@@@\n123\n";
        let findings = parse_machete_text(text, "/tmp/out.txt", "0.7.0", "cargo machete").unwrap();
        assert!(findings.is_empty());
    }

    // ── Text parser — negative control #7: clean run ─────────────────────────

    #[test]
    fn test_parse_clean_no_findings_message() {
        let text = "No unused dependencies found! Nothing to fix.\n";
        let findings = parse_machete_text(text, "/tmp/out.txt", "0.7.0", "cargo machete").unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn test_parse_clean_variant_no_exclamation() {
        let text = "No unused dependencies found\n";
        let findings = parse_machete_text(text, "/tmp/out.txt", "0.7.0", "cargo machete").unwrap();
        assert!(findings.is_empty());
    }

    // ── Text parser — negative control #5: real finding ──────────────────────

    #[test]
    fn test_parse_single_finding() {
        let text = "Found the following unused dependencies in /workspace/crates/my-crate/Cargo.toml:\nunused_dep\n\n";
        let findings =
            parse_machete_text(text, "/tmp/machete.txt", "0.7.0", "cargo machete").unwrap();
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.dep_name, "unused_dep");
        assert_eq!(f.manifest_path, "/workspace/crates/my-crate/Cargo.toml");
        assert_eq!(f.crate_name, "my-crate");
        assert_eq!(f.finding_code, "UNUSED_DEP");
        assert_eq!(f.instrument, "cargo-machete");
        assert_eq!(f.instrument_version, Some("0.7.0".to_string()));
        assert_eq!(f.command_identity, "cargo machete");
        assert!(!f.native_output_ref.is_empty());
    }

    #[test]
    fn test_parse_multiple_findings_one_crate() {
        let text = concat!(
            "Found the following unused dependencies in /ws/crates/foo/Cargo.toml:\n",
            "bar\n",
            "baz\n",
            "\n",
        );
        let findings =
            parse_machete_text(text, "/tmp/out.txt", "0.8.0", "cargo machete --skip-target-dir")
                .unwrap();
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].dep_name, "bar");
        assert_eq!(findings[1].dep_name, "baz");
        // All findings from the same crate
        assert_eq!(findings[0].crate_name, "foo");
        assert_eq!(findings[1].crate_name, "foo");
    }

    #[test]
    fn test_parse_multiple_crates() {
        let text = concat!(
            "Found the following unused dependencies in /ws/crates/alpha/Cargo.toml:\n",
            "dep_a\n",
            "\n",
            "Found the following unused dependencies in /ws/crates/beta/Cargo.toml:\n",
            "dep_b\n",
            "dep_c\n",
            "\n",
        );
        let findings = parse_machete_text(text, "/tmp/out.txt", "0.8.0", "cargo machete").unwrap();
        assert_eq!(findings.len(), 3);
        assert_eq!(findings[0].crate_name, "alpha");
        assert_eq!(findings[0].dep_name, "dep_a");
        assert_eq!(findings[1].crate_name, "beta");
        assert_eq!(findings[1].dep_name, "dep_b");
        assert_eq!(findings[2].crate_name, "beta");
        assert_eq!(findings[2].dep_name, "dep_c");
    }

    // ── Text parser — ANSI stripping ──────────────────────────────────────────

    #[test]
    fn test_ansi_stripped_before_parse() {
        let text = "\x1b[32mFound the following unused dependencies in /ws/crates/foo/Cargo.toml:\x1b[0m\nbaz\n\n";
        let findings = parse_machete_text(text, "/tmp/out.txt", "0.7.0", "cargo machete").unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].dep_name, "baz");
    }

    #[test]
    fn test_strip_ansi_basic() {
        assert_eq!(strip_ansi_and_emoji("hello"), "hello");
        assert_eq!(strip_ansi_and_emoji("\x1b[32mgreen\x1b[0m"), "green");
        assert_eq!(strip_ansi_and_emoji("abc\x1b[1mbold\x1b[m end"), "abcbold end");
    }

    #[test]
    fn test_strip_ansi_empty() {
        assert_eq!(strip_ansi_and_emoji(""), "");
    }

    // ── Non-finding line classification ───────────────────────────────────────

    #[test]
    fn test_is_non_finding_line_cargo_machete_summary() {
        assert!(is_non_finding_line("cargo machete found unused dependencies in 2 crates."));
    }

    #[test]
    fn test_is_non_finding_line_no_unused() {
        assert!(is_non_finding_line("No unused dependencies found! Nothing to fix."));
    }

    #[test]
    fn test_is_non_finding_line_searching() {
        assert!(is_non_finding_line("Searching /path/to/workspace..."));
    }

    #[test]
    fn test_is_non_finding_line_warning() {
        assert!(is_non_finding_line("warning: some warning text"));
    }

    #[test]
    fn test_is_not_non_finding_line_valid_dep() {
        assert!(!is_non_finding_line("serde"));
        assert!(!is_non_finding_line("tokio-util"));
        assert!(!is_non_finding_line("serde_json"));
    }

    // ── Summary line after findings must not create a ghost dep ───────────────

    #[test]
    fn test_summary_line_not_parsed_as_dep() {
        let text = concat!(
            "Found the following unused dependencies in /ws/Cargo.toml:\n",
            "real_dep\n",
            "\n",
            "cargo machete found unused dependencies in 1 crate.\n",
        );
        let findings = parse_machete_text(text, "/out", "0.7.0", "cargo machete").unwrap();
        assert_eq!(findings.len(), 1, "only real_dep should be a finding");
        assert_eq!(findings[0].dep_name, "real_dep");
    }

    // ── Dependency name validation ─────────────────────────────────────────────

    #[test]
    fn test_valid_dep_names() {
        assert!(is_valid_dep_name("serde"));
        assert!(is_valid_dep_name("tokio-util"));
        assert!(is_valid_dep_name("serde_json"));
        assert!(is_valid_dep_name("my-crate-123"));
        assert!(is_valid_dep_name("Abc123"));
    }

    #[test]
    fn test_invalid_dep_names() {
        assert!(!is_valid_dep_name(""));
        assert!(!is_valid_dep_name("has space"));
        assert!(!is_valid_dep_name("has/slash"));
        assert!(!is_valid_dep_name("has.dot"));
        assert!(!is_valid_dep_name("has@at"));
    }

    // ── Manifest path extraction ───────────────────────────────────────────────

    #[test]
    fn test_extract_manifest_path_valid() {
        let line = "Found the following unused dependencies in /workspace/crates/foo/Cargo.toml:";
        assert_eq!(
            extract_manifest_path_from_line(line),
            Some("/workspace/crates/foo/Cargo.toml".to_string())
        );
    }

    #[test]
    fn test_extract_manifest_path_no_match() {
        assert_eq!(extract_manifest_path_from_line("some random line"), None);
        assert_eq!(extract_manifest_path_from_line(""), None);
        assert_eq!(extract_manifest_path_from_line("warning: something"), None);
    }

    #[test]
    fn test_extract_manifest_path_no_trailing_colon() {
        // Path without colon should still parse (format flexibility).
        let line = "Found the following unused dependencies in /path/Cargo.toml";
        assert_eq!(extract_manifest_path_from_line(line), Some("/path/Cargo.toml".to_string()));
    }

    // ── Crate name extraction ─────────────────────────────────────────────────

    #[test]
    fn test_extract_crate_name_normal() {
        assert_eq!(extract_crate_name("/workspace/crates/my-crate/Cargo.toml"), "my-crate");
    }

    #[test]
    fn test_extract_crate_name_root_toml() {
        assert_eq!(extract_crate_name("Cargo.toml"), "unknown");
    }

    #[test]
    fn test_extract_crate_name_workspace_root() {
        assert_eq!(extract_crate_name("/workspace/Cargo.toml"), "workspace");
    }

    // ── Finding metadata completeness ──────────────────────────────────────────

    #[test]
    fn test_finding_has_required_metadata() {
        let text =
            "Found the following unused dependencies in /ws/crates/foo/Cargo.toml:\nunused_dep\n\n";
        let findings = parse_machete_text(text, "/tmp/out.txt", "0.7.0", "cargo machete").unwrap();
        assert_eq!(findings.len(), 1);
        let f = &findings[0];

        assert!(!f.crate_name.is_empty(), "crate_name must not be empty");
        assert!(!f.manifest_path.is_empty(), "manifest_path must not be empty");
        assert!(!f.dep_name.is_empty(), "dep_name must not be empty");
        assert!(!f.instrument.is_empty(), "instrument must not be empty");
        assert!(!f.command_identity.is_empty(), "command_identity must not be empty");
        assert!(!f.finding_code.is_empty(), "finding_code must not be empty");
        assert!(!f.native_output_ref.is_empty(), "native_output_ref must not be empty");
        assert!(!f.limitations.is_empty(), "limitations must be documented");
    }

    // ── Negative control #6: intentional retention via tool-native ignores ─────

    /// Documents the mechanism for intentionally retained dependencies.
    ///
    /// cargo-machete supports `.cargo-machete.toml` per-crate ignore files
    /// as its official exception mechanism. Suppression does not occur in
    /// this module — it occurs upstream in the tool configuration.
    /// The limitation text in each finding documents the false-positive risk.
    #[test]
    fn test_findings_document_false_positive_limitation() {
        let text = "Found the following unused dependencies in /ws/Cargo.toml:\nmy_dep\n\n";
        let findings = parse_machete_text(text, "/out", "0.7.0", "cargo machete").unwrap();
        assert_eq!(findings.len(), 1);
        let lims = &findings[0].limitations;
        assert!(
            lims.iter().any(|l| l.contains("proc-macro") || l.contains("build script")),
            "limitations must document false-positive risk from macros/build scripts"
        );
    }

    // ── Negative control #9: cargo-udeps not in active path ───────────────────

    /// Documents that cargo-udeps is removed from the active dependency
    /// hygiene path per issue #9364. Every finding must identify cargo-machete.
    #[test]
    fn test_all_findings_attribute_to_machete_not_udeps() {
        let text = "Found the following unused dependencies in /ws/Cargo.toml:\nfoo\n\n";
        let findings = parse_machete_text(text, "/out", "0.7.0", "cargo machete").unwrap();
        for f in &findings {
            assert_eq!(
                f.instrument, "cargo-machete",
                "all findings must attribute to cargo-machete, not cargo-udeps"
            );
        }
    }

    // ── Negative control #10: no tool installation in check/report paths ───────

    /// Verifies that a missing binary does not result in an installation side
    /// effect. The probe returns a non-Available result; gather_findings would
    /// return NOT_PROVEN before ever reaching any install logic.
    #[test]
    fn test_no_installation_side_effect_on_missing_tool() {
        let outcome = probe_machete_with_cargo("/nonexistent/binary");
        assert!(
            !matches!(outcome, ProbeOutcome::Available(_)),
            "a missing binary must not become Available (installation must not be a side effect)"
        );
    }
}
