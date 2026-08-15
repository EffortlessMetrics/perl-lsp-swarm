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
const MAX_TIMEOUT_SECS: u64 = 300;

fn bounded_timeout_secs(timeout_secs: u64) -> u64 {
    timeout_secs.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
}

/// Default implementation using `std::process::Command`.
pub struct OsSubprocessRuntime {
    timeout_secs: Option<u64>,
}

impl OsSubprocessRuntime {
    /// Create a new OS subprocess runtime with no timeout.
    pub fn new() -> Self {
        Self { timeout_secs: None }
    }

    /// Create a new OS subprocess runtime with a bounded wall-clock timeout.
    ///
    /// The runtime accepts only the interactive subprocess envelope of 1 to
    /// 300 seconds. A zero value is normalized to one second and a value above
    /// 300 seconds is normalized to 300 seconds. This constructor is therefore
    /// safe for defense-in-depth use even when an upstream caller has not yet
    /// validated externally supplied configuration.
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
        Self { timeout_secs: Some(bounded_timeout_secs(timeout_secs)) }
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
    fn timeout_envelope_normalizes_zero_and_preserves_reasonable_values() {
        assert_eq!(OsSubprocessRuntime::with_timeout(0).timeout_secs, Some(1));
        assert_eq!(OsSubprocessRuntime::with_timeout(1).timeout_secs, Some(1));
        assert_eq!(OsSubprocessRuntime::with_timeout(10).timeout_secs, Some(10));
        assert_eq!(OsSubprocessRuntime::with_timeout(300).timeout_secs, Some(300));
    }

    #[test]
    fn timeout_envelope_bounds_values_above_the_product_maximum() {
        assert_eq!(OsSubprocessRuntime::with_timeout(301).timeout_secs, Some(300));
        assert_eq!(OsSubprocessRuntime::with_timeout(u64::MAX).timeout_secs, Some(300));
    }
}
