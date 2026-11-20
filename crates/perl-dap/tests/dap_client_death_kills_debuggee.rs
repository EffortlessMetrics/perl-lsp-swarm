//! Pin abrupt client-transport death during an active debug session: closing the
//! editor stdin (EOF on the adapter transport) must exit the adapter and reap the
//! launched debuggee (#16623 item 2).

mod common;

use anyhow::{Context, Result, anyhow, bail, ensure};
use common::{debuggee_perl_or_typed_skip, perl_available};
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const IO_TIMEOUT: Duration = Duration::from_secs(20);
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);

struct ClientDeathSession {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Receiver<Result<Value, String>>,
    reader: Option<JoinHandle<()>>,
    backlog: Vec<Value>,
    stderr_log: PathBuf,
    seq: i64,
}

impl ClientDeathSession {
    fn start(root: &Path) -> Result<Self> {
        let binary = std::env::var_os("PERL_DAP_TEST_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_perl-dap")));
        let stderr_log = root.join("adapter-stderr.log");
        let mut child = Command::new(&binary)
            .args(["--stdio", "--trusted-root"])
            .arg(root)
            .args(["--log-level", "error"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(fs::File::create(&stderr_log)?))
            .spawn()
            .with_context(|| format!("spawning perl-dap binary {binary:?}"))?;
        let stdin = child.stdin.take().context("adapter stdin")?;
        let stdout = child.stdout.take().context("adapter stdout")?;
        let (tx, messages) = channel();
        let reader = thread::spawn(move || {
            let mut stream = BufReader::new(stdout);
            loop {
                let next = read_message(&mut stream).map_err(|error| error.to_string());
                let failed = next.is_err();
                if tx.send(next).is_err() || failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            stdin: Some(stdin),
            messages,
            reader: Some(reader),
            backlog: Vec::new(),
            stderr_log,
            seq: 0,
        })
    }

    fn receive(&self, deadline: Instant) -> Result<Value> {
        self.messages
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .context("waiting for framed adapter output")?
            .map_err(|error| anyhow!("{error}\n{}", self.render_stderr_tail()))
    }

    fn request(&mut self, command: &str, arguments: Value) -> Result<Value> {
        self.seq += 1;
        let body = serde_json::to_vec(&json!({
            "type": "request",
            "seq": self.seq,
            "command": command,
            "arguments": arguments,
        }))?;
        let stdin = self.stdin.as_mut().context("adapter stdin closed")?;
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len())?;
        stdin.write_all(&body)?;
        stdin.flush()?;
        let deadline = Instant::now() + IO_TIMEOUT;
        loop {
            let message = self.receive(deadline)?;
            if message["type"] == "response" && message["request_seq"] == self.seq {
                ensure!(message["command"] == command, "response command correlation");
                return Ok(message);
            }
            self.backlog.push(message);
        }
    }

    fn success(&mut self, command: &str, arguments: Value) -> Result<Value> {
        let response = self.request(command, arguments)?;
        if response["success"] != true {
            bail!("{command} failed: {response}");
        }
        Ok(response["body"].clone())
    }

    fn event(&mut self, name: &str) -> Result<()> {
        if let Some(index) = self.backlog.iter().position(|value| value["event"] == name) {
            self.backlog.remove(index);
            return Ok(());
        }
        let deadline = Instant::now() + IO_TIMEOUT;
        loop {
            let message = self.receive(deadline)?;
            if message["event"] == name {
                return Ok(());
            }
            self.backlog.push(message);
        }
    }

    fn render_stderr_tail(&self) -> String {
        match fs::read_to_string(&self.stderr_log) {
            Ok(text) if text.is_empty() => "adapter stderr: <empty>".to_string(),
            Ok(text) => format!("adapter stderr tail:\n{text}"),
            Err(error) => format!("adapter stderr unavailable: {error}"),
        }
    }

    fn close_stdin_and_wait_exit(&mut self) -> Result<()> {
        self.stdin.take();
        let deadline = Instant::now() + EXIT_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait()? {
                ensure!(
                    status.success(),
                    "adapter exited unsuccessfully after stdin EOF: {status}"
                );
                if let Some(reader) = self.reader.take() {
                    reader.join().map_err(|_| anyhow!("frame reader panicked"))?;
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("adapter did not exit within bound after client stdin EOF");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for ClientDeathSession {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            self.stdin.take();
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn read_message(reader: &mut impl BufRead) -> Result<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            bail!("adapter stdout EOF");
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = Some(value.trim().parse::<usize>()?);
        }
    }
    let length = length.context("missing Content-Length")?;
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn process_exists(pid: u32) -> Result<bool> {
    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()?;
        if !output.status.success() {
            bail!(
                "tasklist failed while checking PID {pid}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).lines().any(|line| {
            line.split(',')
                .nth(1)
                .map(|field| field.trim_matches('"') == pid.to_string())
                .unwrap_or(false)
        }))
    }
    #[cfg(unix)]
    {
        Ok(Command::new("kill").args(["-0", &pid.to_string()]).status()?.success())
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = pid;
        Ok(false)
    }
}

fn wait_for_process_exit(pid: u32, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while process_exists(pid)? {
        if Instant::now() >= deadline {
            bail!("debuggee PID {pid} survived client transport EOF cleanup");
        }
        thread::sleep(Duration::from_millis(25));
    }
    Ok(())
}

fn wait_for_child_pid(marker: &Path, timeout: Duration) -> Result<u32> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(contents) = fs::read_to_string(marker)
            && let Ok(pid) = contents.trim().parse::<u32>()
        {
            return Ok(pid);
        }
        if Instant::now() >= deadline {
            bail!("DAP launch marker did not publish child PID: {}", marker.display());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn client_transport_eof_mid_session_exits_adapter_and_reaps_debuggee() -> Result<()> {
    if !perl_available() {
        return Ok(());
    }
    let perl = debuggee_perl_or_typed_skip("client_transport_eof_mid_session")
        .ok_or_else(|| anyhow!("perl availability changed before launch"))?;

    let workspace = tempfile::tempdir()?;
    let script = workspace.path().join("hang.pl");
    fs::write(
        &script,
        br#"use strict;
use warnings;
open my $pid_file, '>', $ENV{PERL_LSP_DAP_TEST_CHILD_PID_FILE}
    or die "cannot write child PID: $!";
my $pid = $$;
if ($^O eq 'MSWin32') {
    eval { require Win32; $pid = Win32::GetCurrentProcessId(); 1 }
        or die "cannot resolve native Windows PID: $@";
}
print $pid_file "$pid\n";
close $pid_file or die "cannot close child PID: $!";
while (1) {
    select undef, undef, undef, 0.1;
}
"#,
    )?;
    let child_pid_file = workspace.path().join("dap-child.pid");

    let mut session = ClientDeathSession::start(workspace.path())?;
    session.success(
        "initialize",
        json!({ "adapterID": "perl-dap", "linesStartAt1": true, "columnsStartAt1": true }),
    )?;
    session.event("initialized")?;

    session.success(
        "launch",
        json!({
            "program": script.to_string_lossy(),
            "stopOnEntry": false,
            "perlPath": perl.binary,
            "env": {
                "PERL_LSP_DAP_TEST_CHILD_PID_FILE": child_pid_file.to_string_lossy()
            }
        }),
    )?;
    session.success("configurationDone", json!({}))?;

    let debuggee_pid = wait_for_child_pid(&child_pid_file, Duration::from_secs(5))?;
    ensure!(
        process_exists(debuggee_pid)?,
        "debuggee PID {debuggee_pid} was not live before client transport EOF"
    );

    session.close_stdin_and_wait_exit()?;
    wait_for_process_exit(debuggee_pid, Duration::from_secs(5))?;
    Ok(())
}
