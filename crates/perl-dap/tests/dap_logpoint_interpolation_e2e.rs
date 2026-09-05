//! #9578 — the optional breakpoint fields must be refused per item on the
//! live stdio boundary, and the refused entries must never reach the engine.
//!
//! # What changed and why
//!
//! This suite previously proved the fail-open logpoint path end to end: a
//! `setBreakpoints` entry carrying `logMessage` installed against a real
//! `perl -d`, emitted interpolated `console` output, and continued without a
//! `stopped` event. #9578 floors that advertisement: `supportsLogPoints` (and
//! the conditional/hit-condition rows) are advertised false and every entry
//! carrying those fields is refused per item, because the install → hit →
//! correlated lookup → output → continue contract is not an accepted
//! capability receipt yet. The fail-open receipts the old tests pinned are no
//! longer reachable behavior; the interpolation machinery stays
//! store-level-tested (`interpolate_logpoint_message` unit coverage) and the
//! promotion path owns restoring live receipts (#9000 via #7366 evidence).
//!
//! These tests keep the live-session discipline the old suite established —
//! real `perl -d`, real stdio boundary — and discriminate the floor on it:
//!
//! * a refused `logMessage`/`condition`/`hitCondition` entry comes back
//!   `verified: false` with the exact capability-specific message, on a live
//!   initialized session (not only on an unlaunched adapter);
//! * after the refusal the debuggee runs to `terminated` with **no** `stopped`
//!   event and **no** simulated `console` output — a condition silently
//!   stripped into an installed unconditional breakpoint, a hitCondition
//!   counted locally, or a logMessage converted into an ordinary stopping
//!   breakpoint would each produce a visible stop and fail;
//! * a plain entry in the same request still installs and stops (the base
//!   source-breakpoint cell keeps its independent contract).

#![allow(clippy::print_stderr)]

mod common;

use common::{DapWorkflowSession, perl_available, workflow_timeout};
use perl_dap::debug_adapter::DapMessage;
use serde_json::{Value, json};
use std::fs::write;
use tempfile::tempdir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Shared executable location for the fixtures in this file.
///
/// The fixture is two `use` lines, a blank line, then scalar assignments on
/// lines 4 and 5, so line 6 is an executable statement after the scalars are
/// bound. Line 5 is the plain-entry control location.
const OPTIONAL_LINE: u64 = 6;
const PLAIN_LINE: u64 = 5;

fn optional_script() -> &'static str {
    "use strict;\nuse warnings;\n\nmy $x = 10;\nmy $y = $x + 5;\nmy $z = $x * $y;\nprint \"$z\\n\";\n"
}

/// Per-item floor refusal markers (#9578).
const CONDITION_FLOOR_MARKER: &str = "supportsConditionalBreakpoints";
const HIT_CONDITION_FLOOR_MARKER: &str = "supportsHitConditionalBreakpoints";
const LOG_MESSAGE_FLOOR_MARKER: &str = "supportsLogPoints";

/// Send `setBreakpoints` with raw entries and return the response breakpoints.
fn set_entries(
    session: &mut DapWorkflowSession,
    source_path: &str,
    entries: Value,
) -> Result<Vec<Value>, String> {
    let args = json!({
        "source": { "path": source_path },
        "breakpoints": entries,
    });
    let resp = session.request("setBreakpoints", Some(args));
    let body = session.expect_success(&resp, "setBreakpoints")?.ok_or("setBreakpoints body")?;
    body.get("breakpoints")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| "setBreakpoints response missing breakpoints array".to_string())
}

/// Assert one response entry is the floor refusal for `marker`.
fn assert_floor_refusal(entry: &Value, marker: &str, what: &str) -> Result<(), String> {
    let verified = entry.get("verified").and_then(Value::as_bool).unwrap_or(true);
    if verified {
        return Err(format!("{what}: entry must be unverified while the capability is floored"));
    }
    let message = entry
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{what}: refused entry must carry a message"))?;
    if !message.contains(marker) || !message.contains("#9578") {
        return Err(format!("{what}: expected the #9578 refusal naming {marker}, got {message:?}"));
    }
    Ok(())
}

/// Events collected until `terminated`, split by kind.
///
/// Note: the adapter labels the debugger control stream as DAP
/// `category="stdout"`; `console` lines are the adapter's own logpoint
/// messages. After a floored refusal neither a `stopped` event nor simulated
/// `console` output may appear before `terminated`.
struct RunToTermination {
    console: Vec<String>,
    saw_stopped: bool,
    saw_terminated: bool,
}

fn run_to_termination(session: &DapWorkflowSession) -> RunToTermination {
    let deadline = std::time::Instant::now() + session.timeout;
    let mut out =
        RunToTermination { console: Vec::new(), saw_stopped: false, saw_terminated: false };

    while std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let Ok(msg) = session.rx.recv_timeout(remaining) else {
            break;
        };
        let DapMessage::Event { event, body, .. } = &msg else {
            continue;
        };
        if event == "terminated" {
            out.saw_terminated = true;
            break;
        }
        if event == "stopped" {
            out.saw_stopped = true;
            continue;
        }
        if event != "output" {
            continue;
        }
        let Some(body) = body.as_ref() else {
            continue;
        };
        if body.get("category").and_then(Value::as_str) == Some("console")
            && let Some(text) = body.get("output").and_then(Value::as_str)
        {
            out.console.push(text.trim_end().to_string());
        }
    }

    out
}

/// A `logMessage` entry is refused per item on a live stdio session and the
/// debuggee runs to termination with no stop and no simulated output (#9578
/// tests 6 and 12: the logMessage is not converted into a stopping breakpoint
/// and the false path performs zero backend invocation).
#[test]
fn log_message_entry_is_refused_and_never_installs_on_live_session() -> TestResult {
    if !perl_available() {
        eprintln!("Skipping log_message_entry_is_refused_and_never_installs - perl not available");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("logpoint_floor_e2e.pl");
    write(&script, optional_script())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;

    let breakpoints = set_entries(
        &mut session,
        &script_str,
        json!([{ "line": OPTIONAL_LINE, "logMessage": "x is {$x}" }]),
    )
    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    assert_eq!(breakpoints.len(), 1, "one response per input");
    assert_floor_refusal(&breakpoints[0], LOG_MESSAGE_FLOOR_MARKER, "logMessage entry")
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    session.configuration_done()?;
    let run = run_to_termination(&session);

    assert!(run.saw_terminated, "debuggee must run to termination; the refusal must not hang it");
    assert!(
        !run.saw_stopped,
        "a refused logMessage entry must not stop execution; console was {:?}",
        run.console
    );
    assert!(
        run.console.is_empty(),
        "a refused logMessage entry must not simulate output; console was {:?}",
        run.console
    );

    session.disconnect()?;
    Ok(())
}

/// `condition` and `hitCondition` entries are refused per item on a live
/// session; neither is silently stripped into an installed unconditional
/// breakpoint (#9578 tests 4 and 5 on the live boundary).
#[test]
fn condition_and_hit_condition_entries_are_refused_and_never_install_on_live_session() -> TestResult
{
    if !perl_available() {
        eprintln!("Skipping condition_and_hit_condition_entries_are_refused - perl not available");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("condition_floor_e2e.pl");
    write(&script, optional_script())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;

    let breakpoints = set_entries(
        &mut session,
        &script_str,
        json!([
            { "line": OPTIONAL_LINE, "condition": "$x > 0" },
            { "line": OPTIONAL_LINE, "hitCondition": ">= 1" },
        ]),
    )
    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    assert_eq!(breakpoints.len(), 2, "one response per input, in order");
    assert_floor_refusal(&breakpoints[0], CONDITION_FLOOR_MARKER, "condition entry")
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    assert_floor_refusal(&breakpoints[1], HIT_CONDITION_FLOOR_MARKER, "hitCondition entry")
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    session.configuration_done()?;
    let run = run_to_termination(&session);

    assert!(run.saw_terminated, "debuggee must run to termination");
    assert!(
        !run.saw_stopped,
        "if the condition or hitCondition had been stripped and installed unconditionally, \
         the debuggee would have stopped at line {OPTIONAL_LINE}"
    );

    session.disconnect()?;
    Ok(())
}

/// A mixed request on a live session installs only the plain entry: the plain
/// control stops at its resolved line, and after continue the rejected
/// `logMessage` line never stops and never emits console output (#9578
/// tests 7 and 3, with the preserved base contract as the positive control).
#[test]
fn mixed_request_on_live_session_installs_only_the_plain_entry() -> TestResult {
    if !perl_available() {
        eprintln!("Skipping mixed_request_on_live_session - perl not available");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("mixed_floor_e2e.pl");
    write(&script, optional_script())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;

    let breakpoints = set_entries(
        &mut session,
        &script_str,
        json!([
            { "line": PLAIN_LINE },
            { "line": OPTIONAL_LINE, "logMessage": "z is {$z}" },
        ]),
    )
    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    assert_eq!(breakpoints.len(), 2);
    assert_eq!(
        breakpoints[0].get("verified").and_then(Value::as_bool),
        Some(true),
        "the plain entry keeps its independent contract"
    );
    assert_floor_refusal(&breakpoints[1], LOG_MESSAGE_FLOOR_MARKER, "logMessage entry")
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    session.configuration_done()?;

    // The plain entry stops at its requested line.
    let stopped = session.wait_stopped_with_frame()?;
    assert_eq!(stopped.reason, "breakpoint", "the plain entry must stop like a breakpoint");
    assert_eq!(
        stopped.line, PLAIN_LINE as i64,
        "the plain control must stop at its requested line"
    );

    // After continue, the rejected logMessage line must not stop or emit.
    session.continue_exec(stopped.thread_id)?;
    let run = run_to_termination(&session);
    assert!(run.saw_terminated, "debuggee must run to termination after the control stop");
    assert!(
        !run.saw_stopped,
        "the refused logMessage entry must not become a stopping breakpoint at line {OPTIONAL_LINE}"
    );
    assert!(
        run.console.is_empty(),
        "the refused logMessage entry must not emit simulated output; console was {:?}",
        run.console
    );

    session.disconnect()?;
    Ok(())
}

/// Positive control: a plain breakpoint on the same live boundary still stops
/// — the floor leaves the base source-breakpoint cell intact (#9578 test 2).
#[test]
fn plain_breakpoint_positive_control_still_stops_on_live_session() -> TestResult {
    if !perl_available() {
        eprintln!("Skipping plain_breakpoint_positive_control - perl not available");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("plain_control_e2e.pl");
    write(&script, optional_script())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;

    let resolved = session.set_breakpoints_checked(&script_str, &[PLAIN_LINE])?;
    session.configuration_done()?;

    let stopped = session.wait_stopped_with_frame()?;
    assert_eq!(stopped.reason, "breakpoint");
    assert_eq!(stopped.line, resolved[0], "plain breakpoints must keep their stop contract");

    session.disconnect()?;
    Ok(())
}
