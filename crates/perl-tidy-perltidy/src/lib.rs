//! Explicit external Perl::Tidy text formatting.
//!
//! This package extracts the two existing `perltidy` text consumers behind
//! one executor. It has no native formatter, parser, editor positions, file
//! editing, profile discovery or cache. Callers own selection and admission.
//!
//! [`LegacyPerlTidyExecutor`] contains the pre-domain runtime seam during the
//! process-supervisor migration. It does not establish authorization, bounded
//! output, cancellation or process-tree cleanup. Those guarantees require the
//! shared process owner; moving this adapter does not supply them.
#![deny(unsafe_code)]
#![warn(missing_docs)]
#![deny(clippy::map_err_ignore)]

mod config;
pub use config::ExternalPerlTidyConfig;

use perl_subprocess_runtime::SubprocessRuntime;

/// Failure from one explicit external text-formatting invocation.
#[derive(Debug, thiserror::Error)]
pub enum ExternalPerlTidyError {
    /// The injected runtime could not complete the invocation.
    #[error("{message}")]
    Runtime {
        /// Original runtime message, preserved for compatibility callers.
        message: String,
    },
    /// A completed command wrapper reported an unavailable target.
    ///
    /// Status 9009 and the retained Windows message are compatibility
    /// heuristics, not authenticated proof of process launch provenance.
    #[error("Perltidy failed: {stderr}")]
    CommandNotFound {
        /// Completed wrapper status.
        status_code: i32,
        /// Original stderr text with lossy decoding, as in the former adapters.
        stderr: String,
    },
    /// The tool returned a non-success status unrelated to the missing-command heuristic.
    #[error("Perltidy failed: {stderr}")]
    Rejected {
        /// Completed process status.
        status_code: i32,
        /// Original stderr text with lossy decoding, as in the former adapters.
        stderr: String,
    },
    /// A successful process returned bytes that are not UTF-8 source text.
    #[error("Invalid UTF-8 from perltidy: {source}")]
    InvalidUtf8 {
        /// Invalid stdout, retained by the standard decoding error.
        #[source]
        source: std::string::FromUtf8Error,
    },
}

/// The single text executor for the two pre-existing Perl::Tidy adapters.
///
/// The application supplies the exact executable, ordered argv and runtime.
/// Construction performs no discovery or spawn. This deliberately named
/// legacy seam must be replaced by the shared `ProcessSupervisor` route once
/// its OS implementation and lifecycle conformance are available.
///
/// `timeout_secs` in [`ExternalPerlTidyConfig`] is an input for the caller that
/// constructs the legacy runtime. This executor cannot change that runtime's
/// timeout, environment, working directory or process-tree policy.
pub struct LegacyPerlTidyExecutor<'a> {
    runtime: &'a dyn SubprocessRuntime,
}

impl<'a> LegacyPerlTidyExecutor<'a> {
    /// Borrow an already selected runtime without acquiring external state.
    #[must_use]
    pub fn new(runtime: &'a dyn SubprocessRuntime) -> Self {
        Self { runtime }
    }

    /// Send exact source bytes on stdin and return successful UTF-8 stdout.
    ///
    /// Arguments are passed in order without shell interpolation or implicit
    /// fallback. The caller retains its existing `-st` / `-se` contract and
    /// must admit executable, profile and arbitrary arguments before calling.
    pub fn format(
        &self,
        program: &str,
        args: &[String],
        source: &str,
    ) -> Result<String, ExternalPerlTidyError> {
        let args = args.iter().map(String::as_str).collect::<Vec<_>>();
        let output = self
            .runtime
            .run_command(program, &args, Some(source.as_bytes()))
            .map_err(|error| ExternalPerlTidyError::Runtime { message: error.message })?;
        if !output.success() {
            let stderr = output.stderr_lossy();
            return Err(if command_launch_failed(output.status_code, &stderr) {
                ExternalPerlTidyError::CommandNotFound { status_code: output.status_code, stderr }
            } else {
                ExternalPerlTidyError::Rejected { status_code: output.status_code, stderr }
            });
        }
        String::from_utf8(output.stdout)
            .map_err(|source| ExternalPerlTidyError::InvalidUtf8 { source })
    }
}

fn command_launch_failed(status_code: i32, stderr: &str) -> bool {
    status_code == 9009 || stderr.contains("is not recognized as an internal or external command")
}
