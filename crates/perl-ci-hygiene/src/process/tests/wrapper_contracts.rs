use super::super::{
    child_exit_code, command_output_lines, command_output_with_status, command_status,
    command_status_strict, command_timed_status, command_with_input_with_status,
    command_with_output, command_with_output_all, command_with_output_allow_empty_match,
    command_with_output_allow_failure,
};
use super::{TempDir, TestResult};
use std::env;
use std::fs;
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::fd::FromRawFd;
use std::process::ExitStatus;
use std::time::Duration;

pub(super) const FIXTURE_SCENARIO_ENV: &str = "PERL_CI_HYGIENE_PROCESS_FIXTURE";
const FIXTURE_VALUE_ENV: &str = "PERL_CI_HYGIENE_PROCESS_VALUE";
const FIXTURE_FILTER: &str = "process::tests::wrapper_contracts::process_fixture_child";
const FIXTURE_CWD_SENTINEL: &str = "process-fixture-cwd";
const STDOUT_MARKER: &str = "__PERL_CI_HYGIENE_STDOUT__";
const STDERR_MARKER: &str = "__PERL_CI_HYGIENE_STDERR__";
const STDIN_MARKER: &str = "__PERL_CI_HYGIENE_STDIN__";
pub(super) const STDIN_BYTES_MARKER: &str = "__PERL_CI_HYGIENE_STDIN_BYTES__";
pub(super) const STDIN_EOF_MARKER: &str = "__PERL_CI_HYGIENE_STDIN_EOF__";
pub(super) const STDIN_DIGEST_MARKER: &str = "__PERL_CI_HYGIENE_STDIN_DIGEST__";
pub(super) const STDOUT_BEFORE_STDIN_MARKER: &str = "__PERL_CI_HYGIENE_STDOUT_BEFORE_STDIN__";
pub(super) const STDERR_BEFORE_STDIN_MARKER: &str = "__PERL_CI_HYGIENE_STDERR_BEFORE_STDIN__";
pub(super) const EARLY_EXIT_MARKER: &str = "__PERL_CI_HYGIENE_EARLY_EXIT__";
pub(super) const CLOSED_STDIN_MARKER: &str = "__PERL_CI_HYGIENE_CLOSED_STDIN__";
const ENV_MARKER: &str = "__PERL_CI_HYGIENE_ENV__";
const CWD_MARKER: &str = "__PERL_CI_HYGIENE_CWD_SENTINEL__";
const ARGS_MARKER: &str = "__PERL_CI_HYGIENE_ARGS__";
const ARGS_SEP: &str = "\u{1e}";
const LARGE_STREAM_BYTES: usize = 128 * 1024;
/// Larger than an ordinary pipe buffer so write-before-read fills both sides.
pub(super) const PIPE_PRESSURE_BYTES: usize = 256 * 1024;
const STDIN_PAYLOAD: &str = "line 1\narg with spaces\n$HOME\n\"quoted\"\n";

pub(super) fn stdin_digest(bytes: &[u8]) -> u64 {
    let mut hash = 2_166_136_261u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

pub(super) fn fixture_command() -> TestResult<String> {
    env::current_exe()?.into_os_string().into_string().map_err(|path| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "process fixture executable path is not valid UTF-8: {}",
                path.to_string_lossy()
            ),
        )
        .into()
    })
}

pub(super) fn fixture_args() -> [&'static str; 3] {
    [FIXTURE_FILTER, "--exact", "--nocapture"]
}

fn fixture_env(scenario: &str) -> [(&str, &str); 1] {
    [(FIXTURE_SCENARIO_ENV, scenario)]
}

fn extra_args_after_nocapture() -> Vec<String> {
    env::args().skip_while(|argument| argument != "--nocapture").skip(1).collect()
}

fn emit_fixture_output(stdin_payload: Option<&str>) -> TestResult {
    let value = env::var(FIXTURE_VALUE_ENV).unwrap_or_default();
    let cwd_sentinel_exists = env::current_dir()?.join(FIXTURE_CWD_SENTINEL).is_file();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    writeln!(stdout, "{STDOUT_MARKER}")?;
    writeln!(stdout, "{ENV_MARKER}:{value}")?;
    writeln!(stdout, "{CWD_MARKER}:{cwd_sentinel_exists}")?;
    if let Some(payload) = stdin_payload {
        writeln!(stdout, "{STDIN_MARKER}:{payload}")?;
        writeln!(stdout, "{STDIN_BYTES_MARKER}:{}", payload.len())?;
        writeln!(stdout, "{STDIN_EOF_MARKER}:1")?;
    }
    writeln!(stderr, "{STDERR_MARKER}")?;
    stdout.flush()?;
    stderr.flush()?;
    Ok(())
}

fn close_stdin_keep_stdout() -> TestResult {
    #[cfg(unix)]
    {
        // SAFETY: this fixture inherited fd 0 as stdin. Owning and dropping it
        // closes the read end so the parent observes EPIPE while this process
        // keeps stdout open and stays alive.
        let stdin = unsafe { std::fs::File::from_raw_fd(0) };
        drop(stdin);
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{CLOSED_STDIN_MARKER}")?;
        stdout.flush()?;
        loop {
            std::thread::sleep(Duration::from_secs(30));
        }
    }
    #[cfg(not(unix))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "close-stdin-keep-stdout is a Unix pipe-closure fixture",
    )
    .into())
}

/// Exact libtest-owned child. Inert unless `PERL_CI_HYGIENE_PROCESS_FIXTURE` is set,
/// so an ordinary suite run cannot recurse into the parent population.
#[test]
fn process_fixture_child() -> TestResult {
    let Ok(scenario) = env::var(FIXTURE_SCENARIO_ENV) else {
        return Ok(());
    };

    match scenario.as_str() {
        "success" => emit_fixture_output(None),
        "stdin-exit-7" => {
            let mut payload = String::new();
            io::stdin().read_to_string(&mut payload)?;
            emit_fixture_output(Some(&payload))?;
            std::process::exit(7);
        }
        "exit-1" => {
            emit_fixture_output(None)?;
            std::process::exit(1);
        }
        "exit-2" => {
            emit_fixture_output(None)?;
            std::process::exit(2);
        }
        "exit-7" => {
            emit_fixture_output(None)?;
            std::process::exit(7);
        }
        "quiet-success" => Ok(()),
        "quiet-exit-7" => std::process::exit(7),
        "echo-args" => {
            let extras = extra_args_after_nocapture();
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "{ARGS_MARKER}:{}", extras.join(ARGS_SEP))?;
            stdout.flush()?;
            Ok(())
        }
        "non-utf8" => {
            let mut stdout = io::stdout().lock();
            stdout.write_all(STDOUT_MARKER.as_bytes())?;
            stdout.write_all(b"\n")?;
            stdout.write_all(b"\xff\xfe")?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
            let mut stderr = io::stderr().lock();
            stderr.write_all(STDERR_MARKER.as_bytes())?;
            stderr.write_all(b"\n")?;
            stderr.write_all(b"\x80")?;
            stderr.flush()?;
            Ok(())
        }
        "large-streams" => {
            let stdout_bytes = vec![b'X'; LARGE_STREAM_BYTES];
            let stderr_bytes = vec![b'Y'; LARGE_STREAM_BYTES];
            let mut stdout = io::stdout().lock();
            stdout.write_all(&stdout_bytes)?;
            stdout.flush()?;
            let mut stderr = io::stderr().lock();
            stderr.write_all(&stderr_bytes)?;
            stderr.flush()?;
            Ok(())
        }
        "write-before-read" => {
            {
                let mut stdout = io::stdout().lock();
                writeln!(stdout, "{STDOUT_BEFORE_STDIN_MARKER}")?;
                stdout.write_all(&vec![b'~'; PIPE_PRESSURE_BYTES])?;
                writeln!(stdout)?;
                stdout.flush()?;
            }
            {
                let mut stderr = io::stderr().lock();
                writeln!(stderr, "{STDERR_BEFORE_STDIN_MARKER}")?;
                stderr.write_all(&vec![b'^'; PIPE_PRESSURE_BYTES])?;
                writeln!(stderr)?;
                stderr.flush()?;
            }
            let mut payload = Vec::new();
            io::stdin().read_to_end(&mut payload)?;
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "{STDIN_BYTES_MARKER}:{}", payload.len())?;
            writeln!(stdout, "{STDIN_DIGEST_MARKER}:{:016x}", stdin_digest(&payload))?;
            writeln!(stdout, "{STDIN_EOF_MARKER}:1")?;
            stdout.flush()?;
            Ok(())
        }
        "write-small-before-read" => {
            {
                let mut stdout = io::stdout().lock();
                writeln!(stdout, "{STDOUT_BEFORE_STDIN_MARKER}")?;
                stdout.flush()?;
            }
            {
                let mut stderr = io::stderr().lock();
                writeln!(stderr, "{STDERR_BEFORE_STDIN_MARKER}")?;
                stderr.flush()?;
            }
            let mut payload = Vec::new();
            io::stdin().read_to_end(&mut payload)?;
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "{STDIN_BYTES_MARKER}:{}", payload.len())?;
            writeln!(stdout, "{STDIN_DIGEST_MARKER}:{:016x}", stdin_digest(&payload))?;
            writeln!(stdout, "{STDIN_EOF_MARKER}:1")?;
            stdout.flush()?;
            Ok(())
        }
        "early-exit-without-stdin" => {
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "{EARLY_EXIT_MARKER}")?;
            stdout.flush()?;
            std::process::exit(3);
        }
        "close-stdin-keep-stdout" => close_stdin_keep_stdout(),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown process fixture scenario: {other}"),
        )
        .into()),
    }
}

#[test]
fn command_with_output_preserves_success_cwd_env_and_stdout_only() -> TestResult {
    let temp = TempDir::new("output-success")?;
    fs::write(temp.path().join(FIXTURE_CWD_SENTINEL), b"")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let output = command_with_output(
        temp.path(),
        &command,
        &args,
        &[(FIXTURE_SCENARIO_ENV, "success"), (FIXTURE_VALUE_ENV, "explicit-value")],
    )?;

    assert!(output.contains(STDOUT_MARKER));
    assert!(output.contains(&format!("{ENV_MARKER}:explicit-value")));
    assert!(output.contains(&format!("{CWD_MARKER}:true")));
    assert!(!output.contains(STDERR_MARKER));
    Ok(())
}

#[test]
fn command_with_output_reports_nonzero_status_and_stderr_only() -> TestResult {
    let temp = TempDir::new("output-failure")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let error = command_with_output(temp.path(), &command, &args, &fixture_env("exit-7"))
        .err()
        .ok_or_else(|| io::Error::other("expected command_with_output to reject exit 7"))?;
    let message = error.to_string();

    assert!(message.contains("exit 7"));
    assert!(message.contains(STDERR_MARKER));
    assert!(!message.contains(STDOUT_MARKER));
    Ok(())
}

#[test]
fn command_with_output_all_combines_streams_on_success_and_failure() -> TestResult {
    let temp = TempDir::new("output-all")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let combined = command_with_output_all(temp.path(), &command, &args, &fixture_env("success"))?;
    let stdout_position = combined
        .find(STDOUT_MARKER)
        .ok_or_else(|| io::Error::other("combined output omitted stdout"))?;
    let stderr_position = combined
        .find(STDERR_MARKER)
        .ok_or_else(|| io::Error::other("combined output omitted stderr"))?;
    // Post-process concatenation, not live interleaving: stdout buffer, then stderr.
    assert!(stdout_position < stderr_position);

    let error = command_with_output_all(temp.path(), &command, &args, &fixture_env("exit-7"))
        .err()
        .ok_or_else(|| io::Error::other("expected command_with_output_all to reject exit 7"))?;
    let message = error.to_string();
    assert!(message.contains("exit 7"));
    assert!(message.contains(STDOUT_MARKER));
    assert!(message.contains(STDERR_MARKER));
    Ok(())
}

#[test]
fn command_with_input_returns_nonzero_status_and_complete_stdin() -> TestResult {
    let temp = TempDir::new("input-status")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let (status, combined) = command_with_input_with_status(
        temp.path(),
        &command,
        &args,
        &fixture_env("stdin-exit-7"),
        STDIN_PAYLOAD,
    )?;

    assert_eq!(status, 7);
    assert!(combined.contains(&format!("{STDIN_MARKER}:{STDIN_PAYLOAD}")));
    assert!(combined.contains(&format!("{STDIN_BYTES_MARKER}:{}", STDIN_PAYLOAD.len())));
    assert!(combined.contains(&format!("{STDIN_EOF_MARKER}:1")));
    assert!(combined.contains(STDOUT_MARKER));
    assert!(combined.contains(STDERR_MARKER));
    Ok(())
}

#[test]
fn command_output_with_status_returns_stdout_but_not_stderr() -> TestResult {
    let temp = TempDir::new("output-status")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let (status, output) =
        command_output_with_status(temp.path(), &command, &args, &fixture_env("exit-7"))?;

    assert_eq!(status, 7);
    assert!(output.contains(STDOUT_MARKER));
    assert!(!output.contains(STDERR_MARKER));
    Ok(())
}

#[test]
fn allow_empty_match_accepts_zero_and_one_and_rejects_other_nonzero_status() -> TestResult {
    let temp = TempDir::new("allow-empty")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let output = command_with_output_allow_empty_match(
        temp.path(),
        &command,
        &args,
        &fixture_env("success"),
    )?;
    assert!(output.contains(STDOUT_MARKER));

    let output = command_with_output_allow_empty_match(
        temp.path(),
        &command,
        &args,
        &fixture_env("exit-1"),
    )?;
    assert!(output.contains(STDOUT_MARKER));

    let error =
        command_with_output_allow_empty_match(temp.path(), &command, &args, &fixture_env("exit-2"))
            .err()
            .ok_or_else(|| io::Error::other("expected exit 2 to be rejected"))?;
    let message = error.to_string();
    assert!(message.contains("exit 2"));
    assert!(message.contains(STDERR_MARKER));
    Ok(())
}

#[test]
fn allow_failure_preserves_stdout_from_nonzero_child() -> TestResult {
    let temp = TempDir::new("allow-failure")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let output =
        command_with_output_allow_failure(temp.path(), &command, &args, &fixture_env("exit-7"))?;

    assert!(output.contains(STDOUT_MARKER));
    assert!(!output.contains(STDERR_MARKER));
    Ok(())
}

#[test]
fn status_helpers_preserve_raw_and_strict_behavior() -> TestResult {
    let temp = TempDir::new("status")?;
    let command = fixture_command()?;
    let args = fixture_args();

    let status = command_status(temp.path(), &command, &args, &fixture_env("quiet-exit-7"))?;
    assert_eq!(status, 7);

    command_status_strict(temp.path(), &command, &args, &fixture_env("quiet-success"))?;
    let error = command_status_strict(temp.path(), &command, &args, &fixture_env("quiet-exit-7"))
        .err()
        .ok_or_else(|| io::Error::other("expected strict status to reject exit 7"))?;
    let message = error.to_string();
    assert!(message.contains("failed with code 7"));
    assert!(
        message.contains(&command),
        "strict failure must retain the command identity, got {message}"
    );
    Ok(())
}

#[test]
fn timed_status_returns_nonzero_child_status_and_a_duration() -> TestResult {
    let temp = TempDir::new("timed-status")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let (status, elapsed) =
        command_timed_status(temp.path(), &command, &args, &fixture_env("quiet-exit-7"))?;

    assert_eq!(status, 7);
    // Inspecting the duration pins that the helper returned a measured value
    // rather than omitting it. No wall-clock performance threshold.
    let _nanos: u128 = elapsed.as_nanos();
    let _: Duration = elapsed;
    Ok(())
}

#[test]
fn output_lines_trim_blanks_and_preserve_order() {
    let expected = vec!["first".to_owned(), "second".to_owned(), "third".to_owned()];

    let lf_lines = command_output_lines("  first  \n\n\tsecond\t\n   \nthird\n");
    assert_eq!(lf_lines, expected);

    let crlf_lines = command_output_lines("  first  \r\n\r\n\tsecond\t\r\n   \r\nthird\r\n");
    assert_eq!(crlf_lines, expected);
}

#[test]
fn argument_boundaries_preserve_spaces_and_metacharacters() -> TestResult {
    let temp = TempDir::new("args")?;
    let command = fixture_command()?;
    let extras = ["arg with spaces", "$HOME", "a|b&c", "*.rs", "\"quoted\""];
    let mut args = Vec::from(fixture_args());
    args.extend_from_slice(&extras);
    let output = command_with_output(temp.path(), &command, &args, &fixture_env("echo-args"))?;

    let needle = format!("{ARGS_MARKER}:");
    let reported = output
        .find(&needle)
        .and_then(|at| output.get(at + needle.len()..).and_then(|rest| rest.lines().next()));
    let reported = reported.ok_or_else(|| {
        io::Error::other(format!("fixture did not echo forwarded args: {output}"))
    })?;
    assert_eq!(
        reported.split(ARGS_SEP).collect::<Vec<_>>(),
        extras,
        "wrapper must forward argv bytes without shell interpretation"
    );
    Ok(())
}

#[test]
fn missing_executable_returns_typed_invocation_context() -> TestResult {
    let temp = TempDir::new("missing-exe")?;
    let missing = temp.path().join("definitely-missing-perl-ci-hygiene-probe");
    let command = missing.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing-exe fixture path is not UTF-8")
    })?;

    let output_error = command_with_output(temp.path(), command, &[], &[])
        .err()
        .ok_or_else(|| io::Error::other("expected missing executable to fail capture"))?;
    let output_message = format!("{output_error:#}");
    assert!(
        output_message.contains("running ") && output_message.contains(command),
        "capture failure must name the invocation, got {output_message}"
    );

    let status_error = command_status(temp.path(), command, &[], &[])
        .err()
        .ok_or_else(|| io::Error::other("expected missing executable to fail status"))?;
    let status_message = format!("{status_error:#}");
    assert!(
        status_message.contains("running ") && status_message.contains(command),
        "status failure must name the invocation, got {status_message}"
    );
    Ok(())
}

#[test]
fn non_utf8_bytes_use_lossy_text_without_panic() -> TestResult {
    let temp = TempDir::new("non-utf8")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let stdout_only = command_with_output(temp.path(), &command, &args, &fixture_env("non-utf8"))?;
    assert!(stdout_only.contains(STDOUT_MARKER));
    assert!(stdout_only.contains('\u{FFFD}'));
    assert!(!stdout_only.contains(STDERR_MARKER));

    let combined = command_with_output_all(temp.path(), &command, &args, &fixture_env("non-utf8"))?;
    assert!(combined.contains(STDOUT_MARKER));
    assert!(combined.contains(STDERR_MARKER));
    assert!(combined.contains('\u{FFFD}'));
    Ok(())
}

#[test]
fn large_bounded_streams_do_not_deadlock_captured_output() -> TestResult {
    let temp = TempDir::new("large-streams")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let combined =
        command_with_output_all(temp.path(), &command, &args, &fixture_env("large-streams"))?;

    let stdout_bytes = combined.bytes().filter(|byte| *byte == b'X').count();
    let stderr_bytes = combined.bytes().filter(|byte| *byte == b'Y').count();
    assert_eq!(stdout_bytes, LARGE_STREAM_BYTES);
    assert_eq!(stderr_bytes, LARGE_STREAM_BYTES);
    assert!(
        combined.find('X').unwrap_or(usize::MAX) < combined.find('Y').unwrap_or(0),
        "large stdout must still precede stderr in post-process concatenation"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn child_exit_code_maps_signaled_wait_status_to_one() {
    use std::os::unix::process::ExitStatusExt;

    // wait(2): a process terminated by signal N has wait status N in the low bits.
    const SIGTERM: i32 = 15;
    let signaled = ExitStatus::from_raw(SIGTERM);
    assert_eq!(signaled.code(), None, "signal termination must have no numeric exit code");
    assert_eq!(child_exit_code(signaled), 1);

    let exited = ExitStatus::from_raw(7 << 8);
    assert_eq!(exited.code(), Some(7));
    assert_eq!(child_exit_code(exited), 7);
}

#[cfg(windows)]
#[test]
fn child_exit_code_preserves_windows_raw_code() {
    use std::os::windows::process::ExitStatusExt;

    let status = ExitStatus::from_raw(7);
    assert_eq!(status.code(), Some(7));
    assert_eq!(child_exit_code(status), 7);
}
