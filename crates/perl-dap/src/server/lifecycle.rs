use crate::debug_adapter::DebugAdapter;
use crate::security::launch_authority::LaunchAuthority;
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
    /// * `config` - Server configuration including logging, workspace context,
    ///   and the launch-authority startup inputs (#8656).
    ///
    /// # Errors
    ///
    /// Construction retains a result boundary for configuration and runtime
    /// initialization failures. When launch-authority inputs are configured,
    /// they are validated here and invalid inputs (for example a missing
    /// trusted root) reject startup before any debuggee process can spawn.
    /// Without authority inputs the server still starts for boundary-free
    /// management flows, and every launch request is refused fail-closed
    /// (see `handle_launch`).
    pub fn new(config: DapConfig) -> anyhow::Result<Self> {
        let adapter = DebugAdapter::new();
        // Resolve the launch-authority decision from the user/machine-owned
        // startup inputs when any are configured. A configured
        // `workspace_root` is a startup-owned boundary: it joins the
        // trusted-root set so the historical workspace-bound behavior keeps
        // working under the explicit contract.
        let mut startup = config.launch_authority.clone();
        if let Some(root) = config.workspace_root.clone() {
            if !startup.trusted_roots.iter().any(|listed| listed == &root) {
                startup.trusted_roots.push(root.clone());
            }
            adapter.set_workspace_root(root);
        }
        if !startup.trusted_roots.is_empty() || startup.allow_unbounded.is_some() {
            let authority = LaunchAuthority::resolve(&startup).map_err(|error| {
                anyhow::anyhow!("launch authority rejected at startup: {error}")
            })?;
            tracing::info!(
                mode = authority.mode().label(),
                authority_identity = %authority.identity(),
                "launch authority resolved"
            );
            adapter.set_launch_authority(authority);
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
