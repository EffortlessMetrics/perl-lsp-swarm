//! Native launch argv admission and recovery through the real public adapter.
//! Missing exact binaries or pipe-capable Perl fail this proof; they never pass
//! as an installed-product or mocked debugger acceptance result (#17733).

mod common;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(20);
const FIXTURE: &str = "use strict;\nuse warnings; BEGIN { if (my $p = $ENV{ARGV_RECEIPT}) { open my $f, '>', qq($p.begin) or die $!; print {$f} 'executed'; close $f; } }\nmy $argvhex = join ':', map { unpack 'H*', $_ } @ARGV;\nmy $argc = scalar @ARGV;\nmy $marker = 7;\nopen my $out, '>', $ENV{ARGV_RECEIPT} or die $!;\nprint {$out} $argvhex;\nclose $out;\n$marker += 1;\n$marker += 2;\nprint qq($argc\\n);\n";

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Receiver<Result<Value, String>>,
    reader: Option<JoinHandle<()>>,
    backlog: Vec<Value>,
    seq: i64,
}

impl Session {
    fn start(root: &Path) -> Result<Self> {
        let binary = std::env::var_os("PERL_DAP_TEST_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_perl-dap").into());
        let mut child = Command::new(binary)
            .args(["--stdio", "--trusted-root"])
            .arg(root)
            .args(["--log-level", "error"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(fs::File::create(root.join("adapter-stderr.log"))?))
            .spawn()
            .context("spawning exact public perl-dap binary")?;
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
        let mut session = Self {
            child,
            stdin: Some(stdin),
            messages,
            reader: Some(reader),
            backlog: Vec::new(),
            seq: 0,
        };
        session.success(
            "initialize",
            json!({
                "adapterID": "perl-dap", "linesStartAt1": true, "columnsStartAt1": true
            }),
        )?;
        session.event("initialized")?;
        Ok(session)
    }

    fn receive(&self, deadline: Instant) -> Result<Value> {
        self.messages
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .context("waiting for framed adapter output")?
            .map_err(|error| anyhow!(error))
    }

    fn request(&mut self, command: &str, arguments: Value) -> Result<Value> {
        self.seq += 1;
        let body = serde_json::to_vec(&json!({
            "type": "request", "seq": self.seq, "command": command, "arguments": arguments
        }))?;
        let stdin = self.stdin.as_mut().context("adapter stdin closed")?;
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len())?;
        stdin.write_all(&body)?;
        stdin.flush()?;
        let deadline = Instant::now() + WAIT;
        loop {
            let message = self.receive(deadline)?;
            if message["type"] == "response" && message["request_seq"] == self.seq {
                assert_eq!(message["command"], command, "response command correlation");
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

    fn event(&mut self, name: &str) -> Result<Value> {
        if let Some(index) = self.backlog.iter().position(|value| value["event"] == name) {
            return Ok(self.backlog.remove(index)["body"].clone());
        }
        let deadline = Instant::now() + WAIT;
        loop {
            let message = self.receive(deadline)?;
            if message["event"] == name {
                return Ok(message["body"].clone());
            }
            self.backlog.push(message);
        }
    }

    fn finish(&mut self) -> Result<()> {
        self.success("disconnect", json!({"terminateDebuggee": true}))?;
        self.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait()? {
                assert!(status.success(), "adapter exit: {status}");
                if let Some(reader) = self.reader.take() {
                    reader.join().map_err(|_| anyhow!("frame reader panicked"))?;
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("adapter did not exit after disconnect");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.request("disconnect", json!({"terminateDebuggee": true}));
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
    if length > 1024 * 1024 {
        bail!("adapter frame exceeds proof limit");
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn launch(program: &Path, perl: &Path, receipt: &Path) -> Value {
    json!({
        "program": program, "perlPath": perl, "stopOnEntry": false,
        "env": {"ARGV_RECEIPT": receipt}, "debuggeeTimeoutSeconds": 30
    })
}

fn assert_refused(response: &Value, field: &str) -> Result<()> {
    assert_eq!(response["success"], false, "malformed argv must refuse: {response}");
    let message = response["message"].as_str().context("refusal message")?;
    assert!(message.contains(field), "field-specific refusal: {message}");
    assert!(!message.contains("private-payload"), "argument data leaked: {message}");
    Ok(())
}

#[test]
fn malformed_launch_args_refuse_before_interpreter_or_program_probes() -> Result<()> {
    let root = tempfile::tempdir()?;
    let mut session = Session::start(root.path())?;
    for (args, field) in [
        (json!("private-payload"), "args"),
        (json!({"private-payload": 1}), "args"),
        (json!(17), "args"),
        (json!(false), "args"),
        (json!([17, "valid"]), "args[0]"),
        (json!(["valid", null, "valid"]), "args[1]"),
        (json!(["valid", false]), "args[1]"),
        (json!(["valid", {"private-payload": 1}]), "args[1]"),
        (json!(["valid", []]), "args[1]"),
    ] {
        let mut arguments = launch(
            &root.path().join("missing.pl"),
            &root.path().join("missing-perl"),
            &root.path().join("never-written"),
        );
        arguments["args"] = args;
        assert_refused(&session.request("launch", arguments)?, field)?;
        let threads = session.success("threads", Value::Null)?;
        assert_eq!(threads["threads"], json!([]), "refusal must not create a session");
    }
    session.finish()
}

fn stopped_marker(session: &mut Session, thread: i64, expected_line: i64) -> Result<Vec<Value>> {
    let stack = session.success("stackTrace", json!({"threadId": thread}))?;
    let frames = stack["stackFrames"].as_array().context("stack frames")?;
    let top = frames.first().context("current stopped frame")?;
    assert_eq!(top["line"], expected_line, "real stopped source line");
    let scopes = session.success("scopes", json!({"frameId": top["id"]}))?;
    let locals = scopes["scopes"]
        .as_array()
        .context("scopes")?
        .iter()
        .find(|scope| scope["name"] == "Locals")
        .context("Locals scope")?;
    let variables = session.success(
        "variables",
        json!({
            "variablesReference": locals["variablesReference"]
        }),
    )?;
    Ok(variables["variables"].as_array().context("Variables rows")?.clone())
}

#[test]
fn launch_args_real_refusal_recovery_step_variables_and_relaunch() -> Result<()> {
    let perl = common::resolve_launch_perl_path()
        .map_err(|error| anyhow!(error))?
        .context("NOT_PROVEN: a pipe-capable real Perl is required")?;
    let root = tempfile::tempdir()?;
    let program = root.path().join("argv.pl");
    let receipt = root.path().join("argv.hex");
    let begin_canary = root.path().join("argv.hex.begin");
    fs::write(&program, FIXTURE)?;
    let mut session = Session::start(root.path())?;
    let mut arguments = launch(&program, &perl, &receipt);
    arguments["args"] = json!(["left", 17, "right"]);
    assert_refused(&session.request("launch", arguments.clone())?, "args[1]")?;
    assert!(!receipt.exists(), "refused launch executed the fixture");
    assert!(!begin_canary.exists(), "refused launch compiled or executed the fixture");

    let argv = ["left space", "", "雪", "'quoted'", "\\backslash", "$(touch private-payload);*"];
    arguments["args"] = json!(argv);
    session.success("launch", arguments)?;
    session.success(
        "setBreakpoints",
        json!({"source": {"path": program}, "breakpoints": [{"line": 9}]}),
    )?;
    session.success("configurationDone", Value::Null)?;
    let stopped = session.event("stopped")?;
    let thread = stopped["threadId"].as_i64().context("stopped thread")?;
    let variables = stopped_marker(&mut session, thread, 9)?;
    assert!(
        variables.iter().any(|row| row["name"] == "$argc" && row["value"] == "6"),
        "real argc: {variables:?}"
    );
    assert!(
        variables.iter().any(|row| row["name"] == "$marker" && row["value"] == "7"),
        "real marker: {variables:?}"
    );
    let expected_hex = argv
        .iter()
        .map(|arg| arg.as_bytes().iter().map(|byte| format!("{byte:02x}")).collect::<String>())
        .collect::<Vec<_>>()
        .join(":");
    assert_eq!(fs::read_to_string(&receipt)?, expected_hex, "exact debuggee argv bytes");
    assert_eq!(fs::read_to_string(&begin_canary)?, "executed", "BEGIN canary is live");

    session.success("next", json!({"threadId": thread}))?;
    session.event("stopped")?;
    let later = stopped_marker(&mut session, thread, 10)?;
    assert!(
        later.iter().any(|row| row["name"] == "$marker" && row["value"] == "8"),
        "stepped real marker: {later:?}"
    );
    session.success("terminate", Value::Null)?;
    session.event("terminated")?;

    let missing_perl =
        root.path().join("missing").join(if cfg!(windows) { "perl.exe" } else { "perl" });
    let mut missing = launch(&program, &missing_perl, &receipt);
    missing["args"] = json!([]);
    let refused = session.request("launch", missing)?;
    assert_eq!(refused["success"], false, "missing Perl must remain a real refusal");
    let diagnostic = refused["message"].as_str().context("missing interpreter diagnostic")?;
    assert!(
        diagnostic.contains(&missing_perl.to_string_lossy().to_string()),
        "explicit missing interpreter identity: {diagnostic}"
    );

    for args in [None, Some(Value::Null), Some(json!([]))] {
        fs::remove_file(&receipt)?;
        let mut arguments = launch(&program, &perl, &receipt);
        if let Some(args) = args {
            arguments["args"] = args;
        }
        session.success("launch", arguments)?;
        session.success(
            "setBreakpoints",
            json!({"source": {"path": program}, "breakpoints": [{"line": 9}]}),
        )?;
        session.success("configurationDone", Value::Null)?;
        let stopped = session.event("stopped")?;
        let thread = stopped["threadId"].as_i64().context("relaunch thread")?;
        let variables = stopped_marker(&mut session, thread, 9)?;
        assert!(
            variables.iter().any(|row| row["name"] == "$argc" && row["value"] == "0"),
            "empty real argv: {variables:?}"
        );
        assert_eq!(fs::read_to_string(&receipt)?, "", "empty argv reaches Perl");
        session.success("terminate", Value::Null)?;
        session.event("terminated")?;
    }
    session.finish()
}
