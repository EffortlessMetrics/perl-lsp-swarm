use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, BufWriter, Write};

fn protocol_error(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Value> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(protocol_error("EOF reading fixture message headers"));
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|error| protocol_error(format!("invalid Content-Length: {error}")))?,
            );
        }
    }
    let length = content_length.ok_or_else(|| protocol_error("missing Content-Length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map_err(|error| protocol_error(format!("invalid fixture JSON: {error}")))
}

fn write_message(writer: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = message.to_string();
    write!(writer, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    writer.flush()
}

fn expect_response(response: &Value, expected_id: &Value, expected_method: &str) -> io::Result<()> {
    if response.get("id") != Some(expected_id) {
        return Err(protocol_error(format!(
            "response id mismatch for {expected_method}: {response}"
        )));
    }
    if response.pointer("/error/code") != Some(&json!(-32601)) {
        return Err(protocol_error(format!(
            "expected capability rejection for {expected_method}: {response}"
        )));
    }
    let message = response.pointer("/error/message").and_then(Value::as_str).unwrap_or("");
    if !message.contains("Client capability not advertised") {
        return Err(protocol_error(format!(
            "missing capability evidence for {expected_method}: {response}"
        )));
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());

    let initialize = read_message(&mut reader)?;
    if initialize.get("method").and_then(Value::as_str) != Some("initialize") {
        return Err(protocol_error(format!("expected initialize, got {initialize}")));
    }
    write_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": initialize.get("id").cloned().unwrap_or(Value::Null),
            "result": { "capabilities": {} }
        }),
    )?;

    let initialized = read_message(&mut reader)?;
    if initialized.get("method").and_then(Value::as_str) != Some("initialized") {
        return Err(protocol_error(format!("expected initialized, got {initialized}")));
    }

    if let Ok(mode) = std::env::var("UX_FIXTURE_PROTOCOL_FAILURE") {
        let output = match mode.as_str() {
            "malformed-frame" => "Content-Length: nope\r\n\r\n{}",
            "invalid-json" => "Content-Length: 8\r\n\r\nnot json",
            _ => return Err(protocol_error(format!("unknown protocol failure mode: {mode}"))),
        };
        writer.write_all(output.as_bytes())?;
        writer.flush()?;
        return Ok(());
    }

    let requests = [
        json!({
            "jsonrpc": "2.0",
            "id": 41,
            "method": "workspace/textDocumentContent/refresh",
            "params": {}
        }),
        json!({
            "jsonrpc": "2.0",
            "id": "configuration-42",
            "method": "workspace/configuration",
            "params": { "items": [{ "section": "perl" }] }
        }),
    ];
    for request in requests {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        write_message(&mut writer, &request)?;
        let response = read_message(&mut reader)?;
        expect_response(&response, &id, method)?;
    }

    write_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "test/ux-round-trip-complete",
            "params": { "requests": 2 }
        }),
    )?;

    loop {
        let message = read_message(&mut reader)?;
        match message.get("method").and_then(Value::as_str) {
            Some("shutdown") => {
                write_message(
                    &mut writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": message.get("id").cloned().unwrap_or(Value::Null),
                        "result": null
                    }),
                )?;
            }
            Some("exit") => return Ok(()),
            Some(method) => {
                return Err(protocol_error(format!("unexpected client message {method}")));
            }
            None => return Err(protocol_error(format!("client message has no method: {message}"))),
        }
    }
}
