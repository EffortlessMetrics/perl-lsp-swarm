//! Identity proof for a valid configured debuggee pin (#12594).
//!
//! The availability matrix proves that a rejected pin cannot be rescued by a
//! PATH interpreter. This companion test proves the positive direction with
//! two usable path identities: the ambient `perl` executable and an explicit
//! spelling of that same executable through a `.` path component. Both are
//! independently exercised by the real pipe-conformance probe, then the pin
//! is selected and the resolver must retain the exact pinned spelling.

#![expect(
    clippy::print_stderr,
    reason = "Integration-test diagnostic output; tracing is not wired into test helpers."
)]
#![allow(unsafe_code)] // required for std::env::set_var/remove_var in Rust 2024 (unsafe fn)

mod common;

use common::{DEBUGGEE_PERL_OVERRIDE_ENV, probe_debuggee_perl_for_test, resolve_debuggee_perl};
use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

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

    // Establish that both the ambient spelling and the explicit spelling are
    // usable under the same real pipe probe before testing selection. A
    // resolver that only records the pin without executing it cannot satisfy
    // this control.
    if let Err(reason) = probe_debuggee_perl_for_test(&ambient, Duration::from_secs(15), false) {
        eprintln!("SKIP valid pin identity: ambient perl is not pipe-capable ({reason})");
        return Ok(());
    }
    if let Err(reason) = probe_debuggee_perl_for_test(&pinned, Duration::from_secs(15), false) {
        eprintln!("SKIP valid pin identity: explicit perl spelling is not pipe-capable ({reason})");
        return Ok(());
    }

    let _guard = EnvGuard::set(pinned.as_os_str());
    let resolved = resolve_debuggee_perl().ok_or("valid pin did not resolve")?;
    let launch_path = common::resolve_launch_perl_path()
        .map_err(|reason| format!("valid pin could not resolve for launch: {reason}"))?;
    assert_eq!(
        launch_path,
        Some(pinned.clone()),
        "shared launch helpers must receive the exact pinned identity"
    );
    assert_eq!(
        resolved.binary, pinned,
        "resolver must retain the exact usable pinned identity instead of selecting PATH perl"
    );
    assert!(
        !resolved.identity.trim().is_empty(),
        "selected pinned interpreter must produce a non-empty probe identity"
    );
    Ok(())
}
