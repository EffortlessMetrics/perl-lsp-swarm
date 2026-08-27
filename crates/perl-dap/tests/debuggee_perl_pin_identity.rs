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

use common::{
    DEBUGGEE_PERL_OVERRIDE_ENV, DapWorkflowSession, probe_debuggee_perl_for_test,
    resolve_debuggee_perl, workflow_timeout,
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

fn compile_probe_control(
    directory: &std::path::Path,
    name: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let source = directory.join(format!("{name}.rs"));
    let binary =
        directory.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() });
    let identity = name.replace('-', "_");
    fs::write(&source, format!("fn main() {{ println!(\"{identity}\\n15\"); }}\n"))?;
    let output = Command::new("rustc")
        .args(["--edition", "2024"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "failed to compile {name}: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(binary)
}

#[test]
#[serial(dap_debuggee_environment)]
fn valid_pin_selects_the_pinned_usable_identity() -> Result<(), Box<dyn Error>> {
    let controls = tempfile::tempdir()?;
    let ambient_control = compile_probe_control(controls.path(), "perl")?;
    let pinned = compile_probe_control(controls.path(), "pinned-perl")?;
    let mut path_value = controls.path().as_os_str().to_os_string();
    path_value.push(if cfg!(windows) { ";" } else { ":" });
    path_value.push(env::var_os("PATH").unwrap_or_default());
    let _path_guard = EnvGuard::set("PATH", &path_value);

    // Establish that both the ambient spelling and the explicit spelling are
    // usable under the same real pipe probe before testing selection. A
    // resolver that only records the pin without executing it cannot satisfy
    // this control.
    let ambient_probe =
        probe_debuggee_perl_for_test(&ambient_control, Duration::from_secs(2), false)
            .map_err(|reason| format!("ambient control was not probe-capable: {reason}"))?;
    let pinned_probe = probe_debuggee_perl_for_test(&pinned, Duration::from_secs(2), false)
        .map_err(|reason| format!("pinned control was not probe-capable: {reason}"))?;
    if ambient_probe.identity == pinned_probe.identity {
        return Err("ambient and pinned controls must expose distinct identities".into());
    }
    let expected_pinned = common::normalize_explicit_debuggee_pin(&pinned)
        .map_err(|reason| format!("pinned control did not canonicalize: {reason}"))?;
    let expected_pinned_text = expected_pinned.to_string_lossy().into_owned();

    let _guard = EnvGuard::set(DEBUGGEE_PERL_OVERRIDE_ENV, pinned.as_os_str());
    let resolved = resolve_debuggee_perl().ok_or("valid pin did not resolve")?;
    let launch_path = common::resolve_launch_perl_path()
        .map_err(|reason| format!("valid pin could not resolve for launch: {reason}"))?;
    if launch_path != Some(expected_pinned.clone()) {
        return Err(format!(
            "shared launch helpers must receive the exact pinned identity, got {launch_path:?}"
        )
        .into());
    }
    let launch_arguments = common::resolved_launch_arguments_for_test("fixture.pl", None, true)
        .map_err(|reason| format!("resolved launch request could not be built: {reason}"))?;
    if launch_arguments.get("perlPath").and_then(|value| value.as_str())
        != Some(expected_pinned_text.as_str())
    {
        return Err("the convenience launch request must carry the pinned identity".into());
    }
    if resolved.binary != expected_pinned {
        return Err(
            "resolver must retain the exact usable pinned identity instead of selecting PATH perl"
                .into(),
        );
    }
    if !resolved.identity.contains("pinned_perl") || resolved.identity.contains("ambient_perl") {
        return Err(format!(
            "selected identity must come from the pinned control, got: {}",
            resolved.identity
        )
        .into());
    }
    Ok(())
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
    let reported_lower = reported.to_ascii_lowercase();
    if !(reported_lower.contains("pinned-perl")
        && !reported_lower.contains("\\perl.exe")
        && !reported_lower.contains("/perl\n"))
    {
        return Err(format!(
            "live DebugAdapter evaluated $^X from the wrong interpreter: {reported}"
        )
        .into());
    }
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
