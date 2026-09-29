//! Stdio `textDocument/completion` proof for interpolation-variable admission (#16863).
//!
//! Drives a just-built `perllsp` over the production completion route. Helper-only
//! provider tests are not sufficient for this claim.

use serde_json::json;
use std::time::Duration;

mod common;
use common::{
    completion_items, initialize_lsp, read_notification_for_uri, send_notification, send_request,
    start_lsp_server,
};
use perl_tdd_support::must;

fn await_document_indexed(server: &common::LspServer, uri: &str) {
    let budget = Duration::from_secs(10);
    must(
        read_notification_for_uri(server, "perl-lsp/active-document-ready", uri, budget)
            .ok_or_else(|| {
                format!(
                    "server never acknowledged indexing of {uri} within {budget:?}; {}",
                    server.stderr_tail()
                )
            }),
    );
}

fn labels(items: &[serde_json::Value]) -> Vec<String> {
    items.iter().filter_map(|item| item["label"].as_str().map(|s| s.to_string())).collect()
}

#[test]
fn interpolation_completion_stdio_is_lexical_and_quiet_on_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let server = start_lsp_server();
    initialize_lsp(&server);

    let uri = "file:///interpolation.pl";
    let text = concat!(
        "package Animal;\n",
        "my $name = \"x\";\n",
        "my $text = \"hello $\";\n",
        "my $quiet = 'hello $na';\n",
    );
    let interpolating_character = text
        .lines()
        .nth(2)
        .and_then(|line| line.find("hello $").map(|idx| idx + "hello $".len()))
        .ok_or("interpolating slot")?;
    let quiet_character = text
        .lines()
        .nth(3)
        .and_then(|line| line.find("hello $na").map(|idx| idx + "hello $na".len()))
        .ok_or("quiet slot")?;
    send_notification(
        &server,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": text
                }
            }
        }),
    );
    await_document_indexed(&server, uri);

    let interpolating = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 2, "character": interpolating_character }
            }
        }),
    );
    let interpolating_items = completion_items(&interpolating);
    let interpolating_labels = labels(interpolating_items);
    assert!(
        interpolating_labels.iter().any(|label| label == "$name"),
        "stdio interpolation slot missing $name: {interpolating_labels:?}"
    );
    assert!(
        interpolating_labels.iter().all(|label| label.starts_with('$')
            || label.starts_with('@')
            || label.starts_with('%')),
        "stdio interpolation slot leaked unsigil-compatible items: {interpolating_labels:?}"
    );
    assert!(
        !interpolating_labels.iter().any(|label| label == "Animal"),
        "stdio interpolation slot leaked workspace package Animal: {interpolating_labels:?}"
    );

    let name_item = interpolating_items
        .iter()
        .find(|item| item["label"].as_str() == Some("$name"))
        .ok_or("$name item")?;
    let range = name_item.get("textEdit").and_then(|edit| edit.get("range"));
    if let Some(range) = range {
        assert_eq!(range["start"]["line"].as_u64(), Some(2));
        assert_eq!(
            range["start"]["character"].as_u64(),
            Some((interpolating_character - 1) as u64)
        );
        assert_eq!(range["end"]["line"].as_u64(), Some(2));
        assert_eq!(range["end"]["character"].as_u64(), Some(interpolating_character as u64));
    }

    let quiet = send_request(
        &server,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": quiet_character }
            }
        }),
    );
    let quiet_labels = labels(completion_items(&quiet));
    assert!(
        !quiet_labels.iter().any(|label| label == "$name"),
        "stdio single-quoted slot leaked $name: {quiet_labels:?}"
    );

    Ok(())
}
