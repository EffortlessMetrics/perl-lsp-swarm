//! End-to-end DAP smoke test using the native debug adapter and real `perl -d`.

#![expect(
    clippy::print_stderr,
    reason = "Integration-test diagnostic and skip output; tracing is not the harness logger."
)]
use perl_dap::{DapMessage, DebugAdapter};
use serde_json::{Value, json};
use std::fs::write;
use std::sync::mpsc::{Receiver, sync_channel};
use std::time::Duration;
use tempfile::tempdir;

mod common;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn perl_available() -> bool {
    common::debuggee_perl_or_typed_skip("dap_smoke_e2e").is_some()
}

fn smoke_timeout() -> Duration {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some()
        || std::env::var_os("CARGO_LLVM_COV").is_some()
    {
        Duration::from_mins(1)
    } else {
        Duration::from_secs(10)
    }
}

fn wait_for_event(
    rx: &Receiver<DapMessage>,
    event_name: &str,
    timeout: Duration,
) -> Result<DapMessage, String> {
    common::wait_for_event(rx, event_name, timeout)
}

fn response_success(response: DapMessage, command: &str) -> Result<Option<Value>, String> {
    match response {
        DapMessage::Response { success, command: actual, body, message, .. } => {
            if actual != command {
                return Err(format!("expected `{command}` response, got `{actual}`"));
            }
            if !success {
                return Err(format!(
                    "command `{command}` failed: {}",
                    message.unwrap_or_else(|| "<no message>".to_string())
                ));
            }
            Ok(body)
        }
        _ => Err(format!("expected response message for `{command}`")),
    }
}

fn stopped_reason(message: &DapMessage) -> Option<String> {
    match message {
        DapMessage::Event { event, body, .. } if event == "stopped" => body
            .as_ref()
            .and_then(|payload| payload.get("reason"))
            .and_then(Value::as_str)
            .map(ToString::to_string),
        _ => None,
    }
}

#[test]
fn dap_smoke_e2e() -> TestResult {
    if !perl_available() {
        eprintln!("Skipping dap_smoke_e2e - perl executable is not available");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script_path = workspace.path().join("smoke.pl");
    write(
        &script_path,
        r#"use strict;
use warnings;
my $x = 1;
$x++;
print "$x\n";
"#,
    )?;

    let script_path_str = script_path
        .to_str()
        .ok_or("script path could not be converted to UTF-8 string")?
        .to_string();
    let timeout = smoke_timeout();

    let mut adapter = DebugAdapter::new();
    let (tx, rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    let init_body = response_success(adapter.handle_request(1, "initialize", None), "initialize")?;
    let capabilities = init_body.ok_or("initialize response missing capability body")?;
    assert!(
        capabilities
            .get("supportsConfigurationDoneRequest")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    );
    // #9089: the routed inlineValues extension stays unadvertised until a
    // versioned negotiation contract is proven. `unwrap_or(true)` so a missing
    // key fails this assertion instead of passing vacuously.
    assert!(
        !capabilities.get("supportsInlineValues").and_then(|v| v.as_bool()).unwrap_or(true),
        "supportsInlineValues must be false until #9089's negotiation gate passes"
    );
    let _initialized = wait_for_event(&rx, "initialized", timeout)?;

    let perl_path = common::resolve_launch_perl_path()
        .map_err(|reason| format!("could not resolve the launch interpreter: {reason}"))?
        .ok_or("the availability gate resolved no pipe-capable launch interpreter")?;

    response_success(
        adapter.handle_request(
            2,
            "launch",
            Some(json!({
                "program": script_path_str,
                "args": [],
                "stopOnEntry": true,
                "perlPath": perl_path.to_string_lossy(),
                "env": {
                    "PERL_PERTURB_KEYS": "0",
                    "PERL_HASH_SEED": "0",
                    "LC_ALL": "C",
                    "TZ": "UTC"
                }
            })),
        ),
        "launch",
    )?;
    let entry_stop = wait_for_event(&rx, "stopped", timeout)?;
    let entry_reason = stopped_reason(&entry_stop);
    assert!(
        matches!(entry_reason.as_deref(), Some("entry" | "step")),
        "expected initial stopped reason `entry` or `step`, got: {entry_stop:#?}"
    );

    let request_seq = 3;

    response_success(
        adapter.handle_request(request_seq, "disconnect", Some(json!({}))),
        "disconnect",
    )?;
    let _terminated = wait_for_event(&rx, "terminated", timeout)?;

    Ok(())
}
