#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);
const ABSENCE_TIMEOUT: Duration = Duration::from_millis(250);

fn file_uri(path: &Path) -> String {
    #[cfg(windows)]
    {
        format!("file:///{}", path.display().to_string().replace('\\', "/"))
    }
    #[cfg(not(windows))]
    {
        format!("file://{}", path.display())
    }
}

fn initialize_request(id: Value, root_uri: &str, configuration: bool) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "processId": null,
            "clientInfo": {
                "name": "post-initialize-configuration-contract",
                "version": "1"
            },
            "rootUri": root_uri,
            "workspaceFolders": [
                {
                    "uri": root_uri,
                    "name": "fixture"
                }
            ],
            "capabilities": {
                "workspace": {
                    "configuration": configuration,
                    "workspaceFolders": true
                }
            },
            "initializationOptions": {
                "perl": {
                    "workspace": {
                        "includePaths": ["lib"],
                        "useSystemInc": false
                    }
                }
            }
        }
    })
}

fn assert_initialize_response(response: &Value, id: &Value) -> Result<()> {
    ensure!(response.get("id") == Some(id), "initialize response used the wrong id: {response}");
    ensure!(response.get("error").is_none(), "initialize returned an error: {response}");
    ensure!(
        response.pointer("/result/capabilities").is_some(),
        "initialize response is missing server capabilities: {response}"
    );
    Ok(())
}

fn respond_to_workspace_configuration(
    client: &mut RealProcessClient,
    request: &Value,
) -> Result<()> {
    let request_id =
        request.get("id").cloned().context("workspace/configuration request missing id")?;
    let items = request
        .pointer("/params/items")
        .and_then(Value::as_array)
        .context("workspace/configuration request missing params.items")?;
    ensure!(
        items.len() == 2,
        "one global item plus one workspace-folder item expected, got {items:?}"
    );
    client.respond(
        request_id,
        json!([
            {"workspace": {"useSystemInc": false}},
            {"workspace": {"includePaths": ["client_lib"]}}
        ]),
    )
}

fn clean_shutdown(client: &mut RealProcessClient) -> Result<()> {
    let response = client.request(json!(900), "shutdown", json!(null), PROCESS_TIMEOUT)?;
    ensure!(response.get("error").is_none(), "shutdown returned an error: {response}");
    ensure!(response.get("result").is_some_and(Value::is_null), "shutdown must return null");
    client.notify("exit", json!(null))?;
    let status = client.wait_for_exit(PROCESS_TIMEOUT)?;
    ensure!(status.success(), "perllsp exited unsuccessfully: {status}");
    client.assert_transport_clean()
}

#[test]
fn workspace_configuration_is_emitted_only_after_initialized() -> Result<()> {
    let workspace = tempfile::tempdir().context("create workspace fixture")?;
    std::fs::create_dir_all(workspace.path().join("lib"))?;
    let root_uri = file_uri(workspace.path());
    let initialize_id = json!(101);
    let initialize = initialize_request(initialize_id.clone(), &root_uri, true);

    let mut client = RealProcessClient::spawn_exact()?;
    client.send_raw_bytes(&RealProcessClient::encode_message(&initialize))?;

    // Retain the initialize response after reading every earlier frame. If the
    // server sent workspace/configuration first, the strict client buffered it
    // and the next lookup returns it immediately.
    let _retained_initialize =
        client.receive_response_and_retain(&initialize_id, PROCESS_TIMEOUT)?;
    let premature = client.receive_server_request("workspace/configuration", ABSENCE_TIMEOUT);
    ensure!(
        premature.is_err(),
        "workspace/configuration escaped before InitializeResult: {premature:?}"
    );
    let response = client.receive_response(&initialize_id, ABSENCE_TIMEOUT)?;
    assert_initialize_response(&response, &initialize_id)?;

    client.notify("initialized", json!({}))?;
    let configuration =
        client.receive_server_request("workspace/configuration", PROCESS_TIMEOUT)?;
    respond_to_workspace_configuration(&mut client, &configuration)?;

    let duplicate = client.receive_server_request("workspace/configuration", ABSENCE_TIMEOUT);
    ensure!(
        duplicate.is_err(),
        "initial configuration pull must happen exactly once: {duplicate:?}"
    );

    clean_shutdown(&mut client)
}

#[test]
fn client_without_workspace_configuration_support_receives_no_request() -> Result<()> {
    let workspace = tempfile::tempdir().context("create workspace fixture")?;
    let root_uri = file_uri(workspace.path());
    let initialize_id = json!(201);

    let mut client = RealProcessClient::spawn_exact()?;
    let response = client.request(
        initialize_id.clone(),
        "initialize",
        initialize_request(initialize_id.clone(), &root_uri, false)["params"].clone(),
        PROCESS_TIMEOUT,
    )?;
    assert_initialize_response(&response, &initialize_id)?;
    client.notify("initialized", json!({}))?;

    let configuration = client.receive_server_request("workspace/configuration", ABSENCE_TIMEOUT);
    ensure!(
        configuration.is_err(),
        "unsupported client received workspace/configuration: {configuration:?}"
    );

    clean_shutdown(&mut client)
}

#[test]
fn compatibility_initialization_starts_the_deferred_pull_after_initialize_response() -> Result<()> {
    let workspace = tempfile::tempdir().context("create workspace fixture")?;
    let source_path = workspace.path().join("main.pl");
    std::fs::write(&source_path, "use strict;\n")?;
    let root_uri = file_uri(workspace.path());
    let source_uri = file_uri(&source_path);
    let initialize_id = json!(301);
    let initialize = initialize_request(initialize_id.clone(), &root_uri, true);

    let mut client = RealProcessClient::spawn_exact()?;
    client.send_raw_bytes(&RealProcessClient::encode_message(&initialize))?;
    let _retained_initialize =
        client.receive_response_and_retain(&initialize_id, PROCESS_TIMEOUT)?;
    let premature = client.receive_server_request("workspace/configuration", ABSENCE_TIMEOUT);
    ensure!(
        premature.is_err(),
        "compatibility client saw configuration before initialize response: {premature:?}"
    );
    let response = client.receive_response(&initialize_id, ABSENCE_TIMEOUT)?;
    assert_initialize_response(&response, &initialize_id)?;

    // This client deliberately omits `initialized`. The first later request
    // advances the reviewed compatibility path only after InitializeResult.
    let hover = client.request(
        json!(302),
        "textDocument/hover",
        json!({
            "textDocument": {"uri": source_uri},
            "position": {"line": 0, "character": 1}
        }),
        PROCESS_TIMEOUT,
    )?;
    ensure!(hover.get("error").is_none(), "compatibility hover failed: {hover}");

    let configuration =
        client.receive_server_request("workspace/configuration", PROCESS_TIMEOUT)?;
    respond_to_workspace_configuration(&mut client, &configuration)?;
    let duplicate = client.receive_server_request("workspace/configuration", ABSENCE_TIMEOUT);
    ensure!(
        duplicate.is_err(),
        "compatibility completion emitted duplicate configuration pull: {duplicate:?}"
    );

    clean_shutdown(&mut client)
}
