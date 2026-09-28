#![cfg(feature = "cold-index-proof")]

//! One-process stdio proof for the cold references window in #16650.

use anyhow::{Context, Result, anyhow, ensure};
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

fn wait_for_marker(path: &Path, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    ensure!(path.exists(), "server did not reach test-only marker {}", path.display());
    Ok(())
}

fn contains_uri(response: &Value, uri: &str) -> Result<bool> {
    ensure!(response.get("error").is_none(), "references returned error: {response}");
    let locations = response
        .get("result")
        .and_then(Value::as_array)
        .context("references response must contain a Location array")?;
    Ok(locations.iter().any(|location| location.get("uri").and_then(Value::as_str) == Some(uri)))
}

#[test]
fn first_references_ask_waits_for_closed_file_index_over_stdio() -> Result<()> {
    let binary = std::env::var("PERL_LSP_BIN")
        .context("set PERL_LSP_BIN to the exact just-built expose_lsp_test_api binary")?;
    let binary = std::fs::canonicalize(&binary).context("PERL_LSP_BIN must exist")?;
    ensure!(binary.is_absolute(), "PERL_LSP_BIN must be absolute");
    let expected_hash = std::env::var("PERL_LSP_PROOF_SHA256")
        .context("set PERL_LSP_PROOF_SHA256 from the just-built PERL_LSP_BIN")?;
    let actual_hash = format!("{:x}", Sha256::digest(std::fs::read(&binary)?));
    ensure!(
        actual_hash.eq_ignore_ascii_case(&expected_hash),
        "PERL_LSP_BIN hash differs from the just-built proof binary"
    );
    eprintln!("cold references proof binary: {} sha256={actual_hash}", binary.display());

    let gate = tempfile::tempdir().context("create stdio indexing gate")?;
    let gate_dir = gate.path().to_string_lossy().into_owned();
    let consumer = "sub local_probe { 1 }\nGlob::Alias::real_impl();\n";
    let glob = "package Glob::Alias;\nsub real_impl { 1 }\n1;\n";
    let config = ScenarioConfig::default()
        .env("PERL_LSP_TEST_INDEX_GATE_DIR", gate_dir)
        .with_file("lib/Glob.pm", glob)
        .with_file("consumer_glob.pl", consumer);
    let harness = UxHarness::new(config)?;
    wait_for_marker(&gate.path().join("indexing-entered"), Duration::from_secs(3))?;
    harness.open_fixture("consumer_glob.pl")?;

    let consumer_uri = harness.workspace.uri("consumer_glob.pl");
    let glob_uri = harness.workspace.uri("lib/Glob.pm");
    let call_line = consumer.lines().nth(1).context("fixture has call line")?;
    let character = u32::try_from(call_line.find("real_impl").context("fixture has real_impl")?)?;
    let params = json!({
        "textDocument": { "uri": consumer_uri },
        "position": { "line": 1, "character": character },
        "context": { "includeDeclaration": true }
    });

    std::thread::scope(|scope| -> Result<()> {
        let (sender, receiver) = mpsc::channel();
        let first_params = params.clone();
        let client = &harness.client;
        scope.spawn(move || {
            let result =
                client.request("textDocument/references", first_params, Duration::from_secs(12));
            let _ = sender.send(result);
        });

        wait_for_marker(&gate.path().join("references-wait-entered"), Duration::from_secs(3))?;
        match receiver.recv_timeout(Duration::from_millis(2_250)) {
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(anyhow!("references request thread exited without a response"));
            }
            Ok(response) => {
                return Err(anyhow!(
                    "first references ask answered while the closed file was unindexed: {response:?}"
                ));
            }
        }

        // A held references request must not stall an unrelated local read.
        let local = harness.client.request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": consumer_uri } }),
            Duration::from_secs(2),
        )?;
        ensure!(local.get("error").is_none(), "documentSymbol stalled or failed: {local}");
        let symbols = local
            .get("result")
            .and_then(Value::as_array)
            .context("documentSymbol must return an array")?;
        ensure!(
            symbols
                .iter()
                .any(|symbol| symbol.get("name").and_then(Value::as_str) == Some("local_probe")),
            "unrelated local declaration was missing while references waited: {local}"
        );

        ensure!(
            !gate.path().join("indexing-gate-timed-out").exists(),
            "index gate watchdog released discovery before the test"
        );
        match receiver.try_recv() {
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err(anyhow!("references request thread exited before gate release"));
            }
            Ok(response) => {
                return Err(anyhow!(
                    "references answered before the index gate was released: {response:?}"
                ));
            }
        }

        std::fs::write(gate.path().join("release"), b"")?;
        let first = receiver.recv_timeout(Duration::from_secs(8))??;
        ensure!(
            contains_uri(&first, &glob_uri)?,
            "first references ask missed closed-file declaration after warm-up: {first}"
        );

        let warm =
            harness.client.request("textDocument/references", params, Duration::from_secs(5))?;
        ensure!(
            contains_uri(&warm, &glob_uri)?,
            "same query after warm-up missed closed-file declaration: {warm}"
        );
        Ok(())
    })?;

    harness.assert_no_crash();
    Ok(())
}
