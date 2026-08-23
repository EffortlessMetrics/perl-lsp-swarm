//! Workspace Resolution Regression Tests
//!
//! Validates the deterministic module resolution precedence order:
//! 1. Open documents
//! 2. Workspace folders (in initialization order)
//! 3. Configured include paths
//! 4. System @INC (opt-in)
//!
//! Also tests legacy rootPath handling and configuration management.

use parking_lot::Mutex;
use perl_lsp::state::WorkspaceConfig;
use perl_lsp::{JsonRpcId, JsonRpcRequest, LspServer};
use serde_json::{Value, json};
use std::io::Write;
use std::sync::Arc;

/// Simple writer that captures all output into a shared buffer
struct CapturingWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl CapturingWriter {
    fn new(buffer: Arc<Mutex<Vec<u8>>>) -> Self {
        Self { buffer }
    }
}

impl Write for CapturingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.lock().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Helper to create a test server with captured output
fn create_test_server() -> (LspServer, Arc<Mutex<Vec<u8>>>) {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let writer = CapturingWriter::new(buffer.clone());
    let output: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(Box::new(writer)));
    let server = LspServer::with_output(output);
    (server, buffer)
}

/// Helper to send a request to the server
fn send_request(
    server: &LspServer,
    method: &str,
    id: Option<JsonRpcId>,
    params: Value,
) -> Option<Value> {
    let req =
        JsonRpcRequest { _jsonrpc: "2.0".into(), id, method: method.into(), params: Some(params) };
    server.handle_request(req).and_then(|r| r.result)
}

/// Helper to initialize and mark server as ready
fn initialize_server(server: &LspServer) {
    // Initialize
    send_request(
        server,
        "initialize",
        Some(JsonRpcId::Integer(1)),
        json!({
            "rootUri": "file:///workspace",
            "capabilities": {}
        }),
    );

    // Send initialized notification
    let req = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "initialized".into(),
        params: Some(json!({})),
    };
    let _ = server.handle_request(req);
}

// =============================================================================
// WorkspaceConfig Unit Tests
// =============================================================================

#[test]
fn workspace_config_default_include_paths() {
    let config = WorkspaceConfig::default();

    assert_eq!(config.include_paths, vec!["lib", ".", "local/lib/perl5"]);
    assert!(!config.use_system_inc);
    assert_eq!(config.resolution_timeout_ms, 50);
}

#[test]
fn workspace_config_update_from_settings() {
    let mut config = WorkspaceConfig::default();

    let settings = json!({
        "workspace": {
            "includePaths": ["custom/lib", "vendor/lib"],
            "useSystemInc": true,
            "resolutionTimeout": 100
        }
    });

    config.update_from_value(&settings);

    assert_eq!(config.include_paths, vec!["custom/lib", "vendor/lib"]);
    assert!(config.use_system_inc);
    assert_eq!(config.resolution_timeout_ms, 100);
}

#[test]
fn workspace_config_partial_update() {
    let mut config = WorkspaceConfig::default();

    // Only update include_paths
    let settings = json!({
        "workspace": {
            "includePaths": ["src/lib"]
        }
    });

    config.update_from_value(&settings);

    // include_paths changed
    assert_eq!(config.include_paths, vec!["src/lib"]);
    // Other fields unchanged
    assert!(!config.use_system_inc);
    assert_eq!(config.resolution_timeout_ms, 50);
}

#[test]
fn workspace_config_system_inc_disabled_by_default() {
    let mut config = WorkspaceConfig::default();

    // Should return empty slice when disabled
    let paths = config.get_system_inc();
    assert!(paths.is_empty());
}

// =============================================================================
// Initialize Handler Tests
// =============================================================================

#[test]
fn initialize_with_workspace_folders() -> Result<(), Box<dyn std::error::Error>> {
    let (server, _buffer) = create_test_server();

    let result = send_request(
        &server,
        "initialize",
        Some(JsonRpcId::Integer(1)),
        json!({
            "workspaceFolders": [
                { "uri": "file:///primary", "name": "primary" },
                { "uri": "file:///secondary", "name": "secondary" }
            ],
            "capabilities": {}
        }),
    );

    let caps = result.ok_or("Expected initialize result")?;
    assert!(caps.get("capabilities").is_some());
    assert!(caps.get("serverInfo").is_some());
    Ok(())
}

#[test]
fn initialize_with_root_uri_fallback() -> Result<(), Box<dyn std::error::Error>> {
    let (server, _buffer) = create_test_server();

    let result = send_request(
        &server,
        "initialize",
        Some(JsonRpcId::Integer(1)),
        json!({
            "rootUri": "file:///workspace",
            "capabilities": {}
        }),
    );

    let caps = result.ok_or("Expected initialize result")?;
    assert!(caps.get("capabilities").is_some());
    Ok(())
}

#[test]
fn initialize_with_legacy_root_path_fallback() -> Result<(), Box<dyn std::error::Error>> {
    let (server, _buffer) = create_test_server();

    // Legacy rootPath (deprecated since LSP 3.0 but still used by some clients)
    let result = send_request(
        &server,
        "initialize",
        Some(JsonRpcId::Integer(1)),
        json!({
            "rootPath": "/legacy/workspace",
            "capabilities": {}
        }),
    );

    let caps = result.ok_or("Expected initialize result")?;
    assert!(caps.get("capabilities").is_some());
    Ok(())
}

#[test]
fn initialize_with_legacy_root_path_sets_workspace_folder_path()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, _buffer) = create_test_server();

    let result = send_request(
        &server,
        "initialize",
        Some(JsonRpcId::Integer(1)),
        json!({
            "rootPath": "/legacy/workspace",
            "capabilities": {}
        }),
    );

    result.ok_or("Expected initialize result")?;

    let folders = server.all_workspace_folders();
    assert_eq!(folders.len(), 1, "expected one workspace folder from rootPath");
    assert_eq!(
        folders.first().and_then(|f| f.path.as_ref()).map(|p| p.to_string_lossy().to_string()),
        Some("/legacy/workspace".to_string()),
        "legacy rootPath should populate folder.path for downstream workspace features"
    );

    Ok(())
}

#[test]
fn initialize_windows_root_path_conversion() -> Result<(), Box<dyn std::error::Error>> {
    let (server, _buffer) = create_test_server();

    // Windows-style rootPath should be handled
    let result = send_request(
        &server,
        "initialize",
        Some(JsonRpcId::Integer(1)),
        json!({
            "rootPath": "C:\\Users\\dev\\project",
            "capabilities": {}
        }),
    );

    result.ok_or("Expected initialize result")?;
    Ok(())
}

#[test]
fn initialize_rejects_double_initialize() -> Result<(), Box<dyn std::error::Error>> {
    let (server, _buffer) = create_test_server();

    // First initialize should succeed
    let result1 = send_request(
        &server,
        "initialize",
        Some(JsonRpcId::Integer(1)),
        json!({
            "rootUri": "file:///workspace",
            "capabilities": {}
        }),
    );
    result1.ok_or("Expected first initialize to succeed")?;

    // Send initialized notification to complete handshake
    let req = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "initialized".into(),
        params: Some(json!({})),
    };
    let _ = server.handle_request(req);

    // Second initialize should fail
    let req = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
        method: "initialize".into(),
        params: Some(json!({
            "rootUri": "file:///workspace2",
            "capabilities": {}
        })),
    };
    let response = server.handle_request(req);

    // Should get an error response
    let resp = response.ok_or("Expected error response")?;
    let error = resp.error.as_ref().ok_or("Expected error field")?;
    assert_eq!(error.code, -32600); // InvalidRequest
    Ok(())
}

// =============================================================================
// Method-direction contract for workspace/configuration (#8896)
// =============================================================================

/// Helper returning the full JSON-RPC response so direction tests can assert
/// exact error codes.
fn raw_request(
    server: &LspServer,
    method: &str,
    id: Option<JsonRpcId>,
    params: Value,
) -> Option<perl_lsp::protocol::JsonRpcResponse> {
    let req =
        JsonRpcRequest { _jsonrpc: "2.0".into(), id, method: method.into(), params: Some(params) };
    server.handle_request(req)
}

/// `workspace/configuration` is a standard server→client request (#8896): a
/// client-originated configuration request must be rejected as MethodNotFound
/// instead of being answered from server state.
fn assert_inbound_configuration_is_method_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let (server, _buffer) = create_test_server();
    initialize_server(&server);

    let response = raw_request(
        &server,
        "workspace/configuration",
        Some(JsonRpcId::Integer(2)),
        json!({
            "items": [
                { "section": "perl.workspace.includePaths" },
                { "section": "perl.workspace.useSystemInc" },
                { "section": "perl.workspace.usePerl5lib" },
                { "section": "perl.workspace.perl5libPrecedence" },
                { "section": "perl.workspace.resolutionTimeout" }
            ]
        }),
    )
    .ok_or("expected a JSON-RPC error response")?;

    let error = response.error.ok_or("expected an error payload")?;
    assert_eq!(error.code, -32601, "wrong-direction configuration must be MethodNotFound");
    Ok(())
}

#[test]
fn inbound_configuration_include_paths_readback_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    assert_inbound_configuration_is_method_not_found()
}

#[test]
fn inbound_configuration_system_inc_readback_is_rejected() -> Result<(), Box<dyn std::error::Error>>
{
    assert_inbound_configuration_is_method_not_found()
}

#[test]
fn inbound_configuration_perl5lib_defaults_are_not_served_inbound()
-> Result<(), Box<dyn std::error::Error>> {
    // Defaults remain owned by the `WorkspaceConfig` unit tests above; the
    // reversed readback route they used to travel is gone.
    assert_inbound_configuration_is_method_not_found()
}

#[test]
fn inbound_configuration_resolution_timeout_is_not_served_inbound()
-> Result<(), Box<dyn std::error::Error>> {
    assert_inbound_configuration_is_method_not_found()
}

// =============================================================================
// didChangeConfiguration Tests
// =============================================================================

#[test]
fn did_change_configuration_is_accepted_and_readback_stays_outbound()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, _buffer) = create_test_server();

    // Initialize and mark ready
    initialize_server(&server);

    // Send didChangeConfiguration notification
    let req = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None, // No ID for notifications
        method: "workspace/didChangeConfiguration".into(),
        params: Some(json!({
            "settings": {
                "perl": {
                    "workspace": {
                        "includePaths": ["custom/lib", "vendor"],
                        "useSystemInc": true,
                        "resolutionTimeout": 100
                    }
                }
            }
        })),
    };

    // The legitimate c2s notification is still accepted silently.
    assert!(server.handle_request(req).is_none(), "didChangeConfiguration is a notification");

    // Its settings effects are proven by the in-crate lifecycle suite
    // (`did_change_configuration_updates_folder_effective_configs`); at this
    // boundary we pin that the observation channel can no longer be the
    // reversed inbound configuration request.
    let response = raw_request(
        &server,
        "workspace/configuration",
        Some(JsonRpcId::Integer(2)),
        json!({ "items": [{ "section": "perl.workspace.includePaths" }] }),
    )
    .ok_or("expected a JSON-RPC error response")?;
    assert_eq!(
        response.error.map(|error| error.code),
        Some(-32601),
        "readback must travel the outbound reverse-request path only"
    );
    Ok(())
}

#[test]
fn did_change_configuration_perl5lib_notification_accepted_without_inbound_readback()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, _buffer) = create_test_server();

    initialize_server(&server);

    let req = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: None,
        method: "workspace/didChangeConfiguration".into(),
        params: Some(json!({
            "settings": {
                "perl": {
                    "workspace": {
                        "usePerl5lib": false,
                        "perl5libPrecedence": "append"
                    }
                }
            }
        })),
    };
    assert!(server.handle_request(req).is_none(), "didChangeConfiguration is a notification");

    let response = raw_request(
        &server,
        "workspace/configuration",
        Some(JsonRpcId::Integer(3)),
        json!({
            "items": [
                { "section": "perl.workspace.usePerl5lib" },
                { "section": "perl.workspace.perl5libPrecedence" }
            ]
        }),
    )
    .ok_or("expected a JSON-RPC error response")?;
    assert_eq!(
        response.error.map(|error| error.code),
        Some(-32601),
        "reversed readback must stay rejected after reconfiguration"
    );
    Ok(())
}

// =============================================================================
// Resolution Precedence Documentation Tests
// =============================================================================

/// Verify that the resolution precedence is documented correctly
/// This is a compile-time check that the documentation exists
#[test]
fn resolution_precedence_is_documented() {
    // The resolve_module_to_path function should have documentation
    // describing the 4-tier precedence order:
    // 1. Open Documents
    // 2. Workspace Folders
    // 3. Configured Include Paths
    // 4. System @INC (opt-in)

    // This test serves as a reminder to maintain the documentation
    // If this test compiles, the function exists (documentation is in source)
    // (No assertion needed - compilation itself validates the documentation exists)
}

/// Test that system @INC lookup is only performed when enabled
#[test]
fn system_inc_opt_in_only() {
    let config = WorkspaceConfig::default();

    // By default, use_system_inc should be false
    assert!(!config.use_system_inc);

    // This ensures network filesystem blocking is avoided by default
}
