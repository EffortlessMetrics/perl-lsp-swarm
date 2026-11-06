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

// --- Sub-local container cross-sigil highlights (#18312) ---

fn highlight_session(source: &str) -> Result<(RealProcessClient, String)> {
    let mut client = RealProcessClient::spawn_exact()?;
    let root_uri = Url::from_directory_path(client.workspace_path())
        .map_err(|()| anyhow::anyhow!("fixture root must be a file URI"))?
        .to_string();
    let source_path = client.workspace_path().join("highlight-sub-local.pl");
    std::fs::write(&source_path, source)?;
    let uri = Url::from_file_path(&source_path)
        .map_err(|()| anyhow::anyhow!("fixture source must be a file URI"))?
        .to_string();
    let initialized = client.request(
        json!("initialize-highlight-sub-local"),
        "initialize",
        json!({
            "rootUri": root_uri,
            "workspaceFolders": [{"uri": root_uri, "name": "highlight-sub-local-fixture"}],
            "capabilities": {"general": {"positionEncodings": ["utf-16"]}}
        }),
        TIMEOUT,
    )?;
    ensure!(initialized.get("error").is_none(), "initialize failed: {initialized}");
    client.notify("initialized", json!({}))?;
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": "perl", "version": 1, "text": source}}),
    )?;
    Ok((client, uri))
}

fn highlight_at(
    client: &mut RealProcessClient,
    uri: &str,
    line: u32,
    character: u32,
    id: &str,
) -> Result<Value> {
    let response = client.request(
        json!(id),
        METHOD,
        json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}}),
        TIMEOUT,
    )?;
    ensure!(response.get("error").is_none(), "highlight failed: {response}");
    response.get("result").cloned().context("highlight omitted result")
}

fn normalize_highlights(result: Value) -> Value {
    let mut items = result.as_array().cloned().unwrap_or_default();
    items.sort_by(|a, b| {
        let al = a["range"]["start"]["line"].as_u64().unwrap_or(0);
        let bl = b["range"]["start"]["line"].as_u64().unwrap_or(0);
        al.cmp(&bl).then_with(|| {
            a["range"]["start"]["character"]
                .as_u64()
                .unwrap_or(0)
                .cmp(&b["range"]["start"]["character"].as_u64().unwrap_or(0))
        })
    });
    json!(items)
}

fn finish_highlight_session(mut client: RealProcessClient) -> Result<()> {
    let shutdown =
        client.request(json!("highlight-sub-local-shutdown"), "shutdown", Value::Null, TIMEOUT)?;
    ensure!(shutdown.get("error").is_none() && shutdown["result"].is_null());
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(TIMEOUT)?.success());
    client.assert_transport_clean()
}

#[test]
fn sub_local_array_decl_and_element_use_highlight_together() -> Result<()> {
    let source = "sub demo {\n    my @items = (1);\n    print $items[0];\n}\n";
    let (mut client, uri) = highlight_session(source)?;
    let expected = json!([
        {"range": {"start": {"line": 1, "character": 7}, "end": {"line": 1, "character": 13}}, "kind": 3},
        {"range": {"start": {"line": 2, "character": 10}, "end": {"line": 2, "character": 16}}, "kind": 2}
    ]);
    let on_decl = normalize_highlights(highlight_at(&mut client, &uri, 1, 8, "hl-array-decl")?);
    ensure!(on_decl == expected, "on @items decl: {on_decl} vs {expected}");
    let on_use = normalize_highlights(highlight_at(&mut client, &uri, 2, 11, "hl-array-use")?);
    ensure!(on_use == expected, "on element use: {on_use} vs {expected}");
    finish_highlight_session(client)
}

#[test]
fn sub_local_hash_decl_and_element_use_highlight_together() -> Result<()> {
    let source = "sub demo {\n    my %h = (a => 1);\n    print $h{a};\n}\n";
    let (mut client, uri) = highlight_session(source)?;
    let expected = json!([
        {"range": {"start": {"line": 1, "character": 7}, "end": {"line": 1, "character": 9}}, "kind": 3},
        {"range": {"start": {"line": 2, "character": 10}, "end": {"line": 2, "character": 12}}, "kind": 2}
    ]);
    let on_decl = normalize_highlights(highlight_at(&mut client, &uri, 1, 8, "hl-hash-decl")?);
    ensure!(on_decl == expected, "on %h decl: {on_decl} vs {expected}");
    let on_use = normalize_highlights(highlight_at(&mut client, &uri, 2, 11, "hl-hash-use")?);
    ensure!(on_use == expected, "on hash element use: {on_use} vs {expected}");
    finish_highlight_session(client)
}

#[test]
fn sub_local_array_last_index_highlights_with_declaration() -> Result<()> {
    let source = "sub demo {\n    my @items = (1);\n    my $n = $#items;\n}\n";
    let (mut client, uri) = highlight_session(source)?;
    let expected = json!([
        {"range": {"start": {"line": 1, "character": 7}, "end": {"line": 1, "character": 13}}, "kind": 3},
        {"range": {"start": {"line": 2, "character": 12}, "end": {"line": 2, "character": 19}}, "kind": 2}
    ]);
    let on_decl = normalize_highlights(highlight_at(&mut client, &uri, 1, 8, "hl-last-decl")?);
    ensure!(on_decl == expected, "on @items: {on_decl} vs {expected}");
    let on_last = normalize_highlights(highlight_at(&mut client, &uri, 2, 14, "hl-last-use")?);
    ensure!(on_last == expected, "on last-index use: {on_last} vs {expected}");
    finish_highlight_session(client)
}

#[test]
fn sub_local_same_named_scalar_index_is_excluded() -> Result<()> {
    let source =
        "sub demo {\n    my @items = (1);\n    my $items = 0;\n    print $foo[$items];\n}\n";
    let (mut client, uri) = highlight_session(source)?;
    let expected = json!([
        {"range": {"start": {"line": 1, "character": 7}, "end": {"line": 1, "character": 13}}, "kind": 3}
    ]);
    let on_decl = normalize_highlights(highlight_at(&mut client, &uri, 1, 8, "hl-scalar-index")?);
    ensure!(
        on_decl == expected,
        "scalar index operand must not cross-match array decl: {on_decl} vs {expected}"
    );
    finish_highlight_session(client)
}

#[test]
fn sub_local_other_sub_occurrence_is_excluded() -> Result<()> {
    let source = "sub other { print $items[0]; }\nsub demo {\n    my @items = (1);\n    print $items[0];\n}\n";
    let (mut client, uri) = highlight_session(source)?;
    let expected = json!([
        {"range": {"start": {"line": 2, "character": 7}, "end": {"line": 2, "character": 13}}, "kind": 3},
        {"range": {"start": {"line": 3, "character": 10}, "end": {"line": 3, "character": 16}}, "kind": 2}
    ]);
    let on_decl = normalize_highlights(highlight_at(&mut client, &uri, 2, 8, "hl-other-sub")?);
    ensure!(on_decl == expected, "other sub must stay out of scope: {on_decl} vs {expected}");
    finish_highlight_session(client)
}

#[test]
fn file_scope_array_element_highlight_unchanged() -> Result<()> {
    let source = "my @arr = (1);\nprint $arr[0];\n";
    let (mut client, uri) = highlight_session(source)?;
    let expected = json!([
        {"range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 7}}, "kind": 3},
        {"range": {"start": {"line": 1, "character": 6}, "end": {"line": 1, "character": 10}}, "kind": 2}
    ]);
    let on_decl = normalize_highlights(highlight_at(&mut client, &uri, 0, 4, "hl-file-decl")?);
    ensure!(on_decl == expected, "file-scope regression: {on_decl} vs {expected}");
    finish_highlight_session(client)
}
