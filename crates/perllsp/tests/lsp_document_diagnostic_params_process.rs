//! Diagnostic cache parameter refusal and recovery through the public process.
#![deny(clippy::map_err_ignore)]

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const URI: &str = "file:///diagnostic-params.pl";
const CLEAN: &str = "use strict;\nuse warnings;\nsub marker { return 1; }\nmarker();\n";
const BROKEN: &str = "use strict;\nuse warnings;\nsub marker {\n";
const TIMEOUT: Duration = Duration::from_secs(10);

fn success(client: &mut RealProcessClient, method: &str, params: Value) -> Result<Value> {
    let response = client.request(json!(method), method, params, TIMEOUT)?;
    ensure!(response.get("error").is_none(), "{method} failed: {response}");
    response.get("result").cloned().context("response omitted result")
}

fn start(mode: &str) -> Result<RealProcessClient> {
    let mut client = RealProcessClient::spawn_exact_with_env(&[
        ("PERL_LSP_DIAGNOSTIC_MODE", mode),
        ("RUST_LOG", "warn"),
    ])?;
    let root_uri = url::Url::from_file_path(client.workspace_path())
        .map_err(|()| anyhow::anyhow!("fixture workspace cannot be expressed as a file URI"))?;
    let initialized = success(
        &mut client,
        "initialize",
        json!({
            "processId": null,
            "rootUri": root_uri.as_str(),
            "workspaceFolders": [{"uri": root_uri.as_str(), "name": "fixture"}],
            "capabilities": {"textDocument": {"diagnostic": {}}},
            "initializationOptions": {"perl": {"workspace": {"useSystemInc": false}}}
        }),
    )?;
    ensure!(initialized.pointer("/capabilities/diagnosticProvider").is_some());
    client.notify("initialized", json!({}))?;
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": URI, "languageId": "perl", "version": 1, "text": CLEAN}}),
    )?;
    wait_for_parse(&mut client)?;
    ensure!(
        !has_parser_error(&report(&mut client, None)?),
        "clean didOpen must lack parser errors"
    );
    Ok(client)
}

fn wait_for_parse(client: &mut RealProcessClient) -> Result<()> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let result = success(
            client,
            "textDocument/semanticTokens/full",
            json!({"textDocument": {"uri": URI}}),
        )?;
        if result.get("resultId").and_then(Value::as_str).is_some()
            && result.get("data").and_then(Value::as_array).is_some_and(|data| !data.is_empty())
        {
            return Ok(());
        }
        ensure!(Instant::now() < deadline, "current parse never became ready: {result}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn change(client: &mut RealProcessClient, version: i32, text: &str) -> Result<()> {
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": URI, "version": version}, "contentChanges": [{"text": text}]}),
    )?;
    wait_for_parse(client)
}

fn params(previous: Option<Value>) -> Value {
    let mut params = json!({"textDocument": {"uri": URI}});
    if let Some(previous) = previous {
        params["previousResultId"] = previous;
    }
    params
}

fn report(client: &mut RealProcessClient, previous: Option<Value>) -> Result<Value> {
    success(client, "textDocument/diagnostic", params(previous))
}

fn has_parser_error(report: &Value) -> bool {
    report.get("items").and_then(Value::as_array).is_some_and(|items| {
        items
            .iter()
            .any(|item| matches!(item.get("code").and_then(Value::as_str), Some("PL001" | "PL002")))
    })
}

fn assert_unclosed_block(report: &Value, mode: &str) -> Result<()> {
    let expected_code = if mode == "normal" { "PL002" } else { "PL001" };
    let items =
        report.get("items").and_then(Value::as_array).context("missing diagnostic items")?;
    ensure!(
        items.iter().any(|item| {
            item.get("code") == Some(&json!(expected_code))
                && item.get("severity") == Some(&json!(1))
                && item
                    .get("message")
                    .and_then(Value::as_str)
                    .is_some_and(|message| message.starts_with("Unclosed block: expected '}'"))
        }),
        "expected {mode} unclosed-block error {expected_code}: {report}"
    );
    Ok(())
}

fn finish(mut client: RealProcessClient) -> Result<()> {
    ensure!(success(&mut client, "shutdown", Value::Null)?.is_null());
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(TIMEOUT)?.success(), "server did not exit cleanly");
    client.assert_transport_clean()
}

#[test]
fn accepted_result_ids_keep_cached_and_edited_diagnostics_current() -> Result<()> {
    let mut client = start("normal")?;
    for previous in [None, Some(Value::Null), Some(json!("")), Some(json!("unknown-result"))] {
        let clean = report(&mut client, previous)?;
        ensure!(clean.get("kind") == Some(&json!("full")), "unknown/absent ID: {clean}");
        ensure!(!has_parser_error(&clean));
    }
    let clean = report(&mut client, None)?;
    let clean_id = clean.get("resultId").and_then(Value::as_str).context("missing clean ID")?;
    let cached = report(&mut client, Some(json!(clean_id)))?;
    ensure!(cached.get("kind") == Some(&json!("unchanged")), "cache control: {cached}");
    ensure!(cached.get("resultId") == Some(&json!(clean_id)));

    change(&mut client, 2, BROKEN)?;
    let broken = report(&mut client, Some(json!(clean_id)))?;
    assert_unclosed_block(&broken, "normal")?;
    ensure!(has_parser_error(&broken), "broken didChange must retain its parser error: {broken}");
    let broken_id = broken.get("resultId").and_then(Value::as_str).context("missing broken ID")?;
    ensure!(broken_id != clean_id, "an edit must invalidate the old diagnostic ID");
    change(&mut client, 3, CLEAN)?;
    let recovered = report(&mut client, Some(json!(broken_id)))?;
    ensure!(recovered.get("kind") == Some(&json!("full")), "recovery: {recovered}");
    ensure!(
        !has_parser_error(&recovered),
        "recovered code retained its stale parser error: {recovered}"
    );
    finish(client)
}

fn malformed_result_id_journey(mode: &str) -> Result<()> {
    let mut client = start(mode)?;
    for previous in [None, Some(Value::Null), Some(json!("")), Some(json!("unknown-result"))] {
        let clean = report(&mut client, previous)?;
        ensure!(clean.get("kind") == Some(&json!("full")), "accepted parameter control: {clean}");
        ensure!(!has_parser_error(&clean));
    }
    change(&mut client, 2, BROKEN)?;
    let broken = report(&mut client, None)?;
    assert_unclosed_block(&broken, mode)?;
    ensure!(has_parser_error(&broken), "positive syntax error control: {broken}");
    let previous = broken.get("resultId").cloned();
    let mut failures = Vec::new();
    for (index, invalid) in
        [json!(7), json!(-1.5), json!(false), json!([]), json!({}), json!(["id"])]
            .into_iter()
            .enumerate()
    {
        let response = client.request(
            json!(format!("invalid-{index}")),
            "textDocument/diagnostic",
            params(Some(invalid.clone())),
            TIMEOUT,
        )?;
        let message = response.pointer("/error/message").and_then(Value::as_str).unwrap_or("");
        if response.pointer("/error/code") != Some(&json!(-32602))
            || response.get("result").is_some()
            || !message.contains("previousResultId")
            || !message.contains("string")
            || !message.contains("omit")
        {
            failures.push(format!(
                "previousResultId={invalid} expected actionable InvalidParams, got {response}"
            ));
        }
    }
    // Rejections must leave a valid same-session request and its cache usable.
    let recovered_request = report(&mut client, previous.clone())?;
    if mode == "normal" {
        ensure!(
            recovered_request.get("kind") == Some(&json!("unchanged")),
            "valid request after errors: {recovered_request}"
        );
        ensure!(recovered_request.get("resultId") == previous.as_ref());
    } else {
        ensure!(has_parser_error(&recovered_request), "syntax-only recovery lost its parser error");
    }
    change(&mut client, 3, CLEAN)?;
    let clean = report(&mut client, None)?;
    ensure!(!has_parser_error(&clean), "repaired source retained its parser error: {clean}");
    finish(client)?;
    ensure!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[test]
fn malformed_result_ids_refuse_and_recover_in_normal_mode() -> Result<()> {
    malformed_result_id_journey("normal")
}

#[test]
fn malformed_result_ids_refuse_and_recover_in_syntax_only_mode() -> Result<()> {
    malformed_result_id_journey("syntax-only")
}

#[test]
fn malformed_result_ids_refuse_for_clean_and_closed_documents() -> Result<()> {
    let mut client = start("normal")?;
    let mut failures = Vec::new();
    for phase in ["clean", "closed"] {
        if phase == "closed" {
            client.notify("textDocument/didClose", json!({"textDocument": {"uri": URI}}))?;
        }
        let accepted = report(&mut client, None)?;
        ensure!(accepted.get("kind") == Some(&json!("full")), "valid {phase} request: {accepted}");
        for invalid in [json!(7), json!(true), json!({"resultId": "id"})] {
            let response = client.request(
                json!(format!("{phase}-{invalid}")),
                "textDocument/diagnostic",
                params(Some(invalid.clone())),
                TIMEOUT,
            )?;
            if response.pointer("/error/code") != Some(&json!(-32602)) {
                failures
                    .push(format!("{phase} previousResultId={invalid} was accepted: {response}"));
            }
        }
    }
    finish(client)?;
    ensure!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}
