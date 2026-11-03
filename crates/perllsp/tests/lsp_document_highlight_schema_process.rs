//! Exact public-process request/result schema proof for documentHighlight.
//! Only this provider's selected frames are validated; this is not an
//! all-frame capture or aggregate coverage receipt.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use perllsp::protocol::schema::{
    Direction, MessageKind, ProtocolSchemaValidator, ProtocolVersion, ValidationContext,
};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use url::Url;

const METHOD: &str = "textDocument/documentHighlight";
const TIMEOUT: Duration = Duration::from_secs(10);

fn exchange(client: &mut RealProcessClient, id: Value, params: Value) -> Result<(Value, Value)> {
    let message = RealProcessClient::method_message_for_test(Some(id.clone()), METHOD, params)?;
    let wire = RealProcessClient::encode_message(&message);
    let captured_request = RealProcessClient::parse_stdout_frame_for_test(&wire)?
        .context("highlight request must round-trip through strict framing")?;
    ensure!(captured_request == message, "encoded request changed its payload");
    client.send_raw_bytes(&wire)?;
    let response = client.receive_response(&id, TIMEOUT)?;
    ensure!(response.get("error").is_none(), "highlight returned an error: {response}");
    ensure!(response.get("result").is_some(), "highlight response omitted result: {response}");
    Ok((captured_request, response))
}

#[test]
fn exact_public_highlight_request_and_results_match_authoritative_schema() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let root_uri = Url::from_directory_path(client.workspace_path())
        .map_err(|()| anyhow::anyhow!("fixture root must be a file URI"))?
        .to_string();
    let source_path = client.workspace_path().join("highlight.pl");
    let text = "my $count = 1;\nprint $count;\n\n";
    std::fs::write(&source_path, text)?;
    let uri = Url::from_file_path(&source_path)
        .map_err(|()| anyhow::anyhow!("fixture source must be a file URI"))?
        .to_string();
    let initialize = client.request(
        json!("initialize-highlight"),
        "initialize",
        json!({
            "rootUri": root_uri,
            "workspaceFolders": [{"uri": root_uri, "name": "highlight-fixture"}],
            "capabilities": {"general": {"positionEncodings": ["utf-16"]}}
        }),
        TIMEOUT,
    )?;
    ensure!(initialize.get("error").is_none(), "initialize failed: {initialize}");
    ensure!(
        initialize["result"]["capabilities"]["documentHighlightProvider"] == json!(true),
        "highlight must be advertised: {initialize}"
    );
    client.notify("initialized", json!({}))?;
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": "perl", "version": 1, "text": text}}),
    )?;

    let expected = json!([
        {"range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 9}}, "kind": 3},
        {"range": {"start": {"line": 1, "character": 6}, "end": {"line": 1, "character": 12}}, "kind": 2}
    ]);
    let mut frames = Vec::new();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let pair = exchange(
            &mut client,
            json!(format!("highlight-live-{}", frames.len())),
            json!({"textDocument": {"uri": uri}, "position": {"line": 0, "character": 4}}),
        )?;
        let result = pair.1["result"].clone();
        frames.push(pair);
        if result == expected {
            break;
        }
        ensure!(Instant::now() < deadline, "exact highlights never became visible: {result}");
        std::thread::sleep(Duration::from_millis(10));
    }
    let empty_pair = exchange(
        &mut client,
        json!(9),
        json!({"textDocument": {"uri": uri}, "position": {"line": 2, "character": 0}}),
    )?;
    ensure!(empty_pair.1["result"] == json!([]), "blank line must return []: {:?}", empty_pair.1);
    frames.push(empty_pair);

    // Existing normal lifecycle cleanup; recovery behavior is out of scope.
    let shutdown = client.request(json!("highlight-finish"), "shutdown", Value::Null, TIMEOUT)?;
    ensure!(shutdown.get("error").is_none() && shutdown["result"].is_null());
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(TIMEOUT)?.success());
    client.assert_transport_clean()?;

    // Delay schema assertions until live behavior is demonstrated so a missing
    // registry entry cannot masquerade as missing provider support.
    for (request, response) in frames {
        for (frame, direction, kind) in [
            (request, Direction::ClientToServer, MessageKind::Request),
            (response, Direction::ServerToClient, MessageKind::SuccessResponse),
        ] {
            let validated = ProtocolSchemaValidator::default()
                .validate(&frame, ValidationContext { direction, method: Some(METHOD) })?;
            ensure!(validated.method == METHOD && validated.direction == direction);
            ensure!(validated.kind == kind && validated.version == ProtocolVersion::Lsp317);
        }
    }
    Ok(())
}
