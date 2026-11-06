//! Exact-process qualification of copied executable launch under concurrency.
#![cfg(target_os = "linux")]
use anyhow::{Context, Result};
use perl_lsp_ux_tests::{FakeWorkspace, ScenarioConfig, UxClient, UxHarness, verify_binary_source};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Barrier};
use std::time::Duration;

#[test]
fn captured_current_server_refuses_persistent_busy_probe_then_launches_after_release() -> Result<()>
{
    let source = perl_lsp_ux_tests::resolve_binary()?;
    let dir = tempfile::tempdir()?;
    let captured = dir.path().join("captured-current-server");
    std::fs::copy(source, &captured)?;
    std::fs::set_permissions(&captured, std::fs::Permissions::from_mode(0o700))?;
    let binary = captured.to_str().context("captured path")?;
    verify_binary_source(binary)?;
    let workspace = FakeWorkspace::new()?;
    let writer = std::fs::OpenOptions::new().write(true).open(&captured)?;
    let mut command = std::process::Command::new(&captured);
    command
        .args(["--info", "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    match command.spawn() {
        Err(error) => assert_eq!(error.raw_os_error(), Some(26)),
        Ok(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("writer did not prevent exec");
        }
    }
    let error = verify_binary_source(binary)
        .err()
        .context("persistent writer must refuse identity probe")?;
    assert_eq!(
        error.downcast_ref::<std::io::Error>().and_then(std::io::Error::raw_os_error),
        Some(26)
    );
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        drop(writer);
    });
    let probe = verify_binary_source(binary);
    release.join().map_err(|_| anyhow::anyhow!("probe writer release thread failed"))?;
    probe.context("identity probe must recover after actual busy refusal")?;

    // Hold the same captured bytes writable again to exercise the separate
    // server-launch call site after successful identity verification.
    let writer = std::fs::OpenOptions::new().write(true).open(&captured)?;
    match command.spawn() {
        Err(error) => assert_eq!(error.raw_os_error(), Some(26)),
        Ok(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("second writer did not prevent exec");
        }
    }
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        drop(writer);
    });
    let client = UxClient::spawn(binary, &workspace, &ScenarioConfig::default());
    release.join().map_err(|_| anyhow::anyhow!("writer release thread failed"))?;
    let client = client?;
    client.request(
        "workspace/executeCommand",
        serde_json::json!({"command":"perl.workspaceTrustReport","arguments":[]}),
        Duration::from_secs(5),
    )?;
    client.shutdown_and_exit(Duration::from_secs(5))?;
    verify_binary_source(binary)?;
    Ok(())
}

#[test]
fn simultaneous_harness_captures_remain_current_and_respond() -> Result<()> {
    let start = Arc::new(Barrier::new(4));
    let workers: Vec<_> = (0..4).map(|_| {
        let start = Arc::clone(&start);
        std::thread::spawn(move || -> Result<()> {
            start.wait();
            let harness = UxHarness::new(ScenarioConfig::default())?;
            harness.open_file("main.pl", "my $answer = 42;\n")?;
            let uri = harness.workspace.uri("main.pl");
            harness.client.request("textDocument/hover", serde_json::json!({"textDocument":{"uri":uri},"position":{"line":0,"character":4}}), Duration::from_secs(5))?;
            harness.client.shutdown_and_exit(Duration::from_secs(5))?;
            Ok(())
        })
    }).collect();
    let mut failures = Vec::new();
    for worker in workers {
        match worker.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => failures.push(error.to_string()),
            Err(_) => failures.push("capture worker failed".to_owned()),
        }
    }
    anyhow::ensure!(failures.is_empty(), "concurrent capture failures: {failures:?}");
    Ok(())
}
