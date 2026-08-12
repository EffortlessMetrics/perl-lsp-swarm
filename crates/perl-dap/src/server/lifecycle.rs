// The lifecycle dispatcher retains the deprecated bridge mode solely for
// existing library consumers and conformance comparisons. It must still match
// the hidden variant in default builds so unsupported selection fails closed.
#![allow(deprecated)]

#[cfg(feature = "legacy-pls-bridge")]
use crate::bridge_adapter::BridgeAdapter;
use crate::debug_adapter::DebugAdapter;
use crate::server::config::DapConfig;
use crate::server::mode::DapMode;

/// Marks a failure opening the native DAP TCP listener, before a client session exists.
///
/// The operating-system error remains the source in the `anyhow` chain so
/// callers that historically downcast socket failures to `std::io::Error`
/// keep that compatibility surface.
#[derive(Debug, thiserror::Error)]
#[error("failed to bind DAP socket on 127.0.0.1:{port}")]
pub struct DapSocketBindError {
    /// The requested local port.
    pub port: u16,
}

impl perl_parser_core::ErrorClass for DapSocketBindError {
    fn error_class(&self) -> perl_parser_core::ErrorCategory {
        // OS-level socket bind failure — external resource/port unavailable.
        perl_parser_core::ErrorCategory::Infra
    }
}

/// Native DAP server lifecycle.
///
/// New callers should use [`DapMode::Native`], which drives the built-in
/// [`DebugAdapter`] through the local Perl interpreter. The deprecated bridge
/// mode remains as an inert source-compatibility variant in default builds and
/// is executable only when the `legacy-pls-bridge` feature is selected.
pub struct DapServer {
    /// Server configuration.
    pub config: DapConfig,
    /// The underlying native debug adapter.
    adapter: DebugAdapter,
}

impl DapServer {
    /// Create a new DAP server instance.
    ///
    /// # Arguments
    ///
    /// * `config` - Server configuration including operating mode.
    ///
    /// # Errors
    ///
    /// Currently always succeeds. Construction retains a result boundary for
    /// configuration and runtime initialization failures.
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

    /// Run the DAP server over stdio.
    ///
    /// [`DapMode::Native`] is the supported product path. Selecting the
    /// deprecated bridge mode without its explicit compatibility feature fails
    /// closed rather than spawning an external backend unexpectedly.
    pub fn run(&mut self) -> anyhow::Result<()> {
        match self.config.mode {
            DapMode::Native => self.adapter.run().map_err(Into::into),
            DapMode::Bridge => {
                #[cfg(feature = "legacy-pls-bridge")]
                {
                    tracing::warn!(
                        "Starting deprecated Perl::LanguageServer bridge compatibility mode"
                    );
                    let rt = tokio::runtime::Runtime::new()?;
                    return rt.block_on(async {
                        let mut bridge = BridgeAdapter::new();
                        bridge.spawn_pls_dap().await?;
                        bridge.proxy_messages().await?;
                        bridge.shutdown().await?;
                        Ok(())
                    });
                }

                #[cfg(not(feature = "legacy-pls-bridge"))]
                anyhow::bail!(
                    "legacy Perl::LanguageServer bridge support is not enabled; use DapMode::Native"
                );
            }
        }
    }

    /// Run the native DAP server over TCP socket transport.
    ///
    /// This binds to `127.0.0.1:<port>` and serves one DAP client session.
    ///
    /// # Errors
    ///
    /// Returns an error if the deprecated bridge mode is selected, since socket
    /// transport is supported only by the native adapter.
    pub fn run_socket(&mut self, port: u16) -> anyhow::Result<()> {
        if self.config.mode == DapMode::Bridge {
            anyhow::bail!("Socket transport is not supported in legacy bridge mode");
        }
        self.adapter.run_socket(port)
    }
}
