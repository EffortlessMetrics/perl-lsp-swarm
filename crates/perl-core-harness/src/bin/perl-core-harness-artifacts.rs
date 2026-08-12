#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

use color_eyre::eyre::{Context, Result, bail};
use perl_core_harness_types::{
    HarnessMode, HarnessProfile, HarnessRunner, ObservedSemanticBoundary,
    RUN_REPORT_SCHEMA_VERSION, RUNNER_RECORD_SCHEMA_VERSION, RunFailure, RunFileResult, RunReport,
    RunnerRecord, RunnerStatus, SemanticBoundaryConfidence, SemanticBoundaryDisposition,
    SemanticBoundaryLockScope, SemanticBoundaryRecord,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DISCOVERY_RAW_SCHEMA_VERSION: &str = "perl_core_harness.discovery_raw.v2";
const RAW_STREAM_ENCODING: &str = "hex";
const RAW_STREAM_MAX_BYTES: usize = 4 * 1024 * 1024;

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "usage: perl-core-harness-artifacts <capture-discovery|derive-runner-records|check-runner-records> [options]"
        )
    })?;
    let options = Options::parse(args)?;
    match command.as_str() {
        "capture-discovery" => capture_discovery(CaptureDiscoveryConfig::from_options(options)?),
        "derive-runner-records" => {
            derive_runner_records(DeriveRunnerRecordsConfig::from_options(options)?)
        }
        "check-runner-records" => {
            check_runner_records(CheckRunnerRecordsConfig::from_options(options)?)
        }
        _ => bail!("unknown perl-core-harness-artifacts command: {command}"),
    }
}

#[derive(Debug, Default)]
struct Options {
    values: BTreeMap<String, VecDeque<String>>,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut args = args.peekable();
        let mut values = BTreeMap::<String, VecDeque<String>>::new();
        while let Some(flag) = args.next() {
            if !flag.starts_with("--") {
                bail!("expected an option beginning with --, found {flag}");
            }
            let value =
                args.next().ok_or_else(|| color_eyre::eyre::eyre!("missing value for {flag}"))?;
            if value.starts_with("--") {
                bail!("missing value for {flag}; found option {value}");
            }
            values.entry(flag).or_default().push_back(value);
        }
        Ok(Self { values })
    }

    fn required(&mut self, flag: &str) -> Result<String> {
        let value =
            self.values.get_mut(flag).and_then(VecDeque::pop_front).ok_or_else(|| {
                color_eyre::eyre::eyre!("required option {flag} was not supplied")
            })?;
        if self.values.get(flag).is_some_and(|values| !values.is_empty()) {
            bail!("option {flag} may be supplied only once");
        }
        self.values.remove(flag);
        Ok(value)
    }

    fn optional(&mut self, flag: &str) -> Result<Option<String>> {
        let Some(values) = self.values.get_mut(flag) else {
            return Ok(None);
        };
        let value = values
            .pop_front()
            .ok_or_else(|| color_eyre::eyre::eyre!("option {flag} has no value"))?;
        if !values.is_empty() {
            bail!("option {flag} may be supplied only once");
        }
        self.values.remove(flag);
        Ok(Some(value))
    }

    fn repeated(&mut self, flag: &str) -> Vec<String> {
        self.values.remove(flag).map(|values| values.into_iter().collect()).unwrap_or_default()
    }

    fn finish(self) -> Result<()> {
        if self.values.is_empty() {
            return Ok(());
        }
        bail!(
            "unrecognized option(s): {}",
            self.values.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    }
}

#[derive(Debug)]
struct CaptureDiscoveryConfig {
    perl_tree: PathBuf,
    host_perl: PathBuf,
    runner: HarnessRunner,
    profile: HarnessProfile,
    output: PathBuf,
}

impl CaptureDiscoveryConfig {
    fn from_options(mut options: Options) -> Result<Self> {
        let config = Self {
            perl_tree: PathBuf::from(options.required("--perl-tree")?),
            host_perl: PathBuf::from(options.required("--host-perl")?),
            runner: parse_runner(&options.required("--runner")?)?,
            profile: parse_profile(&options.required("--profile")?)?,
            output: PathBuf::from(options.required("--output")?),
        };
        options.finish()?;
        Ok(config)
    }
}

#[derive(Debug)]
struct DeriveRunnerRecordsConfig {
    reports: Vec<PathBuf>,
    output: PathBuf,
    boundaries_output: PathBuf,
}

impl DeriveRunnerRecordsConfig {
    fn from_options(mut options: Options) -> Result<Self> {
        let reports =
            options.repeated("--report").into_iter().map(PathBuf::from).collect::<Vec<_>>();
        if reports.is_empty() {
            bail!("derive-runner-records requires at least one --report");
        }
        let config = Self {
            reports,
            output: PathBuf::from(options.required("--output")?),
            boundaries_output: PathBuf::from(options.required("--boundaries-output")?),
        };
        options.finish()?;
        Ok(config)
    }
}

#[derive(Debug)]
struct CheckRunnerRecordsConfig {
    reports: Vec<PathBuf>,
    records: PathBuf,
    boundaries: Option<PathBuf>,
}

impl CheckRunnerRecordsConfig {
    fn from_options(mut options: Options) -> Result<Self> {
        let reports =
            options.repeated("--report").into_iter().map(PathBuf::from).collect::<Vec<_>>();
        if reports.is_empty() {
            bail!("check-runner-records requires at least one --report");
        }
        let config = Self {
            reports,
            records: PathBuf::from(options.required("--records")?),
            boundaries: options.optional("--boundaries")?.map(PathBuf::from),
        };
        options.finish()?;
        Ok(config)
    }
}

#[derive(Debug)]
struct CapturedStream {
    retained: Vec<u8>,
    observed_byte_length: u64,
    full_sha256: String,
    truncated: bool,
    capture_error: Option<String>,
}

impl CapturedStream {
    fn empty() -> Self {
        Self {
            retained: Vec::new(),
            observed_byte_length: 0,
            full_sha256: sha256_digest(&[]),
            truncated: false,
            capture_error: None,
        }
    }

    fn failed(message: &str) -> Self {
        Self {
            capture_error: Some(message.to_string()),
            ..Self::empty()
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RawByteStream {
    encoding: String,
    limit_bytes: usize,
    observed_byte_length: u64,
    retained_byte_length: usize,
    sha256: String,
    retained_sha256: String,
    payload_hex: String,
    truncated: bool,
    capture_error: Option<String>,
}

impl RawByteStream {
    fn from_capture(capture: CapturedStream, limit_bytes: usize) -> Self {
        let retained_sha256 = sha256_digest(&capture.retained);
        Self {
            encoding: RAW_STREAM_ENCODING.to_string(),
            limit_bytes,
            observed_byte_length: capture.observed_byte_length,
            retained_byte_length: capture.retained.len(),
            sha256: capture.full_sha256,
            retained_sha256,
            payload_hex: encode_hex(&capture.retained),
            truncated: capture.truncated,
            capture_error: capture.capture_error,
        }
    }

    fn empty(limit_bytes: usize) -> Self {
        Self::from_capture(CapturedStream::empty(), limit_bytes)
    }

    fn bytes(&self) -> Result<Vec<u8>> {
        if self.encoding != RAW_STREAM_ENCODING {
            bail!("unsupported raw stream encoding: {}", self.encoding);
        }
        decode_hex(&self.payload_hex)
    }

    fn validate(&self) -> Result<()> {
        let bytes = self.bytes()?;
        if bytes.len() != self.retained_byte_length {
            bail!(
                "raw stream retained length mismatch: declared {}, decoded {}",
                self.retained_byte_length,
                bytes.len()
            );
        }
        if self.retained_byte_length > self.limit_bytes {
            bail!("raw stream retained bytes exceed the declared capture limit");
        }
        let retained_u64 = u64::try_from(self.retained_byte_length).unwrap_or(u64::MAX);
        if self.observed_byte_length < retained_u64 {
            bail!("raw stream observed length is smaller than its retained length");
        }
        if self.truncated != (self.observed_byte_length > retained_u64) {
            bail!("raw stream truncation flag disagrees with observed and retained lengths");
        }
        if self.truncated && self.retained_byte_length != self.limit_bytes {
            bail!("truncated raw stream did not retain exactly its configured byte limit");
        }
        let retained_digest = sha256_digest(&bytes);
        if retained_digest != self.retained_sha256 {
            bail!("raw stream retained digest mismatch");
        }
        if !self.truncated && self.capture_error.is_none() && self.sha256 != retained_digest {
            bail!("complete raw stream digest mismatch");
        }
        Ok(())
    }

    fn complete(&self) -> bool {
        !self.truncated && self.capture_error.is_none()
    }

    fn utf8_text(&self) -> Result<Option<String>> {
        if !self.complete() {
            return Ok(None);
        }
        Ok(String::from_utf8(self.bytes()?).ok())
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum DiscoveryProcessOutcome {
    Exited { code: i32 },
    TerminatedWithoutCode,
    SpawnFailed { error: String },
    WaitFailed { error: String },
    CaptureSetupFailed { stream: String },
}

impl DiscoveryProcessOutcome {
    const fn succeeded(&self) -> bool {
        matches!(self, Self::Exited { code: 0 })
    }

    fn summary(&self) -> String {
        match self {
            Self::Exited { code } => format!("exited with status {code}"),
            Self::TerminatedWithoutCode => "terminated without an exit code".to_string(),
            Self::SpawnFailed { .. } => "spawn failed".to_string(),
            Self::WaitFailed { .. } => "wait failed".to_string(),
            Self::CaptureSetupFailed { stream } => {
                format!("failed to attach bounded {stream} capture")
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DiscoveryRawEnvelope {
    schema_version: String,
    runner: HarnessRunner,
    profile: HarnessProfile,
    host_perl: String,
    working_directory: String,
    argv: Vec<String>,
    process: DiscoveryProcessOutcome,
    stdout: RawByteStream,
    stderr: RawByteStream,
}

impl DiscoveryRawEnvelope {
    fn validate(&self) -> Result<()> {
        if self.schema_version != DISCOVERY_RAW_SCHEMA_VERSION {
            bail!("unsupported raw discovery schema: {}", self.schema_version);
        }
        self.stdout.validate().context("validating raw discovery stdout")?;
        self.stderr.validate().context("validating raw discovery stderr")?;
        for error in match &self.process {
            DiscoveryProcessOutcome::SpawnFailed { error }
            | DiscoveryProcessOutcome::WaitFailed { error } => Some(error),
            DiscoveryProcessOutcome::CaptureSetupFailed { stream } => Some(stream),
            DiscoveryProcessOutcome::Exited { .. }
            | DiscoveryProcessOutcome::TerminatedWithoutCode => None,
        } {
            if error.trim().is_empty() {
                bail!("raw discovery process outcome contains an empty failure detail");
            }
        }
        Ok(())
    }

    fn complete_success(&self) -> bool {
        self.process.succeeded() && self.stdout.complete() && self.stderr.complete()
    }
}

fn capture_stream<R: Read>(mut reader: R, limit_bytes: usize) -> CapturedStream {
    let mut retained = Vec::with_capacity(limit_bytes.min(64 * 1024));
    let mut observed_byte_length = 0u64;
    let mut full_hasher = Sha256::new();
    let mut capture_error = None;
    let mut buffer = [0u8; 16 * 1024];

    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                let read_u64 = u64::try_from(read).unwrap_or(u64::MAX);
                observed_byte_length = observed_byte_length.saturating_add(read_u64);
                full_hasher.update(&buffer[..read]);
                let remaining = limit_bytes.saturating_sub(retained.len());
                let keep = remaining.min(read);
                retained.extend_from_slice(&buffer[..keep]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                capture_error = Some(error.to_string());
                break;
            }
        }
    }

    let retained_u64 = u64::try_from(retained.len()).unwrap_or(u64::MAX);
    CapturedStream {
        retained,
        observed_byte_length,
        full_sha256: format!("sha256:{:x}", full_hasher.finalize()),
        truncated: observed_byte_length > retained_u64,
        capture_error,
    }
}

fn run_bounded_command(mut command: Command) -> (DiscoveryProcessOutcome, RawByteStream, RawByteStream) {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return (
                DiscoveryProcessOutcome::SpawnFailed { error: error.to_string() },
                RawByteStream::empty(RAW_STREAM_MAX_BYTES),
                RawByteStream::empty(RAW_STREAM_MAX_BYTES),
            );
        }
    };

    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return (
            DiscoveryProcessOutcome::CaptureSetupFailed { stream: "stdout".to_string() },
            RawByteStream::empty(RAW_STREAM_MAX_BYTES),
            RawByteStream::empty(RAW_STREAM_MAX_BYTES),
        );
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return (
            DiscoveryProcessOutcome::CaptureSetupFailed { stream: "stderr".to_string() },
            RawByteStream::empty(RAW_STREAM_MAX_BYTES),
            RawByteStream::empty(RAW_STREAM_MAX_BYTES),
        );
    };

    let (status_result, stdout_capture, stderr_capture) = std::thread::scope(|scope| {
        let stdout_handle = scope.spawn(|| capture_stream(stdout, RAW_STREAM_MAX_BYTES));
        let stderr_handle = scope.spawn(|| capture_stream(stderr, RAW_STREAM_MAX_BYTES));
        let status_result = child.wait();
        if status_result.is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let stdout_capture = stdout_handle
            .join()
            .unwrap_or_else(|_| CapturedStream::failed("stdout capture thread panicked"));
        let stderr_capture = stderr_handle
            .join()
            .unwrap_or_else(|_| CapturedStream::failed("stderr capture thread panicked"));
        (status_result, stdout_capture, stderr_capture)
    });

    let process = match status_result {
        Ok(status) => match status.code() {
            Some(code) => DiscoveryProcessOutcome::Exited { code },
            None => DiscoveryProcessOutcome::TerminatedWithoutCode,
        },
        Err(error) => DiscoveryProcessOutcome::WaitFailed { error: error.to_string() },
    };
    (
        process,
        RawByteStream::from_capture(stdout_capture, RAW_STREAM_MAX_BYTES),
        RawByteStream::from_capture(stderr_capture, RAW_STREAM_MAX_BYTES),
    )
}

fn capture_discovery(config: CaptureDiscoveryConfig) -> Result<()> {
    let perl_tree = fs::canonicalize(&config.perl_tree).with_context(|| {
        format!("canonicalizing prepared Perl tree {}", config.perl_tree.display())
    })?;
    if !perl_tree.is_dir() {
        bail!("prepared Perl tree is not a directory: {}", perl_tree.display());
    }
    let t_dir = perl_tree.join("t");
    let script = t_dir.join(config.runner.script_name());
    if !script.is_file() {
        bail!("prepared Perl tree is missing runner script {}", script.display());
    }
    reject_output_aliases(std::slice::from_ref(&script), std::slice::from_ref(&config.output))?;

    let script_name = script
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| color_eyre::eyre::eyre!("runner script has no UTF-8 file name"))?;
    let profile_args = discovery_profile_args(&t_dir, config.runner, config.profile)?;
    let mut argv = vec![script_name.to_string(), "--dumptests".to_string()];
    argv.extend(profile_args.iter().cloned());

    let mut command = Command::new(&config.host_perl);
    command.current_dir(&t_dir).args(&argv);
    command.env("LC_ALL", "C");
    sanitize_perl_env(&mut command);

    let (process, stdout, stderr) = run_bounded_command(command);
    let envelope = DiscoveryRawEnvelope {
        schema_version: DISCOVERY_RAW_SCHEMA_VERSION.to_string(),
        runner: config.runner,
        profile: config.profile,
        host_perl: config.host_perl.display().to_string(),
        working_directory: t_dir.display().to_string(),
        argv,
        process,
        stdout,
        stderr,
    };
    envelope.validate()?;
    write_json(&config.output, &envelope)?;
    if !envelope.complete_success() {
        bail!(
            "upstream discovery did not produce complete byte-exact evidence ({}; stdout_complete={}; stderr_complete={}); raw evidence was written to {}",
            envelope.process.summary(),
            envelope.stdout.complete(),
            envelope.stderr.complete(),
            config.output.display()
        );
    }
    let stdout = envelope.stdout.utf8_text()?.ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "upstream discovery stdout is not valid UTF-8; byte-exact raw evidence is {}",
            config.output.display()
        )
    })?;
    if !stdout.lines().any(|line| line.trim().ends_with(".t")) {
        bail!(
            "upstream discovery succeeded but emitted no .t paths; raw evidence is {}",
            config.output.display()
        );
    }
    Ok(())
}

fn discovery_profile_args(
    t_dir: &Path,
    runner: HarnessRunner,
    profile: HarnessProfile,
) -> Result<Vec<String>> {
    match runner {
        HarnessRunner::Harness => {
            Ok(profile.roots().iter().map(|root| format!("{root}/*.t")).collect())
        }
        HarnessRunner::Test => {
            let mut paths = Vec::new();
            for root in profile.roots() {
                collect_test_paths(t_dir, &t_dir.join(root), &mut paths)?;
            }
            paths.sort();
            paths.dedup();
            if paths.is_empty() {
                bail!("profile {profile} contains no discoverable .t files");
            }
            Ok(paths)
        }
    }
}

fn collect_test_paths(t_dir: &Path, directory: &Path, paths: &mut Vec<String>) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)
        .with_context(|| format!("reading profile directory {}", directory.display()))?
    {
        let entry = entry.with_context(|| format!("reading entry in {}", directory.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading file type for {}", path.display()))?;
        if file_type.is_dir() {
            collect_test_paths(t_dir, &path, paths)?;
        } else if file_type.is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("t")
        {
            let relative = path
                .strip_prefix(t_dir)
                .with_context(|| format!("normalizing test path {}", path.display()))?;
            paths.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

fn derive_runner_records(config: DeriveRunnerRecordsConfig) -> Result<()> {
    reject_output_aliases(
        &config.reports,
        &[config.output.clone(), config.boundaries_output.clone()],
    )?;
    let reports = read_reports(&config.reports)?;
    validate_report_collection(&reports)?;
    let expected = records_from_reports(&reports)?;
    write_json_lines(&config.output, &expected)?;
    let boundaries = compile_boundaries(&reports)?;
    write_json(&config.boundaries_output, &boundaries)?;
    validate_record_files(&reports, &config.output, Some(&config.boundaries_output))
}

fn check_runner_records(config: CheckRunnerRecordsConfig) -> Result<()> {
    let reports = read_reports(&config.reports)?;
    validate_report_collection(&reports)?;
    validate_record_files(&reports, &config.records, config.boundaries.as_deref())
}

fn read_reports(paths: &[PathBuf]) -> Result<Vec<RunReport>> {
    paths
        .iter()
        .map(|path| {
            let raw = fs::read_to_string(path)
                .with_context(|| format!("reading run report {}", path.display()))?;
            let report: RunReport = serde_json::from_str(&raw)
                .with_context(|| format!("decoding run report {}", path.display()))?;
            validate_report(&report).with_context(|| format!("validating {}", path.display()))?;
            Ok(report)
        })
        .collect()
}

fn validate_report_collection(reports: &[RunReport]) -> Result<()> {
    let first =
        reports.first().ok_or_else(|| color_eyre::eyre::eyre!("no run reports were supplied"))?;
    let expected_membership = report_membership(first);
    let mut modes = BTreeSet::new();
    for report in reports {
        if report.commit != first.commit
            || report.perl_ref != first.perl_ref
            || report.runner != first.runner
            || report.profile != first.profile
            || report.prepared_tree != first.prepared_tree
            || report.host_perl != first.host_perl
        {
            bail!(
                "run reports do not describe one measured subject: commit, Perl ref, runner, profile, prepared tree, and host Perl must match"
            );
        }
        if !modes.insert(report.mode.as_str()) {
            bail!("multiple run reports declare {} mode", report.mode);
        }
        if report_membership(report) != expected_membership {
            bail!("run report membership differs across modes for the measured subject");
        }
    }
    Ok(())
}

fn report_membership(report: &RunReport) -> BTreeSet<String> {
    report.file_results.iter().map(|result| result.path.clone()).collect()
}

fn validate_report(report: &RunReport) -> Result<()> {
    if report.schema_version != RUN_REPORT_SCHEMA_VERSION {
        bail!("unsupported run report schema: {}", report.schema_version);
    }
    for (label, value) in [
        ("commit", report.commit.as_str()),
        ("Perl ref", report.perl_ref.as_str()),
        ("prepared tree", report.prepared_tree.as_str()),
        ("host Perl", report.host_perl.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("run report has an empty {label}");
        }
    }

    let mut files = BTreeMap::<String, &RunFileResult>::new();
    for result in &report.file_results {
        validate_test_path(&result.path)?;
        if result.assertions_passed > result.assertions_total {
            bail!("{} passes more assertions than it declares", result.path);
        }
        if files.insert(result.path.clone(), result).is_some() {
            bail!("run report contains duplicate file result {}", result.path);
        }
    }
    if files.len() != report.summary.files_total {
        bail!(
            "run report summary declares {} files but contains {} results",
            report.summary.files_total,
            files.len()
        );
    }
    let passed =
        report.file_results.iter().filter(|result| result.status == RunnerStatus::Pass).count();
    let failed = report.file_results.len().saturating_sub(passed);
    if passed != report.summary.files_passed || failed != report.summary.files_failed {
        bail!("run report file status counts do not match its summary");
    }
    let assertions_total: usize =
        report.file_results.iter().map(|result| result.assertions_total).sum();
    let assertions_passed: usize =
        report.file_results.iter().map(|result| result.assertions_passed).sum();
    if assertions_total != report.summary.tap_assertions_total
        || assertions_passed != report.summary.tap_assertions_passed
    {
        bail!("run report assertion counts do not match its file results");
    }

    let mut failures = BTreeMap::<String, &RunFailure>::new();
    for failure in &report.failures {
        validate_test_path(&failure.path)?;
        if failure.bucket.trim().is_empty()
            || failure.phase.trim().is_empty()
            || failure.first_diagnostic.trim().is_empty()
        {
            bail!("failure {} has incomplete typed evidence", failure.path);
        }
        let result = files.get(&failure.path).ok_or_else(|| {
            color_eyre::eyre::eyre!("failure {} is absent from file results", failure.path)
        })?;
        if result.status != RunnerStatus::Fail {
            bail!("passing file {} carries failure evidence", failure.path);
        }
        if failures.insert(failure.path.clone(), failure).is_some() {
            bail!("run report contains duplicate failure evidence for {}", failure.path);
        }
    }
    for result in &report.file_results {
        let has_failure = failures.contains_key(&result.path);
        if (result.status == RunnerStatus::Fail) != has_failure {
            bail!("file {} status and failure evidence disagree", result.path);
        }
    }

    validate_semantic_boundary_inventory(&report.semantic_boundaries)?;
    for boundary in &report.semantic_boundaries {
        if !files.contains_key(&boundary.path) {
            bail!("semantic boundary path {} is absent from file results", boundary.path);
        }
    }
    Ok(())
}

fn validate_semantic_boundary_inventory(boundaries: &[ObservedSemanticBoundary]) -> Result<()> {
    let mut violations = Vec::new();
    let mut keys = BTreeSet::new();
    for boundary in boundaries {
        let mut add = |message: &str| {
            violations.push(format!("{} {}: {message}", boundary.path, boundary.id));
        };

        if !keys.insert(boundary_key(boundary)) {
            add("semantic boundary inventory contains a duplicate key");
        }
        if boundary.path.trim().is_empty() {
            add("semantic boundary has an empty path");
        } else if validate_test_path(&boundary.path).is_err() {
            add("semantic boundary has an invalid normalized test path");
        }
        if boundary.id.trim().is_empty() {
            add("semantic boundary has an empty stable id");
        }
        if boundary.reason.trim().is_empty() {
            add("semantic boundary has an empty reason");
        }
        if boundary.source_kind.trim().is_empty() {
            add("semantic boundary has an empty source kind");
        }
        if boundary.owner_workstream.trim().is_empty() {
            add("semantic boundary has no owning workstream");
        }
        if boundary.supporting_test.trim().is_empty() {
            add("semantic boundary has no supporting test");
        }
        if boundary.source_span.start > boundary.source_span.end {
            add("semantic boundary source span is reversed");
        }

        match boundary.disposition {
            SemanticBoundaryDisposition::SourceLockedCompatibility => {
                if boundary.lock_scope != SemanticBoundaryLockScope::PathAndSource {
                    add("source-locked compatibility boundary must use a path_and_source lock");
                }
                if boundary.confidence != SemanticBoundaryConfidence::Exact {
                    add("source-locked compatibility boundary must have exact confidence");
                }
                if boundary.blocks_compilation {
                    add("source-locked compatibility boundary must not block compilation");
                }
            }
            SemanticBoundaryDisposition::Unknown => {
                add("unknown semantic boundary disposition is not admissible");
                if boundary.confidence != SemanticBoundaryConfidence::Unresolved {
                    add("unknown semantic boundary must have unresolved confidence");
                }
                if !boundary.blocks_compilation {
                    add("unknown semantic boundary must block compilation");
                }
            }
            SemanticBoundaryDisposition::Unsupported => {
                if boundary.confidence != SemanticBoundaryConfidence::Unresolved {
                    add("unsupported semantic boundary must have unresolved confidence");
                }
                if !boundary.blocks_compilation {
                    add("unsupported semantic boundary must block compilation");
                }
            }
            SemanticBoundaryDisposition::ImplementedStatic
            | SemanticBoundaryDisposition::StaticallyClassified
            | SemanticBoundaryDisposition::OrdinaryRuntime
            | SemanticBoundaryDisposition::DeferredRuntime
            | SemanticBoundaryDisposition::DeferredLifecycle => {
                if boundary.blocks_compilation {
                    add("non-blocking semantic boundary disposition cannot block compilation");
                }
            }
            SemanticBoundaryDisposition::GovernedCompileTimeDynamic => {}
        }
    }
    if !violations.is_empty() {
        bail!(
            "semantic-boundary inventory is invalid with {} violation(s):\n{}",
            violations.len(),
            violations.join("\n")
        );
    }
    Ok(())
}

fn records_from_reports(reports: &[RunReport]) -> Result<Vec<RunnerRecord>> {
    let mut records = Vec::new();
    let mut keys = BTreeSet::new();
    for report in reports {
        let failures = report
            .failures
            .iter()
            .map(|failure| (failure.path.as_str(), failure))
            .collect::<BTreeMap<_, _>>();
        for result in &report.file_results {
            let key = (report.mode.as_str().to_string(), result.path.clone());
            if !keys.insert(key) {
                bail!("multiple reports declare {} mode for {}", report.mode, result.path);
            }
            let failure = failures.get(result.path.as_str()).copied();
            let semantic_boundaries = report
                .semantic_boundaries
                .iter()
                .filter(|boundary| boundary.path == result.path)
                .map(boundary_record)
                .collect();
            records.push(RunnerRecord {
                schema_version: RUNNER_RECORD_SCHEMA_VERSION.to_string(),
                mode: report.mode.as_str().to_string(),
                path: result.path.clone(),
                status: result.status,
                assertions_passed: result.assertions_passed,
                assertions_total: result.assertions_total,
                bucket: failure.map(|value| value.bucket.clone()),
                first_diagnostic: failure.map(|value| value.first_diagnostic.clone()),
                semantic_boundaries,
            });
        }
    }
    sort_records(&mut records);
    Ok(records)
}

fn boundary_record(boundary: &ObservedSemanticBoundary) -> SemanticBoundaryRecord {
    SemanticBoundaryRecord {
        id: boundary.id.clone(),
        disposition: boundary.disposition,
        reason: boundary.reason.clone(),
        source_span: boundary.source_span,
        source_kind: boundary.source_kind.clone(),
        confidence: boundary.confidence,
        blocks_compilation: boundary.blocks_compilation,
        blocks_downstream_static_facts: boundary.blocks_downstream_static_facts,
        lock_scope: boundary.lock_scope,
        owner_workstream: boundary.owner_workstream.clone(),
        supporting_test: boundary.supporting_test.clone(),
    }
}

fn compile_boundaries(reports: &[RunReport]) -> Result<Vec<ObservedSemanticBoundary>> {
    let compile_reports =
        reports.iter().filter(|report| report.mode == HarnessMode::Compile).collect::<Vec<_>>();
    if compile_reports.len() > 1 {
        bail!("runner-record derivation accepts at most one compile report");
    }
    let mut boundaries = compile_reports
        .first()
        .map(|report| report.semantic_boundaries.clone())
        .unwrap_or_default();
    boundaries.sort_by_key(boundary_key);
    Ok(boundaries)
}

fn validate_record_files(
    reports: &[RunReport],
    records_path: &Path,
    boundaries_path: Option<&Path>,
) -> Result<()> {
    let expected = records_from_reports(reports)?;
    let mut actual = read_json_lines(records_path)?;
    sort_records(&mut actual);
    if actual != expected {
        bail!("runner-record JSONL does not exactly match the supplied run reports");
    }
    if let Some(path) = boundaries_path {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("reading semantic boundaries {}", path.display()))?;
        let mut actual_boundaries: Vec<ObservedSemanticBoundary> = serde_json::from_str(&raw)
            .with_context(|| format!("decoding semantic boundaries {}", path.display()))?;
        validate_semantic_boundary_inventory(&actual_boundaries)?;
        actual_boundaries.sort_by_key(boundary_key);
        if actual_boundaries != compile_boundaries(reports)? {
            bail!("semantic-boundary artifact does not match the compile report");
        }
    }
    Ok(())
}

fn read_json_lines(path: &Path) -> Result<Vec<RunnerRecord>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading runner records {}", path.display()))?;
    let mut records = Vec::new();
    for (index, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record: RunnerRecord = serde_json::from_str(line).with_context(|| {
            format!("decoding runner record line {} in {}", index + 1, path.display())
        })?;
        if record.schema_version != RUNNER_RECORD_SCHEMA_VERSION {
            bail!("runner record has unsupported schema {}", record.schema_version);
        }
        validate_test_path(&record.path)?;
        records.push(record);
    }
    if records.is_empty() {
        bail!("runner-record artifact is empty: {}", path.display());
    }
    Ok(records)
}

fn sort_records(records: &mut [RunnerRecord]) {
    records
        .sort_by(|left, right| left.mode.cmp(&right.mode).then_with(|| left.path.cmp(&right.path)));
}

fn boundary_key(boundary: &ObservedSemanticBoundary) -> (String, String, usize, usize) {
    (
        boundary.path.clone(),
        boundary.id.clone(),
        boundary.source_span.start,
        boundary.source_span.end,
    )
}

fn reject_output_aliases(inputs: &[PathBuf], outputs: &[PathBuf]) -> Result<()> {
    let input_paths = inputs
        .iter()
        .map(|path| {
            fs::canonicalize(path)
                .with_context(|| format!("canonicalizing input evidence {}", path.display()))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let mut output_paths = BTreeSet::new();
    for output in outputs {
        let resolved = resolve_destination(output)?;
        if input_paths.contains(&resolved) {
            bail!("output path {} aliases an input evidence file", output.display());
        }
        if !output_paths.insert(resolved) {
            bail!("multiple output options resolve to the same path");
        }
    }
    Ok(())
}

fn resolve_destination(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return fs::canonicalize(path)
            .with_context(|| format!("canonicalizing output path {}", path.display()));
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().context("reading current directory")?.join(path)
    };
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        let component = ancestor.file_name().ok_or_else(|| {
            color_eyre::eyre::eyre!("output path has no existing ancestor: {}", path.display())
        })?;
        suffix.push(component.to_os_string());
        ancestor = ancestor.parent().ok_or_else(|| {
            color_eyre::eyre::eyre!("output path has no existing ancestor: {}", path.display())
        })?;
    }
    let mut resolved = fs::canonicalize(ancestor)
        .with_context(|| format!("canonicalizing output ancestor {}", ancestor.display()))?;
    for component in suffix.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn validate_test_path(path: &str) -> Result<()> {
    let normalized = path.replace('\\', "/");
    if path != normalized
        || normalized.trim().is_empty()
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.split('/').any(|part| part.is_empty() || part == "." || part == "..")
        || !normalized.ends_with(".t")
    {
        bail!("invalid normalized Perl test path: {path}");
    }
    Ok(())
}

fn parse_runner(value: &str) -> Result<HarnessRunner> {
    match value {
        "test" => Ok(HarnessRunner::Test),
        "harness" => Ok(HarnessRunner::Harness),
        _ => bail!("unsupported harness runner: {value}"),
    }
}

fn parse_profile(value: &str) -> Result<HarnessProfile> {
    match value {
        "base" => Ok(HarnessProfile::Base),
        "comp" => Ok(HarnessProfile::Comp),
        "run" => Ok(HarnessProfile::Run),
        "core" => Ok(HarnessProfile::Core),
        "lib" => Ok(HarnessProfile::Lib),
        "full" => Ok(HarnessProfile::Full),
        _ => bail!("unsupported harness profile: {value}"),
    }
}

fn sanitize_perl_env(command: &mut Command) {
    for key in
        ["PERL5LIB", "PERLLIB", "PERL5OPT", "PERL_UNICODE", "PERL_LOCAL_LIB_ROOT", "PERL_MB_OPT"]
    {
        command.env_remove(key);
    }
}

fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        bail!("raw stream hex payload has odd length");
    }
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        decoded.push((high << 4) | low);
    }
    Ok(decoded)
}

fn hex_nibble(value: u8) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => bail!("raw stream hex payload contains a non-hex byte"),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    create_parent(path)?;
    let json = serde_json::to_string_pretty(value).context("serializing JSON evidence")?;
    fs::write(path, format!("{json}\n"))
        .with_context(|| format!("writing JSON evidence {}", path.display()))
}

fn write_json_lines(path: &Path, records: &[RunnerRecord]) -> Result<()> {
    create_parent(path)?;
    let file = fs::File::create(path)
        .with_context(|| format!("creating runner records {}", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    for record in records {
        serde_json::to_writer(&mut writer, record).context("serializing runner record")?;
        writer
            .write_all(b"\n")
            .with_context(|| format!("writing runner records {}", path.display()))?;
    }
    writer.flush().with_context(|| format!("flushing runner records {}", path.display()))
}

fn create_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_core_harness_types::RunSummary;
    use perl_core_harness_types::SemanticBoundarySourceSpan;
    use std::io::Cursor;

    type TestResult = Result<()>;

    #[test]
    fn records_cover_parse_and_compile_without_overwrite() -> TestResult {
        let parse = sample_report(HarnessMode::Parse);
        let mut compile = sample_report(HarnessMode::Compile);
        compile.semantic_boundaries.push(sample_boundary());
        validate_report_collection(&[parse.clone(), compile.clone()])?;
        let records = records_from_reports(&[parse, compile])?;
        if records.len() != 4 {
            bail!("expected four records, found {}", records.len());
        }
        let modes = records.iter().map(|record| record.mode.as_str()).collect::<BTreeSet<_>>();
        if modes != BTreeSet::from(["compile", "parse"]) {
            bail!("runner records did not preserve both modes: {modes:?}");
        }
        let compile_ok = records
            .iter()
            .find(|record| record.mode == "compile" && record.path == "base/ok.t")
            .ok_or_else(|| color_eyre::eyre::eyre!("compile record was not derived"))?;
        if compile_ok.semantic_boundaries.len() != 1 {
            bail!("compile record did not retain its semantic boundary");
        }
        Ok(())
    }

    #[test]
    fn report_collection_rejects_cross_subject_modes() -> TestResult {
        let parse = sample_report(HarnessMode::Parse);
        let mut compile = sample_report(HarnessMode::Compile);
        compile.commit = "b".repeat(40);
        let Err(error) = validate_report_collection(&[parse, compile]) else {
            bail!("reports from different commits must be rejected");
        };
        if !error.to_string().contains("one measured subject") {
            bail!("unexpected cross-subject error: {error}");
        }
        Ok(())
    }

    #[test]
    fn report_collection_rejects_membership_drift() -> TestResult {
        let parse = sample_report(HarnessMode::Parse);
        let mut compile = sample_report(HarnessMode::Compile);
        compile.file_results[1].path = "base/drift.t".into();
        let Err(error) = validate_report_collection(&[parse, compile]) else {
            bail!("cross-mode membership drift must be rejected");
        };
        if !error.to_string().contains("membership differs") {
            bail!("unexpected membership error: {error}");
        }
        Ok(())
    }

    #[test]
    fn derivation_rejects_output_aliasing_report() -> TestResult {
        let temp = tempfile::tempdir()?;
        let report = temp.path().join("report.json");
        let boundaries = temp.path().join("boundaries.json");
        fs::write(&report, "{}\n")?;
        let Err(error) =
            reject_output_aliases(std::slice::from_ref(&report), &[report.clone(), boundaries])
        else {
            bail!("output aliases must be rejected before writing");
        };
        if !error.to_string().contains("aliases an input") {
            bail!("unexpected output-alias error: {error}");
        }
        Ok(())
    }

    #[test]
    fn derivation_rejects_output_aliasing_boundaries_report() -> TestResult {
        let temp = tempfile::tempdir()?;
        let report = temp.path().join("report.json");
        let output = temp.path().join("records.jsonl");
        fs::write(&report, "{}\n")?;
        let Err(error) =
            reject_output_aliases(std::slice::from_ref(&report), &[output, report.clone()])
        else {
            bail!("boundary output aliases must be rejected before writing");
        };
        if !error.to_string().contains("aliases an input") {
            bail!("unexpected boundary-output alias error: {error}");
        }
        Ok(())
    }

    #[test]
    fn report_validation_rejects_missing_failure_evidence() -> TestResult {
        let mut report = sample_report(HarnessMode::Compile);
        report.file_results[1].status = RunnerStatus::Fail;
        report.summary.files_passed = 1;
        report.summary.files_failed = 1;
        let Err(error) = validate_report(&report) else {
            bail!("a failing file without typed failure evidence must be rejected");
        };
        if !error.to_string().contains("status and failure evidence disagree") {
            bail!("unexpected report validation error: {error}");
        }
        Ok(())
    }

    #[test]
    fn report_validation_rejects_contradictory_source_lock() -> TestResult {
        let mut report = sample_report(HarnessMode::Compile);
        let mut boundary = sample_boundary();
        boundary.confidence = SemanticBoundaryConfidence::Unresolved;
        boundary.blocks_compilation = true;
        report.semantic_boundaries.push(boundary);
        let Err(error) = validate_report(&report) else {
            bail!("contradictory source-lock evidence must be rejected");
        };
        let text = error.to_string();
        if !text.contains("exact confidence") || !text.contains("must not block compilation") {
            bail!("unexpected boundary-invariant error: {error}");
        }
        Ok(())
    }

    #[test]
    fn check_rejects_stale_runner_mode() -> TestResult {
        let temp = tempfile::tempdir()?;
        let report = sample_report(HarnessMode::Parse);
        let mut records = records_from_reports(std::slice::from_ref(&report))?;
        records[0].mode = "compile".into();
        let records_path = temp.path().join("records.jsonl");
        write_json_lines(&records_path, &records)?;
        let Err(error) = validate_record_files(&[report], &records_path, None) else {
            bail!("stale runner mode must be rejected");
        };
        if !error.to_string().contains("does not exactly match") {
            bail!("unexpected stale-record error: {error}");
        }
        Ok(())
    }

    #[test]
    fn discovery_envelope_preserves_failure_detail() -> TestResult {
        let envelope = DiscoveryRawEnvelope {
            schema_version: DISCOVERY_RAW_SCHEMA_VERSION.into(),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            host_perl: "perl".into(),
            working_directory: "t".into(),
            argv: vec!["TEST".into(), "--dumptests".into()],
            process: DiscoveryProcessOutcome::Exited { code: 7 },
            stdout: RawByteStream::from_capture(
                capture_stream(Cursor::new(b"partial output"), RAW_STREAM_MAX_BYTES),
                RAW_STREAM_MAX_BYTES,
            ),
            stderr: RawByteStream::from_capture(
                capture_stream(Cursor::new(b"broken prepared tree"), RAW_STREAM_MAX_BYTES),
                RAW_STREAM_MAX_BYTES,
            ),
        };
        envelope.validate()?;
        let encoded = serde_json::to_string(&envelope)?;
        let decoded: DiscoveryRawEnvelope = serde_json::from_str(&encoded)?;
        if decoded != envelope
            || decoded.stderr.utf8_text()?.as_deref() != Some("broken prepared tree")
        {
            bail!("failed discovery evidence did not survive round-trip");
        }
        if decoded.complete_success() {
            bail!("nonzero process status must not become successful discovery");
        }
        Ok(())
    }

    #[test]
    fn raw_stream_round_trips_invalid_utf8_and_nul_within_limit() -> TestResult {
        let bytes = vec![0, 0x80, 0xff, b'a', b'\n'];
        let stream = RawByteStream::from_capture(
            capture_stream(Cursor::new(bytes.clone()), RAW_STREAM_MAX_BYTES),
            RAW_STREAM_MAX_BYTES,
        );
        stream.validate()?;
        if stream.bytes()? != bytes {
            bail!("raw stream did not round-trip byte-for-byte");
        }
        if stream.utf8_text()?.is_some() {
            bail!("invalid UTF-8 stream must not acquire a text projection");
        }
        Ok(())
    }

    fn assert_over_limit_capture(label: &str) -> TestResult {
        let limit = 8;
        let bytes = vec![b'x'; limit + 5];
        let stream = RawByteStream::from_capture(
            capture_stream(Cursor::new(bytes), limit),
            limit,
        );
        stream.validate()?;
        if !stream.truncated
            || stream.observed_byte_length != 13
            || stream.retained_byte_length != limit
            || stream.complete()
        {
            bail!("{label} over-limit capture was not represented as bounded truncation");
        }
        Ok(())
    }

    #[test]
    fn stdout_over_limit_is_bounded_and_non_authoritative() -> TestResult {
        assert_over_limit_capture("stdout")
    }

    #[test]
    fn stderr_over_limit_is_bounded_and_non_authoritative() -> TestResult {
        assert_over_limit_capture("stderr")
    }

    #[test]
    fn distinct_lossy_sequences_remain_distinct() -> TestResult {
        let first = RawByteStream::from_capture(
            capture_stream(Cursor::new([0x80]), RAW_STREAM_MAX_BYTES),
            RAW_STREAM_MAX_BYTES,
        );
        let second = RawByteStream::from_capture(
            capture_stream(Cursor::new([0x81]), RAW_STREAM_MAX_BYTES),
            RAW_STREAM_MAX_BYTES,
        );
        if String::from_utf8_lossy(&[0x80]) != String::from_utf8_lossy(&[0x81]) {
            bail!("fixture must demonstrate a lossy-decoding collision");
        }
        if first.payload_hex == second.payload_hex || first.sha256 == second.sha256 {
            bail!("byte-exact streams collapsed distinct invalid UTF-8 sequences");
        }
        Ok(())
    }

    #[test]
    fn process_outcome_cannot_contradict_a_separate_success_flag() -> TestResult {
        let contradictory_success = r#"{
            "schema_version":"perl_core_harness.discovery_raw.v2",
            "runner":"test",
            "profile":"base",
            "host_perl":"perl",
            "working_directory":"t",
            "argv":["TEST","--dumptests"],
            "status":7,
            "success":true,
            "stdout":{},
            "stderr":{}
        }"#;
        let contradictory_failure = contradictory_success.replace("\"status\":7", "\"status\":0").replace("\"success\":true", "\"success\":false");
        let missing_outcome = contradictory_success
            .replace("\"status\":7,", "")
            .replace("\"success\":true,", "");
        for invalid in [
            contradictory_success.to_string(),
            contradictory_failure,
            missing_outcome,
        ] {
            if serde_json::from_str::<DiscoveryRawEnvelope>(&invalid).is_ok() {
                bail!("legacy contradictory or missing process outcome must not decode as v2");
            }
        }
        Ok(())
    }

    #[test]
    fn complete_success_is_derived_from_process_and_stream_completeness() -> TestResult {
        let stream = RawByteStream::from_capture(
            capture_stream(Cursor::new(b"base/ok.t\n"), RAW_STREAM_MAX_BYTES),
            RAW_STREAM_MAX_BYTES,
        );
        let mut envelope = DiscoveryRawEnvelope {
            schema_version: DISCOVERY_RAW_SCHEMA_VERSION.into(),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            host_perl: "perl".into(),
            working_directory: "t".into(),
            argv: vec!["TEST".into(), "--dumptests".into()],
            process: DiscoveryProcessOutcome::Exited { code: 0 },
            stdout: stream,
            stderr: RawByteStream::empty(RAW_STREAM_MAX_BYTES),
        };
        assert!(envelope.complete_success());
        envelope.process = DiscoveryProcessOutcome::Exited { code: 1 };
        assert!(!envelope.complete_success());
        Ok(())
    }

    #[test]
    fn legacy_text_only_envelope_is_not_silently_v2() -> TestResult {
        let legacy = r#"{
            "schema_version":"perl_core_harness.discovery_raw.v1",
            "runner":"test",
            "profile":"base",
            "host_perl":"perl",
            "working_directory":"t",
            "argv":["TEST","--dumptests"],
            "status":0,
            "success":true,
            "stdout":"base/ok.t\n",
            "stderr":"",
            "spawn_error":null
        }"#;
        if serde_json::from_str::<DiscoveryRawEnvelope>(legacy).is_ok() {
            bail!("text-only v1 evidence must not be reinterpreted as byte-exact v2");
        }
        Ok(())
    }

    fn sample_report(mode: HarnessMode) -> RunReport {
        RunReport {
            schema_version: RUN_REPORT_SCHEMA_VERSION.into(),
            commit: "a".repeat(40),
            timestamp: "2026-08-11T00:00:00Z".into(),
            perl_ref: "perl-ref".into(),
            prepared_tree: "<prepared-tree>".into(),
            run_tree: format!("<run-tree-{}>", mode.as_str()),
            host_perl: "perl".into(),
            runner: HarnessRunner::Test,
            mode,
            profile: HarnessProfile::Base,
            harness_status: Some(0),
            summary: RunSummary {
                files_total: 2,
                files_passed: 2,
                files_failed: 0,
                tap_assertions_total: 2,
                tap_assertions_passed: 2,
            },
            buckets: BTreeMap::new(),
            file_results: vec![
                RunFileResult {
                    path: "base/ok.t".into(),
                    status: RunnerStatus::Pass,
                    assertions_passed: 1,
                    assertions_total: 1,
                },
                RunFileResult {
                    path: "base/other.t".into(),
                    status: RunnerStatus::Pass,
                    assertions_passed: 1,
                    assertions_total: 1,
                },
            ],
            failures: Vec::new(),
            semantic_boundaries: Vec::new(),
        }
    }

    fn sample_boundary() -> ObservedSemanticBoundary {
        ObservedSemanticBoundary {
            path: "base/ok.t".into(),
            id: "source_locked_probe".into(),
            disposition: SemanticBoundaryDisposition::SourceLockedCompatibility,
            reason: "exact fixture compatibility".into(),
            source_span: SemanticBoundarySourceSpan { start: 1, end: 2 },
            source_kind: "probe".into(),
            confidence: SemanticBoundaryConfidence::Exact,
            blocks_compilation: false,
            blocks_downstream_static_facts: true,
            lock_scope: SemanticBoundaryLockScope::PathAndSource,
            owner_workstream: "parser_recovery".into(),
            supporting_test: "crates/perl-core-harness/tests/source_locked_probe.rs".into(),
        }
    }
}
