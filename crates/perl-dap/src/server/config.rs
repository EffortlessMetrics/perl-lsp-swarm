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
    /// Workspace root directory.
    pub workspace_root: Option<std::path::PathBuf>,
}
