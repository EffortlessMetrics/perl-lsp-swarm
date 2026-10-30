//! Public-process regression for variable tokens that share builtin names (#17776).
#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);
const URI: &str = "file:///hover-sigil-builtin-17776.pl";

fn session(source: &str) -> Result<RealProcessClient> {
    let mut client = RealProcessClient::spawn_exact()?;
    let initialized = client.request(
        json!("initialize"),
        "initialize",
        json!({
            "processId": null, "rootUri": null, "workspaceFolders": [],
            "capabilities": {
                "general": {"positionEncodings": ["utf-16"]},
                "textDocument": {"hover": {"contentFormat": ["markdown"]}}
            }
        }),
        TIMEOUT,
    )?;
    ensure!(initialized.get("error").is_none(), "initialize failed: {initialized}");
    ensure!(
        initialized.pointer("/result/capabilities/positionEncoding").and_then(Value::as_str)
            == Some("utf-16"),
        "UTF-16 negotiation failed: {initialized}"
    );
    client.notify("initialized", json!({}))?;
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": URI, "languageId": "perl", "version": 1, "text": source
        }}),
    )?;
    let barrier = client.request(
        json!("document-ready"),
        "textDocument/documentSymbol",
        json!({"textDocument": {"uri": URI}}),
        TIMEOUT,
    )?;
    ensure!(barrier.get("error").is_none(), "document barrier failed: {barrier}");
    Ok(client)
}

fn hover(client: &mut RealProcessClient, line: u32, character: u32) -> Result<Value> {
    let id = json!(format!("hover-{line}-{character}"));
    let response = client.request(
        id.clone(),
        "textDocument/hover",
        json!({
            "textDocument": {"uri": URI}, "position": {"line": line, "character": character}
        }),
        TIMEOUT,
    )?;
    ensure!(response.get("id") == Some(&id), "hover response ID mismatch: {response}");
    ensure!(response.get("error").is_none(), "hover request failed: {response}");
    response.get("result").cloned().context("hover response lacks terminal result")
}

fn finish(client: &mut RealProcessClient) -> Result<()> {
    let shutdown = client.request(json!("shutdown"), "shutdown", json!(null), TIMEOUT)?;
    ensure!(shutdown.get("error").is_none(), "shutdown failed: {shutdown}");
    client.notify("exit", json!(null))?;
    let status = client.wait_for_exit(TIMEOUT)?;
    ensure!(status.success(), "server exit failed: {status}; {}", client.stderr_tail());
    client.assert_transport_clean()
}

fn assert_card(
    source: &str,
    cursors: &[u32],
    line: u32,
    start: u32,
    end: u32,
    markdown: &str,
) -> Result<()> {
    let mut client = session(source)?;
    let expected = json!({
        "contents": {"kind": "markdown", "value": markdown},
        "range": {
            "start": {"line": line, "character": start},
            "end": {"line": line, "character": end}
        }
    });
    for &cursor in cursors {
        let actual = hover(&mut client, line, cursor)?;
        ensure!(
            actual == expected,
            "wrong hover at {line}:{cursor}: {actual}; expected {expected}"
        );
    }
    finish(&mut client)
}

// Literal LSP coordinates and cards are independent of the production tokenizer,
// builtin table, and UTF-16 conversion. Probe sigil, identifier, and final letter.
#[test]
fn unresolved_scalar_length_is_a_variable_token() -> Result<()> {
    assert_card("print $length;\n", &[6, 7, 12], 0, 6, 13, "**Perl**: `$length`")
}

#[test]
fn unresolved_array_length_is_a_variable_token() -> Result<()> {
    assert_card("print @length;\n", &[6, 7, 12], 0, 6, 13, "**Perl**: `@length`")
}

#[test]
fn unresolved_hash_length_is_a_variable_token() -> Result<()> {
    assert_card("print %length;\n", &[6, 7, 12], 0, 6, 13, "**Perl**: `%length`")
}

#[test]
fn unresolved_scalar_print_is_a_variable_token() -> Result<()> {
    assert_card("print $print;\n", &[6, 7, 11], 0, 6, 12, "**Perl**: `$print`")
}

#[test]
fn unresolved_variable_survives_incomplete_trailing_code() -> Result<()> {
    assert_card("print $length;\nmy $unfinished = (", &[6, 7, 12], 0, 6, 13, "**Perl**: `$length`")
}

#[test]
fn unresolved_variable_range_uses_utf16() -> Result<()> {
    assert_card("\"😀\"; print $length;\n", &[12, 13, 18], 0, 12, 19, "**Perl**: `$length`")
}

#[test]
fn actual_length_retains_builtin_documentation() -> Result<()> {
    // Top-level expression reaches the same unqualified fallback as the regression.
    assert_card(
        "length('abc');\n",
        &[0, 1, 5],
        0,
        0,
        6,
        "**Built-in Function**\n\n```\nlength EXPR\nlength\n```\n\nReturns the length in characters of the value of EXPR.",
    )
}

#[test]
fn declared_variable_retains_symbol_documentation() -> Result<()> {
    let mut client = session("my $length = 3;\nprint $length;\n")?;
    for cursor in [6, 7, 12] {
        let actual = hover(&mut client, 1, cursor)?;
        let markdown = actual
            .pointer("/contents/value")
            .and_then(Value::as_str)
            .context("declared variable lacks markdown")?;
        for retained in [
            "**Scalar Variable**",
            "`$length`",
            "**Declaration**: `my`",
            "**Declared at**: line 1",
            "**Scope**: file scope",
        ] {
            ensure!(markdown.contains(retained), "missing {retained} in {actual}");
        }
        ensure!(
            actual.get("range")
                == Some(&json!({
                    "start": {"line": 1, "character": 6},
                    "end": {"line": 1, "character": 13}
                })),
            "wrong declared variable range: {actual}"
        );
        ensure!(
            !markdown.contains("Built-in Function"),
            "declared variable became builtin: {actual}"
        );
    }
    finish(&mut client)
}

#[test]
fn special_variable_retains_documentation() -> Result<()> {
    let mut client = session("print %ENV;\n")?;
    for cursor in [6, 7, 9] {
        let actual = hover(&mut client, 0, cursor)?;
        let markdown = actual
            .pointer("/contents/value")
            .and_then(Value::as_str)
            .context("special variable lacks markdown")?;
        ensure!(
            markdown.starts_with("**`%ENV` — Environment Variables**"),
            "wrong special variable card: {actual}"
        );
        ensure!(
            markdown.contains("inherited by child processes"),
            "special variable docs lost: {actual}"
        );
        ensure!(
            actual.get("range")
                == Some(&json!({
                    "start": {"line": 0, "character": 6},
                    "end": {"line": 0, "character": 10}
                })),
            "wrong special variable range: {actual}"
        );
    }
    finish(&mut client)
}

#[test]
fn ordinary_sub_retains_signature_hover() -> Result<()> {
    let mut client = session(
        "use feature 'signatures';\nsub combine ($left, $right) { return $left; }\ncombine(1, 2);\n",
    )?;
    let actual = hover(&mut client, 2, 1)?;
    let markdown = actual
        .pointer("/contents/value")
        .and_then(Value::as_str)
        .context("subroutine lacks markdown")?;
    ensure!(markdown.contains("**Subroutine**"), "subroutine kind lost: {actual}");
    ensure!(markdown.contains("sub combine($left, $right)"), "signature lost: {actual}");
    ensure!(
        actual.get("range")
            == Some(&json!({
                "start": {"line": 2, "character": 0},
                "end": {"line": 2, "character": 7}
            })),
        "wrong subroutine range: {actual}"
    );
    finish(&mut client)
}

#[test]
fn lexical_non_code_does_not_gain_variable_or_builtin_hover() -> Result<()> {
    // Each source is a separate generation with no enclosing declaration fallback.
    for (source, line, cursor) in [
        ("# print $length;\n", 0, 9),
        ("'print $length';\n", 0, 8),
        ("q{print $length};\n", 0, 9),
        ("/length/;\n", 0, 2),
        ("=pod\nprint $length;\n=cut\n", 1, 7),
        ("print <<'END';\n$length\nEND\n", 1, 1),
        ("__DATA__\nprint $length;\n", 1, 7),
    ] {
        let mut client = session(source)?;
        let actual = hover(&mut client, line, cursor)?;
        ensure!(
            actual.is_null(),
            "non-code acquired hover at {line}:{cursor} in {source:?}: {actual}"
        );
        finish(&mut client)?;
    }
    Ok(())
}
