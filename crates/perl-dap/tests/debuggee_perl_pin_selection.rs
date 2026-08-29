//! Positive selection proof for a configured debuggee Perl pin (#12594).
//!
//! This test is intentionally a separate integration-test binary because the
//! resolver's process-global `OnceLock` must be initialized only after the
//! deterministic PATH and pin controls are installed.

#![allow(unsafe_code)]

mod common;

use common::{DEBUGGEE_PERL_OVERRIDE_ENV, probe_debuggee_perl_for_test, resolve_debuggee_perl};
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
    // This is a Rust control, not Perl: the test-only probe accepts stdout
    // containing `15` in its final signal check as a deterministic usable
    // interpreter stand-in.
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
