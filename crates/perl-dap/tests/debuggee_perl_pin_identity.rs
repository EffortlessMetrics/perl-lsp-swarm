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
use std::path::Path;
use std::path::PathBuf;
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
            // Windows extension matching is case-insensitive (`perl528.dll`
            // and `PERL528.DLL` are the same file), so the filter must be too.
            if entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
            {
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
    // Enumerate every search-path candidate instead of parsing a locator:
    // Unix `which` resolves a bare name to only its first PATH hit, which
    // would end this proof at the first candidate even when a later PATH
    // interpreter survives staging.
    for candidate in common::search_path_perl_candidates() {
        if !candidate.is_file()
            || probe_debuggee_perl_for_test(&candidate, Duration::from_secs(10), false).is_err()
        {
            continue;
        }
        // An in-place probe is not sufficient: an interpreter whose `@INC` is
        // mount-relative (a Git-Bash/MSYS perl) passes at its installation yet
        // cannot load perl5db.pl once copied out of it. This proof stages bare
        // copies before launching, so only a relocation-surviving candidate
        // can host it.
        if staged_copy_pipe_probe(&candidate).is_ok() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// Copy `source` into a throwaway directory (with its adjacent DLLs on
/// Windows) and pipe-probe the copy, mirroring the staging this proof applies
/// to the ambient and pinned controls before launch.
fn staged_copy_pipe_probe(source: &Path) -> Result<(), String> {
    let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
    let file_name = source.file_name().ok_or("candidate has no file name")?;
    fs::copy(source, staging.path().join(file_name)).map_err(|error| error.to_string())?;
    #[cfg(windows)]
    if let Some(source_dir) = source.parent() {
        for entry in fs::read_dir(source_dir)
            .map_err(|error| format!("candidate DLL scan failed: {error}"))?
        {
            let entry = entry.map_err(|error| error.to_string())?;
            // Windows extension matching is case-insensitive: `perl528.dll`
            // and `PERL528.DLL` are the same file on disk.
            if entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
            {
                fs::copy(entry.path(), staging.path().join(entry.file_name()))
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    probe_debuggee_perl_for_test(&staging.path().join(file_name), Duration::from_secs(10), false)
        .map(|_| ())
}
