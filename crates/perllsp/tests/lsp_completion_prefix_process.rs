//! Prefix admission and method-token edits through the exact public server.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Result, anyhow, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);
const URI: &str = "file:///completion-prefix.pl";
const DOG: &str = "package Animal::Dog;\nsub new { bless {}, shift }\nsub speak { 'woof' }\nsub fetch { 'ball' }\npackage main;\nmy $dog = Animal::Dog->new();\n";

fn start(source: &str) -> Result<RealProcessClient> {
    let mut client = RealProcessClient::spawn_exact()?;
    let response = client.request(
        json!("initialize"),
        "initialize",
        json!({"processId": null, "rootUri": null, "capabilities": {
            "general": {"positionEncodings": ["utf-16"]},
            "textDocument": {"completion": {"contextSupport": true}}
        }}),
        TIMEOUT,
    )?;
    ensure!(
        response.pointer("/result/capabilities/completionProvider").is_some(),
        "public server must advertise completion: {response}"
    );
    client.notify("initialized", json!({}))?;
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": URI, "languageId": "perl", "version": 1, "text": source
        }}),
    )?;
    Ok(client)
}

fn finish(mut client: RealProcessClient) -> Result<()> {
    let response = client.request(json!("shutdown"), "shutdown", Value::Null, TIMEOUT)?;
    ensure!(response.get("result").is_some_and(Value::is_null), "shutdown failed: {response}");
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(TIMEOUT)?.success(), "public server did not exit cleanly");
    client.assert_transport_clean()
}

fn complete(
    client: &mut RealProcessClient,
    id: Value,
    line: u32,
    character: u32,
) -> Result<Vec<Value>> {
    let response = client.request(
        id.clone(),
        "textDocument/completion",
        json!({
            "textDocument": {"uri": URI}, "position": {"line": line, "character": character},
            "context": {"triggerKind": 1}
        }),
        TIMEOUT,
    )?;
    ensure!(response["id"] == id && response["jsonrpc"] == "2.0", "wrong response: {response}");
    ensure!(response.get("error").is_none(), "completion failed: {response}");
    // While the current generation's parse is pending, the server answers
    // with the bounded fallback and `isIncomplete: true` (#17775); that list
    // is not the final answer. Poll until the parse lands so assertions run
    // against the settled list.
    let mut response = response;
    for _ in 0..100 {
        if response.pointer("/result/isIncomplete").and_then(Value::as_bool) != Some(true) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
        response = client.request(
            id.clone(),
            "textDocument/completion",
            json!({
                "textDocument": {"uri": URI}, "position": {"line": line, "character": character},
                "context": {"triggerKind": 1}
            }),
            TIMEOUT,
        )?;
        ensure!(response.get("error").is_none(), "completion failed: {response}");
    }
    ensure!(
        response.pointer("/result/isIncomplete").and_then(Value::as_bool) != Some(true),
        "completion stayed incomplete while its parse was pending: {response}"
    );
    response
        .pointer("/result/items")
        .or_else(|| response.get("result"))
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| anyhow!("completion omitted its item list: {response}"))
}

fn item<'a>(items: &'a [Value], label: &str) -> Result<&'a Value> {
    let matching: Vec<_> = items.iter().filter(|item| item["label"] == label).collect();
    ensure!(matching.len() == 1, "expected exactly one {label}: {items:?}");
    Ok(matching[0])
}

fn assert_edit(item: &Value, line: u32, start: u32, end: u32, new_text: &str) -> Result<()> {
    ensure!(
        item["textEdit"]
            == json!({
                "range": {"start": {"line": line, "character": start},
                          "end": {"line": line, "character": end}},
                "newText": new_text
            }),
        "completion must replace only the typed method token: {item}"
    );
    ensure!(
        item.get("additionalTextEdits").is_none_or(|edits| edits == &json!([])),
        "method completion must not add unrelated edits: {item}"
    );
    Ok(())
}

#[test]
fn incomplete_method_prefix_filters_generic_noise_and_recovers_after_edits() -> Result<()> {
    let mut client = start(&format!("{DOG}my $answer = $dog->sp"))?;
    let items = complete(&mut client, json!("incomplete-eof"), 6, 21)?;
    assert_edit(item(&items, "speak")?, 6, 19, 21, "speak()")?;
    ensure!(
        items.iter().all(|item| item["label"].as_str().is_some_and(|name| name.starts_with("sp"))),
        "unrelated generic methods must not crowd a typed prefix: {items:?}"
    );

    // Keep the same document open, repair the incomplete call, and query before
    // its semicolon. Matching generics must survive; removing the catalog fails.
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": URI, "version": 2},
            "contentChanges": [{"text": format!("{DOG}my $answer = $dog->ca;\n")}]
        }),
    )?;
    client.notify("$/cancelRequest", json!({"id": "incomplete-eof"}))?;
    let items = complete(&mut client, json!("repaired"), 6, 21)?;
    assert_edit(item(&items, "can")?, 6, 19, 21, "can()")?;
    ensure!(
        items.iter().all(|item| item["label"] == "can"),
        "repaired completion must use the current prefix: {items:?}"
    );

    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": URI, "version": 3},
            "contentChanges": [{"text": format!("{DOG}my $answer = $dog->;")}]
        }),
    )?;
    let items = complete(&mut client, json!("empty-prefix"), 6, 19)?;
    for label in ["new", "isa", "can", "DOES", "VERSION", "DESTROY", "AUTOLOAD", "speak", "fetch"] {
        assert_edit(item(&items, label)?, 6, 19, 19, &format!("{label}()"))?;
    }
    finish(client)
}

#[test]
fn imported_api_catalog_keeps_matching_generic_and_api_methods() -> Result<()> {
    let source = "use HTTP::Tiny;\nmy $client = HTTP::Tiny->new();\n$client->ca;\nHTTP::Tiny->ca;\n$client->po;\n";
    let mut client = start(source)?;
    for (id, line, start, end) in [("instance", 2, 9, 11), ("static", 3, 12, 14)] {
        let items = complete(&mut client, json!(id), line, end)?;
        for label in ["can", "can_ssl"] {
            assert_edit(item(&items, label)?, line, start, end, &format!("{label}()"))?;
        }
        ensure!(
            items
                .iter()
                .all(|item| item["label"].as_str().is_some_and(|name| name.starts_with("ca"))),
            "both generic and imported API catalogs must respect the prefix: {items:?}"
        );
    }
    let items = complete(&mut client, json!("instance-only"), 4, 11)?;
    for label in ["post", "post_form"] {
        assert_edit(item(&items, label)?, 4, 9, 11, &format!("{label}()"))?;
    }
    finish(client)
}

#[test]
fn generic_prefix_is_leading_and_case_sensitive() -> Result<()> {
    // A generic-only receiver prevents a local 'new' definition from masking
    // the catalog. 'n' must retain new without admitting the substring in can.
    let mut client =
        start("my $opaque = bless {}, 'Unindexed';\n$opaque->n;\n$opaque->d;\n$opaque->D;\n")?;
    let items = complete(&mut client, json!("leading-prefix"), 1, 10)?;
    assert_edit(item(&items, "new")?, 1, 9, 10, "new()")?;
    ensure!(items.len() == 1, "substring matches must be excluded: {items:?}");
    let items = complete(&mut client, json!("lowercase"), 2, 10)?;
    ensure!(items.is_empty(), "method prefixes must remain case-sensitive: {items:?}");
    let items = complete(&mut client, json!("uppercase"), 3, 10)?;
    for label in ["DESTROY", "DOES"] {
        assert_edit(item(&items, label)?, 3, 9, 10, &format!("{label}()"))?;
    }
    ensure!(items.len() == 2, "only the uppercase catalog matches belong here: {items:?}");
    finish(client)
}

#[test]
fn mid_token_method_edit_stops_before_existing_parentheses() -> Result<()> {
    let mut client = start(&format!("{DOG}my $answer = $dog->speak();"))?;
    // Geometry only: existing-parentheses normalization remains unresolved.
    let items = complete(&mut client, json!("mid-token"), 6, 21)?;
    assert_edit(item(&items, "speak")?, 6, 19, 24, "speak()")?;
    ensure!(items.len() == 1, "expected only the speak result in this fixture: {items:?}");
    finish(client)
}

#[test]
fn method_completion_applies_whole_token_at_midpoint_and_end() -> Result<()> {
    let source = format!("{DOG}my $answer = $dog->speak;");
    let expected = format!("{DOG}my $answer = $dog->speak();");
    ensure!(source.is_ascii(), "this fixture maps UTF-16 columns directly to bytes");
    for (id, cursor) in [("mid-token-apply", 21), ("end-token-apply", 24)] {
        let mut client = start(&source)?;
        let items = complete(&mut client, json!(id), 6, cursor)?;
        let completion = item(&items, "speak")?;
        assert_edit(completion, 6, 19, 24, "speak()")?;
        ensure!(items.len() == 1, "expected only the speak result in this fixture: {items:?}");

        // assert_edit pins both returned lines to 6; DOG is exactly those six lines.
        // Read the actual returned edit rather than reconstructing expected bytes.
        let edit = &completion["textEdit"];
        let start = usize::try_from(
            edit["range"]["start"]["character"].as_u64().ok_or_else(|| anyhow!("missing start"))?,
        )?;
        let end = usize::try_from(
            edit["range"]["end"]["character"].as_u64().ok_or_else(|| anyhow!("missing end"))?,
        )?;
        let text = edit["newText"].as_str().ok_or_else(|| anyhow!("missing newText"))?;
        ensure!(start <= end && end <= source.len() - DOG.len(), "edit outside call line");
        let applied =
            format!("{}{}{}", &source[..DOG.len() + start], text, &source[DOG.len() + end..]);
        ensure!(applied == expected, "completion must preserve the full document: {applied}");
        finish(client)?;
    }
    Ok(())
}

#[test]
fn variable_completion_edit_matches_lexer_token_at_apostrophe() -> Result<()> {
    // #18462: the completion replacement range must stop where the lexer's
    // variable token stops. Real perl (5.42.2) folds `'` into a name only as
    // an old-style package separator with a further name segment behind it —
    // `"$dog's bowl"` interpolates `$dog::s`, while `print $dog'9` fails to
    // parse because the apostrophe opens a string. So the edit for
    // `$count'9` ends before the apostrophe and leaves the suffix in place.
    let source =
        "my $count = 42;\nmy $t = \"the $count'9 bowl\";\nmy $u = \"the $count's bowl\";\n";
    let mut client = start(source)?;
    // Line 1: `my $t = "the $count'9 bowl";` — cursor after `$co` (char 16).
    // The lexer's Variable part is `$count` [13..19]; `'9` stays literal, so
    // the edit is 13..19 and the client keeps `'9 bowl`.
    let items = complete(&mut client, json!("apostrophe-digit"), 1, 16)?;
    assert_edit(item(&items, "$count")?, 1, 13, 19, "$count")?;
    // Line 2: `my $u = "the $count's bowl";` — `$count's` is one lexer token
    // (perl interpolates `$dog::s`), so the edit covers the folded segment
    // whole: 13..21.
    let items = complete(&mut client, json!("apostrophe-fold"), 2, 16)?;
    assert_edit(item(&items, "$count")?, 2, 13, 21, "$count")?;
    finish(client)
}

#[test]
fn ordinary_lexical_and_package_completion_remain_usable() -> Result<()> {
    let source = "package App;\nsub process_record { 1 }\nmy $record_count = 2;\n$rec;\npro;\npackage Other;\nsub other_task { 1 }\noth;\n";
    let mut client = start(source)?;
    let items = complete(&mut client, json!("lexical"), 3, 4)?;
    item(&items, "$record_count")?;
    let items = complete(&mut client, json!("package-app"), 4, 3)?;
    item(&items, "process_record")?;
    ensure!(
        !items.iter().any(|item| item["label"] == "other_task"),
        "foreign package leaked: {items:?}"
    );
    let items = complete(&mut client, json!("package-other"), 7, 3)?;
    item(&items, "other_task")?;
    finish(client)
}
