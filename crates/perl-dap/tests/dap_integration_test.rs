#![expect(
    clippy::print_stderr,
    reason = "Integration-test diagnostic and skip output; tracing is not the harness logger."
)]
use perl_dap::{DapMessage, DebugAdapter};
use perl_lsp_rs_core::config::PerlOracleEnv;
use serde_json::json;
use std::fs::write;
use std::sync::mpsc::sync_channel;
use std::time::{Duration, Instant};
use tempfile::tempdir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn test_dap_basic_flow() -> TestResult {
    // Skip if perl is not available
    if PerlOracleEnv::for_dap_test_fixture().is_none() {
        eprintln!("Skipping DAP basic flow test - perl not available");
        return Ok(());
    }

    let dir = tempdir()?;
    let script_path = dir.path().join("sample.pl");
    write(
        &script_path,
        r#"use strict;
use warnings;

my $x = 1;
$x++;
print "x=$x\n";
"#,
    )?;

    let mut adapter = DebugAdapter::new();
    crate::install_unbounded_test_authority(&adapter);
    let (tx, rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    // Initialize
    let init_response = adapter.handle_request(1, "initialize", None);
    match init_response {
        DapMessage::Response { success, .. } => assert!(success, "Initialize should succeed"),
        _ => return Err("Expected initialize response".into()),
    }

    // Require the lifecycle event, while tolerating unrelated protocol events.
    let initialized_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let remaining = initialized_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Timed out waiting for initialized event".into());
        }
        match rx.recv_timeout(remaining) {
            Ok(DapMessage::Event { event, .. }) if event == "initialized" => break,
            Ok(_) => continue,
            Err(_) => return Err("Timed out waiting for initialized event".into()),
        }
    }

    // Launch
    let launch_args = json!({
        "program": script_path.to_str().ok_or("Failed to convert path to string")?,
        "args": [],
        "stopOnEntry": true
    });
    let launch_response = adapter.handle_request(2, "launch", Some(launch_args));
    match launch_response {
        DapMessage::Response { success, message, .. } => {
            assert!(success, "Launch should succeed, got: {message:?}");

            let stopped_deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let remaining = stopped_deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err("Timed out waiting for stopped event".into());
                }
                match rx.recv_timeout(remaining) {
                    Ok(DapMessage::Event { event, .. }) if event == "stopped" => break,
                    Ok(_) => continue,
                    Err(_) => return Err("Timed out waiting for stopped event".into()),
                }
            }
        }
        _ => return Err("Expected launch response".into()),
    }

    // Disconnect
    let disconnect_response = adapter.handle_request(3, "disconnect", None);
    match disconnect_response {
        DapMessage::Response { success, .. } => assert!(success, "Disconnect should succeed"),
        _ => return Err("Expected disconnect response".into()),
    }

    eprintln!("DAP basic flow test completed");
    Ok(())
}

/// Install an explicitly unbounded startup authority (#8656).
///
/// These tests exercise debugging workflows, not the launch-authority
/// contract. Without an installed authority every launch is refused, so each
/// adapter opts into unbounded mode with a visible test acknowledgement.
fn install_unbounded_test_authority(adapter: &perl_dap::DebugAdapter) {
    use perl_dap::{
        LaunchAuthority, LaunchAuthoritySource, LaunchAuthorityStartup, UnboundedAcknowledgement,
    };
    let authority = LaunchAuthority::resolve(&LaunchAuthorityStartup {
        trusted_roots: Vec::new(),
        allow_unbounded: Some(UnboundedAcknowledgement::new(
            LaunchAuthoritySource::CommandLine,
            "test: unbounded session",
        )),
    })
    .expect("test authority resolution");
    adapter.set_launch_authority(authority);
}
