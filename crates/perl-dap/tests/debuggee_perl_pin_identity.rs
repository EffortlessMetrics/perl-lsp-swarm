//! Identity proof for a valid configured debuggee pin (#12594).
//!
//! This drives the real DAP launch boundary with a valid pin and a deliberately
//! conflicting PATH interpreter, then observes the selected identity in the
//! stopped session.

#![expect(
    clippy::print_stderr,
    reason = "Integration-test diagnostic output; tracing is not wired into test helpers."
)]
#![allow(unsafe_code)] // required for std::env::set_var/remove_var in Rust 2024 (unsafe fn)

mod common;

use common::{
    DEBUGGEE_PERL_OVERRIDE_ENV, DapWorkflowSession, probe_debuggee_perl_for_test,
    resolve_debuggee_perl,
};
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tempfile::tempdir;

struct EnvGuard(Option<std::ffi::OsString>);

impl EnvGuard {
    fn set(value: &std::ffi::OsStr) -> Self {
        let previous = env::var_os(DEBUGGEE_PERL_OVERRIDE_ENV);
        unsafe { env::set_var(DEBUGGEE_PERL_OVERRIDE_ENV, value) };
        Self(previous)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => unsafe { env::set_var(DEBUGGEE_PERL_OVERRIDE_ENV, value) },
            None => unsafe { env::remove_var(DEBUGGEE_PERL_OVERRIDE_ENV) },
        }
    }
}

fn ambient_perl_path() -> Result<Option<PathBuf>, Box<dyn Error>> {
    let lookup = if cfg!(windows) { "where.exe" } else { "which" };
    let output = Command::new(lookup).arg("perl").output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(path) = stdout.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return Ok(None);
    };
    Ok(Some(std::fs::canonicalize(path)?))
}

#[test]
fn valid_pin_selects_the_pinned_usable_identity() -> Result<(), Box<dyn Error>> {
    let Some(ambient) = ambient_perl_path()? else {
        eprintln!("SKIP valid pin identity: no perl executable is available on PATH");
        return Ok(());
    };
    let Some(parent) = ambient.parent() else {
        return Err("ambient perl path has no parent directory".into());
    };
    let Some(name) = ambient.file_name() else {
        return Err("ambient perl path has no executable name".into());
    };
    let pinned = parent.join(".").join(name);

    if let Err(reason) = probe_debuggee_perl_for_test(&ambient, Duration::from_secs(15), false) {
        eprintln!("SKIP valid pin identity: ambient perl is not pipe-capable ({reason})");
        return Ok(());
    }
    if let Err(reason) = probe_debuggee_perl_for_test(&pinned, Duration::from_secs(15), false) {
        eprintln!("SKIP valid pin identity: explicit perl spelling is not pipe-capable ({reason})");
        return Ok(());
    }

    let controls = tempdir()?;
    let conflicting = controls.path().join(if cfg!(windows) { "perl.exe" } else { "perl" });
    let source = controls.path().join("conflicting.rs");
    fs::write(&source, "fn main() { std::process::exit(97); }\n")?;
    let output = Command::new("rustc")
        .args(["--edition", "2024"])
        .arg(&source)
        .arg("-o")
        .arg(&conflicting)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "failed to compile conflicting PATH control: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    let _guard = EnvGuard::set(pinned.as_os_str());
    let resolved = resolve_debuggee_perl().ok_or("valid pin did not resolve")?;
    assert_eq!(resolved.binary, pinned, "resolver must retain the exact usable pinned identity");
    assert!(
        !resolved.identity.trim().is_empty(),
        "selected pinned interpreter must produce a probe identity"
    );
    let script = controls.path().join("identity.pl");
    fs::write(&script, "print \"PIN_IDENTITY:$^X\\n\";\n")?;
    let mut session = DapWorkflowSession::new(Duration::from_secs(15))?;
    session.launch_pinned_with_env(
        &pinned,
        script.to_str().ok_or("script path is not UTF-8")?,
        &serde_json::json!({"PATH": controls.path()}),
    )?;
    let stopped = session.wait_stopped()?;
    let (identity, _) = session.evaluate_expression("$^X", stopped.thread_id)?;
    assert!(
        identity.contains(
            pinned.file_name().and_then(|name| name.to_str()).ok_or("pinned name is not UTF-8")?
        ),
        "the launched debugger identity must come from the pinned interpreter, got {identity:?}"
    );
    session.disconnect()?;
    Ok(())
}
