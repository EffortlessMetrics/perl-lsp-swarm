//! Discriminating proof that native editor-facing TCP is retired (#10565).
//!
//! These tests spawn the packaged `perl-dap` binary. They fail if native
//! `--socket`/`--port` still binds, is silently ignored in favor of stdio, or
//! if debugger-peer TCP / DAP attach are collapsed into editor transport.

use anyhow::{Context, Result, anyhow};
use perl_dap::DapMessage;
use perl_dap::backend::peer_launch::ENV_PEER_TOKEN;
use perl_dap::backend::{
    ExternalPeerLaunchConfig, PeerRendezvousMode, prepare_mirror_listen_session,
};
use perl_lsp_rs_core::transport::framing::frame;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread;
use std::time::{Duration, Instant};

const HISTORICAL_NATIVE_PORT: u16 = 13_603;
const CLI_TIMEOUT: Duration = Duration::from_secs(3);
const STDIO_TIMEOUT: Duration = Duration::from_secs(5);

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

fn run_native_cli(args: &[&str]) -> io::Result<(ExitStatus, String, String)> {
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
                    "perl-dap {args:?} still running after {CLI_TIMEOUT:?}; native --socket must fail before bind rather than accept"
                ),
            ));
        }
    };
    let stdout = stdout.join().unwrap_or_else(|_| "<stdout reader panicked>".to_owned());
    let stderr = stderr.join().unwrap_or_else(|_| "<stderr reader panicked>".to_owned());
    Ok((status, stdout, stderr))
}

fn assert_native_socket_retired(status: &ExitStatus, stdout: &str, stderr: &str) -> Result<()> {
    if status.success() {
        return Err(anyhow!(
            "native --socket/--port must exit nonzero before bind; got success. stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    let combined = format!("{stdout}\n{stderr}");
    if !combined.contains("perl-dap --stdio") {
        return Err(anyhow!(
            "retired native socket error must name `perl-dap --stdio`; stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    if combined.contains("already in use") {
        return Err(anyhow!(
            "native --socket must fail before bind; AddrInUse remediation means the listener still ran. stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    if combined.to_ascii_lowercase().contains("listening") {
        return Err(anyhow!(
            "retired native socket path must not start a listener. stdout={stdout:?} stderr={stderr:?}"
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

struct StdioAdapter {
    child: Child,
    stdin: std::process::ChildStdin,
    rx: Receiver<std::result::Result<DapMessage, String>>,
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
        Ok(Self { child, stdin, rx: spawn_frame_reader(stdout) })
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
        write!(self.stdin, "Content-Length: {}\r\n\r\n", payload.len())?;
        self.stdin.write_all(&payload)?;
        self.stdin.flush()?;
        Ok(())
    }

    fn wait_for_response(&self, request_seq: i64, command: &str) -> Result<DapMessage> {
        wait_for_message(&self.rx, format!("response `{command}` #{request_seq}"), |msg| {
            matches!(
                msg,
                DapMessage::Response { request_seq: actual, command: actual_command, .. }
                    if *actual == request_seq && actual_command == command
            )
        })
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

fn wait_for_message<F>(
    rx: &Receiver<std::result::Result<DapMessage, String>>,
    description: String,
    matches_message: F,
) -> Result<DapMessage>
where
    F: Fn(&DapMessage) -> bool,
{
    let deadline = Instant::now() + STDIO_TIMEOUT;
    let mut observed = Vec::new();
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(anyhow!("timeout waiting for {description}; observed {observed:?}"));
        }
        match rx.recv_timeout(deadline.saturating_duration_since(now)) {
            Ok(Ok(message)) if matches_message(&message) => return Ok(message),
            Ok(Ok(message)) => observed.push(format!("{message:?}")),
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

fn assert_no_child_listeners(pid: u32) -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Ok(());
    }
    let ports = child_listening_tcp_ports(pid)
        .with_context(|| format!("failed to inspect listening sockets for pid {pid}"))?;
    if !ports.is_empty() {
        return Err(anyhow!("perl-dap pid {pid} bound editor-facing TCP listener(s) on {ports:?}"));
    }
    Ok(())
}

#[test]
fn stdio_starts_without_a_tcp_listener() -> Result<()> {
    let mut adapter = StdioAdapter::spawn(&["--stdio", "--log-level", "error"])?;
    thread::sleep(Duration::from_millis(150));
    assert_no_child_listeners(adapter.pid())?;

    adapter.send_request(
        1,
        "initialize",
        Some(json!({"adapterID": "perl-dap", "clientID": "retirement-test"})),
    )?;
    match adapter.wait_for_response(1, "initialize")? {
        DapMessage::Response { success: true, .. } => {}
        other => return Err(anyhow!("initialize must succeed over stdio, got {other:?}")),
    }
    assert_no_child_listeners(adapter.pid())?;
    adapter.send_request(2, "disconnect", Some(json!({})))?;
    Ok(())
}

#[test]
fn default_native_launch_is_stdio_only() -> Result<()> {
    let mut adapter = StdioAdapter::spawn(&["--log-level", "error"])?;
    thread::sleep(Duration::from_millis(150));
    assert_no_child_listeners(adapter.pid())?;
    adapter.send_request(
        1,
        "initialize",
        Some(json!({"adapterID": "perl-dap", "clientID": "retirement-test"})),
    )?;
    match adapter.wait_for_response(1, "initialize")? {
        DapMessage::Response { success: true, .. } => {}
        other => return Err(anyhow!("default launch must speak DAP over stdio, got {other:?}")),
    }
    Ok(())
}

#[test]
fn old_native_socket_flag_fails_before_bind_with_stdio_migration() -> Result<()> {
    let occupied = TcpListener::bind(("127.0.0.1", 0))?;
    let port = occupied.local_addr()?.port();
    let (status, stdout, stderr) =
        run_native_cli(&["--socket", "--port", &port.to_string(), "--log-level", "error"])?;
    assert_native_socket_retired(&status, &stdout, &stderr)?;
    Ok(())
}

#[test]
fn old_native_port_flag_fails_before_bind_with_stdio_migration() -> Result<()> {
    let occupied = TcpListener::bind(("127.0.0.1", 0))?;
    let port = occupied.local_addr()?.port();
    let (status, stdout, stderr) =
        run_native_cli(&["--port", &port.to_string(), "--log-level", "error"])?;
    assert_native_socket_retired(&status, &stdout, &stderr)?;
    Ok(())
}

#[test]
fn native_socket_on_a_free_port_still_fails_before_bind() -> Result<()> {
    let probe = TcpListener::bind(("127.0.0.1", 0))?;
    let port = probe.local_addr()?.port();
    drop(probe);
    let (status, stdout, stderr) =
        run_native_cli(&["--socket", "--port", &port.to_string(), "--log-level", "error"])?;
    assert_native_socket_retired(&status, &stdout, &stderr)?;
    Ok(())
}

#[test]
fn holding_the_historical_port_does_not_affect_stdio() -> Result<()> {
    let _holder = TcpListener::bind(("127.0.0.1", HISTORICAL_NATIVE_PORT)).ok();
    let mut adapter = StdioAdapter::spawn(&["--stdio", "--log-level", "error"])?;
    thread::sleep(Duration::from_millis(150));
    assert_no_child_listeners(adapter.pid())?;
    adapter.send_request(
        1,
        "initialize",
        Some(json!({"adapterID": "perl-dap", "clientID": "retirement-test"})),
    )?;
    match adapter.wait_for_response(1, "initialize")? {
        DapMessage::Response { success: true, .. } => {}
        other => {
            return Err(anyhow!(
                "stdio initialize must succeed while historical port is held, got {other:?}"
            ));
        }
    }
    Ok(())
}

#[test]
fn debugger_peer_tcp_listener_remains_authenticated() -> Result<()> {
    let config = ExternalPeerLaunchConfig {
        mode: PeerRendezvousMode::Listen,
        port: 0,
        ..ExternalPeerLaunchConfig::default()
    };
    let (listener, endpoint, _bridge) = prepare_mirror_listen_session(&config)
        .context("debugger-peer listen bind must remain available")?;
    let addr = listener.local_addr()?;
    if addr.port() == 0 {
        return Err(anyhow!("debugger-peer listener must expose a bound port"));
    }
    let env = endpoint.env_vars();
    let token = env
        .iter()
        .find(|(key, _)| key == ENV_PEER_TOKEN)
        .map(|(_, value)| value.as_str())
        .ok_or_else(|| anyhow!("debugger-peer env contract lost PERL_DAP_PEER_TOKEN"))?;
    if token.is_empty() {
        return Err(anyhow!("debugger-peer token must remain non-empty"));
    }
    if format!("{endpoint:?}").contains(token) {
        return Err(anyhow!("debugger-peer Debug impl must keep the token redacted"));
    }
    Ok(())
}

#[test]
fn attach_over_stdio_stays_independent_of_editor_tcp() -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    let (accepted_tx, accepted_rx) = channel();
    let server = thread::spawn(move || -> Result<()> {
        listener.set_nonblocking(false)?;
        let (mut socket, _) = listener.accept().context("expected outbound DAP attach connect")?;
        socket.set_read_timeout(Some(Duration::from_secs(2)))?;
        accepted_tx.send(()).context("failed to report accepted attach")?;
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
        socket.write_all(&frame(stopped.as_bytes()))?;
        socket.flush()?;
        let mut buf = [0u8; 64];
        loop {
            match socket.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::TimedOut => break,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    });

    let mut adapter = StdioAdapter::spawn(&["--stdio", "--log-level", "error"])?;
    adapter.send_request(
        1,
        "initialize",
        Some(json!({"adapterID": "perl-dap", "clientID": "retirement-test"})),
    )?;
    match adapter.wait_for_response(1, "initialize")? {
        DapMessage::Response { success: true, .. } => {}
        other => return Err(anyhow!("initialize must succeed before attach, got {other:?}")),
    }
    assert_no_child_listeners(adapter.pid())?;

    adapter.send_request(
        2,
        "attach",
        Some(json!({
            "host": "127.0.0.1",
            "port": port,
            "timeout": 2000
        })),
    )?;
    match adapter.wait_for_response(2, "attach")? {
        DapMessage::Response { success: true, .. } => {}
        DapMessage::Response { success: false, message, .. } => {
            return Err(anyhow!(
                "attach over stdio must remain a protocol request, failed: {message:?}"
            ));
        }
        other => return Err(anyhow!("expected attach response, got {other:?}")),
    }
    accepted_rx.recv_timeout(STDIO_TIMEOUT).context(
        "adapter must connect outbound to the attach host/port; missing accept means editor TCP was used or attach vanished",
    )?;
    assert_no_child_listeners(adapter.pid())?;
    adapter.send_request(3, "disconnect", Some(json!({})))?;
    server.join().map_err(|_| anyhow!("attach stub thread panicked"))??;
    Ok(())
}

#[test]
fn production_source_rejects_a_returned_native_editor_listener() -> Result<()> {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for relative in ["src/debug_adapter/transport.rs", "src/server/lifecycle.rs", "src/main.rs"] {
        let path = crate_root.join(relative);
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if text.contains("pub fn run_socket") || text.contains("pub(crate) fn run_socket") {
            return Err(anyhow!(
                "{relative} regained native editor run_socket production admission"
            ));
        }
    }
    let transport = fs::read_to_string(crate_root.join("src/debug_adapter/transport.rs"))?;
    let production = production_source_without_cfg_test_mods(&transport);
    if production.contains("TcpListener::bind") {
        return Err(anyhow!(
            "debug_adapter/transport.rs production source regained TcpListener::bind"
        ));
    }
    let main = production_source_without_cfg_test_mods(&fs::read_to_string(
        crate_root.join("src/main.rs"),
    )?);
    if !main.contains("native_editor_socket_retired") {
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

#[test]
fn historical_port_connect_does_not_reach_a_stdio_adapter() -> Result<()> {
    let adapter = StdioAdapter::spawn(&["--stdio", "--log-level", "error"])?;
    thread::sleep(Duration::from_millis(150));
    if TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], HISTORICAL_NATIVE_PORT)),
        Duration::from_millis(200),
    )
    .is_ok()
        && cfg!(target_os = "linux")
    {
        let ports = child_listening_tcp_ports(adapter.pid()).unwrap_or_default();
        if ports.contains(&HISTORICAL_NATIVE_PORT) {
            return Err(anyhow!(
                "stdio adapter bound the historical native editor port {HISTORICAL_NATIVE_PORT}"
            ));
        }
    }
    assert_no_child_listeners(adapter.pid())?;
    Ok(())
}
