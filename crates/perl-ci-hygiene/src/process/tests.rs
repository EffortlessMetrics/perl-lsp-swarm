use super::{
    command_exists_in_path, command_output_lines, command_output_with_status, command_status,
    command_status_strict, command_timed_status, command_with_input_with_status,
    command_with_output, command_with_output_all, command_with_output_allow_empty_match,
    command_with_output_allow_failure, windows_command_candidates,
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

    /// Probe selection mirrored against the seam the same way
    /// `command_exists_in_path` uses it: first existing candidate.
    fn probe_selection(command: &str, path: &OsStr) -> Option<PathBuf> {
        windows_command_candidates(command, path).into_iter().find(|c| c.is_file())
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

        assert_eq!(probe_selection("probe-parity-dir", path.as_os_str()), None);
        let launch = Command::new("probe-parity-dir").env("PATH", path.as_os_str()).output();
        assert!(launch.is_err(), "a directory named <name>.exe must not be launched");
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
