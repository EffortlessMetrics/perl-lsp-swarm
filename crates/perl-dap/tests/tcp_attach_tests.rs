//! TCP Attach Tests
//!
//! Comprehensive tests for TCP attach functionality in the DAP adapter.
//!
//! These tests validate:
//! - TCP connection establishment
//! - Message proxying between client and debugger
//! - Event handling and propagation
//! - Error recovery and timeout handling
//! - Cross-platform compatibility

use perl_dap::tcp_attach::{DapEvent, TcpAttachConfig, TcpAttachSession};
use perl_lsp_rs_core::transport::framing::frame;
use perl_tdd_support::must;
use std::io::Write;
use std::net::TcpListener;
use std::sync::mpsc::{channel, sync_channel};
use std::thread;
use std::time::Duration;

/// Capacity for the attach session's bounded fan-in queue (#9521). The test
/// event volumes here are tiny, so a small bounded queue suffices.
const TEST_ATTACH_EVENT_CAPACITY: usize = 8;

/// Test helper to create a valid TCP attach configuration
fn create_valid_config() -> TcpAttachConfig {
    TcpAttachConfig::new("127.0.0.1".to_string(), 13603)
}

#[test]
fn test_tcp_attach_config_validation() {
    // Test valid configuration
    let mut config = create_valid_config();
    assert!(config.validate().is_ok());

    // Test with timeout
    let mut config = create_valid_config().with_timeout(5000);
    assert!(config.validate().is_ok());

    // Test empty host
    let mut config = TcpAttachConfig::new("".to_string(), 13603);
    assert!(config.validate().is_err());

    // Test invalid port
    let mut config = TcpAttachConfig::new("localhost".to_string(), 0);
    assert!(config.validate().is_err());

    // Test zero timeout
    let mut config = create_valid_config().with_timeout(0);
    assert!(config.validate().is_err());

    // Test timeout too large
    let mut config = create_valid_config().with_timeout(300_001);
    assert!(config.validate().is_err());
}

#[test]
fn test_tcp_attach_timeout_duration() {
    // Test default timeout
    let config = create_valid_config();
    assert_eq!(config.timeout_duration(), Duration::from_secs(5));

    // Test custom timeout
    let config = create_valid_config().with_timeout(10000);
    assert_eq!(config.timeout_duration(), Duration::from_secs(10));
}

#[test]
fn test_tcp_attach_session_creation() {
    let session = TcpAttachSession::new();
    assert!(!session.is_connected());
}

#[test]
fn test_tcp_attach_session_event_sender() {
    let mut session = TcpAttachSession::new();
    let (tx, rx) = sync_channel::<DapEvent>(TEST_ATTACH_EVENT_CAPACITY);
    session.set_event_sender(tx.clone());
    // Send an event and verify it's received
    let event =
        DapEvent::Output { category: "stdout".to_string(), output: "test output".to_string() };
    must(tx.send(event));

    let received = must(rx.recv_timeout(Duration::from_millis(100)));
    match received {
        DapEvent::Output { category, output } => {
            assert_eq!(category, "stdout");
            assert_eq!(output, "test output");
        }
        _ => must(Err::<(), _>("Received unexpected event type")),
    }
}

#[test]
fn test_tcp_attach_event_variants() {
    // Test all event variants
    let (tx, rx) = channel::<DapEvent>();

    // Test Output event
    must(tx.send(DapEvent::Output { category: "stdout".to_string(), output: "test".to_string() }));
    if let DapEvent::Output { .. } = must(rx.recv_timeout(Duration::from_millis(100))) {
        // Success
    } else {
        must(Err::<(), _>("Expected Output event"));
    }

    // Test Stopped event
    must(tx.send(DapEvent::Stopped { reason: "breakpoint".to_string(), thread_id: 1 }));
    if let DapEvent::Stopped { .. } = must(rx.recv_timeout(Duration::from_millis(100))) {
        // Success
    } else {
        must(Err::<(), _>("Expected Stopped event"));
    }

    // Test Continued event
    must(tx.send(DapEvent::Continued { thread_id: 1 }));
    if let DapEvent::Continued { .. } = must(rx.recv_timeout(Duration::from_millis(100))) {
        // Success
    } else {
        must(Err::<(), _>("Expected Continued event"));
    }

    // Test Terminated event
    must(tx.send(DapEvent::Terminated { reason: "normal".to_string() }));
    if let DapEvent::Terminated { .. } = must(rx.recv_timeout(Duration::from_millis(100))) {
        // Success
    } else {
        must(Err::<(), _>("Expected Terminated event"));
    }

    // Test Error event
    must(tx.send(DapEvent::Error { message: "test error".to_string() }));
    if let DapEvent::Error { .. } = must(rx.recv_timeout(Duration::from_millis(100))) {
        // Success
    } else {
        must(Err::<(), _>("Expected Error event"));
    }
}

#[test]
fn test_tcp_attach_session_disconnect() {
    let mut session = TcpAttachSession::new();
    assert!(!session.is_connected());

    // Disconnecting when not connected should not fail
    let result = session.disconnect();
    assert!(result.is_ok());
    assert!(!session.is_connected());
}

#[test]
fn test_tcp_attach_config_edge_cases() {
    // Test with IPv6 loopback address
    let mut config = TcpAttachConfig::new("::1".to_string(), 13603);
    assert!(config.validate().is_ok());

    // Test with a numeric public IP (hermetic — no DNS dependency). 93.184.216.34
    // is example.com's well-known public address, but specified numerically so
    // the test does not depend on external DNS resolution (chatgpt-codex review).
    let mut config = TcpAttachConfig::new("93.184.216.34".to_string(), 13603);
    assert!(config.validate().is_ok());

    // SSRF defense (#5257): private IP addresses must be rejected.
    let mut config = TcpAttachConfig::new("192.168.1.1".to_string(), 13603);
    assert!(config.validate().is_err(), "private IP must be rejected by the SSRF filter");

    // SSRF defense (#5257): cloud metadata endpoint must be rejected.
    let mut config = TcpAttachConfig::new("169.254.169.254".to_string(), 13603);
    assert!(config.validate().is_err(), "cloud metadata IP must be rejected by the SSRF filter");

    // Test with maximum valid port
    let mut config = TcpAttachConfig::new("localhost".to_string(), 65535);
    assert!(config.validate().is_ok());

    // Test with minimum valid timeout
    let mut config = TcpAttachConfig::new("localhost".to_string(), 13603).with_timeout(1);
    assert!(config.validate().is_ok());

    // Test with maximum valid timeout
    let mut config = TcpAttachConfig::new("localhost".to_string(), 13603).with_timeout(300_000);
    assert!(config.validate().is_ok());
}

#[test]
fn test_tcp_attach_config_whitespace_handling() {
    // Test with whitespace in host - should be trimmed and valid
    let mut config = TcpAttachConfig::new("  localhost  ".to_string(), 13603);
    // The validation trims whitespace, so this should be valid
    assert!(config.validate().is_ok());

    // Test with only whitespace - should be invalid after trimming
    let mut config = TcpAttachConfig::new("   ".to_string(), 13603);
    assert!(config.validate().is_err());
}

#[test]
fn test_tcp_attach_default_implementation() {
    // Test Default trait implementation
    let session1 = TcpAttachSession::new();
    let session2 = TcpAttachSession::default();

    // Both should be disconnected initially
    assert!(!session1.is_connected());
    assert!(!session2.is_connected());
}

#[test]
fn test_tcp_attach_event_serialization() {
    // Test that events can be cloned and sent through channels
    let (tx, rx) = channel::<DapEvent>();

    let original =
        DapEvent::Output { category: "stderr".to_string(), output: "error message".to_string() };

    // Clone and send
    must(tx.send(original.clone()));

    let received = must(rx.recv_timeout(Duration::from_millis(100)));
    match received {
        DapEvent::Output { category, output } => {
            assert_eq!(category, "stderr");
            assert_eq!(output, "error message");
        }
        _ => must(Err::<(), _>("Expected Output event")),
    }
}

#[test]
fn test_tcp_attach_reader_handles_concatenated_frames() {
    let listener = must(TcpListener::bind(("127.0.0.1", 0)));
    let port = must(listener.local_addr()).port();

    let server_handle = thread::spawn(move || {
        let (mut socket, _) = must(listener.accept());

        let output_event = serde_json::json!({
            "type": "event",
            "seq": 1,
            "event": "output",
            "body": {
                "category": "stdout",
                "output": "hello"
            }
        })
        .to_string();
        let continued_event = serde_json::json!({
            "type": "event",
            "seq": 2,
            "event": "continued",
            "body": {
                "threadId": 7
            }
        })
        .to_string();

        let mut bytes = frame(output_event.as_bytes());
        bytes.extend_from_slice(&frame(continued_event.as_bytes()));
        must(socket.write_all(&bytes));
        must(socket.flush());
    });

    let mut session = TcpAttachSession::new();
    let (event_tx, event_rx) = sync_channel::<DapEvent>(TEST_ATTACH_EVENT_CAPACITY);
    session.set_event_sender(event_tx);

    let mut config = TcpAttachConfig::new("127.0.0.1".to_string(), port).with_timeout(2000);
    must(session.connect(&mut config));
    must(session.start_reader());

    let first = must(event_rx.recv_timeout(Duration::from_secs(2)));
    let second = must(event_rx.recv_timeout(Duration::from_secs(2)));

    match first {
        DapEvent::Output { category, output } => {
            assert_eq!(category, "stdout");
            assert_eq!(output, "hello");
        }
        other => must(Err::<(), _>(format!("Expected Output event, got {other:?}"))),
    }

    match second {
        DapEvent::Continued { thread_id } => {
            assert_eq!(thread_id, 7);
        }
        other => must(Err::<(), _>(format!("Expected Continued event, got {other:?}"))),
    }

    must(server_handle.join().map_err(|_| "Server thread panicked".to_string()));
}

#[test]
fn test_tcp_attach_connect_timeout_for_unreachable_endpoint() {
    let mut session = TcpAttachSession::new();
    let mut config = TcpAttachConfig::new("203.0.113.1".to_string(), 6553).with_timeout(50);

    let result = session.connect(&mut config);
    assert!(result.is_err(), "expected timeout or network error for unreachable endpoint");
    assert!(!session.is_connected(), "failed connect must keep session disconnected");
}

#[test]
fn test_tcp_attach_reader_emits_stopped_and_terminated_events() {
    let listener = must(TcpListener::bind(("127.0.0.1", 0)));
    let port = must(listener.local_addr()).port();

    let server_handle = thread::spawn(move || {
        let (mut socket, _) = must(listener.accept());

        let stopped_event = serde_json::json!({
            "type": "event",
            "seq": 1,
            "event": "stopped",
            "body": {
                "reason": "breakpoint",
                "threadId": 42
            }
        })
        .to_string();

        let terminated_event = serde_json::json!({
            "type": "event",
            "seq": 2,
            "event": "terminated",
            "body": {
                "reason": "completed"
            }
        })
        .to_string();

        let mut bytes = frame(stopped_event.as_bytes());
        bytes.extend_from_slice(&frame(terminated_event.as_bytes()));
        must(socket.write_all(&bytes));
        must(socket.flush());
    });

    let mut session = TcpAttachSession::new();
    let (event_tx, event_rx) = sync_channel::<DapEvent>(TEST_ATTACH_EVENT_CAPACITY);
    session.set_event_sender(event_tx);

    let mut config = TcpAttachConfig::new("127.0.0.1".to_string(), port).with_timeout(2000);
    must(session.connect(&mut config));
    must(session.start_reader());

    let first = must(event_rx.recv_timeout(Duration::from_secs(2)));
    let second = must(event_rx.recv_timeout(Duration::from_secs(2)));

    match first {
        DapEvent::Stopped { reason, thread_id } => {
            assert_eq!(reason, "breakpoint");
            assert_eq!(thread_id, 42);
        }
        other => must(Err::<(), _>(format!("Expected Stopped event, got {other:?}"))),
    }

    match second {
        DapEvent::Terminated { reason } => {
            assert_eq!(reason, "completed");
        }
        other => must(Err::<(), _>(format!("Expected Terminated event, got {other:?}"))),
    }

    must(server_handle.join().map_err(|_| "Server thread panicked".to_string()));
}

#[test]
fn test_tcp_attach_disconnect_after_reader_allows_reconnect() {
    let listener1 = must(TcpListener::bind(("127.0.0.1", 0)));
    let port1 = must(listener1.local_addr()).port();
    let server1 = thread::spawn(move || {
        let (_socket, _) = must(listener1.accept());
        thread::sleep(Duration::from_millis(800));
    });

    let listener2 = must(TcpListener::bind(("127.0.0.1", 0)));
    let port2 = must(listener2.local_addr()).port();
    let server2 = thread::spawn(move || {
        let (_socket, _) = must(listener2.accept());
        thread::sleep(Duration::from_millis(200));
    });

    let mut session = TcpAttachSession::new();
    let mut config1 = TcpAttachConfig::new("127.0.0.1".to_string(), port1).with_timeout(2000);
    must(session.connect(&mut config1));
    must(session.start_reader());
    assert!(session.is_connected());

    must(session.disconnect());
    assert!(
        !session.is_connected(),
        "disconnect should mark reader-backed session as disconnected"
    );

    let mut config2 = TcpAttachConfig::new("127.0.0.1".to_string(), port2).with_timeout(2000);
    must(session.connect(&mut config2));
    assert!(session.is_connected(), "session should reconnect cleanly after disconnect");

    must(session.disconnect());
    assert!(!session.is_connected());

    must(server1.join().map_err(|_| "Server 1 thread panicked".to_string()));
    must(server2.join().map_err(|_| "Server 2 thread panicked".to_string()));
}

/// #9521 review: a reader parked in cancellation-aware admission (state event
/// against a full fan-in queue) must retire on `disconnect` instead of later
/// delivering the stale event or clobbering a replacement connection's
/// `connected` state. The earlier reconnect test installs no event sender, so
/// its reader never parks; this one fills the queue first.
#[test]
fn test_tcp_attach_parked_reader_retires_on_disconnect_without_clobbering() {
    let listener1 = must(TcpListener::bind(("127.0.0.1", 0)));
    let port1 = must(listener1.local_addr()).port();
    let server1 = thread::spawn(move || {
        let (mut socket, _) = must(listener1.accept());

        // Eight output frames exactly fill the capacity-8 fan-in queue.
        let mut bytes = Vec::new();
        for seq in 1..=TEST_ATTACH_EVENT_CAPACITY as i64 {
            let output = serde_json::json!({
                "type": "event",
                "seq": seq,
                "event": "output",
                "body": { "category": "console", "output": format!("fill {seq}") }
            })
            .to_string();
            bytes.extend_from_slice(&frame(output.as_bytes()));
        }
        must(socket.write_all(&bytes));
        must(socket.flush());

        // Give the reader time to admit all eight, then send the stopped
        // event, which parks the reader in cancellation-aware admission.
        thread::sleep(Duration::from_millis(150));
        let stopped = serde_json::json!({
            "type": "event",
            "seq": 9,
            "event": "stopped",
            "body": { "reason": "breakpoint", "threadId": 7 }
        })
        .to_string();
        must(socket.write_all(&frame(stopped.as_bytes())));
        must(socket.flush());

        // Hold the socket open: only the retire path may end this reader.
        thread::sleep(Duration::from_millis(1200));
    });

    let listener2 = must(TcpListener::bind(("127.0.0.1", 0)));
    let port2 = must(listener2.local_addr()).port();
    let server2 = thread::spawn(move || {
        let (_socket, _) = must(listener2.accept());
        thread::sleep(Duration::from_millis(1400));
    });

    let mut session = TcpAttachSession::new();
    let (event_tx, event_rx) = sync_channel::<DapEvent>(TEST_ATTACH_EVENT_CAPACITY);
    session.set_event_sender(event_tx);

    let mut config1 = TcpAttachConfig::new("127.0.0.1".to_string(), port1).with_timeout(2000);
    must(session.connect(&mut config1));
    must(session.start_reader());

    // Let the reader fill the queue and park on the stopped event.
    thread::sleep(Duration::from_millis(300));

    // The disconnect retires the parked reader; nothing may be delivered and
    // the reader must stop without touching shared state.
    must(session.disconnect());
    assert!(!session.is_connected());

    // Reconnect on the same session: the supported replacement flow.
    let mut config2 = TcpAttachConfig::new("127.0.0.1".to_string(), port2).with_timeout(2000);
    must(session.connect(&mut config2));
    must(session.start_reader());
    assert!(session.is_connected(), "replacement connection must be live");

    // Drain the events admitted before the disconnect: the eight outputs are
    // legitimately queued. The retired reader's parked stopped event must
    // never join them — under the pre-fix behavior it would commit as soon as
    // this drain freed a slot.
    let deadline = std::time::Instant::now() + Duration::from_millis(2000);
    loop {
        match event_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(DapEvent::Stopped { .. }) => {
                must(Err::<(), _>(
                    "a retired reader must not deliver its stale stopped event".to_string(),
                ));
            }
            Ok(_) => {
                if std::time::Instant::now() > deadline {
                    must(Err::<(), _>("drain exceeded its deadline".to_string()));
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                must(Err::<(), _>(
                    "the fan-in queue must stay alive for the replacement".to_string(),
                ));
            }
        }
    }

    // Even after the stale socket closes, the replacement connection's state
    // must remain untouched.
    thread::sleep(Duration::from_millis(200));
    assert!(
        session.is_connected(),
        "a retired reader must not clobber the replacement connection's state"
    );

    let _ = session.disconnect();

    must(server1.join().map_err(|_| "Server 1 thread panicked".to_string()));
    must(server2.join().map_err(|_| "Server 2 thread panicked".to_string()));
}
