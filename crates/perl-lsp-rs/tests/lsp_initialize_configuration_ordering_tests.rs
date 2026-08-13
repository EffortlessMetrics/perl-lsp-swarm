mod support;

use serde_json::json;
use std::time::Duration;
use support::lsp_harness::{LspHarness, TempWorkspace};

fn initialize_params(workspace: &TempWorkspace, configuration: bool) -> serde_json::Value {
    json!({
        "processId": std::process::id(),
        "capabilities": {
            "workspace": {
                "configuration": configuration,
                "workspaceFolders": true
            }
        },
        "rootUri": workspace.root_uri,
        "workspaceFolders": [
            {
                "uri": workspace.root_uri,
                "name": "fixture"
            }
        ]
    })
}

fn is_workspace_configuration_request(message: &serde_json::Value) -> bool {
    message.get("method").and_then(serde_json::Value::as_str) == Some("workspace/configuration")
}

#[test]
fn workspace_configuration_is_requested_only_after_initialized() -> Result<(), String> {
    let workspace = TempWorkspace::new()?;
    let mut harness = LspHarness::new_raw();

    let initialize_result = harness.request_with_timeout(
        "initialize",
        initialize_params(&workspace, true),
        Duration::from_secs(5),
    )?;
    assert!(
        initialize_result.get("capabilities").is_some(),
        "initialize must complete successfully before post-initialize work"
    );

    let pre_initialized_requests = harness.drain_server_requests(150);
    assert!(
        !pre_initialized_requests.iter().any(is_workspace_configuration_request),
        "workspace/configuration must not precede InitializeResult/initialized: {pre_initialized_requests:?}"
    );

    harness.notify("initialized", json!({}));
    let post_initialized_requests = harness.drain_server_requests(750);
    let configuration_requests: Vec<_> = post_initialized_requests
        .iter()
        .filter(|request| is_workspace_configuration_request(request))
        .collect();

    assert_eq!(
        configuration_requests.len(),
        1,
        "post-initialize convergence must issue exactly one workspace/configuration request: {post_initialized_requests:?}"
    );

    let items = configuration_requests[0]
        .pointer("/params/items")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "workspace/configuration request must contain params.items".to_string())?;
    assert_eq!(
        items.first().and_then(|item| item.get("section")).and_then(serde_json::Value::as_str),
        Some("perl"),
        "first item must request the global perl settings section"
    );
    assert!(
        items.iter().any(|item| {
            item.get("section").and_then(serde_json::Value::as_str) == Some("perl")
                && item.get("scopeUri").and_then(serde_json::Value::as_str)
                    == Some(workspace.root_uri.as_str())
        }),
        "request must include a folder-scoped perl settings item for the initialized workspace: {items:?}"
    );

    Ok(())
}

#[test]
fn client_without_workspace_configuration_support_receives_no_configuration_request()
-> Result<(), String> {
    let workspace = TempWorkspace::new()?;
    let mut harness = LspHarness::new_raw();

    let initialize_result = harness.request_with_timeout(
        "initialize",
        initialize_params(&workspace, false),
        Duration::from_secs(5),
    )?;
    assert!(initialize_result.get("capabilities").is_some());

    assert!(
        !harness.drain_server_requests(150).iter().any(is_workspace_configuration_request),
        "unsupported client must not receive workspace/configuration before initialized"
    );

    harness.notify("initialized", json!({}));
    let post_initialized_requests = harness.drain_server_requests(500);
    assert!(
        !post_initialized_requests.iter().any(is_workspace_configuration_request),
        "unsupported client must not receive workspace/configuration after initialized: {post_initialized_requests:?}"
    );

    Ok(())
}
