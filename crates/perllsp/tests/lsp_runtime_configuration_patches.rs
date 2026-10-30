//! Real consumer regression for partial workspace configuration notifications.
//! Uses Cargo's public binary and actual module files; no direct config mutation.
#![deny(clippy::map_err_ignore)]

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use url::Url;

const SOURCE: &str = "use PatchOnly17753;\nuse EnvironmentOnly17753;\nuse LowerLayerOnly17753;\nuse NeverConfigured17753;\n";

fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(String::from)
        .map_err(|()| anyhow::anyhow!("invalid fixture path"))
}

fn request(client: &mut RealProcessClient, id: &str, method: &str, params: Value) -> Result<Value> {
    let response = client.request(json!(id), method, params, Duration::from_secs(10))?;
    ensure!(response["id"] == json!(id), "wrong response identity: {response}");
    ensure!(response.get("error").is_none(), "{method} failed: {response}");
    response.get("result").cloned().context("missing result")
}

fn initialize(client: &mut RealProcessClient, root: &str, settings: Value) -> Result<()> {
    let result = request(
        client,
        "initialize",
        "initialize",
        json!({
            "processId": null,
            "rootUri": root,
            "workspaceFolders": [{"uri": root, "name": "patch-fixture"}],
            "capabilities": {"textDocument": {"diagnostic": {}}},
            "initializationOptions": {"perl": settings}
        }),
    )?;
    ensure!(
        result.pointer("/capabilities/diagnosticProvider").is_some(),
        "pull diagnostics unavailable: {result}"
    );
    client.notify("initialized", json!({}))
}

fn open(client: &mut RealProcessClient, uri: &str) -> Result<()> {
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "perl", "version": 1, "text": SOURCE
        }}),
    )
}

fn patch(client: &mut RealProcessClient, settings: Value) -> Result<()> {
    client.notify("workspace/didChangeConfiguration", json!({"settings": {"perl": settings}}))
}

fn missing_module_lines(client: &mut RealProcessClient, uri: &str, id: &str) -> Result<Vec<u64>> {
    let report =
        request(client, id, "textDocument/diagnostic", json!({"textDocument": {"uri": uri}}))?;
    ensure!(report["kind"] == "full", "expected full report: {report}");
    let items = report["items"].as_array().context("missing diagnostic items")?;
    let mut lines: Vec<u64> = items
        .iter()
        .filter(|item| item["code"] == "PL701")
        .map(|item| {
            item.pointer("/range/start/line")
                .and_then(Value::as_u64)
                .context("PL701 omitted source line")
        })
        .collect::<Result<_>>()?;
    lines.sort();
    ensure!(
        items.iter().any(|item| item["code"] == "PL701"
            && item["message"]
                .as_str()
                .is_some_and(|message| message.contains("NeverConfigured17753"))),
        "missing-module negative control absent: {report}"
    );
    // Search-path prose legitimately changes with resolver options. The consumer
    // oracle is which fixed source import remains unresolved.
    Ok(lines)
}

fn definition(client: &mut RealProcessClient, uri: &str, id: &str) -> Result<Value> {
    request(
        client,
        id,
        "textDocument/definition",
        json!({
            "textDocument": {"uri": uri}, "position": {"line": 0, "character": 8}
        }),
    )
}

fn finish(client: &mut RealProcessClient) -> Result<()> {
    ensure!(request(client, "shutdown", "shutdown", Value::Null)?.is_null());
    client.notify("exit", Value::Null)?;
    ensure!(
        client.wait_for_exit(Duration::from_secs(10))?.success(),
        "unclean shutdown: {}",
        client.stderr_tail()
    );
    client.assert_transport_clean()
}

#[test]
fn partial_workspace_patch_retains_module_roots_and_matches_fresh_server() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let environment_library = tempfile::tempdir()?;
    std::fs::write(
        environment_library.path().join("EnvironmentOnly17753.pm"),
        "package EnvironmentOnly17753;\n1;\n",
    )?;
    let perl5lib = environment_library.path().to_str().context("non-UTF8 fixture path")?;
    std::fs::create_dir(fixture.path().join("site_lib"))?;
    let module_path = fixture.path().join("site_lib/PatchOnly17753.pm");
    std::fs::write(&module_path, "package PatchOnly17753;\n1;\n")?;
    std::fs::write(fixture.path().join("app.pl"), SOURCE)?;
    std::fs::create_dir(fixture.path().join("toml_lib"))?;
    std::fs::write(
        fixture.path().join("toml_lib/LowerLayerOnly17753.pm"),
        "package LowerLayerOnly17753;\n1;\n",
    )?;
    // This real lower-layer module must stop resolving after client replacement;
    // unioning project roots with every client root would be a false-green fix.
    std::fs::write(
        fixture.path().join(".perl-lsp.toml"),
        "[perl]\ninclude_paths = [\"toml_lib\"]\n",
    )?;
    let root = file_uri(fixture.path())?;
    let uri = file_uri(&fixture.path().join("app.pl"))?;
    let mut live = RealProcessClient::spawn_exact_with_env(&[("PERL5LIB", perl5lib)])?;
    initialize(
        &mut live,
        &root,
        json!({"workspace": {"includePaths": ["init_lib"], "useSystemInc": false, "usePerl5lib": false}}),
    )?;
    open(&mut live, &uri)?;
    let baseline = missing_module_lines(&mut live, &uri, "baseline")?;
    ensure!(
        baseline == vec![0, 1, 3],
        "fixture unexpectedly resolves before configuring root: {baseline:?}"
    );

    patch(&mut live, json!({"workspace": {"includePaths": ["site_lib"]}}))?;
    let configured = missing_module_lines(&mut live, &uri, "configured")?;
    ensure!(configured == vec![1, 2, 3], "includePaths did not resolve fixture: {configured:?}");
    let configured_definition = definition(&mut live, &uri, "configured-definition")?;
    ensure!(
        configured_definition.to_string().contains(&file_uri(&module_path)?),
        "definition did not reach configured module: {configured_definition}"
    );

    patch(&mut live, json!({"workspace": {"usePerl5lib": true}}))?;
    let after_patch = missing_module_lines(&mut live, &uri, "after-disjoint-patch")?;
    let expected_final = vec![2, 3];
    ensure!(
        after_patch == expected_final,
        "partial workspace patch lost earlier includePaths: before={configured:?}, after={after_patch:?}"
    );
    ensure!(
        definition(&mut live, &uri, "after-patch-definition")? == configured_definition,
        "partial patch changed module navigation"
    );
    // A third patch must also retain the accumulated root override.
    patch(&mut live, json!({"workspace": {"resolutionTimeout": 200}}))?;
    ensure!(missing_module_lines(&mut live, &uri, "after-third-patch")? == expected_final);

    let mut fresh = RealProcessClient::spawn_exact_with_env(&[("PERL5LIB", perl5lib)])?;
    initialize(
        &mut fresh,
        &root,
        json!({"workspace": {"includePaths": ["init_lib"], "useSystemInc": false, "usePerl5lib": false}}),
    )?;
    // Replay the final client layer in one notification: initializationOptions
    // is below TOML in the precedence stack, unlike accepted client settings.
    patch(
        &mut fresh,
        json!({"workspace": {"includePaths": ["site_lib"], "usePerl5lib": true, "resolutionTimeout": 200}}),
    )?;
    open(&mut fresh, &uri)?;
    ensure!(
        missing_module_lines(&mut fresh, &uri, "fresh-diagnostics")? == expected_final,
        "reconfigured diagnostics differ from fresh initialization"
    );
    ensure!(
        definition(&mut fresh, &uri, "fresh-definition")? == configured_definition,
        "reconfigured navigation differs from fresh initialization"
    );
    finish(&mut fresh)?;

    // Explicit replacement must still take effect; retaining every old root is wrong.
    patch(&mut live, json!({"workspace": {"includePaths": ["removed_lib"]}}))?;
    ensure!(
        missing_module_lines(&mut live, &uri, "removed-root")? == vec![0, 2, 3],
        "explicit root replacement was ignored"
    );
    let removed_definition = definition(&mut live, &uri, "removed-definition")?;
    ensure!(
        removed_definition.is_null() || removed_definition.as_array().is_some_and(Vec::is_empty),
        "removed root still grants module navigation: {removed_definition}"
    );
    patch(&mut live, json!({"workspace": {"includePaths": ["site_lib"]}}))?;
    ensure!(
        missing_module_lines(&mut live, &uri, "recovered")? == expected_final,
        "restoring the include path did not recover"
    );
    ensure!(definition(&mut live, &uri, "recovered-definition")? == configured_definition);
    finish(&mut live)
}
