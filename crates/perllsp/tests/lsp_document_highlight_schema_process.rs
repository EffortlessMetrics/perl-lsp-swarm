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

fn utf16_line_col(source: &str, byte_offset: usize) -> (u32, u32) {
    let prefix = &source[..byte_offset];
    let line =
        u32::try_from(prefix.bytes().filter(|b| *b == b'\n').count()).expect("line count fits u32");
    let line_start = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let column = u32::try_from(source[line_start..byte_offset].encode_utf16().count())
        .expect("column fits u32");
    (line, column)
}

fn position_in(source: &str, needle: &str) -> Value {
    let start = source.find(needle).expect("needle in fixture");
    let offset = start + 1;
    let (line, character) = utf16_line_col(source, offset);
    json!({"line": line, "character": character})
}

fn range_for(source: &str, needle: &str) -> Value {
    let start = source.find(needle).expect("needle in fixture");
    let end = start + needle.len();
    let (start_line, start_character) = utf16_line_col(source, start);
    let (end_line, end_character) = utf16_line_col(source, end);
    json!({
        "start": {"line": start_line, "character": start_character},
        "end": {"line": end_line, "character": end_character}
    })
}

fn highlight_session(source: &str, rel_path: &str) -> Result<(RealProcessClient, String)> {
    let mut client = RealProcessClient::spawn_exact()?;
    let root_uri = Url::from_directory_path(client.workspace_path())
        .map_err(|()| anyhow::anyhow!("fixture root must be a file URI"))?
        .to_string();
    let source_path = client.workspace_path().join(rel_path);
    std::fs::write(&source_path, source)?;
    let uri = Url::from_file_path(&source_path)
        .map_err(|()| anyhow::anyhow!("fixture source must be a file URI"))?
        .to_string();
    let initialize = client.request(
        json!("initialize-highlight-local-container"),
        "initialize",
        json!({
            "rootUri": root_uri,
            "workspaceFolders": [{"uri": root_uri, "name": "highlight-fixture"}],
            "capabilities": {"general": {"positionEncodings": ["utf-16"]}}
        }),
        TIMEOUT,
    )?;
    ensure!(initialize.get("error").is_none(), "initialize failed: {initialize}");
    client.notify("initialized", json!({}))?;
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": "perl", "version": 1, "text": source}}),
    )?;
    Ok((client, uri))
}

fn highlights_at(client: &mut RealProcessClient, uri: &str, position: Value) -> Result<Value> {
    let id = json!(format!("hl-{}", position));
    let (_, response) =
        exchange(client, id, json!({"textDocument": {"uri": uri}, "position": position}))?;
    Ok(response["result"].clone())
}

fn normalize_highlights(mut value: Value) -> Value {
    if let Some(items) = value.as_array_mut() {
        items.sort_by(|a, b| {
            let ak = (
                a["range"]["start"]["line"].as_u64().unwrap_or(0),
                a["range"]["start"]["character"].as_u64().unwrap_or(0),
            );
            let bk = (
                b["range"]["start"]["line"].as_u64().unwrap_or(0),
                b["range"]["start"]["character"].as_u64().unwrap_or(0),
            );
            ak.cmp(&bk)
        });
    }
    value
}

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

/// #18312: subroutine-local array/hash container spellings include scalar element reads.
#[test]
fn local_container_cross_sigil_highlights_inside_sub() -> Result<()> {
    const SOURCE: &str = "\
sub demo {
    my @items = (1);
    print $items[0];
}
sub other {
    my @items = (9);
}
my @items = (0);
";
    const HASH_SOURCE: &str = "\
sub hdemo {
    my %h = (a => 1);
    print $h{a};
}
";
    const LAST_INDEX_SOURCE: &str = "\
sub idx {
    my @items = (1, 2);
    my $last = $#items;
}
";
    const SCALAR_INDEX_SOURCE: &str = "\
sub mix {
    my @items = (1);
    my $items = 2;
    print $x[$items];
}
";
    const FILE_SCOPE_SOURCE: &str = "my @arr = (1, 2, 3);\n$arr[0] = 5;\n";

    let expected_pair = |source: &str, decl: &str, elem: &str| -> Value {
        normalize_highlights(json!([
            {"range": range_for(source, decl), "kind": 3},
            {"range": range_for(source, elem), "kind": 2},
        ]))
    };

    let (mut client, uri) = highlight_session(SOURCE, "hl-local-array.pl")?;
    let expected = expected_pair(SOURCE, "@items", "$items");
    for needle in ["@items", "$items"] {
        let got =
            normalize_highlights(highlights_at(&mut client, &uri, position_in(SOURCE, needle))?);
        ensure!(got == expected, "cursor on {needle}: expected {expected}, got {got}");
    }

    let (mut client, uri) = highlight_session(HASH_SOURCE, "hl-local-hash.pl")?;
    let expected_hash = expected_pair(HASH_SOURCE, "%h", "$h");
    for needle in ["%h", "$h"] {
        let got = normalize_highlights(highlights_at(
            &mut client,
            &uri,
            position_in(HASH_SOURCE, needle),
        )?);
        ensure!(
            got == expected_hash,
            "hash cursor on {needle}: expected {expected_hash}, got {got}"
        );
    }

    let (mut client, uri) = highlight_session(LAST_INDEX_SOURCE, "hl-last-index.pl")?;
    let expected_last = expected_pair(LAST_INDEX_SOURCE, "@items", "$#items");
    for needle in ["@items", "$#items"] {
        let got = normalize_highlights(highlights_at(
            &mut client,
            &uri,
            position_in(LAST_INDEX_SOURCE, needle),
        )?);
        ensure!(
            got == expected_last,
            "last-index cursor on {needle}: expected {expected_last}, got {got}"
        );
    }

    let (mut client, uri) = highlight_session(SCALAR_INDEX_SOURCE, "hl-scalar-index.pl")?;
    let decl_only = normalize_highlights(json!([
        {"range": range_for(SCALAR_INDEX_SOURCE, "@items"), "kind": 3},
    ]));
    let got = normalize_highlights(highlights_at(
        &mut client,
        &uri,
        position_in(SCALAR_INDEX_SOURCE, "@items"),
    )?);
    ensure!(got == decl_only, "scalar index control: expected only @items decl, got {got}");

    let (mut client, uri) = highlight_session(FILE_SCOPE_SOURCE, "hl-file-scope.pl")?;
    let expected_file = expected_pair(FILE_SCOPE_SOURCE, "@arr", "$arr");
    let got = normalize_highlights(highlights_at(
        &mut client,
        &uri,
        position_in(FILE_SCOPE_SOURCE, "@arr"),
    )?);
    ensure!(got == expected_file, "file-scope control: expected {expected_file}, got {got}");

    Ok(())
}
