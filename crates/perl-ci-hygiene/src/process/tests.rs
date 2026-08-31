use super::{
    command_exists_in_path, command_output_lines, command_output_with_status, command_status,
    command_status_strict, command_timed_status, command_with_input_with_status,
    command_with_output, command_with_output_all, command_with_output_allow_empty_match,
    command_with_output_allow_failure,
};
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const FIXTURE_SCENARIO_ENV: &str = "PERL_CI_HYGIENE_PROCESS_FIXTURE";
const FIXTURE_VALUE_ENV: &str = "PERL_CI_HYGIENE_PROCESS_VALUE";
const FIXTURE_FILTER: &str = "process::tests::process_fixture_child";
const FIXTURE_CWD_SENTINEL: &str = "process-fixture-cwd";
const STDOUT_MARKER: &str = "__PERL_CI_HYGIENE_STDOUT__";
const STDERR_MARKER: &str = "__PERL_CI_HYGIENE_STDERR__";
const STDIN_MARKER: &str = "__PERL_CI_HYGIENE_STDIN__";
const ENV_MARKER: &str = "__PERL_CI_HYGIENE_ENV__";
const CWD_MARKER: &str = "__PERL_CI_HYGIENE_CWD_SENTINEL__";

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> TestResult<Self> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path =
            env::temp_dir().join(format!("perl-ci-hygiene-{label}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn command_candidate_name(command: &str) -> String {
    #[cfg(windows)]
    {
        format!("{command}.exe")
    }
    #[cfg(not(windows))]
    {
        command.to_owned()
    }
}

fn joined_path(paths: &[&Path]) -> TestResult<OsString> {
    Ok(env::join_paths(paths.iter().copied())?)
}

fn fixture_command() -> TestResult<String> {
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

fn fixture_args() -> [&'static str; 3] {
    [FIXTURE_FILTER, "--exact", "--nocapture"]
}

fn fixture_env(scenario: &str) -> [(&str, &str); 1] {
    [(FIXTURE_SCENARIO_ENV, scenario)]
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
    }
    writeln!(stderr, "{STDERR_MARKER}")?;
    stdout.flush()?;
    stderr.flush()?;
    Ok(())
}

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
fn command_with_input_returns_nonzero_status_and_both_streams() -> TestResult {
    let temp = TempDir::new("input-status")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let (status, combined) = command_with_input_with_status(
        temp.path(),
        &command,
        &args,
        &fixture_env("stdin-exit-7"),
        "payload with spaces",
    )?;

    assert_eq!(status, 7);
    assert!(combined.contains(&format!("{STDIN_MARKER}:payload with spaces")));
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
    assert!(error.to_string().contains("failed with code 7"));
    Ok(())
}

#[test]
fn timed_status_returns_nonzero_child_status() -> TestResult {
    let temp = TempDir::new("timed-status")?;
    let command = fixture_command()?;
    let args = fixture_args();
    let (status, _elapsed) =
        command_timed_status(temp.path(), &command, &args, &fixture_env("quiet-exit-7"))?;

    assert_eq!(status, 7);
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
fn command_exists_in_path_rejects_directory_candidate() -> TestResult {
    let temp = TempDir::new("directory-only")?;
    let command = "ci-hygiene-probe";
    fs::create_dir(temp.path().join(command_candidate_name(command)))?;
    let path = joined_path(&[temp.path()])?;

    assert!(!command_exists_in_path(command, Some(path.as_os_str())));
    Ok(())
}

#[test]
fn command_exists_in_path_accepts_regular_file_candidate() -> TestResult {
    let temp = TempDir::new("regular-file")?;
    let command = "ci-hygiene-probe";
    fs::write(temp.path().join(command_candidate_name(command)), b"")?;
    let path = joined_path(&[temp.path()])?;

    assert!(command_exists_in_path(command, Some(path.as_os_str())));
    Ok(())
}

#[test]
fn command_exists_in_path_skips_missing_path_entry() -> TestResult {
    let missing_entry_root = TempDir::new("missing-before-file")?;
    let regular_file_candidate = TempDir::new("file-after-missing")?;
    let command = "ci-hygiene-probe";
    let missing_entry = missing_entry_root.path().join("not-created");
    fs::write(regular_file_candidate.path().join(command_candidate_name(command)), b"")?;
    let path = joined_path(&[missing_entry.as_path(), regular_file_candidate.path()])?;

    assert!(command_exists_in_path(command, Some(path.as_os_str())));
    Ok(())
}

#[test]
fn command_exists_in_path_continues_past_directory_candidate() -> TestResult {
    let directory_candidate = TempDir::new("directory-before-file")?;
    let regular_file_candidate = TempDir::new("file-after-directory")?;
    let command = "ci-hygiene-probe";
    let candidate_name = command_candidate_name(command);
    fs::create_dir(directory_candidate.path().join(&candidate_name))?;
    let regular_file = regular_file_candidate.path().join(candidate_name);
    fs::write(&regular_file, b"")?;
    let path = joined_path(&[directory_candidate.path(), regular_file_candidate.path()])?;

    assert!(command_exists_in_path(command, Some(path.as_os_str())));
    fs::remove_file(regular_file)?;
    assert!(!command_exists_in_path(command, Some(path.as_os_str())));
    Ok(())
}

#[test]
fn command_exists_in_path_returns_false_without_path() {
    assert!(!command_exists_in_path("ci-hygiene-probe", None));
}

#[cfg(windows)]
#[test]
fn command_exists_in_path_accepts_windows_command_extensions() -> TestResult {
    let command = "ci-hygiene-probe";
    for suffix in [".cmd", ".bat", ""] {
        let label = match suffix {
            ".cmd" => "cmd",
            ".bat" => "bat",
            "" => "extensionless",
            _ => "unexpected",
        };
        let temp = TempDir::new(&format!("windows-extension-{label}"))?;
        fs::write(temp.path().join(format!("{command}{suffix}")), b"")?;
        let path = joined_path(&[temp.path()])?;

        assert!(
            command_exists_in_path(command, Some(path.as_os_str())),
            "command lookup did not find {command}{suffix}"
        );
    }
    Ok(())
}
