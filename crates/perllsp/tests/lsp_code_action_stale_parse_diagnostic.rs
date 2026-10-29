//! Closing-brace quick fixes must preserve scope and reject stale diagnostics.
//! Complete output is an independent literal oracle: clean reparse alone also
//! accepts the wrong edit that closes an empty subroutine at its opening brace.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, bail, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const URI: &str = "file:///closing-brace-action.pl";
const BROKEN: &str =
    "use strict;\nuse warnings;\nsub value {\n    my $answer = 1;\n    return $answer;\n";
const EXPECTED: &str =
    "use strict;\nuse warnings;\nsub value {\n    my $answer = 1;\n    return $answer;\n\n}";

fn request(client: &mut RealProcessClient, id: &str, method: &str, params: Value) -> Result<Value> {
    let response = client.request(json!(id), method, params, Duration::from_secs(10))?;
    ensure!(response.get("id") == Some(&json!(id)), "wrong response ID: {response}");
    ensure!(response.get("error").is_none(), "{method} failed: {response}");
    response.get("result").cloned().context("response omitted result")
}

fn start() -> Result<RealProcessClient> {
    let mut client = RealProcessClient::spawn_exact()?;
    request(
        &mut client,
        "initialize",
        "initialize",
        json!({"processId": null, "rootUri": null, "capabilities": {
            "general": {"positionEncodings": ["utf-16"]},
            "textDocument": {"diagnostic": {}}
        }}),
    )?;
    client.notify("initialized", json!({}))?;
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": URI, "languageId": "perl", "version": 1, "text": BROKEN
        }}),
    )?;
    wait_for_parse(&mut client)?;
    Ok(client)
}

fn wait_for_parse(client: &mut RealProcessClient) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let result = request(
            client,
            "parse",
            "textDocument/semanticTokens/full",
            json!({"textDocument": {"uri": URI}}),
        )?;
        if result.get("resultId").and_then(Value::as_str).is_some()
            && result.get("data").and_then(Value::as_array).is_some_and(|data| !data.is_empty())
        {
            return Ok(());
        }
        ensure!(Instant::now() < deadline, "current parse not published: {result}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn change(client: &mut RealProcessClient, version: i32, text: &str) -> Result<()> {
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": URI, "version": version},
            "contentChanges": [{"text": text}]
        }),
    )?;
    wait_for_parse(client)
}

fn diagnostics(client: &mut RealProcessClient) -> Result<Vec<Value>> {
    let report = request(
        client,
        "diagnostics",
        "textDocument/diagnostic",
        json!({"textDocument": {"uri": URI}}),
    )?;
    ensure!(report.get("kind") == Some(&json!("full")), "expected full report: {report}");
    report.get("items").and_then(Value::as_array).cloned().context("missing diagnostic items")
}

fn code_actions(
    client: &mut RealProcessClient,
    text: &str,
    diagnostics: &[Value],
) -> Result<Vec<Value>> {
    // All fixtures are ASCII; count UTF-16 line coordinates independently.
    let last_line = text.rsplit('\n').next().context("fixture has no last line")?;
    let result = request(
        client,
        "actions",
        "textDocument/codeAction",
        json!({
            "textDocument": {"uri": URI},
            "range": {"start": {"line": 0, "character": 0}, "end": {
                "line": text.bytes().filter(|byte| *byte == b'\n').count(),
                "character": last_line.len()
            }},
            "context": {"only": ["quickfix"], "diagnostics": diagnostics}
        }),
    )?;
    let actions = result.as_array().context("codeAction result is not an array")?;
    Ok(actions.clone())
}

fn brace_actions(
    client: &mut RealProcessClient,
    text: &str,
    diagnostics: &[Value],
) -> Result<Vec<Value>> {
    Ok(code_actions(client, text, diagnostics)?
        .iter()
        .filter(|action| {
            action
                .get("title")
                .and_then(Value::as_str)
                .is_some_and(|title| title.starts_with("Add closing brace"))
        })
        .cloned()
        .collect())
}

fn offset(text: &str, position: &Value) -> Result<usize> {
    ensure!(text.is_ascii(), "this independent coordinate oracle uses ASCII fixtures");
    let line = position.get("line").and_then(Value::as_u64).context("missing line")? as usize;
    let character =
        position.get("character").and_then(Value::as_u64).context("missing character")? as usize;
    let mut start = 0;
    for _ in 0..line {
        start += text[start..].find('\n').context("line outside fixture")? + 1;
    }
    let end = text[start..].find('\n').map_or(text.len(), |relative| start + relative);
    ensure!(character <= end - start, "character outside fixture");
    Ok(start + character)
}

fn apply(text: &str, action: &Value) -> Result<String> {
    let changes =
        action.pointer("/edit/changes").and_then(Value::as_object).context("missing changes")?;
    ensure!(changes.len() == 1, "brace action must edit only its document: {action}");
    let edits = changes.get(URI).and_then(Value::as_array).context("missing document edits")?;
    ensure!(!edits.is_empty(), "brace action must contain an edit");
    let mut replacements = Vec::new();
    for edit in edits {
        let start = offset(text, &edit["range"]["start"])?;
        let end = offset(text, &edit["range"]["end"])?;
        ensure!(start <= end, "reversed edit range");
        let replacement = edit.get("newText").and_then(Value::as_str).context("missing newText")?;
        replacements.push((start, end, replacement));
    }
    replacements.sort_by_key(|(start, end, _)| (*start, *end));
    let mut result = text.to_string();
    for (start, end, replacement) in replacements.into_iter().rev() {
        result.replace_range(start..end, replacement);
    }
    Ok(result)
}

fn finish(mut client: RealProcessClient) -> Result<()> {
    ensure!(
        request(&mut client, "shutdown", "shutdown", Value::Null)?.is_null(),
        "shutdown must return null"
    );
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(Duration::from_secs(10))?.success(), "server exit failed");
    client.assert_transport_clean()
}

#[test]
fn current_closing_brace_edit_preserves_literal_body_and_reparses() -> Result<()> {
    let mut client = start()?;
    let diagnostic = diagnostics(&mut client)?;
    ensure!(
        diagnostic.iter().any(|d| d.get("code") == Some(&json!("PL002"))),
        "missing genuine PL002: {diagnostic:?}"
    );
    // Empty context proves current parser findings still drive the action.
    let actions = brace_actions(&mut client, BROKEN, &[])?;
    ensure!(actions.len() == 1, "expected one current brace action: {actions:?}");
    let opening_brace = json!({
        "start": {"line": 2, "character": 10},
        "end": {"line": 2, "character": 11}
    });
    ensure!(
        diagnostic
            .iter()
            .any(|d| d.get("code") == Some(&json!("PL002"))
                && d.get("range") == Some(&opening_brace)),
        "unclosed-block diagnostic lost its opening-brace anchor: {diagnostic:?}"
    );
    ensure!(
        actions[0]
            .get("diagnostics")
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().any(|d| d.get("code") == Some(&json!("PL002"))
                && d.get("range") == Some(&opening_brace))),
        "brace action lost the opening-brace diagnostic: {:?}",
        actions[0]
    );
    let applied = apply(BROKEN, &actions[0])?;
    ensure!(applied == EXPECTED, "brace edit moved or changed the literal body: {applied:?}");
    change(&mut client, 2, &applied)?;
    ensure!(diagnostics(&mut client)?.is_empty(), "applied brace repair did not reparse cleanly");
    finish(client)
}

#[test]
fn non_parser_context_remains_available_for_diagnostic_explanation() -> Result<()> {
    let mut client = start()?;
    change(&mut client, 2, EXPECTED)?;
    ensure!(diagnostics(&mut client)?.is_empty(), "literal repair must parse cleanly");
    let supplied = json!({
        "code": "PL701", "source": "perl-lsp", "severity": 2,
        "message": "Module 'Missing::Payload' not found in configured @INC paths.",
        "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 3}}
    });
    let empty = code_actions(&mut client, EXPECTED, &[])?;
    ensure!(
        !empty
            .iter()
            .any(|a| a.pointer("/command/command") == Some(&json!("perl-lsp.explainDiagnostic"))),
        "empty context unexpectedly explains a missing module"
    );
    let contextual = code_actions(&mut client, EXPECTED, &[supplied])?;
    ensure!(
        contextual
            .iter()
            .any(|a| a.pointer("/command/command") == Some(&json!("perl-lsp.explainDiagnostic"))),
        "non-parser diagnostic context was dropped: {contextual:?}"
    );
    finish(client)
}

#[test]
fn closing_brace_is_not_swallowed_by_a_trailing_comment() -> Result<()> {
    let mut client = start()?;
    let broken = "use strict;\nuse warnings;\nsub value {\n    return 1; # trailing comment";
    let expected = "use strict;\nuse warnings;\nsub value {\n    return 1; # trailing comment\n}";
    change(&mut client, 2, broken)?;
    let actions = brace_actions(&mut client, broken, &[])?;
    ensure!(actions.len() == 1, "expected one current brace repair: {actions:?}");
    let applied = apply(broken, &actions[0])?;
    ensure!(applied == expected, "brace must close the block after the comment: {applied:?}");
    change(&mut client, 3, &applied)?;
    ensure!(diagnostics(&mut client)?.is_empty(), "comment-aware repair must reparse cleanly");
    finish(client)
}

#[test]
fn one_brace_cannot_repair_two_unclosed_blocks() -> Result<()> {
    let mut client = start()?;
    let broken = "use strict;\nuse warnings;\nsub value {\n    if (1) {\n        return 1;\n";
    change(&mut client, 2, broken)?;
    let findings = diagnostics(&mut client)?;
    ensure!(
        findings.iter().filter(|d| d.get("code") == Some(&json!("PL002"))).count() == 2,
        "nested control requires two real unclosed-block diagnostics: {findings:?}"
    );
    ensure!(
        brace_actions(&mut client, broken, &[])?.is_empty(),
        "a single brace must not be offered as a complete nested-block repair"
    );
    finish(client)
}

#[test]
fn repaired_document_rejects_stale_and_shifted_parser_diagnostics() -> Result<()> {
    let mut client = start()?;
    let stale = diagnostics(&mut client)?;
    ensure!(
        stale.iter().any(|d| d.get("code") == Some(&json!("PL002"))),
        "missing genuine PL002: {stale:?}"
    );
    change(&mut client, 2, EXPECTED)?;
    ensure!(diagnostics(&mut client)?.is_empty(), "literal repair control must parse cleanly");
    ensure!(
        brace_actions(&mut client, EXPECTED, &[])?.is_empty(),
        "clean document must not offer a brace edit"
    );
    ensure!(
        brace_actions(&mut client, EXPECTED, &stale)?.is_empty(),
        "still-in-bounds stale PL002 authorized a brace edit"
    );
    let shifted = "# shifted source\nuse strict;\nuse warnings;\nsub value {\n    my $answer = 1;\n    return $answer;\n\n}";
    change(&mut client, 3, shifted)?;
    ensure!(diagnostics(&mut client)?.is_empty(), "shifted control must parse cleanly");
    ensure!(
        brace_actions(&mut client, shifted, &stale)?.is_empty(),
        "shifted stale PL002 authorized a brace edit"
    );
    // Legacy parser subtype context must not bypass the same boundary.
    let mut legacy = stale;
    for diagnostic in &mut legacy {
        diagnostic["code"] = json!("parse-error-unclosedbrace");
    }
    if !brace_actions(&mut client, shifted, &legacy)?.is_empty() {
        bail!("legacy parser subtype context manufactured a brace edit");
    }
    finish(client)
}
