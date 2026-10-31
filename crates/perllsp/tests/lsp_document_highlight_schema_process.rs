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

fn line_col(source: &str, byte_offset: usize) -> (u32, u32) {
    let before = &source[..byte_offset];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let character = source[line_start..byte_offset].encode_utf16().count() as u32;
    (line, character)
}

fn highlight_range(source: &str, needle: &str, occurrence: usize) -> Value {
    let mut found = 0usize;
    let mut start = 0usize;
    while let Some(rel) = source[start..].find(needle) {
        let byte = start + rel;
        if found == occurrence {
            let (line, character) = line_col(source, byte);
            let end_byte = byte + needle.len();
            let (end_line, end_character) = line_col(source, end_byte);
            return json!({
                "start": {"line": line, "character": character},
                "end": {"line": end_line, "character": end_character}
            });
        }
        found += 1;
        start = byte + needle.len();
    }
    panic!("needle {needle:?} occurrence {occurrence} not found in source");
}

fn highlight_entry(source: &str, needle: &str, occurrence: usize, kind: u64) -> Value {
    json!({"range": highlight_range(source, needle, occurrence), "kind": kind})
}

struct HighlightFixture {
    client: RealProcessClient,
    uri: String,
    source: String,
}

impl HighlightFixture {
    fn open(source: &str) -> Result<Self> {
        let mut client = RealProcessClient::spawn_exact()?;
        let root_uri = Url::from_directory_path(client.workspace_path())
            .map_err(|()| anyhow::anyhow!("fixture root must be a file URI"))?
            .to_string();
        let source_path = client.workspace_path().join("highlight.pl");
        std::fs::write(&source_path, source)?;
        let uri = Url::from_file_path(&source_path)
            .map_err(|()| anyhow::anyhow!("fixture source must be a file URI"))?
            .to_string();
        let initialize = client.request(
            json!("initialize-highlight-fixture"),
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
        Ok(Self { client, uri, source: source.to_string() })
    }

    fn highlights_at(&mut self, needle: &str, occurrence: usize) -> Result<Value> {
        let mut found = 0usize;
        let mut start = 0usize;
        let byte = loop {
            let rel = self.source[start..]
                .find(needle)
                .ok_or_else(|| anyhow::anyhow!("needle {needle:?} occurrence {occurrence} missing"))?;
            let absolute = start + rel;
            if found == occurrence {
                break absolute + 1;
            }
            found += 1;
            start = absolute + needle.len();
        };
        let (line, character) = line_col(&self.source, byte);
        let (_, response) = exchange(
            &mut self.client,
            json!(format!("hl-{needle}-{occurrence}-{line}-{character}")),
            json!({"textDocument": {"uri": self.uri}, "position": {"line": line, "character": character}}),
        )?;
        Ok(response["result"].clone())
    }
}

fn sort_highlights(value: &Value) -> Value {
    let mut entries = value.as_array().cloned().unwrap_or_default();
    entries.sort_by(|a, b| {
        let al = a["range"]["start"]["line"].as_u64().unwrap_or(0);
        let bl = b["range"]["start"]["line"].as_u64().unwrap_or(0);
        let ac = a["range"]["start"]["character"].as_u64().unwrap_or(0);
        let bc = b["range"]["start"]["character"].as_u64().unwrap_or(0);
        al.cmp(&bl).then(ac.cmp(&bc))
    });
    json!(entries)
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

/// #18312: local container element spellings inside a subroutine stay linked to
/// the array/hash declaration under scoped collection.
#[test]
fn document_highlight_sub_scope_container_element_spellings() -> Result<()> {
    const WRITE: u64 = 3;
    const READ: u64 = 2;

    let array_sub = "sub demo {\n    my @items = (1);\n    print $items[0];\n}\n";
    let mut array_fixture = HighlightFixture::open(array_sub)?;
    let on_decl = sort_highlights(&array_fixture.highlights_at("@items", 0)?);
    ensure!(
        on_decl
            == sort_highlights(&json!([
                highlight_entry(array_sub, "@items", 0, WRITE),
                highlight_entry(array_sub, "$items", 0, READ),
            ])),
        "cursor on @items decl inside sub: {on_decl}"
    );
    let on_use = sort_highlights(&array_fixture.highlights_at("$items", 0)?);
    ensure!(
        on_use
            == sort_highlights(&json!([
                highlight_entry(array_sub, "@items", 0, WRITE),
                highlight_entry(array_sub, "$items", 0, READ),
            ])),
        "cursor on $items[0] container inside sub: {on_use}"
    );

    let hash_sub = "sub demo {\n    my %h = (a => 1);\n    print $h{a};\n}\n";
    let mut hash_fixture = HighlightFixture::open(hash_sub)?;
    let hash_expected = sort_highlights(&json!([
        highlight_entry(hash_sub, "%h", 0, WRITE),
        highlight_entry(hash_sub, "$h", 0, READ),
    ]));
    ensure!(
        sort_highlights(&hash_fixture.highlights_at("%h", 0)?) == hash_expected,
        "cursor on %h decl inside sub"
    );
    ensure!(
        sort_highlights(&hash_fixture.highlights_at("$h", 0)?) == hash_expected,
        "cursor on $h{{key}} container inside sub"
    );

    let last_index = "sub demo {\n    my @items = (1, 2);\n    my $last = $#items;\n}\n";
    let mut last_fixture = HighlightFixture::open(last_index)?;
    let last_expected = sort_highlights(&json!([
        highlight_entry(last_index, "@items", 0, WRITE),
        highlight_entry(last_index, "$#items", 0, READ),
    ]));
    ensure!(
        sort_highlights(&last_fixture.highlights_at("@items", 0)?) == last_expected,
        "cursor on @items with $#items use"
    );
    ensure!(
        sort_highlights(&last_fixture.highlights_at("$#items", 0)?) == last_expected,
        "cursor on $#items inside sub"
    );

    let scalar_shadow = "sub demo {\n    my @items = (1);\n    my $items = 0;\n    print $items;\n}\n";
    let mut shadow_fixture = HighlightFixture::open(scalar_shadow)?;
    ensure!(
        sort_highlights(&shadow_fixture.highlights_at("@items", 0)?)
            == sort_highlights(&json!([highlight_entry(scalar_shadow, "@items", 0, WRITE),])),
        "same-named scalar must not join @items declaration"
    );

    let other_sub = "sub one {\n    my @items = (1);\n}\nsub two {\n    print $items[0];\n}\n";
    let mut other_fixture = HighlightFixture::open(other_sub)?;
    ensure!(
        sort_highlights(&other_fixture.highlights_at("@items", 0)?)
            == sort_highlights(&json!([highlight_entry(other_sub, "@items", 0, WRITE),])),
        "declaration in another sub must stay scoped"
    );

    let file_scope = "my @items = (1);\nprint $items[0];\n";
    let mut file_fixture = HighlightFixture::open(file_scope)?;
    let file_expected = sort_highlights(&json!([
        highlight_entry(file_scope, "@items", 0, WRITE),
        highlight_entry(file_scope, "$items", 0, READ),
    ]));
    ensure!(
        sort_highlights(&file_fixture.highlights_at("@items", 0)?) == file_expected,
        "file-scope control on @items"
    );
    ensure!(
        sort_highlights(&file_fixture.highlights_at("$items", 0)?) == file_expected,
        "file-scope control on $items[0]"
    );

    Ok(())
}
