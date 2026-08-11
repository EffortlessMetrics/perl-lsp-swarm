//! Exact-process document lifecycle proof.
//!
//! The tests use Cargo's exact candidate and observe current-generation parse
//! publication through semantic tokens, whose live path requires
//! `DocumentState::current_parsed()`. They also cover document symbols, pull
//! diagnostics, close ownership, stale-version rejection, and settled
//! close/reopen state reset. They do not hold an old parse in flight across a
//! close/reopen boundary; deterministic parse-worker barrier tests own that ABA
//! race. They also do not claim incremental parser reuse; #1374 remains the
//! performance and reuse owner. The required `lsp_smoke` gate includes this
//! file from `semantic_definition.rs`.

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, bail, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const URI: &str = "file:///document-lifecycle.pl";
const CLOSED_SEMANTIC_TOKENS_MESSAGE: &str = "Document not open: file:///document-lifecycle.pl. textDocument/semanticTokens/full requires the editor to send textDocument/didOpen before requesting tokens; resend after the document is open and synchronized.";

fn timeout() -> Duration {
    Duration::from_secs(10)
}

fn assert_success_response(response: &Value, id: &Value, method: &str) -> Result<Value> {
    ensure!(
        response.get("jsonrpc") == Some(&json!("2.0")),
        "{method} response omitted JSON-RPC 2.0: {response}"
    );
    ensure!(response.get("id") == Some(id), "{method} response ID mismatch: {response}");
    ensure!(response.get("error").is_none(), "{method} returned an error: {response}");
    response
        .get("result")
        .cloned()
        .with_context(|| format!("{method} response omitted result: {response}"))
}

fn request_success(
    client: &mut RealProcessClient,
    id: &str,
    method: &str,
    params: Value,
) -> Result<Value> {
    let id = json!(id);
    let response = client.request(id.clone(), method, params, timeout())?;
    assert_success_response(&response, &id, method)
}

fn initialize(client: &mut RealProcessClient) -> Result<()> {
    let result = request_success(
        client,
        "initialize",
        "initialize",
        json!({
            "processId": null,
            "clientInfo": {
                "name": "perl-lsp-document-lifecycle",
                "version": "1"
            },
            "rootUri": null,
            "capabilities": {
                "general": {
                    "positionEncodings": ["utf-16"]
                },
                "textDocument": {
                    "diagnostic": {
                        "dynamicRegistration": false,
                        "relatedDocumentSupport": true
                    }
                }
            },
            "workspaceFolders": null
        }),
    )?;
    ensure!(
        result.get("capabilities").is_some(),
        "initialize result omitted capabilities: {result}"
    );
    client.notify("initialized", json!({}))
}

fn finish(client: &mut RealProcessClient) -> Result<()> {
    let result = request_success(client, "shutdown", "shutdown", Value::Null)?;
    ensure!(result.is_null(), "shutdown must return null: {result}");
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(timeout())?;
    ensure!(
        status.success(),
        "candidate exited unsuccessfully: {status}; stderr={}",
        client.stderr_tail()
    );
    client.assert_transport_clean()
}

fn did_open(client: &mut RealProcessClient, version: i32, text: &str) -> Result<()> {
    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": URI,
                "languageId": "perl",
                "version": version,
                "text": text
            }
        }),
    )
}

fn did_change(client: &mut RealProcessClient, version: i32, text: &str) -> Result<()> {
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {
                "uri": URI,
                "version": version
            },
            "contentChanges": [
                { "text": text }
            ]
        }),
    )
}

fn did_close(client: &mut RealProcessClient) -> Result<()> {
    client.notify(
        "textDocument/didClose",
        json!({
            "textDocument": { "uri": URI }
        }),
    )
}

fn document_symbol_names(client: &mut RealProcessClient, id: &str) -> Result<Vec<String>> {
    let result = request_success(
        client,
        id,
        "textDocument/documentSymbol",
        json!({
            "textDocument": { "uri": URI }
        }),
    )?;
    let mut names = Vec::new();
    collect_symbol_names(&result, &mut names);
    Ok(names)
}

fn collect_symbol_names(value: &Value, names: &mut Vec<String>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_symbol_names(item, names);
            }
        }
        Value::Object(object) => {
            if let Some(name) = object.get("name").and_then(Value::as_str) {
                names.push(name.to_string());
            }
            if let Some(children) = object.get("children") {
                collect_symbol_names(children, names);
            }
        }
        _ => {}
    }
}

fn wait_for_current_parse_tokens(
    client: &mut RealProcessClient,
    id_prefix: &str,
) -> Result<Vec<u64>> {
    let deadline = Instant::now() + timeout();
    let mut attempt = 0u32;
    let mut last_result = Value::Null;

    loop {
        let id = format!("{id_prefix}-{attempt}");
        let result = request_success(
            client,
            &id,
            "textDocument/semanticTokens/full",
            json!({
                "textDocument": { "uri": URI }
            }),
        )?;
        let has_live_result = result.get("resultId").and_then(Value::as_str).is_some();
        let data = result.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
        if has_live_result && !data.is_empty() {
            ensure!(data.len() % 5 == 0, "semantic-token data must use five-u32 tuples: {result}");
            return data
                .iter()
                .map(|value| {
                    value.as_u64().with_context(|| {
                        format!("semantic-token data item was not an integer: {result}")
                    })
                })
                .collect();
        }

        last_result = result;
        if Instant::now() >= deadline {
            bail!(
                "current-generation parsed snapshot was not published before timeout; last semantic-token result={last_result}"
            );
        }
        attempt = attempt.saturating_add(1);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn diagnostic_items(client: &mut RealProcessClient, id: &str) -> Result<Vec<Value>> {
    let result = request_success(
        client,
        id,
        "textDocument/diagnostic",
        json!({
            "textDocument": { "uri": URI },
            "identifier": "perl-lsp",
            "previousResultId": null
        }),
    )?;
    ensure!(
        result.get("kind").and_then(Value::as_str) == Some("full"),
        "pull diagnostics must return a full report: {result}"
    );
    result
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .context("pull diagnostic report omitted items")
}

fn diagnostic_fingerprint(item: &Value) -> Result<String> {
    let code = item.get("code").cloned().unwrap_or(Value::Null);
    let range = item.get("range").cloned().unwrap_or(Value::Null);
    let source = item.get("source").cloned().unwrap_or(Value::Null);
    let message = item.get("message").cloned().unwrap_or(Value::Null);
    Ok(format!(
        "{}|{}|{}|{}",
        serde_json::to_string(&code)?,
        serde_json::to_string(&range)?,
        serde_json::to_string(&source)?,
        serde_json::to_string(&message)?
    ))
}

fn diagnostic_fingerprints(items: &[Value]) -> Result<Vec<String>> {
    let mut fingerprints = items.iter().map(diagnostic_fingerprint).collect::<Result<Vec<_>>>()?;
    fingerprints.sort();
    Ok(fingerprints)
}

fn is_parser_diagnostic(item: &Value) -> Result<bool> {
    if item.get("source").and_then(Value::as_str) != Some("perl-lsp") {
        return Ok(false);
    }

    let Some(code) = item.get("code") else {
        return Ok(false);
    };
    let number = match code {
        Value::Number(number) => {
            number.as_u64().context("parser diagnostic numeric code must be an unsigned integer")?
        }
        Value::String(code) => {
            let Some(digits) = code.strip_prefix("PL") else {
                return Ok(false);
            };
            if digits.len() != 3 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return Ok(false);
            }
            digits
                .parse::<u64>()
                .with_context(|| format!("invalid parser diagnostic code {code:?}"))?
        }
        Value::Null => return Ok(false),
        other => bail!("unsupported diagnostic code kind: {other}"),
    };
    Ok((1..=99).contains(&number))
}

fn parser_diagnostic_fingerprints(items: &[Value]) -> Result<Vec<String>> {
    let mut fingerprints = Vec::new();
    for item in items {
        if is_parser_diagnostic(item)? {
            fingerprints.push(diagnostic_fingerprint(item)?);
        }
    }
    fingerprints.sort();
    Ok(fingerprints)
}

#[test]
fn parser_diagnostic_classifier_rejects_policy_codes_and_wrong_sources() -> Result<()> {
    let parser = json!({
        "source": "perl-lsp",
        "code": "PL001",
        "message": "parser",
        "range": null
    });
    ensure!(is_parser_diagnostic(&parser)?);

    for non_parser in [
        json!({"source": "perl-lsp", "code": "PL100"}),
        json!({"source": "perl-lsp", "code": "PL000"}),
        json!({"source": "perl-lsp", "code": "PL01"}),
        json!({"source": "perl-lsp", "code": 100}),
        json!({"source": "perlcritic", "code": "PL001"}),
        json!({"code": "PL001"}),
    ] {
        ensure!(
            !is_parser_diagnostic(&non_parser)?,
            "non-parser diagnostic was accepted: {non_parser}"
        );
    }

    let malformed = json!({"source": "perl-lsp", "code": true});
    ensure!(
        is_parser_diagnostic(&malformed).is_err(),
        "malformed diagnostic code kind was silently accepted"
    );
    Ok(())
}

#[test]
fn increasing_change_publishes_current_parse_and_stale_change_is_ignored() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client)?;

    did_open(&mut client, 1, "package Initial;\nsub initial_symbol { return 1; }\n")?;
    let initial_tokens = wait_for_current_parse_tokens(&mut client, "tokens-v1")?;
    ensure!(!initial_tokens.is_empty(), "version 1 current parse produced no semantic tokens");
    let initial_names = document_symbol_names(&mut client, "symbols-v1")?;
    ensure!(
        initial_names.iter().any(|name| name.contains("initial_symbol")),
        "version 1 symbol was not visible: {initial_names:?}"
    );

    did_change(
        &mut client,
        2,
        "package Current;\nsub current_symbol { return 2; }\nsub broken {\n",
    )?;
    let current_tokens = wait_for_current_parse_tokens(&mut client, "tokens-v2")?;
    let current_names = document_symbol_names(&mut client, "symbols-v2")?;
    ensure!(
        current_names.iter().any(|name| name.contains("current_symbol")),
        "version 2 symbol was not published: {current_names:?}"
    );
    ensure!(
        !current_names.iter().any(|name| name.contains("initial_symbol")),
        "version 1 symbol survived version 2 publication: {current_names:?}"
    );
    let current_diagnostics = diagnostic_items(&mut client, "diagnostics-v2")?;
    let current_parser_fingerprints = parser_diagnostic_fingerprints(&current_diagnostics)?;
    ensure!(
        !current_parser_fingerprints.is_empty(),
        "broken version 2 source must produce parser diagnostics: {current_diagnostics:?}"
    );
    let current_fingerprints = diagnostic_fingerprints(&current_diagnostics)?;

    did_change(&mut client, 1, "package Stale;\nsub stale_symbol { return 3; }\n")?;
    let after_stale_tokens = wait_for_current_parse_tokens(&mut client, "tokens-after-stale")?;
    ensure!(
        after_stale_tokens == current_tokens,
        "stale change altered current-generation semantic tokens"
    );
    let after_stale_names = document_symbol_names(&mut client, "symbols-after-stale")?;
    ensure!(
        after_stale_names.iter().any(|name| name.contains("current_symbol")),
        "stale change displaced the current symbol generation: {after_stale_names:?}"
    );
    ensure!(
        !after_stale_names.iter().any(|name| name.contains("stale_symbol")),
        "stale version 1 change became authoritative: {after_stale_names:?}"
    );
    let after_stale_fingerprints =
        diagnostic_fingerprints(&diagnostic_items(&mut client, "diagnostics-after-stale")?)?;
    ensure!(
        after_stale_fingerprints == current_fingerprints,
        "stale clean text changed current diagnostics: before={current_fingerprints:?} after={after_stale_fingerprints:?}"
    );

    finish(&mut client)
}

#[test]
fn close_after_settled_parse_removes_authority_and_reopen_starts_fresh() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    initialize(&mut client)?;

    did_open(&mut client, 7, "package BeforeClose;\nsub before_close { return 1; }\n")?;
    let _clean_tokens = wait_for_current_parse_tokens(&mut client, "tokens-clean")?;

    did_change(
        &mut client,
        8,
        "package BeforeClose;\nsub before_close { return 1; }\nsub broken {\n",
    )?;
    let _before_close_tokens = wait_for_current_parse_tokens(&mut client, "tokens-before-close")?;
    let before_close = document_symbol_names(&mut client, "symbols-before-close")?;
    ensure!(
        before_close.iter().any(|name| name.contains("before_close")),
        "pre-close symbol was not visible: {before_close:?}"
    );
    let before_close_diagnostics = diagnostic_items(&mut client, "diagnostics-before-close")?;
    let before_close_parser_fingerprints =
        parser_diagnostic_fingerprints(&before_close_diagnostics)?;
    ensure!(
        !before_close_parser_fingerprints.is_empty(),
        "broken pre-close source must produce parser diagnostics: {before_close_diagnostics:?}"
    );

    // Version 8 has deliberately settled before close. This exact-process
    // slice proves ownership removal and fresh reopen state, not the separate
    // worker-level ABA case where an old parse is held in flight across reopen.
    did_close(&mut client)?;
    let closed_id = json!("tokens-after-close");
    let closed = client.request(
        closed_id.clone(),
        "textDocument/semanticTokens/full",
        json!({
            "textDocument": { "uri": URI }
        }),
        timeout(),
    )?;
    ensure!(
        closed.get("jsonrpc") == Some(&json!("2.0")),
        "closed-document response omitted JSON-RPC 2.0: {closed}"
    );
    ensure!(closed.get("id") == Some(&closed_id), "closed-document ID mismatch: {closed}");
    ensure!(
        closed.pointer("/error/code") == Some(&json!(-32600)),
        "closed semantic-token request must return InvalidRequest: {closed}"
    );
    ensure!(
        closed.pointer("/error/message").and_then(Value::as_str)
            == Some(CLOSED_SEMANTIC_TOKENS_MESSAGE),
        "closed semantic-token error message drifted: {closed}"
    );

    did_open(&mut client, 1, "package Reopened;\nsub reopened_symbol { return 4; }\n")?;
    let _reopened_tokens = wait_for_current_parse_tokens(&mut client, "tokens-reopened")?;
    let reopened = document_symbol_names(&mut client, "symbols-reopened")?;
    ensure!(
        reopened.iter().any(|name| name.contains("reopened_symbol")),
        "reopened document did not publish fresh symbols: {reopened:?}"
    );
    ensure!(
        !reopened.iter().any(|name| name.contains("before_close")),
        "reopened document inherited a symbol from the closed generation: {reopened:?}"
    );
    let reopened_diagnostics = diagnostic_items(&mut client, "diagnostics-reopened")?;
    let reopened_fingerprints = diagnostic_fingerprints(&reopened_diagnostics)?;
    for stale_fingerprint in &before_close_parser_fingerprints {
        ensure!(
            !reopened_fingerprints.contains(stale_fingerprint),
            "reopened document retained a closed-generation parser diagnostic: {stale_fingerprint}"
        );
    }
    let reopened_parser_fingerprints = parser_diagnostic_fingerprints(&reopened_diagnostics)?;
    ensure!(
        reopened_parser_fingerprints.is_empty(),
        "clean reopened document produced parser diagnostics: {reopened_parser_fingerprints:?}"
    );

    finish(&mut client)
}
