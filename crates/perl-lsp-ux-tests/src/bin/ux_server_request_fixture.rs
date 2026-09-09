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
            "partial-header" => "Content-Length: 10\r\n",
            _ => return Err(protocol_error(format!("unknown protocol failure mode: {mode}"))),
        };
        writer.write_all(output.as_bytes())?;
        writer.flush()?;
        return Ok(());
    }

    let did_close_mode = std::env::var("UX_FIXTURE_DID_CLOSE").ok();
    let requests = if did_close_mode.is_some() {
        vec![json!({
            "jsonrpc": "2.0",
            "id": "did-close-registration",
            "method": "client/registerCapability",
            "params": {
                "registrations": [{
                    "id": "close",
                    "method": "textDocument/didClose",
                    "registerOptions": { "documentSelector": [{ "language": "perl" }] }
                }]
            }
        })]
    } else {
        vec![
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
        ]
    };
    let request_count = requests.len();
    for request in requests {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        write_message(&mut writer, &request)?;
        let response = read_message(&mut reader)?;
        if did_close_mode.as_deref() == Some("advertised") {
            if response.get("id") != Some(&id)
                || response.get("result") != Some(&Value::Null)
                || response.get("error").is_some()
            {
                return Err(protocol_error(format!(
                    "expected successful didClose registration with exact id: {response}"
                )));
            }
        } else {
            expect_response(&response, &id, method)?;
        }
    }

    write_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "test/ux-round-trip-complete",
            "params": { "requests": request_count }
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

#[cfg(test)]
mod tests {
    use super::{expect_response, read_message, write_message};
    use anyhow::{Result, bail};
    use serde_json::json;
    use std::io::{self, Cursor, Write};

    #[test]
    fn fixture_reads_one_utf8_frame_and_preserves_the_next() -> Result<()> {
        let first = r#"{"id":"é","result":null}"#;
        let second = r#"{"method":"exit"}"#;
        let input = format!(
            "Content-Length:  {} \r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{first}Content-Length: {}\r\n\r\n{second}",
            first.len(),
            second.len()
        );
        let mut reader = Cursor::new(input.into_bytes());
        if read_message(&mut reader)? != json!({"id":"é","result":null}) {
            bail!("fixture lost the first UTF-8 response");
        }
        if read_message(&mut reader)? != json!({"method":"exit"}) {
            bail!("fixture consumed bytes belonging to the next message");
        }
        Ok(())
    }

    #[test]
    fn fixture_rejects_invalid_or_truncated_client_frames() -> Result<()> {
        for (input, expected) in [
            ("", "EOF reading fixture message headers"),
            ("Content-Length: 2\r\n", "EOF reading fixture message headers"),
            ("Content-Type: application/json\r\n\r\n{}", "missing Content-Length"),
            ("Content-Length: nope\r\n\r\n{}", "invalid Content-Length"),
            ("Content-Length: 2\r\n\r\n!x", "invalid fixture JSON"),
        ] {
            match read_message(&mut Cursor::new(input.as_bytes())) {
                Err(error)
                    if error.kind() == io::ErrorKind::InvalidData
                        && error.to_string().contains(expected) => {}
                result => bail!("fixture did not reject {input:?} with {expected:?}: {result:?}"),
            }
        }
        match read_message(&mut Cursor::new(b"Content-Length: 4\r\n\r\n{}")) {
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {}
            result => bail!("fixture accepted or misclassified truncated body: {result:?}"),
        }
        Ok(())
    }

    #[derive(Default)]
    struct RecordingWriter {
        bytes: Vec<u8>,
        flushed: bool,
        fail_write: bool,
        fail_flush: bool,
    }

    impl Write for RecordingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.fail_write {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "write rejected"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            self.flushed = true;
            if self.fail_flush {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "flush rejected"));
            }
            Ok(())
        }
    }

    #[test]
    fn fixture_writes_byte_length_flushes_and_propagates_io_failures() -> Result<()> {
        let message = json!({"id":"é"});
        let mut writer = RecordingWriter::default();
        write_message(&mut writer, &message)?;
        // Literal wire oracle: the 10-character JSON body occupies 11 UTF-8 bytes.
        if writer.bytes != "Content-Length: 11\r\n\r\n{\"id\":\"é\"}".as_bytes() || !writer.flushed
        {
            bail!("fixture emitted an incorrect UTF-8 frame or failed to flush");
        }
        for mut writer in [
            RecordingWriter { fail_write: true, ..Default::default() },
            RecordingWriter { fail_flush: true, ..Default::default() },
        ] {
            match write_message(&mut writer, &message) {
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {}
                result => bail!("fixture swallowed the writer failure: {result:?}"),
            }
        }
        Ok(())
    }

    #[test]
    fn fixture_requires_exact_response_id_code_and_capability_evidence() -> Result<()> {
        for id in [json!(41), json!("configuration-42")] {
            let response = json!({"id":id,"error":{"code":-32601,"message":"Client capability not advertised: workspace.configuration"}});
            expect_response(&response, &id, "workspace/configuration")?;
            for invalid in [
                json!({"id":null,"error":response.get("error")}),
                json!({"id":id,"error":{"code":-32602,"message":"Client capability not advertised"}}),
                json!({"id":id,"error":{"code":-32601,"message":"unrelated failure"}}),
                json!({"id":id,"error":{"code":-32601}}),
                json!({"id":id,"result":null}),
            ] {
                match expect_response(&invalid, &id, "workspace/configuration") {
                    Err(error)
                        if error.kind() == io::ErrorKind::InvalidData
                            && error.to_string().contains("workspace/configuration") => {}
                    result => bail!(
                        "fixture accepted an invalid response or lost method context: {invalid}: {result:?}"
                    ),
                }
            }
        }
        Ok(())
    }
}
