//! Constants used by runtime input validation.

/// Maximum allowed path length.
pub(crate) const MAX_PATH_LENGTH: usize = 4096;

/// Maximum allowed URI length.
pub(crate) const MAX_URI_LENGTH: usize = 4096;

/// Maximum allowed JSON payload size for LSP params.
pub(crate) const MAX_PARAMS_SIZE: usize = 1_000_000;

/// Maximum allowed LSP method name length.
pub(crate) const MAX_METHOD_LENGTH: usize = 100;

/// Maximum allowed per-line text length.
pub(crate) const MAX_LINE_LENGTH: usize = 100_000;

/// Allowed file extensions for Perl files.
pub(crate) const ALLOWED_EXTENSIONS: &[&str] = &["pl", "pm", "t", "pod"];

/// Allowed URI schemes for text document synchronization.
///
/// `vscode-notebook-cell:` is admitted because notebook synchronization
/// routes its virtual-document cells through the same `didOpen` sink
/// (`handle_notebook_did_open` -> `handle_did_open`), which keys the
/// documents store by the cell URI.
pub(crate) const ALLOWED_TEXT_DOCUMENT_URI_SCHEMES: &[&str] =
    &["file://", "untitled:", "opencode:", "vscode-notebook-cell:"];
