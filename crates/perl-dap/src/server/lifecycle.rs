use crate::debug_adapter::DebugAdapter;
use crate::server::config::DapConfig;

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
/// `DapServer` owns the supported product runtime: the built-in
/// [`DebugAdapter`] driving the local Perl debugger. Historical proxying to an
/// alternate DAP implementation is not part of this lifecycle.
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

    /// Run the native DAP server over TCP socket transport.
    ///
    /// This binds to `127.0.0.1:<port>` and serves one DAP client session.
    ///
    /// # Errors
    ///
    /// Returns an error when the listener cannot bind or the accepted DAP
    /// session fails.
    pub fn run_socket(&mut self, port: u16) -> anyhow::Result<()> {
        self.adapter.run_socket(port)
    }
}
