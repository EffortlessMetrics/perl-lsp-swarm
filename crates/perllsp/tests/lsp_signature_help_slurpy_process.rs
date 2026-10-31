//! Public-process proof for positional slurpy parameter selection (#8944).
#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);
const URI: &str = "file:///signature-help-slurpy.pl";

fn session() -> Result<RealProcessClient> {
    let mut client = RealProcessClient::spawn_exact()?;
    let response = client.request(
        json!("initialize"),
        "initialize",
        json!({"processId": null, "rootUri": null, "workspaceFolders": [],
            "capabilities": {"general": {"positionEncodings": ["utf-16"]}}}),
        TIMEOUT,
    )?;
    ensure!(response.get("error").is_none(), "initialize failed: {response}");
    client.notify("initialized", json!({}))?;
    Ok(client)
}

fn publish(client: &mut RealProcessClient, source: &str, version: i32) -> Result<()> {
    if version == 1 {
        client.notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": URI, "languageId": "perl", "version": version, "text": source
            }}),
        )?;
    } else {
        client.notify(
            "textDocument/didChange",
            json!({"textDocument": {"uri": URI, "version": version},
                "contentChanges": [{"text": source}]}),
        )?;
    }
    let barrier = client.request(
        json!(format!("symbols-{version}")),
        "textDocument/documentSymbol",
        json!({"textDocument": {"uri": URI}}),
        TIMEOUT,
    )?;
    ensure!(barrier.get("error").is_none(), "document barrier failed: {barrier}");
    Ok(())
}

fn assert_active(
    client: &mut RealProcessClient,
    source: &str,
    needle: &str,
    expected_labels: &[&str],
    expected_active: u64,
) -> Result<()> {
    let offset = source.rfind(needle).context("missing cursor marker")? + needle.len();
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let character = prefix.rsplit('\n').next().context("missing line")?.encode_utf16().count();
    let response = client.request(
        json!("signature"),
        "textDocument/signatureHelp",
        json!({"textDocument": {"uri": URI}, "position": {"line": line, "character": character}}),
        TIMEOUT,
    )?;
    ensure!(response.get("error").is_none(), "signature failed: {response}");
    let parameters = response
        .pointer("/result/signatures/0/parameters")
        .and_then(Value::as_array)
        .context("missing non-null signature parameters")?;
    let labels = parameters
        .iter()
        .map(|parameter| parameter.get("label").and_then(Value::as_str).context("missing label"))
        .collect::<Result<Vec<_>>>()?;
    ensure!(labels == expected_labels, "wrong signature parameters at {needle:?}: {response}");
    ensure!(
        response.pointer("/result/activeParameter").and_then(Value::as_u64)
            == Some(expected_active),
        "wrong active parameter at {needle:?}, expected {expected_active}: {response}"
    );
    Ok(())
}

fn finish(client: &mut RealProcessClient) -> Result<()> {
    let response = client.request(json!("shutdown"), "shutdown", json!(null), TIMEOUT)?;
    ensure!(response.get("error").is_none(), "shutdown failed: {response}");
    client.notify("exit", json!(null))?;
    ensure!(client.wait_for_exit(TIMEOUT)?.success(), "server exit failed");
    client.assert_transport_clean()
}

#[test]
fn fixed_positional_parameters_and_nested_call_remain_selected() -> Result<()> {
    let mut client = session()?;
    let source = "use v5.36;\nsub fixed ($first, $second, $third) {}\nsub outer ($head, @rest) {}\nmy $banner = '😀'; outer(1, fixed(2, 3, 4), 5);\n";
    publish(&mut client, source, 1)?;
    for (needle, expected) in [("fixed(", 0), ("fixed(2, ", 1), ("fixed(2, 3, ", 2)] {
        assert_active(&mut client, source, needle, &["$first", "$second", "$third"], expected)?;
    }
    assert_active(&mut client, source, "outer(", &["$head", "@rest"], 0)?;
    assert_active(&mut client, source, "outer(1, ", &["$head", "@rest"], 1)?;
    finish(&mut client)
}

#[test]
fn repeated_positional_arguments_select_declared_slurpy_tail() -> Result<()> {
    let mut client = session()?;
    for (index, (declaration, labels, active)) in [
        ("$head, @rest", vec!["$head", "@rest"], 1),
        ("@rest", vec!["@rest"], 0),
        ("$head, %rest", vec!["$head", "%rest"], 1),
        ("$head, $optional = 9, @rest", vec!["$head", "$optional = 9", "@rest"], 2),
    ]
    .into_iter()
    .enumerate()
    {
        let source = format!(
            "use v5.36;\nsub collect ({declaration}) {{}}\nmy $banner = '😀'; collect(1, 2, 3, 4, 5);\n"
        );
        publish(&mut client, &source, (index + 1) as i32)?;
        assert_active(&mut client, &source, "collect(1, 2, 3, ", &labels, active)?;
        assert_active(&mut client, &source, "collect(1, 2, 3, 4, ", &labels, active)?;
        if index == 3 {
            assert_active(&mut client, &source, "collect(1, ", &labels, 1)?;
        }
    }
    let source = "use v5.36;\nsub collect ($head, @rest) {}\nsub fixed ($a, $b) {}\ncollect(1, fixed(2, 3), 4);\n";
    publish(&mut client, source, 5)?;
    assert_active(&mut client, source, "collect(1, fixed(2, 3), ", &["$head", "@rest"], 1)?;
    finish(&mut client)
}

#[test]
fn signatures_without_a_positional_slurpy_keep_existing_selection() -> Result<()> {
    let mut client = session()?;
    // These compatibility controls preserve existing ordinals, including those
    // outside the displayed parameter list. Fixing their binding is separate
    // #8944 work; a display label or a parameter count is not slurpy authority.
    for (index, (declaration, labels)) in [
        ("sub collect ($first, $second) {}", vec!["$first", "$second"]),
        ("sub collect { my ($head, @rest) = @_; }", vec!["$head", "@rest"]),
        ("sub collect (:$named, %rest) {}", vec![":$named", "%rest"]),
    ]
    .into_iter()
    .enumerate()
    {
        let source = format!("use v5.44;\n{declaration}\ncollect(1, 2, 3, 4);\n");
        publish(&mut client, &source, (index + 1) as i32)?;
        assert_active(&mut client, &source, "collect(1, 2, ", &labels, 2)?;
    }
    finish(&mut client)
}

#[test]
fn slurpy_selection_uses_current_declaration_after_edit() -> Result<()> {
    let mut client = session()?;
    for (version, declaration, labels, active) in [
        (1, "$head, @rest", vec!["$head", "@rest"], 1),
        (2, "$head, $second, $third", vec!["$head", "$second", "$third"], 2),
        (3, "$head, @rest", vec!["$head", "@rest"], 1),
    ] {
        let source = format!(
            "use v5.36;\nsub collect ({declaration}) {{}}\nmy $banner = '😀'; collect(1, 2, 3);\n"
        );
        publish(&mut client, &source, version)?;
        assert_active(&mut client, &source, "collect(1, 2, ", &labels, active)?;
    }
    finish(&mut client)
}

#[test]
fn method_slurpy_index_excludes_the_invocant() -> Result<()> {
    let mut client = session()?;
    for (index, declaration) in ["$head, @rest", "$receiver: $head, @rest"].into_iter().enumerate()
    {
        let source = format!(
            "use v5.40;\nuse feature 'class';\nclass Gadget {{ method collect ({declaration}) {{}} }}\nmy $object = Gadget->new;\n$object->collect(1, 2, 3);\n"
        );
        publish(&mut client, &source, (index + 1) as i32)?;
        assert_active(&mut client, &source, "collect(1, 2, ", &["$head", "@rest"], 1)?;
    }
    finish(&mut client)
}
