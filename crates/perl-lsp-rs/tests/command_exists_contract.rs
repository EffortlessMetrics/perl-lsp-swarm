//! Public command-availability contract under isolated process environments.
//!
//! Each observation runs in a fresh child test process. The parent never
//! mutates process-global `PATH`, `PATHEXT`, or the current directory, and no
//! fabricated command is executed.
//!
//! # Claim boundary
//!
//! This contract proves only the public free function
//! `perl_lsp::execute_command::command_exists`. The `pub(crate)` instance
//! method in `execute_command/provider.rs` intentionally has divergent
//! platform behavior — Windows delegates to the hardened PATH-only resolver in
//! `perl_subprocess_runtime`, and non-Windows spawns `which` under a 2-second
//! timeout — and is exercised by its own scoped proof, not by this contract.

#![cfg(not(target_arch = "wasm32"))]

use perl_lsp::execute_command::command_exists;
use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::tempdir;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const CHILD_MODE_ENV: &str = "PERL_LSP_COMMAND_EXISTS_CHILD";
const CHILD_COMMAND_ENV: &str = "PERL_LSP_COMMAND_EXISTS_NAME";
const CHILD_EXPECTED_ENV: &str = "PERL_LSP_COMMAND_EXISTS_EXPECTED";
const CHILD_FILTER: &str = "command_exists_contract_child";
const CHILD_PROBE_TIMEOUT: Duration = Duration::from_secs(120);
const CHILD_PROBE_POLL: Duration = Duration::from_millis(20);

fn command_candidate_name(command: &str) -> String {
    #[cfg(windows)]
    {
        format!("{command}.cmd")
    }
    #[cfg(not(windows))]
    {
        command.to_owned()
    }
}

fn joined_path(paths: &[&Path]) -> TestResult<OsString> {
    Ok(env::join_paths(paths.iter().copied())?)
}

#[cfg(windows)]
fn platform_path_ext() -> Option<&'static OsStr> {
    Some(OsStr::new(".COM;.EXE;.BAT;.CMD"))
}

#[cfg(not(windows))]
fn platform_path_ext() -> Option<&'static OsStr> {
    None
}

#[cfg(unix)]
fn set_file_mode(path: &Path, mode: u32) -> TestResult {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn write_valid_candidate(directory: &Path, command: &str) -> TestResult<PathBuf> {
    let candidate = directory.join(command_candidate_name(command));
    #[cfg(windows)]
    fs::write(&candidate, b"@exit /b 0\r\n")?;
    #[cfg(not(windows))]
    fs::write(&candidate, b"command availability fixture\n")?;

    #[cfg(unix)]
    set_file_mode(&candidate, 0o755)?;

    Ok(candidate)
}

fn run_child_probe(
    command: &str,
    path: Option<&OsStr>,
    path_ext: Option<&OsStr>,
    current_dir: &Path,
    expected: bool,
) -> TestResult {
    let test_executable = env::current_exe()?;
    let mut child = Command::new(test_executable);
    child
        .current_dir(current_dir)
        .args([CHILD_FILTER, "--exact", "--nocapture"])
        .env(CHILD_MODE_ENV, "1")
        .env(CHILD_COMMAND_ENV, command)
        .env(CHILD_EXPECTED_ENV, if expected { "true" } else { "false" });

    if let Some(path) = path {
        child.env("PATH", path);
    } else {
        child.env_remove("PATH");
    }

    #[cfg(windows)]
    if let Some(path_ext) = path_ext {
        child.env("PATHEXT", path_ext);
    } else {
        child.env_remove("PATHEXT");
    }
    #[cfg(not(windows))]
    let _ = path_ext;

    child.stdout(Stdio::piped()).stderr(Stdio::piped());
    let probe_started = Instant::now();
    let mut child = child.spawn()?;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if probe_started.elapsed() >= CHILD_PROBE_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other(format!(
                "isolated command probe for {command:?} (expected {expected}) timed out after {CHILD_PROBE_TIMEOUT:?}"
            ))
            .into());
        }
        thread::sleep(CHILD_PROBE_POLL);
    };

    let mut probe_stdout = Vec::new();
    let mut probe_stderr = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_end(&mut probe_stdout)?;
    }
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_end(&mut probe_stderr)?;
    }
    let elapsed = probe_started.elapsed();

    if status.success() {
        return Ok(());
    }

    Err(io::Error::other(format!(
        "isolated command probe failed for {command:?} (expected {expected}, status {status}, elapsed {elapsed:?}):\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&probe_stdout),
        String::from_utf8_lossy(&probe_stderr)
    ))
    .into())
}

#[test]
fn command_exists_contract_child() -> TestResult {
    if env::var_os(CHILD_MODE_ENV).is_none() {
        return Ok(());
    }

    let command = env::var(CHILD_COMMAND_ENV)?;
    let expected_text = env::var(CHILD_EXPECTED_ENV)?;
    let expected = match expected_text.as_str() {
        "true" => true,
        "false" => false,
        other => {
            return Err(
                io::Error::other(format!("invalid {CHILD_EXPECTED_ENV} value: {other:?}")).into()
            );
        }
    };
    let actual = command_exists(&command);

    if actual == expected {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "command_exists({command:?}) returned {actual}, expected {expected}; PATH={:?}, PATHEXT={:?}, cwd={:?}",
            env::var_os("PATH"),
            env::var_os("PATHEXT"),
            env::current_dir()?
        ))
        .into())
    }
}

#[test]
fn public_command_exists_rejects_absent_candidate_and_missing_path() -> TestResult {
    let root = tempdir()?;
    let command = "perl_lsp_missing_command_subject";
    let path = joined_path(&[root.path()])?;

    run_child_probe(command, Some(path.as_os_str()), platform_path_ext(), root.path(), false)?;
    run_child_probe(command, None, platform_path_ext(), root.path(), false)
}

#[test]
fn public_command_exists_rejects_directory_candidate() -> TestResult {
    let root = tempdir()?;
    let command = "perl_lsp_directory_command_subject";
    fs::create_dir(root.path().join(command_candidate_name(command)))?;
    let path = joined_path(&[root.path()])?;

    run_child_probe(command, Some(path.as_os_str()), platform_path_ext(), root.path(), false)
}

#[test]
fn public_command_exists_continues_to_later_valid_path_candidate() -> TestResult {
    let root = tempdir()?;
    let first = root.path().join("first");
    let second = root.path().join("second");
    fs::create_dir_all(&first)?;
    fs::create_dir_all(&second)?;

    let command = "perl_lsp_ordered_command_subject";
    fs::create_dir(first.join(command_candidate_name(command)))?;
    let later_candidate = write_valid_candidate(&second, command)?;
    let path = joined_path(&[first.as_path(), second.as_path()])?;

    run_child_probe(command, Some(path.as_os_str()), platform_path_ext(), root.path(), true)?;

    fs::remove_file(later_candidate)?;
    run_child_probe(command, Some(path.as_os_str()), platform_path_ext(), root.path(), false)
}

#[test]
fn public_command_exists_handles_path_entries_with_spaces() -> TestResult {
    let root = tempdir()?;
    let path_entry = root.path().join("tool path with spaces");
    fs::create_dir_all(&path_entry)?;

    let command = "perl_lsp_spaced_path_command_subject";
    write_valid_candidate(&path_entry, command)?;
    let path = joined_path(&[path_entry.as_path()])?;

    run_child_probe(command, Some(path.as_os_str()), platform_path_ext(), root.path(), true)
}

#[test]
fn public_command_exists_rejects_cwd_sibling_under_empty_path_entry() -> TestResult {
    let root = tempdir()?;
    let command = "perl_lsp_empty_path_entry_command_subject";
    write_valid_candidate(root.path(), command)?;
    let path = joined_path(&[Path::new("")])?;

    // A launchable sibling sits in the child's current directory and the only
    // PATH entry is empty. The public lookup must not interpret the empty
    // entry as the working directory (the CWD-first admission seam): it must
    // reject the candidate. which 8.x filters empty PATH entries outright, so
    // this row also pins that filtering as load-bearing behavior.
    run_child_probe(command, Some(path.as_os_str()), platform_path_ext(), root.path(), false)
}

#[cfg(unix)]
#[test]
fn public_command_exists_requires_unix_executable_mode() -> TestResult {
    let root = tempdir()?;
    let command = "perl_lsp_unix_mode_command_subject";
    let candidate = root.path().join(command);
    fs::write(&candidate, b"unix executable mode fixture\n")?;
    set_file_mode(&candidate, 0o644)?;
    let path = joined_path(&[root.path()])?;

    run_child_probe(command, Some(path.as_os_str()), None, root.path(), false)?;

    set_file_mode(&candidate, 0o755)?;
    run_child_probe(command, Some(path.as_os_str()), None, root.path(), true)
}

#[cfg(unix)]
#[test]
fn public_command_exists_distinguishes_valid_and_broken_symlinks() -> TestResult {
    use std::os::unix::fs::symlink;

    let root = tempdir()?;
    let path_entry = root.path().join("bin");
    fs::create_dir_all(&path_entry)?;

    let command = "perl_lsp_symlink_command_subject";
    let target = root.path().join("real-command-target");
    fs::write(&target, b"symlink target fixture\n")?;
    set_file_mode(&target, 0o755)?;
    symlink(&target, path_entry.join(command))?;
    let path = joined_path(&[path_entry.as_path()])?;

    run_child_probe(command, Some(path.as_os_str()), None, root.path(), true)?;

    fs::remove_file(target)?;
    run_child_probe(command, Some(path.as_os_str()), None, root.path(), false)
}

#[cfg(windows)]
#[test]
fn public_command_exists_honors_windows_pathext() -> TestResult {
    let root = tempdir()?;
    let command = "perl_lsp_windows_pathext_subject";
    write_valid_candidate(root.path(), command)?;
    let path = joined_path(&[root.path()])?;

    run_child_probe(command, Some(path.as_os_str()), Some(OsStr::new(".CMD")), root.path(), true)?;
    run_child_probe(command, Some(path.as_os_str()), Some(OsStr::new(".EXE")), root.path(), false)
}
