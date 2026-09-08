//! Bounded real-Perl compile/parse oracle for quote-like matrix rows.

use super::schema::{ORACLE_INVOCATION, OracleExpectation};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const ORACLE_TIMEOUT: Duration = Duration::from_secs(2);
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    let source_path = temp_dir.join("quote_like_row.pl");
    if let Err(error) = fs::write(&source_path, source) {
        return OracleResult::NotProven { reason: format!("writing oracle tempfile: {error}") };
    }

    let timeout = match resolve_timeout() {
        Ok(path) => path,
        Err(reason) => return OracleResult::NotProven { reason },
    };

    let path_var = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string());
    let output = Command::new(&timeout)
        .args(["--signal=KILL", "2"])
        .arg(&executable)
        .arg("-c")
        .arg(&source_path)
        .env_clear()
        .env("PATH", &path_var)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let _ = fs::remove_dir_all(&temp_dir);

    match output {
        Ok(output) => {
            match interpret_oracle_status(output.status.success(), output.status.code()) {
                InterpretedStatus::Accept => OracleResult::Proven {
                    identity: OracleIdentity { executable, version, invocation: ORACLE_INVOCATION },
                    outcome: OracleOutcome::Accept,
                },
                InterpretedStatus::Reject => OracleResult::Proven {
                    identity: OracleIdentity { executable, version, invocation: ORACLE_INVOCATION },
                    outcome: OracleOutcome::Reject,
                },
                InterpretedStatus::NotProven(reason) => OracleResult::NotProven { reason },
            }
        }
        Err(error) => OracleResult::NotProven { reason: format!("spawning oracle: {error}") },
    }
}

pub fn check_expectation(source: &str, expected: OracleExpectation) -> Result<(), String> {
    match expected {
        OracleExpectation::Skip => Ok(()),
        OracleExpectation::CompileAccept | OracleExpectation::CompileReject => {
            assert_oracle_result(compile_source(source), expected)
        }
    }
}

/// GNU coreutils `timeout` statuses that are instrument failures, not `perl -c` rejection.
///
/// `timeout --signal=KILL` documents 137 (128+SIGKILL), not 124. 124 is the default TERM
/// watchdog. 125–127 are timeout-itself / invoke / not-found failures.
fn interpret_oracle_status(success: bool, code: Option<i32>) -> InterpretedStatus {
    if success {
        return InterpretedStatus::Accept;
    }
    match code {
        Some(124) => InterpretedStatus::NotProven(format!(
            "oracle timed out after {ORACLE_TIMEOUT:?} (timeout status 124)"
        )),
        Some(125) => InterpretedStatus::NotProven("timeout itself failed (status 125)".to_string()),
        Some(126) => InterpretedStatus::NotProven(
            "oracle command found but could not be invoked (status 126)".to_string(),
        ),
        Some(127) => InterpretedStatus::NotProven(
            "oracle command could not be found (status 127)".to_string(),
        ),
        Some(137) => InterpretedStatus::NotProven(format!(
            "oracle timed out after {ORACLE_TIMEOUT:?} (timeout --signal=KILL status 137)"
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

fn resolve_timeout() -> Result<PathBuf, String> {
    which("timeout")
}

fn which(name: &str) -> Result<PathBuf, String> {
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string());
    for dir in path.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(dir).join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!("{name} is not available on PATH"))
}

fn read_version(perl: &Path) -> Result<String, String> {
    let output = Command::new(perl)
        .args(["-e", "print $]"])
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| format!("reading perl version: {error}"))?;
    if !output.status.success() {
        return Err("perl version probe failed".to_string());
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
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

fn tempfile_dir() -> Result<PathBuf, String> {
    let unique = std::env::temp_dir().join("quote-like-lexical-oracle").join(format!(
        "{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&unique).map_err(|error| format!("creating oracle row dir: {error}"))?;
    Ok(unique)
}

#[cfg(test)]
mod tests {
    use super::{
        InterpretedStatus, OracleExpectation, OracleIdentity, OracleOutcome, OracleResult,
        assert_oracle_result, dotted_perl_version, interpret_oracle_status,
    };
    use std::path::PathBuf;

    type R = Result<(), String>;

    #[test]
    fn dotted_version_maps_perl_revision() {
        assert_eq!(dotted_perl_version("5.038002").as_deref(), Some("5.38.2"));
    }

    #[test]
    fn oracle_outcomes_are_distinct() {
        assert_ne!(OracleOutcome::Accept, OracleOutcome::Reject);
    }

    #[test]
    fn gnu_timeout_kill_status_is_not_proven_not_reject() -> R {
        match interpret_oracle_status(false, Some(137)) {
            InterpretedStatus::NotProven(reason) => {
                assert!(reason.contains("137"), "{reason}");
                assert!(reason.contains("KILL") || reason.contains("timed out"), "{reason}");
                Ok(())
            }
            other => Err(format!("expected NOT_PROVEN for timeout --signal=KILL, got {other:?}")),
        }
    }

    #[test]
    fn gnu_timeout_term_status_is_not_proven() -> R {
        match interpret_oracle_status(false, Some(124)) {
            InterpretedStatus::NotProven(reason) => {
                assert!(reason.contains("124"), "{reason}");
                Ok(())
            }
            other => Err(format!("expected NOT_PROVEN for timeout status 124, got {other:?}")),
        }
    }

    #[test]
    fn perl_nonzero_compile_status_is_reject() -> R {
        match interpret_oracle_status(false, Some(255)) {
            InterpretedStatus::Reject => {}
            other => return Err(format!("expected Reject for perl -c status 255, got {other:?}")),
        }
        match interpret_oracle_status(true, Some(0)) {
            InterpretedStatus::Accept => Ok(()),
            other => Err(format!("expected Accept for status 0, got {other:?}")),
        }
    }

    #[test]
    fn timeout_instrument_failures_are_not_proven() -> R {
        for code in [125, 126, 127] {
            match interpret_oracle_status(false, Some(code)) {
                InterpretedStatus::NotProven(reason) => {
                    assert!(reason.contains(&code.to_string()), "{reason}");
                }
                other => {
                    return Err(format!(
                        "expected NOT_PROVEN for timeout status {code}, got {other:?}"
                    ));
                }
            }
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
            reason: "oracle timed out after 2s (timeout --signal=KILL status 137)".to_string(),
        };
        let error = match assert_oracle_result(result, OracleExpectation::CompileReject) {
            Err(error) => error,
            Ok(()) => return Err("timed-out CompileReject must not look proven".to_string()),
        };
        assert!(error.contains("NOT_PROVEN"), "{error}");
        assert!(error.contains("137"), "{error}");
        Ok(())
    }

    #[test]
    fn proven_accept_and_reject_still_discriminate() -> R {
        let identity = OracleIdentity {
            executable: PathBuf::from("/usr/bin/perl"),
            version: "5.38.2".to_string(),
            invocation: "timeout --signal=KILL 2 env -i PATH=$PATH LC_ALL=C perl -c <tempfile>",
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
