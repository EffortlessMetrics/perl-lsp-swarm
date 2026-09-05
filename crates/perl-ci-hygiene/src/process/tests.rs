use super::{
    admissible_search_paths, apply_admissible_search_path, command_exists, command_exists_in_path,
    command_output_lines, command_output_with_status, command_status, command_status_strict,
    command_timed_status, command_with_input_with_status, command_with_output,
    command_with_output_all, command_with_output_allow_empty_match,
    command_with_output_allow_failure, windows_command_candidates,
};
use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
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

#[cfg(not(windows))]
#[test]
fn command_exists_in_path_continues_past_directory_candidate() -> TestResult {
    // Unix parity: execve on a directory candidate fails EACCES/EISDIR and
    // the execvp-style search continues to the next PATH entry.
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

#[cfg(windows)]
#[test]
fn command_exists_in_path_fails_closed_when_std_selects_earlier_directory() -> TestResult {
    // Windows parity: std's `program_exists` (GetFileAttributesW) admits the
    // earlier directory candidate and stops there, so the real launch fails —
    // the probe must NOT report the later regular file the launch would never
    // reach.
    let directory_candidate = TempDir::new("directory-before-file")?;
    let regular_file_candidate = TempDir::new("file-after-directory")?;
    let command = "ci-hygiene-probe";
    fs::create_dir(directory_candidate.path().join(format!("{command}.exe")))?;
    fs::write(regular_file_candidate.path().join(format!("{command}.exe")), b"")?;
    let path = joined_path(&[directory_candidate.path(), regular_file_candidate.path()])?;

    assert!(
        !command_exists_in_path(command, Some(path.as_os_str())),
        "an earlier directory candidate must hide the later tool, matching std's selection"
    );
    Ok(())
}

#[test]
fn command_exists_in_path_returns_false_without_path() {
    assert!(!command_exists_in_path("ci-hygiene-probe", None));
}

// Pure candidate-generation tests for the Windows probe seam. These run on
// every platform: `windows_command_candidates` mirrors the bare-name search
// of the pinned Rust 1.95 `std::process::Command` implementation
// (`library/std/src/sys/process/windows.rs`, `resolve_exe`), and std's rules
// are platform-independent pure path construction.
#[test]
fn windows_candidates_bare_name_try_only_dot_exe_in_path_order() -> TestResult {
    let first = TempDir::new("candidates-first")?;
    let second = TempDir::new("candidates-second")?;
    let path = joined_path(&[first.path(), second.path()])?;

    let candidates = windows_command_candidates("probe-tool", path.as_os_str());

    assert_eq!(
        candidates,
        vec![first.path().join("probe-tool.exe"), second.path().join("probe-tool.exe"),],
        "bare names resolve to <name>.exe only, earliest PATH entry first"
    );
    Ok(())
}

#[test]
fn windows_candidates_explicit_extension_is_verbatim() -> TestResult {
    let temp = TempDir::new("candidates-explicit")?;
    let path = joined_path(&[temp.path()])?;

    for command in ["probe-tool.exe", "probe-tool.cmd", "probe-tool.bat", "probe-tool.ps1"] {
        let candidates = windows_command_candidates(command, path.as_os_str());
        assert_eq!(
            candidates,
            vec![temp.path().join(command)],
            "explicit extension {command} must not be expanded into duplicated suffixes"
        );
    }
    Ok(())
}

#[test]
fn windows_candidates_interior_dot_counts_as_extension() -> TestResult {
    let temp = TempDir::new("candidates-interior-dot")?;
    let path = joined_path(&[temp.path()])?;

    // std checks `contains('.')`, not `Path::extension`: any dot disables the
    // `.exe` append.
    let candidates = windows_command_candidates("probe.tool", path.as_os_str());
    assert_eq!(candidates, vec![temp.path().join("probe.tool")]);
    Ok(())
}

#[test]
fn windows_candidates_never_expand_to_script_or_unsuffixed_names() -> TestResult {
    let temp = TempDir::new("candidates-no-pathext")?;
    let path = joined_path(&[temp.path()])?;

    // PATHEXT is not consulted by `std::process::Command`; the candidate set
    // must contain no .cmd/.bat/unsuffixed form even though a shell would
    // produce them.
    let candidates = windows_command_candidates("probe-tool", path.as_os_str());
    for candidate in &candidates {
        let name = candidate
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::other("candidate file name is not UTF-8"))?;
        assert_eq!(name, "probe-tool.exe");
    }
    Ok(())
}

#[test]
fn windows_candidates_skip_empty_path_entries() -> TestResult {
    let temp = TempDir::new("candidates-empty-entry")?;
    let path = joined_path(&[Path::new(""), temp.path(), Path::new("")])?;

    let candidates = windows_command_candidates("probe-tool", path.as_os_str());

    // An empty entry joined with a name would resolve against the process CWD;
    // std skips empty entries, so the probe must too.
    assert_eq!(candidates, vec![temp.path().join("probe-tool.exe")]);
    Ok(())
}

#[test]
fn windows_candidates_skip_relative_path_entries() -> TestResult {
    let temp = TempDir::new("candidates-relative-entry")?;
    let path = joined_path(&[Path::new("reldir"), temp.path(), Path::new(".")])?;

    let candidates = windows_command_candidates("probe-tool", path.as_os_str());

    // A relative entry resolves against this process's directory here and
    // against the child's `current_dir` at launch, so the Windows probe drops
    // it for the same reason the Unix branch does.
    assert_eq!(candidates, vec![temp.path().join("probe-tool.exe")]);
    Ok(())
}

#[test]
fn windows_candidates_preserve_spaces_and_non_ascii_path_entries() -> TestResult {
    let temp = TempDir::new("candidates-unicode")?;
    let spaced = temp.path().join("dir with spaces");
    let non_ascii = temp.path().join("dïr-ütﬂ-名");
    let path = joined_path(&[spaced.as_path(), non_ascii.as_path()])?;

    let candidates = windows_command_candidates("probe-tool", path.as_os_str());

    assert_eq!(candidates, vec![spaced.join("probe-tool.exe"), non_ascii.join("probe-tool.exe")]);
    Ok(())
}

#[test]
fn windows_candidates_fail_closed_for_separator_bearing_names() -> TestResult {
    let temp = TempDir::new("candidates-separator")?;
    let path = joined_path(&[temp.path()])?;

    // `std::process::Command` does not PATH-search a name carrying a path
    // separator; the probe supports bare names only and fails closed.
    for command in ["", "sub/probe-tool", "sub\\probe-tool", "./probe-tool", "C:\\probe-tool"] {
        assert!(
            windows_command_candidates(command, path.as_os_str()).is_empty(),
            "separator-bearing or empty input {command:?} must produce no PATH candidates"
        );
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn command_exists_in_path_rejects_script_and_unsuffixed_candidates() -> TestResult {
    // `Command::new` never resolves a bare name to `.cmd`, `.bat`, or an
    // unsuffixed file, so the probe must not either (false-positive parity).
    let command = "ci-hygiene-probe";
    for (suffix, label) in [(".cmd", "cmd"), (".bat", "bat"), ("", "extensionless")] {
        let temp = TempDir::new(&format!("windows-reject-{label}"))?;
        fs::write(temp.path().join(format!("{command}{suffix}")), b"")?;
        let path = joined_path(&[temp.path()])?;

        assert!(
            !command_exists_in_path(command, Some(path.as_os_str())),
            "bare-name probe must not find {command}{suffix}"
        );
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn command_exists_in_path_accepts_explicit_script_name() -> TestResult {
    // An explicitly extensioned script name is searched verbatim.
    let temp = TempDir::new("windows-explicit-script")?;
    fs::write(temp.path().join("ci-hygiene-probe.cmd"), b"")?;
    let path = joined_path(&[temp.path()])?;

    assert!(command_exists_in_path("ci-hygiene-probe.cmd", Some(path.as_os_str())));
    Ok(())
}

// Native Windows launch-parity proof: the probe selection is compared against
// the exact process subject `std::process::Command` actually starts. These
// tests run in the `windows-platform-smoke` CI lane
// (`cargo test -p perl-ci-hygiene --bin perl-ci-hygiene process::tests::`).
#[cfg(windows)]
mod windows_launch_parity {
    use super::*;
    use std::ffi::OsStr;
    use std::process::Command;

    /// The candidate std's `resolve_exe` would select, mirrored the same way
    /// `command_exists_in_path` selects it: the first candidate whose
    /// attributes resolve (GetFileAttributesW semantics — directories and
    /// broken links count, matching `symlink_metadata`).
    fn probe_selection(command: &str, path: &OsStr) -> Option<PathBuf> {
        windows_command_candidates(command, path)
            .into_iter()
            .find(|c| fs::symlink_metadata(c).is_ok())
    }

    fn write_batch(dir: &Path, name: &str) -> TestResult<PathBuf> {
        let batch = dir.join(name);
        // Print the batch file's own fully-qualified path, then exit cleanly.
        fs::write(&batch, b"@echo off\r\n@echo %~f0\r\nexit /b 0\r\n")?;
        Ok(batch)
    }

    fn canonical_lower(path: &Path) -> TestResult<String> {
        Ok(fs::canonicalize(path)?.to_string_lossy().to_ascii_lowercase())
    }

    #[test]
    fn bare_name_probe_and_launch_select_the_same_exe() -> TestResult {
        let temp = TempDir::new("launch-parity-exe")?;
        let subject = temp.path().join("probe-parity-subject.exe");
        fs::copy(env::current_exe()?, &subject)?;
        let path = joined_path(&[temp.path()])?;

        let probe = probe_selection("probe-parity-subject", path.as_os_str())
            .ok_or_else(|| io::Error::other("probe did not select the .exe subject"))?;
        assert_eq!(probe, subject);

        // `Command::env("PATH", ..)` makes std search the child PATH first —
        // the same leg the probe covers.
        let output = Command::new("probe-parity-subject")
            .env("PATH", path.as_os_str())
            .arg("--list")
            .output()?;
        assert!(output.status.success(), "launch of probe-selected subject failed: {output:?}");
        Ok(())
    }

    #[test]
    fn bare_name_with_only_a_bat_present_launches_nothing() -> TestResult {
        let temp = TempDir::new("launch-parity-bat-only")?;
        write_batch(temp.path(), "probe-parity-script.bat")?;
        let path = joined_path(&[temp.path()])?;

        assert_eq!(probe_selection("probe-parity-script", path.as_os_str()), None);
        let launch = Command::new("probe-parity-script").env("PATH", path.as_os_str()).output();
        assert!(
            launch.is_err(),
            "std::process::Command must not resolve a bare name to a .bat file"
        );
        Ok(())
    }

    #[test]
    fn explicit_bat_name_launches_and_probe_selects_the_same_subject() -> TestResult {
        let temp = TempDir::new("launch-parity-explicit-bat")?;
        write_batch(temp.path(), "probe-parity-explicit.bat")?;
        let path = joined_path(&[temp.path()])?;

        let probe = probe_selection("probe-parity-explicit.bat", path.as_os_str())
            .ok_or_else(|| io::Error::other("probe did not select the explicit .bat subject"))?;

        let output =
            Command::new("probe-parity-explicit.bat").env("PATH", path.as_os_str()).output()?;
        assert!(output.status.success(), "explicit .bat launch failed: {output:?}");
        // The batch prints its own fully-qualified path (%~f0); compare it
        // with the probe-selected candidate.
        let launched = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        assert_eq!(
            canonical_lower(Path::new(&launched))?,
            canonical_lower(&probe)?,
            "launched subject differs from probe selection"
        );
        Ok(())
    }

    #[test]
    fn unsuffixed_regular_file_is_neither_probed_nor_launched() -> TestResult {
        let temp = TempDir::new("launch-parity-unsuffixed")?;
        fs::write(temp.path().join("probe-parity-plain"), b"")?;
        let path = joined_path(&[temp.path()])?;

        assert_eq!(probe_selection("probe-parity-plain", path.as_os_str()), None);
        let launch = Command::new("probe-parity-plain").env("PATH", path.as_os_str()).output();
        assert!(launch.is_err(), "std must not launch an unsuffixed file for a bare name");
        Ok(())
    }

    #[test]
    fn directory_with_admitted_extension_fails_closed() -> TestResult {
        let temp = TempDir::new("launch-parity-directory")?;
        fs::create_dir(temp.path().join("probe-parity-dir.exe"))?;
        let path = joined_path(&[temp.path()])?;

        // std's resolver selects the directory (GetFileAttributesW admits it);
        // the probe reports not-launchable, and the real launch fails.
        assert!(!command_exists_in_path("probe-parity-dir", Some(path.as_os_str())));
        let launch = Command::new("probe-parity-dir").env("PATH", path.as_os_str()).output();
        assert!(launch.is_err(), "a directory named <name>.exe must not be launched");
        Ok(())
    }

    #[test]
    fn earlier_directory_candidate_hides_later_tool_for_probe_and_launch() -> TestResult {
        let earlier = TempDir::new("launch-parity-hide-earlier")?;
        let later = TempDir::new("launch-parity-hide-later")?;
        fs::create_dir(earlier.path().join("probe-parity-hide.exe"))?;
        fs::copy(env::current_exe()?, later.path().join("probe-parity-hide.exe"))?;
        let path = joined_path(&[earlier.path(), later.path()])?;

        // std selects the earlier directory and CreateProcessW fails; the
        // probe must not claim the later, real tool is launchable.
        assert!(!command_exists_in_path("probe-parity-hide", Some(path.as_os_str())));
        let launch = Command::new("probe-parity-hide").env("PATH", path.as_os_str()).output();
        assert!(
            launch.is_err(),
            "an earlier directory candidate must hide a later tool at launch, and the probe agrees"
        );
        Ok(())
    }

    #[test]
    fn extension_casing_is_case_insensitive_for_probe_and_launch() -> TestResult {
        let temp = TempDir::new("launch-parity-case")?;
        // On-disk subject carries an uppercase extension; the bare-name probe
        // appends lowercase `.exe`, and the Windows filesystem resolves it.
        fs::copy(env::current_exe()?, temp.path().join("probe-parity-case.EXE"))?;
        let path = joined_path(&[temp.path()])?;

        assert!(command_exists_in_path("probe-parity-case", Some(path.as_os_str())));
        let output = Command::new("probe-parity-case")
            .env("PATH", path.as_os_str())
            .arg("--list")
            .output()?;
        assert!(output.status.success(), "case-insensitive launch failed: {output:?}");

        // An explicitly mixed-case extension is searched verbatim and found.
        assert!(command_exists_in_path("probe-parity-case.eXe", Some(path.as_os_str())));
        Ok(())
    }

    #[test]
    fn broken_link_candidate_fails_closed_for_probe_and_launch() -> TestResult {
        // Windows os error 1314: "A required privilege is not held by the
        // client." Symlink creation needs SeCreateSymbolicLinkPrivilege; a
        // runner without it cannot produce the subject, so only that error is
        // a typed skip — every other creation failure must fail the test
        // (same policy as perl-tdd-support::symlink_privilege).
        const SYMLINK_PRIVILEGE_NOT_HELD: i32 = 1314;

        let temp = TempDir::new("launch-parity-broken-link")?;
        let link = temp.path().join("probe-parity-link.exe");
        if let Err(error) =
            std::os::windows::fs::symlink_file(temp.path().join("missing-target.exe"), &link)
        {
            if error.raw_os_error() == Some(SYMLINK_PRIVILEGE_NOT_HELD) {
                eprintln!("skipping broken-link parity: symlink privilege not held");
                return Ok(());
            }
            return Err(error.into());
        }
        let path = joined_path(&[temp.path()])?;

        // std's resolver selects the broken link (GetFileAttributesW does not
        // follow it); CreateProcessW then fails, and the probe fails closed.
        assert!(!command_exists_in_path("probe-parity-link", Some(path.as_os_str())));
        let launch = Command::new("probe-parity-link").env("PATH", path.as_os_str()).output();
        assert!(launch.is_err(), "a broken link named <name>.exe must not be launched");
        Ok(())
    }

    #[test]
    fn pathext_mutation_changes_neither_probe_nor_launch() -> TestResult {
        let temp = TempDir::new("launch-parity-pathext")?;
        write_batch(temp.path(), "probe-parity-pathext.bat")?;
        let path = joined_path(&[temp.path()])?;

        // Even a PATHEXT that would admit the script must have no effect:
        // `std::process::Command` never reads PATHEXT.
        assert_eq!(probe_selection("probe-parity-pathext", path.as_os_str()), None);
        let launch = Command::new("probe-parity-pathext")
            .env("PATH", path.as_os_str())
            .env("PATHEXT", ".BAT;.CMD")
            .output();
        assert!(launch.is_err(), "PATHEXT mutation must not make a bare name launchable");
        Ok(())
    }

    #[test]
    fn earlier_path_candidate_wins_for_probe_and_launch() -> TestResult {
        let earlier = TempDir::new("launch-parity-earlier")?;
        let later = TempDir::new("launch-parity-later")?;
        write_batch(earlier.path(), "probe-parity-order.bat")?;
        write_batch(later.path(), "probe-parity-order.bat")?;
        let path = joined_path(&[earlier.path(), later.path()])?;

        let probe = probe_selection("probe-parity-order.bat", path.as_os_str())
            .ok_or_else(|| io::Error::other("probe selected no ordered candidate"))?;
        assert_eq!(
            canonical_lower(&probe)?,
            canonical_lower(&earlier.path().join("probe-parity-order.bat"))?,
        );

        let output =
            Command::new("probe-parity-order.bat").env("PATH", path.as_os_str()).output()?;
        assert!(output.status.success(), "ordered launch failed: {output:?}");
        let launched = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        assert_eq!(
            canonical_lower(Path::new(&launched))?,
            canonical_lower(&probe)?,
            "a later PATH candidate won despite the launch API selecting the earlier one"
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// #14150 — relative/empty PATH discovery must be coherent with the child cwd.
//
// `command_exists` runs in the parent while every wrapper here launches with
// `Command::current_dir(repo_root)`. A relative PATH component names the
// parent's directory when probing and `repo_root` when launching; an empty
// component means "current directory" to Unix `execvp`, so it does the same
// thing implicitly. These rows pin the admission policy and then prove, with
// real launches, that discovery and launch select one identical subject.
// ---------------------------------------------------------------------------

/// Pure policy rows. Platform-independent: the rule is `Path::is_absolute`.
#[test]
fn admissible_search_paths_without_a_path_variable_is_empty() {
    assert!(admissible_search_paths(None).is_empty());
}

#[test]
fn admissible_search_paths_drops_empty_components_in_every_position() -> TestResult {
    let absolute = TempDir::new("admissible-empty")?;
    let absolute_display = absolute.path().to_string_lossy().into_owned();
    let separator = if cfg!(windows) { ';' } else { ':' };

    for raw in [
        format!("{separator}{absolute_display}"),
        format!("{absolute_display}{separator}{separator}{absolute_display}"),
        format!("{absolute_display}{separator}"),
    ] {
        let admitted = admissible_search_paths(Some(OsStr::new(&raw)));
        assert!(
            admitted.iter().all(|dir| dir.is_absolute() && !dir.as_os_str().is_empty()),
            "empty component survived admission for PATH={raw:?}: {admitted:?}"
        );
        assert!(!admitted.is_empty(), "the absolute component was lost for PATH={raw:?}");
    }

    // An entirely empty PATH value is a single empty component, i.e. "the
    // current directory" at launch — nothing is admissible.
    assert!(admissible_search_paths(Some(OsStr::new(""))).is_empty());
    Ok(())
}

#[test]
fn admissible_search_paths_drops_relative_components_and_keeps_absolute_order() -> TestResult {
    let first = TempDir::new("admissible-first")?;
    let second = TempDir::new("admissible-second")?;
    let separator = if cfg!(windows) { ';' } else { ':' };
    let raw = format!(
        "{}{separator}reldir{separator}.{separator}..{separator}./nested{separator}{}",
        first.path().display(),
        second.path().display()
    );

    assert_eq!(
        admissible_search_paths(Some(OsStr::new(&raw))),
        vec![first.path().to_path_buf(), second.path().to_path_buf()],
        "only absolute components are admissible, in PATH order"
    );
    Ok(())
}

#[test]
fn admissible_search_paths_preserve_spaces_and_non_ascii_absolute_components() -> TestResult {
    let spaced = TempDir::new("admissible dir with spaces")?;
    let unicode = TempDir::new("admissible-ünïcødé")?;
    let path = joined_path(&[spaced.path(), unicode.path()])?;

    assert_eq!(
        admissible_search_paths(Some(path.as_os_str())),
        vec![spaced.path().to_path_buf(), unicode.path().to_path_buf()]
    );
    Ok(())
}

#[test]
fn command_exists_in_path_rejects_explicit_paths_on_every_platform() -> TestResult {
    let temp = TempDir::new("explicit-path")?;
    let command = "ci-hygiene-probe";
    let candidate = temp.path().join(command_candidate_name(command));
    fs::write(&candidate, b"")?;
    let path = joined_path(&[temp.path()])?;

    assert!(command_exists_in_path(command, Some(path.as_os_str())));
    // Separator-bearing names are not PATH-searched by the launch APIs, so the
    // probe has nothing to answer for and fails closed rather than joining them
    // onto every component.
    assert!(!command_exists_in_path("", Some(path.as_os_str())));
    assert!(!command_exists_in_path(&format!("./{command}"), Some(path.as_os_str())));
    assert!(!command_exists_in_path(&candidate.to_string_lossy(), Some(path.as_os_str())));
    Ok(())
}

/// Real-launch coherence oracle.
///
/// Unix-only instrument: the probes are `#!/bin/sh` scripts that print their own
/// identity, which is the cheapest way to prove *which* candidate ran. The
/// admission policy itself is platform-independent and covered by the pure rows
/// above; the equivalent Windows launch rows are not proven here.
#[cfg(unix)]
mod child_cwd_launch_coherence {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    const PROBE: &str = "ci-hygiene-cwd-coherence-probe";

    fn plant(dir: &Path, marker: &str) -> TestResult {
        let candidate = dir.join(PROBE);
        fs::write(&candidate, format!("#!/bin/sh\necho {marker}\n"))?;
        fs::set_permissions(&candidate, fs::Permissions::from_mode(0o755))?;
        Ok(())
    }

    /// Launch `PROBE` under `child_cwd` with `path` as the *inherited* PATH,
    /// applying the production admission policy, and report what actually ran.
    fn launch_with_policy(child_cwd: &Path, path: &OsStr) -> TestResult<String> {
        let mut child = Command::new(PROBE);
        child.current_dir(child_cwd);
        apply_admissible_search_path(&mut child, Some(path));
        Ok(match child.output() {
            Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            Err(error) => format!("<{}>", error.kind()),
        })
    }

    /// The same launch without the policy — the pre-#14150 behavior. Present so
    /// each row proves the policy is load-bearing rather than vacuous.
    fn launch_without_policy(child_cwd: &Path, path: &OsStr) -> TestResult<String> {
        let output = Command::new(PROBE).current_dir(child_cwd).env("PATH", path).output()?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    /// Pull a child's marker value out of its stdout.
    ///
    /// Position-independent on purpose: with `--nocapture` under a single test
    /// thread, libtest writes `test <name> ... ` *without* a trailing newline
    /// before the test body runs, so the child's marker is appended to that
    /// line rather than starting one. Matching only at line start passes
    /// multi-threaded and fails serially.
    fn marker_value<'a>(stdout: &'a str, marker: &str) -> Option<&'a str> {
        let needle = format!("{marker}:");
        stdout
            .lines()
            .find_map(|line| line.find(&needle).map(|at| line[at + needle.len()..].trim_end()))
    }

    fn mixed_path(prefix: &str, installed: &Path) -> OsString {
        OsString::from(format!("{prefix}:{}", installed.display()))
    }

    #[test]
    fn an_ambiguous_component_cannot_hide_the_admitted_installed_candidate() -> TestResult {
        let workspace = TempDir::new("coherence-workspace")?;
        let installed = TempDir::new("coherence-installed")?;
        plant(workspace.path(), "WORKSPACE")?;
        plant(installed.path(), "INSTALLED")?;

        // Each prefix reaches the child's own directory at launch time: "."
        // and a bare relative name explicitly, an empty component implicitly.
        for prefix in [".", "", "reldir"] {
            if prefix == "reldir" {
                let nested = workspace.path().join("reldir");
                fs::create_dir_all(&nested)?;
                plant(&nested, "WORKSPACE")?;
            }
            let path = mixed_path(prefix, installed.path());

            assert!(
                command_exists_in_path(PROBE, Some(path.as_os_str())),
                "the absolute component still admits the installed tool ({prefix:?})"
            );
            assert_eq!(
                launch_with_policy(workspace.path(), path.as_os_str())?,
                "INSTALLED",
                "discovery admitted the installed candidate, so the launch must run it \
                 and not the workspace copy reached through PATH component {prefix:?}"
            );
            // Negative control: without the policy the child's own directory
            // wins, which is precisely the incoherence #14150 reports.
            assert_eq!(
                launch_without_policy(workspace.path(), path.as_os_str())?,
                "WORKSPACE",
                "fixture no longer reproduces the defect for PATH component {prefix:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_wholly_ambiguous_path_admits_nothing_and_launches_nothing_from_the_workspace() -> TestResult
    {
        let workspace = TempDir::new("coherence-workspace-only")?;
        plant(workspace.path(), "WORKSPACE")?;

        for raw in [".", "", ":", ".:"] {
            let path = OsStr::new(raw);

            assert!(
                !command_exists_in_path(PROBE, Some(path)),
                "PATH={raw:?} has no admissible component and must report the tool absent"
            );
            // PATH is removed rather than emptied, so the launch falls back to
            // the platform's absolute default search path — never the
            // workspace. An empty PATH *value* would have meant the workspace.
            assert_eq!(
                launch_with_policy(workspace.path(), path)?,
                "<entity not found>",
                "an unadmitted PATH must not reach the workspace candidate (PATH={raw:?})"
            );
            assert_eq!(
                launch_without_policy(workspace.path(), path)?,
                "WORKSPACE",
                "fixture no longer reproduces the defect for PATH={raw:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_caller_supplied_path_replaces_the_inherited_search_path() -> TestResult {
        let workspace = TempDir::new("coherence-explicit")?;
        let installed = TempDir::new("coherence-explicit-installed")?;
        plant(installed.path(), "INSTALLED")?;

        // The preflight reads the *inherited* PATH, which does not carry PROBE.
        // The launch reads the *effective* one, which does. This divergence is
        // the documented residual on `command_exists`: a wrapper call that
        // supplies its own PATH launches against a search path the probe never
        // examined, so the preflight is not authoritative for it. Pinned here so
        // the boundary is proven rather than assumed — no caller in this crate
        // supplies a PATH, and this row fails if one ever makes the probe agree
        // by accident.
        assert!(
            !command_exists(PROBE),
            "the inherited PATH must not carry PROBE, or this row proves nothing \
             about the caller-supplied path taking precedence"
        );

        let output = command_with_output(
            workspace.path(),
            PROBE,
            &[],
            &[("PATH", &installed.path().to_string_lossy())],
        )?;
        assert_eq!(output.trim(), "INSTALLED");
        Ok(())
    }

    // #14150 review (Devin, round 4): the policy governs only launches whose
    // resolution depends on a search path. An explicit command path is resolved
    // directly, so filtering its child's PATH stripped entries the command's own
    // subprocesses were configured to use, without buying any coherence.

    #[test]
    fn an_explicit_command_keeps_a_caller_supplied_relative_path() -> TestResult {
        let shell = Path::new("/bin/sh");
        if !shell.is_file() {
            return Err(io::Error::other("instrument unavailable: /bin/sh is not a file").into());
        }

        let workspace = TempDir::new("coherence-nested")?;
        let tools = workspace.path().join("tools");
        fs::create_dir_all(&tools)?;
        let nested = tools.join("nested-probe");
        fs::write(&nested, "#!/bin/sh\necho NESTED_RESOLVED\n")?;
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o755))?;

        // The explicit command resolves without a search path; the *nested*
        // bare tool is what needs the caller's relative PATH to survive.
        let output = command_with_output(
            workspace.path(),
            &shell.to_string_lossy(),
            &["-c", "nested-probe"],
            &[("PATH", "tools")],
        )?;

        assert_eq!(
            output.trim(),
            "NESTED_RESOLVED",
            "a caller-configured PATH must reach an explicit command's own subprocesses"
        );
        Ok(())
    }

    // #14150 review (Devin, round 3): the admission policy has to read the
    // *effective* search path. Reading only the inherited one rejected a bare
    // name that the caller's own absolute PATH resolves; reading only the
    // caller's would let a relative component in an explicitly configured PATH
    // reach `repo_root`. The inherited PATH is process-global, so these rows run
    // in a re-exec'd child.

    const CALLER_PATH_WORKSPACE_ENV: &str = "PERL_CI_HYGIENE_CALLER_PATH_WORKSPACE";
    const CALLER_PATH_VALUE_ENV: &str = "PERL_CI_HYGIENE_CALLER_PATH_VALUE";
    const CALLER_PATH_FILTER: &str =
        "process::tests::child_cwd_launch_coherence::caller_path_child";
    const CALLER_PATH_MARKER: &str = "__PERL_CI_HYGIENE_CALLER_PATH__";

    /// Runs inside a child whose *inherited* PATH is unusable. Reports what the
    /// wrapper does when the caller supplies a PATH through `env_vars`.
    #[test]
    fn caller_path_child() -> TestResult {
        let Ok(workspace) = env::var(CALLER_PATH_WORKSPACE_ENV) else {
            return Ok(());
        };
        let configured = env::var(CALLER_PATH_VALUE_ENV).unwrap_or_default();
        let env_vars: Vec<(&str, &str)> =
            if configured.is_empty() { Vec::new() } else { vec![("PATH", configured.as_str())] };

        let launched = match command_with_output(Path::new(&workspace), PROBE, &[], &env_vars) {
            Ok(stdout) => stdout.trim().to_owned(),
            Err(error) => format!("<error: {error}>"),
        };
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{CALLER_PATH_MARKER}:{launched}")?;
        stdout.flush()?;
        Ok(())
    }

    #[test]
    fn a_caller_supplied_path_is_honored_over_an_unusable_inherited_one() -> TestResult {
        let workspace = TempDir::new("coherence-caller-workspace")?;
        let installed = TempDir::new("coherence-caller-installed")?;
        plant(workspace.path(), "WORKSPACE")?;
        plant(installed.path(), "INSTALLED")?;

        let report = |configured: &str| -> TestResult<String> {
            let output = Command::new(env::current_exe()?)
                .args([CALLER_PATH_FILTER, "--exact", "--nocapture"])
                .env("PATH", ".")
                .env(CALLER_PATH_WORKSPACE_ENV, workspace.path())
                .env(CALLER_PATH_VALUE_ENV, configured)
                .output()?;
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            marker_value(&stdout, CALLER_PATH_MARKER).map(ToOwned::to_owned).ok_or_else(|| {
                io::Error::other(format!("caller-path child reported nothing: {stdout}")).into()
            })
        };

        // The regression: an absolute PATH supplied by the caller must launch
        // the tool even though the inherited PATH admits nothing.
        assert_eq!(
            report(&installed.path().to_string_lossy())?,
            "INSTALLED",
            "a caller-supplied absolute PATH must be honored over an unusable inherited one"
        );

        // Control: without that override the same call is refused, so the row
        // above is not passing for some unrelated reason.
        let refused = report("")?;
        assert!(
            refused.starts_with("<error:") && refused.contains("is not available"),
            "an unusable inherited PATH with no caller override must be refused, got {refused}"
        );

        // The policy is uniform: a caller-supplied PATH that is itself
        // unadmissible cannot reach the workspace candidate either.
        let relative = report(".")?;
        assert!(
            relative.starts_with("<error:") && relative.contains("is not available"),
            "a caller-supplied relative PATH must not reach the workspace, got {relative}"
        );
        Ok(())
    }

    // End-to-end wiring proof.
    //
    // `configure_child` reads this process's own `PATH`, so proving the
    // wrappers actually apply the policy needs a parent whose PATH carries an
    // ambiguous component. That is process-global state, so it is established
    // in an exact child process rather than mutated in a parallel in-process
    // test — the same re-exec fixture shape `process_fixture_child` uses.

    const WIRING_SCENARIO_ENV: &str = "PERL_CI_HYGIENE_WIRING_WORKSPACE";
    const WIRING_FILTER: &str = "process::tests::child_cwd_launch_coherence::wrapper_wiring_child";
    const WIRING_MARKER: &str = "__PERL_CI_HYGIENE_WIRING_LAUNCHED__";

    // Discovery side, isolated the same way: the ambiguous PATH component has
    // to resolve against the *probing* process's own directory for the row to
    // mean anything, and that directory is process-global.

    const DISCOVERY_SCENARIO_ENV: &str = "PERL_CI_HYGIENE_DISCOVERY_PATH";
    const DISCOVERY_FILTER: &str = "process::tests::child_cwd_launch_coherence::discovery_child";
    const DISCOVERY_MARKER: &str = "__PERL_CI_HYGIENE_DISCOVERY__";

    /// Runs inside a child whose working directory holds a planted candidate.
    /// Reports both the admission policy's answer and the pre-repair
    /// expression's answer for the same PATH.
    #[test]
    fn discovery_child() -> TestResult {
        let Ok(raw) = env::var(DISCOVERY_SCENARIO_ENV) else {
            return Ok(());
        };
        let path = OsString::from(raw);
        let admitted = command_exists_in_path(PROBE, Some(path.as_os_str()));
        // What `command_exists_in_path` did before #14150: join the component
        // as given, which resolves it against *this* process's directory.
        let unfiltered = env::split_paths(path.as_os_str()).any(|dir| dir.join(PROBE).is_file());
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{DISCOVERY_MARKER}:admitted={admitted},unfiltered={unfiltered}")?;
        stdout.flush()?;
        Ok(())
    }

    #[test]
    fn discovery_rejects_a_candidate_reachable_only_through_an_ambiguous_component() -> TestResult {
        let decoy = TempDir::new("coherence-discovery-decoy")?;
        plant(decoy.path(), "DECOY")?;
        let nested = decoy.path().join("reldir");
        fs::create_dir_all(&nested)?;
        plant(&nested, "DECOY")?;

        for raw in [".", "", "reldir"] {
            let output = Command::new(env::current_exe()?)
                .args([DISCOVERY_FILTER, "--exact", "--nocapture"])
                .current_dir(decoy.path())
                .env(DISCOVERY_SCENARIO_ENV, raw)
                .output()?;
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let reported = marker_value(&stdout, DISCOVERY_MARKER).ok_or_else(|| {
                io::Error::other(format!("discovery child reported nothing: {stdout}"))
            })?;

            assert_eq!(
                reported, "admitted=false,unfiltered=true",
                "PATH={raw:?}: the candidate must still be reachable by the pre-repair \
                 expression, proving the fixture reproduces the defect, yet be rejected \
                 by the admission policy"
            );
        }
        Ok(())
    }

    // #14150 review (Devin): removing PATH is not an empty search list. Unix
    // `execvp` substitutes the platform's own default directories when PATH is
    // unset, so before `configure_child` refused the launch, a wrapper could run
    // `/bin/sh` while `command_exists("sh")` reported it absent — a
    // discovery/launch disagreement of exactly the kind this module prevents.
    //
    // `sh` is deliberately an ambient platform tool rather than a repository
    // probe: the claim is precisely about the platform default search path,
    // which no repository-owned fixture can occupy.

    const PLATFORM_TOOL: &str = "sh";
    const DEFAULT_PATH_SCENARIO_ENV: &str = "PERL_CI_HYGIENE_DEFAULT_PATH_WORKSPACE";
    const DEFAULT_PATH_FILTER: &str =
        "process::tests::child_cwd_launch_coherence::platform_default_child";
    const DEFAULT_PATH_MARKER: &str = "__PERL_CI_HYGIENE_DEFAULT_PATH__";

    /// Runs inside a child with a controlled PATH. Reports whether discovery
    /// admits a tool that lives in the platform's default directories, and
    /// whether the production wrapper will launch it.
    #[test]
    fn platform_default_child() -> TestResult {
        let Ok(workspace) = env::var(DEFAULT_PATH_SCENARIO_ENV) else {
            return Ok(());
        };
        let admitted = command_exists(PLATFORM_TOOL);
        let launched = match command_with_output(
            Path::new(&workspace),
            PLATFORM_TOOL,
            &["-c", "echo RAN"],
            &[],
        ) {
            Ok(stdout) => stdout.trim().to_owned(),
            Err(error) => format!("<error: {error}>"),
        };
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{DEFAULT_PATH_MARKER}:admitted={admitted},launched={launched}")?;
        stdout.flush()?;
        Ok(())
    }

    #[test]
    fn an_unadmitted_path_cannot_launch_a_platform_default_tool() -> TestResult {
        let workspace = TempDir::new("coherence-platform-default")?;

        let report = |path_value: &OsStr| -> TestResult<String> {
            let output = Command::new(env::current_exe()?)
                .args([DEFAULT_PATH_FILTER, "--exact", "--nocapture"])
                .env("PATH", path_value)
                .env(DEFAULT_PATH_SCENARIO_ENV, workspace.path())
                .output()?;
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            marker_value(&stdout, DEFAULT_PATH_MARKER).map(ToOwned::to_owned).ok_or_else(|| {
                io::Error::other(format!("platform-default child reported nothing: {stdout}"))
                    .into()
            })
        };

        // Control: with the real PATH the tool is genuinely reachable, so the
        // rejection below is the policy and not a missing instrument.
        let inherited = env::var_os("PATH").unwrap_or_default();
        assert_eq!(
            report(&inherited)?,
            "admitted=true,launched=RAN",
            "the platform tool must be admitted and launched under an ordinary absolute PATH"
        );

        // With nothing admissible, discovery reports the tool absent. The
        // launch must agree instead of falling back to the platform default
        // directories that discovery never searched.
        for raw in [".", "", ":", ".:"] {
            let reported = report(OsStr::new(raw))?;
            assert!(
                reported.starts_with("admitted=false,launched=<error:"),
                "PATH={raw:?}: discovery and launch must agree that the tool is \
                 unavailable, got {reported}"
            );
            assert!(
                reported.contains("is not available"),
                "PATH={raw:?}: the refusal must name the unusable search path, got {reported}"
            );
        }
        Ok(())
    }

    /// Runs inside the re-exec'd child, whose inherited PATH begins with an
    /// ambiguous component that reaches the workspace directory. Reports which
    /// candidate the production wrapper actually launched.
    #[test]
    fn wrapper_wiring_child() -> TestResult {
        let Ok(workspace) = env::var(WIRING_SCENARIO_ENV) else {
            return Ok(());
        };
        let launched = match command_with_output(Path::new(&workspace), PROBE, &[], &[]) {
            Ok(stdout) => stdout.trim().to_owned(),
            Err(error) => format!("<error: {error}>"),
        };
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{WIRING_MARKER}:{launched}")?;
        stdout.flush()?;
        Ok(())
    }

    #[test]
    fn wrappers_apply_the_policy_to_the_launch_they_perform() -> TestResult {
        let workspace = TempDir::new("coherence-wiring-workspace")?;
        let installed = TempDir::new("coherence-wiring-installed")?;
        plant(workspace.path(), "WORKSPACE")?;
        plant(installed.path(), "INSTALLED")?;

        // The child inherits a PATH whose first component is the current
        // directory. Under `current_dir(workspace)` that names the planted
        // decoy, so an unbound wrapper launches WORKSPACE.
        let output = Command::new(env::current_exe()?)
            .args([WIRING_FILTER, "--exact", "--nocapture"])
            .env("PATH", mixed_path(".", installed.path()))
            .env(WIRING_SCENARIO_ENV, workspace.path())
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let launched = marker_value(&stdout, WIRING_MARKER).ok_or_else(|| {
            io::Error::other(format!("wiring child reported no launch: {stdout}"))
        })?;

        assert_eq!(
            launched, "INSTALLED",
            "the wrappers must launch the candidate discovery admitted, not the \
             workspace copy reached through the inherited `.` PATH component"
        );
        Ok(())
    }
}
