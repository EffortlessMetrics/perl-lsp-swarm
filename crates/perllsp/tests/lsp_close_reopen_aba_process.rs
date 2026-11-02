//! Exact-process proof for close/reopen ABA stale-work rejection and open-buffer
//! authority (#7079). Deterministic parse-worker barriers are driven through
//! `$/test/parseWorkerBarrier`, compiled only with `expose_lsp_test_api`.
#![cfg(feature = "expose_lsp_test_api")]
#![deny(clippy::map_err_ignore)]

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, Instant};
use url::Url;

const TIMEOUT: Duration = Duration::from_secs(15);

fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|()| anyhow::anyhow!("could not convert path to file URI: {}", path.display()))
}

fn request(client: &mut RealProcessClient, id: &str, method: &str, params: Value) -> Result<Value> {
    let id = json!(id);
    let response = client.request(id.clone(), method, params, TIMEOUT)?;
    ensure!(response.get("id") == Some(&id), "response id mismatch: {response}");
    ensure!(response.get("error").is_none(), "{method} failed: {response}");
    response.get("result").cloned().with_context(|| format!("{method} omitted result"))
}

fn parse_worker_barrier(
    client: &mut RealProcessClient,
    action: &str,
    uri: Option<&str>,
    generation: Option<u32>,
) -> Result<Value> {
    let mut params = json!({ "action": action });
    if let Some(uri) = uri {
        params["uri"] = json!(uri);
    }
    if let Some(generation) = generation {
        params["generation"] = json!(generation);
    }
    request(client, "barrier", "$/test/parseWorkerBarrier", params)
}

fn initialize_workspace(client: &mut RealProcessClient) -> Result<String> {
    let root_uri = file_uri(client.workspace_path())?;
    let result = request(
        client,
        "initialize",
        "initialize",
        json!({
            "processId": null,
            "clientInfo": { "name": "close-reopen-aba", "version": "1" },
            "rootUri": root_uri,
            "workspaceFolders": [{ "uri": root_uri, "name": "fixture" }],
            "capabilities": {
                "general": { "positionEncodings": ["utf-16"] },
                "textDocument": {
                    "diagnostic": { "dynamicRegistration": false, "relatedDocumentSupport": true }
                },
                "workspace": { "didChangeWatchedFiles": { "dynamicRegistration": false } }
            }
        }),
    )?;
    ensure!(result.get("capabilities").is_some(), "initialize omitted capabilities");
    client.notify("initialized", json!({}))?;
    Ok(root_uri)
}

fn wait_for_live_semantic_tokens(
    client: &mut RealProcessClient,
    uri: &str,
    id_prefix: &str,
) -> Result<()> {
    let deadline = Instant::now() + TIMEOUT;
    let mut attempt = 0u32;
    loop {
        let id = format!("{id_prefix}-{attempt}");
        let result = request(
            client,
            &id,
            "textDocument/semanticTokens/full",
            json!({ "textDocument": { "uri": uri } }),
        )?;
        let live = result.get("resultId").and_then(Value::as_str).is_some()
            && result.get("data").and_then(Value::as_array).is_some_and(|data| !data.is_empty());
        if live {
            return Ok(());
        }
        ensure!(Instant::now() < deadline, "semantic tokens never became current: {result}");
        attempt = attempt.saturating_add(1);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn document_symbol_names(
    client: &mut RealProcessClient,
    uri: &str,
    id: &str,
) -> Result<Vec<String>> {
    let result = request(
        client,
        id,
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": uri } }),
    )?;
    let mut names = Vec::new();
    collect_symbol_names(&result, &mut names);
    Ok(names)
}

fn collect_symbol_names(value: &Value, names: &mut Vec<String>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_symbol_names(item, names);
            }
        }
        Value::Object(object) => {
            if let Some(name) = object.get("name").and_then(Value::as_str) {
                names.push(name.to_string());
            }
            if let Some(children) = object.get("children") {
                collect_symbol_names(children, names);
            }
        }
        _ => {}
    }
}

fn finish(client: &mut RealProcessClient) -> Result<()> {
    ensure!(request(client, "shutdown", "shutdown", Value::Null)?.is_null());
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(TIMEOUT)?.success(), "{}", client.stderr_tail());
    client.assert_transport_clean()
}

/// Held generation-2 parse work must not publish into a reopened document instance.
#[test]
fn held_parse_never_publishes_after_close_reopen() -> Result<()> {
    let script = "aba_lifecycle.pl";
    let v1 = "package Aba;\nsub generation_one { 1 }\n1;\n";
    let v2 = "package Aba;\nsub generation_two { 2 }\n1;\n";
    let reopened = "package Aba;\nsub reopened_instance { 9 }\n1;\n";

    let mut client = RealProcessClient::spawn_exact()?;
    let path = client.workspace_path().join(script);
    std::fs::write(&path, v1)?;
    let uri = file_uri(&path)?;
    initialize_workspace(&mut client)?;

    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri, "languageId": "perl", "version": 1, "text": v1
            }
        }),
    )?;
    wait_for_live_semantic_tokens(&mut client, &uri, "open-v1")?;
    let baseline = document_symbol_names(&mut client, &uri, "symbols-v1")?;
    ensure!(
        baseline.iter().any(|name| name.contains("generation_one")),
        "baseline symbol missing: {baseline:?}"
    );

    parse_worker_barrier(&mut client, "arm", Some(&uri), Some(2))?;
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": v2 }]
        }),
    )?;
    parse_worker_barrier(&mut client, "waitPaused", None, None)?;

    let paused_tokens = request(
        &mut client,
        "tokens-paused",
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": uri } }),
    )?;
    ensure!(
        paused_tokens.get("resultId").and_then(Value::as_str).is_none()
            || paused_tokens.get("data").and_then(Value::as_array).is_some_and(Vec::is_empty),
        "held parse must not publish current semantic tokens: {paused_tokens}"
    );

    client.notify("textDocument/didClose", json!({ "textDocument": { "uri": uri } }))?;
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri, "languageId": "perl", "version": 1, "text": reopened
            }
        }),
    )?;
    wait_for_live_semantic_tokens(&mut client, &uri, "reopened-live")?;
    let pre_release = document_symbol_names(&mut client, &uri, "symbols-pre-release")?;
    ensure!(
        pre_release.iter().any(|name| name.contains("reopened_instance")),
        "reopened instance must be live before stale release: {pre_release:?}"
    );

    parse_worker_barrier(&mut client, "release", None, None)?;
    std::thread::sleep(Duration::from_millis(50));
    let metrics = parse_worker_barrier(&mut client, "metrics", None, None)?;
    ensure!(
        metrics.get("jobsRejectedStale").and_then(Value::as_u64).unwrap_or(0) >= 1,
        "stale held job must be rejected: {metrics}"
    );

    let after = document_symbol_names(&mut client, &uri, "symbols-after-release")?;
    ensure!(
        after.iter().any(|name| name.contains("reopened_instance")),
        "reopened symbols must remain authoritative: {after:?}"
    );
    ensure!(
        !after.iter().any(|name| name.contains("generation_two")),
        "stale held generation-two parse must not appear after release: {after:?}"
    );
    ensure!(
        !after.iter().any(|name| name.contains("generation_one")),
        "closed generation-one facts must not return: {after:?}"
    );

    finish(&mut client)
}

/// CRLF full-sync under UTF-16 negotiation (#2298 integrated via buffer bytes).
#[test]
fn utf16_crlf_full_sync_publishes_current_generation() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let uri = "file:///utf16-crlf-ranged.pl";
    initialize_workspace(&mut client)?;
    let open_text = "package Range;\nsub seed { 1 }\n";
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri, "languageId": "perl", "version": 1, "text": open_text
            }
        }),
    )?;
    wait_for_live_semantic_tokens(&mut client, uri, "crlf-open")?;
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{
                "text": "package Range;\r\nsub seed { 1 }\r\nsub add_marker { 2 }\r\n"
            }]
        }),
    )?;
    wait_for_live_semantic_tokens(&mut client, uri, "crlf-full")?;
    let after_full = document_symbol_names(&mut client, uri, "crlf-after-full")?;
    ensure!(
        after_full.iter().any(|name| name.contains("add_marker")),
        "CRLF full-sync must publish current symbols: {after_full:?}"
    );
    finish(&mut client)
}

#[test]
fn did_save_with_and_without_replacement_text() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let path = client.workspace_path().join("save_handoff.pl");
    let before = "package Save;\nsub before_save { 1 }\n1;\n";
    std::fs::write(&path, before)?;
    let uri = file_uri(&path)?;
    initialize_workspace(&mut client)?;
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": { "uri": uri, "languageId": "perl", "version": 1, "text": before }
        }),
    )?;
    wait_for_live_semantic_tokens(&mut client, &uri, "save-open")?;

    client.notify("textDocument/didSave", json!({ "textDocument": { "uri": uri } }))?;
    let unchanged = document_symbol_names(&mut client, &uri, "save-unchanged")?;
    ensure!(
        unchanged.iter().any(|name| name.contains("before_save")),
        "save without text keeps current buffer facts: {unchanged:?}"
    );

    let after = "package Save;\nsub after_save { 2 }\n1;\n";
    client
        .notify("textDocument/didSave", json!({ "textDocument": { "uri": uri }, "text": after }))?;
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let names = document_symbol_names(&mut client, &uri, "save-replaced")?;
        if names.iter().any(|name| name.contains("after_save"))
            && !names.iter().any(|name| name.contains("before_save"))
        {
            break;
        }
        ensure!(Instant::now() < deadline, "save replacement never published: {names:?}");
        std::thread::sleep(Duration::from_millis(10));
    }

    finish(&mut client)
}

#[test]
fn unsaved_open_buffer_stays_authoritative_over_disk_and_watcher() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let path = client.workspace_path().join("buffer_authority.pl");
    let v1 = "package Buffer;\nsub buffer_v1 { 1 }\n1;\n";
    std::fs::write(&path, v1)?;
    let uri = file_uri(&path)?;
    initialize_workspace(&mut client)?;
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": { "uri": uri, "languageId": "perl", "version": 1, "text": v1 }
        }),
    )?;
    wait_for_live_semantic_tokens(&mut client, &uri, "buffer-open")?;

    let v2 = "package Buffer;\nsub buffer_v2 { 2 }\n1;\n";
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": v2 }]
        }),
    )?;
    wait_for_live_semantic_tokens(&mut client, &uri, "buffer-edit")?;

    let v3 = "package Buffer;\nsub disk_v3 { 3 }\n1;\n";
    std::fs::write(&path, v3)?;
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": uri, "type": 2 }] }),
    )?;
    std::thread::sleep(Duration::from_millis(100));

    let names = document_symbol_names(&mut client, &uri, "buffer-after-watcher")?;
    ensure!(
        names.iter().any(|name| name.contains("buffer_v2")),
        "open buffer must remain authoritative: {names:?}"
    );
    ensure!(
        !names.iter().any(|name| name.contains("disk_v3")),
        "disk/watcher must not overwrite unsaved buffer symbols: {names:?}"
    );

    client.notify("textDocument/didClose", json!({ "textDocument": { "uri": uri } }))?;
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": { "uri": uri, "languageId": "perl", "version": 1, "text": v3 }
        }),
    )?;
    wait_for_live_semantic_tokens(&mut client, &uri, "buffer-reopen-disk")?;
    let after_close = document_symbol_names(&mut client, &uri, "buffer-after-close")?;
    ensure!(
        after_close.iter().any(|name| name.contains("disk_v3")),
        "after close, disk/workspace authority must be observable: {after_close:?}"
    );

    finish(&mut client)
}

#[test]
fn uri_aliases_normalize_to_one_open_document_owner() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let dir = client.workspace_path().join("fixtures space#percent%25");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join("alias.pl");
    std::fs::write(&file, "package Alias;\nsub alias_symbol { 1 }\n1;\n")?;
    let canonical = file_uri(&file)?;
    let alias = canonical.replacen("file://", "file://localhost", 1);
    ensure!(alias != canonical, "fixture must expose localhost alias spelling");
    initialize_workspace(&mut client)?;

    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": canonical, "languageId": "perl", "version": 1,
                "text": "package Alias;\nsub alias_symbol { 1 }\n1;\n"
            }
        }),
    )?;
    wait_for_live_semantic_tokens(&mut client, &canonical, "alias-open")?;

    let alias_names = document_symbol_names(&mut client, &alias, "alias-spelling")?;
    ensure!(
        alias_names.iter().any(|name| name.contains("alias_symbol")),
        "percent-encoded alias must resolve to the open document: {alias_names:?}"
    );
    let canonical_names = document_symbol_names(&mut client, &canonical, "alias-canonical")?;
    ensure!(
        canonical_names == alias_names,
        "alias spellings must share one owner: canonical={canonical_names:?} alias={alias_names:?}"
    );

    client.notify("textDocument/didClose", json!({ "textDocument": { "uri": canonical } }))?;
    let closed_id = json!("tokens-closed-alias");
    let closed = client.request(
        closed_id.clone(),
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": alias } }),
        TIMEOUT,
    )?;
    ensure!(closed.get("id") == Some(&closed_id));
    ensure!(
        closed.get("error").is_some(),
        "closed alias must not retain alternate stale authority: {closed}"
    );

    finish(&mut client)
}
