use anyhow::{Result, anyhow};
use perl_lsp_ux_tests::{FakeWorkspace, ScenarioConfig, UxClient};
use serde_json::json;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn active_client_round_trips_two_gated_requests_through_a_child_process() -> Result<()> {
    let workspace = FakeWorkspace::new()?;
    let config = ScenarioConfig { timeout: Duration::from_secs(5), ..Default::default() };
    let client =
        UxClient::spawn(env!("CARGO_BIN_EXE_ux_server_request_fixture"), &workspace, &config)?;
    let deadline = Instant::now() + Duration::from_secs(5);

    let requests = loop {
        let requests = client.peek_server_requests();
        if requests.len() >= 2 {
            break requests;
        }
        if let Some(error) = client.peek_transport_error() {
            return Err(anyhow!("child-process round trip transport failure: {error}"));
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("timed out waiting for two child-process server requests"));
        }
        thread::sleep(Duration::from_millis(10));
    };

    assert_eq!(requests[0]["id"], 41);
    assert_eq!(requests[0]["method"], "workspace/textDocumentContent/refresh");
    assert_eq!(requests[1]["id"], "configuration-42");
    assert_eq!(requests[1]["method"], "workspace/configuration");

    let complete_deadline = Instant::now() + Duration::from_secs(5);
    while !client.peek_raw_events().iter().any(|event| {
        event["method"] == "test/ux-round-trip-complete" && event["params"]["requests"] == 2
    }) {
        if let Some(error) = client.peek_transport_error() {
            return Err(anyhow!("child-process round trip transport failure: {error}"));
        }
        if Instant::now() >= complete_deadline {
            return Err(anyhow!("child process did not report completed response round trips"));
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(
        client.peek_capability_violations(),
        vec![
            perl_lsp_ux_tests::CapabilityViolation {
                id: json!(41),
                method: "workspace/textDocumentContent/refresh".to_owned(),
                capability: "workspace.textDocumentContent.refreshSupport".to_owned(),
            },
            perl_lsp_ux_tests::CapabilityViolation {
                id: json!("configuration-42"),
                method: "workspace/configuration".to_owned(),
                capability: "workspace.configuration".to_owned(),
            },
        ]
    );
    let _drained_events = client.drain_events();
    assert_eq!(client.peek_server_requests().len(), 2);
    assert!(client.peek_transport_error().is_none());

    client.shutdown_and_wait(Duration::from_secs(2))?;
    assert!(client.peek_transport_error().is_none());
    Ok(())
}

fn assert_protocol_failure(mode: &str, expected: &str) -> Result<()> {
    let workspace = FakeWorkspace::new()?;
    let config = ScenarioConfig {
        timeout: Duration::from_secs(2),
        extra_env: vec![("UX_FIXTURE_PROTOCOL_FAILURE".to_owned(), Some(mode.to_owned()))],
        ..Default::default()
    };
    let client =
        UxClient::spawn(env!("CARGO_BIN_EXE_ux_server_request_fixture"), &workspace, &config)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(error) = client.peek_transport_error() {
            assert!(error.contains(expected), "mode={mode} error={error}");
            break;
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("fixture mode {mode} did not report a transport failure"));
        }
        thread::sleep(Duration::from_millis(10));
    }
    drop(client);
    Ok(())
}

#[test]
fn malformed_frame_from_child_is_a_transport_failure() -> Result<()> {
    assert_protocol_failure("malformed-frame", "No Content-Length")
}

#[test]
fn invalid_json_from_child_is_a_transport_failure() -> Result<()> {
    assert_protocol_failure("invalid-json", "Failed to parse LSP JSON body")
}
