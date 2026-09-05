//! Typed skip for symlink-creating tests on Windows sessions that lack
//! `SeCreateSymbolicLinkPrivilege`.
//!
//! Windows only permits file-symlink creation from elevated sessions or on
//! machines with Developer Mode enabled. Every other Windows session fails
//! [`std::os::windows::fs::symlink_file`] with os error 1314 ("A required
//! privilege is not held by the client") — an environment gap, not a product
//! defect. Tests that prove symlink or reparse-point behavior should skip
//! visibly in that case instead of failing red, and must still run in full
//! when the privilege is present.
//!
//! The skip is typed and 1314-only: every other symlink-creation error is a
//! real failure and must be surfaced, never skipped. Substituting a junction
//! or a copy would not exercise reparse-point semantics and is not supported
//! by this helper. The skip reason is written outside the test harness's
//! output capture, so a passing skip stays visible under a default `cargo
//! test` run instead of hiding behind `--show-output`.
//!
//! # Convention
//!
//! Call [`symlink_test_decision`] before creating any symlink fixture, and
//! return early when it skips visibly:
//!
//! ```no_run
//! # fn test_body() -> Result<(), Box<dyn std::error::Error>> {
//! if perl_tdd_support::symlink_test_decision().skip_visibly() {
//!     return Ok(());
//! }
//! // Full test body: create the symlink and prove the real semantics.
//! // A non-1314 creation error still fails the test honestly.
//! # Ok(())
//! # }
//! ```
//!
//! For tests that create the symlink mid-body, classify the actual creation
//! error instead of probing:
//!
//! ```no_run
//! # fn test_body() -> Result<(), Box<dyn std::error::Error>> {
//! # let target = std::path::Path::new("target");
//! # let link = std::path::Path::new("link");
//! #[cfg(windows)]
//! if let Err(error) = std::os::windows::fs::symlink_file(target, link) {
//!     if perl_tdd_support::classify_symlink_error(&error).skip_visibly() {
//!         return Ok(());
//!     }
//!     return Err(error.into());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Enabling Windows Developer Mode (or running from an elevated session)
//! grants the privilege and opts the machine out of every skip, so the
//! affected tests execute in full.

use std::io;
use std::sync::OnceLock;

/// Windows os error 1314: "A required privilege is not held by the client."
///
/// Returned by symlink creation when the session lacks
/// `SeCreateSymbolicLinkPrivilege` (no Developer Mode, no elevation).
pub const SYMLINK_PRIVILEGE_NOT_HELD: i32 = 1314;

/// Whether a symlink-creating test may run on this machine.
///
/// Produced by [`classify_symlink_error`] (from an actual creation error) and
/// [`symlink_test_decision`] (from a once-per-process capability probe).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymlinkTestDecision {
    /// Run the test in full. On non-Windows platforms this is always the
    /// answer; on Windows it means symlink creation is available.
    Run,
    /// Skip the test with a visible reason. Produced only for a Windows os
    /// error 1314 — never for any other symlink-creation failure.
    TypedSkip {
        /// Human-readable explanation of the skip, including how to opt out.
        reason: String,
    },
}

impl SymlinkTestDecision {
    /// The visible skip reason, or `None` when the test must run.
    pub fn skip_reason(&self) -> Option<&str> {
        match self {
            SymlinkTestDecision::Run => None,
            SymlinkTestDecision::TypedSkip { reason } => Some(reason),
        }
    }

    /// Skip the current test visibly.
    ///
    /// Writes `SKIPPED: <reason>` outside the default test-harness output
    /// capture (libtest hides `eprintln!` output of passing tests unless
    /// `--nocapture`/`--show-output` is passed, which would make the skip
    /// silent): to the real terminal when stderr is live, following the
    /// redirection otherwise. Returns `true` when this decision is a
    /// [`SymlinkTestDecision::TypedSkip`]; `false` when the test must run in
    /// full.
    pub fn skip_visibly(&self) -> bool {
        match self.skip_reason() {
            Some(reason) => {
                write_uncaptured_stderr(&format!("SKIPPED: {reason}\n"));
                true
            }
            None => false,
        }
    }
}

/// Write `message` to the process's real stderr, outside libtest's capture.
///
/// The default test harness captures `eprintln!`/`print!` output of passing
/// tests and only shows it with `--nocapture` or `--show-output`, which would
/// make a passing typed skip indistinguishable from a fully exercised test.
/// Reopening the real stderr sink (`/dev/stderr` on Unix, the console on
/// Windows when stderr is the terminal) bypasses that capture because the
/// write never goes through the harness's thread-local output redirection.
/// When stderr is redirected (or the direct sink cannot be opened, e.g. a
/// headless session), fall back to `eprint!` so the reason still follows the
/// redirection and remains available in captured output.
fn write_uncaptured_stderr(message: &str) {
    #[cfg(unix)]
    {
        use std::io::Write;

        // Append mode: when stderr is redirected to a regular file, a
        // write-only reopen would truncate it.
        if let Ok(mut stderr) = std::fs::OpenOptions::new().append(true).open("/dev/stderr") {
            let _ = stderr.write_all(message.as_bytes());
            return;
        }
    }
    #[cfg(windows)]
    {
        use std::io::IsTerminal;
        use std::io::Write;

        // CONOUT$ is the console screen buffer: the place a developer running
        // `cargo test` is actually looking. Opening it directly bypasses the
        // harness capture even though stderr itself is captured. Only take it
        // when stderr is the terminal; when stderr is redirected, the skip
        // reason belongs in the redirection, not force-printed to the console.
        if std::io::stderr().is_terminal()
            && let Ok(mut console) = std::fs::OpenOptions::new().write(true).open("CONOUT$")
        {
            let _ = console.write_all(message.as_bytes());
            return;
        }
    }
    eprint!("{message}");
}

/// Classify a symlink-creation error.
///
/// Only Windows os error 1314 yields a [`SymlinkTestDecision::TypedSkip`].
/// Every other error — and every error on a non-Windows platform — yields
/// [`SymlinkTestDecision::Run`], meaning the skip does not apply and the
/// caller must surface the failure.
pub fn classify_symlink_error(error: &io::Error) -> SymlinkTestDecision {
    classify_for_platform(cfg!(windows), error)
}

fn classify_for_platform(is_windows: bool, error: &io::Error) -> SymlinkTestDecision {
    if is_windows && error.raw_os_error() == Some(SYMLINK_PRIVILEGE_NOT_HELD) {
        return SymlinkTestDecision::TypedSkip {
            reason: format!(
                "symlink test: this Windows session lacks \
                 SeCreateSymbolicLinkPrivilege (os error {SYMLINK_PRIVILEGE_NOT_HELD}); \
                 enable Developer Mode or use an elevated session to run it in full"
            ),
        };
    }
    SymlinkTestDecision::Run
}

/// Decide once per process whether symlink-creating tests may run.
///
/// Probes the machine by creating and removing a temporary file symlink. When
/// creation fails with Windows os error 1314 the decision is a typed skip;
/// any other probe failure yields [`SymlinkTestDecision::Run`] so the caller's
/// own symlink creation surfaces the real error honestly.
pub fn symlink_test_decision() -> SymlinkTestDecision {
    static DECISION: OnceLock<SymlinkTestDecision> = OnceLock::new();
    DECISION.get_or_init(probe_symlink_capability).clone()
}

#[cfg(windows)]
fn probe_symlink_capability() -> SymlinkTestDecision {
    use std::os::windows::fs::symlink_file;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nonce =
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos());
    let target = std::env::temp_dir()
        .join(format!("perl-tdd-support-symlink-probe-{}-{nonce}.target", std::process::id()));
    let link = std::env::temp_dir()
        .join(format!("perl-tdd-support-symlink-probe-{}-{nonce}.link", std::process::id()));

    let decision = if std::fs::write(&target, b"symlink privilege probe").is_ok() {
        match symlink_file(&target, &link) {
            Ok(()) => SymlinkTestDecision::Run,
            Err(error) => classify_symlink_error(&error),
        }
    } else {
        // The probe itself could not stage its fixture; let the caller's own
        // symlink creation report the environment problem.
        SymlinkTestDecision::Run
    };
    let _ = std::fs::remove_file(&link);
    let _ = std::fs::remove_file(&target);
    decision
}

#[cfg(not(windows))]
fn probe_symlink_capability() -> SymlinkTestDecision {
    // Symlink creation is not privilege-gated on this platform.
    SymlinkTestDecision::Run
}

#[cfg(test)]
mod tests {
    use super::{SymlinkTestDecision, classify_for_platform};
    use perl_test_must::must_some;
    use std::io;

    #[test]
    fn decision_is_typed_skip_only_for_windows_error_1314() {
        let privilege_missing = io::Error::from_raw_os_error(super::SYMLINK_PRIVILEGE_NOT_HELD);
        assert!(matches!(
            classify_for_platform(true, &privilege_missing),
            SymlinkTestDecision::TypedSkip { .. }
        ));
        // Any other raw error stays a real failure on Windows.
        assert_eq!(
            classify_for_platform(true, &io::Error::from_raw_os_error(5)),
            SymlinkTestDecision::Run
        );
        // Errors without a raw os error code (e.g. wrapped std failures) never skip.
        assert_eq!(
            classify_for_platform(true, &io::Error::other("creation failed")),
            SymlinkTestDecision::Run
        );
        // On non-Windows platforms errno 1314 is an unrelated code, never a
        // Windows privilege skip.
        assert_eq!(classify_for_platform(false, &privilege_missing), SymlinkTestDecision::Run);
    }

    #[test]
    fn typed_skip_reason_names_the_privilege_and_the_opt_out() {
        let decision = classify_for_platform(
            true,
            &io::Error::from_raw_os_error(super::SYMLINK_PRIVILEGE_NOT_HELD),
        );
        let reason = must_some(decision.skip_reason());
        assert!(reason.contains("SeCreateSymbolicLinkPrivilege"));
        assert!(reason.contains("1314"));
        assert!(reason.contains("Developer Mode"));
        assert!(SymlinkTestDecision::Run.skip_reason().is_none());
    }

    #[test]
    fn skip_visibly_reports_only_typed_skips() {
        assert!(!SymlinkTestDecision::Run.skip_visibly());
        let decision = classify_for_platform(
            true,
            &io::Error::from_raw_os_error(super::SYMLINK_PRIVILEGE_NOT_HELD),
        );
        // Exercises the visible stderr path once; a skip must never be silent.
        assert!(decision.skip_visibly());
    }

    #[test]
    #[cfg(not(windows))]
    fn probe_never_skips_off_windows() {
        assert_eq!(super::symlink_test_decision(), SymlinkTestDecision::Run);
    }
}
