use super::server_request_script::ServerRequestScript;
use super::{ObservedServerRequest, ScriptedServerRequest, write_shared_message};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static NEXT_ID: AtomicU64 = AtomicU64::new(100);

pub struct ScriptedClient {
    child: Mutex<Child>,
    stdin: Arc<Mutex<ChildStdin>>,
    events: Arc<Mutex<VecDeque<Value>>>,
    responses: Arc<Mutex<VecDeque<Value>>>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
    script: ServerRequestScript,
    _stdout_thread: std::thread::JoinHandle<()>,
    _stderr_thread: std::thread::JoinHandle<()>,
}

impl ScriptedClient {
    pub fn spawn(
        binary_path: &str,
        root_uri: &str,
        script: Vec<ScriptedServerRequest>,
        timeout: Duration,
    ) -> Result<Self> {
        let mut child = Command::new(binary_path)
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!("failed to spawn scripted LSP fixture from {binary_path:?}")
            })?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("fixture stdin unavailable"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("fixture stdout unavailable"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("fixture stderr unavailable"))?;

        let stdin = Arc::new(Mutex::new(stdin));
        let events = Arc::new(Mutex::new(VecDeque::new()));
        let responses = Arc::new(Mutex::new(VecDeque::new()));
        let stderr_lines = Arc::new(Mutex::new(Vec::new()));
        let (script, observer) = ServerRequestScript::new(stdin.clone(), script)?;

        let event_queue = events.clone();
        let response_queue = responses.clone();
        let _stdout_thread = std::thread::Builder::new()
            .name("ux-scripted-client-stdout".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                while let Ok(message) = read_message(&mut reader) {
                    let has_id = message.get("id").is_some_and(|id| !id.is_null());
                    let is_response = has_id
                        && (message.get("result").is_some() || message.get("error").is_some());
                    if is_response {
                        if let Ok(mut queue) = response_queue.lock() {
                            queue.push_back(message);
                        }
                    } else {
                        observer.observe(&message);
                        if let Ok(mut queue) = event_queue.lock() {
                            queue.push_back(message);
                        }
                    }
                }
            })
            .context("failed to spawn scripted client stdout reader")?;

        let captured_stderr = stderr_lines.clone();
        let _stderr_thread = std::thread::Builder::new()
            .name("ux-scripted-client-stderr".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if let Ok(mut lines) = captured_stderr.lock() {
                        lines.push(line);
                    }
                }
            })
            .context("failed to spawn scripted client stderr reader")?;

        let client = Self {
            child: Mutex::new(child),
            stdin,
            events,
            responses,
            stderr_lines,
            script,
            _stdout_thread,
            _stderr_thread,
        };
        client.initialize(root_uri, timeout)?;
        Ok(client)
    }

    fn initialize(&self, root_uri: &str, timeout: Duration) -> Result<()> {
        let response = self.request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {}
            }),
            timeout,
        )?;
        if let Some(error) = response.get("error") {
            return Err(anyhow!("fixture initialize failed: {error}"));
        }
        self.notify("initialized", json!({}))
    }

    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        write_shared_message(
            &self.stdin,
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )?;
        self.wait_for_response(id, timeout)
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<()> {
        write_shared_message(
            &self.stdin,
            &json!({"jsonrpc": "2.0", "method": method, "params": params}),
        )
    }

    pub fn wait_for_script(&self, timeout: Duration) -> Result<Vec<ObservedServerRequest>> {
        self.script.wait(timeout)
    }

    pub fn assert_no_unscripted_requests(&self) -> Result<()> {
        self.script.assert_no_unscripted()
    }

    pub fn peek_events(&self) -> Vec<Value> {
        self.events.lock().unwrap_or_else(|error| error.into_inner()).iter().cloned().collect()
    }

    pub fn stderr_lines(&self) -> Vec<String> {
        self.stderr_lines.lock().unwrap_or_else(|error| error.into_inner()).clone()
    }

    fn wait_for_response(&self, id: u64, timeout: Duration) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let mut responses =
                    self.responses.lock().unwrap_or_else(|error| error.into_inner());
                if let Some(index) =
                    responses.iter().position(|value| value.get("id") == Some(&json!(id)))
                    && let Some(response) = responses.remove(index)
                {
                    return Ok(response);
                }
            }
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "timed out after {}ms waiting for response id={id}; stderr={:#?}",
                    timeout.as_millis(),
                    self.stderr_lines()
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for ScriptedClient {
    fn drop(&mut self) {
        let _ = self.request("shutdown", json!({}), Duration::from_millis(500));
        let _ = self.notify("exit", json!({}));
        for _ in 0..50 {
            if let Ok(mut child) = self.child.lock()
                && child.try_wait().ok().flatten().is_some()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn read_message(reader: &mut impl BufRead) -> Result<Value> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(anyhow!("EOF while reading LSP headers"));
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length") {
            content_length = value.trim_start_matches(':').trim().parse::<usize>().ok();
        }
    }
    let length = content_length.ok_or_else(|| anyhow!("LSP message omitted Content-Length"))?;
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).context("failed to decode LSP JSON body")
}
