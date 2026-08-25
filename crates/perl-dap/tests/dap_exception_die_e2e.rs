//! End-to-end proof that `setExceptionBreakpoints` with the `die` filter causes
//! the adapter to emit `stopped(reason="exception")` when the debuggee dies
//! with an uncaught `die`, and that the program runs to termination without
//! stopping when the filter is not enabled.
//!
//! `dap.exceptions.die` was implemented (`exception_break_on_die` mutex flag
//! toggled by `handle_set_exception_breakpoints`, reader checks `exception_re`)
//! but had `maturity = "not_proven"` in `features.toml` because no E2E fixture
//! drove a real `perl -d` session through the feature path. These tests close
//! that gap.
//!
//! The behaviours under proof:
//!
//! 1. **Default (no filter)** — the die propagates, the program terminates, and
//!    no `stopped` event is emitted.  Proves the feature is strictly opt-in and
//!    cannot change default debugging behaviour.  Termination is a required
//!    witness: a session that hangs instead of terminating fails the test.
//!
//! 2. **Die filter enabled** — a `stopped(reason="exception")` event is emitted
//!    before `terminated`, and the adapter's cached stack frame points to the
//!    source line where `die` was called.  Proves the complete E2E path from
//!    protocol filter to output reader detection to stopped event.
//!
//! 3. **Lookalike output (negative row, #9081)** — the same text shape printed
//!    to stderr without `die` semantics must NOT produce an exception stop.
//!
//! Claim boundary: stock `perl -d` has no stop-on-die primitive — an uncaught
//! `die` terminates the process, and the reader attributes the exception from
//! the bare ` at FILE line N.` line that perl5db's `__DIE__` handler prints
//! (a `print` of identical text never fires the handler; an `eval`-caught die
//! propagates silently). The `stopped(reason="exception")` is therefore an
//! output-attributed stop at process end, not a resumable pre-mortem
//! suspension; the full inspectable-suspension contract remains with #9081,
//! which keeps `dap.exceptions.die` at `not_proven`. `warn` fires perl5db's
//! sibling `__WARN__` handler with an indistinguishable suffix line — that
//! warn/die stream ambiguity is part of the residual #9081 warn-filter claim,
//! not something these tests assert away.

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
/// (`at /path/script.pl line N.`), which `error_re` parses for file and line.
const DIE_LINE: u64 = 5;

/// Perl script that calls `die` on line 5.
///
/// The die message is deliberately token-free (`"boom"`): it contains none of
/// the trigger words (`died`/`panic`/`uncaught exception`) baked into the
/// reader's `exception_re`. Detection must instead attribute the uncaught die
/// from the signal real `perl -d` produces — the bare ` at FILE line N.` line
/// printed by perl5db's `__DIE__` handler — so the fixture cannot collude
/// with the implementation's own trigger words. A detection path that only
/// fires on those words fails these tests.
///
/// Line 4 (`my $x = 42;`) is the first executable line, where `perl -d`
/// always pauses implicitly.  `configurationDone` sends `c` from that point,
/// so execution continues and reaches the `die` on line 5.
fn die_script() -> &'static str {
    // Line 1: use strict;
    // Line 2: use warnings;
    // Line 3: (blank)
    // Line 4: my $x = 42;            <- implicit first-line debugger stop
    // Line 5: die "boom";            <- DIE_LINE
    // Line 6: print "unreachable\n";
    "use strict;\nuse warnings;\n\nmy $x = 42;\ndie \"boom\";\nprint \"unreachable\\n\";\n"
}

/// Enable the `die` exception breakpoint filter before `configurationDone`.
fn set_die_filter(session: &mut DapWorkflowSession) -> Result<(), String> {
    let args = json!({ "filters": ["die"] });
    let resp = session.request("setExceptionBreakpoints", Some(args));
    session.expect_success(&resp, "setExceptionBreakpoints")?;
    Ok(())
}

/// Events observed by [`drain_to_terminated`].
struct DrainOutcome {
    /// Whether any `stopped` event was received before `terminated`.
    saw_stopped: bool,
    /// Whether the session actually reached `terminated`.
    ///
    /// This witness is mandatory: `recv_timeout` expiry or a channel
    /// disconnect exits the drain loop with `saw_terminated = false`, so an
    /// adapter that hangs instead of terminating fails the test instead of
    /// greening vacuously.
    saw_terminated: bool,
}

/// Drain events from the session until `terminated` or the timeout expires.
///
/// Logpoint and output events are silently consumed; this is the simplest
/// "run to termination" drain that also checks for unexpected stops.
fn drain_to_terminated(session: &DapWorkflowSession) -> DrainOutcome {
    let deadline = std::time::Instant::now() + session.timeout;
    let mut outcome = DrainOutcome { saw_stopped: false, saw_terminated: false };

    while std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let Ok(msg) = session.rx.recv_timeout(remaining) else {
            break;
        };
        let DapMessage::Event { event, .. } = &msg else {
            continue;
        };
        if event == "stopped" {
            outcome.saw_stopped = true;
        }
        if event == "terminated" {
            outcome.saw_terminated = true;
            break;
        }
    }

    outcome
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

    let outcome = drain_to_terminated(&session);

    assert!(
        !outcome.saw_stopped,
        "without the `die` exception filter, `die` must NOT produce a `stopped` event; \
         the program should terminate silently"
    );
    assert!(
        outcome.saw_terminated,
        "the debuggee must reach `terminated` after an uncaught die — a timeout or \
         disconnect here means the adapter hung instead of observing program end"
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

    // The exception stop must be followed by real program termination: the
    // uncaught die ends the debuggee, and the adapter must observe it. A
    // timeout here means the stop was emitted but the run never completed.
    session.drain_until_event("terminated")?;

    // Clean up: disconnect tears the session down (terminated already seen).
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

    // Same termination witness as the `filters` path: the stop must be
    // followed by observed program end.
    session.drain_until_event("terminated")?;

    session.disconnect()?;

    Ok(())
}

/// Negative row (#9081): text with the exact die shape printed to stderr
/// WITHOUT `die` semantics must not create an exception stop, even with the
/// `die` filter enabled.
///
/// The lookalike line matches `error_re` (`boom at <script> line N.`), so a
/// detection path keyed on output text alone would raise a spurious
/// `stopped(reason="exception")`. Real attribution requires the perl5db
/// `__DIE__`-handler suffix line that only a genuine uncaught die produces,
/// so this run must reach `terminated` with no `stopped` event.
#[test]
fn test_die_filter_ignores_lookalike_stderr_output() -> TestResult {
    if !perl_available() {
        eprintln!("Skipping test_die_filter_ignores_lookalike_stderr_output - perl not available");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("die_lookalike_e2e.pl");
    // `$0` interpolates the real script path, reproducing the exact byte shape
    // of an uncaught `die "boom"` line — but nothing dies; execution continues.
    write(
        &script,
        "use strict;\nuse warnings;\n\nprint STDERR \"boom at $0 line 4.\\n\";\nprint \"still alive\\n\";\n",
    )?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;
    set_die_filter(&mut session)?;
    session.configuration_done()?;

    let outcome = drain_to_terminated(&session);

    assert!(
        !outcome.saw_stopped,
        "lookalike stderr output must NOT produce a `stopped` event — only a real \
         uncaught die (perl5db-handler-attributed) may stop"
    );
    assert!(
        outcome.saw_terminated,
        "the debuggee must reach `terminated` after printing lookalike output — a \
         timeout or disconnect here means the adapter hung instead of observing program end"
    );

    Ok(())
}
