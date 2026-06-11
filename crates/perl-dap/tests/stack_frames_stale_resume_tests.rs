//! Regression tests for stale stack-frame bugs fixed in issues #933 and #964.
//!
//! **Issue #964**: Every resume handler (continue/next/stepIn/stepOut/pause/goto)
//! cleared `session.variable_cache` but not `session.stack_frames`, so a
//! `stackTrace` arriving after a resume but before the next `stopped` event
//! served the previous stop's frames.
//!
//! **Issue #933**: The degraded-transport fallback in `handle_stack_trace` parsed
//! the recent-output snapshot buffer directly.  Because the buffer is ordered by
//! arrival time, the stale pre-stop context line appeared before the current stop
//! line, so the first returned frame was wrong.
//!
//! Unit-level tests for the degraded-path snapshot isolation live in the internal
//! test module of `debug_adapter/parsing.rs` (they require the private
//! `push_recent_output_line_for_test` helper).  This file holds the externally-
//! testable surface and the E2E regression guard that requires a live Perl process.

mod common;

use common::perl_available;
use perl_dap::DebugAdapter;
use perl_dap::debug_adapter::DapMessage;
use serde_json::json;

// ─── Response shape invariants ───────────────────────────────────────────────

/// `stackTrace` response must always include both `stackFrames` and
/// `totalFrames` keys, even when both are empty/zero.
#[test]
fn test_stack_trace_response_always_has_required_keys() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = DebugAdapter::new();
    let response = adapter.handle_request(1, "stackTrace", None);

    let DapMessage::Response { success, body, command, .. } = response else {
        return Err("expected Response for stackTrace".into());
    };
    assert!(success);
    assert_eq!(command, "stackTrace");

    let body = body.ok_or("stackTrace response missing body")?;
    assert!(body.get("stackFrames").is_some(), "must include stackFrames");
    assert!(body.get("totalFrames").is_some(), "must include totalFrames");

    let frames_len = body
        .get("stackFrames")
        .and_then(|v| v.as_array())
        .ok_or("stackFrames must be array")?
        .len();
    let total =
        body.get("totalFrames").and_then(|v| v.as_u64()).ok_or("totalFrames must be number")?;
    assert!(
        total >= frames_len as u64,
        "totalFrames ({total}) must be >= stackFrames length ({frames_len})"
    );
    Ok(())
}

/// Without a session and without snapshot content, `stackTrace` must return
/// the honest empty response that fix #995 established — not a fabricated frame.
#[test]
fn test_stack_trace_no_session_returns_empty_frames() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = DebugAdapter::new();
    let response = adapter.handle_request(1, "stackTrace", Some(json!({"threadId": 1})));

    let DapMessage::Response { success, body, .. } = response else {
        return Err("expected Response".into());
    };
    assert!(success);
    let body = body.ok_or("missing body")?;
    let frames = body.get("stackFrames").and_then(|v| v.as_array()).ok_or("missing stackFrames")?;

    assert_eq!(
        frames.len(),
        0,
        "no-session stackTrace must return empty frames (got {})",
        frames.len()
    );
    Ok(())
}

// ─── E2E: stack frames cleared on resume (requires perl) ─────────────────────

/// After a `continue` the second `stackTrace` must report the NEW stop location,
/// not the previous stop's line.  This guards the fix from #964 where
/// `session.stack_frames` was never cleared on resume, causing stale frames to be
/// served in the window between resume and the next `stopped` event.
///
/// Requires `perl` on PATH; skips gracefully otherwise.
#[test]
fn test_stack_frames_report_new_location_after_continue() -> Result<(), Box<dyn std::error::Error>>
{
    use common::{DapWorkflowSession, workflow_timeout};
    use std::fs::write;
    use tempfile::tempdir;

    if !perl_available() {
        eprintln!(
            "Skipping test_stack_frames_report_new_location_after_continue — perl not available"
        );
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("stale_frames.pl");
    // Two sequential executable statements so we can stop twice at different lines.
    write(
        &script,
        "use strict;\nuse warnings;\n\nmy $x = 1;\nmy $y = 2;\nmy $z = $x + $y;\nprint \"$z\\n\";\n",
    )?;
    let script_str = script.to_str().ok_or("non-UTF-8 path")?.to_string();

    let timeout = workflow_timeout();
    let mut session = DapWorkflowSession::new(timeout)?;
    session.launch(&script_str)?;

    // Breakpoints on lines 5 AND 6 — two distinct stops at two different lines.
    session.set_breakpoints(&script_str, &[5, 6])?;
    session.configuration_done()?;

    // First stop.
    let first = session.wait_stopped_with_frame()?;
    assert_eq!(first.reason, "breakpoint");
    let first_line = first.line;

    // Continue — clears session.stack_frames (the #964 fix).
    session.continue_exec(first.thread_id)?;

    // Second stop.
    let second = session.wait_stopped_with_frame()?;
    assert_eq!(second.reason, "breakpoint");

    // Must report a DIFFERENT line.  The stale-frames bug caused the second
    // stackTrace to report the first stop's line again.
    assert_ne!(
        second.line, first_line,
        "second stop must report a different line than the first \
         (first={first_line}, second={}); stale-frames bug reproduced",
        second.line
    );

    session.disconnect()?;
    Ok(())
}

/// Same invariant for `next` (step-over): two consecutive steps must report
/// increasing line numbers, never the same stale line.
#[test]
fn test_stack_frames_report_new_location_after_next() -> Result<(), Box<dyn std::error::Error>> {
    use common::{DapWorkflowSession, workflow_timeout};
    use std::fs::write;
    use tempfile::tempdir;

    if !perl_available() {
        eprintln!("Skipping test_stack_frames_report_new_location_after_next — perl not available");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("stale_frames_next.pl");
    write(
        &script,
        "use strict;\nuse warnings;\n\nmy $x = 1;\nmy $y = 2;\nmy $z = $x + $y;\nprint \"$z\\n\";\n",
    )?;
    let script_str = script.to_str().ok_or("non-UTF-8 path")?.to_string();

    let timeout = workflow_timeout();
    let mut session = DapWorkflowSession::new(timeout)?;
    session.launch(&script_str)?;

    session.set_breakpoints(&script_str, &[5])?;
    session.configuration_done()?;

    // Stop at the breakpoint.
    let first = session.wait_stopped_with_frame()?;
    assert_eq!(first.reason, "breakpoint");
    let first_line = first.line;

    // Step over one statement — clears session.stack_frames (the #964 fix).
    session.step_over(first.thread_id)?;

    // Next stop.
    let second = session.wait_stopped_with_frame()?;

    // Must advance beyond the first stop line.
    assert!(
        second.line > first_line,
        "step-over must advance to a later line (first={first_line}, second={}); \
         stale-frames bug would keep reporting the same line",
        second.line
    );

    session.disconnect()?;
    Ok(())
}
