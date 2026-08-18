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
pub(crate) use path_selection::{candidate_priority, select_path_candidate};

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
}
