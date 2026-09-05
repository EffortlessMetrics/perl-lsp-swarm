//! Live-wiring tests for the mirror-mode external-peer **launch** path.
//!
//! These prove the [`MirrorPeerBridge`] behaviors that turn the #3321 substrate
//! into a drivable mirror session: breakpoints that arrive before the peer's
//! handshake are queued and flushed once the peer says hello; the peer's
//! `stopped`/`output`/`terminated` reach the DAP client; and editor-initiated
//! control is rejected in mirror mode. Everything is exercised against an
//! in-repo **fake ptkdb peer** (same pattern as `external_peer_conformance`);
//! end-to-end editor↔real-`Devel::ptkdb` sessions remain deferred.

use perl_tdd_support::{must, must_some};
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn Error>>;

use perl_dap::backend::capabilities::ControlMode;
use perl_dap::backend::external_peer::ExternalDebuggerPeerBackend;
use perl_dap::backend::peer_launch::MirrorPeerBridge;
use perl_dap::backend::{DebugBackend, InitializeBackendParams};
use perl_dap::debug_adapter::DapMessage;
use perl_dap::peer_protocol::message::{
    PeerEvent, PeerMessage, PeerRequest, PeerResponse, command, event,
};
use perl_dap::peer_protocol::payloads::{
    HelloArgs, OutputEventBody, SetBreakpointsArgs, SetBreakpointsResponseBody,
    SetFunctionBreakpointsArgs, StoppedEventBody, WireResolvedBreakpoint, WireSource,
};
use perl_dap::peer_protocol::{
    PROTOCOL_VERSION, PeerFrameDecoder, PeerReportedCapabilities, encode_message,
};

/// A configurable fake ptkdb peer that listens for the host to connect, says
/// hello, records every host request, answers `debugger/setBreakpoints`, and can
/// emit scripted events right after the handshake or drop the connection.
struct FakePeer {
    handle: JoinHandle<()>,
    addr: std::net::SocketAddr,
    /// Lines of every `debugger/setBreakpoints` the peer received.
    breakpoint_lines: Arc<Mutex<Vec<u32>>>,
    /// Names of every `debugger/setFunctionBreakpoints` the peer received.
    function_breakpoint_names: Arc<Mutex<Vec<String>>>,
}

struct FakePeerScript {
    caps: PeerReportedCapabilities,
    /// Events to emit immediately after the handshake.
    emit_after_hello: Vec<PeerEvent>,
    /// If true, the peer drops the connection right after the handshake.
    drop_after_hello: bool,
}

impl FakePeer {
    fn start(script: FakePeerScript) -> Self {
        let listener = must(TcpListener::bind(("127.0.0.1", 0)));
        let addr = must(listener.local_addr());
        let breakpoint_lines = Arc::new(Mutex::new(Vec::new()));
        let function_breakpoint_names = Arc::new(Mutex::new(Vec::new()));
        let lines = Arc::clone(&breakpoint_lines);
        let names = Arc::clone(&function_breakpoint_names);
        let handle = std::thread::spawn(move || run_peer(listener, script, lines, names));
        FakePeer { handle, addr, breakpoint_lines, function_breakpoint_names }
    }
}

fn run_peer(
    listener: TcpListener,
    script: FakePeerScript,
    breakpoint_lines: Arc<Mutex<Vec<u32>>>,
    function_breakpoint_names: Arc<Mutex<Vec<String>>>,
) {
    let (stream, _) = must(listener.accept());
    let mut write = must(stream.try_clone());
    let mut read = stream;
    let mut seq = 700i64;

    let send = |w: &mut TcpStream, m: &PeerMessage| {
        let _ = w.write_all(&must(encode_message(m)));
        let _ = w.flush();
    };

    // Handshake.
    seq += 1;
    send(
        &mut write,
        &PeerMessage::Request(PeerRequest {
            seq,
            command: command::HELLO.to_string(),
            arguments: serde_json::to_value(HelloArgs {
                peer: "FakePtkdb".to_string(),
                peer_version: Some("0.1".to_string()),
                protocol_version: PROTOCOL_VERSION.to_string(),
                token: None,
                capabilities: script.caps,
            })
            .ok(),
        }),
    );

    // Scripted post-hello events.
    for mut ev in script.emit_after_hello {
        seq += 1;
        ev.seq = seq;
        send(&mut write, &PeerMessage::Event(ev));
    }

    if script.drop_after_hello {
        // Close the connection to simulate a peer crash/exit.
        return;
    }

    let mut decoder = PeerFrameDecoder::new();
    let mut buf = [0u8; 4096];
    read.set_read_timeout(Some(Duration::from_millis(300))).ok();
    let deadline = Instant::now() + Duration::from_secs(5);
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
                            let args: SetBreakpointsArgs = must_some(
                                req.arguments.clone().and_then(|a| serde_json::from_value(a).ok()),
                            );
                            let mut resolved = Vec::new();
                            let mut lines = must(breakpoint_lines.lock());
                            for (i, b) in args.breakpoints.iter().enumerate() {
                                lines.push(b.line);
                                resolved.push(WireResolvedBreakpoint {
                                    id: i as i64 + 1,
                                    verified: true,
                                    line: b.line,
                                    column: None,
                                    message: None,
                                });
                            }
                            serde_json::to_value(SetBreakpointsResponseBody {
                                breakpoints: resolved,
                            })
                            .ok()
                        }
                        command::SET_FUNCTION_BREAKPOINTS => {
                            let args: SetFunctionBreakpointsArgs = must_some(
                                req.arguments.clone().and_then(|a| serde_json::from_value(a).ok()),
                            );
                            let mut resolved = Vec::new();
                            let mut names = must(function_breakpoint_names.lock());
                            for (i, name) in args.names.iter().enumerate() {
                                names.push(name.clone());
                                resolved.push(WireResolvedBreakpoint {
                                    id: i as i64 + 1,
                                    verified: true,
                                    line: 0,
                                    column: None,
                                    message: None,
                                });
                            }
                            serde_json::to_value(SetBreakpointsResponseBody {
                                breakpoints: resolved,
                            })
                            .ok()
                        }
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
                    if req.command == command::DISCONNECT || req.command == command::GOODBYE {
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
}

fn full_caps() -> PeerReportedCapabilities {
    PeerReportedCapabilities {
        can_continue: true,
        can_step: true,
        can_pause: true,
        can_evaluate: true,
        can_set_breakpoints: true,
        can_set_function_breakpoints: true,
        can_condition_breakpoints: true,
        can_list_stack: true,
        can_list_variables: true,
        ..Default::default()
    }
}

/// Connect the real peer backend to `peer` and complete the handshake.
fn live_backend(peer: &FakePeer) -> Box<dyn DebugBackend> {
    let mut backend = must(ExternalDebuggerPeerBackend::connect(peer.addr, Duration::from_secs(5)));
    must(backend.initialize(InitializeBackendParams::default()));
    Box::new(backend)
}

fn find_event<'a>(msgs: &'a [DapMessage], name: &str) -> Option<&'a DapMessage> {
    msgs.iter().find(|m| matches!(m, DapMessage::Event { event, .. } if event == name))
}

fn as_response(
    msg: &DapMessage,
) -> Result<(&str, bool, Option<&serde_json::Value>), Box<dyn Error>> {
    match msg {
        DapMessage::Response { command, success, body, .. } => {
            Ok((command.as_str(), *success, body.as_ref()))
        }
        other => Err(format!("expected response, got {other:?}").into()),
    }
}

#[test]
fn dap_external_peer_launch_queues_breakpoints_before_handshake() -> TestResult {
    // No peer connected yet: setBreakpoints must be queued and answered with an
    // unverified `pending` response, not sent anywhere or dropped.
    let mut bridge = MirrorPeerBridge::new_pending(ControlMode::Mirror);
    let init = bridge.dispatch(1, "initialize", Some(serde_json::json!({ "adapterID": "perl" })));
    // Static conservative capabilities are advertised before any peer exists.
    let caps = must_some(as_response(init.first().ok_or("initialize response missing")?)?.2);
    assert_eq!(caps["supportsConditionalBreakpoints"], true);
    assert_eq!(caps["supportsLogPoints"], false);

    let out = bridge.dispatch(
        2,
        "setBreakpoints",
        Some(serde_json::json!({
            "source": { "path": "/work/script.pl" },
            "breakpoints": [{ "line": 42 }, { "line": 7 }],
        })),
    );
    assert_eq!(bridge.pending_source_count(), 1, "the source's breakpoints are queued");
    let (_, ok, body) = as_response(out.first().ok_or("breakpoint response missing")?)?;
    assert!(ok, "a queued setBreakpoints still returns success");
    let bps = must_some(must_some(body)["breakpoints"].as_array()).clone();
    assert_eq!(bps.len(), 2, "response matches the request positionally");
    assert_eq!(bps[0]["verified"], false, "queued breakpoints are unverified until flush");
    assert_eq!(bps[0]["line"], 42);
    Ok(())
}

#[test]
fn dap_external_peer_launch_flushes_breakpoints_after_hello() -> TestResult {
    let peer = FakePeer::start(FakePeerScript {
        caps: full_caps(),
        emit_after_hello: vec![],
        drop_after_hello: false,
    });

    let mut bridge = MirrorPeerBridge::new_pending(ControlMode::Mirror);
    bridge.dispatch(1, "initialize", Some(serde_json::json!({ "adapterID": "perl" })));
    bridge.dispatch(
        2,
        "setBreakpoints",
        Some(serde_json::json!({
            "source": { "path": "/work/script.pl" },
            "breakpoints": [{ "line": 42 }],
        })),
    );
    assert_eq!(bridge.pending_source_count(), 1);
    // Before the peer connects, it has received nothing.
    assert!(must(peer.breakpoint_lines.lock()).is_empty());

    // The peer handshakes; flush sends the queued breakpoints to it.
    let backend = live_backend(&peer);
    let flush = bridge.go_live(backend);
    assert!(bridge.is_live());
    assert_eq!(bridge.pending_source_count(), 0, "queue is drained after flush");

    // The peer actually received the queued breakpoint over the wire.
    assert_eq!(
        *must(peer.breakpoint_lines.lock()),
        vec![42],
        "the queued breakpoint reached the peer on flush"
    );

    // The flush surfaces the resolved breakpoint as a `breakpoint` changed event.
    let changed = must_some(find_event(&flush, "breakpoint"));
    if let DapMessage::Event { body: Some(b), .. } = changed {
        assert_eq!(b["reason"], "changed");
        assert_eq!(b["breakpoint"]["verified"], true);
        assert_eq!(b["breakpoint"]["line"], 42);
    } else {
        return Err("breakpoint event had no body".into());
    }

    drop(bridge);
    let _ = peer.handle.join();
    Ok(())
}

#[test]
fn dap_external_peer_stopped_event_reaches_dap_client() -> TestResult {
    let stopped = PeerEvent {
        seq: 0,
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
    };
    let peer = FakePeer::start(FakePeerScript {
        caps: full_caps(),
        emit_after_hello: vec![stopped],
        drop_after_hello: false,
    });

    let mut bridge = MirrorPeerBridge::new_pending(ControlMode::Mirror);
    let backend = live_backend(&peer);
    bridge.go_live(backend);

    let mut acc: Vec<DapMessage> = Vec::new();
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
        return Err("stopped event had no body".into());
    }

    drop(bridge);
    let _ = peer.handle.join();
    Ok(())
}

#[test]
fn dap_external_peer_output_event_reaches_dap_client() -> TestResult {
    let output = PeerEvent {
        seq: 0,
        event: event::OUTPUT.to_string(),
        body: serde_json::to_value(OutputEventBody {
            category: "stderr".to_string(),
            output: "boom\n".to_string(),
        })
        .ok(),
    };
    let peer = FakePeer::start(FakePeerScript {
        caps: full_caps(),
        emit_after_hello: vec![output],
        drop_after_hello: false,
    });

    let mut bridge = MirrorPeerBridge::new_pending(ControlMode::Mirror);
    let backend = live_backend(&peer);
    bridge.go_live(backend);

    let mut acc: Vec<DapMessage> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    while find_event(&acc, "output").is_none() && Instant::now() < deadline {
        acc.extend(bridge.poll_events());
        if find_event(&acc, "output").is_none() {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let output = must_some(find_event(&acc, "output"));
    if let DapMessage::Event { body: Some(b), .. } = output {
        assert_eq!(b["category"], "stderr");
        assert_eq!(b["output"], "boom\n");
    } else {
        return Err("output event had no body".into());
    }

    drop(bridge);
    let _ = peer.handle.join();
    Ok(())
}

#[test]
fn dap_external_peer_terminated_on_peer_disconnect() {
    // The peer says hello then drops without an explicit `debugger/terminated`.
    let peer = FakePeer::start(FakePeerScript {
        caps: full_caps(),
        emit_after_hello: vec![],
        drop_after_hello: true,
    });

    let mut bridge = MirrorPeerBridge::new_pending(ControlMode::Mirror);
    let backend = live_backend(&peer);
    bridge.go_live(backend);
    // Let the peer thread finish and close the socket.
    let _ = peer.handle.join();

    let mut acc: Vec<DapMessage> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    while find_event(&acc, "terminated").is_none() && Instant::now() < deadline {
        acc.extend(bridge.poll_events());
        if find_event(&acc, "terminated").is_none() {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    assert!(
        find_event(&acc, "terminated").is_some(),
        "a peer disconnect must synthesize a DAP terminated event: {acc:?}"
    );

    // Terminated is emitted at most once — subsequent polls add no duplicate.
    let more = bridge.poll_events();
    assert!(
        find_event(&more, "terminated").is_none(),
        "terminated must not be emitted twice: {more:?}"
    );
}

#[test]
fn dap_external_peer_rejects_control_in_mirror_mode() -> TestResult {
    // Even with a fully-capable peer, mirror mode means the peer's UI owns
    // execution: editor-initiated continue/step must be rejected gracefully.
    let peer = FakePeer::start(FakePeerScript {
        caps: full_caps(),
        emit_after_hello: vec![],
        drop_after_hello: false,
    });

    let mut bridge = MirrorPeerBridge::new_pending(ControlMode::Mirror);
    let backend = live_backend(&peer);
    bridge.go_live(backend);

    for cmd in ["continue", "next", "stepIn", "stepOut", "pause"] {
        let out = bridge.dispatch(10, cmd, Some(serde_json::json!({ "threadId": 1 })));
        let Some(response) = out.first() else {
            return Err(format!("expected a response for {cmd}").into());
        };
        let DapMessage::Response { command, success, message, .. } = response else {
            return Err(format!("expected a response for {cmd}").into());
        };
        assert_eq!(command, cmd);
        assert!(!success, "{cmd} must be rejected while in mirror mode");
        let msg = message.as_deref().unwrap_or("");
        assert!(msg.contains("mirror mode"), "rejection must explain mirror mode: {msg}");
    }

    drop(bridge);
    let _ = peer.handle.join();
    Ok(())
}

#[test]
fn dap_external_peer_launch_queues_function_breakpoints_before_handshake() -> TestResult {
    // No peer connected yet: setFunctionBreakpoints must be queued (mirroring
    // the setBreakpoints queue) and answered with an unverified `pending`
    // response, not silently dropped (CodeRabbit finding: peer_launch.rs:~330).
    let mut bridge = MirrorPeerBridge::new_pending(ControlMode::Mirror);
    bridge.dispatch(1, "initialize", Some(serde_json::json!({ "adapterID": "perl" })));

    let out = bridge.dispatch(
        2,
        "setFunctionBreakpoints",
        Some(serde_json::json!({
            "breakpoints": [{ "name": "My::App::dispatch" }],
        })),
    );
    assert!(
        bridge.has_pending_function_breakpoints(),
        "function breakpoints set before handshake must be queued, not dropped"
    );
    let (_, ok, body) = as_response(out.first().ok_or("function breakpoint response missing")?)?;
    assert!(ok, "a queued setFunctionBreakpoints still returns success");
    let bps = must_some(must_some(body)["breakpoints"].as_array()).clone();
    assert_eq!(bps.len(), 1);
    assert_eq!(bps[0]["verified"], false, "queued function breakpoints are unverified until flush");
    Ok(())
}

#[test]
fn dap_external_peer_launch_flushes_function_breakpoints_after_hello() -> TestResult {
    // The queued function breakpoints must actually reach the peer once it
    // handshakes, proving go_live flushes pending_function_breakpoints.
    let peer = FakePeer::start(FakePeerScript {
        caps: full_caps(),
        emit_after_hello: vec![],
        drop_after_hello: false,
    });

    let mut bridge = MirrorPeerBridge::new_pending(ControlMode::Mirror);
    bridge.dispatch(1, "initialize", Some(serde_json::json!({ "adapterID": "perl" })));
    bridge.dispatch(
        2,
        "setFunctionBreakpoints",
        Some(serde_json::json!({
            "breakpoints": [{ "name": "My::App::dispatch" }],
        })),
    );
    assert!(bridge.has_pending_function_breakpoints());
    assert!(must(peer.function_breakpoint_names.lock()).is_empty());

    let backend = live_backend(&peer);
    let flush = bridge.go_live(backend);
    assert!(bridge.is_live());
    assert!(
        !bridge.has_pending_function_breakpoints(),
        "the function-breakpoint queue is drained after flush"
    );

    assert_eq!(
        *must(peer.function_breakpoint_names.lock()),
        vec!["My::App::dispatch".to_string()],
        "the queued function breakpoint reached the peer on flush"
    );

    let changed = must_some(find_event(&flush, "breakpoint"));
    if let DapMessage::Event { body: Some(b), .. } = changed {
        assert_eq!(b["reason"], "changed");
        assert_eq!(b["breakpoint"]["verified"], true);
    } else {
        return Err("breakpoint event had no body".into());
    }

    drop(bridge);
    let _ = peer.handle.join();
    Ok(())
}

#[test]
fn dap_external_peer_launch_reports_flush_failure_to_editor() -> TestResult {
    // The peer negotiates *without* source-breakpoint support, so the queued
    // flush in go_live fails; the editor must be told (a `breakpoint` changed
    // event with verified:false and a failure message), not left holding the
    // stale "pending" placeholder forever (CodeRabbit finding: ~416).
    let mut caps_without_breakpoints = full_caps();
    caps_without_breakpoints.can_set_breakpoints = false;
    let peer = FakePeer::start(FakePeerScript {
        caps: caps_without_breakpoints,
        emit_after_hello: vec![],
        drop_after_hello: false,
    });

    let mut bridge = MirrorPeerBridge::new_pending(ControlMode::Mirror);
    bridge.dispatch(1, "initialize", Some(serde_json::json!({ "adapterID": "perl" })));
    bridge.dispatch(
        2,
        "setBreakpoints",
        Some(serde_json::json!({
            "source": { "path": "/work/script.pl" },
            "breakpoints": [{ "line": 42 }],
        })),
    );
    assert_eq!(bridge.pending_source_count(), 1);

    let backend = live_backend(&peer);
    let flush = bridge.go_live(backend);
    assert!(bridge.is_live());
    assert_eq!(bridge.pending_source_count(), 0, "the queue is drained even when the flush fails");

    let changed = must_some(find_event(&flush, "breakpoint"));
    if let DapMessage::Event { body: Some(b), .. } = changed {
        assert_eq!(b["reason"], "changed");
        assert_eq!(b["breakpoint"]["verified"], false, "a failed flush is never reported verified");
        let message = must_some(b["breakpoint"]["message"].as_str());
        assert!(
            message.contains("failed to set breakpoint"),
            "message must explain the flush failure, got: {message}"
        );
    } else {
        return Err("breakpoint event had no body".into());
    }

    drop(bridge);
    let _ = peer.handle.join();
    Ok(())
}

#[test]
fn dap_external_peer_launch_terminate_does_not_duplicate_terminated_after_peer_close() -> TestResult
{
    // If the peer already emitted `terminated` (connection closed), a later
    // editor-driven `terminate` must not push a second one (CodeRabbit
    // finding: ~545).
    let peer = FakePeer::start(FakePeerScript {
        caps: full_caps(),
        emit_after_hello: vec![],
        drop_after_hello: true,
    });

    let mut bridge = MirrorPeerBridge::new_pending(ControlMode::Mirror);
    let backend = live_backend(&peer);
    bridge.go_live(backend);
    let _ = peer.handle.join();

    // Drive poll_events until the synthesized terminated (from peer close)
    // shows up.
    let mut acc: Vec<DapMessage> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    while find_event(&acc, "terminated").is_none() && Instant::now() < deadline {
        acc.extend(bridge.poll_events());
        if find_event(&acc, "terminated").is_none() {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    assert!(find_event(&acc, "terminated").is_some(), "peer close must synthesize terminated");

    // The editor now sends its own `terminate` — this must not emit a second
    // `terminated` event.
    let out = bridge.dispatch(20, "terminate", None);
    assert!(
        find_event(&out, "terminated").is_none(),
        "terminate after an already-emitted terminated must not duplicate it: {out:?}"
    );
    let (cmd, ok, _) = as_response(out.first().ok_or("terminate response missing")?)?;
    assert_eq!(cmd, "terminate");
    assert!(ok, "terminate itself must still succeed");
    Ok(())
}
