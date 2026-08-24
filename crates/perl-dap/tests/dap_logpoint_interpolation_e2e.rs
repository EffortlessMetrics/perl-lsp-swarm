//! End-to-end proof that logpoints interpolate live variable values and do not
//! pause execution.
//!
//! `interpolate_logpoint_message` was unit-tested but unreachable from the live
//! path: the output reader called `register_breakpoint_hit` with no variables, so a
//! user who set a logpoint `x is {$x}` saw exactly that text instead of `x is 10`
//! (#5045). These tests drive a real `perl -d` and assert on the console output the
//! client actually receives.
//!
//! The "continue semantics" test closes the second gap: the logpoint infrastructure
//! is proven to not emit a `stopped` event — the debuggee keeps running and
//! eventually emits `terminated`.  Without that proof, a logpoint that secretly
//! pauses execution would still pass the interpolation tests (which only look at
//! console output) because those tests wait for `terminated` without checking
//! whether a `stopped` event arrived first.

// These tests skip when `perl` is unavailable; the skip must be visible in CI logs.
#![allow(clippy::print_stderr)]

mod common;

use common::{DapWorkflowSession, perl_available, workflow_timeout};
use perl_dap::debug_adapter::DapMessage;
use serde_json::{Value, json};
use std::fs::write;
use std::time::Instant;
use tempfile::tempdir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Shared logpoint location for every fixture in this file.
///
/// All four fixtures are separate string literals that must keep the same shape: two
/// `use` lines, a blank line, then scalar assignments on lines 4 and 5, so a logpoint
/// on line 6 sees them all. Edit one fixture's leading lines and its scalar is no
/// longer assigned when the logpoint fires — the test then fails with a confusing
/// "value missing" message rather than pointing at the fixture.
const LOGPOINT_LINE: u64 = 6;

fn logpoint_script() -> &'static str {
    "use strict;\nuse warnings;\n\nmy $x = 10;\nmy $y = $x + 5;\nmy $z = $x * $y;\nprint \"$z\\n\";\n"
}

/// Set a single logpoint (a breakpoint carrying a `logMessage`).
fn set_logpoint(
    session: &mut DapWorkflowSession,
    source_path: &str,
    line: u64,
    log_message: &str,
) -> Result<(), String> {
    let args = json!({
        "source": { "path": source_path },
        "breakpoints": [{ "line": line, "logMessage": log_message }]
    });
    let resp = session.request("setBreakpoints", Some(args));
    let body = session.expect_success(&resp, "setBreakpoints")?.ok_or("setBreakpoints body")?;
    let verified = body
        .get("breakpoints")
        .and_then(|v| v.as_array())
        .and_then(|bps| bps.first())
        .and_then(|bp| bp.get("verified"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !verified {
        return Err(format!("logpoint on line {line} was not verified"));
    }
    Ok(())
}

/// Collect every `console`-category `output` event until the session terminates.
///
/// Logpoint text is emitted on `console`; ordinary debuggee output uses `stdout`,
/// so this isolates the adapter's own logpoint messages.
fn collect_console_output(session: &DapWorkflowSession) -> Vec<String> {
    let deadline = Instant::now() + session.timeout;
    let mut console = Vec::new();

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(msg) = session.rx.recv_timeout(remaining) else {
            break;
        };
        let DapMessage::Event { event, body, .. } = &msg else {
            continue;
        };
        if event == "terminated" {
            break;
        }
        if event != "output" {
            continue;
        }
        let Some(body) = body.as_ref() else {
            continue;
        };
        if body.get("category").and_then(Value::as_str) != Some("console") {
            continue;
        }
        if let Some(text) = body.get("output").and_then(Value::as_str) {
            console.push(text.trim_end().to_string());
        }
    }

    console
}

/// A logpoint referencing in-scope scalars must report their values, not the
/// template, and must not stop the debuggee.
#[test]
fn test_logpoint_interpolates_live_scalar_values() -> TestResult {
    if !perl_available() {
        eprintln!("Skipping test_logpoint_interpolates_live_scalar_values - perl not available");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("logpoint_e2e.pl");
    write(&script, logpoint_script())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;
    set_logpoint(&mut session, &script_str, LOGPOINT_LINE, "x is {$x} and y is {$y}")?;
    session.configuration_done()?;

    let console = collect_console_output(&session);

    assert!(
        console.iter().any(|line| line.contains("x is 10 and y is 15")),
        "logpoint must report live values; console output was {console:?}"
    );
    assert!(
        !console.iter().any(|line| line.contains("{$x}")),
        "raw template must never reach the client; console output was {console:?}"
    );

    Ok(())
}

/// A scalar holding a multi-line string must arrive whole.
///
/// The capture is a line-oriented protocol layered on the debugger's output stream,
/// so an unescaped `p "DAPLPV:x\t" . $x` splits a value containing a newline across
/// several `read_line` calls and everything after the first segment is swallowed as
/// framing noise. This drives the real debugger rather than asserting the wire
/// format, so it fails if the escaping and the unescaping ever disagree.
#[test]
fn test_logpoint_interpolates_multi_line_scalar_values() -> TestResult {
    if !perl_available() {
        eprintln!("Skipping test_logpoint_interpolates_multi_line_scalar_values - perl missing");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("logpoint_multiline_e2e.pl");
    // `$m` spans two lines and also contains a backslash, so both escapes are live.
    write(
        &script,
        "use strict;\nuse warnings;\n\nmy $m = \"first\\nsecond C:\\\\path\";\nmy $n = 1;\nmy $z = $n;\nprint \"$z\\n\";\n",
    )?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;
    set_logpoint(&mut session, &script_str, LOGPOINT_LINE, "m is [{$m}]")?;
    session.configuration_done()?;

    let console = collect_console_output(&session);
    let joined = console.join("");

    assert!(
        joined.contains("first\nsecond C:\\path"),
        "a multi-line value must survive the capture whole; console was {console:?}"
    );
    assert!(
        !joined.contains("m is [first]"),
        "value must not be truncated at the first newline; console was {console:?}"
    );
    assert!(
        !joined.contains("\\n"),
        "the wire escaping must not leak into the user-visible message; console was {console:?}"
    );

    Ok(())
}

/// A value whose own text contains a `DB<N>` prompt token must survive.
///
/// The reader normalizes debugger output by truncating each line to whatever follows
/// the *last* `DB<...>` token. Feeding that to the capture destroys the `DAPLPV:`
/// prefix of any value containing `DB<4>`, so the reply is mistaken for framing noise
/// and the user gets the raw template back. This is end-to-end on purpose: the choice
/// of which text reaches the capture is made in the reader, not in `observe_line`, so
/// a unit test on the capture cannot catch a regression here.
#[test]
fn test_logpoint_value_containing_a_prompt_token_survives() -> TestResult {
    if !perl_available() {
        eprintln!("Skipping test_logpoint_value_containing_a_prompt_token_survives - perl missing");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("logpoint_prompt_token_e2e.pl");
    write(
        &script,
        "use strict;\nuse warnings;\n\nmy $d = \"DB<4> was logged\";\nmy $n = 1;\nmy $z = $n;\nprint \"$z\\n\";\n",
    )?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;
    set_logpoint(&mut session, &script_str, LOGPOINT_LINE, "d is [{$d}]")?;
    session.configuration_done()?;

    let console = collect_console_output(&session);
    let joined = console.join("");

    assert!(
        joined.contains("d is [DB<4> was logged]"),
        "a value containing a prompt token must survive normalization; console was {console:?}"
    );
    assert!(
        !joined.contains("d is [{$d}]"),
        "the template must not fall back to its raw form; console was {console:?}"
    );

    Ok(())
}

/// A value's own trailing whitespace must reach the client.
///
/// End-to-end on purpose: the loss happened in the reader's `trim_end()` on the whole
/// line, upstream of the capture, so a unit test on `observe_line` cannot see it.
#[test]
fn test_logpoint_preserves_trailing_whitespace_in_values() -> TestResult {
    if !perl_available() {
        eprintln!("Skipping test_logpoint_preserves_trailing_whitespace_in_values - perl missing");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("logpoint_trailing_ws_e2e.pl");
    write(
        &script,
        "use strict;\nuse warnings;\n\nmy $w = \"abc  \";\nmy $n = 1;\nmy $z = $n;\nprint \"$z\\n\";\n",
    )?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;
    set_logpoint(&mut session, &script_str, LOGPOINT_LINE, "w=[{$w}]")?;
    session.configuration_done()?;

    let console = collect_console_output(&session);
    let joined = console.join("");

    assert!(
        joined.contains("w=[abc  ]"),
        "trailing spaces belong to the value and must survive; console was {console:?}"
    );
    assert!(
        !joined.contains("w=[abc]"),
        "the value must not be silently trimmed; console was {console:?}"
    );

    Ok(())
}

/// A logpoint with nothing to interpolate must still be emitted.
///
/// Wiring interpolation in must not cost the plain case: the templates are handed
/// to the capture builder, and an implementation that drops them when no capture is
/// needed would silently swallow `"reached checkpoint"` entirely.
#[test]
fn test_logpoint_without_interpolation_is_still_emitted() -> TestResult {
    if !perl_available() {
        eprintln!("Skipping test_logpoint_without_interpolation_is_still_emitted - perl missing");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("logpoint_plain_e2e.pl");
    write(&script, logpoint_script())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;
    set_logpoint(&mut session, &script_str, LOGPOINT_LINE, "reached checkpoint")?;
    session.configuration_done()?;

    let console = collect_console_output(&session);

    assert!(
        console.iter().any(|line| line.contains("reached checkpoint")),
        "a logpoint with no expressions must still reach the client; console was {console:?}"
    );

    Ok(())
}

/// Events collected by [`collect_output_and_track_stops`], split by DAP category.
///
/// Tracks whether a `stopped` event was received so the continue-semantics tests
/// can assert it never fires during a logpoint run.
///
/// Note: the adapter reads from `perl -d`'s stderr (the debugger control stream)
/// and labels ALL those lines as DAP `category="stdout"`.  The Perl script's
/// actual `print` output goes to the perl process's piped stdout, which the
/// adapter does not forward.  `console` lines are the adapter's own logpoint
/// messages, which IS correct per the DAP spec.
struct AllOutputEvents {
    /// Lines emitted on the `console` category (logpoint messages from the adapter).
    console: Vec<String>,
    /// Whether any `stopped` event was received before `terminated`.
    saw_stopped: bool,
}

/// Collect every `output(category="console")` event and track `stopped` until `terminated`.
///
/// This is the prove-continue-semantics counterpart to [`collect_console_output`]:
/// the richer return type lets the test assert that the logpoint message appeared
/// on `console` AND that no `stopped` event interrupted the run.  Both assertions
/// together prove that the logpoint fired and execution continued uninterrupted.
fn collect_output_and_track_stops(session: &DapWorkflowSession) -> AllOutputEvents {
    let deadline = std::time::Instant::now() + session.timeout;
    let mut out = AllOutputEvents { console: Vec::new(), saw_stopped: false };

    while std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let Ok(msg) = session.rx.recv_timeout(remaining) else {
            break;
        };
        let DapMessage::Event { event, body, .. } = &msg else {
            continue;
        };
        if event == "terminated" {
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
        if body.get("category").and_then(Value::as_str) == Some("console") {
            if let Some(text) = body.get("output").and_then(Value::as_str) {
                out.console.push(text.trim_end().to_string());
            }
        }
    }

    out
}

/// A logpoint must emit its message on `console` AND must NOT emit a `stopped`
/// event — execution must continue uninterrupted past the logpoint site.
///
/// This closes the continue-semantics gap: without this test a logpoint
/// implementation that secretly paused execution would still pass the
/// interpolation tests (which only look at console output), because the
/// interpolation tests wait for `terminated` without checking for `stopped` first.
///
/// Note on the program's `print` output: `perl -d` reads its control stream
/// from the process's stderr; the script's own stdout (e.g. `print "$z\n"`)
/// goes to a separate piped stdout that the adapter does not forward as DAP
/// events.  The absence of a `stopped` event before `terminated` proves that
/// the program ran past the logpoint without pausing, which is the definition
/// of continue semantics.
#[test]
fn test_logpoint_does_not_emit_stopped_event() -> TestResult {
    if !perl_available() {
        eprintln!("Skipping test_logpoint_does_not_emit_stopped_event - perl not available");
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("logpoint_continue_semantics.pl");
    write(&script, logpoint_script())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;
    set_logpoint(&mut session, &script_str, LOGPOINT_LINE, "logpoint fired: z={$z}")?;
    session.configuration_done()?;

    let events = collect_output_and_track_stops(&session);

    // The logpoint message must appear on `console` — proves the logpoint fired.
    assert!(
        events.console.iter().any(|line| line.contains("logpoint fired: z=")),
        "logpoint message must appear on console; console events were {:?}",
        events.console
    );

    // The critical continue-semantics assertion: no `stopped` event may appear.
    // A logpoint that secretly paused execution would emit `stopped`; a correct
    // logpoint sends `output(category="console")` and immediately issues `c` to
    // the debugger, so execution continues and `terminated` arrives with no
    // `stopped` in between.
    assert!(
        !events.saw_stopped,
        "a logpoint must NOT emit a `stopped` event — execution must continue uninterrupted; \
         console events were {:?}",
        events.console
    );

    Ok(())
}

/// An expression the interpolator cannot resolve is left verbatim rather than
/// silently dropped or smuggled into the debugger command stream.
#[test]
fn test_logpoint_keeps_unresolvable_expressions_verbatim() -> TestResult {
    if !perl_available() {
        eprintln!(
            "Skipping test_logpoint_keeps_unresolvable_expressions_verbatim - perl not available"
        );
        return Ok(());
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("logpoint_verbatim_e2e.pl");
    write(&script, logpoint_script())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch(&script_str)?;
    // `{ $x }` pins that the extractor and the interpolator agree on inner
    // whitespace: `referenced_scalars` trims before validating, so it queries `x`.
    set_logpoint(&mut session, &script_str, LOGPOINT_LINE, "x={$x} pad={ $x } expr={$x + 1}")?;
    session.configuration_done()?;

    let console = collect_console_output(&session);

    assert!(
        console.iter().any(|line| line.contains("x=10 pad=10 expr={$x + 1}")),
        "resolvable scalars interpolate — including with inner whitespace — while \
         expressions stay verbatim; console was {console:?}"
    );

    Ok(())
}
