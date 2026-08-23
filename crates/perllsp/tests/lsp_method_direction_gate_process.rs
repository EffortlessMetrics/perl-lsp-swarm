//! Exact-process proof for checked method-direction admission (#8896).
//!
//! Drives the exact public `perllsp` binary through the strict real-process
//! client and proves, at the wire boundary:
//!
//! - normal client→server routes still work, server-generated requests are
//!   still emitted, and their responses are consumed by the #7010 path;
//! - `workspace/applyEdit` / `workspace/configuration` sent by the client are
//!   rejected with MethodNotFound (-32601) and cannot reach edit or
//!   configuration-response handling;
//! - `client/registerCapability` / `client/unregisterCapability` sent by the
//!   client are rejected instead of activating or deactivating features;
//! - wrong-direction notifications produce no response frame and no state
//!   mutation;
//! - JSON-RPC response envelopes with numeric or string ids are never mistaken
//!   for methods;
//! - custom project methods keep their declared directions.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::path::Path;
use std::thread;
use std::time::Duration;

const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);
const ABSENCE_TIMEOUT: Duration = Duration::from_millis(250);
const SYMBOL_POLL_ROUNDS: usize = 24;
const SETTLE: Duration = Duration::from_millis(300);

/// Marker a malicious `workspace/applyEdit` payload tries to plant.
const INTRUDER_MARKER: &str = "zzz_intruder_applyedit";
/// Marker applied through the legitimate `textDocument/didChange` route,
/// proving the symbol pipeline observes applied text in the same session.
const POSITIVE_CONTROL_MARKER: &str = "zzz_positive_control";

const BASE_SOURCE: &str = "use strict;\nuse warnings;\n\nsub keep_marker_alpha {\n    return 42;\n}\n\nmy $value = keep_marker_alpha();\n";

/// One initialized perllsp session against a private temp workspace holding
/// [`BASE_SOURCE`] as `main.pl`. Field order matters: the client drops before
/// the workspace directory so the process never holds files being removed.
struct Session {
    source_uri: String,
    _workspace: tempfile::TempDir,
    client: RealProcessClient,
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

fn initialize_params(root_uri: &str, configuration: bool) -> Value {
    json!({
        "processId": null,
        "clientInfo": {
            "name": "method-direction-gate-process",
            "version": "1"
        },
        "rootUri": root_uri,
        "workspaceFolders": [
            { "uri": root_uri, "name": "fixture" }
        ],
        "capabilities": {
            "workspace": {
                "configuration": configuration,
                "workspaceFolders": true
            }
        }
    })
}

impl Session {
    /// Spawn the exact binary, run the handshake, and open `main.pl`.
    ///
    /// `configuration=false` suppresses the legitimate server→client
    /// configuration pull so wrong-direction tests start from an empty
    /// non-notification pending queue.
    fn spawn(configuration: bool) -> Result<Self> {
        let workspace = tempfile::tempdir().context("create workspace fixture")?;
        let root_uri = file_uri(workspace.path());
        let source_path = workspace.path().join("main.pl");
        std::fs::write(&source_path, BASE_SOURCE)?;
        let source_uri = file_uri(&source_path);

        let mut client = RealProcessClient::spawn_exact()?;
        let response = client.request(
            json!(88000),
            "initialize",
            initialize_params(&root_uri, configuration),
            PROCESS_TIMEOUT,
        )?;
        ensure!(response.get("error").is_none(), "initialize failed: {response}");
        client.notify("initialized", json!({}))?;

        Ok(Self { source_uri, _workspace: workspace, client })
    }

    fn did_open_base(&mut self) -> Result<()> {
        self.client.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": self.source_uri.clone(),
                    "languageId": "perl",
                    "version": 1,
                    "text": BASE_SOURCE
                }
            }),
        )
    }

    /// Any successful (non-error) response proves the standard client→server
    /// route still dispatches after the gate.
    fn hover_ok(&mut self) -> Result<()> {
        let response = self.client.request(
            json!(88998),
            "textDocument/hover",
            json!({
                "textDocument": { "uri": self.source_uri.clone() },
                "position": { "line": 7, "character": 4 }
            }),
            PROCESS_TIMEOUT,
        )?;
        ensure!(response.get("error").is_none(), "hover failed: {response}");
        Ok(())
    }

    fn symbol_names(&mut self, query: &str) -> Result<Vec<String>> {
        let response = self.client.request(
            json!(88999),
            "workspace/symbol",
            json!({ "query": query }),
            PROCESS_TIMEOUT,
        )?;
        ensure!(response.get("error").is_none(), "workspace/symbol failed: {response}");
        let Some(items) = response.get("result").and_then(Value::as_array) else {
            return Ok(Vec::new());
        };
        Ok(items
            .iter()
            .filter_map(|item| item.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect())
    }

    /// Poll until `needle` appears under `query`, bounded. Absence claims are
    /// only trusted alongside this same-session positive control.
    fn wait_for_symbol(&mut self, query: &str, needle: &str) -> Result<bool> {
        for _ in 0..SYMBOL_POLL_ROUNDS {
            if self.symbol_names(query)?.iter().any(|name| name == needle) {
                return Ok(true);
            }
            thread::sleep(ABSENCE_TIMEOUT);
        }
        Ok(false)
    }

    fn change_source_full(&mut self, version: i64, text: &str) -> Result<()> {
        self.client.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": self.source_uri.clone(), "version": version },
                "contentChanges": [ { "text": text } ]
            }),
        )
    }

    fn shutdown(mut self) -> Result<()> {
        let response =
            self.client.request(json!(88997), "shutdown", json!(null), PROCESS_TIMEOUT)?;
        ensure!(response.get("error").is_none(), "shutdown returned an error: {response}");
        self.client.notify("exit", json!(null))?;
        let status = self.client.wait_for_exit(PROCESS_TIMEOUT)?;
        ensure!(status.success(), "perllsp exited unsuccessfully: {status}");
        self.client.assert_transport_clean()
    }
}

fn error_code(response: &Value) -> Option<i64> {
    response.get("error").and_then(|error| error.get("code")).and_then(Value::as_i64)
}

fn error_message(response: &Value) -> &str {
    response.pointer("/error/message").and_then(Value::as_str).unwrap_or("")
}

/// Build `workspace/applyEdit` params planting `new_text` at `at_line` of
/// `uri`. The changes map is keyed dynamically, so it is assembled through a
/// [`serde_json::Map`] rather than the `json!` literal syntax.
fn apply_edit_params(uri: &str, at_line: i64, new_text: &str, label: &str) -> Value {
    let edit_item = json!({
        "range": {
            "start": { "line": at_line, "character": 0 },
            "end": { "line": at_line, "character": 0 }
        },
        "newText": new_text
    });
    let mut changes = serde_json::Map::new();
    changes.insert(uri.to_string(), Value::Array(vec![edit_item]));
    json!({ "label": label, "edit": { "changes": changes } })
}

/// Send `method` as a client request and require the direction-gate
/// rejection: MethodNotFound (-32601) naming the client-to-server boundary.
///
/// The message assertion is what fails if someone reintroduces reversed
/// compatibility behavior under the unchanged standard method name (#8896
/// negative control 6): the rejection disappears entirely.
fn expect_direction_rejection(
    session: &mut Session,
    id: i64,
    method: &str,
    params: Value,
) -> Result<()> {
    let response = session.client.request(json!(id), method, params, PROCESS_TIMEOUT)?;
    ensure!(
        error_code(&response) == Some(-32601),
        "`{method}` from the client must be rejected with -32601, got: {response}"
    );
    ensure!(
        error_message(&response).contains("client-to-server"),
        "`{method}` rejection must name the direction boundary: {response}"
    );
    Ok(())
}

/// Baseline positive control: standard client→server routes work, the server
/// still emits its `workspace/configuration` request with a numeric id, and
/// the responding envelope is consumed by the #7010 path without surfacing as
/// a method, an error frame, or transport noise.
#[test]
fn normal_routes_and_outbound_configuration_pull_still_work() -> Result<()> {
    let mut session = Session::spawn(true)?;

    let configuration =
        session.client.receive_server_request("workspace/configuration", PROCESS_TIMEOUT)?;
    let request_id =
        configuration.get("id").cloned().context("configuration request missing id")?;
    ensure!(request_id.is_number(), "server request id must be numeric, got {request_id}");

    session.client.respond(request_id, json!([{ "perl": {} }]))?;
    thread::sleep(SETTLE);
    session.client.assert_no_response_pending()?;

    session.did_open_base()?;
    session.hover_ok()?;
    session.shutdown()
}

/// Negative control 1 (+ the state-mutation acceptance rows): a client-sent
/// `workspace/applyEdit` request is rejected with -32601 and the attempted
/// edit never lands — proven against a same-session positive control where
/// legitimate text changes do reach the symbol pipeline.
#[test]
fn client_sent_apply_edit_request_cannot_mutate_state() -> Result<()> {
    let mut session = Session::spawn(false)?;
    session.did_open_base()?;

    let insert_line = BASE_SOURCE.lines().count() as i64;
    let intruder_uri = session.source_uri.clone();
    expect_direction_rejection(
        &mut session,
        88101,
        "workspace/applyEdit",
        apply_edit_params(
            &intruder_uri,
            insert_line,
            &format!("\nsub {INTRUDER_MARKER} {{}}\n"),
            "malicious",
        ),
    )?;

    // The planted sub must be absent from symbol results…
    let intruder_visible = session.wait_for_symbol(INTRUDER_MARKER, INTRUDER_MARKER)?;
    ensure!(
        !intruder_visible,
        "client-sent workspace/applyEdit mutated server state despite the gate"
    );

    // …while the same pipeline immediately observes a legitimate change.
    let mut grown = BASE_SOURCE.to_string();
    grown.push_str(&format!("\nsub {POSITIVE_CONTROL_MARKER} {{}}\n"));
    session.change_source_full(2, &grown)?;
    let positive_visible =
        session.wait_for_symbol(POSITIVE_CONTROL_MARKER, POSITIVE_CONTROL_MARKER)?;
    ensure!(
        positive_visible,
        "positive control failed: legitimate didChange edits do not reach \
         the symbol pipeline in this session"
    );
    let intruder_still_hidden =
        !session.symbol_names(INTRUDER_MARKER)?.iter().any(|name| name == INTRUDER_MARKER);
    ensure!(
        intruder_still_hidden,
        "the rejected applyEdit payload appeared once real edits were indexed"
    );

    session.shutdown()
}

/// A wrong-direction `workspace/applyEdit` notification produces no response
/// frame at all (JSON-RPC forbids replying to notifications; the gate drops
/// it) and mutates nothing.
#[test]
fn client_sent_apply_edit_notification_is_silently_dropped() -> Result<()> {
    let mut session = Session::spawn(false)?;
    session.did_open_base()?;

    let intruder_uri = session.source_uri.clone();
    session.client.notify(
        "workspace/applyEdit",
        apply_edit_params(
            &intruder_uri,
            0,
            &format!("sub {INTRUDER_MARKER} {{}}\n"),
            "malicious-notification",
        ),
    )?;
    thread::sleep(SETTLE);

    session.client.assert_no_response_pending()?;
    let intruder_visible = session.wait_for_symbol(INTRUDER_MARKER, INTRUDER_MARKER)?;
    ensure!(!intruder_visible, "client-sent applyEdit notification mutated server state");
    session.hover_ok()?;

    session.shutdown()
}

/// Negative controls 2 and 6: a client-sent `workspace/configuration` request
/// must not be answered out of server configuration state — it is a
/// server→client request and gets the named direction rejection.
#[test]
fn client_sent_configuration_request_is_not_a_config_response() -> Result<()> {
    let mut session = Session::spawn(false)?;
    session.did_open_base()?;

    let response = session.client.request(
        json!(88301),
        "workspace/configuration",
        json!({ "items": [ { "section": "perl" } ] }),
        PROCESS_TIMEOUT,
    )?;
    ensure!(
        error_code(&response) == Some(-32601),
        "client-sent workspace/configuration must be rejected, got: {response}"
    );
    ensure!(
        response.get("result").is_none(),
        "a configuration array must never be returned to a client-sent \
         workspace/configuration request: {response}"
    );

    session.hover_ok()?;
    session.shutdown()
}

/// `client/registerCapability` / `client/unregisterCapability` are
/// server→client requests; a client sending them must get -32601 rather than
/// activating or deactivating features.
#[test]
fn client_sent_capability_registration_requests_are_rejected() -> Result<()> {
    let mut session = Session::spawn(false)?;
    session.did_open_base()?;

    expect_direction_rejection(
        &mut session,
        88401,
        "client/registerCapability",
        json!({ "registrations": [
            { "id": "probe", "method": "workspace/didChangeWatchedFiles" }
        ] }),
    )?;
    expect_direction_rejection(
        &mut session,
        88402,
        "client/unregisterCapability",
        json!({ "unregistrations": [
            { "id": "probe", "method": "workspace/didChangeWatchedFiles" }
        ] }),
    )?;

    session.hover_ok()?;
    session.shutdown()
}

/// Wrong-direction notifications (`$/progress`, `window/showMessage`) produce
/// no response frame and leave the server fully responsive — the
/// transport-level face of negative control 3.
#[test]
fn wrong_direction_notifications_produce_no_response_or_state_change() -> Result<()> {
    let mut session = Session::spawn(false)?;
    session.did_open_base()?;

    session.client.notify(
        "$/progress",
        json!({ "token": "hostile-token", "value": { "kind": "begin", "title": "hostile" } }),
    )?;
    session
        .client
        .notify("window/showMessage", json!({ "type": 1, "message": "hostile client message" }))?;
    thread::sleep(SETTLE);

    session.client.assert_no_response_pending()?;
    session.hover_ok()?;
    session.shutdown()
}

/// JSON-RPC response envelopes — numeric or string ids, result or error — are
/// consumed as responses and never enter method dispatch as methods. If the
/// framing layer regressed into routing them, some frame would answer back or
/// the session would break; neither may happen here (negative control 5).
#[test]
fn response_envelopes_with_numeric_or_string_ids_never_become_methods() -> Result<()> {
    let mut session = Session::spawn(false)?;
    session.did_open_base()?;

    for raw in [
        json!({ "jsonrpc": "2.0", "id": 987_654, "result": { "applied": true } }),
        json!({ "jsonrpc": "2.0", "id": "perllsp-direction-probe", "result": null }),
        json!({ "jsonrpc": "2.0", "id": 987_655, "error":
            { "code": -32603, "message": "synthetic" } }),
    ] {
        session.client.send_raw_bytes(&RealProcessClient::encode_message(&raw))?;
    }
    thread::sleep(SETTLE);

    session.client.assert_no_response_pending()?;
    session.hover_ok()?;
    session.shutdown()
}

/// Custom project methods keep their declared directions: the watchdog
/// request answers, `perl/showAst` still dispatches as a client→server
/// extension request, and the internal `$/perl-lsp/clientResponse`
/// notification carrier remains inbound-only and silent.
#[test]
fn custom_methods_retain_declared_directions() -> Result<()> {
    let mut session = Session::spawn(false)?;
    session.did_open_base()?;

    let watchdog = session.client.request(
        json!(88701),
        "$/perl-lsp/watchdog",
        json!(null),
        PROCESS_TIMEOUT,
    )?;
    ensure!(watchdog.get("error").is_none(), "$/perl-lsp/watchdog must stay reachable: {watchdog}");

    let show_ast = session.client.request(
        json!(88702),
        "perl/showAst",
        json!({ "uri": session.source_uri.clone() }),
        PROCESS_TIMEOUT,
    )?;
    let code = error_code(&show_ast);
    ensure!(
        code.is_none() || code != Some(-32601),
        "perl/showAst must keep its client-to-server direction, got: {show_ast}"
    );

    session
        .client
        .notify("$/perl-lsp/clientResponse", json!({ "id": "not-pending", "result": null }))?;
    thread::sleep(SETTLE);
    session.client.assert_no_response_pending()?;

    session.hover_ok()?;
    session.shutdown()
}
