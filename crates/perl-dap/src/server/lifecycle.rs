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
        // The adapter is constructed with the configured authority rather than
        // having a root pushed into it afterwards, so the trust boundary exists
        // before the adapter can serve a single request and cannot be replaced
        // later. See `handle_launch` for the narrowing-only rule applied to a
        // launch-args `workspaceRoot`.
        let adapter = DebugAdapter::with_workspace_authority(config.workspace_authority.clone());
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
