//! Thin CLI for the transition classify + check command surface.
//!
//! `classify` loads V2 accepted baseline + current run-report JSON, optionally
//! binds `--series` identity and `--discovery` to that series, applies the
//! in-lib `classify_transition` core, and writes a non-authorizing
//! classification receipt (with input digests) to `--output`.
//!
//! `check` reloads the same evidence (and optional series/discovery),
//! recomputes classification + digests, and verifies an existing receipt
//! matches exactly.
//!
//! Hard-link (and Unix inode) output/evidence identity is enforced when the
//! output path already exists. Classification consumes only structurally
//! validated evidence (`ValidatedRunReport` / accepted baseline validation);
//! closed RunReport serde uses `deny_unknown_fields`.

#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

use color_eyre::eyre::{Context, Result, bail};
use perl_core_harness::transition::{AcceptedBaseline, Classification, classify_transition};
use perl_core_harness_types::{
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompatibilityTransition, CompileBaselineV2,
    DISCOVERY_SCHEMA_VERSION, DiscoveryReport, RUN_REPORT_SCHEMA_VERSION, RunReport,
    SERIES_MANIFEST_SCHEMA_VERSION, SeriesManifest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

include!("perl-core-harness-transition/cli.rs");

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "usage: perl-core-harness-transition <classify|check> --accepted-baseline <path> --compile <path> (--output|--receipt) <path>"
        )
    })?;
    let options = Options::parse(args)?;
    match command.as_str() {
        "classify" => {
            let config = ClassifyConfig::from_options(options)?;
            run_classify(&config)
        }
        "check" => {
            let config = CheckConfig::from_options(options)?;
            run_check(&config)
        }
        _ => bail!("unknown perl-core-harness-transition command: {command}"),
    }
}

fn run_classify(config: &ClassifyConfig) -> Result<()> {
    reject_output_input_path_collision(config)?;
    let accepted_bytes = read_bytes(&config.accepted_baseline, "accepted baseline")?;
    let compile_bytes = read_bytes(&config.compile, "compile observation")?;
    let accepted = decode_accepted_v2(&accepted_bytes, &config.accepted_baseline)?;
    let current = decode_run_report(&compile_bytes, &config.compile)?;
    if let Some(series_path) = &config.series {
        let series = load_series_manifest(series_path)?;
        bind_series_identity(&series, &accepted)?;
        bind_report_to_series(&current, &series)?;
        if let Some(discovery_path) = &config.discovery {
            let discovery = load_discovery_report(discovery_path)?;
            bind_discovery_to_series(&discovery, &series)?;
        }
    }
    let classification = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    write_classification_receipt(
        &config.output,
        &classification,
        &sha256_digest_bytes(&accepted_bytes),
        &sha256_digest_bytes(&compile_bytes),
    )
}

fn run_check(config: &CheckConfig) -> Result<()> {
    let accepted_bytes = read_bytes(&config.accepted_baseline, "accepted baseline")?;
    let compile_bytes = read_bytes(&config.compile, "compile observation")?;
    let accepted = decode_accepted_v2(&accepted_bytes, &config.accepted_baseline)?;
    let current = decode_run_report(&compile_bytes, &config.compile)?;
    if let Some(series_path) = &config.series {
        let series = load_series_manifest(series_path)?;
        bind_series_identity(&series, &accepted)?;
        bind_report_to_series(&current, &series)?;
        if let Some(discovery_path) = &config.discovery {
            let discovery = load_discovery_report(discovery_path)?;
            bind_discovery_to_series(&discovery, &series)?;
        }
    }
    let expected = classify_transition(&AcceptedBaseline::V2(Box::new(accepted)), &current);
    let expected_accepted_digest = sha256_digest_bytes(&accepted_bytes);
    let expected_compile_digest = sha256_digest_bytes(&compile_bytes);
    let receipt = load_classify_receipt(&config.receipt)?;
    verify_classify_receipt(
        &receipt,
        &expected,
        &expected_accepted_digest,
        &expected_compile_digest,
    )
}

fn reject_output_input_path_collision(config: &ClassifyConfig) -> Result<()> {
    let protected: Vec<&Path> = [
        Some(config.accepted_baseline.as_path()),
        Some(config.compile.as_path()),
        config.series.as_deref(),
        config.discovery.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect();

    for source in &protected {
        if paths_equal(&config.output, source) {
            bail!(
                "output path must not alias --accepted-baseline, --compile, --series, or --discovery; refusing to overwrite evidence"
            );
        }
    }

    // When --output already exists, reject hard-link (and Unix inode) aliases of
    // protected evidence. Path-string equality above cannot see those aliases.
    if config.output.exists() {
        for source in protected {
            if same_file_identity(&config.output, source)? {
                bail!(
                    "output path is a hard-link alias of --accepted-baseline, --compile, --series, or --discovery; refusing to overwrite evidence"
                );
            }
        }
    }
    Ok(())
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(unix)]
fn same_file_identity(output: &Path, source: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let output = fs::metadata(output)
        .with_context(|| format!("reading classify output metadata {}", output.display()))?;
    let source = match fs::metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("reading protected evidence metadata {}", source.display())
            });
        }
    };
    Ok(output.dev() == source.dev() && output.ino() == source.ino())
}

#[cfg(windows)]
fn same_file_identity(output: &Path, source: &Path) -> Result<bool> {
    let output_identity = windows_file_identity(output)?;
    let source_identity = windows_file_identity(source)?;
    Ok(output_identity.is_some() && output_identity == source_identity)
}

#[cfg(windows)]
fn windows_file_identity(path: &Path) -> Result<Option<(u32, u32, u32)>> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use winapi::um::winnt::{FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE};

    let display_path = path.display().to_string();
    // Route pathname opening through Rust's standard Windows path handling so
    // absolute paths beyond MAX_PATH receive the same verbatim normalization
    // as the fs operations that created the receipt sibling. This deliberately
    // follows links for output/evidence alias detection; temporary-entry checks
    // reject symlinks with `symlink_metadata` before calling this helper.
    let file = match OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("reading file identity {display_path}"));
        }
    };
    windows_handle_identity(file.as_raw_handle().cast(), &display_path).map(Some)
}

#[cfg(windows)]
fn windows_handle_identity(
    handle: winapi::um::winnt::HANDLE,
    label: &str,
) -> Result<(u32, u32, u32)> {
    use winapi::um::fileapi::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};

    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let result = unsafe { GetFileInformationByHandle(handle, &mut information) };
    if result == 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("reading file identity {label}"));
    }
    Ok((information.dwVolumeSerialNumber, information.nFileIndexHigh, information.nFileIndexLow))
}

#[cfg(not(any(unix, windows)))]
fn same_file_identity(_output: &Path, _source: &Path) -> Result<bool> {
    // Path-string equality covers direct aliases. Stable hard-link identities are
    // not exposed by the standard library on this platform.
    Ok(false)
}

fn read_bytes(path: &Path, label: &str) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("reading {label} {}", path.display()))
}

fn decode_accepted_v2(bytes: &[u8], path: &Path) -> Result<CompileBaselineV2> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .with_context(|| format!("decoding accepted baseline JSON {}", path.display()))?;
    let schema =
        value.get("schema_version").and_then(serde_json::Value::as_str).unwrap_or("missing");
    if schema != COMPILE_BASELINE_V2_SCHEMA_VERSION {
        bail!(
            "unsupported accepted baseline schema: {schema}; classify I/O accepts {} only",
            COMPILE_BASELINE_V2_SCHEMA_VERSION
        );
    }
    serde_json::from_value(value)
        .with_context(|| format!("decoding accepted V2 baseline {}", path.display()))
}

fn decode_run_report(bytes: &[u8], path: &Path) -> Result<RunReport> {
    let report: RunReport = serde_json::from_slice(bytes)
        .with_context(|| format!("decoding compile observation JSON {}", path.display()))?;
    if report.schema_version != RUN_REPORT_SCHEMA_VERSION {
        bail!(
            "unsupported compile observation schema: {}; expected {}",
            report.schema_version,
            RUN_REPORT_SCHEMA_VERSION
        );
    }
    Ok(report)
}

fn load_series_manifest(path: &Path) -> Result<SeriesManifest> {
    let bytes = read_bytes(path, "series manifest")?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding series manifest JSON {}", path.display()))?;
    let schema =
        value.get("schema_version").and_then(serde_json::Value::as_str).unwrap_or("missing");
    if schema != SERIES_MANIFEST_SCHEMA_VERSION {
        bail!(
            "unsupported series manifest schema: {schema}; classify I/O accepts {} only",
            SERIES_MANIFEST_SCHEMA_VERSION
        );
    }
    serde_json::from_value(value)
        .with_context(|| format!("decoding series manifest {}", path.display()))
}

/// Lean series identity binding for classify/check.
///
/// Binds accepted V2 to an explicit `--series` manifest via `series_id`,
/// `manifest_hash`, exact `file_membership`/`normalized_manifest` equality, and
/// subject fields (`profile`, `runner`, `perl_resolved_ref`). Does not rehash
/// the series, claim hard-link identity, or centralize #6880 validated wrappers.
fn bind_series_identity(series: &SeriesManifest, accepted: &CompileBaselineV2) -> Result<()> {
    if accepted.series_id != series.series_id {
        bail!("accepted baseline is not bound to series {}: series_id mismatch", series.series_id);
    }
    if accepted.manifest_hash != series.manifest_hash {
        bail!(
            "accepted baseline is not bound to series {}: manifest_hash mismatch",
            series.series_id
        );
    }
    if accepted.file_membership != series.normalized_manifest {
        bail!(
            "accepted baseline is not bound to series {}: file_membership mismatch",
            series.series_id
        );
    }
    if accepted.profile != series.profile {
        bail!("accepted baseline is not bound to series {}: profile mismatch", series.series_id);
    }
    if accepted.runner != series.runner {
        bail!("accepted baseline is not bound to series {}: runner mismatch", series.series_id);
    }
    if accepted.perl_resolved_ref != series.perl_resolved_ref {
        bail!(
            "accepted baseline is not bound to series {}: perl_resolved_ref mismatch",
            series.series_id
        );
    }
    Ok(())
}

fn load_discovery_report(path: &Path) -> Result<DiscoveryReport> {
    let bytes = read_bytes(path, "discovery report")?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding discovery report JSON {}", path.display()))?;
    let schema =
        value.get("schema_version").and_then(serde_json::Value::as_str).unwrap_or("missing");
    if schema != DISCOVERY_SCHEMA_VERSION {
        bail!(
            "unsupported discovery report schema: {schema}; classify I/O accepts {} only",
            DISCOVERY_SCHEMA_VERSION
        );
    }
    serde_json::from_value(value)
        .with_context(|| format!("decoding discovery report {}", path.display()))
}

/// Lean discovery→series binding for classify/check.
///
/// When `--discovery` is supplied (requires `--series`), binds discovery
/// `schema_version`/`profile`/`runner`/`perl_ref`/`commit` and sorted test
/// paths to the series harness schema, subject fields, repository commit, and
/// `normalized_manifest`. Does not re-normalize paths, rehash the series,
/// claim hard-link identity, or centralize #6880 validated wrappers.
fn bind_discovery_to_series(discovery: &DiscoveryReport, series: &SeriesManifest) -> Result<()> {
    if discovery.schema_version != series.harness_schema_version {
        bail!(
            "discovery report is not bound to series {}: harness_schema_version mismatch",
            series.series_id
        );
    }
    if discovery.profile != series.profile {
        bail!("discovery report is not bound to series {}: profile mismatch", series.series_id);
    }
    if discovery.runner != series.runner {
        bail!("discovery report is not bound to series {}: runner mismatch", series.series_id);
    }
    if discovery.perl_ref != series.perl_resolved_ref {
        bail!(
            "discovery report is not bound to series {}: perl_resolved_ref mismatch",
            series.series_id
        );
    }
    if discovery.commit != series.repository_commit {
        bail!(
            "discovery report is not bound to series {}: repository_commit mismatch",
            series.series_id
        );
    }
    let mut discovery_paths: Vec<String> =
        discovery.tests.iter().map(|test| test.path.clone()).collect();
    discovery_paths.sort();
    if discovery_paths != series.normalized_manifest {
        bail!(
            "discovery report is not bound to series {}: normalized_manifest mismatch",
            series.series_id
        );
    }
    Ok(())
}

/// Lean compile-observation→series binding for classify/check.
///
/// When `--series` is supplied, binds the measured run report's
/// `commit`/`perl_ref`/`runner`/`profile` to the series identity so discovery
/// and series cannot classify an observation from a different subject.
/// Mirrors the production `validate_report_against_series` identity checks
/// without centralizing #6880 validated wrappers.
fn bind_report_to_series(report: &RunReport, series: &SeriesManifest) -> Result<()> {
    if report.commit != series.repository_commit {
        bail!(
            "compile observation is not bound to series {}: repository_commit mismatch",
            series.series_id
        );
    }
    if report.perl_ref != series.perl_resolved_ref {
        bail!(
            "compile observation is not bound to series {}: perl_resolved_ref mismatch",
            series.series_id
        );
    }
    if report.runner != series.runner {
        bail!("compile observation is not bound to series {}: runner mismatch", series.series_id);
    }
    if report.profile != series.profile {
        bail!("compile observation is not bound to series {}: profile mismatch", series.series_id);
    }
    Ok(())
}

fn write_classification_receipt(
    path: &Path,
    classification: &Classification,
    accepted_baseline_digest: &str,
    compile_digest: &str,
) -> Result<()> {
    write_classification_receipt_with_hook(
        path,
        classification,
        accepted_baseline_digest,
        compile_digest,
        |_| Ok(()),
    )
}

fn write_classification_receipt_with_hook(
    path: &Path,
    classification: &Classification,
    accepted_baseline_digest: &str,
    compile_digest: &str,
    before_persist: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let parent =
        path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("creating output directory {}", parent.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| color_eyre::eyre::eyre!("classify output path has no file name"))?;
    #[cfg(windows)]
    let storage_parent = parent.canonicalize().with_context(|| {
        format!("canonicalizing classify output directory {}", parent.display())
    })?;
    #[cfg(not(windows))]
    let storage_parent = parent.to_path_buf();
    let publication_path = storage_parent.join(file_name);
    let receipt = ClassifyReceipt {
        schema_version: CLASSIFY_RECEIPT_SCHEMA_VERSION.to_string(),
        command: "classify".to_string(),
        transition: classification.transition,
        reason: classification.reason.clone(),
        requires_candidate: classification.requires_candidate,
        semantic_boundary_change: classification.semantic_boundary_change,
        accepted_baseline_digest: accepted_baseline_digest.to_string(),
        compile_digest: compile_digest.to_string(),
        claim_boundary: CLASSIFY_CLAIM_BOUNDARY.to_string(),
    };
    let encoded = serde_json::to_string_pretty(&receipt).context("serializing classify receipt")?;

    // `tempfile_in` uses exclusive creation and bounded collision retries. A
    // longer random component makes the sibling impractical to pre-plant, and
    // all bytes are written through the exclusively opened handle rather than
    // by reopening its path (which would follow a planted link).
    let mut temporary = tempfile::Builder::new()
        .prefix(".perl-core-harness-transition-")
        .suffix(".tmp")
        .rand_bytes(16)
        // Automatic pathname cleanup cannot distinguish our file from an
        // attacker replacement. Cleanup below is identity-gated instead.
        .disable_cleanup(true)
        .tempfile_in(&storage_parent)
        .with_context(|| {
            format!("securely creating temporary classify receipt in {}", parent.display())
        })?;
    let write_result = temporary
        .write_all(format!("{encoded}\n").as_bytes())
        .with_context(|| format!("writing temporary classify receipt for {}", path.display()))
        .and_then(|()| {
            temporary.flush().with_context(|| {
                format!("flushing temporary classify receipt for {}", path.display())
            })
        })
        .and_then(|()| {
            temporary.as_file().sync_all().with_context(|| {
                format!("synchronizing temporary classify receipt for {}", path.display())
            })
        });
    if let Err(error) = write_result {
        cleanup_owned_temporary(&temporary)?;
        return Err(error);
    }

    if let Err(error) = before_persist(temporary.path()) {
        cleanup_owned_temporary(&temporary)?;
        return Err(error);
    }

    // `persist` atomically replaces a distinct existing entry on supported
    // Windows and Unix hosts. It renames the sibling pathname and never opens
    // the destination for writing, so an alias cannot truncate protected
    // evidence. Path identity is checked immediately before persistence and
    // cleanup. Final-path integrity still requires a directory that peers
    // cannot mutate between that check and the pathname operation.
    persist_classification_receipt(temporary, &publication_path, &storage_parent)?;
    Ok(())
}

#[cfg(unix)]
fn temporary_path_matches_open_file(temporary: &tempfile::NamedTempFile) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let opened = temporary.as_file().metadata().context("reading opened temporary identity")?;
    let entry = match fs::symlink_metadata(temporary.path()) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("reading temporary path identity"),
    };
    Ok(entry.file_type().is_file() && opened.dev() == entry.dev() && opened.ino() == entry.ino())
}

#[cfg(windows)]
fn temporary_path_matches_open_file(temporary: &tempfile::NamedTempFile) -> Result<bool> {
    use std::os::windows::io::AsRawHandle;

    let entry = match fs::symlink_metadata(temporary.path()) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("reading temporary path identity"),
    };
    if !entry.file_type().is_file() {
        return Ok(false);
    }
    let opened = windows_handle_identity(
        temporary.as_file().as_raw_handle().cast(),
        "opened temporary classify receipt",
    )?;
    Ok(windows_file_identity(temporary.path())?.is_some_and(|path| path == opened))
}

#[cfg(not(any(unix, windows)))]
fn temporary_path_matches_open_file(_temporary: &tempfile::NamedTempFile) -> Result<bool> {
    Ok(false)
}

fn cleanup_owned_temporary(temporary: &tempfile::NamedTempFile) -> Result<()> {
    // Automatic NamedTempFile cleanup is disabled because it would unlink the
    // saved pathname even after a peer replaced that entry. Under the output
    // directory authority contract this identity check ensures ordinary
    // failures remove only the sibling opened by this process. A detected
    // replacement is deliberately left untouched.
    if temporary_path_matches_open_file(temporary)? {
        fs::remove_file(temporary.path()).with_context(|| {
            format!("removing owned temporary classify receipt {}", temporary.path().display())
        })?;
    }
    Ok(())
}

#[cfg(windows)]
fn persist_classification_receipt(
    mut temporary: tempfile::NamedTempFile,
    path: &Path,
    _parent: &Path,
) -> Result<()> {
    const MAX_PERSIST_ATTEMPTS: usize = 8;

    for attempt in 0..MAX_PERSIST_ATTEMPTS {
        if !temporary_path_matches_open_file(&temporary)? {
            bail!(
                "temporary classify receipt path no longer identifies the exclusively opened file; refusing to persist"
            );
        }
        match temporary.persist(path) {
            Ok(file) => {
                drop(file);
                return Ok(());
            }
            Err(error)
                if retryable_windows_persist_error(&error.error)
                    && attempt + 1 < MAX_PERSIST_ATTEMPTS =>
            {
                // Concurrent MoveFileEx replacements can briefly hold the
                // destination entry. Retain ownership of the same exclusive
                // sibling and retry with bounded exponential backoff; never
                // reopen it.
                temporary = error.file;
                std::thread::sleep(std::time::Duration::from_millis(1 << attempt.min(6)));
            }
            Err(error) => {
                let persistence_error = color_eyre::eyre::eyre!(
                    "atomically persisting classify receipt {}: {}",
                    path.display(),
                    error.error
                );
                cleanup_owned_temporary(&error.file)?;
                return Err(persistence_error);
            }
        }
    }
    bail!("bounded classify receipt persistence exhausted without a result")
}

#[cfg(windows)]
fn retryable_windows_persist_error(error: &std::io::Error) -> bool {
    // ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION.
    // MoveFileExW reports these while another process retains a conflicting
    // destination handle. Other permission failures are not made retryable by
    // their broad ErrorKind alone.
    matches!(error.raw_os_error(), Some(5 | 32 | 33))
}

#[cfg(not(windows))]
fn persist_classification_receipt(
    temporary: tempfile::NamedTempFile,
    path: &Path,
    parent: &Path,
) -> Result<()> {
    if !temporary_path_matches_open_file(&temporary)? {
        bail!(
            "temporary classify receipt path no longer identifies the exclusively opened file; refusing to persist"
        );
    }
    match temporary.persist(path) {
        Ok(file) => {
            drop(file);
            #[cfg(unix)]
            fs::File::open(parent).and_then(|directory| directory.sync_all()).with_context(
                || format!("synchronizing classify receipt directory {}", parent.display()),
            )?;
            Ok(())
        }
        Err(error) => {
            let persistence_error = color_eyre::eyre::eyre!(
                "atomically persisting classify receipt {}: {}",
                path.display(),
                error.error
            );
            cleanup_owned_temporary(&error.file)?;
            Err(persistence_error)
        }
    }
}

fn load_classify_receipt(path: &Path) -> Result<ClassifyReceipt> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading classify receipt {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("decoding classify receipt {}", path.display()))
}

fn verify_classify_receipt(
    receipt: &ClassifyReceipt,
    expected: &Classification,
    expected_accepted_digest: &str,
    expected_compile_digest: &str,
) -> Result<()> {
    if receipt.schema_version != CLASSIFY_RECEIPT_SCHEMA_VERSION {
        bail!(
            "classify receipt schema mismatch: {}; expected {}",
            receipt.schema_version,
            CLASSIFY_RECEIPT_SCHEMA_VERSION
        );
    }
    if receipt.command != "classify" {
        bail!("classify receipt command mismatch: {}; expected classify", receipt.command);
    }
    if receipt.accepted_baseline_digest != expected_accepted_digest {
        bail!("classify receipt accepted_baseline_digest does not match current evidence bytes");
    }
    if receipt.compile_digest != expected_compile_digest {
        bail!("classify receipt compile_digest does not match current evidence bytes");
    }
    if receipt.transition != expected.transition {
        bail!(
            "classify receipt transition mismatch: {:?}; recomputed {:?}",
            receipt.transition,
            expected.transition
        );
    }
    if receipt.reason != expected.reason {
        bail!("classify receipt reason does not match recomputed classification");
    }
    if receipt.requires_candidate != expected.requires_candidate {
        bail!("classify receipt requires_candidate does not match recomputed classification");
    }
    if receipt.semantic_boundary_change != expected.semantic_boundary_change {
        bail!("classify receipt semantic_boundary_change does not match recomputed classification");
    }
    if receipt.claim_boundary != CLASSIFY_CLAIM_BOUNDARY {
        bail!("classify receipt claim_boundary does not match current command contract");
    }
    Ok(())
}

fn sha256_digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex_lower(&Sha256::digest(bytes)))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from_digit(u32::from(*byte >> 4), 16).unwrap_or('0'));
        output.push(char::from_digit(u32::from(*byte & 0x0f), 16).unwrap_or('0'));
    }
    output
}

const CLASSIFY_RECEIPT_SCHEMA_VERSION: &str = "perl_core_harness.transition_classify_result.v1";
const CLASSIFY_CLAIM_BOUNDARY: &str = "loads V2 accepted baseline + run-report JSON, optionally binds --series via series_id/manifest_hash/file_membership/profile/runner/perl_resolved_ref plus compile observation commit/perl_ref/runner/profile, and --discovery via harness_schema_version/profile/runner/perl_resolved_ref/repository_commit/normalized_manifest, classifies via in-lib classify_transition, writes non-authorizing receipt with input digests via atomic temp+rename; check recomputes digests+classification; refuses path-string and existing hard-link output aliases of evidence; does not accept ratchets or centralize #6880 validated wrappers";

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ClassifyReceipt {
    schema_version: String,
    command: String,
    transition: CompatibilityTransition,
    reason: String,
    requires_candidate: bool,
    semantic_boundary_change: bool,
    accepted_baseline_digest: String,
    compile_digest: String,
    claim_boundary: String,
}

#[cfg(test)]
mod classify_config_observer {
    use super::*;

    /// RIPR-named observer for Options::parse unrecognized-option rejection.
    #[test]
    fn unrecognized_option_parse_bail_is_observed() {
        let err = Options::parse(
            [
                "--accepted-baseline".to_string(),
                "accepted.json".to_string(),
                "--compile".to_string(),
                "compile.json".to_string(),
                "--output".to_string(),
                "out.json".to_string(),
                "--unknown".to_string(),
                "x.json".to_string(),
            ]
            .into_iter(),
        )
        .expect_err("unrecognized options must fail")
        .to_string();
        assert_eq!(err, "unrecognized option(s): --unknown");
    }

    #[test]
    fn series_option_is_accepted_by_parse() {
        let options = Options::parse(
            [
                "--accepted-baseline".to_string(),
                "accepted.json".to_string(),
                "--compile".to_string(),
                "compile.json".to_string(),
                "--output".to_string(),
                "out.json".to_string(),
                "--series".to_string(),
                "series.json".to_string(),
            ]
            .into_iter(),
        )
        .expect("series option must parse");
        let config = ClassifyConfig::from_options(options).expect("classify config");
        assert_eq!(config.series, Some(PathBuf::from("series.json")));
        assert_eq!(config.discovery, None);
    }

    #[test]
    fn discovery_option_requires_series() {
        let err = ClassifyConfig::from_options(
            Options::parse(
                [
                    "--accepted-baseline".to_string(),
                    "accepted.json".to_string(),
                    "--compile".to_string(),
                    "compile.json".to_string(),
                    "--output".to_string(),
                    "out.json".to_string(),
                    "--discovery".to_string(),
                    "discovery.json".to_string(),
                ]
                .into_iter(),
            )
            .expect("parse"),
        )
        .expect_err("discovery without series must fail")
        .to_string();
        assert_eq!(
            err,
            "--discovery requires --series; discovery binds to the comparison-series manifest"
        );
    }

    #[test]
    fn discovery_option_is_accepted_with_series() {
        let options = Options::parse(
            [
                "--accepted-baseline".to_string(),
                "accepted.json".to_string(),
                "--compile".to_string(),
                "compile.json".to_string(),
                "--output".to_string(),
                "out.json".to_string(),
                "--series".to_string(),
                "series.json".to_string(),
                "--discovery".to_string(),
                "discovery.json".to_string(),
            ]
            .into_iter(),
        )
        .expect("discovery+series options must parse");
        let config = ClassifyConfig::from_options(options).expect("classify config");
        assert_eq!(config.series, Some(PathBuf::from("series.json")));
        assert_eq!(config.discovery, Some(PathBuf::from("discovery.json")));
    }

    /// RIPR observer for `Options::optional` empty-recorded queue → absent.
    #[test]
    fn optional_empty_recorded_value_is_treated_as_absent() {
        let mut values = BTreeMap::new();
        values.insert("--series".to_string(), VecDeque::new());
        let mut options = Options { values };
        let value = options.optional("--series").expect("empty recorded optional is absent");
        assert_eq!(value, None);
    }

    /// RIPR observer for `Options::optional` duplicate-value rejection.
    #[test]
    fn optional_duplicate_value_error_is_observed() {
        let mut values = BTreeMap::new();
        let mut recorded = VecDeque::new();
        recorded.push_back("a.json".to_string());
        recorded.push_back("b.json".to_string());
        values.insert("--series".to_string(), recorded);
        let mut options = Options { values };
        let err =
            options.optional("--series").expect_err("duplicate optional must fail").to_string();
        assert_eq!(err, "option --series may be supplied only once");
    }

    #[test]
    fn duplicate_option_is_rejected() {
        let err = Options::parse(
            [
                "--output".to_string(),
                "a.json".to_string(),
                "--output".to_string(),
                "b.json".to_string(),
            ]
            .into_iter(),
        )
        .and_then(|mut options| options.required("--output").map(|_| ()))
        .expect_err("duplicate option must fail")
        .to_string();
        assert_eq!(err, "option --output may be supplied only once");
    }

    #[test]
    fn check_rejects_output_option() {
        let err = CheckConfig::from_options(
            Options::parse(
                [
                    "--accepted-baseline".to_string(),
                    "accepted.json".to_string(),
                    "--compile".to_string(),
                    "compile.json".to_string(),
                    "--receipt".to_string(),
                    "receipt.json".to_string(),
                    "--output".to_string(),
                    "out.json".to_string(),
                ]
                .into_iter(),
            )
            .expect("parse"),
        )
        .expect_err("check must reject --output")
        .to_string();
        assert_eq!(err, "unrecognized option(s) for command: --output");
    }
}

#[cfg(test)]
mod classify_io_observer {
    use super::*;
    use color_eyre::eyre::ensure;

    fn test_classification(reason: &str) -> Classification {
        Classification {
            transition: CompatibilityTransition::NoChange,
            reason: reason.into(),
            requires_candidate: false,
            semantic_boundary_change: false,
        }
    }

    fn temporary_receipt_entries(parent: &Path) -> Result<Vec<PathBuf>> {
        fs::read_dir(parent)?
            .filter_map(|entry| match entry {
                Ok(entry) => {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    (name.starts_with(".perl-core-harness-transition-") && name.ends_with(".tmp"))
                        .then(|| Ok(entry.path()))
                }
                Err(error) => Some(Err(error.into())),
            })
            .collect()
    }

    /// RIPR boundary discriminator for `paths_equal` (`left == right`).
    #[test]
    fn paths_equal_boundary_discriminator() {
        assert_eq!(paths_equal(Path::new("accepted.json"), Path::new("accepted.json")), true);
        assert_eq!(paths_equal(Path::new("accepted.json"), Path::new("compile.json")), false);
    }

    /// RIPR boundary discriminator for accepted schema inequality.
    #[test]
    fn load_accepted_v2_boundary_discriminator() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("accepted.json");
        let schema = "perl_core_harness.compile_baseline.v1";
        assert_eq!(schema != COMPILE_BASELINE_V2_SCHEMA_VERSION, true);
        fs::write(&path, format!(r#"{{"schema_version":"{schema}"}}"#)).expect("write");
        let bytes = fs::read(&path).expect("read");
        assert_eq!(decode_accepted_v2(&bytes, &path).is_err(), true);
    }

    /// RIPR boundary discriminator for run-report schema inequality.
    #[test]
    fn load_run_report_boundary_discriminator() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("compile.json");
        // Minimal JSON that deserializes as RunReport but fails the schema gate.
        let raw = format!(
            r#"{{
              "schema_version":"perl_core_harness.run_report.not_v1",
              "commit":"{commit}",
              "timestamp":"2026-08-11T00:00:00Z",
              "perl_ref":"perl",
              "prepared_tree":"<prepared>",
              "run_tree":"<run>",
              "host_perl":"perl",
              "runner":"test",
              "mode":"compile",
              "profile":"base",
              "harness_status":0,
              "summary":{{
                "files_total":0,
                "files_passed":0,
                "files_failed":0,
                "tap_assertions_total":0,
                "tap_assertions_passed":0
              }},
              "buckets":{{}},
              "file_results":[],
              "failures":[],
              "semantic_boundaries":[]
            }}"#,
            commit = "a".repeat(40)
        );
        fs::write(&path, raw).expect("write");
        let bytes = fs::read(&path).expect("read");
        let report: RunReport = serde_json::from_slice(&bytes).expect("decode");
        assert_eq!(report.schema_version != RUN_REPORT_SCHEMA_VERSION, true);
        assert_eq!(decode_run_report(&bytes, &path).is_err(), true);
    }

    /// RIPR-named observer for output/input path-string collision rejection.
    #[test]
    fn output_path_collision_bail_is_observed() {
        let err = reject_output_input_path_collision(&ClassifyConfig {
            accepted_baseline: PathBuf::from("accepted.json"),
            compile: PathBuf::from("compile.json"),
            output: PathBuf::from("accepted.json"),
            series: None,
            discovery: None,
        })
        .expect_err("alias must fail")
        .to_string();
        assert_eq!(err.contains("output path must not alias"), true);
    }

    /// RIPR-named observer for output/--series path-string collision rejection.
    #[test]
    fn output_series_path_collision_bail_is_observed() {
        let err = reject_output_input_path_collision(&ClassifyConfig {
            accepted_baseline: PathBuf::from("accepted.json"),
            compile: PathBuf::from("compile.json"),
            output: PathBuf::from("series.json"),
            series: Some(PathBuf::from("series.json")),
            discovery: None,
        })
        .expect_err("series alias must fail")
        .to_string();
        assert_eq!(err.contains("--series"), true);
    }

    /// RIPR-named observer for output/--discovery path-string collision rejection.
    #[test]
    fn output_discovery_path_collision_bail_is_observed() {
        let err = reject_output_input_path_collision(&ClassifyConfig {
            accepted_baseline: PathBuf::from("accepted.json"),
            compile: PathBuf::from("compile.json"),
            output: PathBuf::from("discovery.json"),
            series: Some(PathBuf::from("series.json")),
            discovery: Some(PathBuf::from("discovery.json")),
        })
        .expect_err("discovery alias must fail")
        .to_string();
        assert_eq!(err.contains("--discovery"), true);
    }

    /// Discriminator: distinct path strings that hard-link the same file are rejected.
    #[cfg(any(unix, windows))]
    #[test]
    fn output_hard_link_alias_of_accepted_is_rejected() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let accepted = dir.path().join("accepted.json");
        let compile = dir.path().join("compile.json");
        let output = dir.path().join("out.json");
        fs::write(&accepted, b"accepted-bytes")?;
        fs::write(&compile, b"compile-bytes")?;
        fs::hard_link(&accepted, &output)?;
        ensure!(!paths_equal(&output, &accepted), "paths must remain lexically distinct");
        ensure!(same_file_identity(&output, &accepted)?, "hard links must share identity");
        let Err(error) = reject_output_input_path_collision(&ClassifyConfig {
            accepted_baseline: accepted.clone(),
            compile,
            output: output.clone(),
            series: None,
            discovery: None,
        }) else {
            bail!("hard-link alias unexpectedly passed validation");
        };
        ensure!(error.to_string().contains("hard-link alias"), "unexpected error: {error}");
        ensure!(fs::read(&accepted)? == b"accepted-bytes", "accepted evidence changed");
        Ok(())
    }

    /// Negative control: distinct regular files are not treated as hard-link aliases.
    #[cfg(any(unix, windows))]
    #[test]
    fn distinct_output_file_is_not_hard_link_alias() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let accepted = dir.path().join("accepted.json");
        let compile = dir.path().join("compile.json");
        let output = dir.path().join("out.json");
        fs::write(&accepted, b"accepted-bytes")?;
        fs::write(&compile, b"compile-bytes")?;
        fs::write(&output, b"prior-out")?;
        ensure!(!same_file_identity(&output, &accepted)?, "distinct files shared identity");
        reject_output_input_path_collision(&ClassifyConfig {
            accepted_baseline: accepted,
            compile,
            output,
            series: None,
            discovery: None,
        })
    }

    /// A planted alias at the formerly predictable PID-derived sibling is never opened.
    #[cfg(any(unix, windows))]
    #[test]
    fn secure_receipt_write_ignores_preplanted_legacy_hard_link() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let evidence = dir.path().join("accepted.json");
        let output = dir.path().join("out.json");
        let planted = dir.path().join(format!(".out.json.classify-{}.tmp", std::process::id()));
        fs::write(&evidence, b"protected-evidence")?;
        fs::hard_link(&evidence, &planted)?;

        write_classification_receipt(
            &output,
            &test_classification("hard-link plant"),
            &"a".repeat(64),
            &"b".repeat(64),
        )?;

        ensure!(fs::read(&evidence)? == b"protected-evidence", "protected evidence changed");
        ensure!(same_file_identity(&planted, &evidence)?, "attacker-owned alias was removed");
        ensure!(!same_file_identity(&output, &evidence)?, "receipt aliases protected evidence");
        ensure!(temporary_receipt_entries(dir.path())?.is_empty(), "owned temporary remained");
        Ok(())
    }

    /// A planted symbolic link at the formerly predictable sibling is never followed.
    #[cfg(any(unix, windows))]
    #[test]
    fn secure_receipt_write_ignores_preplanted_legacy_symlink() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let evidence = dir.path().join("accepted.json");
        let output = dir.path().join("out.json");
        let planted = dir.path().join(format!(".out.json.classify-{}.tmp", std::process::id()));
        fs::write(&evidence, b"protected-evidence")?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&evidence, &planted)?;
        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_file(&evidence, &planted) {
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314)
            {
                return Ok(());
            }
            return Err(error.into());
        }

        write_classification_receipt(
            &output,
            &test_classification("symbolic-link plant"),
            &"a".repeat(64),
            &"b".repeat(64),
        )?;

        ensure!(fs::read(&evidence)? == b"protected-evidence", "protected evidence changed");
        ensure!(planted.symlink_metadata()?.file_type().is_symlink(), "planted symlink changed");
        ensure!(temporary_receipt_entries(dir.path())?.is_empty(), "owned temporary remained");
        Ok(())
    }

    /// Discriminator: atomic rename onto a hard-linked output path must not
    /// truncate the protected evidence inode (closes check-to-write TOCTOU).
    #[cfg(any(unix, windows))]
    #[test]
    fn atomic_receipt_write_does_not_truncate_hard_linked_evidence() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let evidence = dir.path().join("accepted.json");
        let output = dir.path().join("out.json");
        fs::write(&evidence, b"protected-evidence")?;
        fs::hard_link(&evidence, &output)?;
        write_classification_receipt(
            &output,
            &test_classification("destination replacement"),
            &"a".repeat(64),
            &"b".repeat(64),
        )?;
        ensure!(fs::read(&evidence)? == b"protected-evidence", "protected evidence changed");
        ensure!(
            fs::read_to_string(&output)?.contains("\"command\": \"classify\""),
            "receipt was not persisted"
        );
        ensure!(!same_file_identity(&output, &evidence)?, "output still aliases evidence");
        Ok(())
    }

    /// Existing distinct output is deliberately replaced on Windows and Unix.
    #[test]
    fn secure_receipt_write_replaces_existing_distinct_output() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("out.json");
        fs::write(&output, b"stale receipt")?;
        write_classification_receipt(
            &output,
            &test_classification("replace existing"),
            &"a".repeat(64),
            &"b".repeat(64),
        )?;
        let written = fs::read_to_string(&output)?;
        ensure!(written.contains("\"reason\": \"replace existing\""), "old output survived");
        ensure!(temporary_receipt_entries(dir.path())?.is_empty(), "owned temporary remained");
        Ok(())
    }

    /// Concurrent contenders can only publish complete receipts, never redirect writes.
    #[test]
    fn secure_receipt_write_handles_concurrent_contenders() -> Result<()> {
        use std::sync::{Arc, Barrier};

        let dir = tempfile::tempdir()?;
        let output = dir.path().join("out.json");
        let barrier = Arc::new(Barrier::new(8));
        let mut handles = Vec::new();
        for contender in 0..8 {
            let barrier = Arc::clone(&barrier);
            let output = output.clone();
            handles.push(std::thread::spawn(move || -> Result<()> {
                barrier.wait();
                write_classification_receipt(
                    &output,
                    &test_classification(&format!("contender {contender}")),
                    &"a".repeat(64),
                    &"b".repeat(64),
                )
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(result) => result?,
                Err(_) => bail!("concurrent receipt writer panicked"),
            }
        }
        let receipt: ClassifyReceipt = serde_json::from_str(&fs::read_to_string(&output)?)?;
        ensure!(
            receipt.reason.starts_with("contender "),
            "partial or redirected receipt persisted"
        );
        ensure!(temporary_receipt_entries(dir.path())?.is_empty(), "owned temporary remained");
        Ok(())
    }

    /// A peer that swaps the actual randomized sibling cannot redirect a
    /// write, publish its replacement, or make cleanup unlink that replacement.
    #[cfg(any(unix, windows))]
    #[test]
    fn secure_receipt_write_fails_closed_when_random_sibling_is_swapped() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let evidence = dir.path().join("accepted.json");
        let output = dir.path().join("out.json");
        fs::write(&evidence, b"protected-evidence")?;

        let result = write_classification_receipt_with_hook(
            &output,
            &test_classification("random sibling swap"),
            &"a".repeat(64),
            &"b".repeat(64),
            |temporary_path| {
                ensure!(
                    temporary_path.file_name().and_then(|name| name.to_str()).is_some_and(|name| {
                        name.starts_with(".perl-core-harness-transition-") && name.ends_with(".tmp")
                    }),
                    "hook did not receive the actual randomized sibling"
                );
                fs::remove_file(temporary_path)?;
                fs::hard_link(&evidence, temporary_path)?;
                Ok(())
            },
        );

        let Err(error) = result else {
            bail!("replaced randomized sibling did not fail closed");
        };
        ensure!(error.to_string().contains("no longer identifies"), "unexpected error: {error}");
        ensure!(fs::read(&evidence)? == b"protected-evidence", "protected evidence changed");
        ensure!(!output.exists(), "attacker replacement was published");
        let remaining = temporary_receipt_entries(dir.path())?;
        ensure!(remaining.len() == 1, "attacker replacement was removed or multiplied");
        ensure!(
            same_file_identity(&remaining[0], &evidence)?,
            "remaining entry is not the attacker replacement"
        );
        Ok(())
    }

    /// Unix entry validation must inspect the sibling itself rather than
    /// following a swapped symbolic link back to the still-open inode.
    #[cfg(unix)]
    #[test]
    fn secure_receipt_write_rejects_symlink_to_moved_random_sibling() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("out.json");
        let moved = dir.path().join("attacker-held-original.tmp");

        let result = write_classification_receipt_with_hook(
            &output,
            &test_classification("random sibling symlink swap"),
            &"a".repeat(64),
            &"b".repeat(64),
            |temporary_path| {
                fs::rename(temporary_path, &moved)?;
                std::os::unix::fs::symlink(&moved, temporary_path)?;
                Ok(())
            },
        );

        ensure!(result.is_err(), "symlink replacement did not fail closed");
        ensure!(!output.exists(), "symlink replacement was published");
        ensure!(moved.is_file(), "moved original receipt disappeared");
        let planted = temporary_receipt_entries(dir.path())?;
        ensure!(planted.len() == 1, "planted symlink was removed or multiplied");
        ensure!(
            planted[0].symlink_metadata()?.file_type().is_symlink(),
            "remaining attacker entry is not a symlink"
        );
        Ok(())
    }

    /// An injected pre-persist error does not make cleanup remove a replacement
    /// planted at the actual randomized sibling pathname.
    #[cfg(any(unix, windows))]
    #[test]
    fn secure_receipt_cleanup_preserves_replaced_random_sibling() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("out.json");
        let result = write_classification_receipt_with_hook(
            &output,
            &test_classification("cleanup replacement"),
            &"a".repeat(64),
            &"b".repeat(64),
            |temporary_path| {
                fs::remove_file(temporary_path)?;
                fs::write(temporary_path, b"attacker-sentinel")?;
                bail!("injected pre-persist failure")
            },
        );

        let Err(error) = result else {
            bail!("injected pre-persist failure unexpectedly succeeded");
        };
        ensure!(error.to_string().contains("injected pre-persist failure"));
        ensure!(!output.exists(), "failed write published an output");
        let remaining = temporary_receipt_entries(dir.path())?;
        ensure!(remaining.len() == 1, "attacker replacement was removed or multiplied");
        ensure!(fs::read(&remaining[0])? == b"attacker-sentinel", "attacker entry changed");
        Ok(())
    }

    /// Windows retries only the destination-sharing errors emitted by
    /// MoveFileExW; unrelated permission failures fail immediately.
    #[cfg(windows)]
    #[test]
    fn windows_persist_retry_classifier_is_narrow() -> Result<()> {
        for code in [5, 32, 33] {
            ensure!(
                retryable_windows_persist_error(&std::io::Error::from_raw_os_error(code)),
                "Windows error {code} was not retryable"
            );
        }
        for code in [2, 3, 87, 123] {
            ensure!(
                !retryable_windows_persist_error(&std::io::Error::from_raw_os_error(code)),
                "Windows error {code} was unexpectedly retryable"
            );
        }
        Ok(())
    }

    /// A destination handle that denies delete sharing exhausts the bounded
    /// Windows retry without damaging the destination or leaking our sibling.
    #[cfg(windows)]
    #[test]
    fn secure_receipt_write_bounds_windows_destination_contention() -> Result<()> {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;
        use winapi::um::winnt::{FILE_SHARE_READ, FILE_SHARE_WRITE};

        let dir = tempfile::tempdir()?;
        let output = dir.path().join("out.json");
        fs::write(&output, b"held-destination")?;
        let holder = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&output)?;

        let result = write_classification_receipt(
            &output,
            &test_classification("held destination"),
            &"a".repeat(64),
            &"b".repeat(64),
        );
        ensure!(result.is_err(), "persist unexpectedly bypassed held destination");
        ensure!(fs::read(&output)? == b"held-destination", "held destination changed");
        ensure!(temporary_receipt_entries(dir.path())?.is_empty(), "owned temporary remained");

        drop(holder);
        write_classification_receipt(
            &output,
            &test_classification("after contention"),
            &"a".repeat(64),
            &"b".repeat(64),
        )?;
        ensure!(
            fs::read_to_string(&output)?.contains("\"reason\": \"after contention\""),
            "receipt did not publish after contention cleared"
        );
        Ok(())
    }

    /// Windows identity reopens use std's verbatim-path normalization so an
    /// otherwise supported absolute output path beyond MAX_PATH can publish.
    #[cfg(windows)]
    #[test]
    fn secure_receipt_write_supports_windows_long_output_path() -> Result<()> {
        use std::os::windows::ffi::OsStrExt;

        let dir = tempfile::tempdir()?;
        let mut long_dir = dir.path().to_path_buf();
        while long_dir.as_os_str().encode_wide().count() <= 280 {
            long_dir.push("transition-receipt-long-path-segment");
        }
        if let Err(error) = fs::create_dir_all(&long_dir) {
            // ERROR_FILENAME_EXCED_RANGE means long-path support is disabled by
            // this Windows host policy. That environment cannot execute the
            // integration control; every other creation error is actionable.
            if error.raw_os_error() == Some(206) {
                return Ok(());
            }
            return Err(error).context("creating Windows long-path receipt directory");
        }

        let output = long_dir.join("out.json");
        ensure!(
            output.as_os_str().encode_wide().count() > 260,
            "test output did not exceed MAX_PATH"
        );
        write_classification_receipt(
            &output,
            &test_classification("Windows long output path"),
            &"a".repeat(64),
            &"b".repeat(64),
        )?;

        ensure!(windows_file_identity(&output)?.is_some(), "long-path output identity was absent");
        ensure!(
            fs::read_to_string(&output)?.contains("\"reason\": \"Windows long output path\""),
            "long-path receipt did not publish"
        );
        ensure!(temporary_receipt_entries(&long_dir)?.is_empty(), "owned temporary remained");
        Ok(())
    }

    /// Persist failure identity-checks and removes the exclusively owned sibling.
    #[test]
    fn secure_receipt_write_cleans_up_after_persist_failure() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("output-is-a-directory");
        fs::create_dir(&output)?;
        let result = write_classification_receipt(
            &output,
            &test_classification("must fail"),
            &"a".repeat(64),
            &"b".repeat(64),
        );
        ensure!(result.is_err(), "persisting over a directory unexpectedly succeeded");
        ensure!(output.is_dir(), "failed persistence removed the destination directory");
        ensure!(temporary_receipt_entries(dir.path())?.is_empty(), "owned temporary remained");
        Ok(())
    }

    /// RIPR boundary discriminator for unsupported series schema rejection.
    #[test]
    fn load_series_manifest_schema_boundary_discriminator() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("series.json");
        let schema = "perl_core_harness.comparison_series.not_v1";
        assert_eq!(schema != SERIES_MANIFEST_SCHEMA_VERSION, true);
        fs::write(&path, format!(r#"{{"schema_version":"{schema}"}}"#)).expect("write");
        let err = load_series_manifest(&path)
            .expect_err("unsupported series schema must fail")
            .to_string();
        assert_eq!(
            err,
            format!(
                "unsupported series manifest schema: {schema}; classify I/O accepts {} only",
                SERIES_MANIFEST_SCHEMA_VERSION
            )
        );
    }

    /// RIPR boundary discriminator for accepted.series_id != series.series_id.
    #[test]
    fn bind_series_series_id_boundary_discriminator() {
        let (series, mut accepted) = series_bind_fixture();
        accepted.series_id = "other".into();
        assert_eq!(accepted.series_id != series.series_id, true);
        let err = bind_series_identity(&series, &accepted)
            .expect_err("series_id mismatch must fail")
            .to_string();
        assert_eq!(err, "accepted baseline is not bound to series series: series_id mismatch");
    }

    /// RIPR boundary discriminator for accepted.manifest_hash != series.manifest_hash.
    #[test]
    fn bind_series_manifest_hash_boundary_discriminator() {
        let (series, mut accepted) = series_bind_fixture();
        accepted.manifest_hash = "other-hash".into();
        assert_eq!(accepted.manifest_hash != series.manifest_hash, true);
        let err = bind_series_identity(&series, &accepted)
            .expect_err("manifest_hash mismatch must fail")
            .to_string();
        assert_eq!(err, "accepted baseline is not bound to series series: manifest_hash mismatch");
    }

    /// RIPR boundary discriminator for accepted.file_membership != series.normalized_manifest.
    #[test]
    fn bind_series_file_membership_boundary_discriminator() {
        let (series, mut accepted) = series_bind_fixture();
        accepted.file_membership = vec!["base/9.t".into()];
        assert_eq!(accepted.file_membership != series.normalized_manifest, true);
        let err = bind_series_identity(&series, &accepted)
            .expect_err("file_membership mismatch must fail")
            .to_string();
        assert_eq!(
            err,
            "accepted baseline is not bound to series series: file_membership mismatch"
        );
    }

    /// RIPR boundary discriminator for accepted.profile != series.profile.
    #[test]
    fn bind_series_profile_boundary_discriminator() {
        let (series, mut accepted) = series_bind_fixture();
        accepted.profile = perl_core_harness_types::HarnessProfile::Full;
        assert_eq!(accepted.profile != series.profile, true);
        let err = bind_series_identity(&series, &accepted)
            .expect_err("profile mismatch must fail")
            .to_string();
        assert_eq!(err, "accepted baseline is not bound to series series: profile mismatch");
    }

    /// RIPR boundary discriminator for accepted.runner != series.runner.
    #[test]
    fn bind_series_runner_boundary_discriminator() {
        let (series, mut accepted) = series_bind_fixture();
        accepted.runner = perl_core_harness_types::HarnessRunner::Harness;
        assert_eq!(accepted.runner != series.runner, true);
        let err = bind_series_identity(&series, &accepted)
            .expect_err("runner mismatch must fail")
            .to_string();
        assert_eq!(err, "accepted baseline is not bound to series series: runner mismatch");
    }

    /// RIPR boundary discriminator for accepted.perl_resolved_ref != series.perl_resolved_ref.
    #[test]
    fn bind_series_perl_resolved_ref_boundary_discriminator() {
        let (series, mut accepted) = series_bind_fixture();
        accepted.perl_resolved_ref = "other-perl".into();
        assert_eq!(accepted.perl_resolved_ref != series.perl_resolved_ref, true);
        let err = bind_series_identity(&series, &accepted)
            .expect_err("perl_resolved_ref mismatch must fail")
            .to_string();
        assert_eq!(
            err,
            "accepted baseline is not bound to series series: perl_resolved_ref mismatch"
        );
    }

    /// RIPR boundary discriminator for unsupported discovery schema rejection.
    #[test]
    fn load_discovery_report_schema_boundary_discriminator() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("discovery.json");
        let schema = "perl_core_harness.discovery.not_v1";
        assert_eq!(schema != DISCOVERY_SCHEMA_VERSION, true);
        fs::write(&path, format!(r#"{{"schema_version":"{schema}"}}"#)).expect("write");
        let err = load_discovery_report(&path)
            .expect_err("unsupported discovery schema must fail")
            .to_string();
        assert_eq!(
            err,
            format!(
                "unsupported discovery report schema: {schema}; classify I/O accepts {} only",
                DISCOVERY_SCHEMA_VERSION
            )
        );
    }

    /// RIPR boundary discriminator for report.commit != series.repository_commit.
    #[test]
    fn bind_report_commit_boundary_discriminator() {
        let (series, mut report) = report_bind_fixture();
        report.commit = "b".repeat(40);
        assert_eq!(report.commit != series.repository_commit, true);
        let err = bind_report_to_series(&report, &series)
            .expect_err("commit mismatch must fail")
            .to_string();
        assert_eq!(
            err,
            "compile observation is not bound to series series: repository_commit mismatch"
        );
    }

    /// RIPR boundary discriminator for report.perl_ref != series.perl_resolved_ref.
    #[test]
    fn bind_report_perl_ref_boundary_discriminator() {
        let (series, mut report) = report_bind_fixture();
        report.perl_ref = "other-perl".into();
        assert_eq!(report.perl_ref != series.perl_resolved_ref, true);
        let err = bind_report_to_series(&report, &series)
            .expect_err("perl_ref mismatch must fail")
            .to_string();
        assert_eq!(
            err,
            "compile observation is not bound to series series: perl_resolved_ref mismatch"
        );
    }

    /// RIPR boundary discriminator for report.runner != series.runner.
    #[test]
    fn bind_report_runner_boundary_discriminator() {
        let (series, mut report) = report_bind_fixture();
        report.runner = perl_core_harness_types::HarnessRunner::Harness;
        assert_eq!(report.runner != series.runner, true);
        let err = bind_report_to_series(&report, &series)
            .expect_err("runner mismatch must fail")
            .to_string();
        assert_eq!(err, "compile observation is not bound to series series: runner mismatch");
    }

    /// RIPR boundary discriminator for report.profile != series.profile.
    #[test]
    fn bind_report_profile_boundary_discriminator() {
        let (series, mut report) = report_bind_fixture();
        report.profile = perl_core_harness_types::HarnessProfile::Full;
        assert_eq!(report.profile != series.profile, true);
        let err = bind_report_to_series(&report, &series)
            .expect_err("profile mismatch must fail")
            .to_string();
        assert_eq!(err, "compile observation is not bound to series series: profile mismatch");
    }

    fn report_bind_fixture() -> (SeriesManifest, RunReport) {
        let (series, _) = series_bind_fixture();
        let report = RunReport {
            schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
            commit: series.repository_commit.clone(),
            timestamp: "2026-08-11T00:00:00Z".into(),
            perl_ref: series.perl_resolved_ref.clone(),
            prepared_tree: "<prepared>".into(),
            run_tree: "<run>".into(),
            host_perl: "perl".into(),
            runner: series.runner,
            mode: perl_core_harness_types::HarnessMode::Compile,
            profile: series.profile,
            harness_status: Some(0),
            summary: perl_core_harness_types::RunSummary {
                files_total: 1,
                files_passed: 1,
                files_failed: 0,
                tap_assertions_total: 1,
                tap_assertions_passed: 1,
            },
            buckets: BTreeMap::new(),
            file_results: vec![perl_core_harness_types::RunFileResult {
                path: "base/0.t".into(),
                status: perl_core_harness_types::RunnerStatus::Pass,
                assertions_passed: 1,
                assertions_total: 1,
            }],
            failures: Vec::new(),
            semantic_boundaries: Vec::new(),
        };
        (series, report)
    }

    /// RIPR boundary discriminator for discovery.schema_version != series.harness_schema_version.
    #[test]
    fn bind_discovery_schema_boundary_discriminator() {
        let (series, mut discovery) = discovery_bind_fixture();
        discovery.schema_version = "perl_core_harness.discovery.not_v1".into();
        assert_eq!(discovery.schema_version != series.harness_schema_version, true);
        let err = bind_discovery_to_series(&discovery, &series)
            .expect_err("schema mismatch must fail")
            .to_string();
        assert_eq!(
            err,
            "discovery report is not bound to series series: harness_schema_version mismatch"
        );
    }

    /// RIPR boundary discriminator for discovery.profile != series.profile.
    #[test]
    fn bind_discovery_profile_boundary_discriminator() {
        let (series, mut discovery) = discovery_bind_fixture();
        discovery.profile = perl_core_harness_types::HarnessProfile::Full;
        assert_eq!(discovery.profile != series.profile, true);
        let err = bind_discovery_to_series(&discovery, &series)
            .expect_err("profile mismatch must fail")
            .to_string();
        assert_eq!(err, "discovery report is not bound to series series: profile mismatch");
    }

    /// RIPR boundary discriminator for discovery.runner != series.runner.
    #[test]
    fn bind_discovery_runner_boundary_discriminator() {
        let (series, mut discovery) = discovery_bind_fixture();
        discovery.runner = perl_core_harness_types::HarnessRunner::Harness;
        assert_eq!(discovery.runner != series.runner, true);
        let err = bind_discovery_to_series(&discovery, &series)
            .expect_err("runner mismatch must fail")
            .to_string();
        assert_eq!(err, "discovery report is not bound to series series: runner mismatch");
    }

    /// RIPR boundary discriminator for discovery.perl_ref != series.perl_resolved_ref.
    #[test]
    fn bind_discovery_perl_ref_boundary_discriminator() {
        let (series, mut discovery) = discovery_bind_fixture();
        discovery.perl_ref = "other-perl".into();
        assert_eq!(discovery.perl_ref != series.perl_resolved_ref, true);
        let err = bind_discovery_to_series(&discovery, &series)
            .expect_err("perl_ref mismatch must fail")
            .to_string();
        assert_eq!(
            err,
            "discovery report is not bound to series series: perl_resolved_ref mismatch"
        );
    }

    /// RIPR boundary discriminator for discovery.commit != series.repository_commit.
    #[test]
    fn bind_discovery_commit_boundary_discriminator() {
        let (series, mut discovery) = discovery_bind_fixture();
        discovery.commit = "b".repeat(40);
        assert_eq!(discovery.commit != series.repository_commit, true);
        let err = bind_discovery_to_series(&discovery, &series)
            .expect_err("commit mismatch must fail")
            .to_string();
        assert_eq!(
            err,
            "discovery report is not bound to series series: repository_commit mismatch"
        );
    }

    /// RIPR boundary discriminator for discovery paths != series.normalized_manifest.
    #[test]
    fn bind_discovery_membership_boundary_discriminator() {
        let (series, mut discovery) = discovery_bind_fixture();
        discovery.tests = vec![perl_core_harness_types::DiscoveredTest {
            path: "base/9.t".into(),
            root: "base".into(),
        }];
        let mut paths: Vec<String> = discovery.tests.iter().map(|t| t.path.clone()).collect();
        paths.sort();
        assert_eq!(paths != series.normalized_manifest, true);
        let err = bind_discovery_to_series(&discovery, &series)
            .expect_err("membership mismatch must fail")
            .to_string();
        assert_eq!(
            err,
            "discovery report is not bound to series series: normalized_manifest mismatch"
        );
    }

    fn discovery_bind_fixture() -> (SeriesManifest, DiscoveryReport) {
        let (series, _) = series_bind_fixture();
        let discovery = DiscoveryReport {
            schema_version: DISCOVERY_SCHEMA_VERSION.into(),
            commit: series.repository_commit.clone(),
            timestamp: "2026-08-11T00:00:00Z".into(),
            perl_ref: series.perl_resolved_ref.clone(),
            prepared_tree: "<prepared>".into(),
            host_perl: "perl".into(),
            runner: series.runner,
            profile: series.profile,
            tests: series
                .normalized_manifest
                .iter()
                .map(|path| perl_core_harness_types::DiscoveredTest {
                    path: path.clone(),
                    root: "base".into(),
                })
                .collect(),
        };
        (series, discovery)
    }

    fn series_bind_fixture() -> (SeriesManifest, CompileBaselineV2) {
        let series = SeriesManifest {
            schema_version: SERIES_MANIFEST_SCHEMA_VERSION.to_string(),
            series_id: "series".into(),
            profile: perl_core_harness_types::HarnessProfile::Base,
            profile_roots: vec!["base".into()],
            repository_commit: "a".repeat(40),
            perl_requested_ref: "perl".into(),
            perl_resolved_ref: "perl".into(),
            runner: perl_core_harness_types::HarnessRunner::Test,
            normalized_manifest: vec!["base/0.t".into()],
            manifest_hash: "manifest".into(),
            preparation_receipt_id: "prepare".into(),
            preparation_receipt_digest: "sha256:prep".into(),
            harness_schema_version: "perl_core_harness.discovery.v1".into(),
            compiler_subject_identity: "compiler".into(),
            invocation_identity: "invocation".into(),
            capability_identity: "capability".into(),
            environment_identity: "environment".into(),
            normalization_version: "path-normalization.v1".into(),
            created_at: "2026-08-11T00:00:00Z".into(),
            replaces_series_id: None,
            change_reason: None,
        };
        let accepted = CompileBaselineV2 {
            schema_version: COMPILE_BASELINE_V2_SCHEMA_VERSION.into(),
            report_schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
            series_id: "series".into(),
            manifest_hash: "manifest".into(),
            repository_commit: "a".repeat(40),
            perl_resolved_ref: "perl".into(),
            preparation_receipt_id: "prepare".into(),
            compiler_subject_identity: "compiler".into(),
            invocation_identity: "invocation".into(),
            capability_identity: "capability".into(),
            environment_identity: "environment".into(),
            source_report_digest: "digest".into(),
            accepted_transition_id: None,
            evidence_bundle: None,
            mode: perl_core_harness_types::HarnessMode::Compile,
            profile: perl_core_harness_types::HarnessProfile::Base,
            runner: perl_core_harness_types::HarnessRunner::Test,
            file_membership: vec!["base/0.t".into()],
            files_total: 1,
            files_passed: 1,
            files_failed: 0,
            tap_assertions_total: 1,
            tap_assertions_passed: 1,
            buckets: BTreeMap::new(),
            expected_failures: Vec::new(),
            file_results: vec![perl_core_harness_types::RunFileResult {
                path: "base/0.t".into(),
                status: perl_core_harness_types::RunnerStatus::Pass,
                assertions_passed: 1,
                assertions_total: 1,
            }],
            semantic_boundaries: Vec::new(),
            boundary_retirements: Vec::new(),
        };
        (series, accepted)
    }

    /// RIPR boundary discriminator for digest mismatch rejection.
    #[test]
    fn verify_classify_receipt_digest_mismatch_is_observed() {
        let expected = Classification {
            transition: CompatibilityTransition::NoChange,
            reason: "complete observation exactly matches the accepted v2 ratchet".into(),
            requires_candidate: false,
            semantic_boundary_change: false,
        };
        let receipt = ClassifyReceipt {
            schema_version: CLASSIFY_RECEIPT_SCHEMA_VERSION.to_string(),
            command: "classify".to_string(),
            transition: expected.transition,
            reason: expected.reason.clone(),
            requires_candidate: expected.requires_candidate,
            semantic_boundary_change: expected.semantic_boundary_change,
            accepted_baseline_digest: "sha256:aaaa".to_string(),
            compile_digest: "sha256:bbbb".to_string(),
            claim_boundary: CLASSIFY_CLAIM_BOUNDARY.to_string(),
        };
        let err = verify_classify_receipt(&receipt, &expected, "sha256:cccc", "sha256:bbbb")
            .expect_err("digest mismatch must fail")
            .to_string();
        assert_eq!(
            err.contains("accepted_baseline_digest does not match current evidence bytes"),
            true
        );
    }
}
