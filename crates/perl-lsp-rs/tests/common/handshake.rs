//! LSP initialize/shutdown handshake helpers.
//!
//! Provides `initialize_lsp`, `await_index_ready`, and `shutdown_and_exit` which
//! encapsulate the full LSP lifecycle for integration tests.

// Integration tests print diagnostic output for CI troubleshooting; this is
// not the LSP server's stdio transport, so print_stderr doesn't apply the
// way it does to production code.
#![allow(clippy::print_stderr)]

use perl_tdd_support::must;
use serde_json::{Value, json};
use std::time::Duration;

use super::protocol_io::{
    map_send_error, read_notification_method, read_response_matching_i64, send_message_inner,
};
use super::{
    LspServer, adaptive_timeout, max_concurrent_threads, next_id, send_notification, send_request,
};

fn initialize_lsp_with_params(server: &LspServer, params: Value) -> Value {
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": params
    });

    // write without reading
    {
        let body = init.to_string();
        if let Err(e) =
            send_message_inner(&mut *server.writer.lock().unwrap_or_else(|e| e.into_inner()), &body)
        {
            // Handle write errors gracefully with proper JSON-RPC envelope (id=1)
            return map_send_error(Some(json!(1)), e, "initialize");
        }
    }

    // wait specifically for id=1 - use extended timeout for initialization
    // Enhanced timeout for LSP cancellation tests with environment-aware scaling
    let base_multiplier = 3; // Increased base multiplier for cancellation infrastructure tests (increased from 2x to 3x)
    let thread_count = max_concurrent_threads();
    let env_multiplier = if thread_count <= 2 { 3 } else { 2 }; // Extra time for constrained environments with cancellation infrastructure (increased from 2x to 3x)

    // Additional CI environment detection for graceful degradation
    let ci_multiplier = if std::env::var("CI").is_ok()
        || std::env::var("GITHUB_ACTIONS").is_ok()
        || std::env::var("CONTINUOUS_INTEGRATION").is_ok()
    {
        2 // Extra time for CI environments with limited resources
    } else {
        1
    };

    let init_timeout = adaptive_timeout() * base_multiplier * env_multiplier * ci_multiplier;

    // Enhanced retry logic for cancellation infrastructure tests
    let mut retry_count = 0;
    let max_retries = 2; // Allow 2 retries for infrastructure tests

    let resp = loop {
        match read_response_matching_i64(server, 1, init_timeout) {
            Some(response) => break response,
            None => {
                retry_count += 1;
                if retry_count > max_retries {
                    eprintln!(
                        "LSP server failed to respond to initialize request within {:?} after {} retries",
                        init_timeout, max_retries
                    );
                    eprintln!(
                        "Check if server started properly and is responding to JSON-RPC requests"
                    );
                    eprintln!("Server process alive: {}", server.is_alive());
                    must(Err::<(), _>("initialize response timeout - server may have crashed or is not responding".to_string()))
                } else {
                    eprintln!(
                        "Initialize timeout attempt {}/{}, retrying with fresh request...",
                        retry_count,
                        max_retries + 1
                    );
                    // Brief delay before retry
                    std::thread::sleep(Duration::from_millis(200));
                    // Send another initialize request with a new ID
                    let retry_id = next_id();
                    send_request(
                        server,
                        json!({"id":retry_id,"method":"initialize","params":{"capabilities":{}}}),
                    );
                    // Try reading the retry response
                    if let Some(retry_resp) =
                        read_response_matching_i64(server, retry_id, init_timeout)
                    {
                        break retry_resp;
                    }
                }
            }
        }
    };

    // Send initialized notification with a brief delay
    std::thread::sleep(Duration::from_millis(50));
    send_notification(server, json!({"jsonrpc":"2.0","method":"initialized"}));

    // Wait for index-ready notification to ensure deterministic completion behavior
    await_index_ready(server);

    resp
}

/// Initialize a deliberately rootless session.
///
/// Both root channels are declared empty: `rootUri: null` and a present
/// `workspaceFolders: null`. Under #8161 a present null is an explicit
/// no-active-folder declaration, so this handshake never adopts a legacy root
/// and never reaches the process-working-directory compatibility fallback.
/// That is the intent — these tests drive documents they open themselves and
/// must not race a bulk index of the runner's working directory. Tests that
/// need a real filesystem root call [`initialize_lsp_with_root_path`] instead.
pub fn initialize_lsp(server: &LspServer) -> Value {
    initialize_lsp_with_params(
        server,
        json!({
            "capabilities": {},
            "clientInfo": {"name":"perl-parser-tests","version":"0"},
            "rootUri": null,
            "workspaceFolders": null
        }),
    )
}

/// As [`initialize_lsp`], with client capabilities under the caller's control.
///
/// The session is rootless for the same reason and by the same declaration.
pub fn initialize_lsp_with_capabilities(server: &LspServer, capabilities: Value) -> Value {
    initialize_lsp_with_params(
        server,
        json!({
            "capabilities": capabilities,
            "clientInfo": {"name":"perl-parser-tests","version":"0"},
            "rootUri": null,
            "workspaceFolders": null
        }),
    )
}

/// Initialize the server with an explicit filesystem workspace root.
///
/// Tests that drive virtual (never-on-disk) documents through `didOpen` need
/// this to keep the server's workspace index isolated: a client that declares
/// *nothing at all* still falls back to the process working directory
/// (lightweight-client compatibility, #8161 `NoWorkspaceRoot`) and bulk-indexes
/// it, racing those tests' assertions with a real-directory scan and polluting
/// the index with files unrelated to the scenario under test. Pointing the
/// server at an empty directory keeps the index populated only by the documents
/// the test opens.
///
/// `workspaceFolders` is omitted here rather than sent as null: a present null
/// is an explicit no-active-folder declaration that suppresses the legacy
/// `rootPath` channel this helper depends on.
pub fn initialize_lsp_with_root_path(server: &LspServer, root_path: &str) -> Value {
    initialize_lsp_with_params(
        server,
        json!({
            "capabilities": {},
            "clientInfo": {"name":"perl-parser-tests","version":"0"},
            "rootPath": root_path,
            "rootUri": null
        }),
    )
}

/// Wait for the index-ready notification from the server
pub fn await_index_ready(server: &LspServer) {
    // Wait for perl-lsp/index-ready notification with a reasonable timeout
    if let Some(_notification) =
        read_notification_method(server, "perl-lsp/index-ready", Duration::from_millis(500))
    {
        eprintln!("Index ready notification received");
    } else {
        eprintln!("No index-ready notification received within timeout (proceeding anyway)");
    }
}

/// Gracefully shut the server down (best-effort) without hanging tests.
pub fn shutdown_and_exit(server: &LspServer) {
    use std::sync::atomic::Ordering;

    // Mark shutdown as initiated to prevent duplicate shutdown in Drop
    if server.shutdown_initiated.swap(true, Ordering::SeqCst) {
        // Already initiated, just wait for exit
        for _ in 0..20 {
            if server
                .process
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .try_wait()
                .ok()
                .flatten()
                .is_some()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        return;
    }

    // Try a graceful shutdown first; if the server ignores, we'll still exit the test.
    let _ = send_request(
        server,
        json!({"jsonrpc":"2.0","id": 999_001,"method":"shutdown","params":{}}),
    );
    send_notification(server, json!({"jsonrpc":"2.0","method":"exit"}));

    // Give it a moment, then force-kill if needed.
    for _ in 0..20 {
        if server
            .process
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_wait()
            .ok()
            .flatten()
            .is_some()
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = server.process.lock().unwrap_or_else(|e| e.into_inner()).kill();
}

#[cfg(test)]
mod initialize_lsp_with_root_path_tests {
    use super::{LspServer, initialize_lsp_with_root_path};
    use serde_json::Value;

    type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

    /// The root-scoped handshake must complete against a real server and an
    /// empty root directory: the initialize response carries capabilities,
    /// and the `rootPath` channel resolves the same folder state the suite's
    /// isolated-workspace tests rely on.
    #[test]
    fn initializes_server_against_an_explicit_empty_root() -> TestResult<()> {
        let root =
            std::env::temp_dir().join(format!("perl-lsp-root-path-test-{}", std::process::id()));
        std::fs::create_dir_all(&root)
            .map_err(|err| format!("failed to create root {}: {err}", root.display()))?;

        let server: LspServer = super::super::start_lsp_server();
        let response = initialize_lsp_with_root_path(&server, &root.to_string_lossy());

        let capabilities = response
            .get("result")
            .and_then(|result| result.get("capabilities"))
            .cloned()
            .unwrap_or(Value::Null);
        assert!(
            capabilities.is_object(),
            "initialize must answer with a capabilities object, got: {response:?}"
        );

        super::shutdown_and_exit(&server);
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }
}
