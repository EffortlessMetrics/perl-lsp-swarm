//! Tests for window/* and telemetry/event LSP features

use parking_lot::Mutex;
use perl_lsp::{
    LspServer,
    server::{MessageType, ShowDocumentOptions},
};
use serde_json::{Value, json};
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Capture output for testing
struct OutputCapture {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl OutputCapture {
    fn new() -> Self {
        Self { buffer: Arc::new(Mutex::new(Vec::new())) }
    }

    /// Parse the captured stream as LSP frames, honoring each
    /// `Content-Length` header. Splitting on blank lines instead would glue a
    /// frame's body to the next frame's header (frames are written
    /// back-to-back with no separator) and silently drop every message except
    /// the last one in a batch.
    fn get_messages(&self) -> Vec<Value> {
        let buffer = self.buffer.lock();
        let mut messages = Vec::new();
        let mut rest: &[u8] = &buffer;
        while let Some(header_end) = rest.windows(4).position(|w| w == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&rest[..header_end]);
            let Some(len) = header
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
            else {
                break;
            };
            let body_start = header_end + 4;
            let Some(body) = rest.get(body_start..body_start + len) else {
                break;
            };
            if let Ok(msg) = serde_json::from_slice::<Value>(body) {
                messages.push(msg);
            }
            rest = &rest[body_start + len..];
        }
        messages
    }

    fn clear(&self) {
        self.buffer.lock().clear();
    }
}

fn lsp_frame(body: &[u8]) -> Vec<u8> {
    let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    frame.extend_from_slice(body);
    frame
}

#[test]
fn output_capture_parses_two_coalesced_frames() {
    let output = OutputCapture::new();
    let mut writer = output.clone();
    writer
        .write_all(
            &[
                lsp_frame(br#"{"id":1,"result":"first"}"#),
                lsp_frame(br#"{"id":2,"result":"second"}"#),
            ]
            .concat(),
        )
        .expect("capture accepts coalesced frames");

    assert_eq!(
        output.get_messages(),
        vec![json!({"id": 1, "result": "first"}), json!({"id": 2, "result": "second"}),]
    );
}

#[test]
fn output_capture_waits_for_split_frame() {
    let output = OutputCapture::new();
    let frame = lsp_frame(br#"{"id":3,"result":"split"}"#);
    let split = frame.len() / 2;
    let mut writer = output.clone();
    writer.write_all(&frame[..split]).expect("capture accepts frame prefix");
    assert!(output.get_messages().is_empty(), "truncated frame must not parse");

    writer.write_all(&frame[split..]).expect("capture accepts frame suffix");
    assert_eq!(output.get_messages(), vec![json!({"id": 3, "result": "split"})]);
}

#[test]
fn output_capture_preserves_crlf_in_frame_body() {
    let output = OutputCapture::new();
    let mut writer = output.clone();
    let body = b"{\r\n\"id\":4,\r\n\"result\":\"body\"\r\n}\r\n\r\n";
    writer.write_all(&lsp_frame(body)).expect("capture accepts CRLF body");

    assert_eq!(output.get_messages(), vec![json!({"id": 4, "result": "body"})]);
}

#[test]
fn output_capture_rejects_malformed_and_truncated_lengths() {
    let output = OutputCapture::new();
    let mut writer = output.clone();
    writer.write_all(b"Content-Length: nope\r\n\r\n{}").expect("capture accepts malformed frame");
    assert!(output.get_messages().is_empty(), "malformed length must not parse");

    output.clear();
    let body = br#"{"id":5,"result":"truncated"}"#;
    let frame = lsp_frame(body);
    writer.write_all(&frame[..frame.len() - 1]).expect("capture accepts truncated frame");
    assert!(output.get_messages().is_empty(), "truncated body must not parse");

    writer.write_all(&frame[frame.len() - 1..]).expect("capture accepts final byte");
    assert_eq!(output.get_messages(), vec![json!({"id": 5, "result": "truncated"})]);
}

fn wait_for_messages(output: &OutputCapture, minimum_count: usize) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        let messages = output.get_messages();
        if messages.len() >= minimum_count {
            return messages;
        }
        if Instant::now() >= deadline {
            return messages;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_method(output: &OutputCapture, method: &str) -> Option<Value> {
    // Returns as soon as the method arrives; the long deadline only bounds the
    // failure path so outbound writer scheduling under parallel test load
    // (#13492) cannot flake an otherwise-delivered message.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let messages = output.get_messages();
        if let Some(message) =
            messages.into_iter().find(|message| message["method"].as_str() == Some(method))
        {
            return Some(message);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Wait for a `$/progress` notification for a specific token and kind. The
/// server emits its own `$/progress` traffic (e.g. indexing progress after
/// initialization), so matching on method alone can select a server-owned
/// progress notification instead of the one the test drives.
fn wait_for_progress(output: &OutputCapture, token: &str, kind: &str) -> Option<Value> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let messages = output.get_messages();
        if let Some(found) = messages.into_iter().find(|m| {
            m["method"].as_str() == Some("$/progress")
                && m["params"]["token"].as_str() == Some(token)
                && m["params"]["value"]["kind"].as_str() == Some(kind)
        }) {
            return Some(found);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Wait for a specific notification, discriminated by `params.message`. The
/// server emits its own `window/logMessage` traffic during initialization, so
/// matching on method alone can select a server log instead of the one the
/// test just sent.
fn wait_for_notification_message(
    output: &OutputCapture,
    method: &str,
    message: &str,
) -> Option<Value> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let messages = output.get_messages();
        if let Some(found) = messages.into_iter().find(|m| {
            m["method"].as_str() == Some(method) && m["params"]["message"].as_str() == Some(message)
        }) {
            return Some(found);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Complete the LSP lifecycle handshake after `initialize` (#13492).
///
/// Server-to-client requests (`window/showMessageRequest`, `window/showDocument`,
/// `window/workDoneProgress/create`, ...) are rejected with `WouldBlock` until
/// initialization completes: `LspServer::send_request` guards the common seam
/// because LSP 3.17 forbids server-originated requests before the handshake
/// finishes (#7708; a35d535023, ff7ac8a084, f3d86f5514). Tests that drive those
/// APIs must deliver the `initialized` notification first, exactly like real
/// clients do.
fn complete_initialization(server: &LspServer) {
    let response = server.handle_request(perl_lsp::JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "initialized".to_string(),
        params: Some(json!({})),
    });
    assert!(
        response.is_none(),
        "initialized notification must not produce a response: {response:?}"
    );
    assert!(
        server.is_initialized(),
        "initialized notification must be accepted during the handshake"
    );
}

fn initialize_for_window_test(server: &LspServer, init_params: Value) {
    let response = server.handle_request(perl_lsp::JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".to_string(),
        params: Some(init_params),
    });
    assert!(
        matches!(&response, Some(response) if response.error.is_none()),
        "initialize request must succeed: {response:?}"
    );
    complete_initialization(server);
}

impl Write for OutputCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.lock().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.buffer.lock().flush()
    }
}

impl Clone for OutputCapture {
    fn clone(&self) -> Self {
        Self { buffer: Arc::clone(&self.buffer) }
    }
}

#[test]
fn lsp_window_show_message_request_format() -> Result<(), Box<dyn std::error::Error>> {
    let output = OutputCapture::new();
    let output_box: Box<dyn Write + Send> = Box::new(output.clone());
    let server = LspServer::with_output(Arc::new(Mutex::new(output_box)));

    // Initialize server to enable capabilities
    let init_params = json!({
        "capabilities": {
            "window": {
                "showDocument": {
                    "support": true
                },
                "workDoneProgress": true
            }
        }
    });

    initialize_for_window_test(&server, init_params);

    let _ = wait_for_messages(&output, 1);
    output.clear();

    // Send showMessageRequest
    let result = server.show_message_request(
        MessageType::Warning,
        "Do you want to continue?",
        vec!["Yes", "No"],
    );

    // Verify request was sent
    assert!(result.is_ok());

    let request = wait_for_method(&output, "window/showMessageRequest")
        .ok_or("Expected showMessageRequest to be sent")?;
    assert_eq!(request["jsonrpc"], "2.0");
    assert_eq!(request["method"], "window/showMessageRequest");
    assert_eq!(request["params"]["type"], 2); // Warning = 2
    assert_eq!(request["params"]["message"], "Do you want to continue?");

    let actions = request["params"]["actions"].as_array().ok_or("Expected actions array")?;
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0]["title"], "Yes");
    assert_eq!(actions[1]["title"], "No");

    Ok(())
}

#[test]
fn lsp_window_show_document_requires_capability() -> Result<(), Box<dyn std::error::Error>> {
    let output = OutputCapture::new();
    let output_box: Box<dyn Write + Send> = Box::new(output.clone());
    let server = LspServer::with_output(Arc::new(Mutex::new(output_box)));

    // Try to show document without capability
    let result =
        server.show_document("file:///test.pl", ShowDocumentOptions { ..Default::default() });

    // Should fail with Unsupported error
    assert!(result.is_err());
    let err = result.err().ok_or("Expected error result")?;
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    assert!(err.to_string().contains("doesn't support"));

    Ok(())
}

#[test]
fn lsp_window_show_document_with_capability() {
    let output = OutputCapture::new();
    let output_box: Box<dyn Write + Send> = Box::new(output.clone());
    let server = LspServer::with_output(Arc::new(Mutex::new(output_box)));

    // Initialize with showDocument capability
    let init_params = json!({
        "capabilities": {
            "window": {
                "showDocument": {
                    "support": true
                }
            }
        }
    });

    initialize_for_window_test(&server, init_params);

    let _ = wait_for_messages(&output, 1);
    output.clear();

    // Send showDocument with options
    let options = ShowDocumentOptions {
        external: false,
        take_focus: true,
        selection: Some(lsp_types::Range {
            start: lsp_types::Position { line: 10, character: 5 },
            end: lsp_types::Position { line: 10, character: 15 },
        }),
    };

    let result = server.show_document("file:///test.pl", options);
    assert!(result.is_ok());

    let request = wait_for_method(&output, "window/showDocument");
    assert!(request.is_some(), "Expected window/showDocument request");
    let request = request.unwrap_or_else(|| unreachable!());
    assert_eq!(request["method"], "window/showDocument");
    assert_eq!(request["params"]["uri"], "file:///test.pl");
    assert_eq!(request["params"]["takeFocus"], true);
    assert!(request["params"]["selection"].is_object());
}

#[test]
fn lsp_window_progress_lifecycle() {
    let output = OutputCapture::new();
    let output_box: Box<dyn Write + Send> = Box::new(output.clone());
    let server = LspServer::with_output(Arc::new(Mutex::new(output_box)));

    // Initialize with workDoneProgress capability
    let init_params = json!({
        "capabilities": {
            "window": {
                "workDoneProgress": true
            }
        }
    });

    initialize_for_window_test(&server, init_params);

    let _ = wait_for_messages(&output, 1);
    output.clear();

    // Create progress token
    let token = "test-progress-1";
    let result = server.create_work_done_progress(token);
    assert!(result.is_ok(), "Failed to create progress: {:?}", result);

    let create_message = wait_for_method(&output, "window/workDoneProgress/create");
    assert!(create_message.is_some(), "Expected window/workDoneProgress/create request");
    let create_message = create_message.unwrap_or_else(|| unreachable!());
    assert_eq!(create_message["method"], "window/workDoneProgress/create");
    assert_eq!(create_message["params"]["token"], token);

    output.clear();

    // Report progress begin
    let result = server.report_progress_begin(token, "Indexing", Some("Starting..."));
    assert!(result.is_ok());

    let begin_message = wait_for_progress(&output, token, "begin");
    assert!(begin_message.is_some(), "Expected begin $/progress notification");
    let begin_message = begin_message.unwrap_or_else(|| unreachable!());
    assert_eq!(begin_message["params"]["token"], token);
    assert_eq!(begin_message["params"]["value"]["title"], "Indexing");
    assert_eq!(begin_message["params"]["value"]["message"], "Starting...");

    output.clear();

    // Report progress update
    let result = server.report_progress_report(token, Some("50% complete"), Some(50));
    assert!(result.is_ok());

    let report_message = wait_for_progress(&output, token, "report");
    assert!(report_message.is_some(), "Expected report $/progress notification");
    let report_message = report_message.unwrap_or_else(|| unreachable!());
    assert_eq!(report_message["params"]["value"]["percentage"], 50);

    output.clear();

    // Report progress end
    let result = server.report_progress_end(token, Some("Complete"));
    assert!(result.is_ok());

    let end_message = wait_for_progress(&output, token, "end");
    assert!(end_message.is_some(), "Expected end $/progress notification");
    let end_message = end_message.unwrap_or_else(|| unreachable!());
    assert_eq!(end_message["params"]["value"]["message"], "Complete");
}

#[test]
fn lsp_window_progress_duplicate_token_fails() -> Result<(), Box<dyn std::error::Error>> {
    let output = OutputCapture::new();
    let output_box: Box<dyn Write + Send> = Box::new(output.clone());
    let server = LspServer::with_output(Arc::new(Mutex::new(output_box)));

    // Initialize with workDoneProgress capability
    let init_params = json!({
        "capabilities": {
            "window": {
                "workDoneProgress": true
            }
        }
    });

    initialize_for_window_test(&server, init_params);

    // Create first token
    let token = "duplicate-token";
    let result = server.create_work_done_progress(token);
    assert!(result.is_ok());

    // Try to create same token again
    let result = server.create_work_done_progress(token);
    assert!(result.is_err());
    let err = result.err().ok_or("Expected error result")?;
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);

    Ok(())
}

#[test]
fn lsp_window_progress_cancel_handler() {
    let output = OutputCapture::new();
    let output_box: Box<dyn Write + Send> = Box::new(output.clone());
    let server = LspServer::with_output(Arc::new(Mutex::new(output_box)));

    // Initialize with workDoneProgress capability
    let init_params = json!({
        "capabilities": {
            "window": {
                "workDoneProgress": true
            }
        }
    });

    let _ = server.handle_request(perl_lsp::JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".to_string(),
        params: Some(init_params),
    });

    // Send initialized notification
    let _ = server.handle_request(perl_lsp::JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "initialized".to_string(),
        params: Some(json!({})),
    });

    // Create progress token
    let token = "cancellable-progress";
    let result = server.create_work_done_progress(token);
    assert!(result.is_ok());

    // Send cancel notification
    let cancel_response = server.handle_request(perl_lsp::JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None, // Notification
        method: "window/workDoneProgress/cancel".to_string(),
        params: Some(json!({ "token": token })),
    });

    // Notification returns None (no response for notifications)
    assert!(cancel_response.is_none());
}

#[test]
fn lsp_window_telemetry_respects_config() {
    let output = OutputCapture::new();
    let output_box: Box<dyn Write + Send> = Box::new(output.clone());
    let server = LspServer::with_output(Arc::new(Mutex::new(output_box)));

    // Initialize server first
    let _ = server.handle_request(perl_lsp::JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(1_i64)),
        method: "initialize".to_string(),
        params: Some(json!({})),
    });

    let _ = wait_for_messages(&output, 1);
    let _ = server.handle_request(perl_lsp::JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None,
        method: "initialized".to_string(),
        params: Some(json!({})),
    });

    output.clear();

    // Telemetry is disabled by default
    let event = json!({
        "event": "test",
        "data": { "value": 123 }
    });

    let result = server.send_telemetry(event.clone());
    assert!(result.is_ok());

    // No telemetry/event notification should be sent while disabled.
    std::thread::sleep(Duration::from_millis(50));
    let messages = output.get_messages();
    let telemetry_sent =
        messages.iter().any(|message| message["method"].as_str() == Some("telemetry/event"));
    assert!(!telemetry_sent, "Telemetry sent when disabled");

    output.clear();

    // Enable telemetry via configuration using didChangeConfiguration
    let config_response = server.handle_request(perl_lsp::JsonRpcRequest {
        _jsonrpc: "2.0".to_string(),
        id: None, // Notification
        method: "workspace/didChangeConfiguration".to_string(),
        params: Some(json!({
            "settings": {
                "perl": {
                    "telemetry": {
                        "enabled": true
                    }
                }
            }
        })),
    });
    // Notification returns None
    assert!(config_response.is_none());

    let result = server.send_telemetry(event);
    assert!(result.is_ok());

    // Telemetry should now be sent
    let telemetry_message = wait_for_method(&output, "telemetry/event");
    assert!(telemetry_message.is_some(), "Expected telemetry/event notification");
    let telemetry_message = telemetry_message.unwrap_or_else(|| unreachable!());
    assert_eq!(telemetry_message["method"], "telemetry/event");
    assert_eq!(telemetry_message["params"]["event"], "test");
}

#[test]
fn lsp_window_message_types() {
    let output = OutputCapture::new();
    let output_box: Box<dyn Write + Send> = Box::new(output.clone());
    let server = LspServer::with_output(Arc::new(Mutex::new(output_box)));

    // Server-to-client requests are deferred until the handshake completes
    // (#7708), so complete initialization before driving showMessageRequest.
    initialize_for_window_test(&server, json!({ "capabilities": {} }));

    // Test all message types
    let types = [
        (MessageType::Error, 1),
        (MessageType::Warning, 2),
        (MessageType::Info, 3),
        (MessageType::Log, 4),
        (MessageType::Debug, 5),
    ];

    for (msg_type, expected_value) in types {
        output.clear();

        let sent = server.show_message_request(msg_type, "Test message", vec![]);
        assert!(sent.is_ok(), "show_message_request({msg_type:?}) must send: {sent:?}");

        let message = wait_for_method(&output, "window/showMessageRequest");
        assert!(message.is_some(), "Expected window/showMessageRequest");
        let message = message.unwrap_or_else(|| unreachable!());
        assert_eq!(message["params"]["type"], expected_value);
    }
}

#[test]
fn lsp_window_debug_message_type_serializes_to_five() -> Result<(), Box<dyn std::error::Error>> {
    let output = OutputCapture::new();
    let output_box: Box<dyn Write + Send> = Box::new(output.clone());
    let server = LspServer::with_output(Arc::new(Mutex::new(output_box)));

    // The final leg sends a server-to-client window/showMessageRequest, which
    // is deferred until the handshake completes (#7708); initialize first.
    initialize_for_window_test(&server, json!({ "capabilities": {} }));

    server.log_message(MessageType::Debug, "debug log")?;
    let log_message = wait_for_notification_message(&output, "window/logMessage", "debug log")
        .ok_or("Expected window/logMessage debug notification")?;
    assert_eq!(log_message["params"]["type"], 5);
    assert_eq!(log_message["params"]["message"], "debug log");

    output.clear();

    server.show_message(MessageType::Debug, "debug show")?;
    let show_message = wait_for_method(&output, "window/showMessage")
        .ok_or("Expected window/showMessage debug notification")?;
    assert_eq!(show_message["params"]["type"], 5);
    assert_eq!(show_message["params"]["message"], "debug show");

    output.clear();

    server.show_message_request(MessageType::Debug, "debug request", vec!["Inspect"])?;
    let request = wait_for_method(&output, "window/showMessageRequest")
        .ok_or("Expected window/showMessageRequest debug request")?;
    assert_eq!(request["params"]["type"], 5);
    assert_eq!(request["params"]["message"], "debug request");
    assert_eq!(request["params"]["actions"][0]["title"], "Inspect");

    Ok(())
}

#[test]
fn lsp_window_progress_without_capability() -> Result<(), Box<dyn std::error::Error>> {
    let output = OutputCapture::new();
    let output_box: Box<dyn Write + Send> = Box::new(output.clone());
    let server = LspServer::with_output(Arc::new(Mutex::new(output_box)));

    // Try to create progress without capability
    let result = server.create_work_done_progress("test");

    assert!(result.is_err());
    let err = result.err().ok_or("Expected error result")?;
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    assert!(err.to_string().contains("doesn't support"));

    Ok(())
}

#[test]
fn lsp_window_show_document_external_flag() {
    let output = OutputCapture::new();
    let output_box: Box<dyn Write + Send> = Box::new(output.clone());
    let server = LspServer::with_output(Arc::new(Mutex::new(output_box)));

    // Initialize with showDocument capability
    let init_params = json!({
        "capabilities": {
            "window": {
                "showDocument": {
                    "support": true
                }
            }
        }
    });

    initialize_for_window_test(&server, init_params);

    let _ = wait_for_messages(&output, 1);
    output.clear();

    // Test external = true
    let options = ShowDocumentOptions { external: true, take_focus: false, selection: None };

    let _ = server.show_document("https://example.com", options);

    let message = wait_for_method(&output, "window/showDocument");
    assert!(message.is_some(), "Expected window/showDocument");
    let message = message.unwrap_or_else(|| unreachable!());
    assert_eq!(message["params"]["external"], true);
    assert_eq!(message["params"]["uri"], "https://example.com");
}
