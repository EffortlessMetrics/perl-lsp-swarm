//! In-memory-testable protocol fixture for scripted server requests.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

const CONFIGURATION_ID: &str = "fixture-configuration";
const REGISTRATION_ID: u64 = 41;
const PROGRESS_ID: &str = "fixture-progress";
const SHOW_DOCUMENT_ID: &str = "fixture-show-document";

/// Run the scripted server-request fixture over framed JSON-RPC streams.
pub fn run<R: BufRead, W: Write>(reader: &mut R, writer: &mut W) -> Result<()> {
    let initialize = read_one_message(reader).context("fixture expected initialize request")?;
    let initialize_id = request_id(&initialize, "initialize")?;
    write_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": initialize_id,
            "result": {
                "capabilities": {}
            }
        }),
    )?;

    let initialized = read_one_message(reader).context("fixture expected initialized")?;
    require_notification(&initialized, "initialized")?;

    write_message(
        writer,
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
        read_response(reader, &json!(CONFIGURATION_ID), "workspace/configuration")?;

    let registration_started = Instant::now();
    write_message(
        writer,
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
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": PROGRESS_ID,
            "method": "window/workDoneProgress/create",
            "params": {
                "token": "fixture-progress-token"
            }
        }),
    )?;

    let first_concurrent_response =
        read_one_message(reader).context("fixture expected a registration or progress response")?;
    let first_response_id = response_id(&first_concurrent_response)?;
    let first_response_elapsed = registration_started.elapsed();
    let second_concurrent_response = read_one_message(reader)
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
        writer,
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
        writer,
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

    serve_until_exit(reader, writer)
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

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{bail, ensure};
    use std::io::Cursor;

    fn frame(message: &Value) -> Vec<u8> {
        let body = message.to_string();
        format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
    }

    fn input_messages(messages: &[Value]) -> Cursor<Vec<u8>> {
        let mut input = Vec::new();
        for message in messages {
            input.extend(frame(message));
        }
        Cursor::new(input)
    }

    fn parse_messages(output: &[u8], count: usize) -> Result<Vec<Value>> {
        let mut reader = Cursor::new(output);
        let mut messages = Vec::with_capacity(count);
        for _ in 0..count {
            messages.push(read_one_message(&mut reader)?);
        }
        Ok(messages)
    }

    fn conversation(registration_first: bool, show_document_response: bool) -> Result<Vec<Value>> {
        let registration_response = json!({
            "jsonrpc": "2.0",
            "id": REGISTRATION_ID,
            "result": {"registered": true}
        });
        let progress_response = json!({
            "jsonrpc": "2.0",
            "id": PROGRESS_ID,
            "result": {"created": true}
        });
        let mut responses = vec![
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {}
            }),
            json!({"jsonrpc": "2.0", "method": "initialized"}),
            json!({
                "jsonrpc": "2.0",
                "id": CONFIGURATION_ID,
                "result": [{"perlPath": "fixture-perl"}]
            }),
        ];
        if registration_first {
            responses.push(registration_response);
            responses.push(progress_response);
        } else {
            responses.push(progress_response);
            responses.push(registration_response);
        }
        if show_document_response {
            responses.push(json!({
                "jsonrpc": "2.0",
                "id": SHOW_DOCUMENT_ID,
                "result": {"shown": true}
            }));
        }
        responses.push(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown",
            "params": {}
        }));
        responses.push(json!({"jsonrpc": "2.0", "method": "exit"}));

        let mut reader = input_messages(&responses);
        let mut output = Vec::new();
        run(&mut reader, &mut output)?;
        parse_messages(&output, 7)
    }

    fn message_at(messages: &[Value], index: usize) -> Result<&Value> {
        messages.get(index).ok_or_else(|| anyhow!("missing output message {index}"))
    }

    #[test]
    fn happy_path_emits_exact_ids_and_round_trip_payload() -> Result<()> {
        let messages = conversation(true, true)?;
        ensure!(message_at(&messages, 0)?.get("id") == Some(&json!(1)));
        ensure!(message_at(&messages, 1)?.get("method") == Some(&json!("workspace/configuration")));
        ensure!(message_at(&messages, 1)?.get("id") == Some(&json!(CONFIGURATION_ID)));
        ensure!(
            message_at(&messages, 2)?.get("method") == Some(&json!("client/registerCapability"))
        );
        ensure!(message_at(&messages, 2)?.get("id") == Some(&json!(REGISTRATION_ID)));
        ensure!(
            message_at(&messages, 3)?.get("method")
                == Some(&json!("window/workDoneProgress/create"))
        );
        ensure!(message_at(&messages, 3)?.get("id") == Some(&json!(PROGRESS_ID)));
        ensure!(message_at(&messages, 4)?.get("method") == Some(&json!("window/showDocument")));
        ensure!(message_at(&messages, 4)?.get("id") == Some(&json!(SHOW_DOCUMENT_ID)));
        let notification = message_at(&messages, 5)?;
        ensure!(notification.get("method") == Some(&json!("fixture/server-request-round-trips")));
        ensure!(
            notification.pointer("/params/configurationResponse/result/0/perlPath")
                == Some(&json!("fixture-perl"))
        );
        ensure!(
            notification.pointer("/params/registrationResponse/result/registered")
                == Some(&json!(true))
        );
        ensure!(
            notification.pointer("/params/progressResponse/result/created") == Some(&json!(true))
        );
        ensure!(notification.pointer("/params/responseOrder") == Some(&json!([41, PROGRESS_ID])));
        ensure!(notification.pointer("/params/registrationWasDelayed") == Some(&json!(false)));
        ensure!(
            message_at(&messages, 6)?.pointer("/result/unexpectedShowDocumentResponse")
                == Some(&json!(true))
        );
        Ok(())
    }

    #[test]
    fn progress_first_preserves_response_mapping_and_order() -> Result<()> {
        let messages = conversation(false, true)?;
        let notification = message_at(&messages, 5)?;
        ensure!(
            notification.pointer("/params/registrationResponse/result/registered")
                == Some(&json!(true))
        );
        ensure!(
            notification.pointer("/params/progressResponse/result/created") == Some(&json!(true))
        );
        ensure!(notification.pointer("/params/responseOrder") == Some(&json!([PROGRESS_ID, 41])));
        Ok(())
    }

    #[test]
    fn shutdown_without_show_document_response_reports_false() -> Result<()> {
        let messages = conversation(true, false)?;
        ensure!(
            message_at(&messages, 6)?.pointer("/result/unexpectedShowDocumentResponse")
                == Some(&json!(false))
        );
        Ok(())
    }

    #[test]
    fn request_and_notification_validation_reports_exact_errors() -> Result<()> {
        let Err(wrong_method) =
            request_id(&json!({"jsonrpc": "2.0", "id": 1, "method": "other"}), "initialize")
        else {
            bail!("wrong method must fail");
        };
        ensure!(wrong_method.to_string().contains("fixture expected initialize"));
        ensure!(wrong_method.to_string().contains("\"method\":\"other\""));
        for message in [
            json!({"jsonrpc": "2.0", "method": "initialize"}),
            json!({"jsonrpc": "2.0", "id": null, "method": "initialize"}),
        ] {
            let Err(error) = request_id(&message, "initialize") else {
                bail!("missing id must fail");
            };
            ensure!(error.to_string().contains("fixture initialize request had no id"));
        }
        let Err(wrong_notification) =
            require_notification(&json!({"jsonrpc": "2.0", "method": "other"}), "initialized")
        else {
            bail!("wrong notification method must fail");
        };
        ensure!(wrong_notification.to_string().contains("fixture expected initialized"));
        let Err(notification_id) = require_notification(
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialized"}),
            "initialized",
        ) else {
            bail!("notification id must fail");
        };
        ensure!(notification_id.to_string().contains("notification without id"));
        Ok(())
    }

    #[test]
    fn response_validation_reports_exact_errors() -> Result<()> {
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": "request"});
        let Err(response_error) = response_id(&request) else {
            bail!("request must not be a response");
        };
        ensure!(response_error.to_string().contains("expected a JSON-RPC response"));
        for (first, second) in [
            (json!(REGISTRATION_ID), json!(REGISTRATION_ID)),
            (json!(REGISTRATION_ID), json!("other")),
        ] {
            let Err(error) = require_response_pair(&first, &second) else {
                bail!("invalid response pair must fail");
            };
            ensure!(error.to_string().contains("fixture expected responses for ids"));
        }
        Ok(())
    }

    #[test]
    fn framing_validation_reports_header_errors() -> Result<()> {
        let Err(eof) = read_one_message(&mut Cursor::new(b"Content-Length: 4\r\n".to_vec())) else {
            bail!("truncated headers must fail");
        };
        ensure!(eof.to_string().contains("fixture reached EOF while reading LSP headers"));
        let Err(missing_length) =
            read_one_message(&mut Cursor::new(b"X-Test: value\r\n\r\n".to_vec()))
        else {
            bail!("missing content length must fail");
        };
        ensure!(missing_length.to_string() == "fixture message had no Content-Length");
        Ok(())
    }
}
