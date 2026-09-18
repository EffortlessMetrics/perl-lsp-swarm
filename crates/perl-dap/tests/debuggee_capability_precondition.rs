//! Discriminating proof for the debugger-capability precondition (#15429).
#![allow(clippy::expect_used)]
//!
//! Before pinning an interpreter, resolution must refuse one that cannot
//! load `perl5db.pl` with the typed precondition reason ("selected perl
//! cannot host the debugger") instead of the generic mid-session pipe
//! failure. The captured real-world shape is a Git-Bash/MSYS perl whose
//! `@INC` cannot locate `perl5db.pl`; until the precondition existed, such a
//! candidate burned the full pipe-probe budget and surfaced as
//! "no usable debugger session over pipes", which names the wrong repair.
//!
//! Scenarios:
//!
//! - the classification contract is proven directly on synthetic outcomes,
//!   so the typed-vs-generic boundary holds without depending on which
//!   perls a host ships;
//! - a stub interpreter that reproduces the missing-library stderr must
//!   fail [`common::probe_debuggee_perl_for_test`] with the TYPED reason.
//!   The same stub under the pipe probe alone would produce the generic
//!   text, so asserting the typed text proves the precondition runs first;
//! - when a real interpreter is present, requiring `perl5db.pl` succeeds
//!   and the composed probe stays green (skipped on hosts without perl).

mod common;

use common::{classify_debugger_capability, probe_debuggee_perl_for_test};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const MISSING_LIBRARY_STDERR: &str = "Can't locate perl5db.pl in @INC (@INC entries checked: /usr/lib/perl5/site_perl \
     /usr/share/perl5) at -e line 1.";

/// A stub interpreter whose stderr reproduces the missing-debugger-library
/// failure and which exits nonzero, ignoring every argument.
fn write_missing_library_stub(dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    #[cfg(windows)]
    {
        let stub = dir.join("perl-without-debugger.cmd");
        std::fs::write(
            &stub,
            "@echo off\r\n\
             echo Can't locate perl5db.pl in @INC (@INC entries checked:) at -e line 1. 1>&2\r\n\
             exit /b 2\r\n",
        )?;
        Ok(stub)
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let stub = dir.join("perl-without-debugger");
        std::fs::write(
            &stub,
            "#!/bin/sh\necho \"Can't locate perl5db.pl in @INC (@INC entries checked:) \
             at -e line 1.\" >&2\nexit 2\n",
        )?;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))?;
        Ok(stub)
    }
}

/// The typed-vs-generic classification boundary: only the missing-library
/// stderr earns the actionable "cannot host the debugger" reason; every
/// other nonzero outcome stays a generic probe failure; success passes.
#[test]
fn capability_classification_names_missing_debugger_library_typed() {
    let typed = classify_debugger_capability(false, "exit code: 2", MISSING_LIBRARY_STDERR)
        .expect_err("a missing perl5db.pl must classify as a typed incapability");
    assert!(
        typed.reason.contains("cannot host the debugger"),
        "typed reason must name the precondition: {}",
        typed.reason
    );
    assert!(
        typed.reason.contains("perl5db.pl is not loadable"),
        "typed reason must name the missing library: {}",
        typed.reason
    );
    assert!(!typed.transient, "a missing library is deterministic");
}

#[test]
fn capability_classification_keeps_other_failures_generic() {
    let generic = classify_debugger_capability(false, "exit code: 9", "unrelated catastrophe")
        .expect_err("a failing probe must not pass classification");
    assert!(
        generic.reason.starts_with("capability probe failed"),
        "unexpected stderr must stay generic: {}",
        generic.reason
    );
    assert!(
        !generic.reason.contains("cannot host the debugger"),
        "the typed reason must be reserved for the missing-library shape: {}",
        generic.reason
    );
}

#[test]
fn capability_classification_passes_a_successful_load() {
    classify_debugger_capability(true, "exit code: 0", "")
        .expect("a successful perl5db.pl load must pass the precondition");
}

/// End to end: the stub fails resolution with the TYPED precondition text —
/// proving the capability probe runs before (and instead of) the
/// timing-sensitive pipe probe for an interpreter that cannot even load the
/// debugger library.
#[test]
fn resolution_refuses_a_missing_library_interpreter_with_typed_reason() -> Result<(), Box<dyn Error>>
{
    let controls = tempfile::tempdir()?;
    let stub = write_missing_library_stub(controls.path())?;
    let reason = probe_debuggee_perl_for_test(&stub, Duration::from_secs(10), false)
        .expect_err("an interpreter without perl5db.pl must fail resolution");
    assert!(
        reason.contains("cannot host the debugger"),
        "resolution must surface the typed precondition reason, got: {reason}"
    );
    assert!(
        !reason.contains("no usable debugger session over pipes"),
        "the generic pipe-probe text must not replace the typed reason: {reason}"
    );
    Ok(())
}

/// Positive control: when the host ships a real perl, requiring perl5db.pl
/// exits successfully — the exact subprocess shape the precondition runs —
/// and the composed resolution probe accepts the interpreter.
#[test]
#[allow(clippy::print_stderr)]
fn a_capable_host_interpreter_passes_the_capability_precondition() -> Result<(), Box<dyn Error>> {
    let locator = if cfg!(windows) { "where.exe" } else { "which" };
    let output = Command::new(locator).arg("perl").output()?;
    if !output.status.success() {
        eprintln!(
            "SKIP a_capable_host_interpreter_passes_the_capability_precondition: no perl on PATH"
        );
        return Ok(());
    }
    let Some(candidate) = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .find(|path| path.is_file())
    else {
        eprintln!(
            "SKIP a_capable_host_interpreter_passes_the_capability_precondition: no perl file on PATH"
        );
        return Ok(());
    };
    let require = Command::new(&candidate)
        .args(["-e", "require 'perl5db.pl';"])
        .env_remove("PERL5LIB")
        .env_remove("PERL5OPT")
        .env("LC_ALL", "C")
        .output()?;
    assert!(
        require.status.success(),
        "host perl {} could not load perl5db.pl: {}",
        candidate.display(),
        String::from_utf8_lossy(&require.stderr)
    );
    probe_debuggee_perl_for_test(&candidate, Duration::from_secs(10), false)
        .map_err(|reason| format!("pipe-capable host perl was rejected: {reason}"))?;
    Ok(())
}
