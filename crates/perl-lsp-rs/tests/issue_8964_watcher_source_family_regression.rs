//! Regression guards for #8964 under the superseding catch-all lifecycle contract
//! (#13308/#14074/#14186): handler-side Perl admission must accept canonical
//! source extensions such as `.cgi`/`.fcgi`, and watcher registration must stay
//! aligned with workspace file-operation filters.

mod support;

use perl_lsp_rs_core::runtime::tuning::RuntimeTuning;
use serde_json::{Value, json};
use std::time::Duration;
use support::lsp_harness::{LspHarness, WaitForSymbolMode};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn cgi_and_fcgi_watched_file_create_reaches_workspace_index() -> TestResult {
    let (mut harness, workspace) = LspHarness::with_workspace(&[])?;
    workspace
        .write("app.cgi", "#!/usr/bin/env perl\npackage CgiApp;\nuse strict;\nsub hello { 1 }\n")?;
    workspace.write("run.fcgi", "#!/usr/bin/env perl\npackage FcgiRun;\nuse strict;\n1;\n")?;

    let cgi_uri = workspace.uri("app.cgi");
    let fcgi_uri = workspace.uri("run.fcgi");
    harness.notify(
        "workspace/didChangeWatchedFiles",
        json!({
            "changes": [
                { "uri": cgi_uri, "type": 1 },
                { "uri": fcgi_uri, "type": 1 },
            ]
        }),
    );

    harness.wait_for_symbol_with_mode(
        "CgiApp",
        Some(cgi_uri.as_str()),
        Duration::from_secs(30),
        WaitForSymbolMode::Default,
    )?;
    harness.wait_for_symbol_with_mode(
        "FcgiRun",
        Some(fcgi_uri.as_str()),
        Duration::from_secs(30),
        WaitForSymbolMode::Default,
    )?;
    Ok(())
}

#[test]
fn watcher_registration_matches_file_operation_catch_all_globs() -> TestResult {
    let mut harness = LspHarness::new_with_tuning(RuntimeTuning::normal_defaults());
    let init = harness.initialize(Some(watcher_and_file_operation_caps()))?;

    let file_operations = init
        .pointer("/capabilities/workspace/fileOperations")
        .and_then(Value::as_object)
        .ok_or("initialize must advertise workspace.fileOperations")?;
    for (name, op) in file_operations {
        let filters = op
            .get("filters")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{name} must include filters"))?;
        assert_eq!(filters.len(), 1, "{name} must advertise exactly one catch-all filter: {op:?}");
        assert_eq!(
            filters[0].pointer("/pattern/glob").and_then(Value::as_str),
            Some("**/*"),
            "{name} must use the shared catch-all glob"
        );
    }

    let requests = harness.drain_server_requests(250);
    let registration = registration_for_method(&requests, "workspace/didChangeWatchedFiles")
        .ok_or("dynamic watcher registration must be sent")?;
    let watchers = registration
        .pointer("/registerOptions/watchers")
        .and_then(Value::as_array)
        .ok_or("watcher registration must include watchers")?;
    assert_eq!(
        watchers.len(),
        1,
        "watcher registration must advertise exactly one catch-all watcher"
    );
    assert_eq!(
        watchers[0]["globPattern"],
        json!("**/*"),
        "watcher glob must match file-operation authority"
    );
    Ok(())
}

fn watcher_and_file_operation_caps() -> Value {
    json!({
        "workspace": {
            "didChangeWatchedFiles": {
                "dynamicRegistration": true
            },
            "fileOperations": {
                "didCreate": true,
                "didDelete": true,
                "didRename": true,
                "willCreate": true,
                "willDelete": true,
                "willRename": true
            }
        },
        "textDocument": {}
    })
}

fn registration_for_method<'a>(requests: &'a [Value], method: &str) -> Option<&'a Value> {
    requests.iter().find_map(|request| {
        if request.get("method").and_then(Value::as_str) != Some("client/registerCapability") {
            return None;
        }
        request.pointer("/params/registrations").and_then(Value::as_array).and_then(
            |registrations| {
                registrations
                    .iter()
                    .find(|entry| entry.get("method").and_then(Value::as_str) == Some(method))
            },
        )
    })
}
