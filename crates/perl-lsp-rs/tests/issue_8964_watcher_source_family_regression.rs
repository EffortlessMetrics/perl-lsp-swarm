//! Regression guards for #8964 under the superseding catch-all lifecycle contract
//! (#13308/#14074/#14186): handler-side Perl admission must accept canonical
//! source extensions such as `.cgi`/`.fcgi`, and watcher registration must stay
//! aligned with workspace file-operation filters.

mod support;

use perl_lsp_rs_core::runtime::tuning::RuntimeTuning;
use serde_json::{Value, json};
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn cgi_and_fcgi_watched_file_notifications_are_accepted() -> TestResult {
    let server = perl_lsp::LspServer::with_output(std::sync::Arc::new(parking_lot::Mutex::new(
        Box::new(Vec::new()) as Box<dyn std::io::Write + Send>,
    )));

    let init_params = json!({
        "processId": 1234,
        "rootUri": "file:///test/workspace",
        "capabilities": {}
    });
    let init = perl_lsp::JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1)),
        method: "initialize".to_string(),
        params: Some(init_params),
    };
    server.handle_request(init);
    server.handle_request(perl_lsp::JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "initialized".to_string(),
        params: Some(json!({})),
    });

    for uri in ["file:///test/workspace/app.cgi", "file:///test/workspace/run.fcgi"] {
        let watched = perl_lsp::JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: None,
            method: "workspace/didChangeWatchedFiles".to_string(),
            params: Some(json!({
                "changes": [{ "uri": uri, "type": 1 }]
            })),
        };
        assert!(
            server.handle_request(watched).is_none(),
            "didChangeWatchedFiles notifications must be handled without a JSON-RPC response"
        );
    }
    Ok(())
}

#[test]
fn watcher_registration_matches_file_operation_catch_all_globs() -> TestResult {
    let mut harness = LspHarness::new_with_tuning(RuntimeTuning::normal_defaults());
    let init = harness.initialize(Some(watcher_and_file_operation_caps()))?;

    let file_operations = init
        .pointer("/capabilities/workspace/fileOperations")
        .and_then(Value::as_object)
        .ok_or("initialize must advertise workspace.fileOperations")?;
    for (name, op) in file_operations {
        let filters = op
            .get("filters")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{name} must include filters"))?;
        assert!(
            filters.iter().any(|filter| {
                filter.pointer("/pattern/glob").and_then(Value::as_str) == Some("**/*")
            }),
            "{name} must advertise the catch-all glob: {op:?}"
        );
    }

    let requests = harness.drain_server_requests(250);
    let registration = registration_for_method(&requests, "workspace/didChangeWatchedFiles")
        .ok_or("dynamic watcher registration must be sent")?;
    let watchers = registration
        .pointer("/registerOptions/watchers")
        .and_then(Value::as_array)
        .ok_or("watcher registration must include watchers")?;
    assert!(
        watchers.iter().any(|watcher| watcher["globPattern"] == json!("**/*")),
        "watcher registration must advertise catch-all glob: {watchers:?}"
    );
    Ok(())
}

fn watcher_and_file_operation_caps() -> Value {
    json!({
        "workspace": {
            "didChangeWatchedFiles": {
                "dynamicRegistration": true
            },
            "fileOperations": {
                "didCreate": true,
                "didDelete": true,
                "didRename": true,
                "willCreate": true,
                "willDelete": true,
                "willRename": true
            }
        },
        "textDocument": {}
    })
}

fn registration_for_method<'a>(requests: &'a [Value], method: &str) -> Option<&'a Value> {
    requests.iter().find_map(|request| {
        if request.get("method").and_then(Value::as_str) != Some("client/registerCapability") {
            return None;
        }
        request.pointer("/params/registrations").and_then(Value::as_array).and_then(
            |registrations| {
                registrations
                    .iter()
                    .find(|entry| entry.get("method").and_then(Value::as_str) == Some(method))
            },
        )
    })
}
