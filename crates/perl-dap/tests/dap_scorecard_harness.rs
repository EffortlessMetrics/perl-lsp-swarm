//! DAP scorecard harness.
//!
//! Measures launch success and session correctness metrics used by
//! `docs/project/status/dap.md`.
//!
//! # Running
//!
//! ```text
//! cargo test -p perl-dap --test dap_scorecard_harness -- --nocapture
//! ```

#![expect(
    clippy::print_stderr,
    reason = "Integration-test diagnostic and skip output; tracing is not the harness logger."
)]
mod common;

use common::{DapWorkflowSession, debuggee_perl_or_typed_skip, perl_available, workflow_timeout};
use perl_dap::{DapMessage, DebugAdapter};
use perl_lsp_rs_core::transport::framing::frame;
use serde::Serialize;
use serde_json::{Value, json};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, sync_channel};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::tempdir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const RECEIPT_FILE: &str = "dap_scorecard_receipt.json";

#[derive(Debug, Clone, Serialize)]
struct FixtureResult {
    name: &'static str,
    elapsed_ms: Option<u128>,
    error: Option<String>,
}

impl FixtureResult {
    fn passed(&self) -> bool {
        self.error.is_none()
    }
}

#[derive(Debug, Clone, Serialize)]
struct RateMetric {
    passed: usize,
    total: usize,
    threshold_pct: u8,
    p50_ms: Option<u128>,
    p95_ms: Option<u128>,
    details: Vec<FixtureResult>,
}

#[derive(Debug, Clone, Serialize)]
struct BinaryMetric {
    status: &'static str,
    detail: String,
}

#[derive(Debug, Clone, Serialize)]
struct ScorecardReceipt {
    perl_available: bool,
    launch: RateMetric,
    attach: RateMetric,
    variables: BinaryMetric,
    evaluate: BinaryMetric,
    deep_pagination: BinaryMetric,
    memory: BinaryMetric,
}

fn wait_for_event(
    rx: &Receiver<DapMessage>,
    event_name: &str,
    timeout: Duration,
) -> Result<DapMessage, String> {
    common::wait_for_event(rx, event_name, timeout)
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

fn percentile(sorted: &[u128], pct: u8) -> Option<u128> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((pct as f64 / 100.0) * sorted.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    Some(sorted[idx])
}

fn launch_timeout() -> Duration {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some()
        || std::env::var_os("CARGO_LLVM_COV").is_some()
    {
        Duration::from_mins(1)
    } else {
        Duration::from_secs(10)
    }
}

fn probe_launch(script_path: &Path, perl_binary: &Path, timeout: Duration) -> Result<u128, String> {
    let script_str =
        script_path.to_str().ok_or("fixture path contains non-UTF-8 characters")?.to_string();

    let mut adapter = DebugAdapter::new();
    let (tx, rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    let init_resp = adapter.handle_request(1, "initialize", None);
    response_success(init_resp, "initialize")?;
    wait_for_event(&rx, "initialized", timeout)?;

    let t_launch = Instant::now();
    let launch_resp = adapter.handle_request(
        2,
        "launch",
        Some(json!({
            "program": script_str,
            "args": [],
            "stopOnEntry": true,
            "perlPath": perl_binary.to_string_lossy(),
            "env": {
                "PERL_PERTURB_KEYS": "0",
                "PERL_HASH_SEED": "0",
                "LC_ALL": "C",
                "TZ": "UTC"
            }
        })),
    );
    response_success(launch_resp, "launch")?;
    wait_for_event(&rx, "stopped", timeout)?;
    let elapsed_ms = t_launch.elapsed().as_millis();

    let _ = adapter.handle_request(3, "disconnect", Some(json!({})));
    Ok(elapsed_ms)
}

fn probe_attach(timeout: Duration) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let server_handle = thread::spawn(move || -> Result<(), String> {
        let (mut socket, _) = listener.accept().map_err(|e| e.to_string())?;
        let stopped = json!({
            "type": "event",
            "seq": 1,
            "event": "stopped",
            "body": {
                "reason": "breakpoint",
                "threadId": 7,
                "allThreadsStopped": true
            }
        })
        .to_string();
        socket.write_all(&frame(stopped.as_bytes())).map_err(|e| e.to_string())?;
        socket.flush().map_err(|e| e.to_string())?;

        let mut buf = [0u8; 512];
        loop {
            match socket.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err.to_string()),
            }
        }
        Ok(())
    });

    let mut adapter = DebugAdapter::new();
    let (tx, rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    response_success(adapter.handle_request(1, "initialize", None), "initialize")?;
    wait_for_event(&rx, "initialized", timeout)?;
    response_success(
        adapter.handle_request(
            2,
            "attach",
            Some(json!({
                "host": "127.0.0.1",
                "port": port,
                "timeout": 2000
            })),
        ),
        "attach",
    )?;
    wait_for_event(&rx, "stopped", timeout)?;
    response_success(adapter.handle_request(3, "disconnect", Some(json!({}))), "disconnect")?;
    wait_for_event(&rx, "terminated", timeout)?;

    match server_handle.join() {
        Ok(result) => result,
        Err(_) => Err("fake TCP debugger server panicked".to_string()),
    }
}

fn metric_from_result(result: Result<String, String>) -> BinaryMetric {
    match result {
        Ok(detail) => BinaryMetric { status: "PASS", detail },
        Err(detail) => BinaryMetric { status: "FAIL", detail },
    }
}

fn scopes_for_frame(session: &mut DapWorkflowSession, frame_id: i64) -> Result<Vec<Value>, String> {
    let response = session.request("scopes", Some(json!({ "frameId": frame_id })));
    let body = session.expect_success(&response, "scopes")?.ok_or("scopes response had no body")?;
    body.get("scopes")
        .and_then(Value::as_array)
        .cloned()
        .ok_or("scopes response body missing `scopes` array".to_string())
}

fn locals_scope_ref_with_authority(
    session: &mut DapWorkflowSession,
    thread_id: i64,
    frame_id: i64,
) -> Result<i64, String> {
    let scopes = scopes_for_frame(session, frame_id)?;
    if let Some(scope) = scopes.iter().find(|scope| {
        scope.get("presentationHint").and_then(Value::as_str) == Some("locals")
            || scope.get("name").and_then(Value::as_str) == Some("Locals")
    }) {
        return scope
            .get("variablesReference")
            .and_then(Value::as_i64)
            .ok_or("Locals scope missing `variablesReference`".to_string());
    }

    let current_stack = match session.stack_trace(thread_id) {
        Ok((current_id, source, line)) => {
            format!("frame_id={current_id}, source={source:?}, line={line}")
        }
        Err(error) => format!("unavailable ({error})"),
    };
    Err(format!(
        "No Locals scope for requested stopped frame_id={frame_id}; current stack authority: {current_stack}; scopes response: {scopes:?}"
    ))
}

fn normalized_source_path(path: &str) -> String {
    let mut normalized = path.replace('\\', "/");
    if let Some(stripped) = normalized.strip_prefix("//?/") {
        normalized = stripped.to_string();
    }
    while normalized.contains("//") {
        normalized = normalized.replace("//", "/");
    }
    if cfg!(windows) {
        normalized.make_ascii_lowercase();
    }
    normalized.trim_end_matches('/').to_string()
}

/// Canonicalize both filesystem spellings before the lexical slash/case
/// normalization. A symlinked `TMPDIR` or invoked-script path makes perl5db
/// preserve the invoked spelling in the frame path, so resolving only the
/// expected side would reject the correct stopped frame; resolving only one
/// side is not enough for either direction.
fn canonical_path_text(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    canonical.to_string_lossy().into_owned()
}

fn assert_source_path_matches(actual: &str, expected: &Path) -> Result<(), String> {
    let expected_text = canonical_path_text(expected);
    let actual_text = canonical_path_text(Path::new(actual));
    if normalized_source_path(&actual_text) != normalized_source_path(&expected_text) {
        return Err(format!(
            "stopped frame source path does not identify scorecard script: actual={actual:?}, expected={expected_text:?}"
        ));
    }
    Ok(())
}

/// Accept a stopped frame as scorecard authority only when BOTH the line is
/// the admitted breakpoint AND the normalized source path identifies the
/// scorecard script. The source comparison is part of the acceptance
/// condition, not merely its failure text: a same-line stop reported for a
/// different file must fail the probe instead of feeding the scope checks.
fn assert_stopped_frame_is_admitted_breakpoint(
    actual_source: &str,
    actual_line: i64,
    expected_source: &Path,
    expected_line: i64,
    label: &str,
) -> Result<(), String> {
    if actual_line != expected_line {
        return Err(format!(
            "{label} stopped frame is not the admitted breakpoint: source={actual_source:?}, line={actual_line}, expected_line={expected_line}"
        ));
    }
    assert_source_path_matches(actual_source, expected_source)
        .map_err(|detail| format!("{label}: {detail}"))
}

fn assert_stale_frame_has_no_scopes(
    session: &mut DapWorkflowSession,
    frame_id: i64,
) -> Result<(), String> {
    // `handle_scopes` → `exact_current_stopped_frame_id` admits a frame only
    // when the session is Stopped and the requested id equals the current
    // banner frame's id, so a prior-generation id gets an empty scope list
    // rather than another frame's pad. The sibling positive control below,
    // which asserts the current frame's Locals contain `@big` and `$marker`,
    // keeps this stale-frame assertion from passing vacuously.
    let scopes = scopes_for_frame(session, frame_id)?;
    if !scopes.is_empty() {
        return Err(format!(
            "prior-generation frame_id={frame_id} returned scopes instead of an honest empty list: {scopes:?}"
        ));
    }
    Ok(())
}

/// Assert the attach success-rate threshold on a receipt. Shared by the live
/// and skipped paths because attach probes run in both.
fn assert_attach_rate(receipt: &ScorecardReceipt) {
    let attach_threshold =
        (receipt.attach.total * usize::from(receipt.attach.threshold_pct)).div_ceil(100);
    assert!(
        receipt.attach.passed >= attach_threshold,
        "DAP attach success rate below threshold: {}/{} passed (need ≥{})",
        receipt.attach.passed,
        receipt.attach.total,
        attach_threshold
    );
}

fn probe_session_metrics(
    perl_binary: &Path,
) -> Result<(BinaryMetric, BinaryMetric, BinaryMetric), String> {
    let workspace = tempdir().map_err(|e| e.to_string())?;
    let script_path = workspace.path().join("scorecard_session.pl");
    // `@big` is lexical so it is enumerated through the advertised Locals
    // scope (#10563: Package/Globals are not advertised at a live frame);
    // `our $x` stays for the evaluate-in-frame proof.
    let script_text = r#"use strict;
use warnings;
our $x = 41;
sub scorecard_frame {
    my @big = (1..500);
    our %meta = (name => "dap-scorecard");
    my $marker = $x + 1;
    print "marker=$marker\n";
}
scorecard_frame();
scorecard_frame();
"#;
    fs::write(&script_path, script_text).map_err(|e| e.to_string())?;

    let script_str =
        script_path.to_str().ok_or_else(|| "script path is not valid UTF-8".to_string())?;

    let mut session = DapWorkflowSession::new(workflow_timeout())?;
    session.launch_pinned(perl_binary, script_str)?;
    // The repeated stop must live INSIDE `scorecard_frame`: every lexical the
    // session metrics claim authority over (`@big`, `$marker`) belongs to the
    // sub's pad, and only a suspension whose stopped frame owns that pad can
    // prove live locals/evaluate authority. Both invocations stop on the same
    // executable line, each producing a fresh suspension identity.
    let resolved_lines = session.set_breakpoints_checked(script_str, &[7])?;
    session.configuration_done()?;

    let stop = session.wait_stopped()?;
    let (first_frame_id, first_source_path, first_frame_line) =
        session.stack_trace(stop.thread_id)?;
    let expected_line =
        resolved_lines.first().copied().ok_or("setBreakpoints returned no resolved lines")?;
    assert_stopped_frame_is_admitted_breakpoint(
        &first_source_path,
        first_frame_line,
        &script_path,
        expected_line,
        "first",
    )?;
    session.continue_exec(stop.thread_id)?;
    let second_stop = session.wait_stopped()?;
    if second_stop.thread_id != stop.thread_id {
        return Err(format!(
            "second stopped event changed thread authority: first={}, second={}",
            stop.thread_id, second_stop.thread_id
        ));
    }
    let (frame_id, source_path, frame_line) = session.stack_trace(second_stop.thread_id)?;
    let repeated_line =
        resolved_lines.first().copied().ok_or("setBreakpoints returned no resolved lines")?;
    assert_stopped_frame_is_admitted_breakpoint(
        &source_path,
        frame_line,
        &script_path,
        repeated_line,
        "second",
    )?;
    if frame_id == first_frame_id {
        return Err(format!(
            "second stopped generation reused the first frame id {frame_id}; stale-frame control is not valid"
        ));
    }
    assert_stale_frame_has_no_scopes(&mut session, first_frame_id)?;
    // #10563: a live frame advertises Locals (plus Arguments); Globals is not
    // advertised, so the session metrics measure the Locals enumeration.
    let locals_ref = locals_scope_ref_with_authority(&mut session, stop.thread_id, frame_id)?;
    let locals = session.variables(locals_ref)?;

    let vars_metric = metric_from_result((|| {
        if locals.is_empty() {
            return Err("locals scope returned no variables".to_string());
        }
        if locals.iter().any(|var| var.get("name").and_then(Value::as_str).unwrap_or("").is_empty())
        {
            return Err("locals scope contains variables with empty names".to_string());
        }
        // The stop lives inside `scorecard_frame`, so the advertised Locals
        // scope must actually name the sub-owned lexicals the metrics claim
        // authority over — a main-pad enumeration (the #13020 failure mode)
        // contains neither `@big` nor `$marker` and must fail here.
        let names: Vec<&str> =
            locals.iter().filter_map(|var| var.get("name").and_then(Value::as_str)).collect();
        for required in ["@big", "$marker"] {
            if !names.contains(&required) {
                return Err(format!(
                    "locals scope does not name the sub-owned lexical {required} at the stopped frame: names={names:?}"
                ));
            }
        }
        let n = locals.len();
        Ok(format!(
            "locals scope returned {} named {} (including @big and $marker)",
            n,
            if n == 1 { "variable" } else { "variables" }
        ))
    })());

    let eval_metric = metric_from_result((|| {
        let eval_response = session.request(
            "evaluate",
            Some(json!({
                "expression": "$x + 1",
                "frameId": frame_id,
                "context": "watch",
                "allowSideEffects": false
            })),
        );
        let eval_body = session.expect_success(&eval_response, "evaluate")?;
        let eval_body = eval_body.ok_or_else(|| "evaluate response missing body".to_string())?;
        let result = eval_body.get("result").and_then(Value::as_str).unwrap_or("");
        if !result.contains("42") {
            return Err(format!("evaluate result does not contain 42: {result:?}"));
        }
        Ok("evaluate($x + 1) returns 42".to_string())
    })());

    let deep_metric = if let Some(expandable) = locals.iter().find(|var| {
        var.get("variablesReference").and_then(Value::as_i64).unwrap_or(0) > 0
            && var.get("indexedVariables").and_then(Value::as_i64).unwrap_or(0) >= 200
    }) {
        metric_from_result((|| {
            let vars_ref = expandable
                .get("variablesReference")
                .and_then(Value::as_i64)
                .ok_or_else(|| "expandable variable missing variablesReference".to_string())?;
            let indexed_count =
                expandable.get("indexedVariables").and_then(Value::as_i64).unwrap_or(0);

            let response = session.request(
                "variables",
                Some(json!({
                    "variablesReference": vars_ref,
                    "start": 250,
                    "count": 25
                })),
            );
            let body = session.expect_success(&response, "variables")?;
            let body = body.ok_or_else(|| "variables response missing body".to_string())?;
            let vars = body
                .get("variables")
                .and_then(Value::as_array)
                .ok_or_else(|| "variables body missing variables array".to_string())?;

            if vars.len() != 25 {
                return Err(format!("expected 25 paged variables, got {}", vars.len()));
            }
            let first =
                vars.first().and_then(|v| v.get("name")).and_then(Value::as_str).unwrap_or("");
            let last =
                vars.last().and_then(|v| v.get("name")).and_then(Value::as_str).unwrap_or("");
            if first != "[250]" || last != "[274]" {
                return Err(format!("unexpected page bounds: first={first:?}, last={last:?}"));
            }
            Ok(format!("pagination verified on variable with indexedVariables={indexed_count}"))
        })())
    } else {
        // Not a skip: under the current locals contract this proof cannot run.
        // `build_locals_b_eval_cmd` deliberately renders lexical aggregates as
        // opaque `ARRAY(0x0)`/`HASH(0x0)` markers without variablesReference or
        // indexedVariables until bounded lexical-aggregate enumeration lands
        // (#7358), so no real-session local can satisfy the pagination
        // predicate today. Record the gap as not proven rather than letting a
        // permanently unreachable metric read as an incidental skip.
        BinaryMetric {
            status: "NOT_PROVEN",
            detail: "no expandable lexical aggregate at this stop: locals aggregates render as \
                     opaque markers until #7358 lands, so live deep pagination is not proven"
                .to_string(),
        }
    };

    session.disconnect()?;
    Ok((vars_metric, eval_metric, deep_metric))
}

fn linux_rss_kb_best_effort() -> Option<u64> {
    let content = fs::read_to_string("/proc/self/status").ok()?;
    let vm_rss_line = content.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kb = vm_rss_line.split_whitespace().nth(1).and_then(|raw| raw.parse::<u64>().ok())?;
    Some(kb)
}

fn memory_metric() -> BinaryMetric {
    let adapter_size = std::mem::size_of::<DebugAdapter>();
    match linux_rss_kb_best_effort() {
        Some(rss_kb) => BinaryMetric {
            status: "MEASURED",
            detail: format!(
                "debug_adapter_struct_size={} bytes; best_effort_process_rss={} KiB",
                adapter_size, rss_kb
            ),
        },
        None => BinaryMetric {
            status: "BEST_EFFORT",
            detail: format!(
                "debug_adapter_struct_size={} bytes; process RSS unavailable on this platform",
                adapter_size
            ),
        },
    }
}

fn target_receipt_path() -> PathBuf {
    if let Some(custom) = std::env::var_os("CARGO_TARGET_DIR") {
        return PathBuf::from(custom).join(RECEIPT_FILE);
    }

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    Path::new(&manifest_dir).join("..").join("..").join("target").join(RECEIPT_FILE)
}

fn write_receipt(receipt: &ScorecardReceipt) -> Result<(), String> {
    let path = target_receipt_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    let payload = serde_json::to_string_pretty(receipt).map_err(|e| e.to_string())?;
    fs::write(&path, payload).map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    eprintln!("DAP scorecard receipt: {}", path.display());
    Ok(())
}

fn print_marker_friendly_summary(receipt: &ScorecardReceipt) {
    eprintln!();
    eprintln!("<!-- BEGIN: DAP_LAUNCH_SCORECARD -->");
    eprintln!("| Metric | Value | Target | Status |");
    eprintln!("|---|---|---|---|");
    let launch_pct = (receipt.launch.passed * 100).checked_div(receipt.launch.total).unwrap_or(0);
    let launch_status =
        if launch_pct >= usize::from(receipt.launch.threshold_pct) { "PASS" } else { "FAIL" };
    eprintln!(
        "| Launch success rate | {}/{} ({} %) | ≥ {} % | {} |",
        receipt.launch.passed,
        receipt.launch.total,
        launch_pct,
        receipt.launch.threshold_pct,
        launch_status
    );
    eprintln!("| Fixtures tested | hello, loops, eval, args, begin_end | 5 | — |");
    if let Some(p50) = receipt.launch.p50_ms {
        let status = if p50 <= 2_000 { "PASS" } else { "FAIL" };
        eprintln!("| cold_launch_p50 | {p50} ms | ≤ 2 000 ms | {status} |");
    }
    if let Some(p95) = receipt.launch.p95_ms {
        let status = if p95 <= 5_000 { "PASS" } else { "FAIL" };
        eprintln!("| cold_launch_p95 | {p95} ms | ≤ 5 000 ms | {status} |");
    }
    eprintln!("<!-- END: DAP_LAUNCH_SCORECARD -->");

    eprintln!();
    eprintln!("<!-- BEGIN: DAP_SESSION_SCORECARD -->");
    eprintln!("| Metric | Value | Target | Status |");
    eprintln!("|---|---|---|---|");

    let attach_pct = (receipt.attach.passed * 100).checked_div(receipt.attach.total).unwrap_or(0);
    let attach_status =
        if attach_pct >= usize::from(receipt.attach.threshold_pct) { "PASS" } else { "FAIL" };
    eprintln!(
        "| Attach success rate (TCP loopback) | {}/{} ({} %) | ≥ {} % | {} |",
        receipt.attach.passed,
        receipt.attach.total,
        attach_pct,
        receipt.attach.threshold_pct,
        attach_status
    );
    eprintln!(
        "| Variables pane correctness (real session) | {} | expected named variables in scope | {} |",
        receipt.variables.detail, receipt.variables.status
    );
    eprintln!(
        "| Evaluate correctness (real session) | {} | evaluate($x + 1) => 42 | {} |",
        receipt.evaluate.detail, receipt.evaluate.status
    );
    eprintln!(
        "| Deep truncation/pagination correctness | {} | page [250..274] over @big | {} |",
        receipt.deep_pagination.detail, receipt.deep_pagination.status
    );
    eprintln!(
        "| Memory footprint baseline (portable proxy) | {} | best-effort baseline | {} |",
        receipt.memory.detail, receipt.memory.status
    );
    eprintln!("<!-- END: DAP_SESSION_SCORECARD -->");
    eprintln!();
}

#[test]
fn scorecard_launch_success_rate() -> TestResult {
    if !perl_available() {
        let skipped = ScorecardReceipt {
            perl_available: false,
            launch: RateMetric {
                passed: 0,
                total: 0,
                threshold_pct: 80,
                p50_ms: None,
                p95_ms: None,
                details: Vec::new(),
            },
            attach: RateMetric {
                passed: 0,
                total: 0,
                threshold_pct: 80,
                p50_ms: None,
                p95_ms: None,
                details: Vec::new(),
            },
            variables: BinaryMetric { status: "SKIP", detail: "perl not on PATH".to_string() },
            evaluate: BinaryMetric { status: "SKIP", detail: "perl not on PATH".to_string() },
            deep_pagination: BinaryMetric {
                status: "SKIP",
                detail: "perl not on PATH".to_string(),
            },
            memory: memory_metric(),
        };
        print_marker_friendly_summary(&skipped);
        write_receipt(&skipped)?;
        eprintln!("scorecard_launch_success_rate: skipping — perl not on PATH");
        return Ok(());
    }

    // Attach probes involve zero perl — a fake TCP server stands in for the
    // debuggee — so they run regardless of whether a pipe-capable debuggee
    // interpreter resolves; gating them on identity resolution would silently
    // discard perl-independent attach proof (#12594 item 6 review). Run 5
    // attach probes so the 80 % threshold (div_ceil(5 * 80 / 100) = 4) is
    // meaningful: exactly 1 failure is tolerated.  With n < 5 the ceil
    // rounding makes the effective threshold 100 %, defeating the intent.
    let mut attach_results: Vec<FixtureResult> = Vec::new();
    for _attempt in 0..5 {
        let (elapsed_ms, error) = match probe_attach(launch_timeout()) {
            Ok(()) => (None, None),
            Err(err) => (None, Some(err)),
        };
        attach_results.push(FixtureResult { name: "tcp_loopback", elapsed_ms, error });
    }
    let attach_passed = attach_results.iter().filter(|r| r.passed()).count();

    // Live launch/session probes need an interpreter whose perl5db actually
    // operates over piped stdio; native MSWin32 builds hang at bootstrap
    // (#12594 item 6b). Record the gap as a typed skip rather than letting
    // every session metric fail on timeouts. Only the live-session probes are
    // gated on this resolution.
    let Some(debuggee_perl) = debuggee_perl_or_typed_skip("scorecard_launch_success_rate") else {
        let skipped = ScorecardReceipt {
            perl_available: true,
            launch: RateMetric {
                passed: 0,
                total: 0,
                threshold_pct: 80,
                p50_ms: None,
                p95_ms: None,
                details: Vec::new(),
            },
            attach: RateMetric {
                passed: attach_passed,
                total: attach_results.len(),
                threshold_pct: 80,
                p50_ms: None,
                p95_ms: None,
                details: attach_results,
            },
            variables: BinaryMetric {
                status: "SKIP",
                detail: "no pipe-capable perl debugger for live sessions".to_string(),
            },
            evaluate: BinaryMetric {
                status: "SKIP",
                detail: "no pipe-capable perl debugger for live sessions".to_string(),
            },
            deep_pagination: BinaryMetric {
                status: "SKIP",
                detail: "no pipe-capable perl debugger for live sessions".to_string(),
            },
            memory: memory_metric(),
        };
        print_marker_friendly_summary(&skipped);
        write_receipt(&skipped)?;
        assert_attach_rate(&skipped);
        eprintln!(
            "scorecard_launch_success_rate: skipping live-session probes — no pipe-capable perl \
             debugger"
        );
        return Ok(());
    };

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let fixture_dir = Path::new(&manifest_dir).join("tests").join("fixtures");
    let fixtures: &[(&str, &str)] = &[
        ("hello", "hello.pl"),
        ("loops", "loops.pl"),
        ("eval", "eval.pl"),
        ("args", "args.pl"),
        ("begin_end", "breakpoints_begin_end.pl"),
    ];

    let mut launch_results: Vec<FixtureResult> = Vec::with_capacity(fixtures.len());
    for (name, filename) in fixtures {
        let path = fixture_dir.join(filename);
        let (elapsed_ms, error) = match probe_launch(&path, &debuggee_perl.binary, launch_timeout())
        {
            Ok(ms) => (Some(ms), None),
            Err(err) => (None, Some(err)),
        };
        launch_results.push(FixtureResult { name, elapsed_ms, error });
    }

    let mut latencies: Vec<u128> = launch_results.iter().filter_map(|r| r.elapsed_ms).collect();
    latencies.sort_unstable();

    let launch_passed = launch_results.iter().filter(|r| r.passed()).count();

    let (variables, evaluate, deep_pagination) = match probe_session_metrics(&debuggee_perl.binary)
    {
        Ok(metrics) => metrics,
        Err(err) => (
            BinaryMetric { status: "FAIL", detail: format!("session setup failed: {err}") },
            BinaryMetric { status: "FAIL", detail: "session setup failed".to_string() },
            BinaryMetric { status: "FAIL", detail: "session setup failed".to_string() },
        ),
    };

    let receipt = ScorecardReceipt {
        perl_available: true,
        launch: RateMetric {
            passed: launch_passed,
            total: launch_results.len(),
            threshold_pct: 80,
            p50_ms: percentile(&latencies, 50),
            p95_ms: percentile(&latencies, 95),
            details: launch_results,
        },
        attach: RateMetric {
            passed: attach_passed,
            total: attach_results.len(),
            threshold_pct: 80,
            p50_ms: None,
            p95_ms: None,
            details: attach_results,
        },
        variables,
        evaluate,
        deep_pagination,
        memory: memory_metric(),
    };

    print_marker_friendly_summary(&receipt);
    write_receipt(&receipt)?;

    let launch_threshold =
        (receipt.launch.total * usize::from(receipt.launch.threshold_pct)).div_ceil(100);
    assert!(
        receipt.launch.passed >= launch_threshold,
        "DAP launch success rate below threshold: {}/{} passed (need ≥{})",
        receipt.launch.passed,
        receipt.launch.total,
        launch_threshold
    );
    assert_attach_rate(&receipt);
    assert_eq!(
        receipt.variables.status, "PASS",
        "variables scorecard failed: {}",
        receipt.variables.detail
    );
    assert_eq!(
        receipt.evaluate.status, "PASS",
        "evaluate scorecard failed: {}",
        receipt.evaluate.detail
    );
    assert!(
        receipt.deep_pagination.status == "PASS" || receipt.deep_pagination.status == "NOT_PROVEN",
        "deep pagination scorecard failed: {}",
        receipt.deep_pagination.detail
    );

    Ok(())
}
