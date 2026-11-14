//! Exact-process proof that the shipped `perllsp` binary rejects test-only
//! protocol routes in ordinary product mode (#9207).

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::time::Duration;

const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);

fn error_code(response: &Value) -> Option<i64> {
    response.get("error").and_then(|error| error.get("code")).and_then(Value::as_i64)
}

#[test]
fn perllsp_rejects_test_slow_operation_in_ordinary_product_mode() -> Result<()> {
    let mut client = RealProcessClient::spawn_exact()?;
    let response = client.request(
        json!(9207),
        "initialize",
        json!({
            "processId": null,
            "rootUri": null,
            "capabilities": {}
        }),
        PROCESS_TIMEOUT,
    )?;
    ensure!(response.get("error").is_none(), "initialize failed: {response}");
    client.notify("initialized", json!({}))?;

    let response = client
        .request(json!(9207), "$/test/slowOperation", json!({}), PROCESS_TIMEOUT)
        .context("slowOperation request must produce a response")?;

    ensure!(
        error_code(&response) == Some(-32601),
        "ordinary product binary must reject test-only slowOperation, got: {response}"
    );
    Ok(())
}
