//! Hermetic bounded real-Perl compile oracle for regex conformance rows (#7036).

use perl_regex::conformance::CompileProbeClass;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const ORACLE_INVOCATION: &str =
    "direct perl -c child with a 2s deadline; env_clear with preserved PATH + LC_ALL=C";

const ORACLE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedCompileOutcome {
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleOutcome {
    Accept,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleIdentity {
    pub executable: PathBuf,
    pub version: String,
    pub invocation: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleResult {
    Proven { identity: OracleIdentity, outcome: OracleOutcome },
    NotProven { reason: String },
}

pub fn probe_identity() -> OracleResult {
    match resolve_perl() {
        Ok(executable) => match read_version(&executable) {
            Ok(version) => OracleResult::Proven {
                identity: OracleIdentity { executable, version, invocation: ORACLE_INVOCATION },
                outcome: OracleOutcome::Accept,
            },
            Err(reason) => OracleResult::NotProven { reason },
        },
        Err(reason) => OracleResult::NotProven { reason },
    }
}

pub fn compile_source(source: &str) -> OracleResult {
    let executable = match resolve_perl() {
        Ok(path) => path,
        Err(reason) => return OracleResult::NotProven { reason },
    };
    let version = match read_version(&executable) {
        Ok(version) => version,
        Err(reason) => return OracleResult::NotProven { reason },
    };
    if !version.starts_with("5.") {
        return OracleResult::NotProven { reason: format!("unsupported Perl profile {version}") };
    }

    let temp_dir = match tempfile_dir() {
        Ok(path) => path,
        Err(reason) => return OracleResult::NotProven { reason },
    };
    let source_path = temp_dir.path().join("regex_conformance_row.pl");
    if let Err(error) = fs::write(&source_path, source) {
        return close_tempdir(
            temp_dir,
            OracleResult::NotProven { reason: format!("writing oracle tempfile: {error}") },
        );
    }

    let mut command = Command::new(&executable);
    configure_environment(&mut command);
    let output = command
        .arg("-c")
        .arg(&source_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("spawning oracle: {error}"))
        .and_then(wait_bounded);

    let result = match output {
        Ok(status) => match interpret_oracle_status(status.success(), status.code()) {
            InterpretedStatus::Accept => OracleResult::Proven {
                identity: OracleIdentity { executable, version, invocation: ORACLE_INVOCATION },
                outcome: OracleOutcome::Accept,
            },
            InterpretedStatus::Reject => OracleResult::Proven {
                identity: OracleIdentity { executable, version, invocation: ORACLE_INVOCATION },
                outcome: OracleOutcome::Reject,
            },
            InterpretedStatus::NotProven(reason) => OracleResult::NotProven { reason },
        },
        Err(reason) => OracleResult::NotProven { reason },
    };
    close_tempdir(temp_dir, result)
}

pub fn classify_probe(result: &OracleResult) -> CompileProbeClass {
    match result {
        OracleResult::Proven { outcome, .. } => match outcome {
            OracleOutcome::Accept => CompileProbeClass::Accepted,
            OracleOutcome::Reject => CompileProbeClass::Rejected,
        },
        OracleResult::NotProven { reason } => {
            if reason.contains("timed out") {
                CompileProbeClass::TimedOut
            } else if reason.contains("not available on PATH")
                || reason.contains("unsupported Perl profile")
            {
                CompileProbeClass::Unavailable
            } else if reason.contains("terminated by signal") || reason.contains("abnormal status")
            {
                CompileProbeClass::Crashed
            } else {
                CompileProbeClass::HarnessError
            }
        }
    }
}

pub fn check_expectation(source: &str, expected: ExpectedCompileOutcome) -> Result<(), String> {
    let result = compile_source(source);
    let class = classify_probe(&result);
    match class {
        CompileProbeClass::Accepted | CompileProbeClass::Rejected => {}
        other => {
            return Err(format!("oracle {} (not agreement): {result:?}", other.as_str()));
        }
    }
    match result {
        OracleResult::NotProven { reason } => {
            Err(format!("oracle NOT_PROVEN (not agreement): {reason}"))
        }
        OracleResult::Proven { outcome, identity } => {
            let want_accept = matches!(expected, ExpectedCompileOutcome::Accepted);
            let got_accept = matches!(outcome, OracleOutcome::Accept);
            if want_accept == got_accept {
                Ok(())
            } else {
                Err(format!(
                    "oracle {} via {} ({}) disagreed: expected {expected:?}, got {outcome:?}",
                    identity.executable.display(),
                    identity.invocation,
                    identity.version
                ))
            }
        }
    }
}

/// A direct Perl child owns every ordinary exit status. Only a missing status
/// or a native status outside the portable exit-code range is NOT_PROVEN.
fn interpret_oracle_status(success: bool, code: Option<i32>) -> InterpretedStatus {
    if success {
        return InterpretedStatus::Accept;
    }
    match code {
        Some(code) if !(1..=255).contains(&code) => InterpretedStatus::NotProven(format!(
            "oracle process exited with abnormal status {code}"
        )),
        Some(_) => InterpretedStatus::Reject,
        None => InterpretedStatus::NotProven(
            "oracle process terminated by signal (no exit status)".to_string(),
        ),
    }
}

fn resolve_perl() -> Result<PathBuf, String> {
    which("perl")
}

fn which(name: &str) -> Result<PathBuf, String> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        #[cfg(windows)]
        if Path::new(name).extension().is_none() {
            let candidate = dir.join(format!("{name}.exe"));
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!("{name} is not available on PATH"))
}

fn read_version(perl: &Path) -> Result<String, String> {
    let temp_dir = tempfile_dir()?;
    let result = (|| {
        let version_path = temp_dir.path().join("perl_version.txt");
        let stdout = File::create(&version_path)
            .map_err(|error| format!("creating version output: {error}"))?;
        let mut command = Command::new(perl);
        configure_environment(&mut command);
        let status = command
            .args(["-e", "print $]"])
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("spawning perl version probe: {error}"))
            .and_then(wait_bounded)?;
        if !status.success() {
            return Err("perl version probe failed".to_string());
        }
        let file = File::open(version_path)
            .map_err(|error| format!("opening perl version output: {error}"))?;
        let mut bytes = Vec::new();
        file.take(129)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("reading perl version output: {error}"))?;
        parse_version_output(&bytes)
    })();
    match temp_dir.close() {
        Ok(()) => result,
        Err(error) => Err(format!("{result:?}; cleaning version probe tempfile: {error}")),
    }
}

fn configure_environment(command: &mut Command) {
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("LC_ALL", "C");
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
}

fn wait_bounded(mut child: Child) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + ORACLE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                let kill = child.kill();
                let reap = child.wait();
                return Err(format!("waiting for oracle: {error}; kill={kill:?}, reap={reap:?}"));
            }
        }
        if Instant::now() >= deadline {
            let kill_result = child.kill();
            let wait_result = child.wait();
            return Err(format!(
                "oracle timed out after {ORACLE_TIMEOUT:?}; kill={kill_result:?}, reap={wait_result:?}"
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn parse_version_output(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > 128 {
        return Err("perl version output exceeded 128 bytes".to_string());
    }
    let raw = String::from_utf8_lossy(bytes).trim().to_string();
    Ok(dotted_perl_version(&raw).unwrap_or(raw))
}

fn dotted_perl_version(raw: &str) -> Option<String> {
    let value: f64 = raw.parse().ok()?;
    let major = value.trunc() as u32;
    let scaled = ((value - f64::from(major)) * 1_000_000.0).round() as u32;
    let minor = scaled / 1000;
    let patch = scaled % 1000;
    Some(format!("{major}.{minor}.{patch}"))
}

fn tempfile_dir() -> Result<tempfile::TempDir, String> {
    tempfile::Builder::new()
        .prefix("perl-regex-conformance-oracle-")
        .tempdir()
        .map_err(|error| format!("creating oracle row dir: {error}"))
}

fn close_tempdir(temp_dir: tempfile::TempDir, result: OracleResult) -> OracleResult {
    match temp_dir.close() {
        Ok(()) => result,
        Err(error) => OracleResult::NotProven {
            reason: format!("{result:?}; cleaning oracle tempfile failed: {error}"),
        },
    }
}

#[derive(Debug)]
enum InterpretedStatus {
    Accept,
    Reject,
    NotProven(String),
}

#[cfg(test)]
mod unit_tests {
    use super::{
        ExpectedCompileOutcome, InterpretedStatus, OracleIdentity, OracleOutcome, OracleResult,
        classify_probe, compile_source, interpret_oracle_status, wait_bounded, which,
    };
    use perl_regex::conformance::CompileProbeClass;
    use std::path::PathBuf;
    use std::process::Command;

    type R = Result<(), String>;

    #[test]
    fn compile_probe_classes_stay_distinct_for_instrument_failures() -> R {
        let unavailable =
            OracleResult::NotProven { reason: "perl is not available on PATH".to_string() };
        assert_eq!(classify_probe(&unavailable), CompileProbeClass::Unavailable);
        let timeout = OracleResult::NotProven {
            reason: "oracle timed out after 2s; kill=Ok, reap=Ok".to_string(),
        };
        assert_eq!(classify_probe(&timeout), CompileProbeClass::TimedOut);
        let crash = OracleResult::NotProven {
            reason: "oracle process terminated by signal (no exit status)".to_string(),
        };
        assert_eq!(classify_probe(&crash), CompileProbeClass::Crashed);
        let proven = OracleResult::Proven {
            identity: OracleIdentity {
                executable: PathBuf::from("/usr/bin/perl"),
                version: "5.38.2".to_string(),
                invocation: super::ORACLE_INVOCATION,
            },
            outcome: OracleOutcome::Accept,
        };
        assert_eq!(classify_probe(&proven), CompileProbeClass::Accepted);
        Ok(())
    }

    #[test]
    fn instrument_failure_is_not_agreement() -> R {
        let timeout = OracleResult::NotProven {
            reason: "oracle timed out after 2s; kill=Ok, reap=Ok".to_string(),
        };
        assert_eq!(classify_probe(&timeout), CompileProbeClass::TimedOut);
        let error =
            match super::check_expectation("BEGIN { sleep 5 }", ExpectedCompileOutcome::Rejected) {
                Err(error) => error,
                Ok(()) => {
                    return Err("timed-out compile must not satisfy CompileReject".to_string());
                }
            };
        assert!(error.contains("NOT_PROVEN") || error.contains("not agreement"), "{error}");
        assert!(error.contains("timed out"), "{error}");
        Ok(())
    }

    #[test]
    fn perl_nonzero_compile_status_is_reject() -> R {
        for code in [1, 124, 125, 126, 127, 137, 255] {
            match interpret_oracle_status(false, Some(code)) {
                InterpretedStatus::Reject => {}
                other => {
                    return Err(format!("expected Reject for Perl status {code}, got {other:?}"));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn direct_perl_exit_status_is_reject() -> R {
        match compile_source("BEGIN { exit 124 }\n") {
            OracleResult::Proven { outcome: OracleOutcome::Reject, .. } => Ok(()),
            other => Err(format!("direct Perl status 124 was not proven Reject: {other:?}")),
        }
    }

    #[test]
    fn missing_executable_is_unavailable_class() -> R {
        let missing = format!("perl-regex-oracle-missing-{}", std::process::id());
        match which(&missing) {
            Err(reason) if reason.contains("not available on PATH") => Ok(()),
            other => Err(format!("expected missing executable refusal, got {other:?}")),
        }
    }

    #[test]
    fn direct_timeout_reaps_owned_perl_child() -> R {
        let perl = super::resolve_perl()
            .map_err(|error| format!("timeout fixture requires Perl: {error}"))?;
        let child = Command::new(perl)
            .args(["-e", "sleep 5"])
            .spawn()
            .map_err(|error| format!("spawning timeout fixture: {error}"))?;
        match wait_bounded(child) {
            Err(reason) if reason.contains("timed out") && reason.contains("reap=Ok") => Ok(()),
            other => Err(format!("expected bounded timeout with successful reap, got {other:?}")),
        }
    }
}
