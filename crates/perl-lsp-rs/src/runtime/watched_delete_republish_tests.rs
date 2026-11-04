//! Watched-file / file-operation DELETED republish contract (#17332).
//!
//! Internal proof for the external-mutation republish surface: when a
//! dependency file is deleted (watched `didChangeWatchedFiles` DELETED, or a
//! client `workspace/didDeleteFiles` event), eviction drops the dependency's
//! indexed facts, so open consumer buffers whose diagnostics resolved through
//! it must receive a fresh `textDocument/publishDiagnostics` push with no
//! buffer edit in between.
#![cfg(feature = "workspace")]

use super::{LspServer, WorkspaceFolderState};
use parking_lot::Mutex;
use perl_lsp_rs_core::transport::framing::ContentLengthFramer;
use serde_json::{Value, json};
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
struct OutputCapture {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl OutputCapture {
    fn messages(&self) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        let bytes = self.buffer.lock().clone();
        let mut framer = ContentLengthFramer::new();
        framer.push(&bytes);
        let mut messages = Vec::new();
        while let Some(body) = framer.try_next()? {
            messages.push(serde_json::from_slice::<Value>(&body)?);
        }
        Ok(messages)
    }
}

impl Write for OutputCapture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.lock().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn server_with_output_capture() -> (LspServer, OutputCapture) {
    let output = OutputCapture::default();
    let server = LspServer::with_output(Arc::new(Mutex::new(
        Box::new(output.clone()) as Box<dyn Write + Send>
    )));
    (server, output)
}

/// Shared fixture for the #17332 regression tests: a workspace whose open
/// consumer `app.pl` resolves `use Local::Alpha` through `lib/Local/Alpha.pm`,
/// mirroring the issue's fixture sequence. Returns the capture buffer, the
/// consumer URI, the dependency URI and path.
#[cfg(feature = "workspace")]
fn open_consumer_fixture() -> Result<
    (LspServer, OutputCapture, String, String, std::path::PathBuf, tempfile::TempDir),
    Box<dyn std::error::Error>,
> {
    open_consumer_fixture_with_dependency(true)
}

/// Fixture variant: with `write_dependency = false` the consumer starts
/// stale (the module it uses does not exist yet), for CREATED-event tests.
#[cfg(feature = "workspace")]
fn open_consumer_fixture_with_dependency(
    write_dependency: bool,
) -> Result<
    (LspServer, OutputCapture, String, String, std::path::PathBuf, tempfile::TempDir),
    Box<dyn std::error::Error>,
> {
    let (server, output) = server_with_output_capture();
    let dir = tempfile::tempdir()?;
    std::fs::create_dir_all(dir.path().join("lib").join("Local"))?;
    let alpha_path = dir.path().join("lib").join("Local").join("Alpha.pm");
    if write_dependency {
        std::fs::write(&alpha_path, "package Local::Alpha;\nsub helper { 1 }\n1;\n")?;
    }

    let folder_uri =
        url::Url::from_directory_path(dir.path()).map_err(|_| "invalid folder URI")?.to_string();
    let alpha_uri =
        url::Url::from_file_path(&alpha_path).map_err(|_| "invalid Alpha URI")?.to_string();
    let app_uri = url::Url::from_file_path(dir.path().join("app.pl"))
        .map_err(|_| "invalid app URI")?
        .to_string();
    // Explicit non-probing include state keeps the module resolution of the
    // fixture deterministic: `Local::Alpha` exists exactly while the fixture
    // file does.
    let mut config = perl_lsp_rs_core::config::WorkspaceConfig::default();
    config.include_paths = vec!["lib".to_string()];
    config.use_perl5lib = false;
    config.use_system_inc = false;
    server.workspace_folders.lock().push(
        WorkspaceFolderState::new(folder_uri)
            .with_path(dir.path().to_path_buf())
            .with_effective_workspace_config(config),
    );

    let source = "use strict;\nuse warnings;\nuse Local::Alpha;\n\
                  print Local::Alpha::helper(), \"\\n\";\n";
    server.test_apply_did_open(&app_uri, source, 1)?;
    server.publish_diagnostics(&app_uri);

    // The TempDir guard travels with the fixture: dropping it would delete
    // the on-disk dependency the test still needs to remove explicitly.
    Ok((server, output, app_uri, alpha_uri, alpha_path, dir))
}

/// #17332 regression: a watched `didChangeWatchedFiles` DELETED event for a
/// dependency must push fresh `textDocument/publishDiagnostics` to the
/// still-open consumer buffer. Before the repair the eviction removed the
/// dependency's index facts but nothing republished, so the editor kept the
/// stale clean display until the user edited the buffer — even though pull
/// diagnostics already returned `Module 'Local::Alpha' not found`.
#[cfg(feature = "workspace")]
#[test]
fn watched_dependency_delete_republishes_open_consumer_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, output, app_uri, alpha_uri, alpha_path, _dir) = open_consumer_fixture()?;

    // The publish URI spelling is the document map key (drive letter
    // lowercased on Windows), which can differ from the didOpen spelling.
    let publish_keys = [app_uri.clone(), server.normalize_uri_key(&app_uri)];
    let is_publish_for = |message: &Value| -> bool {
        message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
            && message
                .pointer("/params/uri")
                .and_then(Value::as_str)
                .is_some_and(|uri| publish_keys.iter().any(|key| key == uri))
    };
    let has_pl701 = |message: &Value| -> bool {
        message.pointer("/params/diagnostics").and_then(Value::as_array).is_some_and(
            |diagnostics| {
                diagnostics.iter().any(|diagnostic| diagnostic.get("code") == Some(&json!("PL701")))
            },
        )
    };

    // Baseline: with the dependency on disk the open consumer resolves, so
    // the published set must be clean of PL701 before the delete arrives.
    let deadline = Instant::now() + Duration::from_secs(5);
    let baseline_len = loop {
        let messages = output.messages()?;
        let clean = messages.iter().any(|message| is_publish_for(message) && !has_pl701(message));
        if clean {
            break messages.len();
        }
        assert!(
            Instant::now() < deadline,
            "baseline publish for the consumer never settled clean: {messages:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    };

    // Delete the dependency on disk, then deliver the watched DELETED event.
    std::fs::remove_file(&alpha_path)?;
    server.handle_did_change_watched_files(Some(json!({
        "changes": [{ "uri": alpha_uri, "type": 3 }]
    })))?;

    // The open consumer must receive a fresh publish carrying the
    // module-not-found diagnostic, with no buffer edit in between.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let messages = output.messages()?;
        let republished = messages
            .iter()
            .skip(baseline_len)
            .any(|message| is_publish_for(message) && has_pl701(message));
        if republished {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "watched DELETED never republished the open consumer: tail {messages:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

/// #17332 regression, symmetric delivery path: a client
/// `workspace/didDeleteFiles` file-operation event for a dependency must
/// also push fresh diagnostics to the still-open consumer buffer.
#[cfg(feature = "workspace")]
#[test]
fn file_operation_delete_republishes_open_consumer_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, output, app_uri, alpha_uri, alpha_path, _dir) = open_consumer_fixture()?;

    // The publish URI spelling is the document map key (drive letter
    // lowercased on Windows), which can differ from the didOpen spelling.
    let publish_keys = [app_uri.clone(), server.normalize_uri_key(&app_uri)];
    let is_publish_for = |message: &Value| -> bool {
        message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
            && message
                .pointer("/params/uri")
                .and_then(Value::as_str)
                .is_some_and(|uri| publish_keys.iter().any(|key| key == uri))
    };
    let has_pl701 = |message: &Value| -> bool {
        message.pointer("/params/diagnostics").and_then(Value::as_array).is_some_and(
            |diagnostics| {
                diagnostics.iter().any(|diagnostic| diagnostic.get("code") == Some(&json!("PL701")))
            },
        )
    };

    let deadline = Instant::now() + Duration::from_secs(5);
    let baseline_len = loop {
        let messages = output.messages()?;
        let clean = messages.iter().any(|message| is_publish_for(message) && !has_pl701(message));
        if clean {
            break messages.len();
        }
        assert!(
            Instant::now() < deadline,
            "baseline publish for the consumer never settled clean: {messages:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    };

    std::fs::remove_file(&alpha_path)?;
    server.handle_did_delete_files(Some(json!({
        "files": [{ "uri": alpha_uri }]
    })))?;

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let messages = output.messages()?;
        let republished = messages
            .iter()
            .skip(baseline_len)
            .any(|message| is_publish_for(message) && has_pl701(message));
        if republished {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "didDeleteFiles never republished the open consumer: tail {messages:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

// -------------------------------------------------------------------------
// CREATED/CHANGED batch tail (#17332 remainder)
//
// The DELETED tail above republishes open consumers after eviction, but a
// dependency that appears (restore/checkout) or changes (package rename)
// earned no tail: push consumers kept stale findings and pull consumers
// were never asked to re-pull. These tests pin the mirrored tail on both
// delivery exits — the immediate fallback in `handle_did_change_watched_files`
// and the debounced `handle_watched_file_batch`.
// -------------------------------------------------------------------------

#[cfg(feature = "workspace")]
fn is_publish_for(message: &Value, publish_keys: &[String]) -> bool {
    message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
        && message
            .pointer("/params/uri")
            .and_then(Value::as_str)
            .is_some_and(|uri| publish_keys.iter().any(|key| key == uri))
}

#[cfg(feature = "workspace")]
fn message_has_pl701(message: &Value) -> bool {
    message.pointer("/params/diagnostics").and_then(Value::as_array).is_some_and(|diagnostics| {
        diagnostics.iter().any(|diagnostic| diagnostic.get("code") == Some(&json!("PL701")))
    })
}

/// Wait until a publish for the consumer at or after `from` matches the
/// requested PL701 presence; returns the message count at that point.
#[cfg(feature = "workspace")]
fn wait_for_publish(
    output: &OutputCapture,
    publish_keys: &[String],
    from: usize,
    want_pl701: bool,
    what: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let messages = output.messages()?;
        let found = messages.iter().skip(from).any(|message| {
            is_publish_for(message, publish_keys) && { message_has_pl701(message) == want_pl701 }
        });
        if found {
            return Ok(messages.len());
        }
        if Instant::now() >= deadline {
            return Err(format!("{what}: tail {messages:?}").into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(feature = "workspace")]
#[test]
fn watched_dependency_create_clears_stale_consumer_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, output, app_uri, alpha_uri, alpha_path, _dir) =
        open_consumer_fixture_with_dependency(false)?;
    let publish_keys = [app_uri.clone(), server.normalize_uri_key(&app_uri)];

    // Baseline: without the dependency the open consumer is stale (PL701).
    let baseline_len = wait_for_publish(&output, &publish_keys, 0, true, "baseline never stale")?;

    // Create the dependency on disk, then deliver the watched CREATED event.
    std::fs::write(&alpha_path, "package Local::Alpha;\nsub helper { 1 }\n1;\n")?;
    server.handle_did_change_watched_files(Some(json!({
        "changes": [{ "uri": alpha_uri, "type": 1 }]
    })))?;

    wait_for_publish(
        &output,
        &publish_keys,
        baseline_len,
        false,
        "watched CREATED never cleared the open consumer",
    )?;
    Ok(())
}

#[cfg(feature = "workspace")]
#[test]
fn watched_dependency_change_clears_stale_consumer_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    // PL701 module resolution is filesystem-based, so a CHANGED event can
    // only change a consumer's findings when the file's existence changed;
    // watchers commonly report a newly appeared file as Changed (created
    // before watcher registration, or coalesced by the client).
    let (server, output, app_uri, alpha_uri, alpha_path, _dir) =
        open_consumer_fixture_with_dependency(false)?;
    let publish_keys = [app_uri.clone(), server.normalize_uri_key(&app_uri)];

    // Baseline: without the dependency the open consumer is stale (PL701).
    let baseline_len = wait_for_publish(&output, &publish_keys, 0, true, "baseline never stale")?;

    // Create the dependency on disk, then deliver the watched CHANGED event.
    std::fs::write(&alpha_path, "package Local::Alpha;\nsub helper { 1 }\n1;\n")?;
    server.handle_did_change_watched_files(Some(json!({
        "changes": [{ "uri": alpha_uri, "type": 2 }]
    })))?;

    wait_for_publish(
        &output,
        &publish_keys,
        baseline_len,
        false,
        "watched CHANGED never cleared the open consumer",
    )?;
    Ok(())
}

#[cfg(feature = "workspace")]
#[test]
fn watched_batch_create_clears_stale_consumer_diagnostics() -> Result<(), Box<dyn std::error::Error>>
{
    let (server, output, app_uri, alpha_uri, alpha_path, _dir) =
        open_consumer_fixture_with_dependency(false)?;
    let publish_keys = [app_uri.clone(), server.normalize_uri_key(&app_uri)];

    let baseline_len = wait_for_publish(&output, &publish_keys, 0, true, "baseline never stale")?;

    // The debounced exit: the debouncer delivers CREATED/CHANGED URIs here.
    std::fs::write(&alpha_path, "package Local::Alpha;\nsub helper { 1 }\n1;\n")?;
    server.handle_watched_file_batch(vec![alpha_uri]);

    wait_for_publish(
        &output,
        &publish_keys,
        baseline_len,
        false,
        "watched batch never cleared the open consumer",
    )?;
    Ok(())
}

#[cfg(feature = "workspace")]
#[test]
fn watched_create_requests_pull_diagnostic_refresh() -> Result<(), Box<dyn std::error::Error>> {
    let (server, output, _app_uri, alpha_uri, alpha_path, _dir) =
        open_consumer_fixture_with_dependency(false)?;
    // Mirror the existing watched-delete pull fixture: a refresh request is
    // only sent to a client that advertised refresh support.
    server.client_capabilities.lock().diagnostic_refresh_support = true;
    server.client_supports_pull_diags.store(true, std::sync::atomic::Ordering::Relaxed);
    server.initialized.store(true, std::sync::atomic::Ordering::Relaxed);

    std::fs::write(&alpha_path, "package Local::Alpha;\nsub helper { 1 }\n1;\n")?;
    server.handle_did_change_watched_files(Some(json!({
        "changes": [{ "uri": alpha_uri, "type": 1 }]
    })))?;

    // A pull client receives no push; it must be asked to re-pull.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let messages = output.messages()?;
        let refreshed = messages.iter().any(|message| {
            message.get("method").and_then(Value::as_str) == Some("workspace/diagnostic/refresh")
        });
        if refreshed {
            return Ok(());
        }
        assert!(
            Instant::now() < deadline,
            "watched CREATED never requested a pull refresh: {messages:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

// -------------------------------------------------------------------------
// Save / file-operation tails (#18116)
//
// The watched-file tails above republish open consumers after external
// mutation. Two dependency-change seams earned no tail: saving an open
// dependency (no watched event is guaranteed to echo the save) and
// `didCreateFiles`/`didRenameFiles` (whose `refresh_all` tail only sends
// pull-refresh requests, so push consumers never republish).
// -------------------------------------------------------------------------

// The marker is fixture-only, not an outbound production protocol method.
#[cfg(test)]
mod output_boundary {
    use super::*;

    /// A FIFO marker drains the outbound writer after synchronous diagnostic
    /// publication. These fixtures install a parse worker, never a debounce scheduler.
    pub(super) fn save_output_boundary(
        server: &LspServer,
        output: &OutputCapture,
        phase: &str,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        server.notify("test/save-proof-boundary", json!({ "phase": phase }))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let messages = output.messages()?;
            if let Some(position) = messages.iter().position(|message| {
                message.get("method").and_then(Value::as_str) == Some("test/save-proof-boundary")
                    && message.pointer("/params/phase").and_then(Value::as_str) == Some(phase)
            }) {
                return Ok(position + 1);
            }
            if Instant::now() >= deadline {
                return Err(format!("outbound boundary {phase} never arrived").into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

use output_boundary::save_output_boundary;

fn settle_save_worker(server: &LspServer, key: &str) -> Result<(), Box<dyn std::error::Error>> {
    let worker = server.parse_worker().ok_or("parse worker must be installed")?;
    if !worker.wait_until_settled(key, Duration::from_secs(5)) {
        return Err("save parse worker did not settle".into());
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.pending_index_tasks() != 0 {
        if Instant::now() >= deadline {
            return Err("save background indexing did not settle".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

struct ReleaseSaveSideEffects<'a>(&'a LspServer);

impl Drop for ReleaseSaveSideEffects<'_> {
    fn drop(&mut self) {
        self.0.test_parse_worker_release_side_effect_barrier();
    }
}

const SAVE_ROLE_ORIGINAL: &str = "package Save::RoleA;\nuse Role::Tiny;\nsub original { 1 }\n1;\n";
const SAVE_ROLE_CONFLICT: &str =
    "package Save::RoleA;\nuse Role::Tiny;\nsub shared_method { 1 }\n1;\n";

fn open_save_role_fixture() -> Result<
    (Arc<LspServer>, OutputCapture, String, String, tempfile::TempDir),
    Box<dyn std::error::Error>,
> {
    let (server, output) = server_with_output_capture();
    let dir = tempfile::tempdir()?;
    let uri = |name: &str| -> Result<String, Box<dyn std::error::Error>> {
        Ok(url::Url::from_file_path(dir.path().join(name))
            .map_err(|_| "invalid role URI")?
            .to_string())
    };
    let role_uri = uri("RoleA.pm")?;
    let other_uri = uri("RoleB.pm")?;
    let consumer_uri = uri("Consumer.pm")?;
    // Open/index the roles before the consumer, using the synchronous fallback.
    server.test_apply_did_open(&role_uri, SAVE_ROLE_ORIGINAL, 1)?;
    server.test_apply_did_open(
        &other_uri,
        "package Save::RoleB;\nuse Role::Tiny;\nsub shared_method { 2 }\n1;\n",
        1,
    )?;
    server.test_apply_did_open(
        &consumer_uri,
        "package Save::Consumer;\nuse Role::Tiny::With;\nwith 'Save::RoleA', 'Save::RoleB';\n1;\n",
        1,
    )?;
    let server = Arc::new(server);
    server.install_default_parse_worker();
    let keys = [consumer_uri.clone(), server.normalize_uri_key(&consumer_uri)];
    let end = save_output_boundary(&server, &output, "role-baseline")?;
    let messages = output.messages()?;
    let baseline = messages
        .iter()
        .take(end)
        .rev()
        .find(|m| is_publish_for(m, &keys))
        .ok_or("role consumer must have a baseline publication")?;
    if message_has_pl303(baseline) {
        return Err("distinct role methods must not initially conflict".into());
    }
    Ok((server, output, consumer_uri, role_uri, dir))
}

fn message_has_pl303(message: &Value) -> bool {
    message.pointer("/params/diagnostics").and_then(Value::as_array).is_some_and(|diagnostics| {
        diagnostics.iter().any(|diagnostic| {
            diagnostic.get("code") == Some(&json!("PL303"))
                && diagnostic
                    .get("message")
                    .and_then(Value::as_str)
                    .is_some_and(|message| message.contains("shared_method"))
        })
    })
}

/// A positive cross-file finding rejects empty/stale publications and a refresh
/// computed before the dependency's new source reaches the workspace index.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dependency_save_republishes_open_consumer_diagnostics() -> Result<(), Box<dyn std::error::Error>>
{
    let (server, output, consumer_uri, role_uri, _dir) = open_save_role_fixture()?;
    let keys = [consumer_uri.clone(), server.normalize_uri_key(&consumer_uri)];
    let key = server.normalize_uri_key(&role_uri);
    let before = save_output_boundary(&server, &output, "before-saved-text")?;
    server.test_parse_worker_arm_side_effect_barrier(&key, 2);
    let release = ReleaseSaveSideEffects(&server);
    server.handle_did_save(Some(json!({
        "textDocument": { "uri": role_uri }, "text": SAVE_ROLE_CONFLICT
    })))?;
    server.test_parse_worker_wait_until_side_effects_paused();
    let index = server.coordinator().ok_or("workspace coordinator required")?.index();
    if index.indexed_generation(&key) != Some(1) {
        return Err("held saved-text effects must not touch the predecessor's index".into());
    }
    drop(release);
    settle_save_worker(&server, &key)?;
    if index.indexed_generation(&key) != Some(2) {
        return Err("released save must commit the accepted generation".into());
    }
    let end = save_output_boundary(&server, &output, "saved-text-settled")?;
    if !output
        .messages()?
        .iter()
        .take(end)
        .skip(before)
        .any(|m| is_publish_for(m, &keys) && message_has_pl303(m))
    {
        return Err("saved role conflict never reached the unchanged consumer as PL303".into());
    }
    Ok(())
}

/// Exercise the no-text path while a real edit is held between parse publication
/// and index commit; then prove already-current saves do not refresh consumers.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dependency_no_text_save_republishes_only_when_index_stale()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, output, consumer_uri, role_uri, _dir) = open_save_role_fixture()?;
    let keys = [consumer_uri.clone(), server.normalize_uri_key(&consumer_uri)];
    let key = server.normalize_uri_key(&role_uri);
    server.test_parse_worker_arm_side_effect_barrier(&key, 2);
    let release = ReleaseSaveSideEffects(&server);
    server.test_apply_did_change(&role_uri, SAVE_ROLE_CONFLICT, 2)?;
    server.test_parse_worker_wait_until_side_effects_paused();
    let index = server.coordinator().ok_or("workspace coordinator required")?.index();
    if index.indexed_generation(&key) != Some(1)
        || !server.workspace_index_stale_for_document(&role_uri)
    {
        return Err("fixture must hold generation 2 behind indexed generation 1".into());
    }
    let before = save_output_boundary(&server, &output, "before-no-text")?;
    server.handle_did_save(Some(json!({ "textDocument": { "uri": role_uri } })))?;
    if index.indexed_generation(&key) != Some(2) {
        return Err("no-text save did not reconcile the held generation".into());
    }
    let symbols = index.file_symbols(&key);
    if !symbols.iter().any(|s| s.name == "shared_method")
        || symbols.iter().any(|s| s.name == "original")
    {
        return Err("no-text save retained the predecessor's symbols".into());
    }
    let end = save_output_boundary(&server, &output, "no-text-held")?;
    if !output
        .messages()?
        .iter()
        .take(end)
        .skip(before)
        .any(|m| is_publish_for(m, &keys) && message_has_pl303(m))
    {
        return Err("no-text save must publish the new PL303 before worker release".into());
    }
    drop(release);
    settle_save_worker(&server, &key)?;
    let mut boundary = save_output_boundary(&server, &output, "no-text-settled")?;
    for (phase, params) in [
        ("no-text-again", json!({ "textDocument": { "uri": role_uri } })),
        (
            "identical-text",
            json!({ "textDocument": { "uri": role_uri }, "text": SAVE_ROLE_CONFLICT }),
        ),
    ] {
        server.handle_did_save(Some(params))?;
        let end = save_output_boundary(&server, &output, phase)?;
        if index.indexed_generation(&key) != Some(2)
            || output.messages()?.iter().take(end).skip(boundary).any(|m| is_publish_for(m, &keys))
        {
            return Err(
                format!("{phase} must leave index and consumer publications unchanged").into()
            );
        }
        boundary = end;
    }

    // Identical bytes with a newer accepted generation can return NoOp while
    // still repairing index freshness; this is not an already-current save.
    server.test_parse_worker_arm_side_effect_barrier(&key, 3);
    let release = ReleaseSaveSideEffects(&server);
    server.test_apply_did_change(&role_uri, SAVE_ROLE_CONFLICT, 3)?;
    server.test_parse_worker_wait_until_side_effects_paused();
    if index.indexed_generation(&key) != Some(2) {
        return Err("same-content edit must leave a held generation gap".into());
    }
    let before = save_output_boundary(&server, &output, "same-content-held")?;
    server.handle_did_save(Some(json!({ "textDocument": { "uri": role_uri } })))?;
    let end = save_output_boundary(&server, &output, "same-content-reconciled")?;
    if index.indexed_generation(&key) != Some(3)
        || !output.messages()?.iter().take(end).skip(before).any(|m| is_publish_for(m, &keys))
    {
        return Err(
            "NoOp content reconciliation must advance freshness and refresh consumers".into()
        );
    }
    drop(release);
    settle_save_worker(&server, &key)?;
    Ok(())
}

/// The second parse-ticket producer must retain consumer refresh when an
/// unavailable worker makes the real save path use its synchronous fallback.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dependency_save_worker_shutdown_fallback_republishes_role_conflict()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, output, consumer_uri, role_uri, _dir) = open_save_role_fixture()?;
    server.parse_worker().ok_or("parse worker required")?.request_shutdown();
    let keys = [consumer_uri.clone(), server.normalize_uri_key(&consumer_uri)];
    let key = server.normalize_uri_key(&role_uri);
    let before = save_output_boundary(&server, &output, "fallback-before")?;
    server.handle_did_save(Some(json!({
        "textDocument": { "uri": role_uri }, "text": SAVE_ROLE_CONFLICT
    })))?;
    let end = save_output_boundary(&server, &output, "fallback-after")?;
    if server
        .coordinator()
        .ok_or("workspace coordinator required")?
        .index()
        .indexed_generation(&key)
        != Some(2)
        || !output
            .messages()?
            .iter()
            .take(end)
            .skip(before)
            .any(|m| is_publish_for(m, &keys) && message_has_pl303(m))
    {
        return Err(
            "worker-shutdown fallback must commit and republish the saved role conflict".into()
        );
    }
    Ok(())
}

/// #18116 regression: renaming a file into the path an open consumer
/// resolves must clear the consumer's stale module-not-found diagnostic
/// for push clients (the `refresh_all` tail alone sends them nothing).
#[cfg(feature = "workspace")]
#[test]
fn file_operation_rename_clears_stale_consumer_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, output, app_uri, alpha_uri, alpha_path, dir) =
        open_consumer_fixture_with_dependency(false)?;
    let publish_keys = [app_uri.clone(), server.normalize_uri_key(&app_uri)];

    // Baseline: without the dependency the open consumer is stale (PL701).
    let baseline_len = wait_for_publish(&output, &publish_keys, 0, true, "baseline never stale")?;

    // Stage the module under a non-resolving name, then rename it into
    // place through the file-operation seam only.
    let staging_path = dir.path().join("lib").join("Local").join("Staging.pm");
    std::fs::write(&staging_path, "package Local::Alpha;\nsub helper { 1 }\n1;\n")?;
    let staging_uri =
        url::Url::from_file_path(&staging_path).map_err(|_| "invalid staging URI")?.to_string();
    std::fs::rename(&staging_path, &alpha_path)?;
    server.handle_did_rename_files(Some(json!({
        "files": [{ "oldUri": staging_uri, "newUri": alpha_uri }]
    })))?;

    wait_for_publish(
        &output,
        &publish_keys,
        baseline_len,
        false,
        "didRenameFiles never cleared the open consumer",
    )?;
    Ok(())
}

#[test]
fn file_operation_create_clears_stale_consumer_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, output, app_uri, alpha_uri, alpha_path, _dir) =
        open_consumer_fixture_with_dependency(false)?;
    let keys = [app_uri.clone(), server.normalize_uri_key(&app_uri)];
    let baseline = wait_for_publish(&output, &keys, 0, true, "baseline never stale")?;
    std::fs::write(&alpha_path, "package Local::Alpha;\nsub helper { 1 }\n1;\n")?;
    server.handle_did_create_files(Some(json!({ "files": [{ "uri": alpha_uri }] })))?;
    wait_for_publish(&output, &keys, baseline, false, "didCreateFiles never cleared PL701")?;
    Ok(())
}

#[test]
fn file_operations_without_usable_entries_do_not_republish_consumers()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, output, app_uri, _alpha_uri, _alpha_path, _dir) = open_consumer_fixture()?;
    let keys = [app_uri.clone(), server.normalize_uri_key(&app_uri)];
    let before = save_output_boundary(&server, &output, "empty-operations-before")?;
    for params in [json!({ "files": [] }), json!({ "files": [{}] })] {
        server.handle_did_create_files(Some(params.clone()))?;
        server.handle_did_rename_files(Some(params))?;
    }
    let end = save_output_boundary(&server, &output, "empty-operations-after")?;
    if output.messages()?.iter().take(end).skip(before).any(|m| is_publish_for(m, &keys)) {
        return Err("empty/malformed file operations must not republish the consumer".into());
    }
    Ok(())
}

#[test]
fn file_operation_create_and_rename_refresh_pull_clients() -> Result<(), Box<dyn std::error::Error>>
{
    for rename in [false, true] {
        let (mut server, output, _app_uri, alpha_uri, alpha_path, dir) =
            open_consumer_fixture_with_dependency(false)?;
        // A generous injected window makes the second-event control independent
        // of machine speed; successful invalidations must bypass this timer.
        server.refresh_controller =
            super::refresh::RefreshController::with_debounce(Duration::from_secs(60));
        server.client_capabilities.lock().diagnostic_refresh_support = true;
        server.client_supports_pull_diags.store(true, std::sync::atomic::Ordering::Relaxed);
        server.initialized.store(true, std::sync::atomic::Ordering::Relaxed);
        // The client may finish the first pull before the second event arrives.
        // A leading-edge debounce must not swallow that second invalidation.
        for attempt in 0..2 {
            let before =
                save_output_boundary(&server, &output, &format!("pull-file-before-{attempt}"))?;
            std::fs::write(
                &alpha_path,
                format!("package Local::Alpha;\nsub helper {{ {attempt} }}\n1;\n"),
            )?;
            if rename {
                let old_uri =
                    url::Url::from_file_path(dir.path().join(format!("Staging{attempt}.pm")))
                        .map_err(|_| "invalid staging URI")?
                        .to_string();
                server.handle_did_rename_files(Some(json!({
                    "files": [{ "oldUri": old_uri, "newUri": alpha_uri }]
                })))?;
            } else {
                server.handle_did_create_files(Some(json!({ "files": [{ "uri": alpha_uri }] })))?;
            }
            let end =
                save_output_boundary(&server, &output, &format!("pull-file-after-{attempt}"))?;
            let refreshes = output
                .messages()?
                .iter()
                .take(end)
                .skip(before)
                .filter(|m| {
                    m.get("method").and_then(Value::as_str) == Some("workspace/diagnostic/refresh")
                })
                .count();
            if refreshes != 1 {
                return Err(format!("file operation rename={rename}, attempt={attempt}: expected one pull refresh, got {refreshes}").into());
            }
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dependency_save_refreshes_pull_clients_after_reconciliation()
-> Result<(), Box<dyn std::error::Error>> {
    let (server, output, consumer_uri, role_uri, _dir) = open_save_role_fixture()?;
    let keys = [consumer_uri.clone(), server.normalize_uri_key(&consumer_uri)];
    let key = server.normalize_uri_key(&role_uri);
    server.client_capabilities.lock().diagnostic_refresh_support = true;
    server.client_supports_pull_diags.store(true, std::sync::atomic::Ordering::Relaxed);
    server.initialized.store(true, std::sync::atomic::Ordering::Relaxed);
    let before = save_output_boundary(&server, &output, "pull-save-before")?;
    server.handle_did_save(Some(json!({
        "textDocument": { "uri": role_uri }, "text": SAVE_ROLE_CONFLICT
    })))?;
    settle_save_worker(&server, &key)?;
    let end = save_output_boundary(&server, &output, "pull-save-after")?;
    let messages = output.messages()?;
    let tail = messages.iter().take(end).skip(before);
    if server
        .coordinator()
        .ok_or("workspace coordinator required")?
        .index()
        .indexed_generation(&key)
        != Some(2)
        || !tail.clone().any(|m| {
            m.get("method").and_then(Value::as_str) == Some("workspace/diagnostic/refresh")
        })
        || tail.clone().any(|m| is_publish_for(m, &keys))
    {
        return Err("pull save must reconcile then request refresh without pushing consumer".into());
    }
    Ok(())
}
