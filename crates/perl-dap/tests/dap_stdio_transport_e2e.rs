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
    fn set_pid(&mut self, pid: u32) -> Result<()> {
        if pid == 0 {
            return Err(anyhow!("debuggee marker contained invalid PID: {pid:?}"));
        }
        self.pid = Some(pid);
        Ok(())
    }
}

#[test]
fn marker_pid_requires_complete_validated_record() -> Result<()> {
    let workspace = tempfile::tempdir()?;
    let marker = workspace.path().join("started");
    fs::write(&marker, "started 123")?;
    if read_validated_pid(&marker, Instant::now()).is_ok() {
        return Err(anyhow!("partial PID marker must not be accepted"));
    }
    fs::write(&marker, "started 1234\n")?;
    if read_validated_pid(&marker, Instant::now())? != 1234 {
        return Err(anyhow!("complete PID marker parsed incorrectly"));
    }
    Ok(())
}

fn read_validated_pid(path: &Path, deadline: Instant) -> Result<u32> {
    loop {
        if let Ok(contents) = fs::read_to_string(path)
            && contents.ends_with('\n')
            && contents.lines().count() == 1
        {
            let mut fields = contents.split_whitespace();
            if fields.next() == Some("started")
                && let Some(pid) = fields.next()
                && fields.next().is_none()
                && let Ok(pid) = pid.parse::<u32>()
                && pid > 0
            {
                return Ok(pid);
            }
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("marker did not contain one complete validated PID: {path:?}"));
        }
        thread::sleep(Duration::from_millis(10));
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
            // Transport e2e exercises framing/session behavior, not the
            // launch-authority contract (#8656): without an explicit
            // acknowledgement every `launch` is refused.
            .arg("--allow-unbounded")
            .arg("--unbounded-note")
            .arg("test: stdio transport e2e")
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
                "do {{ open(F, '>', '{}') or die $!; print F 'started ', $$, \"\\n\"; close F; while (!-e '{}') {{ select undef, undef, undef, 0.01 }}; 987654321 }}",
                started_path,
                release_path,
            ),
            "context": "repl",
            "allowSideEffects": true,
        })),
    )?;
    let started_pid = read_validated_pid(&started, Instant::now() + Duration::from_secs(2))?;
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
        assert_debuggee_absent(&started_pid.to_string())?;
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
        "expression": format!("do {{ open(F, '>', '{}') or die $!; print F 'started ', $$, \"\\n\"; close F; (6 * 7) }}", replacement_started_path),
        "context": "repl",
        "allowSideEffects": true
    })))?;
    let replacement_eval = dap
        .wait_for_response(4, "evaluate")?
        .ok_or_else(|| anyhow!("replacement evaluate body missing"))?;
    let replacement_pid =
        read_validated_pid(&replacement_started, Instant::now() + Duration::from_secs(2))?;
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
    assert_debuggee_absent(&replacement_pid.to_string())?;
    release_guard.pid = None;
    Ok(())
}

#[test]
fn stdio_transport_pid_attach_refuses_without_session_events() -> Result<()> {
    let binary = OsString::from(env!("CARGO_BIN_EXE_perl-dap"));
    let mut dap = DapProcess::spawn_binary(&binary)?;
    dap.send_request(
        1,
        "initialize",
        Some(json!({
            "clientID": "perl-dap-stdio-pid-refusal",
            "adapterID": "perl-dap",
            "pathFormat": "path",
        })),
    )?;
    dap.wait_for_response(1, "initialize")?;
    dap.wait_for_event("initialized")?;

    dap.send_request(
        2,
        "attach",
        Some(json!({
            "processId": std::process::id(),
            "stopOnEntry": true,
        })),
    )?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let (success, body, message) = match dap
        .rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        Ok(Ok(DapMessage::Response {
            request_seq: 2, command, success, body, message, ..
        })) if command == "attach" => (success, body, message),
        Ok(Ok(message)) => {
            return Err(anyhow!("unexpected message before PID refusal response: {message:?}"));
        }
        Ok(Err(error)) => return Err(anyhow!("stdio reader failed: {error}")),
        Err(error) => return Err(anyhow!("timed out waiting for PID refusal: {error}")),
    };
    if success || body.is_some() {
        return Err(anyhow!("PID attach was not refused without a response body"));
    }
    let message = message.ok_or_else(|| anyhow!("PID refusal omitted its diagnostic"))?;
    if !message.contains("not supported") || !message.contains("host") || !message.contains("port")
    {
        return Err(anyhow!("PID refusal omitted TCP remediation guidance: {message}"));
    }

    dap.send_request(3, "threads", None)?;
    let threads =
        match dap.rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(DapMessage::Response {
                request_seq: 3, command, success: true, body, ..
            })) if command == "threads" => body,
            Ok(Ok(message)) => return Err(anyhow!("unexpected threads response: {message:?}")),
            Ok(Err(error)) => return Err(anyhow!("stdio reader failed for threads: {error}")),
            Err(error) => return Err(anyhow!("timed out waiting for threads response: {error}")),
        }
        .ok_or_else(|| anyhow!("threads response omitted its body"))?;
    let threads = threads
        .get("threads")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("threads response omitted its threads array"))?;
    if !threads.is_empty() {
        return Err(anyhow!("refused PID attach fabricated threads: {threads:?}"));
    }

    dap.send_request(4, "disconnect", Some(json!({})))?;
    let mut disconnect_response = false;
    loop {
        match dap.rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(DapMessage::Response { request_seq: 4, command, success: true, .. }))
                if command == "disconnect" && !disconnect_response =>
            {
                disconnect_response = true
            }
            Ok(Ok(message)) => {
                return Err(anyhow!("unexpected post-refusal message: {message:?}"));
            }
            Ok(Err(error)) => return Err(anyhow!("stdio reader failed after refusal: {error}")),
            Err(RecvTimeoutError::Disconnected) if disconnect_response => break,
            Err(error) => return Err(anyhow!("stdio stream did not reach EOF: {error}")),
        }
    }
    loop {
        if let Some(status) = dap.child.try_wait()? {
            if !status.success() {
                return Err(anyhow!("adapter exited unsuccessfully after PID refusal: {status}"));
            }
            break;
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("adapter did not exit after PID refusal"));
        }
        thread::sleep(Duration::from_millis(10));
    }
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

// #17951: exercise untyped attach arguments through the exact public binary.
mod attach_option_validation {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    fn next(dap: &DapProcess) -> Result<DapMessage> {
        dap.rx
            .recv_timeout(Duration::from_secs(5))
            .context("missing next DAP frame")?
            .map_err(|error| anyhow!("DAP frame reader failed: {error}"))
    }

    fn response(dap: &DapProcess, seq: i64, command: &str, error: Option<&str>) -> Result<()> {
        match next(dap)? {
            DapMessage::Response {
                request_seq, command: actual, success, body, message, ..
            } if request_seq == seq && actual == command => {
                assert_eq!(success, error.is_none(), "{command}: {message:?}");
                assert!(body.is_none(), "unexpected response body: {body:?}");
                assert_eq!(message.as_deref(), error);
                Ok(())
            }
            other => Err(anyhow!("unexpected next frame for {command}/{seq}: {other:?}")),
        }
    }

    fn start() -> Result<DapProcess> {
        let mut dap = DapProcess::spawn_binary(&OsString::from(env!("CARGO_BIN_EXE_perl-dap")))?;
        dap.send_request(1, "initialize", Some(json!({"adapterID":"perl-dap"})))?;
        match next(&dap)? {
            DapMessage::Response { request_seq: 1, command, success: true, .. }
                if command == "initialize" => {}
            other => return Err(anyhow!("unexpected initialize response: {other:?}")),
        }
        match next(&dap)? {
            DapMessage::Event { event, .. } if event == "initialized" => {}
            other => return Err(anyhow!("unexpected initialize event: {other:?}")),
        }
        Ok(dap)
    }

    fn listener() -> Result<TcpListener> {
        // Bind failure is instrument failure, never a successful no-contact assertion.
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        Ok(listener)
    }

    fn no_contact(listener: &TcpListener) -> Result<()> {
        match listener.accept() {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
            Err(error) => Err(error.into()),
            Ok(_) => Err(anyhow!("refused attach contacted the owned listener")),
        }
    }

    fn arguments(listener: &TcpListener, mut options: Value) -> Result<Value> {
        options["host"] = json!("127.0.0.1");
        options["port"] = json!(listener.local_addr()?.port());
        Ok(options)
    }

    fn attach(dap: &mut DapProcess, seq: i64, options: Value) -> Result<TcpStream> {
        let listener = listener()?;
        dap.send_request(seq, "attach", Some(arguments(&listener, options)?))?;
        response(dap, seq, "attach", None)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match listener.accept() {
                Ok((peer, _)) => return Ok(peer),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(anyhow!("successful attach did not contact owned listener"));
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn finish(dap: &mut DapProcess, seq: i64) -> Result<()> {
        dap.send_request(seq, "disconnect", Some(json!({})))?;
        let mut responded = false;
        let mut terminated = false;
        loop {
            match dap.rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(DapMessage::Response { request_seq, command, success: true, .. }))
                    if request_seq == seq && command == "disconnect" && !responded =>
                {
                    responded = true;
                }
                Ok(Ok(DapMessage::Event { event, .. })) if event == "terminated" && !terminated => {
                    terminated = true
                }
                Err(RecvTimeoutError::Disconnected) if responded => break,
                other => return Err(anyhow!("unexpected disconnect frame: {other:?}")),
            }
        }
        dap.finish_cleanly()
    }

    fn refused_cases() -> Vec<(Value, String)> {
        let mut cases = Vec::new();
        for (value, kind) in [
            (json!("true"), "string"),
            (json!("false"), "string"),
            (json!(""), "string"),
            (json!(0), "number"),
            (json!(1), "number"),
            (json!([]), "array"),
            (json!({}), "object"),
        ] {
            cases.push((
                json!({"stopOnEntry": value}),
                format!("Invalid stopOnEntry: expected a boolean, got JSON {kind}"),
            ));
        }
        for field in ["timeout", "timeoutMs"] {
            for (value, kind) in [
                (json!("5000"), "string"),
                (json!(true), "boolean"),
                (json!(false), "boolean"),
                (json!(-1), "number"),
                (json!(0.5), "number"),
                (json!(5000.0), "number"),
                (json!([]), "array"),
                (json!({}), "object"),
            ] {
                let mut options = json!({});
                options[field] = value;
                cases.push((options, format!(
                    "Invalid {field}: expected a non-negative integer in milliseconds, got JSON {kind}"
                )));
            }
            for value in [0_u64, 300_001, 4_294_972_296, u64::MAX] {
                let mut options = json!({});
                options[field] = json!(value);
                cases.push((
                    options,
                    if value == 0 {
                        "Timeout must be greater than 0 milliseconds"
                    } else {
                        "Timeout cannot exceed 300000 milliseconds (5 minutes)"
                    }
                    .to_string(),
                ));
            }
        }
        cases.extend([
            (json!({"timeout":5000,"timeoutMs":"bad"}),
                "Invalid timeoutMs: expected a non-negative integer in milliseconds, got JSON string".into()),
            (json!({"timeout":null,"timeoutMs":[]}),
                "Invalid timeoutMs: expected a non-negative integer in milliseconds, got JSON array".into()),
            (json!({"timeout":"bad","timeoutMs":5000}),
                "Invalid timeout: expected a non-negative integer in milliseconds, got JSON string".into()),
            (json!({"timeout":0,"timeoutMs":5000}),
                "Timeout must be greater than 0 milliseconds".into()),
            (json!({"stopOnEntry":true}),
                "TCP attach does not support stopOnEntry=true. Set stopOnEntry=false and configure the debugger peer to pause if needed".into()),
        ]);
        cases
    }

    #[test]
    fn stdio_transport_attach_options_refuse_before_contact_and_recover() -> Result<()> {
        let mut dap = start()?;
        let rejected = listener()?;
        let mut seq = 2;
        for (options, error) in refused_cases() {
            dap.send_request(seq, "attach", Some(arguments(&rejected, options)?))?;
            response(&dap, seq, "attach", Some(&error))?;
            no_contact(&rejected)?;
            seq += 1;
            dap.send_request(seq, "threads", None)?;
            match next(&dap)? {
                DapMessage::Response {
                    request_seq,
                    command,
                    success: true,
                    body: Some(body),
                    ..
                } if request_seq == seq && command == "threads" => {
                    assert_eq!(body["threads"], json!([]), "refusal fabricated a session");
                }
                other => return Err(anyhow!("unexpected post-refusal frame: {other:?}")),
            }
            seq += 1;
        }
        let _peer = attach(&mut dap, seq, json!({"stopOnEntry":false,"timeout":5000}))?;
        finish(&mut dap, seq + 1)?;
        no_contact(&rejected)
    }

    #[test]
    fn stdio_transport_attach_options_preserve_defaults_and_alias_precedence() -> Result<()> {
        // Socket acceptance proves admission, not the configured duration. Existing
        // TcpAttachConfig tests own exact default/duration and 1..=300000 bounds.
        for options in [
            json!({}),
            json!({"stopOnEntry":null}),
            json!({"stopOnEntry":false}),
            json!({"timeout":null}),
            json!({"timeoutMs":null}),
            json!({"timeout":5000}),
            json!({"timeoutMs":5000}),
            json!({"timeout":300000}),
            json!({"timeoutMs":300000}),
            json!({"timeout":null,"timeoutMs":0}),
            json!({"timeout":5000,"timeoutMs":0}),
            json!({"timeout":5000,"timeoutMs":18446744073709551615_u64}),
        ] {
            let mut dap = start()?;
            let _peer = attach(&mut dap, 2, options.clone())
                .with_context(|| format!("valid options refused: {options}"))?;
            finish(&mut dap, 3)?;
        }
        Ok(())
    }

    #[test]
    fn stdio_transport_attach_options_preserve_active_session_after_refusal() -> Result<()> {
        let mut dap = start()?;
        let mut peer = attach(&mut dap, 2, json!({"timeout":5000}))?;
        peer.set_write_timeout(Some(Duration::from_secs(2)))?;
        let rejected = listener()?;
        let mut seq = 3;
        for (options, error) in refused_cases() {
            dap.send_request(seq, "attach", Some(arguments(&rejected, options)?))?;
            response(&dap, seq, "attach", Some(&error))?;
            no_contact(&rejected)?;
            let marker = format!("surviving-session-{seq}\n");
            let payload = serde_json::to_vec(&json!({
                "seq":seq,"type":"event","event":"output",
                "body":{"category":"console","output":marker}
            }))?;
            write!(peer, "Content-Length: {}\r\n\r\n", payload.len())?;
            peer.write_all(&payload)?;
            peer.flush()?;
            match next(&dap)? {
                DapMessage::Event { event, body: Some(body), .. } if event == "output" => {
                    assert_eq!(body["output"], marker, "old session did not retain peer identity");
                }
                other => return Err(anyhow!("unexpected surviving-session frame: {other:?}")),
            }
            seq += 1;
        }
        finish(&mut dap, seq)?;
        no_contact(&rejected)
    }
}
