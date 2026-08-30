use anyhow::{Result, anyhow, ensure};
use perl_lsp_ux_tests::{
    ObservedServerRequest, ScriptedServerRequest, ScriptedServerResponse, ServerRequestDelivery,
    UxClient,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const CONFIGURATION_ID: &str = "fixture-configuration";
const REGISTRATION_ID: u64 = 41;
const PROGRESS_ID: &str = "fixture-progress";
const SHOW_DOCUMENT_ID: &str = "fixture-show-document";

#[test]
fn scripted_client_completes_success_error_delay_and_timeout_outcomes() -> Result<()> {
    let binary = env!("CARGO_BIN_EXE_ux_server_request_fixture");
    let timeout = Duration::from_secs(5);
    let script = vec![
        ScriptedServerRequest::success(
            "workspace/configuration",
            json!([{"perlPath": "fixture-perl"}]),
        ),
        ScriptedServerRequest::new(
            "client/registerCapability",
            ScriptedServerResponse::error_with_data(
                -32_001,
                "fixture registration rejected",
                json!({"reason": "negative-control"}),
            )
            .after(Duration::from_millis(500)),
        ),
        ScriptedServerRequest::success("window/workDoneProgress/create", Value::Null),
        ScriptedServerRequest::no_response("window/showDocument"),
    ];
    let client = UxClient::spawn_scripted(binary, "file:///fixture", script, timeout)?;
    let observed = client.wait_for_script(timeout)?;
    ensure!(observed.len() == 4, "expected four server requests, got {observed:#?}");
    client.assert_no_unscripted_requests()?;

    let configuration = request_by_method(&observed, "workspace/configuration")?;
    ensure!(configuration.id == json!(CONFIGURATION_ID));
    ensure!(configuration.delivery == ServerRequestDelivery::Sent);
    ensure!(
        configuration
            .scripted_response
            .as_ref()
            .and_then(|response| response.pointer("/result/0/perlPath"))
            == Some(&json!("fixture-perl"))
    );

    let registration = request_by_method(&observed, "client/registerCapability")?;
    ensure!(registration.id == json!(REGISTRATION_ID));
    ensure!(registration.delivery == ServerRequestDelivery::Sent);
    ensure!(
        registration
            .scripted_response
            .as_ref()
            .and_then(|response| response.pointer("/error/data/reason"))
            == Some(&json!("negative-control"))
    );

    let progress = request_by_method(&observed, "window/workDoneProgress/create")?;
    ensure!(progress.id == json!(PROGRESS_ID));
    ensure!(progress.delivery == ServerRequestDelivery::Sent);

    let show_document = request_by_method(&observed, "window/showDocument")?;
    ensure!(show_document.id == json!(SHOW_DOCUMENT_ID));
    ensure!(show_document.delivery == ServerRequestDelivery::IntentionallyPending);
    ensure!(show_document.scripted_response.is_none());

    let round_trips = wait_for_event(&client, "fixture/server-request-round-trips", timeout)?;
    ensure!(
        round_trips.pointer("/configurationResponse/result/0/perlPath")
            == Some(&json!("fixture-perl"))
    );
    ensure!(round_trips.pointer("/registrationResponse/error/code") == Some(&json!(-32_001)));
    ensure!(round_trips.pointer("/progressResponse/result") == Some(&Value::Null));
    ensure!(
        round_trips.pointer("/responseOrder") == Some(&json!([PROGRESS_ID, REGISTRATION_ID])),
        "unrelated immediate response should arrive before delayed rejection: {round_trips}"
    );
    ensure!(round_trips.pointer("/registrationWasDelayed") == Some(&json!(true)));

    let shutdown = client.request("shutdown", json!({}), timeout)?;
    ensure!(
        shutdown.pointer("/result/unexpectedShowDocumentResponse") == Some(&json!(false)),
        "NoResponse must not become an implicit null success: {shutdown}"
    );
    client.notify("exit", json!({}))?;
    Ok(())
}

fn request_by_method<'a>(
    observed: &'a [ObservedServerRequest],
    method: &str,
) -> Result<&'a ObservedServerRequest> {
    observed
        .iter()
        .find(|request| request.method == method)
        .ok_or_else(|| anyhow!("server request {method} was not observed: {observed:#?}"))
}

fn wait_for_event(client: &UxClient, method: &str, timeout: Duration) -> Result<Value> {
    let deadline = Instant::now() + timeout;
    loop {
        for event in client.peek_raw_events() {
            if event.get("method").and_then(Value::as_str) == Some(method) {
                return Ok(event.get("params").cloned().unwrap_or(Value::Null));
            }
        }
        if Instant::now() >= deadline {
            return Err(anyhow!(
                "timed out after {}ms waiting for {method}; stderr={:#?}",
                timeout.as_millis(),
                client.peek_stderr_lines()
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
