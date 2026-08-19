use color_eyre::eyre::{Context, Result, bail};
use perl_core_harness_types::BaselineViolation;
use perl_core_harness_types::{
    HarnessMode, HarnessProfile, HarnessRunner, ObservedSemanticBoundary,
    RUN_REPORT_SCHEMA_VERSION, RUNNER_RECORD_SCHEMA_VERSION, RunFailure, RunFileResult, RunReport,
    RunnerRecord, RunnerStatus, SemanticBoundaryRecord,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

const DISCOVERY_RAW_SCHEMA_VERSION: &str = "perl_core_harness.discovery_raw.v2";
const DISCOVERY_DERIVED_SCHEMA_VERSION: &str = "perl_core_harness.discovery_derived.v1";
const DISCOVERY_DECODER_VERSION: &str = "utf8_strict.v1";
const DISCOVERY_NORMALIZER_VERSION: &str = "discovery_test_paths.v1";
const RAW_STREAM_ENCODING: &str = "hex";
const RAW_STREAM_MAX_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_CAPTURE_DEADLINE_SECONDS: u64 = 30 * 60;
const MAX_CAPTURE_DEADLINE_SECONDS: u64 = 24 * 60 * 60;
const SUPERVISOR_POLL_INTERVAL: Duration = Duration::from_millis(10);
const TERMINATION_GRACE: Duration = Duration::from_secs(2);
const TERMINATION_HARD_LIMIT: Duration = Duration::from_secs(10);

/// Dispatch one `perl-core-harness-artifacts` subcommand.
///
/// The whole producer lives in the library so its proof is executed by the
/// workspace `--lib` gate; the binary is a thin argv shim over this function.
///
/// # Errors
///
/// Returns an error when the subcommand is unknown, its options are invalid, or
/// the requested artifact cannot be produced or validated.
pub fn run(command: &str, args: impl Iterator<Item = String>) -> Result<()> {
    let options = Options::parse(args)?;
    match command {
        "capture-discovery" => capture_discovery(CaptureDiscoveryConfig::from_options(options)?),
        "check-discovery" => check_discovery(CheckDiscoveryConfig::from_options(options)?),
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
    commit: String,
    perl_ref: String,
    output: PathBuf,
    derived_output: Option<PathBuf>,
    limits: CaptureLimits,
}

impl CaptureDiscoveryConfig {
    fn from_options(mut options: Options) -> Result<Self> {
        let config = Self {
            perl_tree: PathBuf::from(options.required("--perl-tree")?),
            host_perl: PathBuf::from(options.required("--host-perl")?),
            runner: parse_runner(&options.required("--runner")?)?,
            profile: parse_profile(&options.required("--profile")?)?,
            commit: options.required("--commit")?,
            perl_ref: options.required("--perl-ref")?,
            output: PathBuf::from(options.required("--output")?),
            derived_output: options.optional("--derived-output")?.map(PathBuf::from),
            limits: CaptureLimits {
                deadline: parse_deadline(options.optional("--deadline-seconds")?.as_deref())?,
                cancel_file: options.optional("--cancel-file")?.map(PathBuf::from),
            },
        };
        options.finish()?;
        Ok(config)
    }
}

/// Finite bounds applied to one supervised discovery capture.
///
/// The deadline bounds the whole capture, including descendants that inherited
/// the stdout/stderr pipes. The cancel file lets a supervising workflow request
/// an early, explicitly non-authoritative stop.
#[derive(Debug, Clone)]
struct CaptureLimits {
    deadline: Duration,
    cancel_file: Option<PathBuf>,
}

impl CaptureLimits {
    fn cancel_requested(&self) -> Option<String> {
        let path = self.cancel_file.as_ref()?;
        path.exists().then(|| format!("cancellation file {} is present", path.display()))
    }
}

fn parse_deadline(value: Option<&str>) -> Result<Duration> {
    let seconds = match value {
        None => DEFAULT_CAPTURE_DEADLINE_SECONDS,
        Some(raw) => {
            raw.parse::<u64>().with_context(|| format!("parsing --deadline-seconds value {raw}"))?
        }
    };
    if seconds == 0 {
        bail!("--deadline-seconds must be a positive number of seconds");
    }
    if seconds > MAX_CAPTURE_DEADLINE_SECONDS {
        bail!(
            "--deadline-seconds must not exceed {MAX_CAPTURE_DEADLINE_SECONDS}; discovery capture must stay finite"
        );
    }
    Ok(Duration::from_secs(seconds))
}

#[derive(Debug)]
struct CheckDiscoveryConfig {
    raw: PathBuf,
    derived: PathBuf,
}

impl CheckDiscoveryConfig {
    fn from_options(mut options: Options) -> Result<Self> {
        let config = Self {
            raw: PathBuf::from(options.required("--raw")?),
            derived: PathBuf::from(options.required("--derived")?),
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
        Self { capture_error: Some(message.to_string()), ..Self::empty() }
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

/// Which part of the bounded capture was still outstanding when the deadline fired.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum CaptureDeadlinePhase {
    /// The discovery process itself had not yet been reaped.
    Process,
    /// The process was reaped but stdout or stderr was still held open.
    StreamDrain,
}

impl CaptureDeadlinePhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Process => "process",
            Self::StreamDrain => "stream drain",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum DiscoveryProcessOutcome {
    /// The process reached its own exit status.
    Exited {
        code: i32,
    },
    /// The process was terminated by a signal whose identity is preserved.
    Signaled {
        signal: i32,
        signal_name: String,
        core_dumped: bool,
    },
    /// The process terminated without an exit code and this platform exposes no
    /// finer termination identity.
    TerminatedWithoutIdentity {
        platform: String,
    },
    /// This tool stopped the capture because its finite deadline expired.
    TimedOut {
        deadline_ms: u64,
        phase: CaptureDeadlinePhase,
    },
    /// This tool stopped the capture because cancellation was requested.
    Cancelled {
        source: String,
    },
    SpawnFailed {
        error: String,
    },
    WaitFailed {
        error: String,
    },
    CaptureSetupFailed {
        stream: String,
    },
}

impl DiscoveryProcessOutcome {
    const fn succeeded(&self) -> bool {
        matches!(self, Self::Exited { code: 0 })
    }

    fn summary(&self) -> String {
        match self {
            Self::Exited { code } => format!("exited with status {code}"),
            Self::Signaled { signal, signal_name, core_dumped } => format!(
                "terminated by {signal_name} ({signal}){}",
                if *core_dumped { " with a core dump" } else { "" }
            ),
            Self::TerminatedWithoutIdentity { platform } => {
                format!("terminated without an exit code or termination identity on {platform}")
            }
            Self::TimedOut { deadline_ms, phase } => {
                format!("exceeded the {deadline_ms}ms capture deadline during {}", phase.as_str())
            }
            Self::Cancelled { source } => format!("cancelled: {source}"),
            Self::SpawnFailed { .. } => "spawn failed".to_string(),
            Self::WaitFailed { .. } => "wait failed".to_string(),
            Self::CaptureSetupFailed { stream } => {
                format!("failed to attach bounded {stream} capture")
            }
        }
    }

    /// Reject outcomes whose recorded identity is structurally empty.
    fn validate(&self) -> Result<()> {
        let detail = match self {
            Self::SpawnFailed { error } | Self::WaitFailed { error } => Some(error.as_str()),
            Self::CaptureSetupFailed { stream } => Some(stream.as_str()),
            Self::Cancelled { source } => Some(source.as_str()),
            Self::TerminatedWithoutIdentity { platform } => Some(platform.as_str()),
            Self::Signaled { signal, signal_name, .. } => {
                if *signal <= 0 {
                    bail!("signalled discovery outcome recorded a non-positive signal {signal}");
                }
                Some(signal_name.as_str())
            }
            Self::TimedOut { deadline_ms, .. } => {
                if *deadline_ms == 0 {
                    bail!("timed-out discovery outcome recorded a zero deadline");
                }
                None
            }
            Self::Exited { .. } => None,
        };
        if detail.is_some_and(|detail| detail.trim().is_empty()) {
            bail!("raw discovery process outcome contains an empty failure detail");
        }
        Ok(())
    }
}

/// Project a reaped wait status onto the outcome taxonomy without collapsing
/// distinct terminations.
#[cfg(unix)]
fn outcome_from_status(status: ExitStatus) -> DiscoveryProcessOutcome {
    use std::os::unix::process::ExitStatusExt;

    if let Some(code) = status.code() {
        return DiscoveryProcessOutcome::Exited { code };
    }
    if let Some(signal) = status.signal() {
        return DiscoveryProcessOutcome::Signaled {
            signal,
            signal_name: signal_name(signal),
            core_dumped: status.core_dumped(),
        };
    }
    DiscoveryProcessOutcome::TerminatedWithoutIdentity {
        platform: std::env::consts::OS.to_string(),
    }
}

/// Non-Unix hosts expose only an exit code, so termination identity is recorded
/// as explicitly unavailable rather than silently collapsed.
#[cfg(not(unix))]
fn outcome_from_status(status: ExitStatus) -> DiscoveryProcessOutcome {
    match status.code() {
        Some(code) => DiscoveryProcessOutcome::Exited { code },
        None => DiscoveryProcessOutcome::TerminatedWithoutIdentity {
            platform: std::env::consts::OS.to_string(),
        },
    }
}

#[cfg(unix)]
fn signal_name(signal: i32) -> String {
    nix::sys::signal::Signal::try_from(signal)
        .map_or_else(|_| format!("SIG{signal}"), |signal| signal.as_str().to_string())
}

/// The exact subject one discovery capture measured.
///
/// Paths are deliberately absent: an absolute prepared-tree or host-Perl path
/// identifies the machine that ran the capture, not the thing measured, and this
/// evidence is meant to stay useful after the runner that produced it is gone.
/// Identity is carried by the upstream commit and ref plus content digests of
/// the exact runner script and host Perl that were executed.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
struct DiscoverySubject {
    commit: String,
    perl_ref: String,
    runner_script: String,
    runner_script_sha256: String,
    host_perl_file_name: String,
    host_perl_sha256: String,
    tool_version: String,
}

impl DiscoverySubject {
    fn validate(&self) -> Result<()> {
        for (label, value) in [
            ("commit", self.commit.as_str()),
            ("Perl ref", self.perl_ref.as_str()),
            ("runner script", self.runner_script.as_str()),
            ("host Perl file name", self.host_perl_file_name.as_str()),
            ("tool version", self.tool_version.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("discovery subject has an empty {label}");
            }
        }
        for (label, digest) in [
            ("runner script", self.runner_script_sha256.as_str()),
            ("host Perl", self.host_perl_sha256.as_str()),
        ] {
            if !digest.starts_with("sha256:") || digest.len() != "sha256:".len() + 64 {
                bail!("discovery subject {label} digest is not a sha256 digest: {digest}");
            }
        }
        for (label, value) in [
            ("runner script", self.runner_script.as_str()),
            ("host Perl file name", self.host_perl_file_name.as_str()),
        ] {
            // check-discovery validates evidence captured on other hosts, so a
            // Windows-shaped path must be rejected on a Unix reader too.
            if value.contains('/') || value.contains('\\') || value.contains(':') {
                bail!("discovery subject {label} must be a file name, not a host path: {value}");
            }
        }
        Ok(())
    }

    fn digest(&self) -> Result<String> {
        let canonical =
            serde_json::to_string(self).context("serializing the discovery subject identity")?;
        Ok(sha256_digest(canonical.as_bytes()))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DiscoveryRawEnvelope {
    schema_version: String,
    runner: HarnessRunner,
    profile: HarnessProfile,
    subject: DiscoverySubject,
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
        self.subject.validate().context("validating the discovery subject")?;
        if self.working_directory.trim().is_empty()
            || Path::new(&self.working_directory).is_absolute()
        {
            bail!(
                "raw discovery working directory must be recorded relative to the prepared tree, found {}",
                self.working_directory
            );
        }
        self.stdout.validate().context("validating raw discovery stdout")?;
        self.stderr.validate().context("validating raw discovery stderr")?;
        self.process.validate()?;
        Ok(())
    }

    fn complete_success(&self) -> bool {
        self.process.succeeded() && self.stdout.complete() && self.stderr.complete()
    }
}

/// The exact identity of one captured raw stream.
///
/// Both the complete observed digest and the retained-prefix digest are bound so
/// a derived record cannot be replayed against different upstream bytes that
/// happen to share a retained prefix.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RawStreamIdentity {
    observed_byte_length: u64,
    observed_sha256: String,
    retained_byte_length: usize,
    retained_sha256: String,
    truncated: bool,
}

impl RawStreamIdentity {
    fn of(stream: &RawByteStream) -> Self {
        Self {
            observed_byte_length: stream.observed_byte_length,
            observed_sha256: stream.sha256.clone(),
            retained_byte_length: stream.retained_byte_length,
            retained_sha256: stream.retained_sha256.clone(),
            truncated: stream.truncated,
        }
    }
}

/// Normalized discovery evidence bound to the exact raw bytes and named
/// transforms it was produced from.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DiscoveryDerivedEnvelope {
    schema_version: String,
    raw_schema_version: String,
    subject_sha256: String,
    decoder_version: String,
    normalizer_version: String,
    stdout: RawStreamIdentity,
    stderr: RawStreamIdentity,
    normalized_sha256: String,
    test_paths: Vec<String>,
}

impl DiscoveryDerivedEnvelope {
    /// Derive normalized discovery output from complete, authoritative raw evidence.
    fn derive(raw: &DiscoveryRawEnvelope) -> Result<Self> {
        if !raw.complete_success() {
            bail!(
                "refusing to derive normalized discovery from non-authoritative raw evidence ({})",
                raw.process.summary()
            );
        }
        let text = decode_discovery_stdout(&raw.stdout)?;
        let test_paths = normalize_discovery_paths(&text);
        Ok(Self {
            schema_version: DISCOVERY_DERIVED_SCHEMA_VERSION.to_string(),
            raw_schema_version: raw.schema_version.clone(),
            subject_sha256: raw.subject.digest()?,
            decoder_version: DISCOVERY_DECODER_VERSION.to_string(),
            normalizer_version: DISCOVERY_NORMALIZER_VERSION.to_string(),
            stdout: RawStreamIdentity::of(&raw.stdout),
            stderr: RawStreamIdentity::of(&raw.stderr),
            normalized_sha256: normalized_digest(&test_paths),
            test_paths,
        })
    }

    /// Reject a derived record whose raw provenance, transform identity, or
    /// normalized content does not reproduce from the supplied raw evidence.
    fn validate_against(&self, raw: &DiscoveryRawEnvelope) -> Result<()> {
        if self.schema_version != DISCOVERY_DERIVED_SCHEMA_VERSION {
            bail!("unsupported derived discovery schema: {}", self.schema_version);
        }
        if self.raw_schema_version != raw.schema_version {
            bail!(
                "derived discovery was produced from raw schema {} but was replayed against {}",
                self.raw_schema_version,
                raw.schema_version
            );
        }
        if self.subject_sha256 != raw.subject.digest()? {
            bail!(
                "derived discovery summarizes a different measured subject than the supplied raw evidence"
            );
        }
        if self.decoder_version != DISCOVERY_DECODER_VERSION
            || self.normalizer_version != DISCOVERY_NORMALIZER_VERSION
        {
            bail!(
                "derived discovery transform identity drifted: recorded {}/{}, current {DISCOVERY_DECODER_VERSION}/{DISCOVERY_NORMALIZER_VERSION}",
                self.decoder_version,
                self.normalizer_version
            );
        }
        if self.stdout != RawStreamIdentity::of(&raw.stdout) {
            bail!("derived discovery stdout identity does not match the supplied raw stdout");
        }
        if self.stderr != RawStreamIdentity::of(&raw.stderr) {
            bail!("derived discovery stderr identity does not match the supplied raw stderr");
        }
        let expected = Self::derive(raw)?;
        if self.test_paths != expected.test_paths
            || self.normalized_sha256 != expected.normalized_sha256
        {
            bail!("derived discovery normalized content does not reproduce from its raw bytes");
        }
        if self.normalized_sha256 != normalized_digest(&self.test_paths) {
            bail!("derived discovery normalized digest does not cover its own recorded paths");
        }
        Ok(())
    }
}

/// Decode complete raw stdout under the named strict-UTF-8 decoder.
fn decode_discovery_stdout(stdout: &RawByteStream) -> Result<String> {
    stdout.utf8_text()?.ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "discovery stdout is incomplete or is not valid UTF-8 under {DISCOVERY_DECODER_VERSION}"
        )
    })
}

/// Project decoded discovery stdout onto its declared `.t` paths.
fn normalize_discovery_paths(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.ends_with(".t"))
        .map(ToString::to_string)
        .collect()
}

fn normalized_digest(test_paths: &[String]) -> String {
    let mut joined = String::new();
    for path in test_paths {
        joined.push_str(path);
        joined.push('\n');
    }
    sha256_digest(joined.as_bytes())
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
        full_sha256: format!("sha256:{}", encode_hex(&full_hasher.finalize())),
        truncated: observed_byte_length > retained_u64,
        capture_error,
    }
}

/// Why this tool, rather than the process itself, ended the capture.
#[derive(Debug)]
enum CaptureTermination {
    Deadline { phase: CaptureDeadlinePhase },
    Cancelled { source: String },
}

/// Isolate the discovery process into its own process group so descendants that
/// inherit the pipes can be terminated as a unit.
#[cfg(unix)]
fn isolate_process_tree(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

/// Windows and other non-Unix hosts get direct-child termination only; a
/// descendant that keeps the inherited pipes open is bounded by the capture
/// hard limit rather than by process-tree cleanup.
#[cfg(not(unix))]
fn isolate_process_tree(_command: &mut Command) {}

/// Signal the whole isolated process tree, falling back to the direct child.
#[cfg(unix)]
fn signal_process_tree(child: &mut Child, signal: nix::sys::signal::Signal) {
    use nix::unistd::Pid;

    let group = i32::try_from(child.id()).map(Pid::from_raw);
    let delivered = group.is_ok_and(|group| nix::sys::signal::killpg(group, signal).is_ok());
    if !delivered && signal == nix::sys::signal::Signal::SIGKILL {
        let _ = child.kill();
    }
}

#[cfg(not(unix))]
fn terminate_gently(_child: &mut Child) {}

#[cfg(unix)]
fn terminate_gently(child: &mut Child) {
    signal_process_tree(child, nix::sys::signal::Signal::SIGTERM);
}

#[cfg(not(unix))]
fn terminate_forcefully(child: &mut Child) {
    let _ = child.kill();
}

#[cfg(unix)]
fn terminate_forcefully(child: &mut Child) {
    signal_process_tree(child, nix::sys::signal::Signal::SIGKILL);
}

/// Drain one pipe on its own thread so a full stdout cannot deadlock stderr.
///
/// The receiver is polled rather than joined so a descendant holding the write
/// end open cannot block the supervisor past the capture hard limit.
fn spawn_capture<R: Read + Send + 'static>(
    reader: R,
    limit_bytes: usize,
) -> Receiver<CapturedStream> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(capture_stream(reader, limit_bytes));
    });
    receiver
}

fn poll_capture(
    slot: &mut Option<CapturedStream>,
    receiver: &Receiver<CapturedStream>,
    stream: &str,
) {
    if slot.is_some() {
        return;
    }
    match receiver.try_recv() {
        Ok(capture) => *slot = Some(capture),
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => {
            *slot = Some(CapturedStream::failed(&format!("{stream} capture thread panicked")));
        }
    }
}

/// Run one discovery command under a finite deadline with bounded, concurrent
/// stdout and stderr capture and isolated process-tree cleanup.
fn run_bounded_command(
    mut command: Command,
    limits: &CaptureLimits,
) -> (DiscoveryProcessOutcome, RawByteStream, RawByteStream) {
    let empty =
        || (RawByteStream::empty(RAW_STREAM_MAX_BYTES), RawByteStream::empty(RAW_STREAM_MAX_BYTES));
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    isolate_process_tree(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let (stdout, stderr) = empty();
            return (
                DiscoveryProcessOutcome::SpawnFailed { error: error.to_string() },
                stdout,
                stderr,
            );
        }
    };

    let Some(stdout) = child.stdout.take() else {
        terminate_forcefully(&mut child);
        let _ = child.wait();
        let (stdout, stderr) = empty();
        return (
            DiscoveryProcessOutcome::CaptureSetupFailed { stream: "stdout".to_string() },
            stdout,
            stderr,
        );
    };
    let Some(stderr) = child.stderr.take() else {
        terminate_forcefully(&mut child);
        let _ = child.wait();
        let (stdout, stderr) = empty();
        return (
            DiscoveryProcessOutcome::CaptureSetupFailed { stream: "stderr".to_string() },
            stdout,
            stderr,
        );
    };

    let stdout_receiver = spawn_capture(stdout, RAW_STREAM_MAX_BYTES);
    let stderr_receiver = spawn_capture(stderr, RAW_STREAM_MAX_BYTES);

    let started = Instant::now();
    let mut status: Option<std::io::Result<ExitStatus>> = None;
    let mut stdout_capture: Option<CapturedStream> = None;
    let mut stderr_capture: Option<CapturedStream> = None;
    let mut termination: Option<CaptureTermination> = None;
    let mut terminated_at: Option<Instant> = None;
    let mut escalated = false;

    loop {
        if status.is_none() {
            match child.try_wait() {
                Ok(Some(reaped)) => status = Some(Ok(reaped)),
                Ok(None) => {}
                Err(error) => status = Some(Err(error)),
            }
        }
        poll_capture(&mut stdout_capture, &stdout_receiver, "stdout");
        poll_capture(&mut stderr_capture, &stderr_receiver, "stderr");
        if status.is_some() && stdout_capture.is_some() && stderr_capture.is_some() {
            break;
        }

        match terminated_at {
            None => {
                let requested = limits.cancel_requested().map_or_else(
                    || {
                        (started.elapsed() >= limits.deadline).then(|| {
                            CaptureTermination::Deadline {
                                phase: if status.is_none() {
                                    CaptureDeadlinePhase::Process
                                } else {
                                    CaptureDeadlinePhase::StreamDrain
                                },
                            }
                        })
                    },
                    |source| Some(CaptureTermination::Cancelled { source }),
                );
                if let Some(requested) = requested {
                    termination = Some(requested);
                    terminate_gently(&mut child);
                    terminated_at = Some(Instant::now());
                }
            }
            Some(at) => {
                if !escalated && at.elapsed() >= TERMINATION_GRACE {
                    terminate_forcefully(&mut child);
                    escalated = true;
                }
                if at.elapsed() >= TERMINATION_HARD_LIMIT {
                    break;
                }
            }
        }
        std::thread::sleep(SUPERVISOR_POLL_INTERVAL);
    }

    let process = match termination {
        Some(CaptureTermination::Deadline { phase }) => DiscoveryProcessOutcome::TimedOut {
            deadline_ms: u64::try_from(limits.deadline.as_millis()).unwrap_or(u64::MAX),
            phase,
        },
        Some(CaptureTermination::Cancelled { source }) => {
            DiscoveryProcessOutcome::Cancelled { source }
        }
        None => match status {
            Some(Ok(status)) => outcome_from_status(status),
            Some(Err(error)) => DiscoveryProcessOutcome::WaitFailed { error: error.to_string() },
            None => DiscoveryProcessOutcome::WaitFailed {
                error: "the discovery process was never reaped".to_string(),
            },
        },
    };
    let stdout_capture = stdout_capture.unwrap_or_else(|| {
        CapturedStream::failed("stdout was still held open at the capture hard limit")
    });
    let stderr_capture = stderr_capture.unwrap_or_else(|| {
        CapturedStream::failed("stderr was still held open at the capture hard limit")
    });
    (
        process,
        RawByteStream::from_capture(stdout_capture, RAW_STREAM_MAX_BYTES),
        RawByteStream::from_capture(stderr_capture, RAW_STREAM_MAX_BYTES),
    )
}

/// Bind one capture to the exact runner script and host Perl it executed.
fn discovery_subject(
    config: &CaptureDiscoveryConfig,
    script: &Path,
    script_name: &str,
) -> Result<DiscoverySubject> {
    let host_perl_file_name = config
        .host_perl
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| color_eyre::eyre::eyre!("host Perl has no UTF-8 file name"))?;
    let subject = DiscoverySubject {
        commit: config.commit.clone(),
        perl_ref: config.perl_ref.clone(),
        runner_script: script_name.to_string(),
        runner_script_sha256: digest_file(script)?,
        host_perl_file_name: host_perl_file_name.to_string(),
        host_perl_sha256: digest_file(&config.host_perl)?,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    subject.validate()?;
    Ok(subject)
}

/// Digest a file's complete contents without holding it all in memory.
fn digest_file(path: &Path) -> Result<String> {
    let mut file =
        fs::File::open(path).with_context(|| format!("reading subject file {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("reading subject file {}", path.display()));
            }
        }
    }
    Ok(format!("sha256:{}", encode_hex(&hasher.finalize())))
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
    let mut outputs = vec![config.output.clone()];
    outputs.extend(config.derived_output.iter().cloned());
    reject_output_aliases(&[script.clone(), config.host_perl.clone()], &outputs)?;
    reject_subject_destinations(&config.host_perl, &perl_tree, &outputs)?;
    if config.derived_output.as_ref() == Some(&config.output) {
        bail!("--derived-output must not alias the raw discovery --output");
    }

    let script_name = script
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| color_eyre::eyre::eyre!("runner script has no UTF-8 file name"))?;
    let subject = discovery_subject(&config, &script, script_name)?;
    let profile_args = discovery_profile_args(&t_dir, config.runner, config.profile)?;
    let mut argv = vec![script_name.to_string(), "--dumptests".to_string()];
    argv.extend(profile_args.iter().cloned());

    let mut command = Command::new(&config.host_perl);
    command.current_dir(&t_dir).args(&argv);
    command.env("LC_ALL", "C");
    sanitize_perl_env(&mut command);

    let (process, stdout, stderr) = run_bounded_command(command, &config.limits);
    let envelope = DiscoveryRawEnvelope {
        schema_version: DISCOVERY_RAW_SCHEMA_VERSION.to_string(),
        runner: config.runner,
        profile: config.profile,
        subject,
        working_directory: "t".to_string(),
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
    let derived = DiscoveryDerivedEnvelope::derive(&envelope).with_context(|| {
        format!(
            "deriving normalized discovery from byte-exact raw evidence {}",
            config.output.display()
        )
    })?;
    if derived.test_paths.is_empty() {
        bail!(
            "upstream discovery succeeded but emitted no .t paths; raw evidence is {}",
            config.output.display()
        );
    }
    if let Some(derived_output) = &config.derived_output {
        write_json(derived_output, &derived)?;
        let written: DiscoveryDerivedEnvelope = read_json(derived_output)?;
        written.validate_against(&envelope).with_context(|| {
            format!("revalidating derived discovery evidence {}", derived_output.display())
        })?;
    }
    Ok(())
}

/// Reject derived discovery evidence that does not reproduce from the raw
/// evidence it claims to summarize.
fn check_discovery(config: CheckDiscoveryConfig) -> Result<()> {
    let raw: DiscoveryRawEnvelope = read_json(&config.raw)
        .with_context(|| format!("reading raw discovery evidence {}", config.raw.display()))?;
    raw.validate()?;
    let derived: DiscoveryDerivedEnvelope = read_json(&config.derived).with_context(|| {
        format!("reading derived discovery evidence {}", config.derived.display())
    })?;
    derived.validate_against(&raw).with_context(|| {
        format!(
            "binding derived discovery {} to raw discovery {}",
            config.derived.display(),
            config.raw.display()
        )
    })
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

    reject_invalid_semantic_boundaries(&report.semantic_boundaries)?;
    reject_invalid_report_buckets(report)?;
    for boundary in &report.semantic_boundaries {
        if !files.contains_key(&boundary.path) {
            bail!("semantic boundary path {} is absent from file results", boundary.path);
        }
    }
    Ok(())
}

/// Reject a boundary inventory the crate itself considers structurally invalid.
///
/// The invariants are owned by [`crate::validate_semantic_boundary_inventory`];
/// this adapter only turns its violations into the producer's fail-closed error
/// so the two cannot drift apart.
fn reject_invalid_semantic_boundaries(boundaries: &[ObservedSemanticBoundary]) -> Result<()> {
    reject_violations(
        "semantic boundary inventory",
        &crate::validate_semantic_boundary_inventory(boundaries),
    )
}

/// Reject a report whose bucket histogram contradicts its typed failures.
///
/// The invariant is owned by [`crate::validate_report_bucket_shape`]; consuming
/// it here stops the derived records and the histogram that other consumers read
/// from disagreeing about the same authoritative receipt.
fn reject_invalid_report_buckets(report: &RunReport) -> Result<()> {
    reject_violations("run report buckets", &crate::validate_report_bucket_shape(report))
}

fn reject_violations(subject: &str, violations: &[BaselineViolation]) -> Result<()> {
    if violations.is_empty() {
        return Ok(());
    }
    let detail =
        violations.iter().map(|violation| violation.message.clone()).collect::<Vec<_>>().join("; ");
    bail!("{subject} is structurally invalid: {detail}")
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
        let mut boundaries_by_path = BTreeMap::<&str, Vec<&ObservedSemanticBoundary>>::new();
        for boundary in &report.semantic_boundaries {
            boundaries_by_path.entry(boundary.path.as_str()).or_default().push(boundary);
        }
        for result in &report.file_results {
            let key = (report.mode.as_str().to_string(), result.path.clone());
            if !keys.insert(key) {
                bail!("multiple reports declare {} mode for {}", report.mode, result.path);
            }
            let failure = failures.get(result.path.as_str()).copied();
            let semantic_boundaries = boundaries_by_path
                .get(result.path.as_str())
                .map(|boundaries| boundaries.iter().copied().map(boundary_record).collect())
                .unwrap_or_default();
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
    boundaries.sort_by(compare_boundaries);
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
        reject_invalid_semantic_boundaries(&actual_boundaries)?;
        actual_boundaries.sort_by(compare_boundaries);
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

fn compare_boundaries(
    left: &ObservedSemanticBoundary,
    right: &ObservedSemanticBoundary,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then_with(|| left.id.cmp(&right.id))
        .then_with(|| left.source_span.start.cmp(&right.source_span.start))
        .then_with(|| left.source_span.end.cmp(&right.source_span.end))
}

/// The filesystem identity of an existing path, where the host exposes one.
///
/// Canonicalization resolves symlinks but not hard links, so two names for one
/// inode canonicalize differently. Comparing `(dev, ino)` is what stops an
/// output that is an existing hard link to an input from truncating the
/// authoritative evidence while derivation still succeeds from the copy already
/// held in memory.
#[cfg(unix)]
fn file_identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;

    fs::metadata(path).ok().map(|metadata| (metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn file_identity(_path: &Path) -> Option<(u64, u64)> {
    None
}

fn reject_output_aliases(inputs: &[PathBuf], outputs: &[PathBuf]) -> Result<()> {
    let input_paths = inputs
        .iter()
        .map(|path| {
            fs::canonicalize(path)
                .with_context(|| format!("canonicalizing input evidence {}", path.display()))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let input_identities =
        inputs.iter().filter_map(|path| file_identity(path)).collect::<BTreeSet<_>>();
    let mut output_paths = BTreeSet::new();
    let mut output_identities = BTreeSet::new();
    for output in outputs {
        let resolved = resolve_destination(output)?;
        if input_paths.contains(&resolved) {
            bail!("output path {} aliases an input evidence file", output.display());
        }
        if let Some(identity) = file_identity(output) {
            if input_identities.contains(&identity) {
                bail!(
                    "output path {} aliases an input evidence file through a hard link",
                    output.display()
                );
            }
            if !output_identities.insert(identity) {
                bail!("multiple output options resolve to the same file");
            }
        }
        if !output_paths.insert(resolved) {
            bail!("multiple output options resolve to the same path");
        }
    }
    Ok(())
}

/// Reject destinations that would overwrite the measured subject itself.
///
/// The runner script is not the only input `capture-discovery` depends on: it
/// executes `host_perl` and reads the prepared tree. Writing evidence over
/// either mutates the subject after measurement while still reporting success.
fn reject_subject_destinations(
    host_perl: &Path,
    perl_tree: &Path,
    outputs: &[PathBuf],
) -> Result<()> {
    let host_identity = file_identity(host_perl);
    let host_canonical = fs::canonicalize(host_perl).ok();
    for output in outputs {
        let resolved = resolve_destination(output)?;
        if host_canonical.as_deref() == Some(resolved.as_path())
            || (host_identity.is_some() && file_identity(output) == host_identity)
        {
            bail!("output path {} would overwrite the host Perl under test", output.display());
        }
        if resolved.starts_with(perl_tree) {
            bail!(
                "output path {} resolves inside the prepared Perl tree {}",
                output.display(),
                perl_tree.display()
            );
        }
    }
    Ok(())
}

/// Resolve a destination to the path that would actually be written.
///
/// The path is walked forward from the root, canonicalizing each prefix that
/// already exists before continuing. That ordering matters: `..` after a symlink
/// means the parent of the *target*, so folding it lexically without resolving
/// the symlink first would name a different file. Components that do not exist
/// yet cannot be symlinks, so folding those lexically is exact.
///
/// Both guards depend on this. `reject_subject_destinations` compares the result
/// against the prepared tree with `starts_with`, which matches components
/// textually, and `reject_output_aliases` compares it against canonicalized
/// inputs. An unfolded `<elsewhere>/../<perl-tree>/t/x.json` would slip past
/// both and then be written inside the measured subject.
fn resolve_destination(path: &Path) -> Result<PathBuf> {
    use std::path::Component;

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().context("reading current directory")?.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => resolved.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(part) => {
                resolved.push(part);
                if resolved.exists() {
                    resolved = fs::canonicalize(&resolved).with_context(|| {
                        format!("canonicalizing output path component {}", resolved.display())
                    })?;
                }
            }
        }
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
    format!("sha256:{}", encode_hex(&Sha256::digest(bytes)))
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

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("reading JSON {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("decoding JSON {}", path.display()))
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
    use perl_core_harness_types::SemanticBoundarySourceSpan;
    use perl_core_harness_types::{
        RunSummary, SemanticBoundaryConfidence, SemanticBoundaryDisposition,
        SemanticBoundaryLockScope,
    };
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
            subject: sample_subject(),
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
        let stream = RawByteStream::from_capture(capture_stream(Cursor::new(bytes), limit), limit);
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
        let contradictory_failure = contradictory_success
            .replace("\"status\":7", "\"status\":0")
            .replace("\"success\":true", "\"success\":false");
        let missing_outcome =
            contradictory_success.replace("\"status\":7,", "").replace("\"success\":true,", "");
        for invalid in [contradictory_success.to_string(), contradictory_failure, missing_outcome] {
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
            subject: sample_subject(),
            working_directory: "t".into(),
            argv: vec!["TEST".into(), "--dumptests".into()],
            process: DiscoveryProcessOutcome::Exited { code: 0 },
            stdout: stream,
            stderr: RawByteStream::empty(RAW_STREAM_MAX_BYTES),
        };
        assert!(
            envelope.complete_success(),
            "a zero exit with complete streams must be authoritative success"
        );
        envelope.process = DiscoveryProcessOutcome::Exited { code: 1 };
        assert!(!envelope.complete_success(), "a nonzero exit must not be authoritative success");
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

    fn sample_subject() -> DiscoverySubject {
        DiscoverySubject {
            commit: "a".repeat(40),
            perl_ref: "blead".into(),
            runner_script: "TEST".into(),
            runner_script_sha256: sha256_digest(b"TEST"),
            host_perl_file_name: "perl".into(),
            host_perl_sha256: sha256_digest(b"perl"),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    fn raw_envelope(stdout: &[u8], stderr: &[u8]) -> DiscoveryRawEnvelope {
        DiscoveryRawEnvelope {
            schema_version: DISCOVERY_RAW_SCHEMA_VERSION.into(),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            subject: sample_subject(),
            working_directory: "t".into(),
            argv: vec!["TEST".into(), "--dumptests".into()],
            process: DiscoveryProcessOutcome::Exited { code: 0 },
            stdout: RawByteStream::from_capture(
                capture_stream(Cursor::new(stdout.to_vec()), RAW_STREAM_MAX_BYTES),
                RAW_STREAM_MAX_BYTES,
            ),
            stderr: RawByteStream::from_capture(
                capture_stream(Cursor::new(stderr.to_vec()), RAW_STREAM_MAX_BYTES),
                RAW_STREAM_MAX_BYTES,
            ),
        }
    }

    #[cfg(unix)]
    fn bounded_shell(script: &str, limits: &CaptureLimits) -> DiscoveryProcessOutcome {
        let mut command = Command::new("sh");
        command.arg("-c").arg(script);
        run_bounded_command(command, limits).0
    }

    #[cfg(unix)]
    fn short_deadline(cancel_file: Option<PathBuf>) -> CaptureLimits {
        CaptureLimits { deadline: Duration::from_secs(1), cancel_file }
    }

    #[test]
    #[cfg(unix)]
    fn signal_identity_does_not_collapse_distinct_terminations() -> TestResult {
        use std::os::unix::process::ExitStatusExt;

        let sigterm = outcome_from_status(ExitStatus::from_raw(15));
        let sigkill = outcome_from_status(ExitStatus::from_raw(9));
        let DiscoveryProcessOutcome::Signaled { signal: 15, core_dumped: false, .. } = &sigterm
        else {
            bail!("SIGTERM termination lost its signal identity: {sigterm:?}");
        };
        if sigterm == sigkill {
            bail!("SIGTERM and SIGKILL terminations collapsed into one outcome");
        }
        sigterm.validate()?;
        sigkill.validate()?;
        if sigterm.succeeded() || sigkill.succeeded() {
            bail!("signalled termination must never be authoritative success");
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn core_dump_identity_is_retained_separately_from_the_signal() -> TestResult {
        use std::os::unix::process::ExitStatusExt;

        let dumped = outcome_from_status(ExitStatus::from_raw(6 | 0x80));
        let undumped = outcome_from_status(ExitStatus::from_raw(6));
        let DiscoveryProcessOutcome::Signaled { signal: 6, core_dumped: true, signal_name } =
            &dumped
        else {
            bail!("core-dumping termination lost its identity: {dumped:?}");
        };
        if signal_name.trim().is_empty() {
            bail!("signalled termination recorded an empty signal name");
        }
        if dumped == undumped {
            bail!("core-dump state collapsed into the plain signal outcome");
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn complete_capture_is_not_reported_as_a_deadline() -> TestResult {
        let limits = CaptureLimits { deadline: Duration::from_secs(30), cancel_file: None };
        let mut command = Command::new("sh");
        command.arg("-c").arg("printf 'base/ok.t\\nbase/two.t\\n'; printf 'note\\n' >&2");
        let (process, stdout, stderr) = run_bounded_command(command, &limits);
        if process != (DiscoveryProcessOutcome::Exited { code: 0 }) {
            bail!(
                "a promptly exiting process must not acquire a bounded-capture outcome: {process:?}"
            );
        }
        if !stdout.complete() || !stderr.complete() {
            bail!("a promptly exiting process must produce complete stream capture");
        }
        let envelope = DiscoveryRawEnvelope { process, stdout, stderr, ..raw_envelope(b"", b"") };
        envelope.validate()?;
        let derived = DiscoveryDerivedEnvelope::derive(&envelope)?;
        if derived.test_paths != vec!["base/ok.t".to_string(), "base/two.t".to_string()] {
            bail!("live capture did not normalize to its declared paths: {:?}", derived.test_paths);
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn hung_process_is_bounded_by_the_capture_deadline() -> TestResult {
        let started = Instant::now();
        let outcome = bounded_shell("sleep 60", &short_deadline(None));
        let DiscoveryProcessOutcome::TimedOut { phase: CaptureDeadlinePhase::Process, .. } =
            outcome
        else {
            bail!("a hung discovery process must time out during the process phase: {outcome:?}");
        };
        if started.elapsed() >= TERMINATION_HARD_LIMIT {
            bail!("bounded capture did not return before the termination hard limit");
        }
        outcome.validate()?;
        if outcome.succeeded() {
            bail!("a timed-out capture must never be authoritative success");
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn forked_grandchild_cannot_hold_capture_open_past_the_deadline() -> TestResult {
        let started = Instant::now();
        let outcome = bounded_shell("sleep 60 & exit 0", &short_deadline(None));
        let DiscoveryProcessOutcome::TimedOut { phase: CaptureDeadlinePhase::StreamDrain, .. } =
            outcome
        else {
            bail!(
                "a descendant retaining the inherited pipes must time out during stream drain: {outcome:?}"
            );
        };
        if started.elapsed() >= TERMINATION_HARD_LIMIT {
            bail!("process-tree cleanup did not release the captured pipes before the hard limit");
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn cancellation_is_distinct_from_the_deadline() -> TestResult {
        let temp = tempfile::tempdir()?;
        let cancel = temp.path().join("cancel");
        fs::write(&cancel, "requested\n")?;
        let started = Instant::now();
        let outcome = bounded_shell("sleep 60", &short_deadline(Some(cancel)));
        let DiscoveryProcessOutcome::Cancelled { .. } = outcome else {
            bail!("a requested cancellation must not be reported as a deadline: {outcome:?}");
        };
        if started.elapsed() >= Duration::from_secs(1) {
            bail!("cancellation was not observed before the capture deadline could fire");
        }
        outcome.validate()?;
        if outcome.succeeded() {
            bail!("a cancelled capture must never be authoritative success");
        }
        Ok(())
    }

    #[test]
    fn deadline_must_stay_finite_and_positive() -> TestResult {
        if parse_deadline(None)? != Duration::from_secs(DEFAULT_CAPTURE_DEADLINE_SECONDS) {
            bail!("the default capture deadline changed without updating its contract");
        }
        if parse_deadline(Some("0")).is_ok() {
            bail!("a zero deadline must be rejected");
        }
        if parse_deadline(Some(&(MAX_CAPTURE_DEADLINE_SECONDS + 1).to_string())).is_ok() {
            bail!("an unbounded deadline must be rejected");
        }
        if parse_deadline(Some("later")).is_ok() {
            bail!("a non-numeric deadline must be rejected");
        }
        Ok(())
    }

    #[test]
    fn non_authoritative_outcomes_cannot_produce_derived_evidence() -> TestResult {
        for process in [
            DiscoveryProcessOutcome::TimedOut {
                deadline_ms: 1_000,
                phase: CaptureDeadlinePhase::Process,
            },
            DiscoveryProcessOutcome::Cancelled { source: "operator".into() },
            DiscoveryProcessOutcome::Signaled {
                signal: 9,
                signal_name: "SIGKILL".into(),
                core_dumped: false,
            },
            DiscoveryProcessOutcome::TerminatedWithoutIdentity { platform: "windows".into() },
        ] {
            let envelope = DiscoveryRawEnvelope { process, ..raw_envelope(b"base/ok.t\n", b"") };
            envelope.validate()?;
            if envelope.complete_success() {
                bail!("a non-authoritative outcome must not derive complete success");
            }
            if DiscoveryDerivedEnvelope::derive(&envelope).is_ok() {
                bail!("normalized evidence must not be derived from non-authoritative raw bytes");
            }
        }
        Ok(())
    }

    #[test]
    fn incomplete_streams_cannot_produce_derived_evidence() -> TestResult {
        let limit = 4;
        let mut envelope = raw_envelope(b"", b"");
        envelope.stdout = RawByteStream::from_capture(
            capture_stream(Cursor::new(b"base/ok.t\n".to_vec()), limit),
            limit,
        );
        envelope.validate()?;
        if DiscoveryDerivedEnvelope::derive(&envelope).is_ok() {
            bail!("truncated stdout must not be normalized as though it were complete");
        }
        Ok(())
    }

    #[test]
    fn derived_discovery_rejects_replay_against_different_raw_bytes() -> TestResult {
        let recorded = raw_envelope(b"base/ok.t\n", b"");
        let derived = DiscoveryDerivedEnvelope::derive(&recorded)?;
        derived.validate_against(&recorded)?;

        let different_stdout = raw_envelope(b"base/other.t\n", b"");
        if derived.validate_against(&different_stdout).is_ok() {
            bail!("derived discovery was accepted against different stdout bytes");
        }
        let different_stderr = raw_envelope(b"base/ok.t\n", b"warning\n");
        if derived.validate_against(&different_stderr).is_ok() {
            bail!("derived discovery was accepted against different stderr bytes");
        }
        Ok(())
    }

    #[test]
    fn derived_discovery_rejects_normalization_preserving_byte_drift() -> TestResult {
        let recorded = raw_envelope(b"base/ok.t\n", b"");
        let derived = DiscoveryDerivedEnvelope::derive(&recorded)?;
        // The trailing comment is discarded by the normalizer, so only the bound
        // raw identity can detect that the upstream bytes were not the same.
        let drifted = raw_envelope(b"base/ok.t\nskipped note\n", b"");
        let redrived = DiscoveryDerivedEnvelope::derive(&drifted)?;
        if redrived.test_paths != derived.test_paths {
            bail!("fixture must produce identical normalized output from different raw bytes");
        }
        if derived.validate_against(&drifted).is_ok() {
            bail!("derived discovery was accepted against raw bytes it did not summarize");
        }
        Ok(())
    }

    #[test]
    fn derived_discovery_rejects_transform_identity_drift() -> TestResult {
        let recorded = raw_envelope(b"base/ok.t\n", b"");
        let mut decoder_drift = DiscoveryDerivedEnvelope::derive(&recorded)?;
        decoder_drift.decoder_version = "utf8_lossy.v1".into();
        if decoder_drift.validate_against(&recorded).is_ok() {
            bail!("derived discovery was accepted under an unrecorded decoder");
        }
        let mut normalizer_drift = DiscoveryDerivedEnvelope::derive(&recorded)?;
        normalizer_drift.normalizer_version = "discovery_test_paths.v2".into();
        if normalizer_drift.validate_against(&recorded).is_ok() {
            bail!("derived discovery was accepted under an unrecorded normalizer");
        }
        let mut schema_drift = DiscoveryDerivedEnvelope::derive(&recorded)?;
        schema_drift.schema_version = "perl_core_harness.discovery_derived.v0".into();
        if schema_drift.validate_against(&recorded).is_ok() {
            bail!("derived discovery was accepted under an unsupported schema");
        }
        Ok(())
    }

    #[test]
    fn derived_discovery_rejects_normalized_content_drift() -> TestResult {
        let recorded = raw_envelope(b"base/ok.t\n", b"");
        let mut tampered = DiscoveryDerivedEnvelope::derive(&recorded)?;
        tampered.test_paths.push("base/invented.t".into());
        tampered.normalized_sha256 = normalized_digest(&tampered.test_paths);
        if tampered.validate_against(&recorded).is_ok() {
            bail!("derived discovery accepted paths that its raw bytes never declared");
        }
        let mut digest_only = DiscoveryDerivedEnvelope::derive(&recorded)?;
        digest_only.normalized_sha256 = sha256_digest(b"unrelated");
        if digest_only.validate_against(&recorded).is_ok() {
            bail!("derived discovery accepted a digest that does not cover its own paths");
        }
        Ok(())
    }

    #[test]
    fn check_discovery_binds_derived_records_to_their_raw_evidence() -> TestResult {
        let temp = tempfile::tempdir()?;
        let raw_path = temp.path().join("discovery_raw.json");
        let derived_path = temp.path().join("discovery_derived.json");
        let recorded = raw_envelope(b"base/ok.t\n", b"");
        write_json(&raw_path, &recorded)?;
        write_json(&derived_path, &DiscoveryDerivedEnvelope::derive(&recorded)?)?;
        check_discovery(CheckDiscoveryConfig {
            raw: raw_path.clone(),
            derived: derived_path.clone(),
        })?;

        write_json(&raw_path, &raw_envelope(b"base/replaced.t\n", b""))?;
        let Err(error) =
            check_discovery(CheckDiscoveryConfig { raw: raw_path, derived: derived_path })
        else {
            bail!("check-discovery accepted a derived record replayed against other raw bytes");
        };
        if !error.to_string().contains("binding derived discovery") {
            bail!("unexpected check-discovery error: {error}");
        }
        Ok(())
    }

    #[test]
    fn derived_discovery_rejects_replay_across_measured_subjects() -> TestResult {
        let recorded = raw_envelope(b"base/ok.t\n", b"");
        let derived = DiscoveryDerivedEnvelope::derive(&recorded)?;
        // Byte-identical output from a different upstream tree: only the subject
        // binding can tell the two captures apart.
        let mut other_subject = raw_envelope(b"base/ok.t\n", b"");
        other_subject.subject.commit = "b".repeat(40);
        if other_subject.stdout != recorded.stdout {
            bail!("fixture must hold the raw bytes identical across subjects");
        }
        if derived.validate_against(&other_subject).is_ok() {
            bail!("derived discovery was accepted against a different measured subject");
        }
        Ok(())
    }

    #[test]
    fn discovery_subject_rejects_host_paths_and_malformed_digests() -> TestResult {
        for host_path in ["/usr/bin/perl", "C:\\perl\\perl.exe", "bin\\perl"] {
            let mut subject = sample_subject();
            subject.host_perl_file_name = host_path.into();
            if subject.validate().is_ok() {
                bail!("host path {host_path} must not be published as subject identity");
            }
        }
        let mut runner_path = sample_subject();
        runner_path.runner_script = "t\\TEST".into();
        if runner_path.validate().is_ok() {
            bail!("a Windows-shaped runner script path must not be published as identity");
        }
        let mut short_digest = sample_subject();
        short_digest.runner_script_sha256 = "sha256:beef".into();
        if short_digest.validate().is_ok() {
            bail!("a malformed runner-script digest must be rejected");
        }
        let mut empty_commit = sample_subject();
        empty_commit.commit = "  ".into();
        if empty_commit.validate().is_ok() {
            bail!("an empty upstream commit must be rejected");
        }
        Ok(())
    }

    #[test]
    fn raw_envelope_rejects_absolute_working_directories() -> TestResult {
        let mut leaked = raw_envelope(b"base/ok.t\n", b"");
        leaked.working_directory = "/home/runner/work/perl/t".into();
        let Err(error) = leaked.validate() else {
            bail!("an absolute host path must not be published as evidence");
        };
        if !error.to_string().contains("relative to the prepared tree") {
            bail!("unexpected working-directory error: {error}");
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn output_aliasing_an_input_through_a_hard_link_is_rejected() -> TestResult {
        let temp = tempfile::tempdir()?;
        let report = temp.path().join("report.json");
        let output = temp.path().join("records.jsonl");
        fs::write(&report, "{}\n")?;
        fs::hard_link(&report, &output)?;
        if fs::canonicalize(&report)? == fs::canonicalize(&output)? {
            bail!("fixture must produce two distinct names for one inode");
        }
        let Err(error) = reject_output_aliases(std::slice::from_ref(&report), &[output]) else {
            bail!("an output hard-linked to an input must be rejected before writing");
        };
        if !error.to_string().contains("hard link") {
            bail!("unexpected hard-link alias error: {error}");
        }
        Ok(())
    }

    #[test]
    fn discovery_outputs_cannot_overwrite_the_measured_subject() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = temp.path().join("perl");
        fs::create_dir_all(perl_tree.join("t"))?;
        let host_perl = temp.path().join("perl-bin");
        fs::write(&host_perl, "#!/bin/sh\n")?;
        let perl_tree = fs::canonicalize(&perl_tree)?;

        let Err(error) =
            reject_subject_destinations(&host_perl, &perl_tree, std::slice::from_ref(&host_perl))
        else {
            bail!("an output naming the host Perl must be rejected");
        };
        if !error.to_string().contains("overwrite the host Perl") {
            bail!("unexpected host-Perl overwrite error: {error}");
        }

        let inside = perl_tree.join("t").join("discovery_raw.json");
        let Err(error) = reject_subject_destinations(&host_perl, &perl_tree, &[inside]) else {
            bail!("an output inside the prepared tree must be rejected");
        };
        if !error.to_string().contains("inside the prepared Perl tree") {
            bail!("unexpected prepared-tree overwrite error: {error}");
        }

        reject_subject_destinations(
            &host_perl,
            &perl_tree,
            &[temp.path().join("discovery_raw.json")],
        )?;
        Ok(())
    }

    #[test]
    fn report_validation_consumes_the_crate_bucket_invariant() -> TestResult {
        // The producer must not carry its own opinion about bucket shape: the
        // crate already owns this invariant, and two copies would let the
        // derived records and the histogram other consumers read disagree about
        // the same authoritative receipt.
        let bucketed = |bucket: &str| {
            let mut report = sample_report(HarnessMode::Compile);
            report.file_results[1].status = RunnerStatus::Fail;
            report.summary.files_passed = 1;
            report.summary.files_failed = 1;
            report.failures.push(RunFailure {
                path: "base/other.t".into(),
                phase: "compile".into(),
                bucket: bucket.into(),
                first_diagnostic: "syntax error".into(),
                workstream: "parser".into(),
                lsp_impact: Vec::new(),
            });
            report
        };
        // A well-bucketed failure is accepted, so the two rejections below are
        // not an artifact of the fixture being invalid for some other reason.
        validate_report(&bucketed("parse_recovery"))?;

        let Err(error) = validate_report(&bucketed("unknown")) else {
            bail!("a failure bucketed as unknown must be rejected");
        };
        if !error.to_string().contains("run report buckets") {
            bail!("unexpected bucket error: {error}");
        }
        if validate_report(&bucketed("  ")).is_ok() {
            bail!("a failure with an empty bucket must be rejected");
        }

        // A failing file with no failure record at all is the same invariant.
        let mut unrecorded = bucketed("parse_recovery");
        unrecorded.failures.clear();
        if validate_report(&unrecorded).is_ok() {
            bail!("a failing file with no failure bucket record must be rejected");
        }
        Ok(())
    }

    #[test]
    fn traversing_destinations_cannot_escape_either_guard() -> TestResult {
        let temp = tempfile::tempdir()?;
        let perl_tree = temp.path().join("perl");
        fs::create_dir_all(perl_tree.join("t"))?;
        let host_perl = temp.path().join("perl-bin");
        fs::write(&host_perl, "#!/bin/sh\n")?;
        let perl_tree = fs::canonicalize(&perl_tree)?;

        // `absent` does not exist, so the destination's `..` survives into the
        // rejoined suffix. Without folding, `starts_with` compares components
        // textually, the guard accepts it, and the write lands in the tree.
        let traversing = temp.path().join("absent").join("..").join("perl/t/raw.json");
        let Err(error) =
            reject_subject_destinations(&host_perl, &perl_tree, std::slice::from_ref(&traversing))
        else {
            bail!("a traversing destination must not escape the prepared-tree guard");
        };
        if !error.to_string().contains("inside the prepared Perl tree") {
            bail!("unexpected traversal error: {error}");
        }

        // The same shape must not defeat the input-alias guard either.
        let report = temp.path().join("report.json");
        fs::write(&report, "{}\n")?;
        let aliasing = temp.path().join("absent").join("..").join("report.json");
        if reject_output_aliases(std::slice::from_ref(&report), std::slice::from_ref(&aliasing))
            .is_ok()
        {
            bail!("a traversing destination must not defeat the input-alias guard");
        }

        // A destination that genuinely resolves elsewhere is still accepted, so
        // the two rejections above are not a blanket ban on `..`.
        let elsewhere = temp.path().join("out").join("..").join("raw.json");
        reject_subject_destinations(&host_perl, &perl_tree, std::slice::from_ref(&elsewhere))?;
        Ok(())
    }

    #[test]
    fn legacy_terminated_without_code_outcome_is_not_silently_current() -> TestResult {
        let legacy = r#"{"kind":"terminated_without_code"}"#;
        if serde_json::from_str::<DiscoveryProcessOutcome>(legacy).is_ok() {
            bail!("the collapsed termination outcome must not decode as current evidence");
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
