use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, Instant};

const CONFIGURATION_ID: &str = "fixture-configuration";
const REGISTRATION_ID: u64 = 41;
const PROGRESS_ID: &str = "fixture-progress";
const SHOW_DOCUMENT_ID: &str = "fixture-show-document";

fn main() -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();

    let initialize =
        read_one_message(&mut reader).context("fixture expected initialize request")?;
    let initialize_id = request_id(&initialize, "initialize")?;
    write_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": initialize_id,
            "result": {
                "capabilities": {}
            }
        }),
    )?;

    let initialized = read_one_message(&mut reader).context("fixture expected initialized")?;
    require_notification(&initialized, "initialized")?;

    write_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": CONFIGURATION_ID,
            "method": "workspace/configuration",
            "params": {
                "items": [
                    {
                        "scopeUri": "file:///fixture",
                        "section": "perl-lsp"
                    }
                ]
            }
        }),
    )?;
    let configuration_response =
        read_response(&mut reader, &json!(CONFIGURATION_ID), "workspace/configuration")?;

    let registration_started = Instant::now();
    write_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": REGISTRATION_ID,
            "method": "client/registerCapability",
            "params": {
                "registrations": [
                    {
                        "id": "fixture-registration",
                        "method": "workspace/didChangeWatchedFiles",
                        "registerOptions": {
                            "watchers": [
                                {
                                    "globPattern": "**/*.pl",
                                    "kind": 7
                                }
                            ]
                        }
                    }
                ]
            }
        }),
    )?;
    write_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": PROGRESS_ID,
            "method": "window/workDoneProgress/create",
            "params": {
                "token": "fixture-progress-token"
            }
        }),
    )?;

    let first_concurrent_response = read_one_message(&mut reader)
        .context("fixture expected a registration or progress response")?;
    let first_response_id = response_id(&first_concurrent_response)?;
    let first_response_elapsed = registration_started.elapsed();
    let second_concurrent_response = read_one_message(&mut reader)
        .context("fixture expected the remaining registration or progress response")?;
    let second_response_id = response_id(&second_concurrent_response)?;
    let second_response_elapsed = registration_started.elapsed();
    require_response_pair(&first_response_id, &second_response_id)?;

    let (registration_response, registration_elapsed, progress_response) =
        if first_response_id == json!(REGISTRATION_ID) {
            (first_concurrent_response, first_response_elapsed, second_concurrent_response)
        } else {
            (second_concurrent_response, second_response_elapsed, first_concurrent_response)
        };
    let response_order = [first_response_id, second_response_id];
    let registration_was_delayed = registration_elapsed >= Duration::from_millis(300);

    write_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": SHOW_DOCUMENT_ID,
            "method": "window/showDocument",
            "params": {
                "uri": "file:///fixture/lib/Example.pm",
                "takeFocus": true
            }
        }),
    )?;

    write_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "fixture/server-request-round-trips",
            "params": {
                "configurationResponse": configuration_response,
                "registrationResponse": registration_response,
                "progressResponse": progress_response,
                "responseOrder": response_order,
                "registrationWasDelayed": registration_was_delayed
            }
        }),
    )?;

    serve_until_exit(&mut reader, &mut writer)
}

fn response_id(message: &Value) -> Result<Value> {
    if message.get("method").is_some()
        || (message.get("result").is_none() && message.get("error").is_none())
    {
        bail!("fixture expected a JSON-RPC response, received {message}");
    }
    message
        .get("id")
        .filter(|id| !id.is_null())
        .cloned()
        .ok_or_else(|| anyhow!("fixture response had no id: {message}"))
}

fn require_response_pair(first_id: &Value, second_id: &Value) -> Result<()> {
    let registration_id = json!(REGISTRATION_ID);
    let progress_id = json!(PROGRESS_ID);
    let has_registration = first_id == &registration_id || second_id == &registration_id;
    let has_progress = first_id == &progress_id || second_id == &progress_id;
    if first_id == second_id || !has_registration || !has_progress {
        bail!(
            "fixture expected responses for ids {registration_id} and {progress_id}, got {first_id} and {second_id}"
        );
    }
    Ok(())
}

fn serve_until_exit(reader: &mut impl BufRead, writer: &mut impl Write) -> Result<()> {
    let mut unexpected_show_document_response = false;
    loop {
        let message = read_one_message(reader)?;
        if is_response_for(&message, &json!(SHOW_DOCUMENT_ID)) {
            unexpected_show_document_response = true;
            continue;
        }

        match message.get("method").and_then(Value::as_str) {
            Some("shutdown") => {
                let id = request_id(&message, "shutdown")?;
                write_message(
                    writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "unexpectedShowDocumentResponse": unexpected_show_document_response
                        }
                    }),
                )?;
            }
            Some("exit") => return Ok(()),
            Some(_) | None => {}
        }
    }
}

fn request_id(message: &Value, method: &str) -> Result<Value> {
    if message.get("method").and_then(Value::as_str) != Some(method) {
        bail!("fixture expected {method}, received {message}");
    }
    message
        .get("id")
        .filter(|id| !id.is_null())
        .cloned()
        .ok_or_else(|| anyhow!("fixture {method} request had no id: {message}"))
}

fn require_notification(message: &Value, method: &str) -> Result<()> {
    if message.get("method").and_then(Value::as_str) != Some(method) {
        bail!("fixture expected {method}, received {message}");
    }
    if message.get("id").is_some() {
        bail!("fixture expected {method} notification without id: {message}");
    }
    Ok(())
}

fn read_response(reader: &mut impl BufRead, id: &Value, method: &str) -> Result<Value> {
    let response = read_one_message(reader)
        .with_context(|| format!("fixture expected response to {method} id={id}"))?;
    if !is_response_for(&response, id) {
        bail!("fixture expected response to {method} id={id}, received {response}");
    }
    Ok(response)
}

fn is_response_for(message: &Value, id: &Value) -> bool {
    message.get("id") == Some(id)
        && (message.get("result").is_some() || message.get("error").is_some())
        && message.get("method").is_none()
}

fn read_one_message(reader: &mut impl BufRead) -> Result<Value> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            return Err(anyhow!("fixture reached EOF while reading LSP headers"));
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.to_ascii_lowercase().strip_prefix("content-length") {
            content_length = value.trim_start_matches(':').trim().parse::<usize>().ok();
        }
    }

    let length = content_length.ok_or_else(|| anyhow!("fixture message had no Content-Length"))?;
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).context("fixture could not decode LSP JSON body")
}

fn write_message(writer: &mut impl Write, message: &Value) -> Result<()> {
    let body = message.to_string();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes())?;
    writer.write_all(body.as_bytes())?;
    writer.flush()?;
    Ok(())
}
