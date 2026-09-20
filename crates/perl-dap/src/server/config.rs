use crate::security::launch_authority::LaunchAuthorityStartup;
use crate::server::mode::DapMode;

/// DAP server configuration.
///
/// Controls logging and workspace context for the native DAP server. `mode`
/// remains a typed field for API/configuration stability; [`DapMode::Native`] is
/// the only current product runtime.
///
/// `launch_authority` carries the user/machine-owned startup inputs (#8656)
/// from which the adapter resolves exactly one explicit launch-authority mode.
/// A configuration that names neither trusted roots nor an explicit unbounded
/// acknowledgement fails closed at launch admission, before a debuggee process
/// can spawn.
pub struct DapConfig {
    /// Logging level for DAP operations.
    pub log_level: String,
    /// Native debugger operating mode.
    pub mode: DapMode,
    /// Workspace root directory.
    ///
    /// When set, this startup-owned root joins the launch-authority trusted
    /// roots, preserving the historical workspace-bound behavior. Launch
    /// arguments can never populate it.
    pub workspace_root: Option<std::path::PathBuf>,
    /// Launch-authority startup inputs (#8656).
    pub launch_authority: LaunchAuthorityStartup,
}
