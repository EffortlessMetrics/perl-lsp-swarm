//! Bounded real-Perl compile/parse oracle for quote-like matrix rows.

use super::schema::{ORACLE_INVOCATION, OracleExpectation};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const ORACLE_TIMEOUT: Duration = Duration::from_secs(2);

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
        return OracleResult::NotProven { reason: format!("unsupported Perl version {version}") };
    }

    let temp_dir = match tempfile_dir() {
        Ok(path) => path,
        Err(reason) => return OracleResult::NotProven { reason },
    };
    let source_path = temp_dir.path().join("quote_like_row.pl");
    if let Err(error) = fs::write(&source_path, source) {
        return close_tempdir(
            temp_dir,
            OracleResult::NotProven { reason: format!("writing oracle tempfile: {error}") },
        );
    }

    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let output = Command::new(&executable)
        .arg("-c")
        .arg(&source_path)
        .env_clear()
        .env("PATH", &path_var)
        .env("LC_ALL", "C")
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

pub fn check_expectation(source: &str, expected: OracleExpectation) -> Result<(), String> {
    match expected {
        OracleExpectation::Skip => Ok(()),
        OracleExpectation::CompileAccept | OracleExpectation::CompileReject => {
            assert_oracle_result(compile_source(source), expected)
        }
    }
}

/// A direct Perl child owns every ordinary exit status.  Only a missing status
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

fn assert_oracle_result(result: OracleResult, expected: OracleExpectation) -> Result<(), String> {
    match result {
        OracleResult::NotProven { reason } => {
            Err(format!("oracle NOT_PROVEN (not agreement): {reason}"))
        }
        OracleResult::Proven { outcome, identity } => {
            let want_accept = matches!(expected, OracleExpectation::CompileAccept);
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

#[derive(Debug)]
enum InterpretedStatus {
    Accept,
    Reject,
    NotProven(String),
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
        let status = Command::new(perl)
            .args(["-e", "print $]"])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("LC_ALL", "C")
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
        .prefix("quote-like-lexical-oracle-")
        .tempdir()
        .map_err(|error| format!("creating oracle row dir: {error}"))
}

fn close_tempdir(temp_dir: tempfile::TempDir, result: OracleResult) -> OracleResult {
    match temp_dir.close() {
        Ok(()) => result,
        Err(error) => cleanup_failed(result, error.to_string()),
    }
}

fn cleanup_failed(result: OracleResult, error: String) -> OracleResult {
    OracleResult::NotProven {
        reason: format!("{result:?}; cleaning oracle tempfile failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InterpretedStatus, OracleExpectation, OracleIdentity, OracleOutcome, OracleResult,
        assert_oracle_result, cleanup_failed, dotted_perl_version, interpret_oracle_status,
        parse_version_output, resolve_perl, wait_bounded, which,
    };
    use std::path::PathBuf;
    use std::process::Command;

    type R = Result<(), String>;

    #[test]
    fn owned_tempdir_is_removed_and_primary_failure_preserved() -> R {
        let temp_dir = super::tempfile_dir()?;
        let path = temp_dir.path().to_path_buf();
        std::fs::write(path.join("probe.pl"), "1;").map_err(|error| error.to_string())?;
        let expected = OracleResult::NotProven { reason: "spawn failed".to_string() };
        let result = super::close_tempdir(temp_dir, expected.clone());
        if result != expected || path.exists() {
            return Err(format!("cleanup changed failure or retained directory: {result:?}"));
        }
        Ok(())
    }

    #[test]
    fn dotted_version_maps_perl_revision() {
        assert_eq!(dotted_perl_version("5.038002").as_deref(), Some("5.38.2"));
    }

    #[test]
    fn oracle_outcomes_are_distinct() {
        assert_ne!(OracleOutcome::Accept, OracleOutcome::Reject);
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
        match interpret_oracle_status(true, Some(0)) {
            InterpretedStatus::Accept => Ok(()),
            other => Err(format!("expected Accept for status 0, got {other:?}")),
        }
    }

    #[test]
    fn direct_perl_exit_status_is_reject() -> R {
        match super::compile_source("BEGIN { exit 124 }\n") {
            OracleResult::Proven { outcome: OracleOutcome::Reject, .. } => Ok(()),
            other => Err(format!("direct Perl status 124 was not proven Reject: {other:?}")),
        }
    }

    #[test]
    fn missing_executable_is_not_found() -> R {
        let missing = format!("perl-quote-oracle-missing-{}", std::process::id());
        match which(&missing) {
            Err(reason) if reason.contains("not available on PATH") => Ok(()),
            other => Err(format!("expected missing executable refusal, got {other:?}")),
        }
    }

    #[test]
    fn direct_timeout_reaps_owned_perl_child() -> R {
        let perl =
            resolve_perl().map_err(|error| format!("timeout fixture requires Perl: {error}"))?;
        let child = Command::new(perl)
            .args(["-e", "sleep 5"])
            .spawn()
            .map_err(|error| format!("spawning timeout fixture: {error}"))?;
        match wait_bounded(child) {
            Err(reason) if reason.contains("timed out") && reason.contains("reap=Ok") => Ok(()),
            other => Err(format!("expected bounded timeout with successful reap, got {other:?}")),
        }
    }

    #[test]
    fn version_output_is_bounded_before_parsing() -> R {
        let oversized = vec![b'5'; 129];
        let error = match parse_version_output(&oversized) {
            Err(error) => error,
            Ok(version) => return Err(format!("oversized version was accepted: {version}")),
        };
        if !error.contains("exceeded 128 bytes") {
            return Err(format!("unexpected oversized-version error: {error}"));
        }
        match parse_version_output(b"5.038002\n") {
            Ok(version) if version == "5.38.2" => Ok(()),
            other => Err(format!("valid bounded version was not parsed: {other:?}")),
        }
    }

    #[test]
    fn cleanup_failure_preserves_primary_result() -> R {
        let result = cleanup_failed(
            OracleResult::NotProven { reason: "compile child failed".to_string() },
            "access denied".to_string(),
        );
        match result {
            OracleResult::NotProven { reason }
                if reason.contains("compile child failed") && reason.contains("access denied") =>
            {
                Ok(())
            }
            other => Err(format!("cleanup failure lost primary result: {other:?}")),
        }
    }

    #[test]
    fn missing_or_abnormal_status_is_not_proven() -> R {
        match interpret_oracle_status(false, Some(-1_073_741_819)) {
            InterpretedStatus::NotProven(reason) if reason.contains("abnormal status") => {}
            other => return Err(format!("expected abnormal status NOT_PROVEN, got {other:?}")),
        }
        match interpret_oracle_status(false, None) {
            InterpretedStatus::NotProven(_) => Ok(()),
            other => Err(format!("expected NOT_PROVEN for missing exit status, got {other:?}")),
        }
    }

    #[test]
    fn check_expectation_propagates_not_proven_as_failure() -> R {
        let result =
            OracleResult::NotProven { reason: "perl is not available on PATH".to_string() };
        let error = match assert_oracle_result(result, OracleExpectation::CompileAccept) {
            Err(error) => error,
            Ok(()) => return Err("NOT_PROVEN must not become agreement".to_string()),
        };
        assert!(error.contains("NOT_PROVEN"), "{error}");
        assert!(error.contains("not agreement"), "{error}");
        assert!(error.contains("perl is not available on PATH"), "{error}");
        Ok(())
    }

    #[test]
    fn check_expectation_propagates_timeout_not_proven_for_compile_reject_rows() -> R {
        let result = OracleResult::NotProven {
            reason: "oracle timed out after 2s; kill=Ok, reap=Ok".to_string(),
        };
        let error = match assert_oracle_result(result, OracleExpectation::CompileReject) {
            Err(error) => error,
            Ok(()) => return Err("timed-out CompileReject must not look proven".to_string()),
        };
        assert!(error.contains("NOT_PROVEN"), "{error}");
        if !error.contains("timed out") {
            return Err(format!("timeout reason was lost: {error}"));
        }
        Ok(())
    }

    #[test]
    fn proven_accept_and_reject_still_discriminate() -> R {
        let identity = OracleIdentity {
            executable: PathBuf::from("/usr/bin/perl"),
            version: "5.38.2".to_string(),
            invocation: "direct perl -c child with a 2s deadline; env_clear with preserved PATH + LC_ALL=C",
        };
        match assert_oracle_result(
            OracleResult::Proven { identity: identity.clone(), outcome: OracleOutcome::Accept },
            OracleExpectation::CompileAccept,
        ) {
            Ok(()) => {}
            Err(error) => return Err(format!("CompileAccept vs Accept must succeed: {error}")),
        }
        let reject_error = match assert_oracle_result(
            OracleResult::Proven { identity, outcome: OracleOutcome::Accept },
            OracleExpectation::CompileReject,
        ) {
            Err(error) => error,
            Ok(()) => return Err("Accept must not satisfy CompileReject".to_string()),
        };
        assert!(reject_error.contains("disagreed"), "{reject_error}");
        Ok(())
    }
}
