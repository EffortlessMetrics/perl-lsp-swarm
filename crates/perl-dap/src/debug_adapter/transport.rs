//! Transport layer: run (stdin/stdout), run_socket, run_with_io.

use super::*;
use std::sync::mpsc::TryRecvError;

const EVENT_WRITE_BATCH_MAX: usize = 64;

fn write_framed_payload<W: Write>(writer: &mut W, payload: &[u8]) -> io::Result<()> {
    writer.write_all(b"Content-Length: ")?;
    writer.write_all(payload.len().to_string().as_bytes())?;
    writer.write_all(b"\r\n\r\n")?;
    writer.write_all(payload)
}

impl DebugAdapter {
    /// Run the debug adapter server
    pub(crate) fn run(&mut self) -> io::Result<()> {
        self.run_with_io(io::stdin(), io::stdout())
    }

    /// Run the debug adapter over a TCP socket transport.
    ///
    /// This binds to `127.0.0.1:<port>`, accepts one client connection, and
    /// serves the DAP session on that stream.
    pub(crate) fn run_socket(&mut self, port: u16) -> io::Result<()> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        tracing::info!(port, "DAP socket transport listening on 127.0.0.1");

        let (stream, peer_addr) = listener.accept()?;
        tracing::info!(peer_addr = %peer_addr, "DAP socket client connected");

        let reader_stream = stream.try_clone()?;
        self.run_with_io(reader_stream, stream)
    }

    /// Shared DAP transport loop used by stdio and socket modes.
    pub(super) fn run_with_io<R, W>(&mut self, input: R, output: W) -> io::Result<()>
    where
        R: Read,
        W: Write + Send + 'static,
    {
        // Create a shared writer to prevent interleaving between the main loop
        // and the event handler thread.
        let shared_writer: Arc<Mutex<W>> = Arc::new(Mutex::new(output));
        let event_writer = Arc::clone(&shared_writer);

        // Create channel for asynchronous events.
        let (tx, rx) = channel::<DapMessage>();
        self.event_sender = Some(tx.clone());

        thread::spawn(move || {
            while let Ok(first_msg) = rx.recv() {
                let mut batch = Vec::with_capacity(EVENT_WRITE_BATCH_MAX);
                batch.push(first_msg);

                let mut disconnected = false;
                while batch.len() < EVENT_WRITE_BATCH_MAX {
                    match rx.try_recv() {
                        Ok(msg) => batch.push(msg),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            disconnected = true;
                            break;
                        }
                    }
                }

                let mut payloads = Vec::with_capacity(batch.len());
                for msg in batch {
                    match serde_json::to_vec(&msg) {
                        Ok(payload) => payloads.push(payload),
                        Err(e) => {
                            tracing::error!(
                                error = %e,
                                message = ?msg,
                                "Failed to serialize DAP message"
                            );
                        }
                    }
                }

                if payloads.is_empty() {
                    if disconnected {
                        break;
                    }
                    continue;
                }

                let mut writer = lock_or_recover(&event_writer, "event_writer");
                let mut write_failed = false;
                for payload in &payloads {
                    if let Err(e) = write_framed_payload(&mut *writer, payload) {
                        tracing::error!(error = %e, "Failed to write DAP frame in event handler");
                        write_failed = true;
                        break;
                    }
                }
                if !write_failed && let Err(e) = writer.flush() {
                    tracing::error!(error = %e, "Failed to flush DAP frame in event handler");
                }

                if disconnected {
                    break;
                }
            }
            tracing::debug!("Event handler thread terminating - channel closed");
        });

        let mut reader = BufReader::new(input);
        let mut framer = ContentLengthFramer::new();
        let mut read_buf = [0u8; 8 * 1024];

        loop {
            let bytes_read = reader.read(&mut read_buf)?;
            if bytes_read == 0 {
                return Ok(());
            }

            framer.push(&read_buf[..bytes_read]);

            loop {
                let body = match framer.try_next() {
                    Ok(Some(body)) => body,
                    Ok(None) => break,
                    Err(error) => {
                        tracing::warn!(%error, "Failed to parse DAP transport frame");
                        continue;
                    }
                };

                let msg = match serde_json::from_slice::<DapMessage>(&body) {
                    Ok(msg) => msg,
                    Err(_) => {
                        tracing::warn!(body = %String::from_utf8_lossy(&body), "Failed to parse DAP message");
                        continue;
                    }
                };

                let DapMessage::Request { seq, command, arguments } = msg else {
                    continue;
                };

                let response = self.dispatch_request(seq, &command, arguments);
                let payload = match serde_json::to_vec(&response) {
                    Ok(payload) => payload,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to serialize DAP response");
                        continue;
                    }
                };

                let mut writer = lock_or_recover(&shared_writer, "response_writer");
                write_framed_payload(&mut *writer, &payload)?;
                writer.flush()?;

                // DAP requires this event only after initialize response is sent.
                if command == "initialize"
                    && Self::response_succeeded_for_command(&response, "initialize")
                {
                    self.send_event("initialized", None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Stdio transport framing edge-case coverage for `run_with_io`.
    //!
    //! Each test drives the transport loop with an in-memory `std::io::Cursor`
    //! reader and a shared `Arc<Mutex<Vec<u8>>>` writer, asserting NO PANIC and
    //! graceful recovery or clean shutdown on EOF. These live in-crate (rather
    //! than in `tests/`) because `run_with_io` is `pub(super)` — keeping the
    //! transport loop crate-private while still covering its framing seam, with
    //! no production test-only shim.
    //!
    //! Tested at the loop level: behaviour (no panic, skip or continue), not
    //! internal framing error types.

    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    // ── shared output buffer ──────────────────────────────────────────────────

    /// A clonable, thread-safe write buffer satisfying `Write + Send + 'static`.
    ///
    /// Because `run_with_io` requires `W: Write + Send + 'static` (the writer is
    /// moved into the event-handler thread), we cannot pass a `&mut Vec<u8>`.
    /// Instead we wrap the buffer in `Arc<Mutex<_>>` and implement `Write` on the
    /// wrapper so we can inspect the bytes after the loop completes.
    #[derive(Clone, Default)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl SharedBuf {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(Vec::new())))
        }

        fn bytes_snapshot(&self) -> Vec<u8> {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }
    }

    impl io::Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
            guard.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    // ── helpers ────────────────────────────────────────────────────────────────

    /// Build a well-formed Content-Length framed DAP request.
    fn framed_request(seq: i64, command: &str, arguments: Option<serde_json::Value>) -> Vec<u8> {
        let args_part = match arguments {
            Some(v) => v.to_string(),
            None => "null".to_string(),
        };
        let body = format!(
            r#"{{"type":"request","seq":{seq},"command":"{command}","arguments":{args_part}}}"#
        );
        let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        frame.extend_from_slice(body.as_bytes());
        frame
    }

    /// Build a Content-Length framed payload from arbitrary bytes (may not be valid JSON).
    fn framed_raw(body: &[u8]) -> Vec<u8> {
        let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        frame.extend_from_slice(body);
        frame
    }

    /// Extract the body bytes of the first framed message from `buf`.
    ///
    /// The event-handler thread may append an "initialized" event to the shared
    /// writer *after* `run_with_io` returns (the thread is not joined before the
    /// function exits). Parsing `written[separator..end]` as JSON therefore fails
    /// non-deterministically because the slice contains the initialize response
    /// JSON followed by a second Content-Length frame.
    ///
    /// This helper parses the `Content-Length` header of the first frame and
    /// returns only those bytes, avoiding the race.
    fn first_frame_body(buf: &[u8]) -> io::Result<&[u8]> {
        // Locate the header/body separator.
        let sep_pos = buf.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "no CRLF separator in output")
        })?;
        let header = &buf[..sep_pos];
        let body_start = sep_pos + 4;

        // Parse Content-Length from the header.
        let cl_prefix = b"Content-Length: ";
        let cl_start =
            header.windows(cl_prefix.len()).position(|w| w == cl_prefix).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "no Content-Length in header")
            })? + cl_prefix.len();
        let cl_end = header[cl_start..]
            .iter()
            .position(|&b| b == b'\r' || b == b'\n')
            .map(|p| cl_start + p)
            .unwrap_or(header.len());
        let cl_str = std::str::from_utf8(&header[cl_start..cl_end])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let cl: usize = cl_str.trim().parse().map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("bad Content-Length: {e}"))
        })?;

        let body_end = body_start + cl;
        buf.get(body_start..body_end).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "buffer truncated before body end")
        })
    }

    // ── 1. Missing Content-Length header ──────────────────────────────────────

    #[test]
    fn test_transport_missing_content_length_header_no_panic() -> io::Result<()> {
        // A header block with no Content-Length, then EOF.
        // The framer skips the malformed header; the loop exits cleanly on EOF.
        let input = b"X-Custom: foo\r\n\r\n".to_vec();
        let mut adapter = DebugAdapter::new();
        let result = adapter.run_with_io(Cursor::new(input), SharedBuf::new());
        match result {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            Err(e) => return Err(e),
        }
        Ok(())
    }

    // ── 2. Non-numeric Content-Length ──────────────────────────────────────────

    #[test]
    fn test_transport_non_numeric_content_length_no_panic() -> io::Result<()> {
        let input = b"Content-Length: notanumber\r\n\r\n".to_vec();
        let mut adapter = DebugAdapter::new();
        let result = adapter.run_with_io(Cursor::new(input), SharedBuf::new());
        // Any outcome (Ok or Err) is acceptable; we only assert no panic.
        let _ = result;
        Ok(())
    }

    // ── 3. Negative Content-Length ─────────────────────────────────────────────

    #[test]
    fn test_transport_negative_content_length_no_panic() -> io::Result<()> {
        // "-1" cannot parse as usize → InvalidContentLength framing error.
        let input = b"Content-Length: -1\r\n\r\n".to_vec();
        let mut adapter = DebugAdapter::new();
        let result = adapter.run_with_io(Cursor::new(input), SharedBuf::new());
        let _ = result;
        Ok(())
    }

    // ── 4. Valid header + malformed JSON body ──────────────────────────────────

    #[test]
    fn test_transport_valid_header_malformed_json_no_panic() -> io::Result<()> {
        // Correct Content-Length but the body is not valid JSON.
        // The loop logs a warning and reaches EOF cleanly.
        let bad_body = b"this is not json at all!";
        let input = framed_raw(bad_body);
        let mut adapter = DebugAdapter::new();
        let result = adapter.run_with_io(Cursor::new(input), SharedBuf::new());
        let _ = result;
        Ok(())
    }

    // ── 5. Multiple messages in one buffer (both processed) ───────────────────

    #[test]
    fn test_transport_two_messages_in_one_buffer_no_panic() -> io::Result<()> {
        // Two well-formed requests back-to-back in one Cursor.
        // Both must be dispatched without panic; the loop exits on EOF.
        let mut input =
            framed_request(1, "initialize", Some(json!({"clientID": "test", "adapterID": "perl"})));
        input.extend(framed_request(2, "disconnect", None));

        let output = SharedBuf::new();
        let mut adapter = DebugAdapter::new();
        adapter.run_with_io(Cursor::new(input), output.clone())?;

        let written = output.bytes_snapshot();
        assert!(
            written.starts_with(b"Content-Length:"),
            "expected at least one framed response in output, got {} bytes",
            written.len()
        );
        Ok(())
    }

    // ── 6. EOF mid-header ──────────────────────────────────────────────────────

    #[test]
    fn test_transport_eof_mid_header_no_panic() -> io::Result<()> {
        // Partial header, then EOF. The framer never sees a complete frame.
        // The outer read loop exits cleanly on EOF (bytes_read == 0).
        let input = b"Content-Leng".to_vec();
        let mut adapter = DebugAdapter::new();
        let result = adapter.run_with_io(Cursor::new(input), SharedBuf::new());
        match result {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            Err(e) => return Err(e),
        }
        Ok(())
    }

    // ── 7. EOF mid-body ────────────────────────────────────────────────────────

    #[test]
    fn test_transport_eof_mid_body_no_panic() -> io::Result<()> {
        // Header claims 1000 bytes but only 5 bytes are provided before EOF.
        let input = b"Content-Length: 1000\r\n\r\nshort".to_vec();
        let mut adapter = DebugAdapter::new();
        let result = adapter.run_with_io(Cursor::new(input), SharedBuf::new());
        match result {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            Err(e) => return Err(e),
        }
        Ok(())
    }

    // ── 8. Extra / duplicate headers ───────────────────────────────────────────

    #[test]
    fn test_transport_extra_headers_no_panic() -> io::Result<()> {
        // A header block with extra headers preceding Content-Length.
        // The framer should still extract Content-Length and process the body.
        let body_str = r#"{"type":"request","seq":1,"command":"initialize","arguments":{"clientID":"test","adapterID":"perl"}}"#;
        let mut input = format!(
            "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
            body_str.len()
        )
        .into_bytes();
        input.extend_from_slice(body_str.as_bytes());

        let output = SharedBuf::new();
        let mut adapter = DebugAdapter::new();
        adapter.run_with_io(Cursor::new(input), output.clone())?;

        let written = output.bytes_snapshot();
        assert!(!written.is_empty(), "expected a response to the initialize request");
        Ok(())
    }

    // ── 9. LF-only separator instead of CRLF ──────────────────────────────────

    #[test]
    fn test_transport_lf_only_separator_no_panic() -> io::Result<()> {
        // Some clients send `\n\n` instead of `\r\n\r\n`. The framer supports both.
        let body_str = r#"{"type":"request","seq":1,"command":"initialize","arguments":null}"#;
        let mut input = format!("Content-Length: {}\n\n", body_str.len()).into_bytes();
        input.extend_from_slice(body_str.as_bytes());

        let mut adapter = DebugAdapter::new();
        // Must complete without panic.
        adapter.run_with_io(Cursor::new(input), SharedBuf::new())?;
        Ok(())
    }

    // ── 10. Malformed frame followed by well-formed one (recovery) ────────────

    #[test]
    fn test_transport_recovers_after_malformed_frame() -> io::Result<()> {
        // A bad (non-JSON) frame immediately followed by a well-formed initialize.
        // The well-formed request must still be dispatched even after the skip.
        let bad_body = b"not-json!!!";
        let mut input = framed_raw(bad_body);
        input.extend(framed_request(
            1,
            "initialize",
            Some(json!({"clientID": "test", "adapterID": "perl"})),
        ));

        let output = SharedBuf::new();
        let mut adapter = DebugAdapter::new();
        adapter.run_with_io(Cursor::new(input), output.clone())?;

        let written = output.bytes_snapshot();

        // The output must contain a response for the initialize request.
        assert!(
            written.starts_with(b"Content-Length:"),
            "expected a framed response after recovery from malformed frame, got {} bytes",
            written.len()
        );

        // Verify the response is for the initialize command. Use first_frame_body()
        // to avoid a race where the event-handler thread appends the "initialized"
        // event after run_with_io returns, which would make serde_json::from_slice
        // fail on trailing bytes.
        let body = first_frame_body(&written)?;
        let parsed: serde_json::Value = serde_json::from_slice(body)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        assert_eq!(parsed["command"], "initialize", "response must be for initialize");
        assert_eq!(parsed["type"], "response", "must be a response message");
        Ok(())
    }

    // ── 11. Empty input (immediate EOF) ───────────────────────────────────────

    #[test]
    fn test_transport_empty_input_clean_shutdown() -> io::Result<()> {
        let mut adapter = DebugAdapter::new();
        // Must return Ok(()) immediately on empty input.
        adapter.run_with_io(Cursor::new(Vec::<u8>::new()), SharedBuf::new())?;
        Ok(())
    }
}
