//! Identity proof for a valid configured debuggee pin (#12594).
//!
//! The availability matrix proves that a rejected pin cannot be rescued by a
//! PATH interpreter. This companion test proves the positive direction with
//! two distinct, deterministic pipe-probe controls: a fake ambient `perl` on
//! PATH and a separately compiled pinned control. Both emit unique identities
//! through the same probe seam, then the pin must win over PATH and retain its
//! exact executable identity.

#![expect(
    clippy::print_stderr,
    reason = "Integration-test diagnostic output; tracing is not wired into test helpers."
)]
#![allow(unsafe_code)] // required for std::env::set_var/remove_var in Rust 2024 (unsafe fn)

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

#[test]
#[serial(dap_debuggee_environment)]
fn live_debug_adapter_executes_the_pinned_interpreter_identity() -> Result<(), Box<dyn Error>> {
    let Some(source_perl) = find_pipe_usable_path_perl()? else {
        eprintln!(
            "SKIP live_debug_adapter_executes_the_pinned_interpreter_identity: Perl unavailable"
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

    // Both copies must first pass the same real pipe probe. This prevents a
    // path-only control from claiming that the pinned identity is usable.
    for (label, binary) in [("ambient", &ambient), ("pinned", &pinned)] {
        probe_debuggee_perl_for_test(binary, Duration::from_secs(10), false)
            .map_err(|reason| format!("{label} copied Perl was not pipe-usable: {reason}"))?;
    }

    let mut path_value = controls.path().as_os_str().to_os_string();
    path_value.push(if cfg!(windows) { ";" } else { ":" });
    path_value.push(env::var_os("PATH").unwrap_or_default());
    let _path_guard = EnvGuard::set("PATH", &path_value);

    let script = controls.path().join("identity.pl");
    fs::write(
        &script,
        "use strict;\nuse warnings;\nmy $identity_probe = 1;\n$identity_probe++;\n",
    )?;
    let script_text = script.to_string_lossy().into_owned();
    let mut session = DapWorkflowSession::new(workflow_timeout()).map_err(|e| e.to_string())?;
    session.launch_pinned(&pinned, &script_text).map_err(|e| e.to_string())?;
    let breakpoint_line = 4;
    session.set_breakpoints_checked(&script_text, &[breakpoint_line]).map_err(|e| e.to_string())?;
    session.configuration_done().map_err(|e| e.to_string())?;
    let stopped = session.wait_stopped_with_frame().map_err(|e| e.to_string())?;
    let (reported, _) =
        session.evaluate_expression("$^X", stopped.frame_id).map_err(|e| e.to_string())?;
    common::assert_pinned_identity(&reported, &pinned, &ambient, "live DebugAdapter")
        .map_err(std::io::Error::other)?;
    Ok(())
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
