//! End-to-end reachability proof for the external-peer seam.
//!
//! Drives a real [`ExternalDebuggerPeerBackend`] (connected to a fake ptkdb
//! peer) through the [`DapPeerBridge`] using actual DAP requests, and asserts
//! that:
//! - DAP requests reach the peer over the Perl Debugger Peer Protocol and their
//!   responses come back as DAP responses, and
//! - an asynchronous peer `debugger/stopped` event surfaces to the editor as a
//!   DAP `stopped` event.
//!
//! This closes the Reachability axis: the peer backend is live-drivable from a
//! real DAP session, not just component-tested.

use perl_tdd_support::{must, must_some};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use perl_dap::backend::DapPeerBridge;
use perl_dap::backend::capabilities::ControlMode;
use perl_dap::backend::external_peer::ExternalDebuggerPeerBackend;
use perl_dap::backend::peer_launch::PeerListenEndpoint;
use perl_dap::debug_adapter::DapMessage;
use perl_dap::peer_protocol::message::{
    PeerEvent, PeerMessage, PeerRequest, PeerResponse, command, event,
};
use perl_dap::peer_protocol::payloads::{
    HelloArgs, SetBreakpointsResponseBody, StackTraceResponseBody, StoppedEventBody,
    WireResolvedBreakpoint, WireSource, WireStackFrame,
};
use perl_dap::peer_protocol::{
    PROTOCOL_VERSION, PeerFrameDecoder, PeerReportedCapabilities, encode_message,
};

/// A fake ptkdb peer: connects to `addr`, sends hello, answers debugger/*
/// requests, and — on `debugger/continue` — emits a `debugger/stopped` event.
fn spawn_fake_peer(addr: std::net::SocketAddr, token: Option<String>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let stream = match TcpStream::connect(addr) {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut write = must(stream.try_clone());
        let mut read = stream;
        let mut seq = 900i64;

        let caps = PeerReportedCapabilities {
            can_continue: true,
            can_step: true,
            can_pause: true,
            can_evaluate: true,
            can_set_breakpoints: true,
            can_condition_breakpoints: true,
            can_list_stack: true,
            can_list_variables: true,
            ..Default::default()
        };
        seq += 1;
        let hello = PeerMessage::Request(PeerRequest {
            seq,
            command: command::HELLO.to_string(),
            arguments: serde_json::to_value(HelloArgs {
                peer: "FakePtkdb".to_string(),
                peer_version: Some("0.1".to_string()),
                protocol_version: PROTOCOL_VERSION.to_string(),
                token,
                capabilities: caps,
            })
            .ok(),
        });
        let _ = write.write_all(&must(encode_message(&hello)));

        let send = |w: &mut TcpStream, m: &PeerMessage| {
            let _ = w.write_all(&must(encode_message(m)));
            let _ = w.flush();
        };

        let mut decoder = PeerFrameDecoder::new();
        let mut buf = [0u8; 4096];
        read.set_read_timeout(Some(Duration::from_millis(400))).ok();
        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline {
            match read.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    decoder.push(&buf[..n]);
                    while let Ok(Some(msg)) = decoder.try_next() {
                        let PeerMessage::Request(req) = msg else { continue };
                        if req.command == command::HELLO {
                            continue;
                        }
                        seq += 1;
                        let body = match req.command.as_str() {
                            command::SET_BREAKPOINTS => {
                                serde_json::to_value(SetBreakpointsResponseBody {
                                    breakpoints: vec![WireResolvedBreakpoint {
                                        id: 1,
                                        verified: true,
                                        line: 42,
                                        column: None,
                                        message: None,
                                    }],
                                })
                                .ok()
                            }
                            command::STACK_TRACE => serde_json::to_value(StackTraceResponseBody {
                                stack_frames: vec![WireStackFrame {
                                    id: 1,
                                    name: "main::run".to_string(),
                                    source: WireSource {
                                        path: "/work/script.pl".to_string(),
                                        name: None,
                                        source_reference: None,
                                    },
                                    line: 42,
                                    column: 1,
                                }],
                            })
                            .ok(),
                            _ => None,
                        };
                        send(
                            &mut write,
                            &PeerMessage::Response(PeerResponse {
                                seq,
                                request_seq: req.seq,
                                success: true,
                                command: req.command.clone(),
                                message: None,
                                cause: None,
                                body,
                            }),
                        );

                        // After a continue, the debuggee "hits a breakpoint".
                        if req.command == command::CONTINUE {
                            seq += 1;
                            send(
                                &mut write,
                                &PeerMessage::Event(PeerEvent {
                                    seq,
                                    event: event::STOPPED.to_string(),
                                    body: serde_json::to_value(StoppedEventBody {
                                        reason: "breakpoint".to_string(),
                                        thread_id: 1,
                                        source: Some(WireSource {
                                            path: "/work/script.pl".to_string(),
                                            name: None,
                                            source_reference: None,
                                        }),
                                        line: Some(42),
                                        column: Some(1),
                                    })
                                    .ok(),
                                }),
                            );
                        }
                        if req.command == command::DISCONNECT {
                            return;
                        }
                    }
                }
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => break,
            }
        }
    })
}

fn response_body<'a>(
    msg: &'a DapMessage,
    expect_cmd: &str,
) -> Result<&'a serde_json::Value, Box<dyn std::error::Error>> {
    match msg {
        DapMessage::Response { command, success, body, .. } => {
            if command != expect_cmd {
                return Err(format!(
                    "unexpected response command: expected {expect_cmd}, got {command}"
                )
                .into());
            }
            if !success {
                return Err(format!("response for {expect_cmd} was not successful: {msg:?}").into());
            }
            body.as_ref().ok_or_else(|| format!("{expect_cmd} had no body").into())
        }
        _ => Err(format!("expected a response to {expect_cmd}, got {msg:?}").into()),
    }
}

#[test]
fn response_body_reports_malformed_responses() -> Result<(), Box<dyn std::error::Error>> {
    let event = DapMessage::Event { seq: 1, event: "stopped".to_string(), body: None };
    let event_error = response_body(&event, "initialize")
        .err()
        .ok_or("non-response message unexpectedly succeeded")?;
    assert!(
        event_error.to_string().contains("initialize"),
        "event error did not include the expected command"
    );

    let missing_body = DapMessage::Response {
        seq: 1,
        request_seq: 1,
        success: true,
        command: "initialize".to_string(),
        body: None,
        message: None,
    };
    let body_error = response_body(&missing_body, "initialize")
        .err()
        .ok_or("bodyless response unexpectedly succeeded")?;
    assert!(
        body_error.to_string().contains("initialize"),
        "missing-body error did not include the expected command"
    );

    let wrong_command = DapMessage::Response {
        seq: 1,
        request_seq: 1,
        success: true,
        command: "threads".to_string(),
        body: Some(serde_json::json!({})),
        message: None,
    };
    let command_error = response_body(&wrong_command, "initialize")
        .err()
        .ok_or("wrong-command response unexpectedly succeeded")?;
    assert!(
        command_error.to_string().contains("initialize"),
        "wrong-command error did not include the expected command"
    );

    let failed_response = DapMessage::Response {
        seq: 1,
        request_seq: 1,
        success: false,
        command: "initialize".to_string(),
        body: None,
        message: Some("backend failed".to_string()),
    };
    let failure_error = response_body(&failed_response, "initialize")
        .err()
        .ok_or("failed response unexpectedly succeeded")?;
    assert!(
        failure_error.to_string().contains("initialize"),
        "failed-response error did not include the expected command"
    );

    Ok(())
}

fn find_event<'a>(msgs: &'a [DapMessage], name: &str) -> Option<&'a DapMessage> {
    msgs.iter().find(|m| matches!(m, DapMessage::Event { event, .. } if event == name))
}

#[test]
fn full_dap_session_drives_the_live_peer_backend() -> Result<(), Box<dyn std::error::Error>> {
    // Fake ptkdb peer listens; the backend connects to it.
    let (listener, endpoint) = must(PeerListenEndpoint::bind("127.0.0.1", 0, ControlMode::Mirror));
    let addr = endpoint.addr;
    let token = endpoint.session_token();
    let credential = endpoint.session_credential();
    let peer = spawn_fake_peer(addr, Some(token.clone()));
    let (stream, _) = must(listener.accept());
    let backend = must(ExternalDebuggerPeerBackend::from_connected_stream_with_token(
        stream,
        Duration::from_secs(5),
        credential,
    ));

    let mut bridge = DapPeerBridge::new(Box::new(backend));

    // initialize → capabilities + initialized event (also completes the handshake).
    let out = bridge.dispatch(1, "initialize", Some(serde_json::json!({ "adapterID": "perl" })));
    let caps = response_body(&out[0], "initialize")?;
    assert_eq!(caps["supportsConditionalBreakpoints"], true, "ptkdb peer negotiated conditions");
    assert_eq!(caps["supportsLogPoints"], false, "ptkdb v1 has no logpoints");
    assert!(find_event(&out, "initialized").is_some(), "initialized event emitted");

    // setBreakpoints → reaches the peer, resolved breakpoints come back.
    let out = bridge.dispatch(
        2,
        "setBreakpoints",
        Some(serde_json::json!({
            "source": { "path": "/work/script.pl" },
            "breakpoints": [{ "line": 42, "condition": "$x > 10" }],
        })),
    );
    let body = response_body(&out[0], "setBreakpoints")?;
    assert_eq!(body["breakpoints"][0]["id"], 1);
    assert_eq!(body["breakpoints"][0]["verified"], true);
    assert_eq!(body["breakpoints"][0]["line"], 42);

    // continue → response, and the peer asynchronously emits a stopped event.
    let out = bridge.dispatch(3, "continue", Some(serde_json::json!({ "threadId": 1 })));
    assert_eq!(response_body(&out[0], "continue")?["allThreadsContinued"], true);

    // Accumulate events (DapMessage is not Clone) and poll until the peer's
    // stopped event surfaces as a DAP stopped event.
    let mut acc: Vec<DapMessage> = out;
    let deadline = Instant::now() + Duration::from_secs(3);
    while find_event(&acc, "stopped").is_none() && Instant::now() < deadline {
        acc.extend(bridge.poll_events());
        if find_event(&acc, "stopped").is_none() {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let stopped = must_some(find_event(&acc, "stopped"));
    if let DapMessage::Event { body: Some(b), .. } = stopped {
        assert_eq!(b["reason"], "breakpoint");
        assert_eq!(b["threadId"], 1);
        assert_eq!(b["allThreadsStopped"], true);
    } else {
        drop(bridge);
        let _ = peer.join();
        return Err("stopped event had no body".into());
    }

    // stackTrace → proxied to the peer and returned as a DAP stack.
    let out = bridge.dispatch(4, "stackTrace", Some(serde_json::json!({ "threadId": 1 })));
    let frames = &response_body(&out[0], "stackTrace")?["stackFrames"];
    assert_eq!(frames[0]["name"], "main::run");
    assert_eq!(frames[0]["line"], 42);

    // disconnect → clean teardown (a bodyless successful response).
    let out =
        bridge.dispatch(5, "disconnect", Some(serde_json::json!({ "terminateDebuggee": false })));
    match &out[0] {
        DapMessage::Response { command, success, .. } => {
            assert_eq!(command, "disconnect");
            assert!(success, "disconnect should succeed");
        }
        other => return Err(format!("expected disconnect response, got {other:?}").into()),
    }

    drop(bridge);
    let _ = peer.join();
    Ok(())
}
