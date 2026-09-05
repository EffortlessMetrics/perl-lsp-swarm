//! Discriminating proof that external-peer modes expose DAP only over child
//! stdio (#10566).
//!
//! These tests spawn the packaged `perl-dap` binary. They fail if `--socket` /
//! editor `--port` still bind an editor listener in peer modes, are silently
//! ignored in favor of stdio, fall back to native DAP when the peer fails, or
//! leak the debugger-peer token onto DAP stdout.

use anyhow::{Context, Result, anyhow};
use perl_dap::DapMessage;
use perl_dap::peer_protocol::message::{PeerEvent, PeerMessage, PeerRequest, command, event};
use perl_dap::peer_protocol::payloads::{HelloArgs, StoppedEventBody, WireSource};
use perl_dap::peer_protocol::{
    PROTOCOL_VERSION, PeerFrameDecoder, PeerReportedCapabilities, encode_message,
};
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const HISTORICAL_EDITOR_PORT: u16 = 13_603;
const CLI_TIMEOUT: Duration = Duration::from_secs(3);
const STDIO_TIMEOUT: Duration = Duration::from_secs(5);
const TOKEN_CANARY: &str = "dap-10566-peer-token-canary";

fn dap_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perl-dap"))
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn drain_pipe<R: Read + Send + 'static>(reader: Option<R>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut reader) = reader {
            let _ = reader.read_to_string(&mut buf);
        }
        buf
    })
}

fn run_cli(args: &[&str]) -> io::Result<(ExitStatus, String, String)> {
    let mut child = Command::new(dap_binary())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = drain_pipe(child.stdout.take());
    let stderr = drain_pipe(child.stderr.take());
    let status = match wait_for_exit(&mut child, CLI_TIMEOUT)? {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout.join();
            let _ = stderr.join();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "perl-dap {args:?} still running after {CLI_TIMEOUT:?}; editor --socket must fail before bind rather than accept"
                ),
            ));
        }
    };
    let stdout = stdout.join().unwrap_or_else(|_| "<stdout reader panicked>".to_owned());
    let stderr = stderr.join().unwrap_or_else(|_| "<stderr reader panicked>".to_owned());
    Ok((status, stdout, stderr))
}

fn assert_editor_socket_retired(
    status: &ExitStatus,
    stdout: &str,
    stderr: &str,
    expected_stdio_cmd: &str,
) -> Result<()> {
    if status.success() {
        return Err(anyhow!(
            "editor --socket/--port must exit nonzero before bind; got success. stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    let combined = format!("{stdout}\n{stderr}");
    if !combined.contains(expected_stdio_cmd) {
        return Err(anyhow!(
            "retired editor socket error must name `{expected_stdio_cmd}`; stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    if combined.contains("already in use") {
        return Err(anyhow!(
            "editor --socket must fail before bind; AddrInUse remediation means the listener still ran. stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    if combined.to_ascii_lowercase().contains("listening") {
        return Err(anyhow!(
            "retired editor socket path must not start a listener. stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    Ok(())
}

fn child_listening_tcp_ports(pid: u32) -> io::Result<Vec<u16>> {
    if !cfg!(target_os = "linux") {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "listener oracle requires Linux procfs",
        ));
    }
    let mut inodes = HashSet::new();
    let fd_dir = format!("/proc/{pid}/fd");
    for entry in fs::read_dir(&fd_dir)? {
        let entry = entry?;
        let Ok(target) = fs::read_link(entry.path()) else {
            continue;
        };
        let rendered = target.to_string_lossy();
        let Some(inode) = rendered.strip_prefix("socket:[") else {
            continue;
        };
        let Some(inode) = inode.strip_suffix(']') else {
            continue;
        };
        inodes.insert(inode.to_string());
    }

    let mut ports = Vec::new();
    for table in ["tcp", "tcp6"] {
        let path = format!("/proc/{pid}/net/{table}");
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for line in text.lines().skip(1) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 10 || cols[3] != "0A" || !inodes.contains(cols[9]) {
                continue;
            }
            let Some((_, port_hex)) = cols[1].rsplit_once(':') else {
                continue;
            };
            let port = u16::from_str_radix(port_hex, 16).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("bad port hex: {error}"))
            })?;
            ports.push(port);
        }
    }
    ports.sort_unstable();
    ports.dedup();
    Ok(ports)
}

fn assert_no_historical_editor_port(pid: u32) -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Ok(());
    }
    let ports = child_listening_tcp_ports(pid)
        .with_context(|| format!("failed to inspect listening sockets for pid {pid}"))?;
    if ports.contains(&HISTORICAL_EDITOR_PORT) {
        return Err(anyhow!(
            "perl-dap pid {pid} bound the historical editor port {HISTORICAL_EDITOR_PORT}; listeners={ports:?}"
        ));
    }
    Ok(())
}

fn assert_no_child_listeners(pid: u32) -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Ok(());
    }
    let ports = child_listening_tcp_ports(pid)
        .with_context(|| format!("failed to inspect listening sockets for pid {pid}"))?;
    if !ports.is_empty() {
        return Err(anyhow!("perl-dap pid {pid} bound TCP listener(s) on {ports:?}"));
    }
    Ok(())
}

struct StdioAdapter {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    rx: Receiver<std::result::Result<DapMessage, String>>,
    stderr: Option<thread::JoinHandle<String>>,
    pending: VecDeque<DapMessage>,
}

impl StdioAdapter {
    fn spawn(extra_args: &[&str]) -> Result<Self> {
        let mut command = Command::new(dap_binary());
        command
            .args(extra_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().context("failed to spawn perl-dap")?;
        let stdin = child.stdin.take().context("child stdin was not piped")?;
        let stdout = child.stdout.take().context("child stdout was not piped")?;
        let stderr = drain_pipe(child.stderr.take());
        Ok(Self {
            child,
            stdin: Some(stdin),
            rx: spawn_frame_reader(stdout),
            stderr: Some(stderr),
            pending: VecDeque::new(),
        })
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn send_request(&mut self, seq: i64, command: &str, arguments: Option<Value>) -> Result<()> {
        let payload = serde_json::to_vec(&json!({
            "type": "request",
            "seq": seq,
            "command": command,
            "arguments": arguments,
        }))?;
        let stdin = self.stdin.as_mut().ok_or_else(|| anyhow!("child stdin already closed"))?;
        write!(stdin, "Content-Length: {}\r\n\r\n", payload.len())?;
        stdin.write_all(&payload)?;
        stdin.flush()?;
        Ok(())
    }

    fn wait_for_response(&mut self, request_seq: i64, command: &str) -> Result<DapMessage> {
        self.wait_for_message(format!("response `{command}` #{request_seq}"), |msg| {
            matches!(
                msg,
                DapMessage::Response { request_seq: actual, command: actual_command, .. }
                    if *actual == request_seq && actual_command == command
            )
        })
    }

    fn wait_for_event(&mut self, name: &str) -> Result<DapMessage> {
        self.wait_for_message(
            format!("event `{name}`"),
            |msg| matches!(msg, DapMessage::Event { event, .. } if event == name),
        )
    }

    fn wait_for_message(
        &mut self,
        description: String,
        matches_message: impl Fn(&DapMessage) -> bool,
    ) -> Result<DapMessage> {
        if let Some(index) = self.pending.iter().position(&matches_message) {
            return self.pending.remove(index).ok_or_else(|| {
                anyhow!("pending DAP message vanished while waiting for {description}")
            });
        }
        let deadline = Instant::now() + STDIO_TIMEOUT;
        let mut observed = Vec::new();
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(anyhow!("timeout waiting for {description}; observed {observed:?}"));
            }
            match self.rx.recv_timeout(deadline.saturating_duration_since(now)) {
                Ok(Ok(message)) if matches_message(&message) => return Ok(message),
                Ok(Ok(message)) => {
                    observed.push(format!("{message:?}"));
                    self.pending.push_back(message);
                }
                Ok(Err(error)) => {
                    return Err(anyhow!(
                        "DAP reader failed while waiting for {description}: {error}"
                    ));
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(anyhow!(
                        "timeout waiting for {description}; observed {observed:?}"
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(anyhow!(
                        "DAP reader disconnected while waiting for {description}; observed {observed:?}"
                    ));
                }
            }
        }
    }

    fn close_stdin(mut self) -> Result<(Option<ExitStatus>, String)> {
        drop(self.stdin.take());
        let status = wait_for_exit(&mut self.child, STDIO_TIMEOUT)?;
        if status.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let stderr = self
            .stderr
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_else(|| "<stderr reader panicked>".to_owned());
        // Prevent Drop from killing a process we already reaped.
        let _ = self.child.try_wait();
        Ok((status, stderr))
    }
}

impl Drop for StdioAdapter {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn spawn_frame_reader<R>(mut reader: R) -> Receiver<std::result::Result<DapMessage, String>>
where
    R: Read + Send + 'static,
{
    let (tx, rx) = channel();
    thread::spawn(move || {
        loop {
            match read_framed_message(&mut reader) {
                Ok(message) => {
                    if tx.send(Ok(message)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = tx.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    rx
}

fn read_framed_message<R: Read>(reader: &mut R) -> Result<DapMessage> {
    let mut header = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        reader.read_exact(&mut byte).context("failed to read DAP frame header")?;
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
        .ok_or_else(|| anyhow!("DAP frame header missing Content-Length"))?
        .parse::<usize>()
        .context("DAP Content-Length was not a positive integer")?;
    let mut body = vec![0_u8; content_length];
    reader.read_exact(&mut body).context("failed to read DAP frame body")?;
    serde_json::from_slice(&body).context("DAP frame body was not a DapMessage")
}

/// Fake debugger peer that *listens* so `perl-dap --external-peer` can connect.
struct ListeningFakePeer {
    handle: Option<JoinHandle<()>>,
    addr: std::net::SocketAddr,
}

impl ListeningFakePeer {
    fn start(token: Option<String>, emit_stopped: bool, drop_after_hello: bool) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        let handle = thread::spawn(move || {
            run_listening_fake_peer(listener, token, emit_stopped, drop_after_hello);
        });
        Ok(Self { handle: Some(handle), addr })
    }

    fn addr_arg(&self) -> String {
        self.addr.to_string()
    }
}

impl Drop for ListeningFakePeer {
    fn drop(&mut self) {
        // Detach: joining `accept()` hangs if no client connected.
        drop(self.handle.take());
    }
}

fn run_listening_fake_peer(
    listener: TcpListener,
    token: Option<String>,
    emit_stopped: bool,
    drop_after_hello: bool,
) {
    let Ok((stream, _)) = listener.accept() else {
        return;
    };
    let Ok(mut write) = stream.try_clone() else {
        return;
    };
    let mut read = stream;
    let mut seq = 700i64;
    let send = |w: &mut TcpStream, m: &PeerMessage| {
        if let Ok(encoded) = encode_message(m) {
            let _ = w.write_all(&encoded);
            let _ = w.flush();
        }
    };

    seq += 1;
    send(
        &mut write,
        &PeerMessage::Request(PeerRequest {
            seq,
            command: command::HELLO.to_string(),
            arguments: serde_json::to_value(HelloArgs {
                peer: "FakePtkdb".to_string(),
                peer_version: Some("0.1".to_string()),
                protocol_version: PROTOCOL_VERSION.to_string(),
                token,
                capabilities: PeerReportedCapabilities {
                    can_continue: true,
                    can_step: true,
                    can_list_stack: true,
                    ..Default::default()
                },
            })
            .ok(),
        }),
    );

    if emit_stopped {
        seq += 1;
        send(
            &mut write,
            &PeerMessage::Event(PeerEvent {
                seq,
                event: event::STOPPED.to_string(),
                body: serde_json::to_value(StoppedEventBody {
                    reason: "breakpoint".to_string(),
                    thread_id: 1,
                    source: Some(WireSource {
                        path: "/work/script.pl".to_string(),
                        name: None,
                        source_reference: None,
                    }),
                    line: Some(42),
                    column: Some(1),
                })
                .ok(),
            }),
        );
    }

    if drop_after_hello {
        return;
    }

    let mut decoder = PeerFrameDecoder::new();
    let mut buf = [0u8; 4096];
    read.set_read_timeout(Some(Duration::from_millis(300))).ok();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match read.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                decoder.push(&buf[..n]);
                while let Ok(Some(msg)) = decoder.try_next() {
                    let PeerMessage::Request(req) = msg else {
                        continue;
                    };
                    if req.command == command::HELLO || req.command == command::DISCONNECT {
                        continue;
                    }
                    seq += 1;
                    let response =
                        PeerMessage::Response(perl_dap::peer_protocol::message::PeerResponse {
                            seq,
                            request_seq: req.seq,
                            command: req.command,
                            success: true,
                            body: None,
                            message: None,
                            cause: None,
                        });
                    send(&mut write, &response);
                }
            }
            Err(ref error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
    }
}

fn initialize_args() -> Value {
    json!({
        "adapterID": "perl-dap",
        "clientID": "10566-stdio-test",
        "peerToken": "stolen-from-editor",
        "token": "editor-supplied-token"
    })
}

fn dap_text(message: &DapMessage) -> String {
    serde_json::to_string(message).unwrap_or_else(|_| format!("{message:?}"))
}

fn assert_no_token_canary(surface: &str, text: &str) -> Result<()> {
    if text.contains(TOKEN_CANARY) {
        return Err(anyhow!("{surface} leaked the debugger-peer token canary"));
    }
    Ok(())
}

#[test]
fn peer_connect_serves_dap_only_on_stdio() -> Result<()> {
    let peer = ListeningFakePeer::start(Some(TOKEN_CANARY.to_owned()), true, false)?;
    let addr = peer.addr_arg();
    let mut adapter = StdioAdapter::spawn(&["--external-peer", &addr, "--log-level", "error"])?;
    thread::sleep(Duration::from_millis(150));
    assert_no_child_listeners(adapter.pid())?;
    assert_no_historical_editor_port(adapter.pid())?;

    adapter.send_request(1, "initialize", Some(initialize_args()))?;
    let initialize = adapter.wait_for_response(1, "initialize")?;
    match &initialize {
        DapMessage::Response { success: true, .. } => {}
        other => {
            return Err(anyhow!("peer-connect initialize must succeed over stdio, got {other:?}"));
        }
    }
    assert_no_token_canary("initialize response", &dap_text(&initialize))?;

    let stopped = adapter.wait_for_event("stopped")?;
    match &stopped {
        DapMessage::Event { body: Some(body), .. } => {
            if body["reason"] != "breakpoint" {
                return Err(anyhow!("stopped event must cross from the fake peer, got {body}"));
            }
        }
        other => return Err(anyhow!("expected DAP stopped event, got {other:?}")),
    }
    assert_no_token_canary("stopped event", &dap_text(&stopped))?;

    assert_no_child_listeners(adapter.pid())?;
    adapter.send_request(2, "disconnect", Some(json!({})))?;
    let (_, stderr) = adapter.close_stdin()?;
    assert_no_token_canary("DAP stderr", &stderr)?;
    Ok(())
}

#[test]
fn peer_listen_serves_dap_only_on_stdio_and_keeps_peer_listener() -> Result<()> {
    let mut adapter =
        StdioAdapter::spawn(&["--external-peer-listen", "127.0.0.1", "--log-level", "error"])?;
    thread::sleep(Duration::from_millis(200));
    assert_no_historical_editor_port(adapter.pid())?;
    if cfg!(target_os = "linux") {
        let ports = child_listening_tcp_ports(adapter.pid())
            .context("listen mode must expose the debugger-peer listener")?;
        if ports.is_empty() {
            return Err(anyhow!(
                "external-peer listen must bind the authenticated debugger-peer listener"
            ));
        }
        if ports.contains(&HISTORICAL_EDITOR_PORT) {
            return Err(anyhow!("listen mode bound the historical editor port: {ports:?}"));
        }
    }

    adapter.send_request(1, "initialize", Some(initialize_args()))?;
    match adapter.wait_for_response(1, "initialize")? {
        DapMessage::Response { success: true, body: Some(body), .. } => {
            let rendered = body.to_string();
            assert_no_token_canary("initialize body", &rendered)?;
            if rendered.contains("PERL_DAP_PEER_TOKEN") {
                return Err(anyhow!("initialize body leaked a peer token: {body}"));
            }
        }
        other => {
            return Err(anyhow!(
                "peer-listen initialize must succeed over stdio before any peer connects, got {other:?}"
            ));
        }
    }
    adapter.send_request(2, "disconnect", Some(json!({})))?;
    let (_, stderr) = adapter.close_stdin()?;
    assert_no_token_canary("DAP stderr", &stderr)?;
    Ok(())
}

#[test]
fn peer_connect_socket_flag_fails_before_bind_with_stdio_migration() -> Result<()> {
    let occupied = TcpListener::bind(("127.0.0.1", 0))?;
    let port = occupied.local_addr()?.port();
    let (status, stdout, stderr) = run_cli(&[
        "--external-peer",
        "127.0.0.1:9",
        "--socket",
        "--port",
        &port.to_string(),
        "--log-level",
        "error",
    ])?;
    assert_editor_socket_retired(
        &status,
        &stdout,
        &stderr,
        "perl-dap --stdio --external-peer 127.0.0.1:9",
    )?;
    Ok(())
}

#[test]
fn peer_socket_migration_quotes_metacharacter_peer_specs() -> Result<()> {
    let occupied = TcpListener::bind(("127.0.0.1", 0))?;
    let port = occupied.local_addr()?.port();
    let (status, stdout, stderr) = run_cli(&[
        "--external-peer",
        "host; touch /tmp/x",
        "--socket",
        "--port",
        &port.to_string(),
        "--log-level",
        "error",
    ])?;
    let expected = if cfg!(windows) {
        "perl-dap --stdio --external-peer \"host; touch /tmp/x\""
    } else {
        "perl-dap --stdio --external-peer 'host; touch /tmp/x'"
    };
    assert_editor_socket_retired(&status, &stdout, &stderr, expected)?;
    Ok(())
}

#[test]
fn peer_listen_port_flag_fails_before_bind_with_stdio_migration() -> Result<()> {
    let occupied = TcpListener::bind(("127.0.0.1", 0))?;
    let port = occupied.local_addr()?.port();
    let (status, stdout, stderr) = run_cli(&[
        "--external-peer-listen",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "--log-level",
        "error",
    ])?;
    assert_editor_socket_retired(
        &status,
        &stdout,
        &stderr,
        "perl-dap --stdio --external-peer-listen 127.0.0.1",
    )?;
    Ok(())
}

#[test]
fn peer_connect_failure_does_not_fall_back_to_native_or_editor_socket() -> Result<()> {
    let probe = TcpListener::bind(("127.0.0.1", 0))?;
    let port = probe.local_addr()?.port();
    drop(probe);
    let peer = format!("127.0.0.1:{port}");
    let (status, stdout, stderr) = run_cli(&["--external-peer", &peer, "--log-level", "error"])?;
    if status.success() {
        return Err(anyhow!(
            "unavailable peer must be a typed failure, not success. stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    let combined = format!("{stdout}\n{stderr}");
    if !combined.contains("debugger peer") && !combined.contains("failed to connect") {
        return Err(anyhow!(
            "peer failure must name the debugger-peer backend; stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    if combined.contains("Starting DAP server on stdio") {
        return Err(anyhow!(
            "peer failure must not fall back to native DAP; stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    if stdout.contains("\"command\":\"initialize\"") {
        return Err(anyhow!("peer failure must not answer DAP initialize; stdout={stdout:?}"));
    }
    if combined.contains("already in use") {
        return Err(anyhow!(
            "peer failure must not bind an editor listener; stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    Ok(())
}

#[test]
fn stdio_eof_settles_a_peer_connect_session() -> Result<()> {
    let peer = ListeningFakePeer::start(None, false, false)?;
    let addr = peer.addr_arg();
    let adapter = StdioAdapter::spawn(&["--external-peer", &addr, "--log-level", "error"])?;
    thread::sleep(Duration::from_millis(150));
    let (status, stderr) = adapter.close_stdin()?;
    assert_no_token_canary("DAP stderr", &stderr)?;
    match status {
        Some(_) => {}
        None => {
            return Err(anyhow!(
                "stdio EOF must settle the peer session boundedly; stderr={stderr:?}"
            ));
        }
    }
    Ok(())
}

#[test]
fn peer_crash_settles_dap_without_an_editor_listener() -> Result<()> {
    let peer = ListeningFakePeer::start(None, false, true)?;
    let addr = peer.addr_arg();
    let adapter = StdioAdapter::spawn(&["--external-peer", &addr, "--log-level", "error"])?;
    thread::sleep(Duration::from_millis(200));
    assert_no_child_listeners(adapter.pid())?;
    let (status, stderr) = adapter.close_stdin()?;
    assert_no_token_canary("DAP stderr", &stderr)?;
    if status.is_none() {
        return Err(anyhow!(
            "peer close/crash must settle DAP boundedly without an editor listener; stderr={stderr:?}"
        ));
    }
    Ok(())
}

#[test]
fn production_source_rejects_a_returned_editor_listener() -> Result<()> {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let main = fs::read_to_string(crate_root.join("src/main.rs"))?;
    let production = production_source_without_cfg_test_mods(&main);
    if production.contains("fn bind_editor_listener")
        || production.contains("bind_editor_listener(")
    {
        return Err(anyhow!(
            "main.rs production source regained bind_editor_listener after #10566"
        ));
    }
    if production.contains("TcpListener::bind") {
        return Err(anyhow!("main.rs production source regained TcpListener::bind after #10566"));
    }
    if production.contains("run_external_peer_bridge(")
        && !production.contains("run_external_peer_bridge_stdio")
    {
        return Err(anyhow!(
            "main.rs production source regained the socket editor peer-bridge wrapper"
        ));
    }
    if !production.contains("native_editor_socket_retired") {
        return Err(anyhow!(
            "main.rs production source lost native_editor_socket_retired admission"
        ));
    }
    Ok(())
}

fn production_source_without_cfg_test_mods(text: &str) -> String {
    let marker = "#[cfg(test)]";
    let Some(start) = text.find(marker) else {
        return text.to_owned();
    };
    text[..start].to_owned()
}
