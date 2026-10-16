//! Exact public completion edits after sequential full replacements and reopen.
#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Result, anyhow, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn complete(
    client: &mut RealProcessClient,
    uri: &str,
    case: usize,
    line: u32,
    character: u32,
) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut attempt = 0;
    loop {
        let response = client.request(
            json!(format!("completion-{case}-{attempt}")),
            "textDocument/completion",
            json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character},
                   "context": {"triggerKind": 1}}),
            Duration::from_secs(3),
        )?;
        ensure!(response.get("error").is_none(), "completion failed: {response}");
        let result = response.get("result").ok_or_else(|| anyhow!("missing result: {response}"))?;
        if result.get("isIncomplete").and_then(Value::as_bool) != Some(true) {
            return Ok(result.clone());
        }
        ensure!(
            Instant::now() < deadline,
            "current parsed completion never became ready: {result}"
        );
        attempt += 1;
        std::thread::sleep(Duration::from_millis(5));
    }
}

// Apply the wire edit in UTF-16 units, independently of the server's byte mapper.
fn apply_edit(source: &str, edit: &Value) -> Result<String> {
    let mut units: Vec<u16> = source.encode_utf16().collect();
    let offset = |position: &Value| -> Result<usize> {
        let line =
            usize::try_from(position["line"].as_u64().ok_or_else(|| anyhow!("missing line"))?)?;
        let column = usize::try_from(
            position["character"].as_u64().ok_or_else(|| anyhow!("missing character"))?,
        )?;
        let mut start = 0;
        for _ in 0..line {
            start += units[start..]
                .iter()
                .position(|unit| *unit == 10)
                .ok_or_else(|| anyhow!("missing source line"))?
                + 1;
        }
        Ok(start + column)
    };
    let start = offset(&edit["range"]["start"])?;
    let end = offset(&edit["range"]["end"])?;
    ensure!(start <= end && end <= units.len(), "edit outside source: {edit}");
    let replacement = edit["newText"].as_str().ok_or_else(|| anyhow!("missing newText"))?;
    units.splice(start..end, replacement.encode_utf16());
    Ok(String::from_utf16(&units)?)
}

#[test]
fn arrow_method_edits_replace_suffix_after_sequential_edits_and_reopen() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    ensure!(client.candidate_name() == "perllsp", "public facade must be tested");
    let initialized = client.request(
        json!("initialize"),
        "initialize",
        json!({"processId": null, "capabilities": {
            "general": {"positionEncodings": ["utf-16"]},
            "textDocument": {"completion": {"completionItem": {"snippetSupport": false}}}
        }}),
        Duration::from_secs(10),
    )?;
    ensure!(initialized.get("error").is_none(), "initialize failed: {initialized}");
    client.notify("initialized", json!({}))?;
    let uri = "file:///completion-method-reopen.pl";
    let header =
        "package Pet;\nsub speak {}\nsub sprint {}\npackage main;\nmy $dog = Pet->new();\n";
    // Cursor positions and complete replacement ranges are literal UTF-16 oracles.
    let cases = [
        ("$dog->speak;\n", "$dog->speak();\n", 5, 8, 6, 11, "speak", false),
        (
            "print \"😀\"; $dog->sprint, 42;\n",
            "print \"😀\"; $dog->sprint(), 42;\n",
            5,
            20,
            18,
            24,
            "sprint",
            false,
        ),
        (
            "# shifted\n$dog->speak + 42;\n",
            "# shifted\n$dog->speak() + 42;\n",
            6,
            6,
            6,
            11,
            "speak",
            false,
        ),
        (
            "print \"😀\"; $dog->spell;\r\n",
            "print \"😀\"; $dog->spell();\r\n",
            5,
            20,
            18,
            23,
            "spell",
            true,
        ),
        ("$dog->sprint;\n", "$dog->sprint();\n", 5, 12, 6, 12, "sprint", false),
        ("$dog->sp\n", "$dog->speak()\n", 5, 8, 6, 8, "speak", false),
    ];
    for (index, (tail, expected_tail, line, cursor, start, end, label, reopen)) in
        cases.into_iter().enumerate()
    {
        // Change a declaration while preserving its prefix, including across reopen.
        let peer = match index {
            1 => "speed",
            3 => "spell",
            _ => "speak",
        };
        let header = header.replace("speak", peer);
        let source = format!("{header}{tail}");
        if reopen {
            client.notify("textDocument/didClose", json!({"textDocument": {"uri": uri}}))?;
        }
        if index == 0 || reopen {
            client.notify(
                "textDocument/didOpen",
                json!({"textDocument": {
                    "uri": uri, "languageId": "perl", "version": 1, "text": source
                }}),
            )?;
        } else {
            client.notify(
                "textDocument/didChange",
                json!({"textDocument": {
                "uri": uri, "version": index + 2
            }, "contentChanges": [{"text": source}]}),
            )?;
        }
        let result = complete(&mut client, uri, index, line, cursor)?;
        let items = result["items"].as_array().ok_or_else(|| anyhow!("missing items: {result}"))?;
        let item = items
            .iter()
            .find(|item| item["label"] == label)
            .ok_or_else(|| anyhow!("case {index} missing {label}: {result}"))?;
        let expected_edit = json!({"range": {
            "start": {"line": line, "character": start},
            "end": {"line": line, "character": end}
        }, "newText": format!("{label}()")});
        ensure!(item["textEdit"] == expected_edit, "case {index} wrong exact edit: {item}");
        ensure!(
            apply_edit(&source, &item["textEdit"])? == format!("{header}{expected_tail}"),
            "case {index} must replace the method once and preserve surrounding source"
        );
        // A candidate remains present and filtering uses only the pre-cursor prefix.
        // This rules out passing by treating the suffix as a required prefix.
        if cursor == 8 || cursor == 20 {
            for other in [peer, "sprint"] {
                ensure!(
                    items.iter().any(|item| item["label"] == other),
                    "case {index} lost prefix candidate {other}: {result}"
                );
            }
        }
        for obsolete in ["speak", "speed", "spell"].into_iter().filter(|name| *name != peer) {
            ensure!(
                !items.iter().any(|item| item["label"] == obsolete),
                "case {index} retained obsolete declaration {obsolete}: {result}"
            );
        }
    }
    let shutdown =
        client.request(json!("shutdown"), "shutdown", json!(null), Duration::from_secs(3))?;
    ensure!(shutdown.get("error").is_none(), "shutdown failed: {shutdown}");
    client.notify("exit", json!(null))?;
    let status = client.wait_for_exit(Duration::from_secs(5))?;
    ensure!(status.success(), "public server failed clean shutdown: {status}");
    client.assert_transport_clean()?;
    Ok(())
}
