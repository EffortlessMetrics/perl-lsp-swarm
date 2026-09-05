//! Live DAP seam proof for conditional line breakpoints (#9578 floor).
//!
//! # What changed and why
//!
//! This suite previously proved the fail-open conditional path: the adapter
//! persisted the condition, forwarded it to `perl -d`, and execution stopped
//! only when Perl's own condition semantics were true. #9578 floors that
//! advertisement: `supportsConditionalBreakpoints` is advertised false and a
//! `setBreakpoints` entry carrying `condition` is refused per item, because an
//! adapter-accepted condition is not yet an accepted receipt that the engine
//! installed and enforced it (the #8988 re-enable gate owns that proof and
//! #7366 owns the same-session false-path evidence for promotion).
//!
//! The test keeps the live-session discipline — real `perl -d`, real stdio
//! boundary — and now discriminates the floor on it:
//!
//! * the conditional entry must come back `verified: false` with the exact
//!   #9578 conditional refusal, on a live initialized session;
//! * the debuggee must then run to `terminated` with **no** `stopped` event:
//!   if the condition had been silently stripped and the breakpoint installed
//!   unconditionally, the loop would stop at the conditional line — the
//!   precise fail-open behavior this floor forbids.

mod common;

use common::{DapWorkflowSession, perl_available, workflow_timeout};
use perl_dap::debug_adapter::DapMessage;
use serde_json::{Value, json};
use std::fs::write;
use tempfile::tempdir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const CONDITIONAL_BP_LINE: u64 = 5;

fn conditional_breakpoint_script_content() -> &'static str {
    "use strict;\nuse warnings;\n\nfor my $x (1..3) {\n    my $observed = $x;\n    print \"$observed\\n\";\n}\n"
}

#[test]
fn conditional_breakpoint_entry_is_refused_and_never_installs_on_live_session() -> TestResult {
    if !perl_available() {
        eprintln!(
            "Skipping conditional_breakpoint_entry_is_refused_and_never_installs - perl not available"
        );
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("workflow_conditional_breakpoint.pl");
    write(&script, conditional_breakpoint_script_content())?;

    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;

    let response = session.request(
        "setBreakpoints",
        Some(json!({
            "source": { "path": script_str },
            "breakpoints": [{
                "line": CONDITIONAL_BP_LINE,
                "condition": "$x > 2"
            }]
        })),
    );
    let body = session
        .expect_success(&response, "setBreakpoints")?
        .ok_or("setBreakpoints returned no body")?;
    let breakpoints = body
        .get("breakpoints")
        .and_then(Value::as_array)
        .ok_or("setBreakpoints response missing `breakpoints` array")?;
    assert_eq!(breakpoints.len(), 1, "one response breakpoint per input");
    let breakpoint =
        breakpoints.first().ok_or("setBreakpoints response returned no breakpoints")?;

    let verified = breakpoint.get("verified").and_then(Value::as_bool).unwrap_or(true);
    assert!(
        !verified,
        "a conditional entry must be refused while supportsConditionalBreakpoints is floored (#9578)"
    );
    let message = breakpoint
        .get("message")
        .and_then(Value::as_str)
        .ok_or("the refused conditional entry must carry a message")?;
    assert!(
        message.contains("supportsConditionalBreakpoints") && message.contains("#9578"),
        "expected the #9578 conditional floor refusal, got {message:?}"
    );

    session.configuration_done()?;

    // The refused entry must never install: an unconditional install would
    // stop the loop at the conditional line on the first iteration.
    let deadline = std::time::Instant::now() + session.timeout;
    let mut saw_stopped = false;
    let mut saw_terminated = false;
    while std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let Ok(msg) = session.rx.recv_timeout(remaining) else {
            break;
        };
        if let DapMessage::Event { event, .. } = &msg {
            if event == "terminated" {
                saw_terminated = true;
                break;
            }
            if event == "stopped" {
                saw_stopped = true;
                continue;
            }
        }
    }

    assert!(saw_terminated, "the debuggee must run to termination after the refusal");
    assert!(
        !saw_stopped,
        "a refused conditional entry must not install unconditionally; the loop at line \
         {CONDITIONAL_BP_LINE} must never stop"
    );

    session.disconnect()?;

    Ok(())
}
