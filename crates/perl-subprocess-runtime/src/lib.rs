//! Subprocess execution abstraction for provider purity
//!
//! This crate provides a trait-based abstraction for subprocess execution,
//! enabling testing with mock implementations and WASM compatibility.
//!
//! # Two seams, one of which is contained
//!
//! [`process`] is the supervised process domain: a versioned
//! [`process::ProcessPlan`], a pure validator, an ordered event stream, a
//! terminal [`process::ProcessResult`], and the supervisor ports a consumer
//! depends on. It carries the exact identity, authorization, bounds,
//! cancellation, and cleanup contracts a caller needs, and it performs no
//! operating-system spawn: the only supervisor it ships is a deterministic
//! fake for consumer tests.
//!
//! [`SubprocessRuntime`] and [`OsSubprocessRuntime`] are the pre-domain seam.
//! They remain because live consumers compile against them; they are not a
//! second production process authority, they are not open to new consumers,
//! and [`process::legacy`] records what they cannot express and who owns
//! removing them.

#![deny(unsafe_code)]
#![cfg_attr(test, allow(clippy::panic, clippy::unwrap_used, clippy::expect_used))]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]

#[cfg(not(target_arch = "wasm32"))]
mod availability;
mod error;
#[cfg(not(target_arch = "wasm32"))]
mod os_runtime;
mod output;

/// Mock subprocess runtime implementations for tests.
pub mod mock;
pub mod process;

#[cfg(not(target_arch = "wasm32"))]
pub use availability::command_exists;
pub use error::SubprocessError;
#[cfg(not(target_arch = "wasm32"))]
pub use os_runtime::OsSubprocessRuntime;
pub use output::SubprocessOutput;

/// Abstraction trait for subprocess execution.
///
/// # Contained legacy seam
///
/// This is the pre-domain seam. It cannot express an exact working directory,
/// an environment projection, execution authorization, output budgets,
/// cancellation, process-tree cleanup, or terminal-cause precedence — see
/// [`process::legacy::LEGACY_UNSUPPORTED_CAPABILITIES`]. New consumers use
/// [`process::ProcessSupervisor`]; removal of this seam is owned by `#1975`.
pub trait SubprocessRuntime: Send + Sync {
    /// Execute a command with the given arguments and optional stdin.
    fn run_command(
        &self,
        program: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> Result<SubprocessOutput, SubprocessError>;
}

/// Resolve a bare program name to an absolute path that is safe to pass to
/// `std::process::Command::new`.
///
/// On Windows, `Command::new(bare_name)` triggers CreateProcess's CWD-first
/// executable search — a binary planted in the LSP workspace root would run
/// instead of the legitimate tool (binary-planting RCE, #2764/#3028).  This
/// function delegates to the already-hardened `resolve_windows_program` resolver
/// which searches only absolute `PATH` directories and excludes the CWD.
///
/// # Return value
///
/// - **Windows**: returns the absolute path of the resolved binary, or
///   `Err(SubprocessError)` if the program is not found in any absolute `PATH`
///   directory (fail closed — refusing to run is safer than running a planted
///   binary).
/// - **Non-Windows**: CreateProcess CWD-search semantics do not apply; returns
///   `Ok(program.to_string())` unchanged so callers can use this function
///   unconditionally without `#[cfg(windows)]` guards.
///
/// # Usage
///
/// ```ignore
/// let resolved = perl_subprocess_runtime::resolve_program("yath")?;
/// let mut cmd = std::process::Command::new(resolved);
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub fn resolve_program(program: &str) -> Result<String, SubprocessError> {
    #[cfg(windows)]
    {
        os_runtime::resolve_windows_program_pub(program).ok_or_else(|| {
            SubprocessError::new(format!(
                "command not found in any absolute PATH directory \
                 (current directory excluded for security): {program}"
            ))
        })
    }
    #[cfg(not(windows))]
    {
        Ok(program.to_string())
    }
}

#[cfg(test)]
mod tests;
