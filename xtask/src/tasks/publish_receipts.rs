//! Publish a review receipt bundle under review/receipts/YYYY-MM-DD/.
//!
//! Combines `cargo xtask gates` and `cargo xtask receipts` into a single
//! command, archives outputs, and writes a short README with provenance.
//!
//! # Refusing partial or stale runs (#15350)
//!
//! A documentation-truth bundle is published only from one current, complete
//! run. Before any file is copied the publisher validates the freshly
//! generated `artifacts/` outputs and refuses:
//!
//! - a missing expected receipt file (blocking, not a warning);
//! - a `state.json` without typed run identity, subject binding, and domain
//!   statuses (this also rejects synthetic quick/state files);
//! - any domain whose status is not `complete_pass`/`complete_with_failures`
//!   (i.e. `not_run_by_mode` or `instrument_failed` domains);
//! - a state whose subject head differs from the current `git rev-parse HEAD`
//!   (stale run);
//! - a member file whose bytes no longer match the raw-output digest recorded
//!   in `state.json` (mixed-run / stale-file mixture).

use chrono::Utc;
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use crate::tasks::receipts::{DomainStatus, digest_hex};
use crate::utils::project_root;

const RECEIPT_FILES: [&str; 5] =
    ["test-output.txt", "test-summary.json", "rustdoc.log", "doc-summary.json", "state.json"];

/// Members whose bytes are digest-bound to `state.json` (#15350).
const DIGEST_BOUND_FILES: [&str; 4] =
    ["test-output.txt", "rustdoc.log", "test-summary.json", "doc-summary.json"];

pub fn run(date: Option<String>) -> Result<()> {
    let date = date.unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string());
    if date.trim().is_empty() {
        bail!("receipt date cannot be empty");
    }

    let root = project_root()?;
    let destination = root.join("review").join("receipts").join(&date);
    let artifacts_source = root.join("artifacts");
    let artifacts_destination = destination.join("artifacts");

    if destination.join("README.md").exists() {
        bail!(
            "an earlier receipt bundle already exists at {}; overwriting it would mix runs \
             without explicit versioned disposition (#15350). Pick a new date path or remove \
             the earlier bundle deliberately.",
            destination.display()
        );
    }

    fs::create_dir_all(&artifacts_destination)
        .with_context(|| format!("Failed to create {}", artifacts_destination.display()))?;

    println!("Publishing receipts to: {}", destination.display());

    let ci_output = run_and_log(
        "ci gate",
        &root,
        &mut cargo_xtask_gates("merge-gate"),
        &destination.join("ci-gate.log"),
    )?;
    println!("{}", ci_output);

    let receipts_output = run_and_log(
        "receipt generation",
        &root,
        &mut command_receipts(),
        &destination.join("generate-receipts.log"),
    )?;
    println!("{}", receipts_output);

    // Gather every expected member from the run that just executed. A missing
    // member is blocking: no shared-directory residue may satisfy this run.
    let mut file_contents: BTreeMap<&'static str, Vec<u8>> = BTreeMap::new();
    for file in RECEIPT_FILES {
        let source = artifacts_source.join(file);
        let bytes = fs::read(&source).with_context(|| {
            format!(
                "missing receipt member {}; publication requires every declared receipt \
                 file from the current run (#15350)",
                source.display()
            )
        })?;
        file_contents.insert(file, bytes);
    }

    let current_head = repository_head(&root)?;
    let state_json = String::from_utf8_lossy(file_contents["state.json"].as_slice()).to_string();
    let validated = validate_receipt_bundle(&state_json, &file_contents, &current_head)?;

    let mut copied = 0usize;
    for file in RECEIPT_FILES {
        let source = artifacts_source.join(file);
        let target = artifacts_destination.join(file);
        fs::copy(&source, &target).with_context(|| {
            format!("failed to copy {} to {}", source.display(), target.display())
        })?;
        let copied_bytes = fs::read(&target).with_context(|| {
            format!("failed to re-read {} for copy verification", target.display())
        })?;
        if copied_bytes != file_contents[file] {
            bail!(
                "copy verification failed for {file}: published bytes differ from the \
                 validated run output (#15350)"
            );
        }
        copied += 1;
    }

    let readme = format!(
        r#"# Receipt Bundle: {date}

## Provenance
- Run ID: `{run_id}`
- Commit: `{head}` (verified against `artifacts/state.json` subject binding)
- Working tree dirty at generation: {dirty}
- rustc: `{rustc}`
- cargo: `{cargo}`
- Host: `{host}`

## Domain statuses
- tests: {tests_status}
- docs: {docs_status}

## Raw output digests (sha256)
{digest_lines}

## What ran
- `cargo xtask gates --tier merge-gate --receipt` (see `ci-gate.log`)
- `cargo xtask receipts` (see `generate-receipts.log`)
- {copied} artifact file(s) copied from the same run directory

## Limitations
- These counts are evidence only for run `{run_id}` at commit `{head}`.
- A bundle is refused when a member file is missing, a domain is
  `not_run_by_mode`/`instrument_failed`, raw-output digests mismatch, or the
  subject head differs from the repository HEAD at publication (#15350).
"#,
        date = date,
        run_id = validated.run_id,
        head = validated.head,
        dirty = if validated.dirty { "yes" } else { "no" },
        rustc = command_output_or_unknown(&root, &["rustc", "--version"], "UNVERIFIED"),
        cargo = command_output_or_unknown(&root, &["cargo", "--version"], "UNVERIFIED"),
        host = command_output_or_unknown(&root, &["uname", "-a"], "UNVERIFIED"),
        tests_status = validated.tests_status,
        docs_status = validated.docs_status,
        digest_lines = validated.digest_lines,
        copied = copied,
    );

    fs::write(destination.join("README.md"), readme)
        .with_context(|| format!("Failed to write {}", destination.join("README.md").display()))?;

    println!("Receipt bundle ready: {}", destination.display());
    Ok(())
}

/// Read-side view of the typed consolidated state (#15350).
///
/// Untyped/legacy synthetic state (no run id, subject, or domain statuses)
/// fails deserialization into this view and is refused.
#[derive(Debug, Deserialize)]
struct StateView {
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    subject: Option<SubjectView>,
    tests: DomainView,
    docs: DomainView,
    #[serde(default)]
    test_summary_digest: Option<String>,
    #[serde(default)]
    doc_summary_digest: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubjectView {
    head: String,
    #[serde(default)]
    dirty: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct DomainView {
    #[serde(default)]
    status: Option<DomainStatus>,
    #[serde(default)]
    raw_output_digest: Option<String>,
}

/// Evidence joined from one validated, current, complete run.
struct ValidatedBundle {
    run_id: String,
    head: String,
    dirty: bool,
    tests_status: &'static str,
    docs_status: &'static str,
    digest_lines: String,
}

/// Validate one candidate bundle before publication (#15350).
///
/// `file_contents` holds the bytes of every expected member read from the run
/// directory; `current_head` is the repository HEAD at publication time.
pub(crate) fn validate_receipt_bundle(
    state_json: &str,
    file_contents: &BTreeMap<&'static str, Vec<u8>>,
    current_head: &str,
) -> Result<ValidatedBundle> {
    let state: StateView = serde_json::from_str(state_json).map_err(|err| {
        eyre!(
            "state.json is not a typed receipt state; refusing partial or synthetic bundle \
             (#15350): {err}"
        )
    })?;

    let run_id = state
        .run_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| eyre!("state.json has no run_id; refusing unbound bundle (#15350)"))?;

    let subject = state.subject.ok_or_else(|| {
        eyre!("state.json has no subject binding; refusing unbound bundle (#15350)")
    })?;
    if subject.head.trim().is_empty() {
        bail!("state.json subject head is empty; refusing unbound bundle (#15350)");
    }
    if subject.head != current_head {
        bail!(
            "stale run refused (#15350): state.json subject head {} does not match the \
             current repository HEAD {}",
            subject.head,
            current_head
        );
    }

    let status_label = |status: Option<DomainStatus>, domain: &str| -> Result<&'static str> {
        match status {
            Some(DomainStatus::CompletePass) => Ok("complete_pass"),
            Some(DomainStatus::CompleteWithFailures) => Ok("complete_with_failures"),
            Some(DomainStatus::NotRunByMode) => bail!(
                "partial run refused (#15350): {domain} domain recorded not_run_by_mode; \
                 a documentation-truth bundle requires explicitly run domains"
            ),
            Some(DomainStatus::InstrumentFailed) => bail!(
                "instrument failure refused (#15350): {domain} domain recorded \
                 instrument_failed; its counts are diagnostics, not documentation truth"
            ),
            None => bail!(
                "untyped {domain} domain status refused (#15350); a count without \
                 completeness is not evidence"
            ),
        }
    };
    let tests_status = status_label(state.tests.status, "tests")?;
    let docs_status = status_label(state.docs.status, "docs")?;

    // Every expected member must be present (blocking, not a warning).
    for file in RECEIPT_FILES {
        if !file_contents.contains_key(file) {
            bail!("missing receipt member {file}; refusing incomplete bundle (#15350)");
        }
    }

    // Members whose bytes are digest-bound to state.json must match exactly:
    // a surviving file from an earlier run cannot enter this bundle.
    let mut digest_lines = String::new();
    for file in DIGEST_BOUND_FILES {
        let recorded = match file {
            "test-output.txt" => state.tests.raw_output_digest.as_deref(),
            "rustdoc.log" => state.docs.raw_output_digest.as_deref(),
            "test-summary.json" => state.test_summary_digest.as_deref(),
            "doc-summary.json" => state.doc_summary_digest.as_deref(),
            _ => None,
        };
        let Some(recorded) = recorded.filter(|value| value.len() == 64) else {
            bail!(
                "state.json records no usable raw-output digest for {file}; refusing \
                 unverifiable bundle (#15350)"
            );
        };
        let contents = &file_contents[file];
        let actual = digest_hex(contents);
        if actual != recorded {
            bail!(
                "mixed-run refused (#15350): digest mismatch for {file} (state.json \
                 recorded {recorded}, actual {actual}); the file was not produced by run \
                 {run_id}"
            );
        }
        digest_lines.push_str(&format!("- {file}: `{actual}`\n"));
    }

    Ok(ValidatedBundle {
        run_id,
        head: subject.head,
        dirty: subject.dirty.unwrap_or(false),
        tests_status,
        docs_status,
        digest_lines,
    })
}

/// Full head SHA of the repository at `root`; publication requires it.
fn repository_head(root: &Path) -> Result<String> {
    let mut command = Command::new("git");
    command.current_dir(root).args(["rev-parse", "HEAD"]);
    let output = command
        .output()
        .with_context(|| format!("Failed to execute git rev-parse HEAD in {}", root.display()))?;
    if !output.status.success() {
        bail!(
            "cannot establish current repository HEAD; refusing to publish a bundle without \
             a freshness bound (#15350)"
        );
    }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if head.is_empty() {
        bail!("git rev-parse HEAD printed nothing; refusing unbound bundle (#15350)");
    }
    Ok(head)
}

fn cargo_xtask_gates(tier: &str) -> Command {
    let mut command = Command::new("cargo");
    command.args(["xtask", "gates", "--tier", tier, "--receipt"]);
    command
}

fn command_receipts() -> Command {
    let mut command = Command::new("cargo");
    command.args(["xtask", "receipts"]);
    command
}

fn run_and_log(stage: &str, root: &Path, command: &mut Command, log_path: &Path) -> Result<String> {
    let output = command.current_dir(root).output().with_context(|| {
        format!("Failed to execute {stage} command in {}", root.join(".").display())
    })?;

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut log = fs::File::create(log_path)
        .with_context(|| format!("Failed to create log at {}", log_path.display()))?;
    log.write_all(combined.as_bytes())
        .with_context(|| format!("Failed to write {}", log_path.display()))?;

    if !output.status.success() {
        bail!("{stage} failed (see {})", log_path.display());
    }

    Ok(combined)
}

/// Run a provenance command and use its stdout only when it succeeded.
///
/// Non-empty stdout from a failed command cannot establish provenance
/// (#15350): the fallback is used for both failure and empty output.
fn command_output_or_unknown(root: &Path, args: &[&str], fallback: &str) -> String {
    if args.is_empty() {
        return fallback.to_string();
    }

    let mut command = Command::new(args[0]);
    command.current_dir(root).args(&args[1..]);

    command_output_or_unknown_with(command, fallback)
}

/// Shared body so tests can exercise the failure contract directly (#15350).
fn command_output_or_unknown_with(mut command: Command, fallback: &str) -> String {
    match command.output() {
        Ok(output) if output.status.success() => {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if value.is_empty() { fallback.to_string() } else { value }
        }
        _ => fallback.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "1111111111111111111111111111111111111111";
    const OTHER_HEAD: &str = "2222222222222222222222222222222222222222";
    const TEST_OUTPUT: &str = "test result: ok. 7 passed; 0 failed; 0 ignored\n";
    const RUSTDOC_LOG: &str = "warning: missing documentation for x\n";
    const TEST_SUMMARY_JSON: &str = r#"{"passed": 7, "status": "complete_pass"}"#;
    const DOC_SUMMARY_JSON: &str = r#"{"missing_docs": 1, "status": "complete_pass"}"#;

    fn typed_state_json(head: &str, tests_status: &str, docs_status: &str) -> String {
        let test_digest = digest_hex(TEST_OUTPUT.as_bytes());
        let doc_digest = digest_hex(RUSTDOC_LOG.as_bytes());
        let test_summary_digest = digest_hex(TEST_SUMMARY_JSON.as_bytes());
        let doc_summary_digest = digest_hex(DOC_SUMMARY_JSON.as_bytes());
        format!(
            r#"{{
  "version": "0.18.0",
  "run_id": "run-1726000000-00000000abcdef01",
  "subject": {{"head": "{head}", "dirty": false}},
  "tests": {{"passed": 7, "status": "{tests_status}", "raw_output_digest": "{test_digest}"}},
  "docs": {{"missing_docs": 1, "status": "{docs_status}", "raw_output_digest": "{doc_digest}"}},
  "test_summary_digest": "{test_summary_digest}",
  "doc_summary_digest": "{doc_summary_digest}",
  "generated_at": "2026-09-18T00:00:00Z"
}}"#
        )
    }

    fn full_files() -> BTreeMap<&'static str, Vec<u8>> {
        let mut files: BTreeMap<&'static str, Vec<u8>> = BTreeMap::new();
        for file in RECEIPT_FILES {
            files.insert(file, Vec::new());
        }
        files.insert("test-output.txt", TEST_OUTPUT.as_bytes().to_vec());
        files.insert("rustdoc.log", RUSTDOC_LOG.as_bytes().to_vec());
        files.insert("test-summary.json", TEST_SUMMARY_JSON.as_bytes().to_vec());
        files.insert("doc-summary.json", DOC_SUMMARY_JSON.as_bytes().to_vec());
        files
    }

    fn valid_bundle() -> (String, BTreeMap<&'static str, Vec<u8>>) {
        (typed_state_json(HEAD, "complete_pass", "complete_pass"), full_files())
    }

    fn unwrap_ok(result: Result<ValidatedBundle>) -> ValidatedBundle {
        match result {
            Ok(validated) => validated,
            Err(err) => panic!("expected valid bundle, got: {err}"),
        }
    }

    fn unwrap_err(result: Result<ValidatedBundle>) -> String {
        match result {
            Ok(_) => panic!("expected bundle refusal, got Ok"),
            Err(err) => format!("{err:#}"),
        }
    }

    #[test]
    fn current_complete_bundle_is_accepted() {
        let (state, files) = valid_bundle();
        let validated = unwrap_ok(validate_receipt_bundle(&state, &files, HEAD));
        assert_eq!(validated.run_id, "run-1726000000-00000000abcdef01");
        assert_eq!(validated.head, HEAD);
        assert_eq!(validated.tests_status, "complete_pass");
        assert_eq!(validated.docs_status, "complete_pass");
        assert!(validated.digest_lines.contains("test-output.txt"));
    }

    #[test]
    fn complete_with_failures_domain_is_accepted() {
        let state = typed_state_json(HEAD, "complete_with_failures", "complete_pass");
        let validated = unwrap_ok(validate_receipt_bundle(&state, &full_files(), HEAD));
        assert_eq!(validated.tests_status, "complete_with_failures");
    }

    #[test]
    fn missing_member_is_refused() {
        let (state, mut files) = valid_bundle();
        files.remove("doc-summary.json");
        let err = unwrap_err(validate_receipt_bundle(&state, &files, HEAD));
        assert!(err.contains("missing receipt member"), "{err}");
        assert!(err.contains("doc-summary.json"), "{err}");
    }

    #[test]
    fn stale_head_is_refused() {
        let (state, files) = valid_bundle();
        let err = unwrap_err(validate_receipt_bundle(&state, &files, OTHER_HEAD));
        assert!(err.contains("stale run refused"), "{err}");
    }

    #[test]
    fn mixed_run_digest_mismatch_is_refused() {
        let (state, mut files) = valid_bundle();
        files.insert("test-output.txt", b"test result: ok. 1 passed; 0 failed\n".to_vec());
        let err = unwrap_err(validate_receipt_bundle(&state, &files, HEAD));
        assert!(err.contains("mixed-run refused"), "{err}");
        assert!(err.contains("digest mismatch"), "{err}");
    }

    #[test]
    fn not_run_by_mode_domain_is_refused() {
        let state = typed_state_json(HEAD, "not_run_by_mode", "complete_pass");
        let err = unwrap_err(validate_receipt_bundle(&state, &full_files(), HEAD));
        assert!(err.contains("partial run refused"), "{err}");
        assert!(err.contains("not_run_by_mode"), "{err}");
    }

    #[test]
    fn instrument_failed_domain_is_refused() {
        let state = typed_state_json(HEAD, "complete_pass", "instrument_failed");
        let err = unwrap_err(validate_receipt_bundle(&state, &full_files(), HEAD));
        assert!(err.contains("instrument failure refused"), "{err}");
    }

    #[test]
    fn synthetic_state_without_typed_fields_is_refused() {
        // The pre-#15350 / quick-receipts style state: counts plus a fresh
        // timestamp, no run id, subject, or typed statuses.
        let synthetic = r#"{"version": "0.18.0", "tests": {"passed": 0, "failed": 0}, "docs": {"missing_docs": 0}, "generated_at": "2026-09-18T00:00:00Z"}"#;
        let err = unwrap_err(validate_receipt_bundle(synthetic, &full_files(), HEAD));
        assert!(err.contains("not a typed receipt state") || err.contains("run_id"), "{err}");
    }

    #[test]
    fn unbound_state_without_run_id_is_refused() {
        let unbound = typed_state_json(HEAD, "complete_pass", "complete_pass")
            .replace("run-1726000000-00000000abcdef01", "");
        let err = unwrap_err(validate_receipt_bundle(&unbound, &full_files(), HEAD));
        assert!(err.contains("run_id"), "{err}");
    }

    #[test]
    fn foreign_head_subject_is_refused_even_when_file_digests_match() {
        // A state bound to a different head is stale regardless of digests.
        let state = typed_state_json(OTHER_HEAD, "complete_pass", "complete_pass");
        let (_, files) = valid_bundle();
        let err = unwrap_err(validate_receipt_bundle(&state, &files, HEAD));
        assert!(err.contains("stale run refused"), "{err}");
    }

    #[test]
    fn command_output_or_unknown_rejects_stdout_from_failed_command() {
        #[cfg(windows)]
        let mut failing = {
            let mut command = Command::new("cmd");
            command.args(["/C", "echo hello & exit 1"]);
            command
        };
        #[cfg(not(windows))]
        let mut failing = {
            let mut command = Command::new("sh");
            command.args(["-c", "echo hello; exit 1"]);
            command
        };
        let value = command_output_or_unknown_with(failing, "UNVERIFIED");
        assert_eq!(value, "UNVERIFIED");

        let mut succeeding = Command::new("git");
        succeeding.args(["--version"]);
        let value = command_output_or_unknown_with(succeeding, "UNVERIFIED");
        assert!(value.starts_with("git version"), "{value}");
    }
}
