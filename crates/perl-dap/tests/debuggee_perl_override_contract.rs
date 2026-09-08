//! Discriminating proof for the [`DEBUGGEE_PERL_OVERRIDE_ENV`] availability
//! seam (#12594 repair r2, finding 1).
//!
//! Prior behavior under repair: [`common::perl_available`] answered from a
//! PATH-only `perl --version` probe and returned before the debuggee
//! resolution ever consulted the pinned interpreter. On any host with `perl`
//! on `PATH`, a broken/expired pin therefore flipped every consuming gate
//! (scorecard launch harness among them) back to "available" while the pin
//! named the only candidate live sessions were allowed to use.
//!
//! These scenarios deliberately pin a NONEXISTENT absolute path so the
//! asserted outcome is host-independent:
//!
//! - with the pin set, `perl_available` must be `false` whether or not
//!   `perl` exists on `PATH` (pre-repair this returned `true` whenever the
//!   PATH oracle succeeded);
//! - strict mode (`PERL_LSP_DAP_REQUIRE_PERL=1`) must reject a rejected pin
//!   by name, not claim a PATH absence;
//! - without the pin, the PATH-only answer is unchanged.
//!
//! The resolver caches one negative result per process ([`OnceLock`]), which
//! is why every scenario below pins a broken interpreter and this proof runs
//! as its own test binary: mutating process-wide env/state here cannot race
//! the rest of the DAP suites.

#![allow(unsafe_code)] // required for std::env::set_var/remove_var in Rust 2024 (unsafe fn)

mod common;

use common::{
    DEBUGGEE_PERL_OVERRIDE_ENV, DapWorkflowSession, REQUIRE_PERL_ENV, perl_available,
    workflow_timeout,
};
use serial_test::serial;
use std::error::Error;
use std::fs;
use std::panic;
use std::process::Command;

/// Nonexistent interpreter path — probing it fails deterministically on every
/// platform (spawn error), so assertions never depend on which perls a host
/// happens to ship.
const BOGUS_PIN: &str = "/definitely/not/a/real/perl-lsp-probe-pin";

/// Save/restore the process environment keys these scenarios mutate, so a
/// panic inside one block cannot poison the next scenario or leave state
/// behind for whichever binary runs after this one.
struct EnvGuard {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl EnvGuard {
    fn capture(keys: &'static [&'static str]) -> Self {
        let saved = keys.iter().map(|key| (*key, std::env::var_os(key))).collect();
        Self { saved }
    }

    fn remove(key: &str) {
        unsafe { std::env::remove_var(key) };
    }

    fn set(key: &str, value: &str) {
        unsafe { std::env::set_var(key, value) };
    }

    fn set_os(key: &str, value: &std::ffi::OsStr) {
        unsafe { std::env::set_var(key, value) };
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(value) => unsafe { std::env::set_var(key, value) },
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }
}

const GUARDED_KEYS: &[&str] = &[DEBUGGEE_PERL_OVERRIDE_ENV, REQUIRE_PERL_ENV, "PATH"];

fn fake_path_perl() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("fake_perl.rs");
    let binary = directory.path().join(if cfg!(windows) { "perl.exe" } else { "perl" });
    fs::write(&source, "fn main() { println!(\"This is perl 5, fake PATH oracle\"); }\n")?;
    let compile = Command::new("rustc")
        .args(["--edition", "2024"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()?;
    if !compile.status.success() {
        return Err(format!(
            "failed to compile fake PATH perl: {}",
            String::from_utf8_lossy(&compile.stderr)
        )
        .into());
    }
    Ok(directory)
}

#[test]
#[serial(dap_debuggee_environment)]
fn debuggee_pin_is_the_single_availability_source_of_truth() -> Result<(), Box<dyn Error>> {
    let fake_path_perl = fake_path_perl()?;

    // ── Scenario A: pin set ⇒ PATH oracle is never the answer ──────────────
    //
    // Pre-repair discrimination: with `perl` on PATH, `perl_available()`
    // answered `true` before the pin was consulted (tests/common/mod.rs
    // `PerlOracleEnv::for_dap_test_fixture()` early return), so a gate like
    // the scorecard's `if !perl_available()` launched straight past a pin
    // that names the only usable interpreter. Post-repair the pin is
    // probed exclusively and its rejection makes availability `false`.
    {
        let _guard = EnvGuard::capture(GUARDED_KEYS);
        EnvGuard::set_os("PATH", fake_path_perl.path().as_os_str());
        EnvGuard::set(DEBUGGEE_PERL_OVERRIDE_ENV, BOGUS_PIN);
        if perl_available() {
            return Err(format!(
                "a rejected {DEBUGGEE_PERL_OVERRIDE_ENV} pin must make \
                 perl_available() false even when `perl` exists on PATH"
            )
            .into());
        }

        // The scorecard consumption pattern is exactly
        // `if !perl_available() { SKIP } … else debuggee_perl_or_typed_skip()`;
        // availability and session resolution must agree on the rejected pin
        // so the two gates can never disagree about skipping.
        if common::resolve_debuggee_perl().is_some() {
            return Err(
                "availability gate and live-session resolution must agree: both must reject a failed pin"
                    .into(),
            );
        }
    }

    // ── Scenario B: strict mode rejects a broken pin by name ────────────────
    //
    // Pre-repair, strict mode never fired on hosts with PATH perl because the
    // early return answered `true`; a silently broken pin vacuously passed
    // PERL_LSP_DAP_REQUIRE_PERL=1 — the exact vacuous-green class the strict
    // flag was introduced to forbid. Post-repair the panic must identify the
    // pin and its probe diagnostics instead of blaming PATH.
    {
        let _guard = EnvGuard::capture(GUARDED_KEYS);
        EnvGuard::set(DEBUGGEE_PERL_OVERRIDE_ENV, BOGUS_PIN);
        EnvGuard::set(REQUIRE_PERL_ENV, "1");

        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));
        let verdict = panic::catch_unwind(panic::AssertUnwindSafe(perl_available));
        panic::set_hook(prev_hook);

        let payload_text = match verdict.as_ref().err() {
            Some(payload) => payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "<non-string panic>".to_string()),
            None => "<no panic: strict mode accepted a rejected pin>".to_string(),
        };
        if verdict.is_ok() {
            return Err(format!(
                "{REQUIRE_PERL_ENV}=1 must hard-fail when the \
                 {DEBUGGEE_PERL_OVERRIDE_ENV} pin fails its probe; got silent acceptance"
            )
            .into());
        }
        let launch_error = common::resolve_launch_perl_path()
            .err()
            .ok_or("a rejected pin must not fall back to ambient launch resolution")?;
        if !launch_error.contains(DEBUGGEE_PERL_OVERRIDE_ENV) {
            return Err(format!(
                "launch failure must identify the rejected pin, got: {launch_error}"
            )
            .into());
        }
        // The repaired diagnostic names the pinned interpreter and its probe
        // failure; blaming PATH would mean the early return never consulted
        // the pin.
        if !payload_text.contains(BOGUS_PIN) {
            return Err(format!(
                "strict failure must diagnose the rejected pin path {BOGUS_PIN}, \
                 not PATH absence, got: {payload_text}"
            )
            .into());
        }
    }

    // ── Scenario C: no pin ⇒ unchanged PATH-only semantics ─────────────────
    //
    // Availability parity with the PATH oracle keeps the unpinned fast path
    // untouched (cheap `perl --version`, no debuggee probe cascade).
    {
        let _guard = EnvGuard::capture(GUARDED_KEYS);
        EnvGuard::remove(DEBUGGEE_PERL_OVERRIDE_ENV);
        EnvGuard::remove(REQUIRE_PERL_ENV);

        // Replace PATH with a deterministic native helper, independently of
        // what Perl this host ships; without a pin, availability must follow
        // this positive PATH oracle.
        EnvGuard::set_os("PATH", fake_path_perl.path().as_os_str());
        if !perl_available() {
            return Err(
                "without a pin, availability must follow the deterministic positive PATH oracle control"
                    .into(),
            );
        }
    }

    Ok(())
}

#[test]
#[serial(dap_debuggee_environment)]
fn attach_does_not_resolve_launch_pin_during_initialization() -> Result<(), Box<dyn Error>> {
    let _guard = EnvGuard::capture(GUARDED_KEYS);
    EnvGuard::set(DEBUGGEE_PERL_OVERRIDE_ENV, BOGUS_PIN);

    // Attach is independent of launch-interpreter selection. A rejected launch
    // pin must therefore not prevent initialize/attach from reaching the real
    // adapter; resolution belongs to the first launch helper that needs it.
    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.attach(std::process::id(), false)?;
    let stopped = session.wait_stopped()?;
    if stopped.reason != "attach" {
        return Err(format!("attach must stop with reason=attach, got {}", stopped.reason).into());
    }
    let launch_error = common::resolve_launch_perl_path()
        .err()
        .ok_or("a rejected pin must remain rejected for a deferred launch")?;
    if !launch_error.contains(DEBUGGEE_PERL_OVERRIDE_ENV) || !launch_error.contains(BOGUS_PIN) {
        return Err(std::io::Error::other(format!(
            "deferred launch resolution must retain the rejected pin, got: {launch_error}"
        ))
        .into());
    }
    session.disconnect()?;
    Ok(())
}
