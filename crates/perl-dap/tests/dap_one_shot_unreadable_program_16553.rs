//! Discriminating proof that one-shot emit flags fail closed on an unreadable
//! program (#16553).
//!
//! These tests spawn the packaged `perl-dap` binary. They fail if
//! `--ptkdb-bootstrap-rc` or `--debug-session-plan` exit 0 and emit a
//! degenerate artifact (`source_facts: {}`, stub `.ptkdbrc`) when the program
//! cannot be read. They also fail if a *readable* program is treated as an
//! error merely because `source_facts` is empty.
//!
//! `--bin perl-dap` unit tests cannot see this seam: clap, process exit, and
//! stdout/stderr are the user-visible contract.

use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const CLI_TIMEOUT: Duration = Duration::from_secs(15);
const FLAGS: [&str; 2] = ["--ptkdb-bootstrap-rc", "--debug-session-plan"];
const PARSEABLE: &str = "sub run {\n    my $x = 1;\n    return $x;\n}\n";

fn dap_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perl-dap"))
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn drain_pipe<R: Read + Send + 'static>(reader: Option<R>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut reader) = reader {
            let _ = reader.read_to_string(&mut buf);
        }
        buf
    })
}

fn run_cli(args: &[&str]) -> Result<(ExitStatus, String, String)> {
    let mut child = Command::new(dap_binary())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn perl-dap")?;
    let stdout = drain_pipe(child.stdout.take());
    let stderr = drain_pipe(child.stderr.take());
    let status = wait_for_exit(&mut child, CLI_TIMEOUT)?.ok_or_else(|| {
        let _ = child.kill();
        let _ = child.wait();
        anyhow!(
            "perl-dap {args:?} still running after {CLI_TIMEOUT:?}; a one-shot emit must not hang"
        )
    })?;
    let stdout = stdout.join().unwrap_or_else(|_| "<stdout reader panicked>".to_owned());
    let stderr = stderr.join().unwrap_or_else(|_| "<stderr reader panicked>".to_owned());
    Ok((status, stdout, stderr))
}

fn assert_unreadable_fails(flag: &str, program: &str, path_needle: &str) -> Result<()> {
    let (status, stdout, stderr) = run_cli(&[flag, program, "--log-level", "error"])?;
    if status.success() {
        return Err(anyhow!(
            "{flag} must exit nonzero for unreadable {program:?}; stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    if !stdout.trim().is_empty() {
        return Err(anyhow!(
            "{flag} must not emit an artifact for unreadable {program:?}: {stdout:?}"
        ));
    }
    for expected in [flag, "could not be read", path_needle] {
        if !stderr.contains(expected) {
            return Err(anyhow!(
                "{flag} stderr must name the flag, the failure, and the path ({expected}); stderr={stderr:?}"
            ));
        }
    }
    Ok(())
}

#[test]
fn missing_program_fails_nonzero_with_no_artifact_for_both_flags() -> Result<()> {
    let missing = "./no-such-dir/no-such-16553.pl";
    for flag in FLAGS {
        assert_unreadable_fails(flag, missing, "no-such-16553.pl")?;
    }
    Ok(())
}

#[test]
fn directory_program_fails_nonzero_with_no_artifact_for_both_flags() -> Result<()> {
    // `Path::exists()` is true for a directory, while `read_to_string` is not.
    // A guard of `if !program.exists() { return Ok(()) }` (or the inverse that
    // only checks existence) would still emit a degenerate plan.
    let dir = tempfile::tempdir().context("tempdir")?;
    let program = dir.path().to_string_lossy().into_owned();
    let needle = dir
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("tempdir name must be utf-8"))?;
    for flag in FLAGS {
        assert_unreadable_fails(flag, &program, needle)?;
    }
    Ok(())
}

#[test]
fn invalid_utf8_program_fails_nonzero_with_no_artifact_for_both_flags() -> Result<()> {
    let dir = tempfile::tempdir().context("tempdir")?;
    let program = dir.path().join("invalid-utf8-16553.pl");
    fs::write(&program, [0xff, 0xfe, 0x00]).context("write invalid utf-8")?;
    let program_str = program.to_string_lossy().into_owned();
    for flag in FLAGS {
        assert_unreadable_fails(flag, &program_str, "invalid-utf8-16553.pl")?;
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn permission_denied_program_fails_nonzero_with_no_artifact_for_both_flags() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().context("tempdir")?;
    let program = dir.path().join("unreadable-16553.pl");
    fs::write(&program, PARSEABLE).context("write unreadable fixture")?;
    fs::set_permissions(&program, fs::Permissions::from_mode(0o000)).context("chmod 000")?;
    if fs::read_to_string(&program).is_ok() {
        // Root and some filesystems ignore mode bits; this case is not
        // observable here. Missing/directory/invalid-UTF-8 still cover the
        // fail-closed read.
        return Ok(());
    }
    let program_str = program.to_string_lossy().into_owned();
    for flag in FLAGS {
        assert_unreadable_fails(flag, &program_str, "unreadable-16553.pl")?;
    }
    Ok(())
}

#[test]
fn readable_parseable_program_emits_source_facts_and_exits_zero() -> Result<()> {
    let dir = tempfile::tempdir().context("tempdir")?;
    let program = dir.path().join("prog.pl");
    fs::write(&program, PARSEABLE).context("write parseable fixture")?;
    let program_str = program.to_string_lossy().into_owned();

    let (status, stdout, stderr) =
        run_cli(&["--debug-session-plan", &program_str, "--log-level", "error"])?;
    if !status.success() {
        return Err(anyhow!(
            "a readable program must emit the plan and exit zero. stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    let plan: Value =
        serde_json::from_str(&stdout).map_err(|e| anyhow!("stdout must be a JSON plan: {e}"))?;
    let facts = plan.get("source_facts").ok_or_else(|| {
        anyhow!("plan must carry source_facts for a readable program: {stdout:?}")
    })?;
    if facts.as_object().is_none_or(|entries| entries.is_empty()) {
        return Err(anyhow!("source_facts must be populated for a parseable program, got {facts}"));
    }
    Ok(())
}

#[test]
fn readable_parseable_program_bootstrap_rc_exits_zero() -> Result<()> {
    let dir = tempfile::tempdir().context("tempdir")?;
    let program = dir.path().join("prog.pl");
    fs::write(&program, PARSEABLE).context("write parseable fixture")?;
    let program_str = program.to_string_lossy().into_owned();

    let (status, stdout, stderr) =
        run_cli(&["--ptkdb-bootstrap-rc", &program_str, "--log-level", "error"])?;
    if !status.success() {
        return Err(anyhow!(
            "a readable program must emit a bootstrap rc and exit zero. stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    if !stdout.contains("# program:") {
        return Err(anyhow!("bootstrap rc must name the program: {stdout:?}"));
    }
    if !stdout.trim_end().ends_with("1;") {
        return Err(anyhow!("bootstrap rc must be valid Perl ending in 1;: {stdout:?}"));
    }
    Ok(())
}

#[test]
fn readable_empty_program_still_exits_zero() -> Result<()> {
    // Opposite-direction control: empty `source_facts` after a successful read
    // is honest and must not be collapsed into the unreadable-program failure.
    let dir = tempfile::tempdir().context("tempdir")?;
    let program = dir.path().join("empty.pl");
    fs::write(&program, "").context("write empty fixture")?;
    assert_readable_exits_zero(&program)?;
    Ok(())
}

#[test]
fn readable_unparseable_program_still_exits_zero() -> Result<()> {
    // `source_facts_from_text` skips non-parseable sources. That is not a read
    // failure; exit 0 with possibly-empty facts is the honest outcome.
    let dir = tempfile::tempdir().context("tempdir")?;
    let program = dir.path().join("not-perl.pl");
    fs::write(&program, "!!! this is not perl\n").context("write unparseable fixture")?;
    assert_readable_exits_zero(&program)?;
    Ok(())
}

fn assert_readable_exits_zero(program: &Path) -> Result<()> {
    let program_str = program.to_string_lossy().into_owned();
    for flag in FLAGS {
        let (status, stdout, stderr) = run_cli(&[flag, &program_str, "--log-level", "error"])?;
        if !status.success() {
            return Err(anyhow!(
                "{flag} must exit zero after a successful read. stdout={stdout:?} stderr={stderr:?}"
            ));
        }
        if stdout.trim().is_empty() {
            return Err(anyhow!("{flag} must still emit an artifact after a successful read"));
        }
    }
    Ok(())
}
