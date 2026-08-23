#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, bail, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);
const ABSENCE_TIMEOUT: Duration = Duration::from_millis(250);
const STABLE_PROFILE: &str = include_str!("fixtures/helix/25.07.1.initialize.json");
const MASTER_PROFILE: &str = include_str!("fixtures/helix/master-079a789e.initialize.json");
const WATCH_PATTERNS: &[&str] = &["**/*.pl", "**/*.pm", "**/*.t", "**/*.psgi"];

fn profile(raw: &str) -> Result<Value> {
    serde_json::from_str(raw).context("parse checked-in Helix initialize profile")
}

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

fn initialize_params(profile: &Value, workspace: &Path) -> Result<Value> {
    let mut params =
        profile.get("initialize_params").cloned().context("profile missing initialize_params")?;
    let root_uri = file_uri(workspace);
    params["processId"] = Value::Null;
    params["rootPath"] = Value::String(workspace.display().to_string());
    params["rootUri"] = Value::String(root_uri.clone());
    params["workspaceFolders"] = json!([{
        "uri": root_uri,
        "name": "fixture"
    }]);
    Ok(params)
}

fn assert_initialize_response(response: &Value, id: i64) -> Result<()> {
    ensure!(response.get("id").and_then(Value::as_i64) == Some(id), "wrong response id");
    ensure!(response.get("error").is_none(), "initialize failed: {response}");
    ensure!(
        response.pointer("/result/capabilities/positionEncoding").and_then(Value::as_str)
            == Some("utf-16"),
        "Helix profile must retain the current UTF-16 server contract: {response}"
    );
    Ok(())
}

fn settle_workspace_configuration(client: &mut RealProcessClient) -> Result<()> {
    let request = client.receive_server_request("workspace/configuration", PROCESS_TIMEOUT)?;
    let id = request.get("id").cloned().context("configuration request missing id")?;
    let item_count = request
        .pointer("/params/items")
        .and_then(Value::as_array)
        .map(Vec::len)
        .context("configuration request missing items")?;
    client.respond(id, Value::Array(vec![json!({}); item_count]))
}

fn receive_watcher_registration(client: &mut RealProcessClient) -> Result<Value> {
    for _ in 0..4 {
        let request =
            client.receive_server_request("client/registerCapability", PROCESS_TIMEOUT)?;
        let id = request.get("id").cloned().context("registration request missing id")?;
        let watchers = request.pointer("/params/registrations").and_then(Value::as_array).and_then(
            |registrations| {
                registrations.iter().find_map(|registration| {
                    (registration.get("method").and_then(Value::as_str)
                        == Some("workspace/didChangeWatchedFiles"))
                    .then(|| registration.pointer("/registerOptions/watchers").cloned())
                    .flatten()
                })
            },
        );
        client.respond(id, Value::Null)?;
        if let Some(watchers) = watchers {
            return Ok(watchers);
        }
    }
    bail!("no workspace/didChangeWatchedFiles registration was observed")
}

fn assert_string_watchers(watchers: &Value) -> Result<()> {
    let watchers = watchers.as_array().context("watchers must be an array")?;
    ensure!(watchers.len() == WATCH_PATTERNS.len(), "unexpected watchers: {watchers:?}");
    for (watcher, expected) in watchers.iter().zip(WATCH_PATTERNS) {
        ensure!(
            watcher.get("globPattern").and_then(Value::as_str) == Some(*expected),
            "string watcher used the wrong glob: {watcher}"
        );
        ensure!(
            watcher.get("kind").and_then(Value::as_u64) == Some(7),
            "string watcher used the wrong kind: {watcher}"
        );
    }
    Ok(())
}

fn assert_relative_watchers(watchers: &Value, root_uri: &str) -> Result<()> {
    let watchers = watchers.as_array().context("watchers must be an array")?;
    ensure!(watchers.len() == WATCH_PATTERNS.len(), "unexpected watchers: {watchers:?}");
    for (watcher, expected) in watchers.iter().zip(WATCH_PATTERNS) {
        ensure!(
            watcher.pointer("/globPattern/baseUri").and_then(Value::as_str) == Some(root_uri),
            "relative watcher used the wrong base URI: {watcher}"
        );
        ensure!(
            watcher.pointer("/globPattern/pattern").and_then(Value::as_str) == Some(*expected),
            "relative watcher used the wrong pattern: {watcher}"
        );
        ensure!(
            watcher.get("kind").and_then(Value::as_u64) == Some(7),
            "relative watcher used the wrong kind: {watcher}"
        );
    }
    Ok(())
}

/// Both Helix profiles advertise `window.workDoneProgress`, so the shipped
/// server legitimately issues `window/workDoneProgress/create` for its
/// workspace index. A real Helix client answers that request; leaving it
/// unanswered would diverge from the profile being replayed and would leave an
/// unconsumed server request on the transport at shutdown.
fn settle_progress_creates(client: &mut RealProcessClient) -> Result<()> {
    while let Ok(request) =
        client.receive_server_request("window/workDoneProgress/create", ABSENCE_TIMEOUT)
    {
        let id = request.get("id").cloned().context("progress create request missing id")?;
        client.respond(id, Value::Null)?;
    }
    Ok(())
}

fn clean_shutdown(client: &mut RealProcessClient, id: i64) -> Result<()> {
    settle_progress_creates(client)?;
    let response = client.request(json!(id), "shutdown", Value::Null, PROCESS_TIMEOUT)?;
    ensure!(response.get("error").is_none(), "shutdown failed: {response}");
    ensure!(response.get("result").is_some_and(Value::is_null));
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(PROCESS_TIMEOUT)?;
    ensure!(status.success(), "perllsp exited unsuccessfully: {status}");
    client.assert_transport_clean()
}

fn replay_profile(raw: &str, expect_relative_watchers: bool) -> Result<()> {
    let profile = profile(raw)?;
    let workspace = tempfile::tempdir().context("create Helix profile workspace")?;
    let params = initialize_params(&profile, workspace.path())?;
    let root_uri = params["rootUri"].as_str().context("rootUri missing")?.to_string();

    let mut client = RealProcessClient::spawn_exact()?;
    let response = client.request(json!(1), "initialize", params, PROCESS_TIMEOUT)?;
    assert_initialize_response(&response, 1)?;
    client.notify("initialized", json!({}))?;
    settle_workspace_configuration(&mut client)?;
    let watchers = receive_watcher_registration(&mut client)?;
    if expect_relative_watchers {
        assert_relative_watchers(&watchers, &root_uri)?;
    } else {
        assert_string_watchers(&watchers)?;
    }
    clean_shutdown(&mut client, 2)
}

#[test]
fn checked_profiles_preserve_the_exact_helix_cohort_boundary() -> Result<()> {
    let stable = profile(STABLE_PROFILE)?;
    let master = profile(MASTER_PROFILE)?;

    ensure!(
        stable.pointer("/subject/source_sha").and_then(Value::as_str)
            == Some("a05c151bb6e8e9c65ec390b0ae2afe7a5efd619b")
    );
    ensure!(
        master.pointer("/subject/source_sha").and_then(Value::as_str)
            == Some("079a789e8cb08ead67f19e1971a1b7438b37354b")
    );
    ensure!(stable.pointer("/initialize_params/capabilities/textDocument/diagnostic").is_none());
    ensure!(master.pointer("/initialize_params/capabilities/textDocument/diagnostic").is_some());
    // `workspace.diagnostic` is singular on purpose. LSP 3.17 names this client
    // capability `workspace.diagnostics`, but `helix-lsp-types` declares it as
    // `pub diagnostic: Option<DiagnosticWorkspaceClientCapabilities>` under
    // `#[serde(rename_all = "camelCase")]` with no per-field rename, so real
    // Helix puts it on the wire as `diagnostic`. These fixtures mirror Helix,
    // not the spec; do not "correct" this to the plural spelling.
    ensure!(stable.pointer("/initialize_params/capabilities/workspace/diagnostic").is_none());
    ensure!(master.pointer("/initialize_params/capabilities/workspace/diagnostic").is_some());
    ensure!(
        stable
            .pointer("/initialize_params/capabilities/workspace/didChangeWatchedFiles/relativePatternSupport")
            .and_then(Value::as_bool)
            == Some(false)
    );
    ensure!(
        master
            .pointer("/initialize_params/capabilities/workspace/didChangeWatchedFiles/relativePatternSupport")
            .and_then(Value::as_bool)
            == Some(true)
    );
    ensure!(
        stable
            .pointer("/initialize_params/capabilities/workspace/fileOperations/willCreate")
            .is_none()
    );
    ensure!(
        master.pointer("/initialize_params/capabilities/workspace/fileOperations/willCreate")
            == Some(&Value::Bool(true))
    );
    Ok(())
}

#[test]
fn released_stable_profile_uses_push_shape_and_string_watchers() -> Result<()> {
    replay_profile(STABLE_PROFILE, false)
}

#[test]
fn current_master_profile_uses_pull_shape_and_relative_watchers() -> Result<()> {
    replay_profile(MASTER_PROFILE, true)
}

#[test]
fn explicit_false_dynamic_watcher_registration_fails_closed() -> Result<()> {
    let mut stable = profile(STABLE_PROFILE)?;
    stable["initialize_params"]["capabilities"]["workspace"]["didChangeWatchedFiles"]["dynamicRegistration"] =
        Value::Bool(false);
    let workspace = tempfile::tempdir().context("create sparse-profile workspace")?;
    let params = initialize_params(&stable, workspace.path())?;

    let mut client = RealProcessClient::spawn_exact()?;
    let response = client.request(json!(11), "initialize", params, PROCESS_TIMEOUT)?;
    assert_initialize_response(&response, 11)?;
    client.notify("initialized", json!({}))?;
    settle_workspace_configuration(&mut client)?;
    ensure!(
        client.receive_server_request("client/registerCapability", ABSENCE_TIMEOUT).is_err(),
        "watchers were registered after dynamicRegistration=false"
    );
    clean_shutdown(&mut client, 12)
}

#[test]
fn malformed_relative_pattern_support_falls_back_to_string_globs() -> Result<()> {
    let mut master = profile(MASTER_PROFILE)?;
    master["initialize_params"]["capabilities"]["workspace"]["didChangeWatchedFiles"]["relativePatternSupport"] =
        Value::String("true".to_string());
    let workspace = tempfile::tempdir().context("create malformed-profile workspace")?;
    let params = initialize_params(&master, workspace.path())?;

    let mut client = RealProcessClient::spawn_exact()?;
    let response = client.request(json!(21), "initialize", params, PROCESS_TIMEOUT)?;
    assert_initialize_response(&response, 21)?;
    client.notify("initialized", json!({}))?;
    settle_workspace_configuration(&mut client)?;
    let watchers = receive_watcher_registration(&mut client)?;
    assert_string_watchers(&watchers)?;
    clean_shutdown(&mut client, 22)
}

#[test]
fn workspace_configuration_false_emits_no_configuration_request() -> Result<()> {
    let mut stable = profile(STABLE_PROFILE)?;
    stable["initialize_params"]["capabilities"]["workspace"]["configuration"] = Value::Bool(false);
    let workspace = tempfile::tempdir().context("create no-configuration workspace")?;
    let params = initialize_params(&stable, workspace.path())?;

    let mut client = RealProcessClient::spawn_exact()?;
    let response = client.request(json!(31), "initialize", params, PROCESS_TIMEOUT)?;
    assert_initialize_response(&response, 31)?;
    client.notify("initialized", json!({}))?;
    let watchers = receive_watcher_registration(&mut client)?;
    assert_string_watchers(&watchers)?;
    ensure!(
        client.receive_server_request("workspace/configuration", ABSENCE_TIMEOUT).is_err(),
        "workspace/configuration was emitted after configuration=false"
    );
    clean_shutdown(&mut client, 32)
}
