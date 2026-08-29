//! Tool detection and management
//!
//! Handles detection of external tools like perltidy and perlcritic.

use super::super::LspServer;

impl LspServer {
    /// Detect if a tool is available on the system
    ///
    /// Uses the shared command resolution helper so capability advertisement
    /// matches the command execution path on each platform. The underlying
    /// `command_exists` lookup is memoized per process (keyed on the command
    /// and its PATH/PATHEXT environment inputs), so repeated detection — the
    /// initialize-time capability build, capability refreshes, and the
    /// diagnostics-cycle availability guards — does not re-walk PATH.
    pub(crate) fn detect_tool(&self, tool_name: &str) -> bool {
        crate::execute_command::command_exists(tool_name)
    }
}
