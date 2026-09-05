//! Tool detection and management
//!
//! Handles detection of external tools like perltidy and perlcritic.

use super::super::LspServer;

impl LspServer {
    /// Detect if a tool is available on the system.
    ///
    /// Answers through the runtime's single availability authority
    /// ([`crate::execute_command::command_exists`]): the tool must be found in
    /// an absolute `PATH` directory, and the current directory — routinely the
    /// opened workspace — is never searched. Capability advertisement therefore
    /// cannot be driven by a file planted in the workspace.
    ///
    /// This is an *availability* fact, not a support or execution guarantee:
    /// the tool can still be removed, replaced, or fail to spawn afterwards,
    /// and the launch remains authoritative for that.
    pub(crate) fn detect_tool(&self, tool_name: &str) -> bool {
        crate::execute_command::command_exists(tool_name)
    }
}
