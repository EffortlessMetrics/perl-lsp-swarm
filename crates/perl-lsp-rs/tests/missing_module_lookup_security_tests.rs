//! Security regression tests for `perl.explainMissingModuleLookup`.
//!
//! Guards against using crafted module names as a filesystem existence oracle.

use perl_lsp::{JsonRpcRequest, LspServer};
use serde_json::{Value, json};
use std::fs;
use tempfile::TempDir;

fn setup_server(
    root_path: Option<String>,
    initialization_options: Option<Value>,
) -> LspServer {
    let server = LspServer::new();

    let mut init_params = json!({
        "processId": null,
        "rootPath": root_path,
        "capabilities": {}
    });
    if let Some(options) = initialization_options {
        init_params["initializationOptions"] = options;
    }

    let init_request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        method: "initialize".to_string(),
        params: Some(init_params),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
    };
    let _ = server.handle_request(init_request);

    let initialized_request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        method: "initialized".to_string(),
        params: Some(json!({})),
        id: None,
    };
    let _ = server.handle_request(initialized_request);
    server
}

fn explain_missing_module(
    server: &LspServer,
    module: &str,
) -> Option<perl_lsp::protocol::JsonRpcResponse> {
    let execute_request = JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        method: "workspace/executeCommand".to_string(),
        params: Some(json!({
            "command": "perl.explainMissingModuleLookup",
            "arguments": [{ "module": module }]
        })),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(2_i64)),
    };
    server.handle_request(execute_request)
}

fn candidate_exists_values(
    result: &Value,
) -> Result<Vec<Option<bool>>, Box<dyn std::error::Error>> {
    let Some(include_paths) = result
        .pointer("/module_resolution/effective_include_paths")
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };

    include_paths
        .iter()
        .flat_map(|entry| {
            entry
                .get("candidate_paths")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .map(|candidate| match candidate.get("exists") {
            Some(Value::Bool(value)) => Ok(Some(*value)),
            Some(Value::Null) | None => Ok(None),
            Some(other) => Err(format!("unexpected exists payload: {other:?}").into()),
        })
        .collect()
}

#[test]
fn explain_missing_module_rejects_path_traversal_module() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let root_path = temp_dir.path().to_string_lossy().to_string();
    let server = setup_server(Some(root_path), None);

    let response = explain_missing_module(&server, "../../etc/passwd")
        .ok_or("expected JSON-RPC response for traversal module")?;
    assert!(
        response.result.is_none(),
        "path traversal module must not return a success payload: {:?}",
        response.result
    );
    let error = response
        .error
        .ok_or("expected invalid_params error for traversal module")?;
    assert_eq!(error.code, -32602);
    assert!(
        error.message.contains("Invalid module name"),
        "unexpected error message: {}",
        error.message
    );

    Ok(())
}

#[test]
fn explain_missing_module_rejects_slash_module() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let root_path = temp_dir.path().to_string_lossy().to_string();
    let server = setup_server(Some(root_path), None);

    let response = explain_missing_module(&server, "Foo/Bar")
        .ok_or("expected JSON-RPC response for slash module")?;
    assert!(
        response.result.is_none(),
        "slash-shaped module must not return a success payload: {:?}",
        response.result
    );
    let error = response
        .error
        .ok_or("expected invalid_params error for slash module")?;
    assert_eq!(error.code, -32602);

    Ok(())
}

#[test]
fn explain_missing_module_exists_only_inside_workspace() -> Result<(), Box<dyn std::error::Error>> {
    let workspace_dir = TempDir::new()?;
    let outside_dir = TempDir::new()?;

    let inside_module_path = workspace_dir.path().join("lib").join("Foo").join("Bar.pm");
    fs::create_dir_all(inside_module_path.parent().ok_or("missing Foo parent dir")?)?;
    fs::write(&inside_module_path, "package Foo::Bar;\n1;\n")?;

    let inside_server = setup_server(
        Some(workspace_dir.path().to_string_lossy().to_string()),
        None,
    );
    let inside_response = explain_missing_module(&inside_server, "Foo::Bar")
        .ok_or("expected response for in-workspace module")?;
    let inside_result = inside_response
        .result
        .ok_or("in-workspace module lookup should succeed")?;
    assert!(
        candidate_exists_values(&inside_result)?.contains(&Some(true)),
        "in-workspace module should report exists=true for a workspace candidate: {inside_result}"
    );

    let outside_module_path = outside_dir.path().join("Oracle").join("Probe.pm");
    fs::create_dir_all(
        outside_module_path
            .parent()
            .ok_or("missing Oracle parent dir")?,
    )?;
    fs::write(&outside_module_path, "package Oracle::Probe;\n1;\n")?;

    let outside_include_path = outside_dir.path().to_string_lossy().to_string();
    let outside_server = setup_server(
        Some(workspace_dir.path().to_string_lossy().to_string()),
        Some(json!({
            "workspace": {
                "includePaths": [outside_include_path]
            }
        })),
    );
    let outside_response = explain_missing_module(&outside_server, "Oracle::Probe")
        .ok_or("expected response for outside-workspace module")?;
    let outside_result = outside_response
        .result
        .ok_or("outside-workspace module lookup should succeed without error")?;

    let outside_candidates = outside_result
        .pointer("/module_resolution/effective_include_paths")
        .and_then(Value::as_array)
        .ok_or("missing effective_include_paths")?
        .iter()
        .flat_map(|entry| {
            entry
                .get("candidate_paths")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter(|candidate| {
            candidate
                .get("inside_workspace")
                .and_then(Value::as_bool)
                .is_some_and(|inside| !inside)
        })
        .collect::<Vec<_>>();

    assert!(
        !outside_candidates.is_empty(),
        "expected at least one out-of-workspace candidate for external include path"
    );
    for candidate in outside_candidates {
        assert_eq!(
            candidate.get("exists"),
            Some(&Value::Null),
            "out-of-workspace candidate must not leak exists: {candidate:?}"
        );
        assert_eq!(
            candidate.get("probed").and_then(Value::as_bool),
            Some(false),
            "out-of-workspace candidate must not be probed: {candidate:?}"
        );
    }

    assert!(
        !candidate_exists_values(&outside_result)?.contains(&Some(true)),
        "outside-workspace include roots must not report exists=true"
    );

    Ok(())
}
