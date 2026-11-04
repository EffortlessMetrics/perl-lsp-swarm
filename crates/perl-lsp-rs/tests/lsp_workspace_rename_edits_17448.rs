//! Apply serialized rename edits independently of the server's position helpers.
//! Receiver, argument, and text lookalikes must stay byte-identical.

mod support;

use serde_json::{Value, json};
use std::collections::BTreeMap;
use support::lsp_harness::{LspHarness, TempWorkspace};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn offset(text: &str, position: &Value) -> TestResult<usize> {
    let line = position["line"].as_u64().ok_or("missing line")? as usize;
    let character = position["character"].as_u64().ok_or("missing character")? as usize;
    let mut line_start = 0;
    for _ in 0..line {
        line_start += text[line_start..].find('\n').ok_or("line outside document")? + 1;
    }
    let line_text = text[line_start..].split(['\r', '\n']).next().ok_or("missing line")?;
    let mut units = 0;
    for (byte, ch) in line_text.char_indices() {
        if units == character {
            return Ok(line_start + byte);
        }
        units += ch.len_utf16();
        if units > character {
            return Err("position splits a surrogate pair".into());
        }
    }
    if units == character {
        return Ok(line_start + line_text.len());
    }
    Err("character outside line".into())
}

fn apply(text: &str, edits: &[Value], old: &str, new: &str) -> TestResult<String> {
    let mut ranges = Vec::new();
    for edit in edits {
        let start = offset(text, &edit["range"]["start"])?;
        let end = offset(text, &edit["range"]["end"])?;
        assert_eq!(text.get(start..end), Some(old), "edit must select only the method name");
        assert_eq!(edit["newText"], new);
        ranges.push((start, end));
    }
    ranges.sort_unstable();
    for pair in ranges.windows(2) {
        assert!(pair[0].1 <= pair[1].0, "duplicate or overlapping edits");
    }
    let mut result = text.to_string();
    for (start, end) in ranges.into_iter().rev() {
        result.replace_range(start..end, new);
    }
    Ok(result)
}

fn receiver_rename(old: &str, new: &str, document_changes: bool) -> TestResult {
    let workspace = TempWorkspace::new()?;
    let mut harness = LspHarness::new();

    let lib_uri = workspace.uri("Greeter.pm");
    let app_uri = workspace.uri("caller.pl");
    let lib = format!(
        "package Greeter;\r\nuse utf8;\r\nsub new {{ return bless {{}}, shift }}\r\nsub {old} {{ return 1; }}\r\n1;\r\n"
    );
    let app = format!(
        "package Caller;\r\nuse utf8;\r\nuse Greeter;\r\nsub run {{\r\nmy ${old} = Greeter->new();\r\n\
         print \"😀\"; ${old}->{old}(${old});\r\n# {old} and ${old}->{old} are text\r\n\
         my $literal = '{old}';\r\nmy ${old}_long = 0;\r\n}}\r\n1;\r\n"
    );
    workspace.write("Greeter.pm", &lib)?;
    workspace.write("caller.pl", &app)?;
    harness.initialize_with_root(
        &workspace.root_uri,
        Some(json!({
            "workspace": { "workspaceEdit": { "documentChanges": document_changes } }
        })),
    )?;
    harness.open(&lib_uri, &lib)?;
    harness.open(&app_uri, &app)?;
    harness.wait_for_symbol(old, Some(&lib_uri), std::time::Duration::from_secs(5))?;
    harness.wait_for_symbol("run", Some(&app_uri), std::time::Duration::from_secs(5))?;

    let prepared = harness.request(
        "textDocument/prepareRename",
        json!({
            "textDocument": { "uri": lib_uri },
            "position": { "line": 3, "character": 5 }
        }),
    )?;
    assert_eq!(prepared["placeholder"], old, "rename anchor must be ready: {prepared}");

    let response = harness.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": lib_uri },
            "position": { "line": 3, "character": 5 },
            "newName": new
        }),
    )?;
    let trace = harness.request(
        "workspace/executeCommand",
        json!({
            "command": "perl.explainProviderDecision", "arguments": [{"provider":"rename"}]
        }),
    )?;
    assert!(response.is_object(), "rename must return edits: {response}, trace: {trace}");

    let mut changes = BTreeMap::new();
    if document_changes {
        assert!(response.get("changes").is_none(), "negotiated documentChanges: {response}");
        let documents = response["documentChanges"].as_array().ok_or("missing documentChanges")?;
        for document in documents {
            let uri = document["textDocument"]["uri"].as_str().ok_or("missing document URI")?;
            assert_eq!(document["textDocument"]["version"], 1, "opened document version");
            let edits = document["edits"].as_array().ok_or("missing document edits")?;
            assert!(changes.insert(uri, edits).is_none(), "duplicate document");
        }
    } else {
        assert!(response.get("documentChanges").is_none(), "legacy changes: {response}");
        for (uri, edits) in response["changes"].as_object().ok_or("missing changes")? {
            changes.insert(uri.as_str(), edits.as_array().ok_or("missing edits")?);
        }
    }
    assert_eq!(
        changes.keys().copied().collect::<Vec<_>>(),
        vec![lib_uri.as_str(), app_uri.as_str()]
    );
    let lib_edits = changes[lib_uri.as_str()];
    let app_edits = changes[app_uri.as_str()];
    assert_eq!(lib_edits.len(), 1, "exactly one declaration: {response}");
    assert_eq!(app_edits.len(), 1, "exactly one instance call: {response}");
    assert_eq!(
        apply(&lib, lib_edits, old, new)?,
        format!(
            "package Greeter;\r\nuse utf8;\r\nsub new {{ return bless {{}}, shift }}\r\nsub {new} {{ return 1; }}\r\n1;\r\n"
        )
    );
    assert_eq!(
        apply(&app, app_edits, old, new)?,
        format!(
            "package Caller;\r\nuse utf8;\r\nuse Greeter;\r\nsub run {{\r\nmy ${old} = Greeter->new();\r\n\
             print \"😀\"; ${old}->{new}(${old});\r\n# {old} and ${old}->{old} are text\r\n\
             my $literal = '{old}';\r\nmy ${old}_long = 0;\r\n}}\r\n1;\r\n"
        ),
        "receiver, argument, text and longer identifier must be preserved"
    );
    Ok(())
}

#[test]
fn receiver_spelling_ascii_changes() -> TestResult {
    receiver_rename("greet", "saluer", false)
}

#[test]
fn receiver_spelling_unicode_document_changes() -> TestResult {
    receiver_rename("café", "saluer", true)
}

#[test]
fn receiver_spelling_unicode_changes() -> TestResult {
    receiver_rename("café", "saluer", false)
}

#[test]
fn receiver_spelling_ascii_document_changes() -> TestResult {
    receiver_rename("greet", "saluer", true)
}
