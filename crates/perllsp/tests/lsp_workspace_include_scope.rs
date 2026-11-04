//! Exact-process coverage for explicit dependency subtree discovery (#17774).

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use url::Url;

fn uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(|value| value.to_string())
        .map_err(|()| anyhow::anyhow!("invalid fixture file path: {}", path.display()))
}

fn request(client: &mut RealProcessClient, id: &str, method: &str, params: Value) -> Result<Value> {
    let response = client.request(json!(id), method, params, Duration::from_secs(10))?;
    ensure!(response.get("error").is_none(), "{method}: {response}");
    response.get("result").cloned().context("response omitted result")
}

#[test]
fn explicit_include_subtree_excludes_siblings_and_preserves_nested_workspace() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let root = client.workspace_path().join("project");
    fs::create_dir_all(&root)?;
    fs::write(
        root.join(".perl-lsp.toml"),
        "[perl]\ninclude_paths = [\"lib\", \"vendor/perl5\"]\n",
    )?;
    for (path, text) in [
        ("lib/App.pm", "package IncludeScope;\nsub lane_app { 1 }\n"),
        ("vendor/perl5/Dep.pm", "package IncludeScope;\nsub lane_dep { 1 }\n"),
        ("vendor/Other.pm", "package IncludeScope;\nsub lane_stray { 1 }\n"),
        ("vendor/tools/Hidden.pm", "package IncludeScope;\nsub lane_hidden { 1 }\n"),
        ("vendor/perl5/.cache/Noise.pm", "package IncludeScope;\nsub lane_cache { 1 }\n"),
        ("vendor/nested/lib/Nested.pm", "package IncludeScope;\nsub lane_nested { 1 }\n"),
    ] {
        let file = root.join(path);
        fs::create_dir_all(file.parent().context("fixture parent")?)?;
        fs::write(file, text)?;
    }
    let root_uri = uri(&root)?;
    let nested_uri = uri(&root.join("vendor/nested"))?;
    request(
        &mut client,
        "initialize",
        "initialize",
        json!({
            "processId": null,
            "rootUri": root_uri,
            "workspaceFolders": [
                {"uri": root_uri, "name": "project"},
                {"uri": nested_uri, "name": "nested"}
            ],
            "capabilities": {"general": {"positionEncodings": ["utf-16"]}}
        }),
    )?;
    client.notify("initialized", json!({}))?;

    // Bind absence assertions to the completed index, rather than an empty
    // startup answer. This public explanation reports the preceding query.
    let deadline = Instant::now() + Duration::from_secs(10);
    let symbols = loop {
        let symbols =
            request(&mut client, "symbols", "workspace/symbol", json!({"query": "lane_"}))?;
        let trace = request(
            &mut client,
            "readiness",
            "workspace/executeCommand",
            json!({
                "command": "perl.explainProviderDecision",
                "arguments": [{"provider": "workspace_symbols"}]
            }),
        )?;
        if trace.pointer("/request_receipt/source_backed_state").and_then(Value::as_str)
            == Some("ready_workspace_index")
        {
            break symbols;
        }
        ensure!(Instant::now() < deadline, "index did not become ready: {trace}; {symbols}");
    };
    let rows = symbols.as_array().context("workspace/symbol did not return an array")?;
    let mut actual: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "name": row["name"], "kind": row["kind"], "location": row["location"]
            })
        })
        .collect();
    actual.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    let expected = [
        ("lane_app", "lib/App.pm", 18),
        ("lane_dep", "vendor/perl5/Dep.pm", 18),
        ("lane_nested", "vendor/nested/lib/Nested.pm", 21),
    ]
    .into_iter()
    .map(|(name, path, end)| {
        Ok(json!({
            "name": name, "kind": 12,
            "location": {"uri": uri(&root.join(path))?, "range": {
                "start": {"line": 1, "character": 0},
                "end": {"line": 1, "character": end}
            }}
        }))
    })
    .collect::<Result<Vec<_>>>()?;
    let shutdown = request(&mut client, "shutdown", "shutdown", Value::Null)?;
    ensure!(shutdown.is_null(), "shutdown result: {shutdown}");
    client.notify("exit", Value::Null)?;
    ensure!(client.wait_for_exit(Duration::from_secs(10))?.success(), "server exit failed");
    client.assert_transport_clean()?;
    assert_eq!(actual, expected, "explicit include roots must not expose vendor siblings");
    Ok(())
}
