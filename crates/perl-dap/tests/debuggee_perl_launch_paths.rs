//! Live launch-path proof for the configured debuggee Perl pin (#12594).
//!
//! Each convenience launch path must carry the same explicit `perlPath` into
//! the real adapter. PATH is deliberately made to resolve a different copy,
//! and the stopped session observes `$^X` so a PATH fallback cannot pass.

#![allow(unsafe_code)]

mod common;

use common::{DapWorkflowSession, probe_debuggee_perl_for_test, workflow_timeout};
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

fn find_pipe_usable_path_perl() -> Result<Option<PathBuf>, Box<dyn Error>> {
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

fn observe_pin_for_path(
    launch_path: &str,
    pinned: &std::path::Path,
    script: &str,
    cwd: &str,
) -> Result<String, String> {
    let mut session = DapWorkflowSession::new_with_perl(workflow_timeout(), Some(pinned))?;
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
#[serial(dap_debuggee_environment)]
#[allow(clippy::print_stderr)]
fn all_convenience_launch_paths_reach_the_pinned_interpreter() -> Result<(), Box<dyn Error>> {
    let Some(source_perl) = find_pipe_usable_path_perl()? else {
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
    let pinned =
        controls.path().join(if cfg!(windows) { "pinned-perl.exe" } else { "pinned-perl" });
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
        let reported = observe_pin_for_path(launch_path, &pinned, &script_text, &cwd)
            .map_err(|error| format!("{launch_path} failed: {error}"))?;
        let reported_lower = reported.to_ascii_lowercase();
        assert!(
            reported_lower.contains("pinned-perl")
                && !reported_lower.contains("\\perl.exe")
                && !reported_lower.contains("/perl\n"),
            "{launch_path} evaluated $^X from the wrong interpreter: {reported}"
        );
    }
    Ok(())
}
