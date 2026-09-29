//! Exercise the editor-visible stderr boundary, including the documented opt-in.
//!
//! Spawn, exit-status, and UTF-8 failures are fallible helpers so a setup
//! problem reports a useful `Err` instead of panicking. Process-level cases
//! still own the INFO / quiet / color-isolation assertions.

#![deny(clippy::map_err_ignore)]

use std::io;
use std::process::{Command, Output, Stdio};

type TestError = Box<dyn std::error::Error>;
type TestResult = Result<(), TestError>;

/// Child environment keys that must not leak from the test runner into the
/// editor-visible stderr boundary.
///
/// `RUST_LOG` / `PERL_LSP_LOG` enable INFO without `--log`. Color-forcing
/// keys make `should_use_ansi_stderr` paint a piped stderr, so the INFO token
/// would no longer match the plain-text assertions. `PERL_LSP_QUIET` is
/// cleared so default and `--log` sessions can still emit the banner unless a
/// test opts into quiet explicitly.
const CLEARED_CHILD_ENV: &[&str] = &[
    "RUST_LOG",
    "PERL_LSP_LOG",
    "PERL_LSP_LOG_FILE",
    "PERL_LSP_QUIET",
    "FORCE_COLOR",
    "CLICOLOR_FORCE",
    "TERM_PROGRAM",
];

/// Spawn `perllsp` with the log surface reduced to its documented defaults.
fn spawn_perllsp(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_perllsp"));
    command.args(args).stdin(Stdio::null());
    for key in CLEARED_CHILD_ENV {
        command.env_remove(key);
    }
    command
}

/// Decode a successful server's stderr, or report spawn / status / UTF-8 failure.
fn successful_stderr(output: Result<Output, io::Error>) -> Result<String, TestError> {
    let output = output.map_err(|error| format!("start perllsp: {error}"))?;
    if !output.status.success() {
        return Err(format!("server failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    String::from_utf8(output.stderr)
        .map_err(|error| format!("server stderr is UTF-8: {error}").into())
}

fn stderr_for(args: &[&str]) -> Result<String, TestError> {
    successful_stderr(spawn_perllsp(args).output())
}

/// Build a real [`Output`] with a chosen success bit and stderr payload.
fn finished_output(success: bool, stderr: &[u8]) -> Result<Output, TestError> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_perllsp"));
    if success {
        command.arg("--version");
    } else {
        command.args(["--completion", "nushell"]);
    }
    let mut output = command
        .output()
        .map_err(|error| format!("synthesize exit status success={success}: {error}"))?;
    if output.status.success() != success {
        return Err(format!(
            "perllsp could not synthesize success={success}: status={:?}, stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    output.stdout.clear();
    output.stderr = stderr.to_vec();
    Ok(output)
}

fn require_err(result: Result<String, TestError>, why: &str) -> Result<TestError, TestError> {
    match result {
        Err(error) => Ok(error),
        Ok(stderr) => Err(format!("{why}: accepted stderr {stderr:?}").into()),
    }
}

#[test]
fn spawn_failure_reports_start_context() -> TestResult {
    let error = require_err(
        successful_stderr(Err(io::Error::new(io::ErrorKind::NotFound, "missing perllsp"))),
        "spawn IO failure must not decode as success",
    )?;
    let message = error.to_string();
    assert!(message.contains("start perllsp"), "missing spawn context: {message}");
    assert!(message.contains("missing perllsp"), "missing IO cause: {message}");
    Ok(())
}

#[test]
fn failed_status_reports_stderr_without_panic() -> TestResult {
    let error = require_err(
        successful_stderr(Ok(finished_output(false, b"boom from child")?)),
        "non-success status must not decode as success",
    )?;
    let message = error.to_string();
    assert!(message.contains("server failed"), "missing status context: {message}");
    assert!(message.contains("boom from child"), "missing child stderr: {message}");
    Ok(())
}

#[test]
fn failed_status_keeps_invalid_utf8_on_the_status_path() -> TestResult {
    let error = require_err(
        successful_stderr(Ok(finished_output(false, b"boom\xff")?)),
        "failed status with invalid UTF-8 must stay a status error",
    )?;
    let message = error.to_string();
    assert!(message.contains("server failed"), "status path lost: {message}");
    assert!(message.contains("boom"), "lossy status report dropped prefix: {message}");
    assert!(
        !message.to_ascii_lowercase().contains("utf-8"),
        "invalid stderr on a failed status must not be reported as a UTF-8 success-path error: {message}"
    );
    Ok(())
}

#[test]
fn invalid_utf8_success_stderr_is_rejected() -> TestResult {
    let error = require_err(
        successful_stderr(Ok(finished_output(true, &[0xff])?)),
        "invalid UTF-8 on a successful status must not become a string",
    )?;
    let message = error.to_string();
    assert!(message.to_ascii_lowercase().contains("utf-8"), "missing UTF-8 context: {message}");
    assert!(!message.contains('\u{FFFD}'), "lossy decode hid the UTF-8 failure: {message}");
    Ok(())
}

#[test]
fn successful_utf8_stderr_is_returned_verbatim() -> TestResult {
    let stderr = successful_stderr(Ok(finished_output(true, "ok\nINFO token".as_bytes())?))?;
    assert_eq!(stderr, "ok\nINFO token");
    Ok(())
}

#[test]
fn stderr_info_logging_requires_opt_in() -> TestResult {
    let default = stderr_for(&["--stdio"])?;
    assert!(!default.contains(" INFO "), "default session leaked INFO logs: {default}");
    assert!(
        !default.contains("Workspace indexing receipt"),
        "default session leaked telemetry: {default}"
    );
    assert!(
        !default.contains('\u{1b}'),
        "default session leaked ANSI into the editor-visible boundary: {default:?}"
    );

    let explicit = stderr_for(&["--stdio", "--log"])?;
    assert!(explicit.contains(" INFO "), "--log did not enable INFO logs: {explicit}");
    assert!(
        !explicit.contains('\u{1b}'),
        "stripped --log session leaked ANSI into the INFO token: {explicit:?}"
    );
    Ok(())
}

#[test]
fn quiet_default_session_writes_no_stderr() -> TestResult {
    let mut command = spawn_perllsp(&["--stdio"]);
    command.env("PERL_LSP_QUIET", "1");
    let quiet = successful_stderr(command.output())?;
    assert!(quiet.is_empty(), "quiet default session wrote stderr: {quiet}");
    Ok(())
}

#[test]
fn perl_lsp_log_env_enables_info_without_the_flag() -> TestResult {
    let mut command = spawn_perllsp(&["--stdio"]);
    command.env("PERL_LSP_LOG", "info");
    let stderr = successful_stderr(command.output())?;
    assert!(
        stderr.contains(" INFO "),
        "re-applied PERL_LSP_LOG must enable INFO so the default env_remove stays load-bearing: {stderr}"
    );
    Ok(())
}

#[test]
fn force_color_on_the_child_rewrites_piped_info_tokens() -> TestResult {
    let mut command = spawn_perllsp(&["--stdio", "--log"]);
    command.env_remove("NO_COLOR");
    command.env("FORCE_COLOR", "1");
    let stderr = successful_stderr(command.output())?;
    assert!(
        stderr.contains('\u{1b}'),
        "re-applied FORCE_COLOR must color piped --log stderr so the default env_remove stays load-bearing: {stderr:?}"
    );
    Ok(())
}
