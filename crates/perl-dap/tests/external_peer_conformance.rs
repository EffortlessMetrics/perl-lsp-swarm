//! End-to-end conformance harness for the external debugger peer seam.
//!
//! A self-contained *fake ptkdb peer* drives [`ExternalDebuggerPeerBackend`]
//! through the [Perl Debugger Peer Protocol](perl_dap::peer_protocol): handshake,
//! events (stopped/output/sourceFacts), and request/response round-trips
//! (stackTrace/scopes/variables/evaluate). This locks the host side down before
//! any real ptkdb change is requested.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use perl_dap::backend::external_peer::ExternalDebuggerPeerBackend;
use perl_dap::backend::{
    DebugBackend, EvaluateContext, EvaluateParams, InitializeBackendParams, StackTraceParams,
};
use perl_dap::model::{DebugEvent, FrameId, OutputCategory, StopReason, ThreadId, VariablesRef};
use perl_dap::peer_protocol::message::{
    PeerEvent, PeerMessage, PeerRequest, PeerResponse, command, event,
};
use perl_dap::peer_protocol::payloads::{
    EvaluateResponseBody, HelloArgs, ScopesResponseBody, StackTraceResponseBody,
    VariablesResponseBody, WireScope, WireSource, WireStackFrame, WireVariable,
};
use perl_dap::peer_protocol::{
    PROTOCOL_VERSION, PeerFrameDecoder, PeerReportedCapabilities, encode_message,
};

/// Directives the fake peer executes after the handshake, in order.
enum PeerStep {
    /// Emit an event to the host.
    Emit(PeerEvent),
    /// Register a response for a command (answered when that command arrives).
    Answer(&'static str, PeerResponse),
}

/// A fake ptkdb peer: listens, accepts one host connection, says hello, then
/// runs a script of steps + answers host requests.
struct FakePeer {
    handle: JoinHandle<()>,
    addr: std::net::SocketAddr,
}

impl FakePeer {
    fn start(caps: PeerReportedCapabilities, steps: Vec<PeerStep>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || run_peer(listener, caps, steps));
        FakePeer { handle, addr }
    }
}

fn run_peer(listener: TcpListener, caps: PeerReportedCapabilities, steps: Vec<PeerStep>) {
    let (stream, _) = listener.accept().expect("accept");
    let mut write = stream.try_clone().expect("clone");
    let mut read = stream;
    let mut seq = 500i64;

    let send = |w: &mut TcpStream, msg: &PeerMessage| {
        let _ = w.write_all(&encode_message(msg).expect("encode"));
        let _ = w.flush();
    };

    // Handshake: peer says hello.
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
                capabilities: caps,
            })
            .ok(),
        }),
    );

    // Partition steps into events (fire now) and answers (fire on request).
    let mut answers: Vec<(&'static str, PeerResponse)> = Vec::new();
    for step in steps {
        match step {
            PeerStep::Emit(mut ev) => {
                seq += 1;
                ev.seq = seq;
                send(&mut write, &PeerMessage::Event(ev));
            }
            PeerStep::Answer(cmd, resp) => answers.push((cmd, resp)),
        }
    }

    // Serve host requests.
    let mut decoder = PeerFrameDecoder::new();
    let mut buf = [0u8; 4096];
    read.set_read_timeout(Some(Duration::from_millis(500))).ok();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match read.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                decoder.push(&buf[..n]);
                while let Ok(Some(msg)) = decoder.try_next() {
                    if let PeerMessage::Request(req) = msg {
                        if req.command == command::HELLO {
                            continue;
                        }
                        if let Some((_, tmpl)) = answers.iter().find(|(c, _)| *c == req.command) {
                            seq += 1;
                            let mut resp = tmpl.clone();
                            resp.seq = seq;
                            resp.request_seq = req.seq;
                            resp.command = req.command.clone();
                            send(&mut write, &PeerMessage::Response(resp));
                        }
                        if req.command == command::GOODBYE {
                            return;
                        }
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
        can_report_subroutines: true,
        can_report_breakable_lines: true,
        ..Default::default()
    }
}

fn ok_resp(body: Option<serde_json::Value>) -> PeerResponse {
    PeerResponse {
        seq: 0,
        request_seq: 0,
        success: true,
        command: String::new(),
        message: None,
        body,
    }
}

fn connect(peer: &FakePeer) -> ExternalDebuggerPeerBackend {
    ExternalDebuggerPeerBackend::connect(peer.addr, Duration::from_secs(5)).expect("connect")
}

/// Poll `drain_events` until it yields something or a deadline passes.
fn wait_for_event(backend: &mut ExternalDebuggerPeerBackend) -> Vec<DebugEvent> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let evs = backend.drain_events();
        if !evs.is_empty() || Instant::now() >= deadline {
            return evs;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn stopped_event_becomes_model_event() {
    let stopped = PeerEvent {
        seq: 0,
        event: event::STOPPED.to_string(),
        body: serde_json::to_value(perl_dap::peer_protocol::payloads::StoppedEventBody {
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
    let peer = FakePeer::start(full_caps(), vec![PeerStep::Emit(stopped)]);
    let mut backend = connect(&peer);
    backend.initialize(InitializeBackendParams::default()).expect("handshake");

    let events = wait_for_event(&mut backend);
    let stopped =
        events.iter().find(|e| matches!(e, DebugEvent::Stopped { .. })).expect("a stopped event");
    match stopped {
        DebugEvent::Stopped { reason, thread_id, position } => {
            assert_eq!(*reason, StopReason::Breakpoint);
            assert_eq!(*thread_id, ThreadId(1));
            let pos = position.as_ref().expect("position");
            assert_eq!(pos.line, 42);
        }
        _ => unreachable!(),
    }
    drop(backend);
    let _ = peer.handle.join();
}

#[test]
fn output_event_forwards_category_and_text() {
    let out = PeerEvent {
        seq: 0,
        event: event::OUTPUT.to_string(),
        body: serde_json::to_value(perl_dap::peer_protocol::payloads::OutputEventBody {
            category: "stderr".to_string(),
            output: "boom\n".to_string(),
        })
        .ok(),
    };
    let peer = FakePeer::start(full_caps(), vec![PeerStep::Emit(out)]);
    let mut backend = connect(&peer);
    backend.initialize(InitializeBackendParams::default()).expect("handshake");
    let events = wait_for_event(&mut backend);
    let found = events.iter().any(|e| {
        matches!(e, DebugEvent::Output { category, output }
            if *category == OutputCategory::Stderr && output == "boom\n")
    });
    assert!(found, "output event forwarded: {events:?}");
    drop(backend);
    let _ = peer.handle.join();
}

#[test]
fn stack_scopes_variables_evaluate_round_trip() {
    let stack_body = StackTraceResponseBody {
        stack_frames: vec![WireStackFrame {
            id: 1,
            name: "main::run".to_string(),
            source: WireSource {
                path: "/work/script.pl".to_string(),
                name: Some("script.pl".to_string()),
                source_reference: None,
            },
            line: 10,
            column: 1,
        }],
    };
    let scopes_body = ScopesResponseBody {
        scopes: vec![WireScope {
            name: "Locals".to_string(),
            variables_reference: 1000,
            expensive: false,
        }],
    };
    let vars_body = VariablesResponseBody {
        variables: vec![WireVariable {
            name: "$x".to_string(),
            value: "42".to_string(),
            type_name: Some("scalar".to_string()),
            variables_reference: 0,
            indexed_variables: None,
            named_variables: None,
        }],
    };
    let eval_body = EvaluateResponseBody {
        result: "84".to_string(),
        type_name: Some("scalar".to_string()),
        variables_reference: 0,
    };

    let peer = FakePeer::start(
        full_caps(),
        vec![
            PeerStep::Answer(command::STACK_TRACE, ok_resp(serde_json::to_value(stack_body).ok())),
            PeerStep::Answer(command::SCOPES, ok_resp(serde_json::to_value(scopes_body).ok())),
            PeerStep::Answer(command::VARIABLES, ok_resp(serde_json::to_value(vars_body).ok())),
            PeerStep::Answer(command::EVALUATE, ok_resp(serde_json::to_value(eval_body).ok())),
        ],
    );
    let mut backend = connect(&peer);
    backend.initialize(InitializeBackendParams::default()).expect("handshake");

    let frames = backend
        .stack_trace(StackTraceParams { thread_id: ThreadId(1), start_frame: None, levels: None })
        .expect("stack trace");
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].name, "main::run");
    assert_eq!(frames[0].line, 10);

    let scopes = backend.scopes(FrameId(1)).expect("scopes");
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].variables_reference, VariablesRef(1000));

    let vars = backend.variables(VariablesRef(1000)).expect("variables");
    assert_eq!(vars.len(), 1);
    assert_eq!(vars[0].name, "$x");
    assert_eq!(vars[0].value, "42");

    let eval = backend
        .evaluate(EvaluateParams {
            expression: "$x * 2".to_string(),
            frame_id: Some(FrameId(1)),
            context: EvaluateContext::Watch,
        })
        .expect("evaluate");
    assert_eq!(eval.result, "84");

    drop(backend);
    let _ = peer.handle.join();
}

#[test]
fn source_facts_event_is_translated() {
    let facts = PeerEvent {
        seq: 0,
        event: event::SOURCE_FACTS.to_string(),
        body: serde_json::to_value(perl_dap::peer_protocol::payloads::SourceFactsEventBody {
            source: WireSource {
                path: "/work/script.pl".to_string(),
                name: None,
                source_reference: None,
            },
            breakable_lines: vec![7, 8, 12],
            subroutines: vec![],
        })
        .ok(),
    };
    let peer = FakePeer::start(full_caps(), vec![PeerStep::Emit(facts)]);
    let mut backend = connect(&peer);
    backend.initialize(InitializeBackendParams::default()).expect("handshake");
    let events = wait_for_event(&mut backend);
    let found = events.iter().any(|e| {
        matches!(e, DebugEvent::SourceFacts { facts, .. }
            if facts.breakable_line_candidates == vec![7, 8, 12])
    });
    assert!(found, "source facts translated: {events:?}");
    drop(backend);
    let _ = peer.handle.join();
}

#[test]
fn peer_crash_surfaces_as_error_not_hang() {
    // Peer says hello, then immediately drops the connection.
    let peer = FakePeer::start(full_caps(), vec![]);
    let mut backend = ExternalDebuggerPeerBackend::connect(peer.addr, Duration::from_millis(500))
        .expect("connect");
    backend.initialize(InitializeBackendParams::default()).expect("handshake");
    // Let the peer thread finish and close the socket.
    let _ = peer.handle.join();

    // A subsequent request must return an error rather than block forever.
    let err = backend
        .stack_trace(StackTraceParams { thread_id: ThreadId(1), start_frame: None, levels: None })
        .expect_err("peer gone");
    // Either NotConnected (reader saw EOF) or Timeout (race) is acceptable.
    let msg = format!("{err}");
    assert!(
        matches!(
            err,
            perl_dap::backend::BackendError::NotConnected
                | perl_dap::backend::BackendError::Timeout(_)
        ),
        "unexpected error: {msg}"
    );
}

fn output_event(text: String) -> PeerEvent {
    PeerEvent {
        seq: 0,
        event: event::OUTPUT.to_string(),
        body: serde_json::to_value(perl_dap::peer_protocol::payloads::OutputEventBody {
            category: "stdout".to_string(),
            output: text,
        })
        .ok(),
    }
}

fn stopped_event(line: u32) -> PeerEvent {
    PeerEvent {
        seq: 0,
        event: event::STOPPED.to_string(),
        body: serde_json::to_value(perl_dap::peer_protocol::payloads::StoppedEventBody {
            reason: "breakpoint".to_string(),
            thread_id: 1,
            source: Some(WireSource {
                path: "/work/flood.pl".to_string(),
                name: Some("flood.pl".to_string()),
                source_reference: None,
            }),
            line: Some(line),
            column: Some(1),
        })
        .ok(),
    }
}

fn terminated_event() -> PeerEvent {
    PeerEvent {
        seq: 0,
        event: event::TERMINATED.to_string(),
        body: serde_json::to_value(perl_dap::peer_protocol::payloads::TerminatedEventBody {
            exit_code: Some(0),
        })
        .ok(),
    }
}

fn collect_until_terminated(
    backend: &mut ExternalDebuggerPeerBackend,
    timeout: Duration,
) -> Vec<DebugEvent> {
    let deadline = Instant::now() + timeout;
    let mut events = Vec::new();
    loop {
        events.extend(backend.drain_events());
        if events.iter().any(|event| matches!(event, DebugEvent::Terminated { .. }))
            || Instant::now() >= deadline
        {
            return events;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn output_flood_is_bounded_and_preserves_stop_and_termination() {
    let mut steps = (0..2_000)
        .map(|index| PeerStep::Emit(output_event(format!("peer line {index}\n"))))
        .collect::<Vec<_>>();
    steps.push(PeerStep::Emit(stopped_event(42)));
    steps.push(PeerStep::Emit(terminated_event()));

    let peer = FakePeer::start(full_caps(), steps);
    let mut backend = connect(&peer);
    backend.initialize(InitializeBackendParams::default()).expect("handshake");
    let events = collect_until_terminated(&mut backend, Duration::from_secs(5));

    // The helper drains repeatedly while the peer is still sending, so the
    // aggregate delivered count may exceed one queue window. The unit buffer
    // tests own the retained-at-once count/byte assertion; this real transport
    // test proves that a sustained flood is degraded observably without losing
    // the stop or terminal transitions.
    assert!(
        events.iter().any(|event| {
            matches!(event, DebugEvent::Output { output, .. } if output.contains("event stream degraded"))
        }),
        "output loss must be observable: {events:?}"
    );
    assert!(events.iter().any(|event| matches!(event, DebugEvent::Stopped { .. })));
    assert!(events.iter().any(|event| matches!(event, DebugEvent::Terminated { .. })));

    drop(backend);
    let _ = peer.handle.join();
}

#[test]
fn critical_event_flood_closes_with_typed_resource_limit() {
    let steps = (0..400).map(|index| PeerStep::Emit(stopped_event(index + 1))).collect::<Vec<_>>();
    let peer = FakePeer::start(full_caps(), steps);
    let mut backend = connect(&peer);
    backend.initialize(InitializeBackendParams::default()).expect("handshake");

    // Do not drain while the peer is flooding critical events: draining is the
    // consumer backpressure relief path. This fixture deliberately models an
    // editor that never consumes events and waits for the bounded queue to close
    // the peer with a typed resource-limit outcome.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !backend.is_closed() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(backend.is_closed(), "critical overload must close the peer session");
    let events = backend.drain_events();
    assert!(events.iter().any(|event| matches!(event, DebugEvent::Terminated { .. })));
    assert!(events.iter().any(|event| {
        matches!(event, DebugEvent::Output { output, .. } if output.contains("event buffer exhausted"))
    }));

    let error = backend
        .stack_trace(StackTraceParams { thread_id: ThreadId(1), start_frame: None, levels: None })
        .expect_err("closed overloaded peer must reject later requests");
    assert!(matches!(error, perl_dap::backend::BackendError::ResourceLimit(_)));

    drop(backend);
    let _ = peer.handle.join();
}

// ---------------------------------------------------------------------------
// Session-level overload proofs
//
// The unit tests in `backend::external_peer::event_buffer` own the retention
// policy in isolation. The fixtures below own the *session* contract that the
// policy is supposed to deliver over a real socket: what an in-flight request
// observes, whether teardown completes while the peer is still writing, whether
// consecutive overloaded sessions release their reader and socket, how the
// aggregate byte envelope behaves under legal-but-large frames, and that the
// terminal transition stays unique when overload races peer EOF.
// ---------------------------------------------------------------------------

/// What a flooding fake peer observed before it stopped writing.
struct FloodReport {
    /// Events successfully written to the host.
    sent: usize,
    /// The peer observed the host actually release the socket: a read returned
    /// EOF or a connection-reset error. A write that merely hit this fixture's
    /// backpressure timeout does *not* set this, so tests can distinguish "the
    /// editor stopped draining" from "the session was torn down".
    saw_host_close: bool,
}

/// A fake peer that completes the handshake and then writes events as fast as
/// the transport accepts them.
///
/// Unlike [`FakePeer`] it never answers a host request, which is what makes an
/// in-flight host request genuinely pending, and it bounds its own writes with a
/// timeout so a host that stops reading cannot wedge the test thread.
struct FloodPeer {
    handle: JoinHandle<FloodReport>,
    addr: std::net::SocketAddr,
}

impl FloodPeer {
    /// Flood, then hold the socket open until the host closes it.
    fn start(
        caps: PeerReportedCapabilities,
        max_events: usize,
        gap: Duration,
        factory: impl Fn(usize) -> PeerEvent + Send + 'static,
    ) -> Self {
        Self::spawn(caps, max_events, gap, false, factory)
    }

    /// Flood, then drop the socket immediately so overload races peer EOF.
    fn start_then_close(
        caps: PeerReportedCapabilities,
        max_events: usize,
        factory: impl Fn(usize) -> PeerEvent + Send + 'static,
    ) -> Self {
        Self::spawn(caps, max_events, Duration::ZERO, true, factory)
    }

    fn spawn(
        caps: PeerReportedCapabilities,
        max_events: usize,
        gap: Duration,
        close_after_burst: bool,
        factory: impl Fn(usize) -> PeerEvent + Send + 'static,
    ) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            run_flood_peer(listener, caps, max_events, gap, close_after_burst, &factory)
        });
        FloodPeer { handle, addr }
    }
}

fn run_flood_peer(
    listener: TcpListener,
    caps: PeerReportedCapabilities,
    max_events: usize,
    gap: Duration,
    close_after_burst: bool,
    factory: &(impl Fn(usize) -> PeerEvent + Send + 'static),
) -> FloodReport {
    let (stream, _) = listener.accept().expect("accept");
    let mut write = stream.try_clone().expect("clone");
    let mut read = stream;
    // Bound every write so an editor that stops draining cannot block this
    // thread forever; backpressure ends the burst instead of wedging the test.
    write.set_write_timeout(Some(Duration::from_millis(500))).ok();
    let mut seq = 900i64;

    let hello = PeerMessage::Request(PeerRequest {
        seq,
        command: command::HELLO.to_string(),
        arguments: serde_json::to_value(HelloArgs {
            peer: "FloodPtkdb".to_string(),
            peer_version: Some("0.1".to_string()),
            protocol_version: PROTOCOL_VERSION.to_string(),
            token: None,
            capabilities: caps,
        })
        .ok(),
    });
    if write.write_all(&encode_message(&hello).expect("encode")).is_err() {
        return FloodReport { sent: 0, saw_host_close: false };
    }
    let _ = write.flush();

    let mut sent = 0usize;
    let burst_deadline = Instant::now() + Duration::from_secs(20);
    for index in 0..max_events {
        if Instant::now() >= burst_deadline {
            break;
        }
        let mut event = factory(index);
        seq += 1;
        event.seq = seq;
        let encoded = encode_message(&PeerMessage::Event(event)).expect("encode");
        if write.write_all(&encoded).is_err() {
            // Either the host went away or it stopped draining long enough to
            // hit the bounded write timeout. The burst is over either way, but
            // only the read side below can tell which happened.
            break;
        }
        let _ = write.flush();
        sent += 1;
        if !gap.is_zero() {
            std::thread::sleep(gap);
        }
    }

    if close_after_burst {
        return FloodReport { sent, saw_host_close: false };
    }

    // Hold the connection open and report whether the host released it.
    read.set_read_timeout(Some(Duration::from_millis(200))).ok();
    let mut buf = [0u8; 4096];
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut saw_host_close = false;
    while Instant::now() < deadline {
        match read.read(&mut buf) {
            Ok(0) => {
                saw_host_close = true;
                break;
            }
            Ok(_) => {}
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => {
                saw_host_close = true;
                break;
            }
        }
    }
    FloodReport { sent, saw_host_close }
}

/// A `debugger/sourceFacts` event whose breakable-line vector is a legal frame
/// (well under the 16 MiB Content-Length ceiling) but larger than the whole
/// event-retention byte envelope.
fn oversized_source_facts_event(lines: u32) -> PeerEvent {
    PeerEvent {
        seq: 0,
        event: event::SOURCE_FACTS.to_string(),
        body: serde_json::to_value(perl_dap::peer_protocol::payloads::SourceFactsEventBody {
            source: WireSource {
                path: "/work/huge.pl".to_string(),
                name: Some("huge.pl".to_string()),
                source_reference: None,
            },
            breakable_lines: (1..=lines).collect(),
            subroutines: vec![],
        })
        .ok(),
    }
}

/// Sum the bytes of every peer-originated output event in one drain, ignoring
/// the adapter's own console receipts.
fn peer_output_bytes(events: &[DebugEvent]) -> usize {
    events
        .iter()
        .filter_map(|event| match event {
            DebugEvent::Output { output, .. } if !output.starts_with("[perl-dap]") => {
                Some(output.len())
            }
            _ => None,
        })
        .sum()
}

fn terminated_count(events: &[DebugEvent]) -> usize {
    events.iter().filter(|event| matches!(event, DebugEvent::Terminated { .. })).count()
}

#[test]
fn pending_request_wakes_with_typed_resource_limit() {
    // Stopped events are critical: they cannot be evicted by lower-priority
    // traffic, so a peer emitting them faster than the editor drains eventually
    // exhausts the reviewed envelope. The 1ms gap keeps the session live long
    // enough for the request below to be genuinely in flight when it does.
    let peer = FloodPeer::start(full_caps(), 4_000, Duration::from_millis(1), |index| {
        stopped_event(u32::try_from(index).unwrap_or(u32::MAX).saturating_add(1))
    });
    let request_timeout = Duration::from_secs(30);
    let mut backend =
        ExternalDebuggerPeerBackend::connect(peer.addr, request_timeout).expect("connect");
    backend.initialize(InitializeBackendParams::default()).expect("handshake");
    assert!(!backend.is_closed(), "the session must still be live when the request is issued");

    let started = Instant::now();
    let error = backend
        .stack_trace(StackTraceParams { thread_id: ThreadId(1), start_frame: None, levels: None })
        .expect_err("overload must fail the in-flight request");
    let elapsed = started.elapsed();

    // Two falsifiers this pins down: `Timeout` would mean the pending request
    // slept to its own deadline instead of waking on the terminal transition,
    // and `NotConnected` would mean the typed overload cause was lost when the
    // reader tore the session down.
    assert!(
        matches!(error, perl_dap::backend::BackendError::ResourceLimit(_)),
        "in-flight request must observe the typed overload cause, got: {error}"
    );
    assert!(
        elapsed < request_timeout / 2,
        "pending request must wake on the overload transition, not time out (waited {elapsed:?})"
    );

    drop(backend);
    let _ = peer.handle.join();
}

#[test]
fn disconnect_and_drop_complete_while_peer_still_writes() {
    let peer = FloodPeer::start(full_caps(), 50_000, Duration::ZERO, |index| {
        output_event(format!("flood line {index}\n"))
    });
    let mut backend =
        ExternalDebuggerPeerBackend::connect(peer.addr, Duration::from_secs(5)).expect("connect");
    backend.initialize(InitializeBackendParams::default()).expect("handshake");
    // Let the peer get well ahead of the editor before teardown starts.
    assert!(!wait_for_event(&mut backend).is_empty(), "peer must be streaming before teardown");

    let started = Instant::now();
    backend.disconnect(false).expect("disconnect while the peer is still writing");
    let disconnect_elapsed = started.elapsed();
    assert!(backend.is_closed(), "disconnect must mark the session closed");

    let drop_started = Instant::now();
    drop(backend);
    let drop_elapsed = drop_started.elapsed();

    assert!(
        disconnect_elapsed < Duration::from_secs(5),
        "disconnect must not wait on a writing peer (took {disconnect_elapsed:?})"
    );
    // `Drop` joins the reader thread; the reader's bounded socket read timeout
    // and its closed-flag check are what keep that join from depending on peer
    // behaviour.
    assert!(
        drop_elapsed < Duration::from_secs(5),
        "Drop must join the reader thread promptly (took {drop_elapsed:?})"
    );

    let report = peer.handle.join().expect("peer thread");
    assert!(report.sent > 0, "fixture must actually have flooded the host");
    assert!(report.saw_host_close, "teardown must release the peer socket");
}

#[test]
fn repeated_overload_sessions_release_reader_and_socket() {
    // Retained bytes/events live on the backend, not in a process-wide cell.
    // Three consecutive overloaded sessions must each close on their own
    // accounting and leave nothing behind for the next one.
    for session in 0..3 {
        let peer = FloodPeer::start(full_caps(), 4_000, Duration::ZERO, |index| {
            stopped_event(u32::try_from(index).unwrap_or(u32::MAX).saturating_add(1))
        });
        let mut backend = ExternalDebuggerPeerBackend::connect(peer.addr, Duration::from_secs(5))
            .expect("connect");
        backend.initialize(InitializeBackendParams::default()).expect("handshake");

        let deadline = Instant::now() + Duration::from_secs(15);
        while !backend.is_closed() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(backend.is_closed(), "session {session} must close under critical overload");

        let events = backend.drain_events();
        assert!(
            events.iter().any(|event| matches!(event, DebugEvent::Terminated { .. })),
            "session {session} must surface a terminal transition: {events:?}"
        );
        // A second drain proves the retained envelope was released rather than
        // carried forward: the reader has already returned, so anything still
        // queued here would be residue.
        let residue = backend.drain_events();
        assert!(
            residue.is_empty(),
            "session {session} retained residue after draining: {residue:?}"
        );

        let teardown = Instant::now();
        drop(backend);
        assert!(
            teardown.elapsed() < Duration::from_secs(5),
            "session {session} teardown must not hang"
        );

        let report = peer.handle.join().expect("peer thread");
        assert!(report.saw_host_close, "session {session} must release the peer socket");
    }
}

#[test]
fn aggregate_byte_envelope_bounds_legal_large_frames() {
    // Each chunk is individually legal and individually *under* the per-chunk
    // output ceiling, so nothing here is truncated in place. The only mechanism
    // that can produce loss is aggregate byte accounting across retained
    // events: 48 x 32 KiB is roughly 1.5 MiB against a 1 MiB envelope, while 48
    // events stays far below the 256-event count limit.
    const CHUNK_BYTES: usize = 32 * 1024;
    let mut steps = (0..48)
        .map(|index| {
            PeerStep::Emit(output_event(format!("{index:04}:{}\n", "x".repeat(CHUNK_BYTES))))
        })
        .collect::<Vec<_>>();
    steps.push(PeerStep::Emit(stopped_event(7)));
    steps.push(PeerStep::Emit(terminated_event()));

    let peer = FakePeer::start(full_caps(), steps);
    let mut backend = connect(&peer);
    backend.initialize(InitializeBackendParams::default()).expect("handshake");

    // Deliberately do not drain while the burst is in flight: draining is the
    // backpressure relief path, and relieving it would hide the envelope.
    std::thread::sleep(Duration::from_secs(2));
    let events = backend.drain_events();

    assert!(
        events.iter().any(|event| matches!(event, DebugEvent::Terminated { .. })),
        "the whole burst must have been processed: {} event(s)",
        events.len()
    );
    assert!(
        events.iter().any(|event| matches!(event, DebugEvent::Stopped { .. })),
        "the critical stop must survive aggregate byte pressure"
    );

    let receipt = events
        .iter()
        .find_map(|event| match event {
            DebugEvent::Output { output, .. } if output.contains("event stream degraded") => {
                Some(output.clone())
            }
            _ => None,
        })
        .expect("aggregate byte pressure must produce an observable loss receipt");
    // Nothing was truncated in place, so the receipt must attribute the loss to
    // whole evicted output events rather than to per-chunk truncation.
    assert!(
        !receipt.contains("0 output event(s) dropped"),
        "byte-driven eviction must be reported as dropped events: {receipt}"
    );

    let retained = peer_output_bytes(&events);
    assert!(
        retained <= 1024 * 1024,
        "retained peer output ({retained} bytes) must stay inside the 1 MiB envelope"
    );

    drop(backend);
    let _ = peer.handle.join();
}

#[test]
fn oversized_state_frame_closes_session_with_typed_resource_limit() {
    // A single legal frame whose retained cost exceeds the whole envelope. It
    // is not loss-tolerant, so the session must fail closed with a typed cause
    // rather than silently drop the state event.
    let peer =
        FakePeer::start(full_caps(), vec![PeerStep::Emit(oversized_source_facts_event(400_000))]);
    let mut backend =
        ExternalDebuggerPeerBackend::connect(peer.addr, Duration::from_secs(5)).expect("connect");
    backend.initialize(InitializeBackendParams::default()).expect("handshake");

    let deadline = Instant::now() + Duration::from_secs(15);
    while !backend.is_closed() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(backend.is_closed(), "an unrepresentable state event must close the session");

    let events = backend.drain_events();
    assert!(
        events.iter().any(|event| matches!(
            event,
            DebugEvent::Output { output, .. } if output.contains("exceeding")
        )),
        "the overload cause must be observable on the event stream: {events:?}"
    );
    assert_eq!(terminated_count(&events), 1, "exactly one terminal transition: {events:?}");

    let error = backend
        .stack_trace(StackTraceParams { thread_id: ThreadId(1), start_frame: None, levels: None })
        .expect_err("a closed overloaded session must reject later requests");
    assert!(
        matches!(error, perl_dap::backend::BackendError::ResourceLimit(_)),
        "unexpected error: {error}"
    );

    drop(backend);
    let _ = peer.handle.join();
}

#[test]
fn post_terminal_peer_traffic_is_never_exposed() {
    let mut steps = vec![PeerStep::Emit(stopped_event(3)), PeerStep::Emit(terminated_event())];
    steps.extend(
        (0..200).map(|index| PeerStep::Emit(output_event(format!("after terminal {index}\n")))),
    );

    let peer = FakePeer::start(full_caps(), steps);
    let mut backend = connect(&peer);
    backend.initialize(InitializeBackendParams::default()).expect("handshake");
    let mut events = collect_until_terminated(&mut backend, Duration::from_secs(10));
    assert!(
        events.iter().any(|event| matches!(event, DebugEvent::Terminated { .. })),
        "fixture must reach the terminal transition: {events:?}"
    );

    // Give the peer's post-terminal burst time to land in the reader, then prove
    // no later drain exposes any of it.
    std::thread::sleep(Duration::from_secs(1));
    let late = backend.drain_events();
    events.extend(late);
    assert!(
        !events.iter().any(|event| matches!(
            event,
            DebugEvent::Output { output, .. } if output.contains("after terminal")
        )),
        "post-terminal peer traffic must not surface in any drain: {events:?}"
    );
    assert_eq!(
        terminated_count(&events),
        1,
        "the terminal transition must stay unique: {events:?}"
    );

    drop(backend);
    let _ = peer.handle.join();
}

#[test]
fn overload_racing_peer_eof_yields_one_ordered_terminal_transition() {
    // The peer drops its socket the instant the burst is written, so the
    // buffer's resource-limit close races the reader's EOF path. The burst is
    // an order of magnitude larger than the 256-event envelope and the reader
    // decodes frames in order, so overload is reached well before EOF: the
    // editor must see the notice first and exactly one terminal transition.
    let peer = FloodPeer::start_then_close(full_caps(), 1_000, |index| {
        stopped_event(u32::try_from(index).unwrap_or(u32::MAX).saturating_add(1))
    });
    let mut backend =
        ExternalDebuggerPeerBackend::connect(peer.addr, Duration::from_secs(5)).expect("connect");
    backend.initialize(InitializeBackendParams::default()).expect("handshake");

    let deadline = Instant::now() + Duration::from_secs(15);
    while !backend.is_closed() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(backend.is_closed(), "overload racing EOF must still reach a terminal state");

    let mut events = backend.drain_events();
    std::thread::sleep(Duration::from_millis(500));
    events.extend(backend.drain_events());

    let notice_index = events
        .iter()
        .position(|event| {
            matches!(event, DebugEvent::Output { output, .. }
            if output.contains("event buffer exhausted"))
        })
        .expect("overload must publish its typed cause before the terminal transition");
    let terminal_index = events
        .iter()
        .position(|event| matches!(event, DebugEvent::Terminated { .. }))
        .expect("a resource-limit notice must be followed by a terminal transition");
    assert!(
        notice_index < terminal_index,
        "the overload notice must precede the terminal transition: {events:?}"
    );
    assert_eq!(
        terminated_count(&events),
        1,
        "overload racing EOF must expose exactly one terminal transition: {events:?}"
    );

    drop(backend);
    let _ = peer.handle.join();
}
