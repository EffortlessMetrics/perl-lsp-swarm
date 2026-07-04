//! Live-wiring tests for the mirror-mode external-peer **launch** path.
//!
//! These prove the [`MirrorPeerBridge`] behaviors that turn the #3321 substrate
//! into a drivable mirror session: breakpoints that arrive before the peer's
//! handshake are queued and flushed once the peer says hello; the peer's
//! `stopped`/`output`/`terminated` reach the DAP client; and editor-initiated
//! control is rejected in mirror mode. Everything is exercised against an
//! in-repo **fake ptkdb peer** (same pattern as `external_peer_conformance`);
//! end-to-end editor↔real-`Devel::ptkdb` sessions remain deferred.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use perl_dap::backend::capabilities::ControlMode;
use perl_dap::backend::external_peer::ExternalDebuggerPeerBackend;
use perl_dap::backend::peer_launch::MirrorPeerBridge;
use perl_dap::backend::{DebugBackend, InitializeBackendParams};
use perl_dap::debug_adapter::DapMessage;
use perl_dap::peer_protocol::message::{
    PeerEvent, PeerMessage, PeerRequest, PeerResponse, command, event,
};
use perl_dap::peer_protocol::payloads::{
    HelloArgs, OutputEventBody, SetBreakpointsArgs, SetBreakpointsResponseBody, StoppedEventBody,
    WireResolvedBreakpoint, WireSource,
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
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let addr = listener.local_addr().expect("addr");
        let breakpoint_lines = Arc::new(Mutex::new(Vec::new()));
        let lines = Arc::clone(&breakpoint_lines);
        let handle = std::thread::spawn(move || run_peer(listener, script, lines));
        FakePeer { handle, addr, breakpoint_lines }
    }
}

fn run_peer(listener: TcpListener, script: FakePeerScript, breakpoint_lines: Arc<Mutex<Vec<u32>>>) {
    let (stream, _) = listener.accept().expect("accept");
    let mut write = stream.try_clone().expect("clone");
    let mut read = stream;
    let mut seq = 700i64;

    let send = |w: &mut TcpStream, m: &PeerMessage| {
        let _ = w.write_all(&encode_message(m).expect("encode"));
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
                            let args: SetBreakpointsArgs = req
                                .arguments
                                .clone()
                                .and_then(|a| serde_json::from_value(a).ok())
                                .expect("set bp args");
                            let mut resolved = Vec::new();
                            let mut lines = breakpoint_lines.lock().expect("lock");
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
        can_condition_breakpoints: true,
        can_list_stack: true,
        can_list_variables: true,
        ..Default::default()
    }
}

/// Connect the real peer backend to `peer` and complete the handshake.
fn live_backend(peer: &FakePeer) -> Box<dyn DebugBackend> {
    let mut backend = ExternalDebuggerPeerBackend::connect(peer.addr, Duration::from_secs(5))
        .expect("connect to fake peer");
    backend.initialize(InitializeBackendParams::default()).expect("handshake");
    Box::new(backend)
}

fn find_event<'a>(msgs: &'a [DapMessage], name: &str) -> Option<&'a DapMessage> {
    msgs.iter().find(|m| matches!(m, DapMessage::Event { event, .. } if event == name))
}

fn as_response(msg: &DapMessage) -> (&str, bool, Option<&serde_json::Value>) {
    match msg {
        DapMessage::Response { command, success, body, .. } => {
            (command.as_str(), *success, body.as_ref())
        }
        other => panic!("expected response, got {other:?}"),
    }
}

#[test]
fn dap_external_peer_launch_queues_breakpoints_before_handshake() {
    // No peer connected yet: setBreakpoints must be queued and answered with an
    // unverified `pending` response, not sent anywhere or dropped.
    let mut bridge = MirrorPeerBridge::new_pending(ControlMode::Mirror);
    let init = bridge.dispatch(1, "initialize", Some(serde_json::json!({ "adapterID": "perl" })));
    // Static conservative capabilities are advertised before any peer exists.
    let caps = as_response(&init[0]).2.expect("caps");
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
    let (_, ok, body) = as_response(&out[0]);
    assert!(ok, "a queued setBreakpoints still returns success");
    let bps = body.expect("body")["breakpoints"].as_array().expect("array").clone();
    assert_eq!(bps.len(), 2, "response matches the request positionally");
    assert_eq!(bps[0]["verified"], false, "queued breakpoints are unverified until flush");
    assert_eq!(bps[0]["line"], 42);
}

#[test]
fn dap_external_peer_launch_flushes_breakpoints_after_hello() {
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
    assert!(peer.breakpoint_lines.lock().expect("lock").is_empty());

    // The peer handshakes; flush sends the queued breakpoints to it.
    let backend = live_backend(&peer);
    let flush = bridge.go_live(backend);
    assert!(bridge.is_live());
    assert_eq!(bridge.pending_source_count(), 0, "queue is drained after flush");

    // The peer actually received the queued breakpoint over the wire.
    assert_eq!(
        *peer.breakpoint_lines.lock().expect("lock"),
        vec![42],
        "the queued breakpoint reached the peer on flush"
    );

    // The flush surfaces the resolved breakpoint as a `breakpoint` changed event.
    let changed = find_event(&flush, "breakpoint").expect("breakpoint changed event");
    if let DapMessage::Event { body: Some(b), .. } = changed {
        assert_eq!(b["reason"], "changed");
        assert_eq!(b["breakpoint"]["verified"], true);
        assert_eq!(b["breakpoint"]["line"], 42);
    } else {
        panic!("breakpoint event had no body");
    }

    drop(bridge);
    let _ = peer.handle.join();
}

#[test]
fn dap_external_peer_stopped_event_reaches_dap_client() {
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
    let stopped = find_event(&acc, "stopped").expect("peer stopped surfaced as a DAP stopped");
    if let DapMessage::Event { body: Some(b), .. } = stopped {
        assert_eq!(b["reason"], "breakpoint");
        assert_eq!(b["threadId"], 1);
        assert_eq!(b["allThreadsStopped"], true);
    } else {
        panic!("stopped event had no body");
    }

    drop(bridge);
    let _ = peer.handle.join();
}

#[test]
fn dap_external_peer_output_event_reaches_dap_client() {
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
    let output = find_event(&acc, "output").expect("peer output surfaced as a DAP output event");
    if let DapMessage::Event { body: Some(b), .. } = output {
        assert_eq!(b["category"], "stderr");
        assert_eq!(b["output"], "boom\n");
    } else {
        panic!("output event had no body");
    }

    drop(bridge);
    let _ = peer.handle.join();
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
fn dap_external_peer_rejects_control_in_mirror_mode() {
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
        let (rcmd, ok, _) = as_response(&out[0]);
        assert_eq!(rcmd, cmd);
        assert!(!ok, "{cmd} must be rejected while in mirror mode");
        if let DapMessage::Response { message, .. } = &out[0] {
            let msg = message.as_deref().unwrap_or("");
            assert!(msg.contains("mirror mode"), "rejection must explain mirror mode: {msg}");
        } else {
            panic!("expected a response for {cmd}");
        }
    }

    drop(bridge);
    let _ = peer.handle.join();
}
