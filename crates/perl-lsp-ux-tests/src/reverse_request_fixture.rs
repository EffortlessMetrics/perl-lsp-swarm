//! In-memory-testable protocol fixture for conservative reverse-request tests.
//!
//! The spawned binary is a thin stdio entry point over [`run`]; every byte of
//! protocol logic lives here so the ripr+ gate can see each seam exercised by
//! in-memory tests rather than only by a spawned child process.

use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

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

/// Named deliberate wire failure, mirroring the spawned-process controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProtocolFailure {
    /// A header block with no usable Content-Length.
    MalformedFrame,
    /// A correctly framed body that is not valid JSON.
    InvalidJson,
    /// A Content-Length header whose body never arrives.
    PartialHeader,
}

/// Run the reverse-request fixture over framed JSON-RPC streams.
///
/// `protocol_failure` emits one deliberate wire failure after the handshake
/// and exits; `did_close` switches the request schedule to the didClose
/// registration round trip. The spawned binary maps its environment variables
/// onto these parameters so every mode stays in-memory testable.
pub fn run<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    protocol_failure: Option<ProtocolFailure>,
    did_close: Option<&str>,
) -> io::Result<()> {
    let initialize = read_message(reader)?;
    if initialize.get("method").and_then(Value::as_str) != Some("initialize") {
        return Err(protocol_error(format!("expected initialize, got {initialize}")));
    }
    write_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": initialize.get("id").cloned().unwrap_or(Value::Null),
            "result": { "capabilities": {} }
        }),
    )?;

    let initialized = read_message(reader)?;
    if initialized.get("method").and_then(Value::as_str) != Some("initialized") {
        return Err(protocol_error(format!("expected initialized, got {initialized}")));
    }

    if let Some(mode) = protocol_failure {
        let output = match mode {
            ProtocolFailure::MalformedFrame => "Content-Length: nope\r\n\r\n{}",
            ProtocolFailure::InvalidJson => "Content-Length: 8\r\n\r\nnot json",
            ProtocolFailure::PartialHeader => "Content-Length: 10\r\n",
        };
        writer.write_all(output.as_bytes())?;
        writer.flush()?;
        return Ok(());
    }

    let requests = if did_close.is_some() {
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
        write_message(writer, &request)?;
        let response = read_message(reader)?;
        if did_close == Some("advertised") {
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
        writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "test/ux-round-trip-complete",
            "params": { "requests": request_count }
        }),
    )?;

    loop {
        let message = read_message(reader)?;
        match message.get("method").and_then(Value::as_str) {
            Some("shutdown") => {
                write_message(
                    writer,
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
    use super::{ProtocolFailure, expect_response, read_message, run, write_message};
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
        for (input, expected_kind, expected_prefix) in [
            ("", io::ErrorKind::InvalidData, "EOF reading fixture message headers"),
            (
                "Content-Length: 2\r\n",
                io::ErrorKind::InvalidData,
                "EOF reading fixture message headers",
            ),
            (
                "Content-Type: application/json\r\n\r\n{}",
                io::ErrorKind::InvalidData,
                "missing Content-Length",
            ),
            (
                "Content-Length: nope\r\n\r\n{}",
                io::ErrorKind::InvalidData,
                "invalid Content-Length: invalid digit found in string",
            ),
            (
                "Content-Length: 2\r\n\r\n!x",
                io::ErrorKind::InvalidData,
                "invalid fixture JSON: expected value at line 1 column 1",
            ),
        ] {
            match read_message(&mut Cursor::new(input.as_bytes())) {
                Err(error) => {
                    // Exact-kind and exact-message oracles: a wrong path (for
                    // example swallowing the parse error or misclassifying a
                    // truncated frame) must fail this test by name.
                    assert_eq!(error.kind(), expected_kind, "kind for {input:?}");
                    assert!(
                        error.to_string().starts_with(expected_prefix),
                        "message for {input:?} was {:?}, expected prefix {expected_prefix:?}",
                        error.to_string()
                    );
                }
                result => {
                    bail!("fixture did not reject {input:?} with {expected_prefix:?}: {result:?}")
                }
            }
        }
        match read_message(&mut Cursor::new(b"Content-Length: 4\r\n\r\n{}")) {
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {}
            result => bail!("fixture accepted or misclassified truncated body: {result:?}"),
        }
        Ok(())
    }

    /// Each full-session error path is named by its exact protocol_error
    /// message: a wrong handshake stage, a response that is not the exact
    /// capability-rejection envelope, and post-completion traffic discipline.
    #[test]
    fn full_session_error_paths_are_named_exactly() -> Result<()> {
        let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
        let initialized = json!({"jsonrpc":"2.0","method":"initialized"});
        let rejection = json!({
            "jsonrpc": "2.0",
            "id": 41,
            "error": {
                "code": -32601,
                "message": "Client capability not advertised: \
                            workspace.textDocumentContent.refreshSupport for \
                            workspace/textDocumentContent/refresh"
            }
        });
        let second_rejection = json!({
            "jsonrpc": "2.0",
            "id": "configuration-42",
            "error": {
                "code": -32601,
                "message": "Client capability not advertised: workspace.configuration for \
                            workspace/configuration"
            }
        });

        // A non-initialize first message names the exact stage.
        let mut input = Vec::new();
        write_message(&mut input, &json!({"jsonrpc":"2.0","method":"initialized"}))?;
        let mut output = Cursor::new(Vec::new());
        let error =
            run(&mut Cursor::new(input), &mut output, None, None).err().ok_or_else(|| {
                anyhow::anyhow!("a non-initialize first message must fail the session")
            })?;
        assert!(
            error.to_string().starts_with("expected initialize, got {")
                && error.to_string().contains("\"method\":\"initialized\""),
            "exact initialize stage error, got {error}"
        );

        // A non-initialized second message names the exact stage.
        let mut input = Vec::new();
        write_message(&mut input, &initialize)?;
        write_message(
            &mut input,
            &json!({"jsonrpc":"2.0","id":9,"method":"initialize","params":{}}),
        )?;
        let mut output = Cursor::new(Vec::new());
        let error = run(&mut Cursor::new(input), &mut output, None, None)
            .err()
            .ok_or_else(|| anyhow::anyhow!("a repeated initialize must fail the session"))?;
        assert!(
            error.to_string().starts_with("expected initialized, got {\"id\":9,")
                && error.to_string().contains("\"method\":\"initialize\""),
            "exact initialized stage error, got {error}"
        );

        // A response that is a success result instead of the capability
        // rejection names the missing rejection exactly.
        let mut input = Vec::new();
        for message in [&initialize, &initialized, &json!({"jsonrpc":"2.0","id":41,"result":null})]
        {
            write_message(&mut input, message)?;
        }
        let mut output = Cursor::new(Vec::new());
        let error = run(&mut Cursor::new(input), &mut output, None, None)
            .err()
            .ok_or_else(|| anyhow::anyhow!("a success response must fail the gated session"))?;
        assert!(
            error.to_string().contains(
                "expected capability rejection for \
                workspace/textDocumentContent/refresh"
            ),
            "exact rejection evidence error, got {error}"
        );

        // A rejection with the wrong evidence message is named exactly.
        let mut input = Vec::new();
        for message in [&initialize, &initialized] {
            write_message(&mut input, message)?;
        }
        write_message(
            &mut input,
            &json!({"jsonrpc":"2.0","id":41,"error":{"code":-32601,"message":"unrelated"}}),
        )?;
        let mut output = Cursor::new(Vec::new());
        let error = run(&mut Cursor::new(input), &mut output, None, None)
            .err()
            .ok_or_else(|| anyhow::anyhow!("a wrong-evidence rejection must fail the session"))?;
        assert!(
            error.to_string().contains(
                "missing capability evidence for \
                workspace/textDocumentContent/refresh"
            ),
            "exact evidence error, got {error}"
        );

        // A response with a foreign id is named exactly.
        let mut input = Vec::new();
        for message in [&initialize, &initialized] {
            write_message(&mut input, message)?;
        }
        write_message(
            &mut input,
            &json!({"jsonrpc":"2.0","id":77,"error":{"code":-32601,"message":"Client capability not advertised: x"}}),
        )?;
        let mut output = Cursor::new(Vec::new());
        let error = run(&mut Cursor::new(input), &mut output, None, None)
            .err()
            .ok_or_else(|| anyhow::anyhow!("a foreign-id response must fail the session"))?;
        assert!(
            error
                .to_string()
                .starts_with("response id mismatch for workspace/textDocumentContent/refresh:"),
            "exact id mismatch error, got {error}"
        );

        // Post-completion traffic discipline: an unknown method and a
        // method-less message are both named exactly.
        for (extra, expected) in [
            (
                json!({"jsonrpc":"2.0","method":"totally/unknown"}),
                "unexpected client message totally/unknown",
            ),
            (json!({"jsonrpc":"2.0","result":null}), "client message has no method:"),
        ] {
            let mut input = Vec::new();
            for message in [&initialize, &initialized, &rejection, &second_rejection, &extra] {
                write_message(&mut input, message)?;
            }
            let mut output = Cursor::new(Vec::new());
            let error = run(&mut Cursor::new(input), &mut output, None, None)
                .err()
                .ok_or_else(|| anyhow::anyhow!("unexpected post-completion traffic must fail"))?;
            assert!(
                error.to_string().starts_with(expected),
                "exact post-completion error for {extra}, got {error}"
            );
        }
        Ok(())
    }

    /// The advertised didClose round trip names its exact evidence error when
    /// the registration response is anything but the accepted envelope.
    #[test]
    fn did_close_advertised_mode_names_invalid_envelopes_exactly() -> Result<()> {
        let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
        let initialized = json!({"jsonrpc":"2.0","method":"initialized"});
        for wrong in [
            json!({"jsonrpc":"2.0","id":"other","result":null}),
            json!({"jsonrpc":"2.0","id":"did-close-registration","error":{"code":-32601,"message":"Client capability not advertised: x"}}),
        ] {
            let mut input = Vec::new();
            for message in [&initialize, &initialized, &wrong] {
                write_message(&mut input, message)?;
            }
            let mut output = Cursor::new(Vec::new());
            let error = run(&mut Cursor::new(input), &mut output, None, Some("advertised"))
                .err()
                .ok_or_else(|| {
                anyhow::anyhow!("a wrong advertised registration must fail the session")
            })?;
            assert!(
                error
                    .to_string()
                    .starts_with("expected successful didClose registration with exact id:"),
                "exact didClose evidence error, got {error}"
            );
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

    /// Drive the full in-memory session: handshake, two gated requests
    /// answered with the exact conservative rejections, the completion event,
    /// and a clean shutdown/exit exchange.
    #[test]
    fn full_session_round_trips_two_gated_requests_and_clean_shutdown() -> Result<()> {
        let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
        let initialized = json!({"jsonrpc":"2.0","method":"initialized"});
        let first_rejection = json!({
            "jsonrpc": "2.0",
            "id": 41,
            "error": {
                "code": -32601,
                "message": "Client capability not advertised: \
                            workspace.textDocumentContent.refreshSupport for \
                            workspace/textDocumentContent/refresh"
            }
        });
        let second_rejection = json!({
            "jsonrpc": "2.0",
            "id": "configuration-42",
            "error": {
                "code": -32601,
                "message": "Client capability not advertised: workspace.configuration for \
                            workspace/configuration"
            }
        });
        let shutdown = json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":{}});
        let exit = json!({"jsonrpc":"2.0","method":"exit"});
        let mut input = Vec::new();
        for message in [initialize, initialized, first_rejection, second_rejection, shutdown, exit]
        {
            write_message(&mut input, &message)?;
        }

        let mut output = Cursor::new(Vec::new());
        run(&mut Cursor::new(input), &mut output, None, None)?;

        let frames: Vec<String> = String::from_utf8(output.into_inner())
            .map_err(|error| anyhow::anyhow!("fixture emitted non-UTF-8: {error}"))?
            .split("Content-Length: ")
            .filter(|frame| !frame.is_empty())
            .map(|frame| format!("Content-Length: {frame}"))
            .collect();
        let messages: Vec<serde_json::Value> = frames
            .iter()
            .map(|frame| {
                let body = frame.split("\r\n\r\n").nth(1).unwrap_or("");
                serde_json::from_str(body)
                    .map_err(|error| anyhow::anyhow!("bad fixture frame: {error}"))
            })
            .collect::<Result<Vec<_>>>()?;

        anyhow::ensure!(messages.len() == 5, "expected five fixture messages, got {messages:?}");
        anyhow::ensure!(messages[0]["id"] == 1, "initialize response must keep the id");
        anyhow::ensure!(messages[1]["id"] == 41, "first server request must be id 41");
        anyhow::ensure!(
            messages[1]["method"] == "workspace/textDocumentContent/refresh",
            "unexpected first request: {}",
            messages[1]["method"]
        );
        anyhow::ensure!(messages[2]["id"] == "configuration-42");
        anyhow::ensure!(
            messages[3]["method"] == "test/ux-round-trip-complete"
                && messages[3]["params"]["requests"] == 2,
            "completion event must report the request count: {messages:?}"
        );
        anyhow::ensure!(
            messages[4]["id"] == 2 && messages[4]["result"].is_null(),
            "shutdown must be answered with the matching id and a null result"
        );
        Ok(())
    }

    /// The didClose round trip accepts a successful registration and the
    /// rejected round trip requires the capability-rejection envelope; both
    /// reject wrong evidence.
    #[test]
    fn did_close_and_failure_modes_drive_their_named_paths() -> Result<()> {
        // Advertised: the client answers with a null-result registration.
        let mut input = Vec::new();
        write_message(
            &mut input,
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        )?;
        write_message(&mut input, &json!({"jsonrpc":"2.0","method":"initialized"}))?;
        write_message(
            &mut input,
            &json!({"jsonrpc":"2.0","id":"did-close-registration","result":null}),
        )?;
        write_message(&mut input, &json!({"jsonrpc":"2.0","method":"exit"}))?;
        let mut output = Cursor::new(Vec::new());
        run(&mut Cursor::new(input), &mut output, None, Some("advertised"))?;
        anyhow::ensure!(
            String::from_utf8_lossy(&output.into_inner()).contains("test/ux-round-trip-complete"),
            "advertised didClose round trip must complete"
        );

        // Each deliberate wire failure is emitted once after the handshake.
        for (mode, expected) in [
            (ProtocolFailure::MalformedFrame, "Content-Length: nope"),
            (ProtocolFailure::InvalidJson, "not json"),
            (ProtocolFailure::PartialHeader, "Content-Length: 10"),
        ] {
            let mut input = Vec::new();
            write_message(
                &mut input,
                &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            )?;
            write_message(&mut input, &json!({"jsonrpc":"2.0","method":"initialized"}))?;
            let mut output = Cursor::new(Vec::new());
            run(&mut Cursor::new(input), &mut output, Some(mode), None)?;
            let wire = String::from_utf8_lossy(&output.into_inner()).to_string();
            anyhow::ensure!(
                wire.contains(expected),
                "mode {mode:?} must emit its named wire failure: {wire}"
            );
        }
        Ok(())
    }
}
