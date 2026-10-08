//! OS-backed subprocess runtime.

mod cmd_quote;
mod invocation;
mod path_selection;
mod process;
mod validation;
#[cfg(windows)]
mod windows;

pub(crate) use invocation::resolve_command_invocation;
use process::run_os_command;

use crate::{SubprocessError, SubprocessOutput, SubprocessRuntime};

// `select_path_candidate` and `candidate_priority` are cross-platform so they
// are exported for test use on all platforms — not just Windows.  This lets
// the ripr quality gate observe call paths on Linux CI runners.
#[cfg(test)]
pub(crate) use path_selection::candidate_priority;

// `select_path_candidate` is the admission authority on every platform, but it
// is reached differently: on Windows through `windows::resolve_windows_program`
// (which imports it directly from `path_selection`), and on other platforms
// through `crate::availability`.  Both apply the same absolute-only +
// CWD-exclusion invariant, so neither platform grows a second policy.
//
// The re-export is therefore needed for the non-Windows production route and
// for the cross-platform tests, but would be unused on a Windows build.
#[cfg(any(test, not(windows)))]
pub(crate) use path_selection::select_path_candidate;

// `windows_program_priority` is the historical name for `candidate_priority`
// used in Windows-specific tests.  Export it as an alias on Windows so tests
// that use the old name continue to compile.
#[cfg(all(windows, test))]
pub(crate) use path_selection::candidate_priority as windows_program_priority;

#[cfg(all(windows, test))]
pub(crate) use windows::resolve_cmd_exe;

/// Re-export of [`windows::resolve_windows_program`] for use by
/// [`crate::resolve_program`].  The inner function is `pub(crate)`; this
/// wrapper lifts it to `pub(super)` so `lib.rs` can call it without making
/// the Windows-specific internals part of the public API.
#[cfg(windows)]
pub(super) fn resolve_windows_program_pub(program: &str) -> Option<String> {
    windows::resolve_windows_program(program)
}

const MIN_TIMEOUT_SECS: u64 = 1;

/// Default implementation using `std::process::Command`.
pub struct OsSubprocessRuntime {
    timeout_secs: Option<u64>,
}

impl OsSubprocessRuntime {
    /// Create a new OS subprocess runtime with no timeout.
    pub fn new() -> Self {
        Self { timeout_secs: None }
    }

    /// Create a new OS subprocess runtime with the given wall-clock timeout.
    ///
    /// A zero value is normalized to one second so direct construction cannot
    /// panic. This generic constructor deliberately does not impose a product-
    /// specific maximum; callers with a bounded interactive contract should use
    /// [`Self::with_bounded_timeout`].
    ///
    /// If the subprocess does not complete within the normalized timeout the
    /// call returns a `SubprocessError` with a "timed out" message and attempts
    /// to terminate the spawned process before returning.
    ///
    /// # Stdin size caveat
    ///
    /// Stdin data is written synchronously before the timeout poll loop begins.
    /// If the subprocess hangs before consuming stdin and the data exceeds the
    /// OS pipe buffer (~64 KiB on Linux), `run_command` will block in the write
    /// phase and the timeout will not fire. For typical Perl source files this
    /// is not a concern.
    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self { timeout_secs: Some(timeout_secs.max(MIN_TIMEOUT_SECS)) }
    }

    /// Create a runtime using a caller-owned bounded timeout envelope.
    ///
    /// Both zero inputs are normalized to one second. The requested timeout is
    /// then clamped to the normalized maximum, allowing each product surface to
    /// define its own upper bound without changing unrelated subprocess users.
    pub fn with_bounded_timeout(timeout_secs: u64, max_timeout_secs: u64) -> Self {
        let max_timeout_secs = max_timeout_secs.max(MIN_TIMEOUT_SECS);
        Self { timeout_secs: Some(timeout_secs.clamp(MIN_TIMEOUT_SECS, max_timeout_secs)) }
    }
}

impl Default for OsSubprocessRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl SubprocessRuntime for OsSubprocessRuntime {
    fn run_command(
        &self,
        program: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> Result<SubprocessOutput, SubprocessError> {
        run_os_command(program, args, stdin, self.timeout_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::thread;

    #[test]
    fn generic_timeout_normalizes_zero_without_changing_large_valid_values() {
        assert_eq!(OsSubprocessRuntime::with_timeout(0).timeout_secs, Some(1));
        assert_eq!(OsSubprocessRuntime::with_timeout(1).timeout_secs, Some(1));
        assert_eq!(OsSubprocessRuntime::with_timeout(10).timeout_secs, Some(10));
        assert_eq!(OsSubprocessRuntime::with_timeout(u64::MAX).timeout_secs, Some(u64::MAX));
    }

    #[test]
    fn bounded_timeout_uses_the_callers_envelope() {
        assert_eq!(OsSubprocessRuntime::with_bounded_timeout(0, 300).timeout_secs, Some(1));
        assert_eq!(OsSubprocessRuntime::with_bounded_timeout(1, 300).timeout_secs, Some(1));
        assert_eq!(OsSubprocessRuntime::with_bounded_timeout(300, 300).timeout_secs, Some(300));
        assert_eq!(OsSubprocessRuntime::with_bounded_timeout(301, 300).timeout_secs, Some(300));
        assert_eq!(
            OsSubprocessRuntime::with_bounded_timeout(u64::MAX, 300).timeout_secs,
            Some(300)
        );
        assert_eq!(OsSubprocessRuntime::with_bounded_timeout(5, 0).timeout_secs, Some(1));
    }

    /// #17305 regression guard: with no caller-supplied stdin, the child sees
    /// EOF on stdin instead of inheriting the parent's stdin.
    ///
    /// The child drains its whole stdin before printing. Under the fixed
    /// `spawn_child`, stdin is the null device, so the read ends immediately.
    /// If the null-stdin pin ever regresses to `inherit`, the child blocks on
    /// whatever handle it inherited and the deadline reports the timeout as a
    /// failure instead of a hang.
    ///
    /// The child completes on *any* EOF, so running it directly in this test
    /// process is not discriminating: when the ambient stdin of `cargo test`
    /// is closed or already at EOF (CI runners, piped invocations), a
    /// regressed `inherit` child reads that EOF immediately and the guard
    /// passes silently (#17317). The assertion therefore runs in a helper
    /// re-invocation of this test binary whose stdin is a pipe this process
    /// holds open — never at EOF — for the helper's whole lifetime, so a
    /// regressed grandchild can only block on that pipe, and the deadline
    /// turns the block into a deterministic failure.
    #[test]
    fn a_child_without_supplied_stdin_reads_eof_not_the_parent_transport() {
        const HELPER_MODE_ENV: &str = "PLRS_STDIN_EOF_GUARD_HELPER";

        // Helper re-invocation: this process's own stdin is the pipe the
        // spawning parent holds open below, so run the child-spawn assertion
        // against that deterministic environment.
        if env::var_os(HELPER_MODE_ENV).is_some() {
            assert_stdin_reader_child_observes_eof();
            return;
        }

        // libtest names tests by the module path relative to the crate root,
        // while `module_path!()` includes the crate itself; strip that first
        // segment so the `--exact` filter matches the helper's own test.
        let module_path = module_path!().split_once("::").map_or(module_path!(), |(_, rest)| rest);
        let test_name = format!(
            "{module_path}::a_child_without_supplied_stdin_reads_eof_not_the_parent_transport"
        );

        let mut helper = Command::new(
            env::current_exe().expect("test binary path must resolve for stdin-EOF helper"),
        )
        .args(["--exact", test_name.as_str(), "--nocapture"])
        .env(HELPER_MODE_ENV, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("stdin-EOF helper must spawn");

        // Deliberately hold the helper's stdin write end open for its whole
        // lifetime: dropping it early would hand the helper an already-EOF'd
        // stdin — the exact ambient condition that lets a regressed `inherit`
        // pin pass silently (#17317). This binding must outlive `wait`.
        let held_helper_stdin = helper.stdin.take().expect("helper stdin must be piped");

        let mut helper_stdout = helper.stdout.take().expect("helper stdout must be piped");
        let mut helper_stderr = helper.stderr.take().expect("helper stderr must be piped");
        // Drain both pipes concurrently: a helper that fills its stderr pipe —
        // for example a failing guard echoing output under RUST_BACKTRACE=full —
        // would otherwise block on its next stderr write while this parent still
        // waits for stdout EOF, hanging the guard instead of reporting the
        // regression failure it exists to surface.
        let stderr_drainer = thread::spawn(move || {
            let mut stderr_bytes = Vec::new();
            helper_stderr.read_to_end(&mut stderr_bytes).expect("helper stderr must drain");
            stderr_bytes
        });
        let mut stdout_bytes = Vec::new();
        helper_stdout.read_to_end(&mut stdout_bytes).expect("helper stdout must drain");
        let stderr_bytes = stderr_drainer.join().expect("helper stderr drainer must join");

        let status = helper.wait().expect("stdin-EOF helper must be waitable");
        drop(held_helper_stdin);

        let stdout = String::from_utf8_lossy(&stdout_bytes).into_owned();
        let stderr = String::from_utf8_lossy(&stderr_bytes).into_owned();
        assert!(
            status.success() && stdout.contains("test result: ok. 1 passed;"),
            "stdin-EOF helper must run exactly this test and pass\n\
             status: {status}\nstdout: {stdout}\nstderr: {stderr}"
        );
    }

    /// The original child-spawn assertion: a stdin-draining child must observe
    /// EOF and print `stdin-eof`, never block on an inherited handle. Kept
    /// verbatim from the direct-process form of the guard so the failure mode
    /// under regression remains "timeout reported as failure".
    fn assert_stdin_reader_child_observes_eof() {
        let runtime = OsSubprocessRuntime::with_timeout(10);

        #[cfg(windows)]
        let (program, args): (&str, &[&str]) = (
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "$null = [Console]::In.ReadToEnd(); [Console]::Out.Write('stdin-eof')",
            ],
        );

        #[cfg(not(windows))]
        let (program, args): (&str, &[&str]) = ("sh", &["-c", "cat > /dev/null; printf stdin-eof"]);

        let output = runtime
            .run_command(program, args, None)
            .expect("stdin-reader child must spawn and complete");

        assert!(output.success(), "stdin-reader child must exit successfully");
        assert_eq!(
            output.stdout_lossy(),
            "stdin-eof",
            "child must have observed EOF on stdin, not blocked on an inherited handle"
        );
    }
}
