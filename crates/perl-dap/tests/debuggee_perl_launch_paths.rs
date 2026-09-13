//! Live launch-path proof for the configured debuggee Perl pin (#12594).
//!
//! Each convenience launch path must carry the same explicit `perlPath` into
//! the real adapter. PATH is deliberately made to resolve a different copy,
//! and the stopped session observes `$^X` so a PATH fallback cannot pass.

#![allow(unsafe_code)]

mod common;

use common::{
    DEBUGGEE_PERL_OVERRIDE_ENV, DapWorkflowSession, probe_debuggee_perl_for_test, workflow_timeout,
};
use serial_test::serial;
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    #[cfg(windows)]
    fn remove(key: &'static str) -> Self {
        let previous = env::var_os(key);
        unsafe { env::remove_var(key) };
        Self { key, previous }
    }

    fn set(key: &'static str, value: &std::ffi::OsStr) -> Self {
        let previous = env::var_os(key);
        unsafe { env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => unsafe { env::set_var(self.key, value) },
            None => unsafe { env::remove_var(self.key) },
        }
    }
}

fn find_configured_or_path_pipe_perl() -> Result<Option<PathBuf>, Box<dyn Error>> {
    if let Some(configured) = env::var_os(DEBUGGEE_PERL_OVERRIDE_ENV) {
        let candidate = PathBuf::from(configured);
        if !candidate.is_file() {
            return Err(format!(
                "{DEBUGGEE_PERL_OVERRIDE_ENV} names a missing interpreter: {}",
                candidate.display()
            )
            .into());
        }
        probe_debuggee_perl_for_test(&candidate, Duration::from_secs(10), false)
            .map_err(|reason| format!("configured interpreter is not pipe-usable: {reason}"))?;
        return Ok(Some(candidate));
    }

    let locator = if cfg!(windows) { "where.exe" } else { "which" };
    let output = Command::new(locator).arg("perl").output()?;
    if !output.status.success() {
        return Ok(None);
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let candidate = PathBuf::from(line.trim());
        if candidate.is_file()
            && probe_debuggee_perl_for_test(&candidate, Duration::from_secs(10), false).is_ok()
        {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

#[test]
#[cfg(windows)]
#[serial(dap_debuggee_environment)]
#[allow(clippy::print_stderr)]
fn configured_perl_probe_uses_windows_stdio_bootstrap() -> Result<(), Box<dyn Error>> {
    let _emacs = EnvGuard::remove("EMACS");
    let _perl_rl = EnvGuard::remove("PERL_RL");
    let _perl_db_opts = EnvGuard::remove("PERLDB_OPTS");
    let Some(pin) = find_configured_or_path_pipe_perl()? else {
        if env::var_os(common::REQUIRE_PERL_ENV).is_some_and(|value| value == "1") {
            return Err("PERL_LSP_DAP_REQUIRE_PERL=1 but no pipe-capable Perl is available".into());
        }
        eprintln!("SKIP configured_perl_probe_uses_windows_stdio_bootstrap: Perl unavailable");
        return Ok(());
    };
    let _pin = EnvGuard::set(DEBUGGEE_PERL_OVERRIDE_ENV, pin.as_os_str());
    let resolved = probe_debuggee_perl_for_test(&pin, Duration::from_secs(10), false)
        .map_err(|reason| format!("configured Perl was rejected: {reason}"))?;
    let expected = fs::canonicalize(&pin)?;
    if fs::canonicalize(&resolved.binary)? != expected {
        return Err(format!(
            "resolver selected {} instead of pinned {}",
            resolved.binary.display(),
            expected.display()
        )
        .into());
    }
    if resolved.identity.trim().is_empty() {
        return Err("configured Perl probe returned no debugger identity".into());
    }
    Ok(())
}

fn observe_pin_with_session(
    mut session: DapWorkflowSession,
    launch_path: &str,
    script: &str,
    cwd: &str,
) -> Result<String, String> {
    match launch_path {
        "launch" => {
            session.launch(script)?;
            session.set_breakpoints_checked(script, &[4])?;
            session.configuration_done()?;
        }
        "launch_with_stop_on_entry" => session.launch_with_stop_on_entry(script, true)?,
        "launch_with_cwd" => {
            session.launch_with_cwd(script, cwd)?;
            session.set_breakpoints_checked(script, &[4])?;
            session.configuration_done()?;
        }
        other => return Err(format!("unknown launch path {other}")),
    }
    let stopped = session.wait_stopped_with_frame()?;
    session.evaluate_expression("$^X", stopped.frame_id).map(|(value, _)| value)
}

#[test]
#[serial]
#[allow(clippy::print_stderr)]
fn all_convenience_launch_paths_reach_the_pinned_interpreter() -> Result<(), Box<dyn Error>> {
    let Some(source_perl) = find_configured_or_path_pipe_perl()? else {
        eprintln!(
            "SKIP all_convenience_launch_paths_reach_the_pinned_interpreter: Perl unavailable"
        );
        return Ok(());
    };
    let controls = tempfile::tempdir()?;
    if cfg!(windows) {
        let source_dir = source_perl.parent().ok_or("Perl path has no parent directory")?;
        for entry in fs::read_dir(source_dir)? {
            let entry = entry?;
            if entry.path().extension().and_then(|extension| extension.to_str()) == Some("dll") {
                fs::copy(entry.path(), controls.path().join(entry.file_name()))?;
            }
        }
    }
    let ambient = controls.path().join(if cfg!(windows) { "perl.exe" } else { "perl" });
    // Keep the copied pin's basename within the adapter's strict Perl-name
    // contract while still making it distinct from the ambient `perl` copy.
    let pinned = controls.path().join(if cfg!(windows) { "perl5.exe" } else { "perl5" });
    fs::copy(&source_perl, &ambient)?;
    fs::copy(&source_perl, &pinned)?;
    for binary in [&ambient, &pinned] {
        probe_debuggee_perl_for_test(binary, Duration::from_secs(10), false)
            .map_err(|reason| format!("{} is not pipe-usable: {reason}", binary.display()))?;
    }

    let mut path_value = controls.path().as_os_str().to_os_string();
    path_value.push(if cfg!(windows) { ";" } else { ":" });
    path_value.push(env::var_os("PATH").unwrap_or_default());
    let _path_guard = EnvGuard::set("PATH", &path_value);
    let script = controls.path().join("launch-paths.pl");
    fs::write(
        &script,
        "use strict;\nuse warnings;\nmy $identity_probe = 1;\n$identity_probe++;\n",
    )?;
    let script_text = script.to_string_lossy().into_owned();
    let cwd = controls.path().to_string_lossy().into_owned();
    for launch_path in ["launch", "launch_with_stop_on_entry", "launch_with_cwd"] {
        let session = DapWorkflowSession::new_with_perl(workflow_timeout(), Some(&pinned))?;
        let reported = observe_pin_with_session(session, launch_path, &script_text, &cwd)
            .map_err(|error| format!("{launch_path} failed: {error}"))?;
        common::assert_pinned_identity(&reported, &pinned, &ambient, launch_path)
            .map_err(std::io::Error::other)?;
    }

    let _pin_guard = EnvGuard::set(DEBUGGEE_PERL_OVERRIDE_ENV, pinned.as_os_str());
    for launch_path in ["launch", "launch_with_stop_on_entry", "launch_with_cwd"] {
        let session = DapWorkflowSession::new(workflow_timeout())?;
        let configured_identity =
            observe_pin_with_session(session, launch_path, &script_text, &cwd)
                .map_err(|error| format!("configured {launch_path} failed: {error}"))?;
        common::assert_pinned_identity(
            &configured_identity,
            &pinned,
            &ambient,
            &format!("configured {launch_path}"),
        )
        .map_err(std::io::Error::other)?;
    }
    Ok(())
}

#[test]
#[cfg(windows)]
#[serial]
fn windows_pipe_launch_configures_perl_debugger_transport() -> Result<(), Box<dyn Error>> {
    let locator = Command::new("where.exe").arg("perl").output()?;
    if !locator.status.success() {
        std::io::Write::write_all(
            &mut std::io::stderr(),
            b"SKIP windows_pipe_launch_configures_perl_debugger_transport: Perl unavailable\n",
        )?;
        return Ok(());
    }
    let perl = String::from_utf8_lossy(&locator.stdout)
        .lines()
        .map(str::trim)
        .map(PathBuf::from)
        .find(|candidate| candidate.is_file())
        .ok_or("where.exe returned no Perl executable")?;
    let workspace = tempfile::tempdir()?;
    let script = workspace.path().join("pipe-transport.pl");
    fs::write(
        &script,
        "use strict;\nuse warnings;\nmy $executed = 41;\n$executed++;\nprint \"executed\\n\";\n",
    )?;
    let _emacs_guard = EnvGuard::remove("EMACS");
    let mut session = DapWorkflowSession::new_with_perl(workflow_timeout(), Some(&perl))?;
    let script_text = script.to_string_lossy().into_owned();
    session.launch_pinned(&perl, &script_text)?;
    session.set_breakpoints_checked(&script_text, &[5])?;
    session.configuration_done()?;
    let stopped = session.wait_stopped_with_frame()?;
    if stopped.line != 5 {
        return Err(format!("pipe launch stopped at unexpected line {}", stopped.line).into());
    }
    let (value, _) = session.evaluate_expression("$executed", stopped.frame_id)?;
    if value.split_whitespace().last() != Some("42") {
        return Err(format!("debuggee did not execute the expected program: {value}").into());
    }
    Ok(())
}
