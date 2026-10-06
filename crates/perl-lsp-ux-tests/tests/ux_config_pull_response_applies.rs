//! Real-stdio regression for the configuration pull channel (#17338).
//!
//! The server pulls `workspace/configuration` when the client advertises
//! `workspace.configuration`. A standard JSON-RPC client answers with
//! `{"jsonrpc":"2.0","id":N,"result":[...]}` — an error-less success. The
//! transport previously stamped every such rewrite with `"error": null` and
//! the runtime classified any params carrying an `error` key as a failure,
//! so pulled settings could never apply: per-folder include paths were
//! impossible in any standard client and PL701 "module not found" rows never
//! cleared after a re-pull.
//!
//! This test drives the real `perl-lsp` binary over stdio with the issue's
//! own driver: a document importing a module that is only reachable through
//! a pulled `includePaths` entry. The discriminating outcome is a publish in
//! which PL701 no longer appears — impossible unless the standard success
//! response was applied.

use anyhow::{Result, anyhow, bail};
use perl_lsp_ux_tests::{binary_available, resolve_binary};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(30);

/// One framed message from the server's stdout.
struct ServerMessage {
    message: Value,
}

struct StdioSession {
    child: Child,
    stdin: std::process::ChildStdin,
    inbox: std::sync::mpsc::Receiver<ServerMessage>,
    next_id: i64,
}

impl Drop for StdioSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn write_frame<W: Write>(writer: &mut W, message: &Value) -> Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

/// Reader-thread body: forward framed messages until end of stream.
fn read_frames<R: BufRead + Send + 'static>(
    mut reader: R,
    sender: std::sync::mpsc::Sender<ServerMessage>,
) {
    loop {
        let mut content_length: Option<usize> = None;
        let mut end_of_stream = false;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    end_of_stream = true;
                    break;
                }
                Ok(_) => {
                    let line = line.trim_end();
                    if line.is_empty() {
                        break;
                    }
                    if let Some(value) = line.strip_prefix("Content-Length:") {
                        content_length = value.trim().parse::<usize>().ok();
                    }
                }
                Err(_) => {
                    end_of_stream = true;
                    break;
                }
            }
        }
        if end_of_stream {
            return;
        }
        let body = match content_length {
            Some(len) => {
                let mut body = vec![0_u8; len];
                if reader.read_exact(&mut body).is_err() {
                    return;
                }
                body
            }
            None => return,
        };
        match serde_json::from_slice::<Value>(&body) {
            Ok(message) => {
                if sender.send(ServerMessage { message }).is_err() {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

impl StdioSession {
    fn spawn(binary: &str) -> Result<Self> {
        let mut child = Command::new(binary)
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        let (sender, inbox) = std::sync::mpsc::channel();
        std::thread::spawn(move || read_frames(BufReader::new(stdout), sender));
        Ok(Self { child, stdin, inbox, next_id: 1 })
    }

    /// Next message, or `None` on timeout / stream end.
    fn next_message(&mut self, deadline: Instant) -> Result<Option<Value>> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match self.inbox.recv_timeout(remaining) {
            Ok(item) => Ok(Some(item.message)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Ok(None),
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        write_frame(&mut self.stdin, &json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<i64> {
        let id = self.next_id;
        self.next_id += 1;
        write_frame(
            &mut self.stdin,
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )?;
        Ok(id)
    }

    fn respond(&mut self, id: Value, result: Value) -> Result<()> {
        write_frame(&mut self.stdin, &json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }
}

/// The issue's driver: `use LocalMod;` where `LocalMod.pm` lives only in
/// `site_lib/`, delivered by the server's own configuration pull.
#[test]
fn standard_configuration_pull_success_response_applies() -> Result<()> {
    if !binary_available() {
        // Quiet skip, matching the ux-suite skip posture: the suite-wide
        // strict-binary contract (REQUIRE_BINARY_ENV) surfaces missing
        // binaries where they are required.
        return Ok(());
    }
    let binary = resolve_binary()?;
    let temp = tempfile::tempdir()?;
    let site_lib = temp.path().join("site_lib");
    std::fs::create_dir_all(&site_lib)?;
    std::fs::write(temp.path().join("app.pl"), "use LocalMod;\n")?;
    std::fs::write(site_lib.join("LocalMod.pm"), "package LocalMod;\nsub imported {}\n1;\n")?;

    let root_uri = url::Url::from_directory_path(temp.path())
        .map_err(|()| anyhow!("temp dir to URI"))?
        .to_string();
    // The client's own spelling for the opened document (uppercase drive
    // letter on Windows).
    let doc_uri = url::Url::from_file_path(temp.path().join("app.pl"))
        .map_err(|()| anyhow!("doc path to URI"))?
        .to_string();

    let mut session = StdioSession::spawn(&binary)?;
    let initialize_id = session.request(
        "initialize",
        json!({
            "processId": null,
            "rootUri": root_uri,
            "capabilities": {
                "workspace": { "configuration": true }
            }
        }),
    )?;

    let deadline = Instant::now() + WAIT;
    let mut initialized = false;
    let mut answered_pull = false;
    let mut saw_pull = false;
    let mut publish_count = 0_usize;
    let mut publish_states: Vec<String> = Vec::new();
    let mut cleared_seen = false;

    loop {
        let Some(message) = session.next_message(deadline)? else {
            bail!(
                "server stream ended or timed out (initialized={initialized} saw_pull={saw_pull} \
                 answered_pull={answered_pull} publishes={publish_states:?})"
            );
        };
        let has_response_body = message.get("result").is_some() || message.get("error").is_some();
        if !initialized && has_response_body && message.get("id") == Some(&json!(initialize_id)) {
            initialized = true;
            session.notify("initialized", json!({}))?;
            session.notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": doc_uri,
                        "languageId": "perl",
                        "version": 1,
                        "text": "use LocalMod;\n"
                    }
                }),
            )?;
            continue;
        }
        if !initialized {
            // Answer any server request that arrives during the handshake so
            // it cannot stall; the configuration pull is answered below.
            if let Some(id) = message
                .get("id")
                .filter(|id| !id.is_null())
                .filter(|_| message.get("method").is_some())
            {
                session.respond(id.clone(), Value::Null)?;
            }
            continue;
        }
        match message.get("method").and_then(Value::as_str) {
            Some("workspace/configuration") => {
                saw_pull = true;
                let id = message.get("id").cloned().ok_or_else(|| anyhow!("pull without id"))?;
                let item_count = message
                    .pointer("/params/items")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0);
                // The standard client answer: one value per item, `result`
                // member, and — critically — no `error` member (#17338).
                let result: Vec<Value> =
                    vec![json!({"workspace": {"includePaths": ["site_lib"]}}); item_count];
                session.respond(id, json!(result))?;
                answered_pull = true;
            }
            Some(method) if method.starts_with("workspace/") && message.get("id").is_some() => {
                // Other server requests (registrations, progress creation):
                // acknowledge so they cannot stall the session.
                session.respond(message.get("id").cloned().unwrap_or(Value::Null), Value::Null)?;
            }
            Some("textDocument/publishDiagnostics")
                if message.pointer("/params/uri").and_then(Value::as_str)
                    == Some(doc_uri.as_str()) =>
            {
                publish_count += 1;
                let has_pl701 = message.to_string().contains("PL701");
                publish_states.push(format!("publish#{publish_count}:pl701={}", has_pl701 as u8));
                if answered_pull && !has_pl701 {
                    cleared_seen = true;
                    break;
                }
            }
            _ => {}
        }
        if Instant::now() >= deadline {
            break;
        }
    }

    assert!(initialized, "server must complete initialization");
    assert!(saw_pull, "server must send workspace/configuration when advertised");
    assert!(answered_pull, "test must have answered the pull with a standard success response");
    assert!(
        !publish_states.is_empty(),
        "at least one publish for {doc_uri} must have been observed"
    );
    assert!(
        cleared_seen,
        "after applying the standard success response the publish must clear PL701 \
         (module resolved through the pulled includePaths); publishes={publish_states:?}"
    );
    Ok(())
}
