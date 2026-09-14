use crate::security::WorkspaceAuthority;
use crate::server::mode::DapMode;

/// DAP server configuration.
///
/// Controls logging and workspace context for the native DAP server. `mode`
/// remains a typed field for API/configuration stability; [`DapMode::Native`] is
/// the only current product runtime.
pub struct DapConfig {
    /// Logging level for DAP operations.
    pub log_level: String,
    /// Native debugger operating mode.
    pub mode: DapMode,
    /// Trusted launch authority for this adapter process.
    ///
    /// This replaces the earlier `workspace_root: Option<PathBuf>` field. A
    /// bare `Option` could not distinguish "the operator confined this adapter
    /// to a directory" from "nothing was configured", and it was mutable, so a
    /// per-launch `workspaceRoot` overwrote it for every later session. See
    /// [`WorkspaceAuthority`].
    pub workspace_authority: WorkspaceAuthority,
}
