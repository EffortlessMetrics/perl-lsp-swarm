//! Opt-in byte-range evidence; callers record while holding the actual stdin lock.

use serde_json::{Value, json};
use std::io::{self, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

const RECORD_LIMIT: usize = 256;

pub(super) struct WriteProbe {
    server_pid: u32,
    next_frame: AtomicU64,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    sequence: u64,
    offset: u64,
    omitted: u64,
    records: Vec<Value>,
}

pub(super) struct Frame {
    token: u64,
    metadata: Value,
}

impl WriteProbe {
    pub(super) fn new(server_pid: u32) -> Self {
        Self { server_pid, next_frame: AtomicU64::new(1), state: Mutex::new(State::default()) }
    }

    pub(super) fn frame(
        &self,
        origin: &'static str,
        message: Option<&Value>,
        body_bytes: Option<usize>,
    ) -> Frame {
        let method = match message.and_then(|message| message.get("method")).and_then(Value::as_str)
        {
            Some("textDocument/didOpen") => "did_open",
            Some("textDocument/documentSymbol") => "document_symbol",
            _ if origin == "auto_answer" => "client_response",
            _ => "other",
        };
        let document_message = matches!(method, "did_open" | "document_symbol");
        let id = message.and_then(|message| message.get("id"));
        Frame {
            token: self.next_frame.fetch_add(1, Ordering::Relaxed),
            metadata: json!({
                "origin": origin,
                "method_class": method,
                "body_bytes": body_bytes,
                "numeric_id": id.and_then(Value::as_i64),
                "id_observed": message.is_some(),
                "id_omitted": message.is_none() || id.is_some_and(|id| !id.is_i64()),
                "uri_bytes": document_message.then(|| message.and_then(|message| message.pointer("/params/textDocument/uri"))
                    .and_then(Value::as_str).map(str::len)).flatten(),
                "text_bytes": document_message.then(|| message.and_then(|message| message.pointer("/params/textDocument/text"))
                    .and_then(Value::as_str).map(str::len)).flatten(),
                "version": document_message.then(|| message.and_then(|message| message.pointer("/params/textDocument/version"))
                    .and_then(Value::as_i64)).flatten(),
            }),
        }
    }

    pub(super) fn record(&self, frame: &Frame, phase: &'static str, result: &io::Result<usize>) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.sequence += 1;
        let start = state.offset;
        let accepted = result.as_ref().copied().unwrap_or(0);
        state.offset = state.offset.saturating_add(accepted as u64);
        if state.records.len() == RECORD_LIMIT {
            state.omitted += 1;
            return;
        }
        let sequence = state.sequence;
        let end = state.offset;
        state.records.push(json!({
            "server_pid": self.server_pid,
            "write_sequence": sequence,
            "frame_token": frame.token,
            "phase": phase,
            "stream_start": start,
            "stream_end": end,
            "accepted_bytes": accepted,
            "outcome": if result.is_ok() { "ok" } else { "io_error" },
            "io_error_kind": result.as_ref().err().map(|error| format!("{:?}", error.kind())),
            "frame": frame.metadata,
        }));
    }

    pub(super) fn snapshot(&self) -> Value {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        json!({ "records": state.records, "enabled": true, "record_limit": RECORD_LIMIT,
            "omitted_records": state.omitted, "complete": state.omitted == 0 })
    }
}

pub(super) struct ObservedWriter<'a, W> {
    pub(super) writer: &'a mut W,
    pub(super) probe: Option<&'a WriteProbe>,
    pub(super) frame: Option<&'a Frame>,
    pub(super) phase: &'static str,
}

impl<W: Write> Write for ObservedWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let result = self.writer.write(bytes);
        if let (Some(probe), Some(frame)) = (self.probe, self.frame) {
            probe.record(frame, self.phase, &result);
        }
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        let result = self.writer.flush();
        if let (Some(probe), Some(frame)) = (self.probe, self.frame) {
            probe.record(
                frame,
                "flush",
                &result.as_ref().map(|_| 0).map_err(|error| io::Error::from(error.kind())),
            );
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_partial_writes_keep_ranges_and_original_flush_error() -> io::Result<()> {
        struct ShortWriter {
            bytes: Vec<u8>,
        }
        impl Write for ShortWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.bytes.len() == 7 {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "synthetic-write-secret",
                    ));
                }
                let accepted = bytes.len().min(2);
                self.bytes.extend_from_slice(&bytes[..accepted]);
                Ok(accepted)
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::other("synthetic-flush-secret"))
            }
        }
        let probe = WriteProbe::new(55);
        let frame =
            probe.frame("foreground", Some(&json!({ "id": "synthetic-id-secret" })), Some(4));
        let mut target = ShortWriter { bytes: Vec::new() };
        {
            let mut writer = ObservedWriter {
                writer: &mut target,
                probe: Some(&probe),
                frame: Some(&frame),
                phase: "header",
            };
            writer.write_all(b"HDR")?;
            writer.phase = "body";
            writer.write_all(b"BODY")?;
            let error = writer
                .write(b"FAIL")
                .err()
                .ok_or_else(|| io::Error::other("expected write failure"))?;
            assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
            assert_eq!(error.to_string(), "synthetic-write-secret");
            let error =
                writer.flush().err().ok_or_else(|| io::Error::other("expected flush failure"))?;
            assert_eq!(error.to_string(), "synthetic-flush-secret");
        }
        assert_eq!(target.bytes, b"HDRBODY");
        let receipt = probe.snapshot();
        assert_eq!(receipt["records"].as_array().map(Vec::len), Some(6));
        let expected = [(0, 2), (2, 3), (3, 5), (5, 7), (7, 7), (7, 7)];
        for (index, (start, end)) in expected.into_iter().enumerate() {
            assert_eq!(receipt["records"][index]["stream_start"], start);
            assert_eq!(receipt["records"][index]["stream_end"], end);
        }
        assert_eq!(receipt["records"][4]["io_error_kind"], "BrokenPipe");
        assert_eq!(receipt["records"][5]["io_error_kind"], "Other");
        assert!(!receipt.to_string().contains("synthetic-write-secret"));
        assert!(!receipt.to_string().contains("synthetic-flush-secret"));
        assert!(!receipt.to_string().contains("synthetic-id-secret"));
        Ok(())
    }

    #[test]
    fn write_probe_omits_strings_and_marks_overflow() {
        let probe = WriteProbe::new(55);
        let secret = format!("synthetic-id-secret{}", "x".repeat(8192));
        let frame = probe.frame(
            "auto_answer",
            Some(&json!({ "id": secret, "method": "synthetic-method-secret" })),
            Some(9000),
        );
        for _ in 0..RECORD_LIMIT + 1 {
            probe.record(&frame, "body", &Ok(3));
        }
        let receipt = probe.snapshot();
        assert_eq!(receipt["records"].as_array().map(Vec::len), Some(RECORD_LIMIT));
        assert_eq!(receipt["complete"], false);
        assert_eq!(receipt["omitted_records"], 1);
        assert_eq!(receipt["records"][0]["stream_start"], 0);
        assert_eq!(receipt["records"][0]["stream_end"], 3);
        assert_eq!(receipt["records"][1]["stream_start"], 3);
        let encoded = receipt.to_string();
        assert!(!encoded.contains("synthetic-id-secret"));
        assert!(!encoded.contains("synthetic-method-secret"));
        assert!(!encoded.contains(&"x".repeat(32)));
    }
}
