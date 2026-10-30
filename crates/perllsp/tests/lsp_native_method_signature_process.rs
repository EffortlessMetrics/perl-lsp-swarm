//! Exact public-process proof for native method signature projection (#17803).
#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);
const URI: &str = "file:///native-method-signature-17803.pl";

fn position(source: &str, needle: &str) -> Result<Value> {
    let offset = source.rfind(needle).context("missing call cursor marker")? + needle.len();
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix.rsplit('\n').next().context("missing cursor line")?.encode_utf16().count();
    Ok(json!({"line": line, "character": column}))
}

fn session() -> Result<RealProcessClient> {
    let mut client = RealProcessClient::spawn_exact()?;
    let response = client.request(
        json!("initialize"),
        "initialize",
        json!({
            "processId": null, "rootUri": null, "workspaceFolders": [],
            "capabilities": {"general": {"positionEncodings": ["utf-16"]}}
        }),
        TIMEOUT,
    )?;
    ensure!(response.get("error").is_none(), "initialize failed: {response}");
    let encoding = response
        .pointer("/result/capabilities/positionEncoding")
        .and_then(Value::as_str)
        .unwrap_or("utf-16");
    ensure!(encoding == "utf-16", "unexpected negotiated encoding: {response}");
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
            json!({
                "textDocument": {"uri": URI, "version": version},
                "contentChanges": [{"text": source}]
            }),
        )?;
    }
    // Ordered document request before querying the signature of this generation.
    let barrier = client.request(
        json!(format!("symbols-{version}")),
        "textDocument/documentSymbol",
        json!({"textDocument": {"uri": URI}}),
        TIMEOUT,
    )?;
    ensure!(barrier.get("error").is_none(), "document barrier failed: {barrier}");
    Ok(())
}

fn signature(
    client: &mut RealProcessClient,
    source: &str,
    needle: &str,
    id: &str,
) -> Result<Value> {
    let response = client.request(
        json!(id),
        "textDocument/signatureHelp",
        json!({
            "textDocument": {"uri": URI}, "position": position(source, needle)?
        }),
        TIMEOUT,
    )?;
    ensure!(response.get("id") == Some(&json!(id)), "response ID mismatch: {response}");
    ensure!(response.get("error").is_none(), "signature request failed: {response}");
    response.get("result").cloned().context("missing terminal result")
}

fn finish(client: &mut RealProcessClient) -> Result<()> {
    let shutdown = client.request(json!("shutdown"), "shutdown", json!(null), TIMEOUT)?;
    ensure!(shutdown.get("error").is_none(), "shutdown failed: {shutdown}");
    client.notify("exit", json!(null))?;
    let status = client.wait_for_exit(TIMEOUT)?;
    ensure!(status.success(), "server exit failed: {status}; {}", client.stderr_tail());
    client.assert_transport_clean()
}

fn assert_signature(value: &Value, label: &str, parameters: &[&str], active: u64) -> Result<()> {
    let actual = value
        .pointer("/signatures/0/parameters")
        .and_then(Value::as_array)
        .context("signature has no parameter array")?;
    let labels: Vec<&str> = actual
        .iter()
        .map(|p| p.get("label").and_then(Value::as_str).context("parameter has no string label"))
        .collect::<Result<_>>()?;
    ensure!(labels == parameters, "declared parameter loss/wrong invocant: {value}");
    ensure!(
        value.pointer("/signatures/0/label").and_then(Value::as_str) == Some(label),
        "wrong declaration label: {value}"
    );
    ensure!(
        value.get("activeParameter").and_then(Value::as_u64) == Some(active),
        "wrong active parameter: {value}"
    );
    Ok(())
}

#[test]
fn native_method_parameters_over_utf16_process_and_edit() -> Result<()> {
    let mut client = session()?;
    for (version, prefix) in [(1, ""), (2, "café"), (3, "😀😀😀😀"), (4, "e\u{301}")] {
        let source = format!(
            "use v5.40;\nuse feature 'class';\nclass Gadget {{\n    method combine ($left, $right = 2, @rest) {{ return $left; }}\n}}\nmy $object = Gadget->new;\nmy $banner = '{prefix}'; $object->combine(1, 2);\n"
        );
        let source = if version == 3 { source.replace('\n', "\r\n") } else { source };
        publish(&mut client, &source, version)?;
        let value =
            signature(&mut client, &source, "$object->combine(1, ", &format!("method-{version}"))?;
        assert_signature(
            &value,
            "method combine($left, $right = 2, @rest)",
            &["$left", "$right = 2", "@rest"],
            1,
        )?;
    }
    let changed = "use v5.40;\nuse feature 'class';\nclass Gadget {\n    method combine ($first, $last = 9) { return $first; }\n}\nmy $object = Gadget->new;\nmy $banner = '🧪'; $object->combine(1, ";
    publish(&mut client, changed, 5)?;
    let value = signature(&mut client, changed, "$object->combine(1, ", "method-incomplete-v5")?;
    assert_signature(&value, "method combine($first, $last = 9)", &["$first", "$last = 9"], 1)?;
    finish(&mut client)
}

#[test]
fn ordinary_sub_signature_retained_over_utf16_process() -> Result<()> {
    let mut client = session()?;
    let source = "use feature 'signatures';\nsub combine ($left, $right = 2, @rest) { return $left; }\nmy $banner = '😀'; combine(1, 2);\n";
    publish(&mut client, source, 1)?;
    let value = signature(&mut client, source, "combine(1, ", "ordinary-sub")?;
    assert_signature(
        &value,
        "sub combine($left, $right = 2, @rest)",
        &["$left", "$right = 2", "@rest"],
        1,
    )?;
    finish(&mut client)
}

#[test]
fn method_invocant_separator_and_self_name_over_utf16_process() -> Result<()> {
    let mut client = session()?;
    // Colon invocants are an accepted parser dialect, not core perlclass syntax.
    // Names and colons inside parameter defaults/comments must not imply a receiver.
    for (index, (declaration, label, parameters)) in [
        ("$self: $arg, $tail = 2", "method run($arg, $tail = 2)", vec!["$arg", "$tail = 2"]),
        (
            "$receiver # 😀 : comment\n : $arg, $tail = 2",
            "method run($arg, $tail = 2)",
            vec!["$arg", "$tail = 2"],
        ),
        ("$self, $arg", "method run($self, $arg)", vec!["$self", "$arg"]),
        ("$self # : comment\n , $arg", "method run($self, $arg)", vec!["$self", "$arg"]),
        ("$value :param, $arg", "method run($value, $arg)", vec!["$value", "$arg"]),
        (
            "$value = 'a:b', $arg = 2",
            "method run($value = 'a:b', $arg = 2)",
            vec!["$value = 'a:b'", "$arg = 2"],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let source = format!(
            "use v5.40;\nuse feature 'class';\nclass Gadget {{ method run ({declaration}) {{ }} }}\nmy $object = Gadget->new;\nmy $banner = '😀'; $object->run(11, 22);\n"
        );
        publish(&mut client, &source, (index + 1) as i32)?;
        for (needle, active) in [("$object->run(", 0), ("$object->run(11, ", 1)] {
            let value =
                signature(&mut client, &source, needle, &format!("invocant-{index}-{active}"))?;
            assert_signature(&value, label, &parameters, active)?;
        }
    }
    let source = "use v5.40;\nuse feature 'class';\nclass Gadget { method run ($receiver:) { } }\nmy $object = Gadget->new;\nmy $banner = '😀'; $object->run(";
    publish(&mut client, source, 7)?;
    let value = signature(&mut client, source, "$object->run(", "invocant-only")?;
    assert_signature(&value, "method run", &[], 0)?;
    // A real framework spelling of the dialect, without a native class declaration.
    let source = "use Function::Parameters;\npackage Gadget;\nmethod run ($this: $arg, $tail = 2) { }\nmy $object = bless {}, 'Gadget';\nmy $banner = '😀'; $object->run(11, 22);\n";
    publish(&mut client, source, 8)?;
    for (needle, active) in [("$object->run(", 0), ("$object->run(11, ", 1)] {
        let value = signature(&mut client, source, needle, &format!("framework-{active}"))?;
        assert_signature(&value, "method run($arg, $tail = 2)", &["$arg", "$tail = 2"], active)?;
    }
    finish(&mut client)
}
