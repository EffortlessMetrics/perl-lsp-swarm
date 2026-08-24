//! End-to-end proof that `setExceptionBreakpoints` with the `die` filter causes
//! the adapter to emit `stopped(reason="exception")` when the debuggee calls
//! `die`, and that the program runs to termination without stopping when the
//! filter is not enabled.
//!
//! `dap.exceptions.die` was implemented (`exception_break_on_die` mutex flag
//! toggled by `handle_set_exception_breakpoints`, reader checks `exception_re`)
//! but had `maturity = "not_proven"` in `features.toml` because no E2E fixture
//! drove a real `perl -d` session through the feature path. These tests close
//! that gap.
//!
//! The two primary behaviours under proof:
//!
//! 1. **Default (no filter)** — the die propagates, the program terminates, and
//!    no `stopped` event is emitted.  Proves the feature is strictly opt-in and
//!    cannot change default debugging behaviour.
//!
//! 2. **Die filter enabled** — a `stopped(reason="exception")` event is emitted
//!    before `terminated`, and the adapter's cached stack frame points to the
//!    source line where `die` was called.  Proves the complete E2E path from
//!    protocol filter to output reader detection to stopped event.

// Tests skip when `perl` is unavailable; the skip must be visible in CI logs.
#![allow(clippy::print_stderr)]

mod common;

use common::{DapWorkflowSession, perl_available, workflow_timeout};
use perl_dap::debug_adapter::DapMessage;
use serde_json::json;
use std::fs::write;
use tempfile::tempdir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Line in [`die_script`] that calls `die`.
///
/// The message does NOT end with `\n` so Perl appends the location suffix
/// (`at /path/script.pl line N.`), which `error_re` parses for file and line
/// and `exception_re` matches via `\bdied\b`.
const DIE_LINE: u64 = 5;

/// Perl script that calls `die` on line 5.
///
/// The die message contains the word "died" so the reader's `exception_re`
/// (`\bdied\b`) fires on the die output.  The message has no trailing `\n`,
/// so Perl appends the location suffix (`at /path/script.pl line 5.`) which
/// `error_re` uses to set `current_file` and `current_line` in the reader
/// before the exception check fires.
///
/// Line 4 (`my $x = 42;`) is the first executable line, where `perl -d`
/// always pauses implicitly.  `configurationDone` sends `c` from that point,
/// so execution continues and reaches the `die` on line 5.
fn die_script() -> &'static str {
    // Line 1: use strict;
    // Line 2: use warnings;
    // Line 3: (blank)
    // Line 4: my $x = 42;            <- implicit first-line debugger stop
    // Line 5: die "something has died";   <- DIE_LINE
    // Line 6: print "unreachable\n";
    "use strict;\nuse warnings;\n\nmy $x = 42;\ndie \"something has died\";\nprint \"unreachable\\n\";\n"
}

/// Enable the `die` exception breakpoint filter before `configurationDone`.
fn set_die_filter(session: &mut DapWorkflowSession) -> Result<(), String> {
    let args = json!({ "filters": ["die"] });
    let resp = session.request("setExceptionBreakpoints", Some(args));
    session.expect_success(&resp, "setExceptionBreakpoints")?;
    Ok(())
}

/// Drain events from the session until `terminated` or the timeout expires.
///
/// Returns `true` if a `stopped` event was received before `terminated`.
/// Logpoint and output events are silently consumed; this is the simplest
/// "run to termination" drain that also checks for unexpected stops.
fn drain_to_terminated(session: &DapWorkflowSession) -> bool {
    let deadline = std::time::Instant::now() + session.timeout;
    let mut saw_stopped = false;

    while std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let Ok(msg) = session.rx.recv_timeout(remaining) else {
            break;
        };
        let DapMessage::Event { event, .. } = &msg else {
            continue;
        };
        if event == "stopped" {
            saw_stopped = true;
        }
        if event == "terminated" {
            break;
        }
    }

    saw_stopped
}

/// Without any exception filter, a `die` in the debuggee must not produce a
/// `stopped` event — the program runs to termination exactly as if no debugger
/// were attached.
///
/// This is the "feature is opt-in" assertion: enabling the adapter must not
/// silently change default termination behaviour.
#[test]
fn test_die_without_exception_filter_runs_to_termination() -> TestResult {
    if !perl_available() {
        eprintln!(
            "Skipping test_die_without_exception_filter_runs_to_termination - perl not available"
        );
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("die_no_filter_e2e.pl");
    write(&script, die_script())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;
    // Intentionally NO `setExceptionBreakpoints` — the filter is not enabled.
    session.configuration_done()?;

    let saw_stopped = drain_to_terminated(&session);

    assert!(
        !saw_stopped,
        "without the `die` exception filter, `die` must NOT produce a `stopped` event; \
         the program should terminate silently"
    );

    Ok(())
}

/// With the `die` filter enabled, a `die` in the debuggee must cause the
/// adapter to emit `stopped(reason=\"exception\")` before `terminated`, and
/// the adapter's cached stack frame must point to the source file and the line
/// where `die` was called.
///
/// This closes the not-proven gap for `dap.exceptions.die`: the protocol path
/// (`setExceptionBreakpoints` → `exception_break_on_die` flag) is wired to the
/// output reader's `exception_re` detection, and the stopped event reaches the
/// client.
#[test]
fn test_die_with_exception_filter_stops_and_provides_frame() -> TestResult {
    if !perl_available() {
        eprintln!(
            "Skipping test_die_with_exception_filter_stops_and_provides_frame - perl not available"
        );
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("die_with_filter_e2e.pl");
    write(&script, die_script())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;
    // Enable the die filter before configurationDone per the DAP ordering contract.
    set_die_filter(&mut session)?;
    session.configuration_done()?;

    // The die must produce a stopped event before the program terminates.
    let stopped = session.wait_stopped()?;

    assert_eq!(
        stopped.reason, "exception",
        "stopped reason must be `exception` when the `die` filter is enabled; \
         got `{}`",
        stopped.reason
    );

    // The adapter caches the stack frame from the die location in the reader
    // thread.  A `stackTrace` request must return it even though the Perl
    // process may already have exited by the time the test runs.
    let (_, source_path, frame_line) = session.stack_trace(stopped.thread_id)?;

    // Source path must reference the script we launched.
    let script_name =
        script.file_name().and_then(|n| n.to_str()).ok_or("die_script name is not valid UTF-8")?;
    assert!(
        source_path.contains(script_name),
        "stack frame source path must contain the script name `{script_name}`; \
         got `{source_path}`"
    );

    // Frame line must point to the die statement.
    assert_eq!(
        frame_line, DIE_LINE as i64,
        "stack frame line must match the `die` call site (line {DIE_LINE}); \
         got line {frame_line}"
    );

    // Clean up: disconnect drives the session to terminated.
    session.disconnect()?;

    Ok(())
}

/// `setExceptionBreakpoints` with `filterOptions` (the alternative DAP field)
/// must activate the `die` filter through the same code path as `filters`.
///
/// This exercises the `filter_options` arm of `handle_set_exception_breakpoints`
/// and confirms that both protocol entry points reach the same behaviour:
/// `stopped(reason="exception")` on `die`.
#[test]
fn test_die_via_filter_options_also_stops() -> TestResult {
    if !perl_available() {
        eprintln!("Skipping test_die_via_filter_options_also_stops - perl not available");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("die_filter_options_e2e.pl");
    write(&script, die_script())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;

    // Use `filterOptions` instead of `filters` — both must enable the die flag.
    let args = json!({ "filters": [], "filterOptions": [{ "filterId": "die" }] });
    let resp = session.request("setExceptionBreakpoints", Some(args));
    session.expect_success(&resp, "setExceptionBreakpoints")?;

    session.configuration_done()?;

    let stopped = session.wait_stopped()?;

    assert_eq!(
        stopped.reason, "exception",
        "stopped reason must be `exception` when die filter is set via `filterOptions`; \
         got `{}`",
        stopped.reason
    );

    session.disconnect()?;

    Ok(())
}
