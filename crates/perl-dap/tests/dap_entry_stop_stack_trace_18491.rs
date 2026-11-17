//! End-to-end regression for #18491: stackTrace at the entry stop and the
//! reason published by the follow-up breakpoint stop.
//!
//! Drives the adapter in-process against a real `perl -d` session and mirrors
//! the probe ordering from the issue — launch(stopOnEntry) → setBreakpoints →
//! configurationDone — so the engine-breakpoint acknowledgement query overlaps
//! the reader's entry-stop publication window. On the pre-fix adapter that
//! overlap let the acknowledgement's state restore clobber the published entry
//! stop, and every stackTrace at the entry stop answered an empty frame list.

#![expect(
    clippy::print_stderr,
    reason = "Integration-test diagnostic and skip output; tracing is not the harness logger."
)]
use perl_dap::debug_adapter::DapMessageWithEpoch;
use perl_dap::{DapMessage, DebugAdapter};
use perl_tdd_support::{must_err_with, must_with};
use serde_json::{Value, json};
use std::fs::write;
use std::sync::mpsc::{Receiver, sync_channel};
use std::time::{Duration, Instant};
use tempfile::tempdir;

mod common;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn perl_available() -> bool {
    common::debuggee_perl_or_typed_skip("dap_entry_stop_stack_trace_18491").is_some()
}

fn timeout() -> Duration {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some()
        || std::env::var_os("CARGO_LLVM_COV").is_some()
    {
        Duration::from_mins(1)
    } else {
        Duration::from_secs(15)
    }
}

fn wait_for_event(
    rx: &Receiver<DapMessageWithEpoch>,
    event_name: &str,
    bound: Duration,
) -> Result<DapMessage, String> {
    common::wait_for_event(rx, event_name, bound)
}

fn response_success(response: DapMessage, command: &str) -> Result<Option<Value>, String> {
    match response {
        DapMessage::Response { success, command: actual, body, message, .. } => {
            if actual != command {
                return Err(format!("expected `{command}` response, got `{actual}`"));
            }
            if !success {
                return Err(format!(
                    "command `{command}` failed: {}",
                    message.unwrap_or_else(|| "<no message>".to_string())
                ));
            }
            Ok(body)
        }
        _ => Err(format!("expected response message for `{command}`")),
    }
}

fn event_body(message: &DapMessage, event_name: &str) -> Result<Value, String> {
    match message {
        DapMessage::Event { event, body: Some(body), .. } if event == event_name => {
            Ok(body.clone())
        }
        other => Err(format!("expected `{event_name}` event, got: {other:?}")),
    }
}

#[test]
fn entry_stop_serves_stack_trace_and_breakpoint_stop_reports_breakpoint() -> TestResult {
    if !perl_available() {
        eprintln!("Skipping entry-stop stackTrace e2e - perl executable is not available");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script_path = workspace.path().join("entry_stop_18491.pl");
    write(
        &script_path,
        r#"use strict;
use warnings;
my $x = 1;
$x++;
print "$x\n";
"#,
    )?;
    let script_path_str = script_path
        .to_str()
        .ok_or("script path could not be converted to UTF-8 string")?
        .to_string();
    let bound = timeout();

    let mut adapter = DebugAdapter::new();
    let (tx, rx) = sync_channel(64);
    adapter.set_event_sender(tx);
    // This flow exercises entry-stop debugging behavior, not the launch
    // authority contract; without an installed authority the launch is
    // refused before it reaches the interpreter (#8656).
    common::install_unbounded_test_authority(&adapter);

    let _initialize =
        response_success(adapter.handle_request(1, "initialize", None), "initialize")?;
    let _initialized = wait_for_event(&rx, "initialized", bound)?;

    let perl_path = common::resolve_launch_perl_path()
        .map_err(|reason| format!("could not resolve the launch interpreter: {reason}"))?
        .ok_or("the availability gate resolved no pipe-capable launch interpreter")?;

    response_success(
        adapter.handle_request(
            2,
            "launch",
            Some(json!({
                "program": script_path_str,
                "args": [],
                "stopOnEntry": true,
                "perlPath": perl_path.to_string_lossy(),
                "env": {
                    "PERL_PERTURB_KEYS": "0",
                    "PERL_HASH_SEED": "0",
                    "LC_ALL": "C",
                    "TZ": "UTC"
                }
            })),
        ),
        "launch",
    )?;

    // Issue ordering: breakpoints are admitted while the entry stop is still
    // materializing, so the engine acknowledgement framed query overlaps the
    // reader's entry-stop publication window (#18491).
    let breakpoint_body = response_success(
        adapter.handle_request(
            3,
            "setBreakpoints",
            Some(json!({
                "source": { "path": script_path_str },
                "breakpoints": [ { "line": 4 } ]
            })),
        ),
        "setBreakpoints",
    )?;
    let verified = breakpoint_body
        .as_ref()
        .and_then(|body| body.get("breakpoints"))
        .and_then(Value::as_array)
        .ok_or("setBreakpoints returned no breakpoints array")?;
    assert!(
        verified.iter().any(|bp| bp.get("verified").and_then(Value::as_bool) == Some(true)),
        "expected at least one verified breakpoint, got: {breakpoint_body:?}"
    );

    response_success(adapter.handle_request(4, "configurationDone", None), "configurationDone")?;

    let entry_stop = wait_for_event(&rx, "stopped", bound)?;
    let entry_body = event_body(&entry_stop, "stopped")?;
    let entry_reason = entry_body.get("reason").and_then(Value::as_str).unwrap_or("");
    assert!(
        matches!(entry_reason, "entry" | "step"),
        "expected the initial stopped reason `entry` or `step`, got: {entry_body:?}"
    );
    let thread_id = entry_body
        .get("threadId")
        .and_then(Value::as_i64)
        .ok_or("stopped event carried no threadId")?;

    // (a) stackTrace at the entry stop must serve at least one frame, and the
    // frame must name the debuggee source rather than a fabricated placeholder.
    let mut entry_frames: Option<Value> = None;
    let mut attempts = 0;
    let deadline = Instant::now() + Duration::from_secs(6);
    while entry_frames.is_none() {
        attempts += 1;
        assert!(
            Instant::now() < deadline,
            "stackTrace at the entry stop never materialized a frame (#18491)"
        );
        let stack = response_success(
            adapter.handle_request(
                10 + attempts as i64,
                "stackTrace",
                Some(json!({ "threadId": thread_id })),
            ),
            "stackTrace",
        )?;
        let frames = stack
            .as_ref()
            .and_then(|body| body.get("stackFrames"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if !frames.is_empty() {
            entry_frames = Some(Value::Array(frames));
        } else {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    let frames =
        entry_frames.ok_or("entry-stop stack frames were expected to be materialized above")?;
    let top = frames
        .get(0)
        .and_then(|frame| frame.get("source"))
        .and_then(|source| source.get("path"))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("entry-stop stack frame carried no source path: {frames:?}"))?;
    assert_eq!(
        std::path::Path::new(top),
        std::path::Path::new(&script_path_str),
        "entry-stop stack frame must point at the debuggee script, got: {frames:?}"
    );

    // (b) the stopped event after continue-to-breakpoint carries reason
    // `breakpoint`, never a stale `entry` label (#18491).
    response_success(
        adapter.handle_request(6, "continue", Some(json!({ "threadId": thread_id }))),
        "continue",
    )?;
    let breakpoint_stop = wait_for_event(&rx, "stopped", bound)?;
    let breakpoint_body = event_body(&breakpoint_stop, "stopped")?;
    assert_eq!(
        breakpoint_body.get("reason").and_then(Value::as_str),
        Some("breakpoint"),
        "the breakpoint stop must publish reason `breakpoint`, got: {breakpoint_body:?}"
    );
    let hit_ids = breakpoint_body
        .get("hitBreakpointIds")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(
        !hit_ids.is_empty(),
        "the breakpoint stop should identify the hit breakpoint, got: {breakpoint_body:?}"
    );

    response_success(adapter.handle_request(7, "disconnect", Some(json!({}))), "disconnect")?;
    let _terminated = wait_for_event(&rx, "terminated", bound)?;

    Ok(())
}

/// The wait helper must report the events it observed while waiting and
/// classify the stall into exactly one row, so a stalled `stopped` wait is
/// diagnosable instead of an opaque timeout (#19536).
///
/// The success path is asserted too: the awaited event's matching semantics are
/// unchanged by the diagnostic, and a wait that sees the awaited event still
/// returns it rather than reporting a timeout.
#[test]
fn wait_for_event_reports_observed_events_and_classifies_the_stall() {
    use perl_dap::debug_adapter::DrainEpoch;

    fn event(name: &str, body: Option<Value>) -> DapMessageWithEpoch {
        (DapMessage::Event { seq: 1, event: name.to_string(), body }, DrainEpoch::Global)
    }

    const BOUND: Duration = Duration::from_millis(250);

    // (a) Nothing observed: the adapter published no event in the wait window.
    let (_tx, rx) = sync_channel(64);
    let error = must_err_with(
        wait_for_event(&rx, "stopped", BOUND),
        "a wait with no observed events must time out",
    );
    assert!(error.contains("classification: nothing-published"), "{error}");
    assert!(error.contains("observed 0 event(s)"), "{error}");

    // (b) An `output` event was observed: the adapter consumed debuggee output
    // but published no awaited event.
    let (tx, rx) = sync_channel(64);
    must_with(
        tx.send(event("output", Some(json!({"category": "stderr", "output": "boom"})))),
        "send the observed output event",
    );
    let error = must_err_with(
        wait_for_event(&rx, "stopped", BOUND),
        "a wait without the awaited event must time out",
    );
    assert!(error.contains("classification: never-emitted"), "{error}");
    assert!(error.contains("observed 1 event(s)"), "{error}");
    assert!(error.contains("output(category=stderr"), "{error}");

    // (c) A `terminated` event preempted the awaited stop.
    let (tx, rx) = sync_channel(64);
    must_with(
        tx.send(event("terminated", Some(json!({"reason": "debugger_frame_limit"})))),
        "send the terminating event",
    );
    let error = must_err_with(
        wait_for_event(&rx, "stopped", BOUND),
        "a wait without the awaited event must time out",
    );
    assert!(error.contains("classification: preempted"), "{error}");
    assert!(error.contains("terminated(reason=debugger_frame_limit)"), "{error}");

    // (d) The outbound queue reached the channel capacity: the count alone
    // identifies the backpressure row.
    let (tx, rx) = sync_channel(64);
    for _ in 0..64 {
        must_with(
            tx.send(event("continued", None)),
            "send the events that fill the outbound queue",
        );
    }
    let error = must_err_with(
        wait_for_event(&rx, "stopped", BOUND),
        "a wait without the awaited event must time out",
    );
    assert!(error.contains("classification: backpressure"), "{error}");
    assert!(error.contains("observed 64 event(s)"), "{error}");

    // (e) The awaited event is still returned, unchanged: the diagnostic is
    // additive and never alters the matching semantics.
    let (tx, rx) = sync_channel(64);
    must_with(
        tx.send(event("output", Some(json!({"category": "stdout", "output": "hi"})))),
        "send the ignored output event",
    );
    must_with(
        tx.send(event("stopped", Some(json!({"reason": "entry"})))),
        "send the awaited event",
    );
    let message = must_with(
        wait_for_event(&rx, "stopped", BOUND),
        "the awaited event must still be returned",
    );
    assert!(matches!(message, DapMessage::Event { event, .. } if event == "stopped"));
}
