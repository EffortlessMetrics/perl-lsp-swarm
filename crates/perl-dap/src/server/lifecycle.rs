use crate::debug_adapter::DebugAdapter;
use crate::server::config::DapConfig;

/// Native DAP server lifecycle.
///
/// `DapServer` owns the supported product runtime: the built-in
/// [`DebugAdapter`] driving the local Perl debugger. Historical proxying to an
/// alternate DAP implementation is not part of this lifecycle. Native editor
/// TCP (`run_socket`) is retired; production admission is stdio only.
pub struct DapServer {
    /// Server configuration.
    pub config: DapConfig,
    /// The underlying native debug adapter.
    adapter: DebugAdapter,
}

impl DapServer {
    /// Create a new native DAP server instance.
    ///
    /// # Arguments
    ///
    /// * `config` - Server configuration including logging and workspace context.
    ///
    /// # Errors
    ///
    /// Construction retains a result boundary for configuration and runtime
    /// initialization failures.
    pub fn new(config: DapConfig) -> anyhow::Result<Self> {
        let adapter = DebugAdapter::new();
        // Wire the configured workspace boundary (if any) into the adapter so
        // launch requests are validated against it. See
        // `DebugAdapter::set_workspace_root` and `handle_launch` for the
        // narrowing-only override rule applied to launch-args `workspaceRoot`.
        if let Some(root) = config.workspace_root.clone() {
            adapter.set_workspace_root(root);
        }
        Ok(Self { config, adapter })
    }

    /// Run the native DAP server over stdio.
    ///
    /// # Errors
    ///
    /// Returns an error when the DAP transport or native adapter session fails.
    pub fn run(&mut self) -> anyhow::Result<()> {
        self.adapter.run().map_err(Into::into)
    }
}
