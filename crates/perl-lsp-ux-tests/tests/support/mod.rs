mod scripted_client;
mod server_request_script;

pub use scripted_client::ScriptedClient;
pub use server_request_script::{
    ObservedServerRequest, ScriptedServerRequest, ScriptedServerResponse, ServerRequestDelivery,
};

use anyhow::{Context, Result};
use serde_json::Value;
use std::io::Write;
use std::process::ChildStdin;
use std::sync::{Arc, Mutex};

pub(crate) fn write_shared_message(stdin: &Arc<Mutex<ChildStdin>>, message: &Value) -> Result<()> {
    let mut stdin = stdin.lock().unwrap_or_else(|error| error.into_inner());
    let body = message.to_string();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    stdin.write_all(header.as_bytes()).context("failed to write LSP header")?;
    stdin.write_all(body.as_bytes()).context("failed to write LSP body")?;
    stdin.flush().context("failed to flush LSP message")
}
