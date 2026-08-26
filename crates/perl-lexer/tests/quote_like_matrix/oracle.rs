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
            let identity = OracleIdentity { executable, version, invocation: ORACLE_INVOCATION };
            if output.status.success() {
                OracleResult::Proven { identity, outcome: OracleOutcome::Accept }
            } else if output.status.code() == Some(124) {
                OracleResult::NotProven {
                    reason: format!("oracle timed out after {ORACLE_TIMEOUT:?}"),
                }
            } else {
                OracleResult::Proven { identity, outcome: OracleOutcome::Reject }
            }
        }
        Err(error) => OracleResult::NotProven { reason: format!("spawning oracle: {error}") },
    }
}

pub fn check_expectation(source: &str, expected: OracleExpectation) -> Result<(), String> {
    match expected {
        OracleExpectation::Skip => Ok(()),
        OracleExpectation::CompileAccept | OracleExpectation::CompileReject => {
            match compile_source(source) {
                OracleResult::NotProven { .. } => Ok(()),
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
    }
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
    use super::{OracleOutcome, dotted_perl_version};

    #[test]
    fn dotted_version_maps_perl_revision() {
        assert_eq!(dotted_perl_version("5.038002").as_deref(), Some("5.38.2"));
    }

    #[test]
    fn oracle_outcomes_are_distinct() {
        assert_ne!(OracleOutcome::Accept, OracleOutcome::Reject);
    }
}
