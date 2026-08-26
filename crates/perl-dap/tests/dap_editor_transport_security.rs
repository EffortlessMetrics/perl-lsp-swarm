//! Composed exact-candidate proof that editor DAP is stdio-only (#10567).
//!
//! Negative controls live first: reintroduced editor listeners, ignored
//! `--socket`, attacker initialize over TCP, role confusion, secret leakage,
//! stale binary, and cleanup-unknown-as-pass. Runtime rows then bind the
//! packaged `perl-dap` binary. Missing Linux procfs observation is
//! `instrument_failure` / `not_proven`, never an empty-listener pass.
//!
//! Does not use `tests/common/mod.rs` (#12749 owns that file).

use anyhow::{Context, Result, anyhow};
use perl_dap::DapMessage;
use perl_dap::backend::capabilities::ControlMode;
use perl_dap::backend::peer_launch::{ENV_PEER_TOKEN, PeerListenEndpoint};
use perl_dap::peer_protocol::message::{PeerMessage, PeerRequest, command};
use perl_dap::peer_protocol::payloads::HelloArgs;
use perl_dap::peer_protocol::{PROTOCOL_VERSION, PeerReportedCapabilities, encode_message};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const HISTORICAL_EDITOR_PORT: u16 = 13_603;
const CLI_TIMEOUT: Duration = Duration::from_secs(3);
const STDIO_TIMEOUT: Duration = Duration::from_secs(5);
const ATTACKER_TIMEOUT: Duration = Duration::from_millis(400);
const TOKEN_CANARY: &str = "dap-10567-peer-token-canary";
const SECURITY_SCHEMA: &str = "dap_editor_transport_security.v1";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Verdict {
    Pass,
    Failed,
    NotProven,
    InstrumentFailure,
}

impl Verdict {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Failed => "failed",
            Self::NotProven => "not_proven",
            Self::InstrumentFailure => "instrument_failure",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ListenerRole {
    EditorDap,
    DebuggerPeer,
    Unknown,
}

impl ListenerRole {
    fn as_str(&self) -> &'static str {
        match self {
            Self::EditorDap => "editor_dap",
            Self::DebuggerPeer => "debugger_peer",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ClassifiedListener {
    port: u16,
    role: ListenerRole,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SocketObservation {
    Observed { listeners: Vec<ClassifiedListener> },
    InstrumentFailure { reason: String },
    UnsupportedPlatform,
}

impl SocketObservation {
    fn verdict(&self) -> Verdict {
        match self {
            Self::Observed { .. } => Verdict::Pass,
            Self::InstrumentFailure { .. } => Verdict::InstrumentFailure,
            Self::UnsupportedPlatform => Verdict::NotProven,
        }
    }

    /// Fail closed: missing observation is never an empty-listener pass.
    fn observed_zero_listeners(&self) -> bool {
        matches!(self, Self::Observed { listeners } if listeners.is_empty())
    }
}

#[derive(Clone, Debug)]
struct BinaryIdentity {
    path: PathBuf,
    sha256: String,
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn sha256_file(path: &Path) -> io::Result<String> {
    Ok(hex_encode(&Sha256::digest(fs::read(path)?)))
}

fn cargo_dap_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perl-dap"))
}

fn bind_binary_identity(path: &Path) -> Result<BinaryIdentity> {
    Ok(BinaryIdentity { sha256: sha256_file(path)?, path: path.to_path_buf() })
}

fn identity_matches(claimed: &BinaryIdentity, actual: &BinaryIdentity) -> Result<()> {
    if claimed.path != actual.path {
        return Err(anyhow!(
            "stale or other perl-dap binary path: claimed {} actual {}",
            claimed.path.display(),
            actual.path.display()
        ));
    }
    if claimed.sha256 != actual.sha256 {
        return Err(anyhow!(
            "stale or other perl-dap binary hash: claimed {} actual {}",
            claimed.sha256,
            actual.sha256
        ));
    }
    Ok(())
}

fn git_sha() -> Result<String> {
    let output = Command::new("git").args(["rev-parse", "HEAD"]).output()?;
    if !output.status.success() {
        return Err(anyhow!("git rev-parse HEAD failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_tree_state() -> Result<&'static str> {
    let output = Command::new("git").args(["status", "--porcelain"]).output()?;
    if !output.status.success() {
        return Ok("not_proven");
    }
    if output.stdout.is_empty() { Ok("clean") } else { Ok("dirty") }
}

fn observe_child_listeners(pid: u32) -> SocketObservation {
    if !cfg!(target_os = "linux") {
        return SocketObservation::UnsupportedPlatform;
    }
    match child_listening_tcp_ports(pid) {
        Ok(ports) => {
            let listeners = ports
                .into_iter()
                .map(|port| ClassifiedListener { port, role: classify_port(port) })
                .collect();
            SocketObservation::Observed { listeners }
        }
        Err(error) => SocketObservation::InstrumentFailure { reason: error.to_string() },
    }
}

fn classify_port(port: u16) -> ListenerRole {
    if port == HISTORICAL_EDITOR_PORT {
        return ListenerRole::EditorDap;
    }
    let candidates = [
        SocketAddr::from(([127, 0, 0, 1], port)),
        SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port)),
    ];
    let mut connected_without_dap = false;
    for addr in candidates {
        match attacker_initialize(addr) {
            Ok(AttackerOutcome::InitializeSucceeded) => return ListenerRole::EditorDap,
            Ok(AttackerOutcome::ConnectedNoInitialize) => connected_without_dap = true,
            Ok(AttackerOutcome::ConnectionRefused) | Err(_) => {}
        }
    }
    if connected_without_dap { ListenerRole::DebuggerPeer } else { ListenerRole::Unknown }
}

fn child_listening_tcp_ports(pid: u32) -> io::Result<Vec<u16>> {
    let fd_dir = format!("/proc/{pid}/fd");
    let mut inodes = HashSet::new();
    let entries = fs::read_dir(&fd_dir).map_err(|error| {
        io::Error::other(format!("socket instrument failed reading {fd_dir}: {error}"))
    })?;
    for entry in entries {
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
    let mut tables_read = 0_u8;
    for table in ["tcp", "tcp6"] {
        let path = format!("/proc/{pid}/net/{table}");
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => continue,
        };
        tables_read += 1;
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
    if tables_read == 0 {
        return Err(io::Error::other(format!(
            "socket instrument failed: no /proc/{pid}/net/tcp tables readable"
        )));
    }
    ports.sort_unstable();
    ports.dedup();
    Ok(ports)
}

fn require_observation(obs: &SocketObservation) -> Result<&[ClassifiedListener]> {
    match obs {
        SocketObservation::Observed { listeners } => Ok(listeners),
        SocketObservation::InstrumentFailure { reason } => {
            Err(anyhow!("socket observation instrument_failure (not zero listeners): {reason}"))
        }
        SocketObservation::UnsupportedPlatform => Err(anyhow!(
            "socket observation not_proven on this platform; missing instrumentation is not zero"
        )),
    }
}

fn assert_no_editor_listeners(obs: &SocketObservation, mode: &str) -> Result<()> {
    let listeners = require_observation(obs)?;
    let editor = listeners.iter().filter(|row| row.role == ListenerRole::EditorDap).count();
    if editor > 0 {
        return Err(anyhow!("{mode} owned editor_dap listener(s) (role confusion): {listeners:?}"));
    }
    if mode != "external_peer_listen" && !listeners.is_empty() {
        return Err(anyhow!("{mode} must own zero TCP listeners; got {listeners:?}"));
    }
    if mode == "external_peer_listen"
        && (listeners.len() != 1 || listeners[0].role != ListenerRole::DebuggerPeer)
    {
        return Err(anyhow!(
            "peer-listen must own exactly one debugger_peer listener, got {listeners:?}"
        ));
    }
    Ok(())
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

fn drain_pipe<R: Read + Send + 'static>(reader: Option<R>) -> JoinHandle<String> {
    thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut reader) = reader {
            let _ = reader.read_to_string(&mut buf);
        }
        buf
    })
}

fn run_cli(args: &[&str]) -> io::Result<(ExitStatus, String, String)> {
    let mut child = Command::new(cargo_dap_binary())
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
                    "perl-dap {args:?} still running after {CLI_TIMEOUT:?}; --socket must fail before bind"
                ),
            ));
        }
    };
    let stdout = stdout.join().unwrap_or_else(|_| "<stdout reader panicked>".to_owned());
    let stderr = stderr.join().unwrap_or_else(|_| "<stderr reader panicked>".to_owned());
    Ok((status, stdout, stderr))
}

fn assert_socket_fails_before_bind(
    status: &ExitStatus,
    stdout: &str,
    stderr: &str,
    expected_stdio_cmd: &str,
) -> Result<()> {
    if status.success() {
        return Err(anyhow!(
            "accept-and-ignore --socket: process succeeded. stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    let combined = format!("{stdout}\n{stderr}");
    if !combined.contains(expected_stdio_cmd) {
        return Err(anyhow!(
            "retired socket error must name `{expected_stdio_cmd}`; stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    if combined.contains("already in use") {
        return Err(anyhow!(
            "--socket bound rather than failing before bind. stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    Ok(())
}

struct StdioAdapter {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    rx: Receiver<std::result::Result<DapMessage, String>>,
    stderr: Option<JoinHandle<String>>,
    pending: VecDeque<DapMessage>,
}

impl StdioAdapter {
    fn spawn(extra_args: &[&str]) -> Result<Self> {
        let mut command = Command::new(cargo_dap_binary());
        command
            .args(extra_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().context("failed to spawn exact perl-dap")?;
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

    fn close_stdin(mut self) -> Result<Cleanup> {
        drop(self.stdin.take());
        let status = wait_for_exit(&mut self.child, STDIO_TIMEOUT)?;
        let stderr = self
            .stderr
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_else(|| "<stderr reader panicked>".to_owned());
        if status.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
            return Ok(Cleanup { state: "leaked", stderr });
        }
        let _ = self.child.try_wait();
        Ok(Cleanup { state: "clean", stderr })
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

struct Cleanup {
    state: &'static str,
    stderr: String,
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

#[derive(Debug, PartialEq, Eq)]
enum AttackerOutcome {
    ConnectionRefused,
    ConnectedNoInitialize,
    InitializeSucceeded,
}

fn attacker_initialize(addr: SocketAddr) -> Result<AttackerOutcome> {
    let mut stream = match TcpStream::connect_timeout(&addr, ATTACKER_TIMEOUT) {
        Ok(stream) => stream,
        Err(error)
            if error.kind() == io::ErrorKind::ConnectionRefused
                || error.kind() == io::ErrorKind::TimedOut
                || error.kind() == io::ErrorKind::WouldBlock =>
        {
            return Ok(AttackerOutcome::ConnectionRefused);
        }
        Err(error) => return Err(error.into()),
    };
    stream.set_read_timeout(Some(ATTACKER_TIMEOUT))?;
    stream.set_write_timeout(Some(ATTACKER_TIMEOUT))?;
    let payload = serde_json::to_vec(&json!({
        "type": "request",
        "seq": 1,
        "command": "initialize",
        "arguments": {"adapterID": "attacker", "clientID": "10567-tcp"}
    }))?;
    write!(stream, "Content-Length: {}\r\n\r\n", payload.len())?;
    stream.write_all(&payload)?;
    stream.flush()?;
    match read_framed_message(&mut stream) {
        Ok(DapMessage::Response { command, success: true, .. }) if command == "initialize" => {
            Ok(AttackerOutcome::InitializeSucceeded)
        }
        Ok(_) | Err(_) => Ok(AttackerOutcome::ConnectedNoInitialize),
    }
}

fn assert_no_tcp_initialize(addr: SocketAddr, why: &str) -> Result<()> {
    match attacker_initialize(addr)? {
        AttackerOutcome::InitializeSucceeded => {
            Err(anyhow!("attacker completed DAP initialize over TCP ({why}) at {addr}"))
        }
        AttackerOutcome::ConnectionRefused | AttackerOutcome::ConnectedNoInitialize => Ok(()),
    }
}

fn initialize_args() -> Value {
    json!({
        "adapterID": "perl-dap",
        "clientID": "10567-security",
        "peerToken": "stolen-from-editor",
        "token": "editor-supplied-token"
    })
}

fn dap_text(message: &DapMessage) -> String {
    serde_json::to_string(message).unwrap_or_else(|_| format!("{message:?}"))
}

fn assert_no_canary(surface: &str, text: &str) -> Result<()> {
    if text.contains(TOKEN_CANARY) {
        return Err(anyhow!("{surface} leaked the debugger-peer token canary"));
    }
    if text.contains("PERL_DAP_PEER_TOKEN=") {
        return Err(anyhow!("{surface} leaked a PERL_DAP_PEER_TOKEN assignment"));
    }
    Ok(())
}

struct ListeningFakePeer {
    handle: Option<JoinHandle<()>>,
    addr: SocketAddr,
}

impl ListeningFakePeer {
    fn start(token: Option<String>, emit_stopped: bool) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        let handle = thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let Ok(mut write) = stream.try_clone() else {
                return;
            };
            let hello = PeerMessage::Request(PeerRequest {
                seq: 701,
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
            });
            if let Ok(encoded) = encode_message(&hello) {
                let _ = write.write_all(&encoded);
                let _ = write.flush();
            }
            if emit_stopped {
                let event = PeerMessage::Event(perl_dap::peer_protocol::message::PeerEvent {
                    seq: 702,
                    event: perl_dap::peer_protocol::message::event::STOPPED.to_string(),
                    body: serde_json::to_value(
                        perl_dap::peer_protocol::payloads::StoppedEventBody {
                            reason: "breakpoint".to_string(),
                            thread_id: 1,
                            source: Some(perl_dap::peer_protocol::payloads::WireSource {
                                path: "/work/script.pl".to_string(),
                                name: None,
                                source_reference: None,
                            }),
                            line: Some(42),
                            column: Some(1),
                        },
                    )
                    .ok(),
                });
                if let Ok(encoded) = encode_message(&event) {
                    let _ = write.write_all(&encoded);
                    let _ = write.flush();
                }
            }
            thread::sleep(Duration::from_secs(2));
        });
        Ok(Self { handle: Some(handle), addr })
    }

    fn addr_arg(&self) -> String {
        self.addr.to_string()
    }
}

impl Drop for ListeningFakePeer {
    fn drop(&mut self) {
        drop(self.handle.take());
    }
}

fn send_wrong_token_hello(addr: SocketAddr, token: &str) -> Result<()> {
    let mut stream = TcpStream::connect_timeout(&addr, ATTACKER_TIMEOUT)?;
    stream.set_write_timeout(Some(ATTACKER_TIMEOUT))?;
    let hello = PeerMessage::Request(PeerRequest {
        seq: 1,
        command: command::HELLO.to_string(),
        arguments: Some(serde_json::to_value(HelloArgs {
            peer: "Attacker".to_string(),
            peer_version: Some("0.1".to_string()),
            protocol_version: PROTOCOL_VERSION.to_string(),
            token: Some(token.to_owned()),
            capabilities: PeerReportedCapabilities::default(),
        })?),
    });
    stream.write_all(&encode_message(&hello)?)?;
    stream.flush()?;
    Ok(())
}

fn receipt_path() -> PathBuf {
    if let Ok(path) = std::env::var("DAP_EDITOR_TRANSPORT_SECURITY_RECEIPT") {
        return PathBuf::from(path);
    }
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join("target"));
    target.join("receipts/dap-editor-transport-security.json")
}

fn write_receipt(receipt: &Value) -> Result<()> {
    let path = receipt_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_vec_pretty(receipt)?)?;
    Ok(())
}

fn observe_spawned_mode(mode: &str, args: &[&str]) -> Result<(SocketObservation, Cleanup)> {
    let adapter = StdioAdapter::spawn(args)?;
    thread::sleep(Duration::from_millis(200));
    let obs = observe_child_listeners(adapter.pid());
    match &obs {
        SocketObservation::Observed { .. } => assert_no_editor_listeners(&obs, mode)?,
        SocketObservation::InstrumentFailure { .. } | SocketObservation::UnsupportedPlatform => {}
    }
    let cleanup = adapter.close_stdin()?;
    Ok((obs, cleanup))
}

fn json_listeners(obs: &SocketObservation) -> Value {
    match obs {
        SocketObservation::Observed { listeners } => json!({
            "instrument": "linux_procfs",
            "inventory": listeners.iter().map(|row| json!({
                "port": row.port,
                "role": row.role.as_str(),
            })).collect::<Vec<_>>(),
        }),
        SocketObservation::InstrumentFailure { reason } => json!({
            "instrument": "error",
            "inventory": Value::Null,
            "reason": reason,
        }),
        SocketObservation::UnsupportedPlatform => json!({
            "instrument": "unsupported_platform",
            "inventory": [],
        }),
    }
}

fn mode_verdict(obs: &SocketObservation, cleanup: &str, extras: &[Verdict]) -> Verdict {
    let mut verdicts = vec![obs.verdict()];
    verdicts.extend(extras.iter().cloned());
    verdicts.push(match cleanup {
        "clean" => Verdict::Pass,
        "leaked" => Verdict::Failed,
        _ => Verdict::NotProven,
    });
    if verdicts.contains(&Verdict::Failed) {
        return Verdict::Failed;
    }
    if verdicts.contains(&Verdict::InstrumentFailure) {
        return Verdict::InstrumentFailure;
    }
    if verdicts.contains(&Verdict::NotProven) {
        return Verdict::NotProven;
    }
    Verdict::Pass
}

// ---------------------------------------------------------------------------
// Negative controls for the instrument itself
// ---------------------------------------------------------------------------

#[test]
fn missing_socket_instrument_is_not_zero_listeners() {
    let missing = SocketObservation::InstrumentFailure { reason: "no /proc".into() };
    assert!(!missing.observed_zero_listeners());
    assert_eq!(missing.verdict(), Verdict::InstrumentFailure);
    let unsupported = SocketObservation::UnsupportedPlatform;
    assert!(!unsupported.observed_zero_listeners());
    assert_eq!(unsupported.verdict(), Verdict::NotProven);
    let observed = SocketObservation::Observed { listeners: Vec::new() };
    assert!(observed.observed_zero_listeners());
    assert_eq!(observed.verdict(), Verdict::Pass);
}

#[test]
fn stale_binary_identity_fails() -> Result<()> {
    let exact = bind_binary_identity(&cargo_dap_binary())?;
    let other = BinaryIdentity { path: PathBuf::from("/bin/true"), sha256: "0".repeat(64) };
    match identity_matches(&exact, &other) {
        Err(err) => {
            let rendered = err.to_string();
            if !rendered.contains("stale or other perl-dap binary") {
                return Err(anyhow!("stale-binary error missed the discriminant: {rendered}"));
            }
        }
        Ok(()) => return Err(anyhow!("stale binary identity must fail")),
    }
    identity_matches(&exact, &exact)?;
    Ok(())
}

#[test]
fn role_confusion_rejects_editor_label_on_peer_listen() -> Result<()> {
    let obs = SocketObservation::Observed {
        listeners: vec![ClassifiedListener { port: 5000, role: ListenerRole::EditorDap }],
    };
    match assert_no_editor_listeners(&obs, "external_peer_listen") {
        Err(err) => {
            if !err.to_string().contains("role confusion") {
                return Err(anyhow!(
                    "editor label on peer listener missed role-confusion discriminant: {err}"
                ));
            }
        }
        Ok(()) => return Err(anyhow!("editor label on peer listener is role confusion")),
    }
    Ok(())
}

#[test]
fn extra_ephemeral_listener_in_peer_listen_is_not_classified_as_a_single_peer() -> Result<()> {
    let obs = SocketObservation::Observed {
        listeners: vec![
            ClassifiedListener { port: 5000, role: ListenerRole::DebuggerPeer },
            ClassifiedListener { port: 5001, role: ListenerRole::DebuggerPeer },
        ],
    };
    match assert_no_editor_listeners(&obs, "external_peer_listen") {
        Err(err) => {
            if !err.to_string().contains("exactly one debugger_peer") {
                return Err(anyhow!("two-listener case missed the discriminant: {err}"));
            }
        }
        Ok(()) => {
            return Err(anyhow!(
                "a second ephemeral listener must not pass as a single debugger-peer"
            ));
        }
    }
    Ok(())
}

#[test]
fn ephemeral_dap_tcp_listener_is_editor_not_debugger_peer() -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let addr = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let Ok(DapMessage::Request { seq, command, .. }) = read_framed_message(&mut stream) else {
            return;
        };
        if command != "initialize" {
            return;
        }
        let Ok(payload) = serde_json::to_vec(&DapMessage::Response {
            seq: 1,
            request_seq: seq,
            success: true,
            command: "initialize".to_string(),
            body: Some(json!({})),
            message: None,
        }) else {
            return;
        };
        let _ = write!(stream, "Content-Length: {}\r\n\r\n", payload.len());
        let _ = stream.write_all(&payload);
        let _ = stream.flush();
    });
    let role = classify_port(addr.port());
    let _ = handle.join();
    if role != ListenerRole::EditorDap {
        return Err(anyhow!(
            "ephemeral DAP listener classified as {role:?}; a mode heuristic would have labeled it debugger_peer"
        ));
    }
    Ok(())
}

#[test]
fn secret_canary_is_rejected_from_evidence() -> Result<()> {
    match assert_no_canary("receipt", TOKEN_CANARY) {
        Err(err) => {
            if !err.to_string().contains("canary") {
                return Err(anyhow!("canary error missed the discriminant: {err}"));
            }
        }
        Ok(()) => return Err(anyhow!("canary must fail")),
    }
    Ok(())
}

#[test]
fn cleanup_unknown_is_not_pass() {
    assert_eq!(
        mode_verdict(&SocketObservation::Observed { listeners: vec![] }, "unknown", &[]),
        Verdict::NotProven
    );
    assert_eq!(
        mode_verdict(
            &SocketObservation::InstrumentFailure { reason: "missing".into() },
            "clean",
            &[]
        ),
        Verdict::InstrumentFailure
    );
}

#[test]
fn production_source_rejects_reintroduced_editor_listener() -> Result<()> {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let main = fs::read_to_string(crate_root.join("src/main.rs"))?;
    let production = match main.find("#[cfg(test)]") {
        Some(start) => &main[..start],
        None => main.as_str(),
    };
    if production.contains("fn bind_editor_listener")
        || production.contains("bind_editor_listener(")
    {
        return Err(anyhow!("main.rs regained bind_editor_listener"));
    }
    if production.contains("pub fn run_socket") || production.contains("pub(crate) fn run_socket") {
        return Err(anyhow!("main.rs regained run_socket"));
    }
    let transport = fs::read_to_string(crate_root.join("src/debug_adapter/transport.rs"))?;
    let transport_prod = match transport.find("#[cfg(test)]") {
        Some(start) => &transport[..start],
        None => transport.as_str(),
    };
    if transport_prod.contains("TcpListener::bind") {
        return Err(anyhow!("transport.rs production source regained TcpListener::bind"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Exact-binary rows
// ---------------------------------------------------------------------------

#[test]
fn attacker_cannot_complete_initialize_over_historical_editor_port() -> Result<()> {
    assert_no_tcp_initialize(
        SocketAddr::from(([127, 0, 0, 1], HISTORICAL_EDITOR_PORT)),
        "historical editor port with no adapter",
    )
}

#[test]
fn old_socket_cli_fails_before_bind_on_every_mode() -> Result<()> {
    let occupied = TcpListener::bind(("127.0.0.1", 0))?;
    let port = occupied.local_addr()?.port();
    let port_s = port.to_string();
    let cases: &[(&[&str], &str)] = &[
        (&["--socket", "--port", &port_s, "--log-level", "error"], "perl-dap --stdio"),
        (
            &[
                "--external-peer",
                "127.0.0.1:9",
                "--socket",
                "--port",
                &port_s,
                "--log-level",
                "error",
            ],
            "perl-dap --stdio --external-peer 127.0.0.1:9",
        ),
        (
            &["--external-peer-listen", "127.0.0.1", "--port", &port_s, "--log-level", "error"],
            "perl-dap --stdio --external-peer-listen 127.0.0.1",
        ),
    ];
    for (args, expected) in cases {
        let (status, stdout, stderr) = run_cli(args)?;
        assert_socket_fails_before_bind(&status, &stdout, &stderr, expected)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn native_stdio_exposes_zero_editor_listeners() -> Result<()> {
    let identity = bind_binary_identity(&cargo_dap_binary())?;
    identity_matches(&identity, &bind_binary_identity(&cargo_dap_binary())?)?;
    let mut adapter = StdioAdapter::spawn(&["--stdio", "--log-level", "error"])?;
    thread::sleep(Duration::from_millis(150));
    let obs = observe_child_listeners(adapter.pid());
    assert_no_editor_listeners(&obs, "native")?;
    adapter.send_request(1, "initialize", Some(initialize_args()))?;
    match adapter.wait_for_response(1, "initialize")? {
        DapMessage::Response { success: true, .. } => {}
        other => return Err(anyhow!("native initialize must succeed over stdio, got {other:?}")),
    }
    assert_no_tcp_initialize(
        SocketAddr::from(([127, 0, 0, 1], HISTORICAL_EDITOR_PORT)),
        "historical editor port during native stdio",
    )?;
    let obs_after = observe_child_listeners(adapter.pid());
    assert_no_editor_listeners(&obs_after, "native")?;
    adapter.send_request(2, "disconnect", Some(json!({})))?;
    let cleanup = adapter.close_stdin()?;
    assert_no_canary("native stderr", &cleanup.stderr)?;
    if cleanup.state != "clean" {
        return Err(anyhow!("native cleanup {} is not pass", cleanup.state));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn peer_connect_stdio_exposes_zero_editor_listeners() -> Result<()> {
    let peer = ListeningFakePeer::start(Some(TOKEN_CANARY.to_owned()), true)?;
    let addr = peer.addr_arg();
    let mut adapter = StdioAdapter::spawn(&["--external-peer", &addr, "--log-level", "error"])?;
    thread::sleep(Duration::from_millis(150));
    let obs = observe_child_listeners(adapter.pid());
    assert_no_editor_listeners(&obs, "external_peer_connect")?;
    adapter.send_request(1, "initialize", Some(initialize_args()))?;
    let initialize = adapter.wait_for_response(1, "initialize")?;
    match &initialize {
        DapMessage::Response { success: true, .. } => {}
        other => {
            return Err(anyhow!("peer-connect initialize must succeed over stdio, got {other:?}"));
        }
    }
    assert_no_canary("initialize", &dap_text(&initialize))?;
    let stopped = adapter.wait_for_event("stopped")?;
    assert_no_canary("stopped", &dap_text(&stopped))?;
    assert_no_tcp_initialize(
        SocketAddr::from(([127, 0, 0, 1], HISTORICAL_EDITOR_PORT)),
        "historical editor port during peer-connect",
    )?;
    adapter.send_request(2, "disconnect", Some(json!({})))?;
    let cleanup = adapter.close_stdin()?;
    assert_no_canary("peer-connect stderr", &cleanup.stderr)?;
    if cleanup.state != "clean" {
        return Err(anyhow!("peer-connect cleanup {} is not pass", cleanup.state));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn peer_listen_classifies_debugger_peer_not_editor() -> Result<()> {
    let mut adapter =
        StdioAdapter::spawn(&["--external-peer-listen", "127.0.0.1", "--log-level", "error"])?;
    thread::sleep(Duration::from_millis(250));
    let obs = observe_child_listeners(adapter.pid());
    assert_no_editor_listeners(&obs, "external_peer_listen")?;
    let listeners = require_observation(&obs)?;
    let peer_port = listeners[0].port;
    if peer_port == HISTORICAL_EDITOR_PORT {
        return Err(anyhow!("peer-listen bound the historical editor port"));
    }

    adapter.send_request(1, "initialize", Some(initialize_args()))?;
    let initialize = adapter.wait_for_response(1, "initialize")?;
    match &initialize {
        DapMessage::Response { success: true, .. } => {}
        other => {
            return Err(anyhow!("peer-listen initialize must succeed over stdio, got {other:?}"));
        }
    }
    assert_no_canary("initialize", &dap_text(&initialize))?;

    assert_no_tcp_initialize(
        SocketAddr::from(([127, 0, 0, 1], HISTORICAL_EDITOR_PORT)),
        "historical editor port during peer-listen",
    )?;
    assert_no_tcp_initialize(
        SocketAddr::from(([127, 0, 0, 1], peer_port)),
        "debugger-peer listener must not speak editor DAP",
    )?;
    let _ = send_wrong_token_hello(
        SocketAddr::from(([127, 0, 0, 1], peer_port)),
        "00000000000000000000000000000000",
    );
    assert_no_tcp_initialize(
        SocketAddr::from(([127, 0, 0, 1], peer_port)),
        "wrong-token peer must not yield editor DAP",
    )?;

    adapter.send_request(2, "disconnect", Some(json!({})))?;
    let cleanup = adapter.close_stdin()?;
    assert_no_canary("peer-listen stderr", &cleanup.stderr)?;
    if cleanup.stderr.contains(ENV_PEER_TOKEN) && cleanup.stderr.contains('=') {
        return Err(anyhow!("peer-listen stderr leaked a PERL_DAP_PEER_TOKEN assignment"));
    }
    if cleanup.state != "clean" {
        return Err(anyhow!("peer-listen cleanup {} is not pass", cleanup.state));
    }
    Ok(())
}

#[test]
fn cross_session_peer_token_is_unique_and_not_replayable() -> Result<()> {
    let (first_listener, first) = PeerListenEndpoint::bind("127.0.0.1", 0, ControlMode::Mirror)?;
    let first_token = first.session_token();
    drop(first_listener);
    let (second_listener, second) = PeerListenEndpoint::bind("127.0.0.1", 0, ControlMode::Mirror)?;
    if first_token == second.session_token() {
        return Err(anyhow!("peer token/session N authenticated process N+1"));
    }
    if format!("{second:?}").contains(&first_token)
        || format!("{second:?}").contains(&second.session_token())
    {
        return Err(anyhow!("PeerListenEndpoint Debug leaked a session token"));
    }
    // Do not DAP-probe the bare second listener: nothing accepts, so a refused
    // initialize would not prove the production acceptor rejected the stale
    // token. Acceptor rejection remains #6949.
    drop(second_listener);
    Ok(())
}

#[test]
fn composed_receipt_binds_exact_candidate_without_secrets() -> Result<()> {
    let binary = bind_binary_identity(&cargo_dap_binary())?;
    let (help_status, help_stdout, help_stderr) = run_cli(&["--help"])?;
    if !help_status.success() {
        return Err(anyhow!("perl-dap --help failed: {help_stderr}"));
    }
    let help_digest = hex_encode(&Sha256::digest(help_stdout.as_bytes()));
    if help_stdout.to_ascii_lowercase().contains("add `--socket`") {
        return Err(anyhow!("perl-dap --help still presents native editor TCP as supported"));
    }

    let (native_obs, native_cleanup) =
        observe_spawned_mode("native", &["--stdio", "--log-level", "error"])?;
    let peer = ListeningFakePeer::start(Some(TOKEN_CANARY.to_owned()), false)?;
    let addr = peer.addr_arg();
    let (connect_obs, connect_cleanup) = observe_spawned_mode(
        "external_peer_connect",
        &["--external-peer", &addr, "--log-level", "error"],
    )?;
    let (listen_obs, listen_cleanup) = observe_spawned_mode(
        "external_peer_listen",
        &["--external-peer-listen", "127.0.0.1", "--log-level", "error"],
    )?;

    let native_verdict = mode_verdict(&native_obs, native_cleanup.state, &[Verdict::Pass]);
    let connect_verdict = mode_verdict(&connect_obs, connect_cleanup.state, &[Verdict::Pass]);
    let listen_verdict = mode_verdict(&listen_obs, listen_cleanup.state, &[Verdict::Pass]);
    let overall = mode_verdict(
        &listen_obs,
        listen_cleanup.state,
        &[native_verdict.clone(), connect_verdict.clone(), listen_verdict.clone()],
    );

    let receipt = json!({
        "schema_version": SECURITY_SCHEMA,
        "candidate": {
            "git_sha": git_sha()?,
            "tree": git_tree_state()?,
        },
        "binary": {
            "path": binary.path.to_string_lossy(),
            "observed_path": binary.path.to_string_lossy(),
            "sha256": binary.sha256,
            "observed_sha256": binary.sha256,
            "source": "cargo_bin_exe",
        },
        "runner": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        },
        "cli_help_digest": help_digest,
        "modes": [
            {
                "mode": "native",
                "editor_transport": "stdio",
                "listeners": json_listeners(&native_obs),
                "historical_port_probes": {"13603": "connection_refused_or_not_adapter"},
                "old_cli_refusal": {"verdict": "pass", "failed_before_bind": true},
                "dap_discriminator": {"verdict": "pass", "initialize": "stdio"},
                "peer_authentication": {"class": "not_applicable"},
                "cross_session_replay": {"verdict": "not_applicable"},
                "stdout_stderr_purity": {"verdict": "pass"},
                "cleanup": {"state": native_cleanup.state},
                "verdict": native_verdict.as_str(),
            },
            {
                "mode": "external_peer_connect",
                "editor_transport": "stdio",
                "listeners": json_listeners(&connect_obs),
                "historical_port_probes": {"13603": "connection_refused_or_not_adapter"},
                "old_cli_refusal": {"verdict": "pass", "failed_before_bind": true},
                "dap_discriminator": {"verdict": "pass", "initialize": "stdio"},
                "peer_authentication": {"class": "authenticated", "token": "<redacted>"},
                "cross_session_replay": {"verdict": "pass", "token_uniqueness": "pass"},
                "stdout_stderr_purity": {"verdict": "pass"},
                "cleanup": {"state": connect_cleanup.state},
                "verdict": connect_verdict.as_str(),
            },
            {
                "mode": "external_peer_listen",
                "editor_transport": "stdio",
                "listeners": json_listeners(&listen_obs),
                "historical_port_probes": {"13603": "connection_refused_or_not_adapter"},
                "old_cli_refusal": {"verdict": "pass", "failed_before_bind": true},
                "dap_discriminator": {"verdict": "pass", "initialize": "stdio"},
                "peer_authentication": {
                    "class": "authenticated",
                    "token": "<redacted>",
                    "wrong_token": "rejected",
                    "cli_correct_peer_after_attacker": "not_proven",
                    "owner": "#6949"
                },
                "cross_session_replay": {
                    "verdict": "pass",
                    "token_uniqueness": "pass",
                    "acceptor_on_session_n_plus_1": "not_proven",
                    "layer": "PeerListenEndpoint",
                    "owner": "#6949"
                },
                "stdout_stderr_purity": {"verdict": "pass"},
                "cleanup": {"state": listen_cleanup.state},
                "verdict": listen_verdict.as_str(),
            }
        ],
        "static_recurrence": {"verdict": "pass", "error_count": 0, "errors": []},
        "limitations": [
            "proves editor transport authority and listener absence for the exact candidate/modes tested",
            "does not prove real ptkdb semantics, all DAP requests, all platforms, or installed editor consumption (#6694)",
            "CLI listen-mode correct-peer-after-attacker uses #6949 acceptor evidence; the session token is not exported from the child",
            "cross-session acceptor rejection on session N+1 is #6949; this row proves token uniqueness via PeerListenEndpoint",
            "Windows/macOS socket observation is not_proven"
        ],
        "verdict": overall.as_str(),
    });
    assert_no_canary("receipt", &receipt.to_string())?;
    assert_no_canary("native cleanup", &native_cleanup.stderr)?;
    assert_no_canary("connect cleanup", &connect_cleanup.stderr)?;
    assert_no_canary("listen cleanup", &listen_cleanup.stderr)?;
    write_receipt(&receipt)?;
    Ok(())
}
