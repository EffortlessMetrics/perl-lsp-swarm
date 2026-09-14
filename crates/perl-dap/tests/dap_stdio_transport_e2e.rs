//! End-to-end DAP stdio transport tests.
//!
//! These tests spawn the packaged `perl-dap` binary and communicate over real
//! `Content-Length` framed stdio messages. They complement the in-process
//! adapter workflow tests by covering the transport loop, event writer thread,
//! and request/response framing as an editor client would observe them.
//!
//! `PERL_DAP_TEST_BINARY` can point the smoke at an explicitly extracted
//! release candidate. When it is absent, Cargo's test binary remains the
//! default so ordinary crate tests preserve their current behavior.

use anyhow::{Context, Result, anyhow};
use perl_dap::DapMessage;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const EXPLICIT_DAP_BINARY_ENV: &str = "PERL_DAP_TEST_BINARY";
const DAP_SMOKE_RECEIPT_ENV: &str = "PERL_DAP_SMOKE_RECEIPT";
const DAP_SMOKE_SCHEMA_VERSION: &str = "perl_dap_stdio_smoke.v1";
const DAP_SMOKE_CLAIM_BOUNDARY: &str = concat!(
    "real Content-Length framed stdio initialize, threads, and disconnect smoke ",
    "against the configured perl-dap binary; no debug session is started, so ",
    "no terminated event is expected; does not ",
    "prove launch/debug-session semantics or platform-wide process cleanup"
);

struct DapProcess {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<std::result::Result<DapMessage, String>>,
    reader: Option<thread::JoinHandle<()>>,
    observed: Arc<Mutex<Vec<(i64, String)>>>,
}

type FrameObservation = Arc<Mutex<Vec<(i64, String)>>>;
type FrameReader =
    (Receiver<std::result::Result<DapMessage, String>>, thread::JoinHandle<()>, FrameObservation);

struct ReleaseMarkerGuard {
    release: std::path::PathBuf,
    markers: [std::path::PathBuf; 2],
    pid: Option<u32>,
}

impl ReleaseMarkerGuard {
    fn set_pid(&mut self, pid: &str) -> Result<()> {
        let parsed = pid
            .parse::<u32>()
            .ok()
            .filter(|pid| *pid > 0)
            .ok_or_else(|| anyhow!("debuggee marker contained invalid PID: {pid:?}"))?;
        self.pid = Some(parsed);
        Ok(())
    }
}

impl Drop for ReleaseMarkerGuard {
    fn drop(&mut self) {
        let _ = fs::write(&self.release, "release");
        if let Some(pid) = self.pid {
            terminate_debuggee_bounded(pid);
        }
        let _ = fs::remove_file(&self.release);
        for marker in &self.markers {
            let _ = fs::remove_file(marker);
        }
    }
}

fn terminate_debuggee_bounded(pid: u32) {
    let mut child = if cfg!(windows) {
        Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]).spawn().ok()
    } else {
        Command::new("kill").args(["-TERM", &pid.to_string()]).spawn().ok()
    };
    let Some(ref mut child) = child else { return };
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => return,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

impl DapProcess {
    fn spawn_binary(binary: &OsString) -> Result<Self> {
        let mut child = Command::new(binary)
            .arg("--stdio")
            .arg("--log-level")
            .arg("error")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to spawn configured perl-dap binary {binary:?}"))?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow!("child stdin was not piped"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("child stdout was not piped"))?;
        let (rx, reader, observed) = spawn_frame_reader(stdout);

        Ok(Self { child, stdin, rx, reader: Some(reader), observed })
    }

    fn send_request(&mut self, seq: i64, command: &str, arguments: Option<Value>) -> Result<()> {
        let payload = serde_json::to_vec(&json!({
            "type": "request",
            "seq": seq,
            "command": command,
            "arguments": arguments,
        }))?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n", payload.len())?;
        self.stdin.write_all(&payload)?;
        self.stdin.flush()?;
        Ok(())
    }

    fn wait_for_response(&self, request_seq: i64, command: &str) -> Result<Option<Value>> {
        wait_for_message(
            &self.rx,
            format!("response `{command}` for request {request_seq}"),
            |msg| {
                matches!(
                    msg,
                    DapMessage::Response {
                        request_seq: actual_request_seq,
                        command: actual_command,
                        ..
                    } if *actual_request_seq == request_seq && actual_command == command
                )
            },
        )
        .and_then(|message| match message {
            DapMessage::Response { success, body, message, .. } => {
                if success {
                    Ok(body)
                } else {
                    Err(anyhow!(
                        "response `{command}` for request {request_seq} failed: {}",
                        message.unwrap_or_else(|| "<no message>".to_string())
                    ))
                }
            }
            other => Err(anyhow!("expected response `{command}`, got {other:?}")),
        })
    }

    fn wait_for_response_message(&self, request_seq: i64, command: &str) -> Result<DapMessage> {
        wait_for_message(
            &self.rx,
            format!("response `{command}` for request {request_seq}"),
            |msg| {
                matches!(
                    msg,
                    DapMessage::Response {
                        request_seq: actual_request_seq,
                        command: actual_command,
                        ..
                    } if *actual_request_seq == request_seq && actual_command == command
                )
            },
        )
    }

    fn wait_for_event(&self, event_name: &str) -> Result<Option<Value>> {
        wait_for_message(
            &self.rx,
            format!("event `{event_name}`"),
            |msg| matches!(msg, DapMessage::Event { event, .. } if event == event_name),
        )
        .and_then(|message| match message {
            DapMessage::Event { body, .. } => Ok(body),
            other => Err(anyhow!("expected event `{event_name}`, got {other:?}")),
        })
    }

    fn finish_cleanly(&mut self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.child.try_wait()?.is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let status = self
            .child
            .try_wait()?
            .ok_or_else(|| anyhow!("adapter did not exit within cleanup bound"))?;
        if !status.success() {
            return Err(anyhow!("adapter exited unsuccessfully during cleanup: {status}"));
        }
        while !self.reader.as_ref().is_some_and(|reader| reader.is_finished())
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
        }
        if !self.reader.as_ref().is_some_and(|reader| reader.is_finished()) {
            return Err(anyhow!("DAP stdout reader did not finish within cleanup bound"));
        }
        let reader =
            self.reader.take().ok_or_else(|| anyhow!("DAP stdout reader was already consumed"))?;
        if !reader.is_finished() {
            return Err(anyhow!("DAP stdout reader did not finish within cleanup bound"));
        }
        reader.join().map_err(|_| anyhow!("DAP stdout reader panicked"))
    }
}

impl Drop for DapProcess {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !reader.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if reader.is_finished() {
                let _ = reader.join();
            }
        }
    }
}

fn configured_dap_binary_path() -> OsString {
    dap_binary_path(std::env::var_os(EXPLICIT_DAP_BINARY_ENV))
}

fn dap_binary_path(explicit: Option<OsString>) -> OsString {
    explicit.unwrap_or_else(|| OsString::from(env!("CARGO_BIN_EXE_perl-dap")))
}

fn write_smoke_receipt(binary: &OsString) -> Result<()> {
    let Some(output) = std::env::var_os(DAP_SMOKE_RECEIPT_ENV) else {
        return Ok(());
    };
    write_smoke_receipt_to(Path::new(&output), binary)
}

fn write_smoke_receipt_to(output: &Path, binary: &OsString) -> Result<()> {
    if let Some(parent) = output.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create DAP smoke receipt directory {parent:?}"))?;
    }
    let receipt = json!({
        "schema_version": DAP_SMOKE_SCHEMA_VERSION,
        "status": "pass",
        "binary": binary.to_string_lossy(),
        "commands": ["initialize", "threads", "disconnect"],
        "initialized_event": true,
        "terminated_event": false,
        "timeout_seconds": 5,
        "claim_boundary": DAP_SMOKE_CLAIM_BOUNDARY,
    });
    let rendered = serde_json::to_string_pretty(&receipt)?;
    fs::write(output, format!("{rendered}\n"))
        .with_context(|| format!("failed to write DAP smoke receipt {output:?}"))?;
    Ok(())
}

fn spawn_frame_reader<R>(mut reader: R) -> FrameReader
where
    R: Read + Send + 'static,
{
    let (tx, rx) = channel();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let reader_observed = Arc::clone(&observed);
    let reader_thread = thread::spawn(move || {
        loop {
            match read_framed_message(&mut reader) {
                Ok(Some(message)) => {
                    if let DapMessage::Response { request_seq, command, .. } = &message
                        && let Ok(mut entries) = reader_observed.lock()
                    {
                        entries.push((*request_seq, command.clone()));
                    }
                    if tx.send(Ok(message)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = tx.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    (rx, reader_thread, observed)
}

fn read_framed_message<R: Read>(reader: &mut R) -> Result<Option<DapMessage>> {
    let mut header = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        if reader.read(&mut byte).context("failed to read DAP frame header")? == 0 {
            if header.is_empty() {
                return Ok(None);
            }
            return Err(anyhow!("EOF inside DAP frame header"));
        }
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
        if header.len() > 1024 {
            return Err(anyhow!("DAP frame header exceeded 1024 bytes"));
        }
    }

    let header_text = std::str::from_utf8(&header).context("DAP frame header was not UTF-8")?;
    let content_length = header_text
        .split("\r\n")
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .ok_or_else(|| anyhow!("DAP frame header missing Content-Length: {header_text:?}"))?
        .parse::<usize>()
        .context("DAP Content-Length was not a positive integer")?;

    let mut body = vec![0_u8; content_length];
    reader.read_exact(&mut body).context("failed to read DAP frame body")?;
    serde_json::from_slice(&body).map(Some).context("DAP frame body was not a DapMessage")
}

fn wait_for_message<F>(
    rx: &Receiver<std::result::Result<DapMessage, String>>,
    description: String,
    matches_message: F,
) -> Result<DapMessage>
where
    F: Fn(&DapMessage) -> bool,
{
    let timeout = Duration::from_secs(5);
    let deadline = Instant::now() + timeout;
    let mut observed = Vec::new();

    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(anyhow!("timeout waiting for {description}; observed {observed:?}"));
        }

        match rx.recv_timeout(deadline.saturating_duration_since(now)) {
            Ok(Ok(message)) if matches_message(&message) => return Ok(message),
            Ok(Ok(message)) => observed.push(message_label(&message)),
            Ok(Err(error)) => {
                return Err(anyhow!("DAP reader failed while waiting for {description}: {error}"));
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(anyhow!("timeout waiting for {description}; observed {observed:?}"));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(anyhow!(
                    "DAP reader disconnected while waiting for {description}; observed {observed:?}"
                ));
            }
        }
    }
}

fn message_label(message: &DapMessage) -> String {
    match message {
        DapMessage::Request { command, .. } => format!("request:{command}"),
        DapMessage::Response { command, request_seq, success, .. } => {
            format!("response:{command}#{request_seq}:success={success}")
        }
        DapMessage::Event { event, .. } => format!("event:{event}"),
    }
}

#[test]
fn explicit_binary_override_is_preferred() {
    let explicit = OsString::from("/tmp/release-candidate/perl-dap");
    assert_eq!(dap_binary_path(Some(explicit.clone())), explicit);
}

#[test]
fn cargo_binary_remains_the_fallback() {
    assert_eq!(dap_binary_path(None), OsString::from(env!("CARGO_BIN_EXE_perl-dap")));
}

#[test]
fn smoke_receipt_binds_the_configured_binary() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let output = temp.path().join("dap-smoke.json");
    let binary = OsString::from("/tmp/release-candidate/perl-dap");
    write_smoke_receipt_to(&output, &binary)?;

    let value: Value = serde_json::from_str(&fs::read_to_string(output)?)?;
    assert_eq!(value.get("schema_version").and_then(Value::as_str), Some(DAP_SMOKE_SCHEMA_VERSION));
    assert_eq!(value.get("status").and_then(Value::as_str), Some("pass"));
    assert_eq!(
        value.get("binary").and_then(Value::as_str),
        Some("/tmp/release-candidate/perl-dap")
    );
    Ok(())
}

#[test]
fn stdio_transport_framing_initialize_threads_disconnect() -> Result<()> {
    let binary = configured_dap_binary_path();
    let mut dap = DapProcess::spawn_binary(&binary)?;

    dap.send_request(
        1,
        "initialize",
        Some(json!({
            "clientID": "perl-dap-stdio-e2e",
            "adapterID": "perl-dap",
            "pathFormat": "path",
            "linesStartAt1": true,
            "columnsStartAt1": true,
        })),
    )?;
    let init_body = dap
        .wait_for_response(1, "initialize")?
        .ok_or_else(|| anyhow!("initialize response missing capability body"))?;
    assert_eq!(
        init_body.get("supportsConfigurationDoneRequest").and_then(Value::as_bool),
        Some(true),
        "initialize over stdio must advertise configurationDone support"
    );
    // #9089: the routed inlineValues extension is a project extension kept
    // outside standard DAP capability accounting; it must stay unadvertised
    // over the stdio transport too, until the negotiation gate passes.
    assert_eq!(
        init_body.get("supportsInlineValues").and_then(Value::as_bool),
        Some(false),
        "initialize over stdio must not advertise the unnegotiated inlineValues extension"
    );
    dap.wait_for_event("initialized")?;

    dap.send_request(2, "threads", None)?;
    let threads_body = dap
        .wait_for_response(2, "threads")?
        .ok_or_else(|| anyhow!("threads response missing body"))?;
    let threads = threads_body
        .get("threads")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("threads response body missing `threads` array"))?;
    assert!(threads.is_empty(), "stdio e2e starts without an active debuggee");

    dap.send_request(3, "disconnect", Some(json!({})))?;
    // Observe the complete remaining stream: an immediate try_recv would race
    // the event writer and could miss a terminal event before or after the response.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut acknowledged = false;
    loop {
        match dap.rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(DapMessage::Response { request_seq: 3, command, success: true, .. }))
                if command == "disconnect" && !acknowledged =>
            {
                acknowledged = true;
            }
            Ok(message) => return Err(anyhow!("unexpected disconnect message: {message:?}")),
            Err(RecvTimeoutError::Disconnected) if acknowledged => break,
            Err(error) => return Err(anyhow!("disconnect stream did not complete: {error}")),
        }
    }
    loop {
        if let Some(status) = dap.child.try_wait()? {
            if !status.success() {
                return Err(anyhow!("adapter exited unsuccessfully after disconnect: {status}"));
            }
            break;
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("adapter did not exit after disconnect"));
        }
        thread::sleep(Duration::from_millis(10));
    }
    write_smoke_receipt(&binary)?;

    Ok(())
}

#[cfg(windows)]
fn assert_debuggee_absent(pid: &str) -> Result<()> {
    let probe = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()?;
    if !probe.status.success() {
        return Err(anyhow!(
            "debuggee cleanup probe failed: {}; stdout={:?}; stderr={:?}",
            probe.status,
            String::from_utf8_lossy(&probe.stdout),
            String::from_utf8_lossy(&probe.stderr)
        ));
    }
    let output = String::from_utf8_lossy(&probe.stdout);
    if output.lines().any(|line| line.split(',').nth(1).is_some_and(|v| v.trim_matches('"') == pid))
    {
        return Err(anyhow!("debuggee PID {pid} remained after disconnect: {output}"));
    }
    Ok(())
}

#[cfg(unix)]
fn assert_debuggee_absent(pid: &str) -> Result<()> {
    let probe = Command::new("ps").args(["-p", pid, "-o", "pid="]).output()?;
    if !probe.status.success() && probe.status.code() != Some(1) {
        return Err(anyhow!("debuggee cleanup probe failed: {}", probe.status));
    }
    if !String::from_utf8_lossy(&probe.stdout).trim().is_empty() {
        return Err(anyhow!("debuggee PID {pid} remained after disconnect"));
    }
    Ok(())
}

#[test]
fn native_stdio_cancel_reaches_blocked_evaluate_and_recovers() -> Result<()> {
    let perl_probe = Command::new("perl")
        .arg("-e")
        .arg("exit 0")
        .status()
        .context("native stdio cancellation proof requires a runnable Perl interpreter")?;
    if !perl_probe.success() {
        return Err(anyhow!("native stdio Perl interpreter probe failed: {perl_probe}"));
    }
    let workspace = tempfile::tempdir()?;
    let script = workspace.path().join("cancel_stdio.pl");
    fs::write(&script, "use strict;\nuse warnings;\n\nmy $x = 1;\nsleep 60;\n")?;
    let started = workspace.path().join("evaluate-started");
    let replacement_started = workspace.path().join("replacement-started");
    let release = workspace.path().join("evaluate-release");
    let mut release_guard = ReleaseMarkerGuard {
        release: release.clone(),
        markers: [started.clone(), replacement_started.clone()],
        pid: None,
    };
    let perl_path =
        |path: &std::path::Path| path.to_string_lossy().replace('\\', "/").replace('\'', "\\'");
    let started_path = perl_path(&started);
    let replacement_started_path = perl_path(&replacement_started);
    let release_path = perl_path(&release);

    let binary = configured_dap_binary_path();
    let mut dap = DapProcess::spawn_binary(&binary)?;
    dap.send_request(
        1,
        "initialize",
        Some(json!({"clientID": "cancel-e2e", "adapterID": "perl-dap", "pathFormat": "path"})),
    )?;
    let init = dap
        .wait_for_response(1, "initialize")?
        .ok_or_else(|| anyhow!("initialize response missing capability body"))?;
    if init.get("supportsCancelRequest").and_then(Value::as_bool) != Some(true) {
        return Err(anyhow!("native stdio must advertise supportsCancelRequest after promotion"));
    }
    dap.wait_for_event("initialized")?;

    dap.send_request(
        2,
        "launch",
        Some(json!({
            "program": script,
            "stopOnEntry": true,
        })),
    )?;
    dap.wait_for_response(2, "launch")?;
    dap.wait_for_event("stopped")?;

    // Establish the debugger's native formatting for the follow-up expression
    // before introducing cancellation; `x` treats an unparenthesized leading
    // number as a depth/count argument.
    dap.send_request(3, "evaluate", Some(json!({"expression": "(6 * 7)", "context": "repl"})))?;
    let baseline = dap
        .wait_for_response(3, "evaluate")?
        .ok_or_else(|| anyhow!("baseline evaluate response missing body"))?;
    let baseline_result = baseline
        .get("result")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("baseline evaluate result was not a string: {baseline:?}"))?;
    if baseline_result.split_whitespace().collect::<Vec<_>>() != ["0", "42"] {
        return Err(anyhow!("baseline evaluate did not produce 42: {baseline:?}"));
    }

    dap.send_request(
        4,
        "evaluate",
        Some(json!({
            "expression": format!(
                "do {{ open(F, '>', '{}') or die $!; print F 'started ', $$; close F; while (!-e '{}') {{ select undef, undef, undef, 0.01 }}; 987654321 }}",
                started_path,
                release_path,
            ),
            "context": "repl",
            "allowSideEffects": true,
        })),
    )?;
    let started_deadline = Instant::now() + Duration::from_secs(2);
    while !started.exists() && Instant::now() < started_deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if !started.exists() {
        return Err(anyhow!("native evaluate did not reach its blocking expression"));
    }
    let started_marker = fs::read_to_string(&started)?;
    let started_pid = started_marker
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("debuggee marker did not contain its PID: {started_marker:?}"))?;
    release_guard.set_pid(started_pid)?;
    dap.send_request(5, "cancel", Some(json!({"requestId": 4})))?;
    let first = wait_for_message(&dap.rx, "cancel/evaluate responses".to_string(), |msg| {
        matches!(msg, DapMessage::Response { request_seq: 4 | 5, .. })
    })?;
    let (cancel, evaluate_before_cancel) = match first {
        message @ DapMessage::Response { request_seq: 5, .. } => (message, None),
        message @ DapMessage::Response { request_seq: 4, .. } => {
            (dap.wait_for_response_message(5, "cancel")?, Some(message))
        }
        message => return Err(anyhow!("unexpected cancellation response: {message:?}")),
    };
    if !matches!(cancel, DapMessage::Response { success: true, .. }) {
        return Err(anyhow!("cancel was not accepted: {cancel:?}"));
    }
    // Both correlated responses must settle before releasing the blocked
    // debugger expression; this proves cancellation did not depend on its
    // eventual completion.
    let evaluate = match evaluate_before_cancel {
        Some(message) => message,
        None => dap.wait_for_response_message(4, "evaluate")?,
    };
    if !matches!(evaluate, DapMessage::Response { success: false, message: Some(ref message), .. } if message.contains("cancelled"))
    {
        return Err(anyhow!("blocked evaluate did not settle as cancelled: {evaluate:?}"));
    }
    fs::write(&release, "release")?;

    dap.send_request(6, "evaluate", Some(json!({"expression": "6*7", "context": "repl"})))?;
    let follow_up = dap
        .wait_for_response(6, "evaluate")?
        .ok_or_else(|| anyhow!("follow-up evaluate response missing body"))?;
    let follow_up_result = follow_up
        .get("result")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("follow-up evaluate result was not a string: {follow_up:?}"))?;
    if follow_up_result != baseline_result
        || follow_up_result.split_whitespace().collect::<Vec<_>>() != ["0", "42"]
    {
        return Err(anyhow!(
            "follow-up evaluate was contaminated by cancelled output: {follow_up:?}"
        ));
    }
    // Late duplicate and unknown cancellation must be correlated to their own
    // acknowledgements and leave a healthy request untouched.
    dap.send_request(7, "cancel", Some(json!({"requestId": 4})))?;
    dap.send_request(8, "cancel", Some(json!({"requestId": 999})))?;
    for request_seq in [7, 8] {
        let response = dap.wait_for_response_message(request_seq, "cancel")?;
        if !matches!(response, DapMessage::Response { success: true, .. }) {
            return Err(anyhow!("cancel control {request_seq} failed: {response:?}"));
        }
    }
    dap.send_request(9, "evaluate", Some(json!({"expression": "(6 * 7)", "context": "repl"})))?;
    let healthy = dap
        .wait_for_response(9, "evaluate")?
        .ok_or_else(|| anyhow!("healthy post-cancel evaluate response missing body"))?;
    let healthy_result = healthy.get("result").and_then(Value::as_str).unwrap_or("");
    if healthy_result != baseline_result {
        return Err(anyhow!("cancel controls changed healthy evaluate: {healthy:?}"));
    }
    dap.send_request(10, "threads", None)?;
    let _ = dap.wait_for_response(10, "threads")?;
    dap.send_request(11, "terminate", Some(json!({})))?;
    let terminate = dap.wait_for_response_message(11, "terminate")?;
    if !matches!(terminate, DapMessage::Response { success: true, .. }) {
        return Err(anyhow!("terminate failed after cancellation: {terminate:?}"));
    }
    let duplicate_terminal = dap
        .observed
        .lock()
        .map(|entries| {
            entries.iter().filter(|(seq, command)| *seq == 4 && command == "evaluate").count()
        })
        .map_err(|_| anyhow!("DAP frame observation log was poisoned"))?;
    if duplicate_terminal != 1 {
        return Err(anyhow!(
            "request 4 must have exactly one observed evaluate response before sequence reuse: {duplicate_terminal}"
        ));
    }
    {
        let marker = fs::read_to_string(&started)?;
        let pid = marker
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| anyhow!("debuggee marker did not contain its PID: {marker:?}"))?;
        release_guard.set_pid(pid)?;
        assert_debuggee_absent(pid)?;
        release_guard.pid = None;
    }

    // The same adapter session must not let a terminal request lease poison a
    // replacement launch that reuses its request sequence.
    dap.send_request(12, "cancel", Some(json!({"requestId": 4})))?;
    let old_cancel = dap.wait_for_response_message(12, "cancel")?;
    if !matches!(old_cancel, DapMessage::Response { success: true, .. }) {
        return Err(anyhow!("old request cancel poisoned replacement: {old_cancel:?}"));
    }
    dap.send_request(13, "launch", Some(json!({"program": script, "stopOnEntry": true})))?;
    dap.wait_for_response(13, "launch")?;
    dap.wait_for_event("stopped")?;
    dap.send_request(4, "evaluate", Some(json!({
        "expression": format!("do {{ open(F, '>', '{}') or die $!; print F 'started ', $$; close F; (6 * 7) }}", replacement_started_path),
        "context": "repl",
        "allowSideEffects": true
    })))?;
    let replacement_eval = dap
        .wait_for_response(4, "evaluate")?
        .ok_or_else(|| anyhow!("replacement evaluate body missing"))?;
    let replacement_marker = fs::read_to_string(&replacement_started)?;
    let replacement_pid = replacement_marker.split_whitespace().nth(1).ok_or_else(|| {
        anyhow!("replacement marker did not contain its PID: {replacement_marker:?}")
    })?;
    release_guard.set_pid(replacement_pid)?;
    if replacement_eval.get("result").and_then(Value::as_str) != Some(baseline_result) {
        return Err(anyhow!(
            "replacement request sequence inherited stale cancellation: {replacement_eval:?}"
        ));
    }
    dap.send_request(14, "disconnect", Some(json!({})))?;
    let disconnect = dap.wait_for_response_message(14, "disconnect")?;
    if !matches!(disconnect, DapMessage::Response { success: true, .. }) {
        return Err(anyhow!("disconnect failed after replacement: {disconnect:?}"));
    }
    dap.finish_cleanly()?;
    assert_debuggee_absent(replacement_pid)?;
    release_guard.pid = None;
    Ok(())
}

#[test]
fn stdio_transport_stops_when_client_closes_stdout_while_stdin_remains_open() -> Result<()> {
    let binary = configured_dap_binary_path();
    let mut child = Command::new(&binary)
        .arg("--stdio")
        .arg("--log-level")
        .arg("error")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to spawn configured perl-dap binary {binary:?}"))?;
    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("child stdin was not piped"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("child stdout was not piped"))?;
    let (response_tx, response_rx) = channel();
    let mut reader = Some(thread::spawn(move || {
        let mut stdout = stdout;
        response_tx.send((read_framed_message(&mut stdout), stdout))
    }));
    let result = (|| -> Result<()> {
        let initialize = serde_json::to_vec(&json!({
            "type": "request",
            "seq": 1,
            "command": "initialize",
            "arguments": { "adapterID": "perl-dap" },
        }))?;
        write!(stdin, "Content-Length: {}\r\n\r\n", initialize.len())?;
        stdin.write_all(&initialize)?;
        stdin.flush()?;
        let (initialize_result, stdout) = response_rx.recv_timeout(Duration::from_secs(3))?;
        let initialize_response = initialize_result
            .map_err(|error| anyhow!("stdio response reader failed: {error:?}"))?
            .ok_or_else(|| anyhow!("adapter closed stdout before initialize response"))?;
        match initialize_response {
            DapMessage::Response { request_seq: 1, success: true, .. } => {}
            other => {
                return Err(anyhow!("initialize did not succeed before stdout closure: {other:?}"));
            }
        }
        if child.try_wait()?.is_some() {
            return Err(anyhow!("adapter exited before the client closed stdout after initialize"));
        }
        drop(stdout);
        reader
            .take()
            .ok_or_else(|| anyhow!("stdio response reader was already joined"))?
            .join()
            .map_err(|_| anyhow!("stdio response reader panicked"))??;
        // The reader thread has returned and dropped the only stdout handle.
        // Keep stdin open while exercising the next request against the closed
        // client output boundary.
        let threads = serde_json::to_vec(&json!({
            "type": "request",
            "seq": 2,
            "command": "threads",
        }))?;
        let threads_write = (|| -> Result<()> {
            write!(stdin, "Content-Length: {}\r\n\r\n", threads.len())?;
            stdin.write_all(&threads)?;
            stdin.flush()?;
            Ok(())
        })();
        if let Err(error) = threads_write
            && error.downcast_ref::<std::io::Error>().map(std::io::Error::kind)
                != Some(std::io::ErrorKind::BrokenPipe)
        {
            return Err(error);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait()? {
                if status.success() {
                    return Err(anyhow!(
                        "adapter reported success after its stdout was closed: {status}"
                    ));
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "adapter did not stop after client stdout closure while stdin remained open"
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
    })();
    drop(stdin);
    if reader.as_ref().is_some_and(|reader| !reader.is_finished()) {
        let _ = child.kill();
    }
    if let Some(reader) = reader {
        let _ = reader.join();
    }
    if child.try_wait()?.is_none() {
        let _ = child.kill();
    }
    child.wait()?;
    result
}
