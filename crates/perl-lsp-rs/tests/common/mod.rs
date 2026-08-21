//! Common test utilities for LSP integration tests
//!
//! ## Test Harness Contracts
//!
//! - **Deterministic IO**: Background reader thread with bounded queue prevents blocking
//! - **Request IDs**: Auto-generated when omitted from test requests (avoids collisions)
//! - **Response Matching**: Match by ID for request/response pairing
//! - **Timeouts**: Configurable via env vars, with sensible defaults
//! - **Quiet Drain**: Wait for server to settle after changes before assertions
//! - **Portable Spawn**: PERL_LSP_BIN -> canonical perllsp artifact -> PATH -> cargo run fallback
//!
//! ## Environment Variables
//!
//! - `PERL_LSP_BIN`: Explicit path to perl-lsp binary (useful for custom CARGO_TARGET_DIR)
//! - `LSP_TEST_TIMEOUT_MS`: Default per-request timeout (ms), default 5000
//! - `LSP_TEST_SHORT_MS`: "Short" timeout for optional responses (ms), default 500
//! - `LSP_TEST_ECHO_STDERR`: If set, echo perl-lsp stderr lines in tests
//!
//! ## Key Functions
//!
//! - `send_request()`: Sends request and returns matched response (auto-generates ID if missing)
//! - `drain_until_quiet()`: Waits for server to stop sending messages
//! - `read_notification_method()`: Reads specific notification by method name
//! - `read_response_matching()`: Reads response matching specific ID

#![allow(dead_code)] // Common test utilities - some may not be used by all test files
#![allow(unused_imports)]
// Re-exports needed for backwards compatibility across test files
// Integration tests print diagnostic output for CI troubleshooting; this is
// not the LSP server's stdio transport, so print_stderr doesn't apply the
// way it does to production code.
#![allow(clippy::print_stderr)]

// Re-export test_utils for semantic tests
pub mod test_utils;

// Test reliability and timeout utilities
pub mod test_reliability;
pub mod timeout_scaler;

// Extracted submodules
mod binary_resolution;
mod handshake;
pub mod protocol_io;

// Re-export everything from extracted submodules for backwards compatibility
pub use handshake::{
    await_index_ready, initialize_lsp, initialize_lsp_with_capabilities, shutdown_and_exit,
};
pub use protocol_io::{
    ReadResponseOutcome, drain_until_quiet, read_notification_method, read_notification_timeout,
    read_response, read_response_matching, read_response_matching_i64,
    read_response_matching_outcome, read_response_only_timeout, read_response_timeout, send_raw,
    send_raw_message, send_request_no_wait,
};

use binary_resolution::resolve_perl_lsp_cmds;
use protocol_io::{
    ERR_TEST_TIMEOUT, error_response_for_request, map_send_error, send_message_inner,
};

use perl_tdd_support::must;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};

const PENDING_CAP: usize = 512; // Prevent unbounded growth of pending message queue
use std::io::{self, BufRead, BufReader, BufWriter, Read};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc;
use std::time::Duration;

// Auto-generate unique IDs for requests
static NEXT_ID: AtomicI64 = AtomicI64::new(1000);

// Global mutex to serialize LSP server creation to prevent resource conflicts
static LSP_SERVER_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

fn next_id() -> i64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Get completion items from a response, handling both array and object formats
pub fn completion_items(resp: &serde_json::Value) -> &Vec<serde_json::Value> {
    match resp["result"]["items"].as_array() {
        Some(arr) => arr,
        None => match resp["result"].as_array() {
            Some(arr) => arr,
            None => must(Err::<&Vec<serde_json::Value>, _>(format!(
                "completion result should be array or {{ items: [] }}, got: {resp:?}"
            ))),
        },
    }
}

pub struct LspServer {
    pub process: Mutex<Child>,
    pub(crate) writer: Mutex<BufWriter<ChildStdin>>, // keep stdin pinned and flushed
    rx: Mutex<mpsc::Receiver<Value>>,
    /// Protocol-level failures reported by the stdout reader thread
    /// (unparsable frames); drained by the response readers so callers can
    /// distinguish a broken payload from a slow one.
    err_rx: Mutex<mpsc::Receiver<String>>,
    // Keep threads alive for the lifetime of the server
    _stdout_thread: std::thread::JoinHandle<()>,
    _stderr_thread: std::thread::JoinHandle<()>,
    pending: Mutex<VecDeque<Value>>,
    /// Bounded tail of the server's stderr lines (#11848): the drain thread
    /// records the last lines so a handshake stall can be diagnosed from the
    /// failure output alone, without rerunning with LSP_TEST_ECHO_STDERR.
    /// Shared with the drain thread, hence the Arc; the tuple's second
    /// element is the unterminated final line.
    stderr_tail: std::sync::Arc<Mutex<(VecDeque<String>, String)>>,
    /// Flag to track if shutdown has been initiated (prevents double-shutdown)
    pub(crate) shutdown_initiated: std::sync::atomic::AtomicBool,
}

/// How many stderr lines the tail keeps.
const STDERR_TAIL_LINES: usize = 40;
const STDERR_PARTIAL_MAX_BYTES: usize = 8 * 1024;

fn record_stderr_chunk(tail: &mut (VecDeque<String>, String), chunk: &[u8]) -> Vec<String> {
    let mut completed = Vec::new();
    tail.1.push_str(&String::from_utf8_lossy(chunk));
    while let Some(pos) = tail.1.find('\n') {
        let line = tail.1[..pos].trim_end().to_owned();
        tail.1.drain(..=pos);
        if tail.0.len() >= STDERR_TAIL_LINES {
            tail.0.pop_front();
        }
        completed.push(line.clone());
        tail.0.push_back(line);
    }
    if tail.1.len() > STDERR_PARTIAL_MAX_BYTES {
        let mut start = tail.1.len() - STDERR_PARTIAL_MAX_BYTES;
        while !tail.1.is_char_boundary(start) {
            start += 1;
        }
        tail.1.drain(..start);
    }
    completed
}

impl LspServer {
    /// Check if the server process is still running
    pub fn is_alive(&self) -> bool {
        match self.process.lock().unwrap_or_else(|e| e.into_inner()).try_wait() {
            Ok(status) => status.is_none(),
            Err(_) => false, // If we can't check status, assume not alive
        }
    }

    /// Get mutable access to the stdin writer
    pub fn stdin_writer(&self) -> std::sync::MutexGuard<'_, BufWriter<ChildStdin>> {
        self.writer.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Render the captured stderr tail (#11848). Returns an empty string
    /// when the server produced no stderr, so callers can interpolate it
    /// into failure messages unconditionally.
    pub fn stderr_tail(&self) -> String {
        let tail = self.stderr_tail.lock().unwrap_or_else(|e| e.into_inner());
        if tail.0.is_empty() && tail.1.is_empty() {
            return String::new();
        }
        let mut rendered = String::from("server stderr tail:\n");
        for line in tail.0.iter() {
            rendered.push_str("  | ");
            rendered.push_str(line);
            rendered.push('\n');
        }
        if !tail.1.is_empty() {
            rendered.push_str("  | ");
            rendered.push_str(tail.1.trim_end());
            rendered.push_str("  (unterminated final line)\n");
        }
        rendered
    }
}

pub fn start_lsp_server() -> LspServer {
    // Serialize LSP server creation to prevent resource conflicts during concurrent testing
    let _guard = match LSP_SERVER_MUTEX.get_or_init(|| Mutex::new(())).lock() {
        Ok(g) => g,
        Err(poisoned) => {
            eprintln!("Warning: LSP_SERVER_MUTEX was poisoned, recovering...");
            poisoned.into_inner()
        }
    };

    // Try candidates in order; fall back cleanly on NotFound
    let mut last_err: Option<io::Error> = None;
    let mut process: Child = {
        let mut spawned: Option<Child> = None;
        for mut cmd in resolve_perl_lsp_cmds() {
            match cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
                Ok(child) => {
                    spawned = Some(child);
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    last_err = Some(e);
                    continue;
                }
                Err(e) => {
                    last_err = Some(e);
                    continue;
                }
            }
        }
        spawned.unwrap_or_else(|| {
            eprintln!("╔════════════════════════════════════════════════════════════════════╗");
            eprintln!("║ ERROR: Failed to start perl-lsp server                             ║");
            eprintln!("╠════════════════════════════════════════════════════════════════════╣");
            eprintln!("║ Resolution order tried:                                            ║");
            eprintln!("║  1. PERL_LSP_BIN env var: {:?}", std::env::var("PERL_LSP_BIN").ok());
            eprintln!(
                "║  2. Runtime CARGO_BIN_EXE_perllsp: {:?}",
                std::env::var("CARGO_BIN_EXE_perllsp").ok()
            );
            if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
                let crate_dir = std::path::Path::new(&manifest_dir);
                let workspace_root = crate_dir
                    .ancestors()
                    .find(|p| p.join("Cargo.lock").exists())
                    .unwrap_or(crate_dir);
                let debug_binary = workspace_root.join("target/debug/perllsp");
                let release_binary = workspace_root.join("target/release/perllsp");
                eprintln!(
                    "║  5. Debug binary exists: {} ({})",
                    debug_binary.exists(),
                    debug_binary.display()
                );
                eprintln!(
                    "║  6. Release binary exists: {} ({})",
                    release_binary.exists(),
                    release_binary.display()
                );
            }
            eprintln!("║  5. perllsp in PATH: {:?}", which::which("perllsp").ok());
            eprintln!("║  6. cargo run -p perllsp fallback");
            eprintln!("╠════════════════════════════════════════════════════════════════════╣");
            eprintln!("║ Last error: {:?}", last_err);
            eprintln!("╠════════════════════════════════════════════════════════════════════╣");
            eprintln!("║ HINTS:                                                             ║");
            eprintln!("║  • Run: cargo build -p perllsp --bin perllsp                         ║");
            eprintln!("║  • Or:  cargo test -p perl-lsp-rs    (builds + tests automatically)   ║");
            eprintln!("║  • Set PERL_LSP_BIN=/path/to/perllsp for a custom product binary    ║");
            eprintln!("╚════════════════════════════════════════════════════════════════════╝");
            must(Err::<std::process::Child, _>(format!(
                "Failed to start perl-lsp via any available method: {:?}",
                last_err
            )))
        })
    };

    let stdin = match process.stdin.take() {
        Some(s) => s,
        None => must(Err::<std::process::ChildStdin, _>(
            "child stdin should be available after spawn".to_string(),
        )),
    };

    // -------- stderr drain thread (prevents child from blocking on logs) --------
    let stderr = match process.stderr.take() {
        Some(s) => s,
        None => must(Err::<std::process::ChildStderr, _>(
            "stderr should be piped after spawn".to_string(),
        )),
    };
    let echo = std::env::var_os("LSP_TEST_ECHO_STDERR").is_some();
    // Shared tail the drain thread records into (see `LspServer::stderr_tail`).
    // `partial` holds the bytes of a not-yet-terminated final line: a stalled
    // server's last partial diagnostic is often the most informative byte it
    // produced, and a `read_line` drain would sit blocked on it forever while
    // `stderr_tail` reported "no stderr" (#11853 review). Chunked reads keep
    // the partial visible.
    let stderr_tail = std::sync::Arc::new(Mutex::new((VecDeque::<String>::new(), String::new())));
    let stderr_tail_for_thread = std::sync::Arc::clone(&stderr_tail);
    let _stderr_thread =
        match std::thread::Builder::new().name("lsp-stderr-drain".into()).spawn(move || {
            let mut stderr = stderr;
            let mut chunk = [0u8; 4096];
            loop {
                let n = match stderr.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let mut tail = stderr_tail_for_thread.lock().unwrap_or_else(|e| e.into_inner());
                let completed = record_stderr_chunk(&mut tail, &chunk[..n]);
                if echo {
                    for line in completed {
                        eprintln!("[perl-lsp] {line}");
                    }
                }
            }
        }) {
            Ok(handle) => handle,
            Err(e) => must(Err::<std::thread::JoinHandle<()>, _>(format!(
                "Failed to spawn stderr drain thread: {e}"
            ))),
        };

    // -------- stdout LSP reader thread --------
    let stdout = match process.stdout.take() {
        Some(s) => s,
        None => must(Err::<std::process::ChildStdout, _>(
            "stdout should be piped after spawn".to_string(),
        )),
    };
    let (tx, rx) = mpsc::channel::<Value>();
    let (err_tx, err_rx) = mpsc::channel::<String>();
    let debug_reader = std::env::var_os("LSP_TEST_DEBUG_READER").is_some();
    let _stdout_thread =
        match std::thread::Builder::new().name("lsp-stdout-reader".into()).spawn(move || {
            let mut r = BufReader::new(stdout);
            if debug_reader {
                eprintln!("[reader] Thread started");
            }
            loop {
                // Parse headers
                let mut content_len: Option<usize> = None;
                let mut line = String::new();
                loop {
                    line.clear();
                    match r.read_line(&mut line) {
                        Ok(0) => {
                            if debug_reader {
                                eprintln!("[reader] EOF on stdout");
                            }
                            return; // EOF
                        }
                        Ok(_) => {
                            let l = line.trim_end();
                            if l.is_empty() {
                                break;
                            }
                            // Case-insensitive header matching with flexible colon handling
                            let lower = l.to_ascii_lowercase();
                            if let Some(rest) = lower.strip_prefix("content-length") {
                                let rest = rest.trim_start_matches(':').trim();
                                content_len = rest.parse::<usize>().ok();
                            }
                        }
                        Err(e) => {
                            if debug_reader {
                                eprintln!("[reader] Error reading line: {e}");
                            }
                            return;
                        }
                    }
                }
                let len = match content_len {
                    Some(n) => n,
                    None => {
                        let _ = err_tx
                            .send("frame without a parsable Content-Length header".to_owned());
                        continue;
                    }
                };
                // Read body
                let mut buf = vec![0u8; len];
                if r.read_exact(&mut buf).is_err() {
                    if debug_reader {
                        eprintln!("[reader] Error reading body");
                    }
                    return;
                }
                match serde_json::from_slice::<Value>(&buf) {
                    Ok(val) => {
                        if debug_reader {
                            let id = val.get("id").map(|v| v.to_string()).unwrap_or_default();
                            let method = val.get("method").and_then(|v| v.as_str()).unwrap_or("");
                            eprintln!("[reader] Received message id={id} method={method}");
                        }
                        let _ = tx.send(val);
                    }
                    Err(e) => {
                        let snippet = String::from_utf8_lossy(&buf);
                        let _ = err_tx.send(format!(
                            "invalid JSON in {len}-byte frame: {e}; body: {:.200}",
                            snippet.trim()
                        ));
                    }
                }
            }
        }) {
            Ok(handle) => handle,
            Err(e) => must(Err::<std::thread::JoinHandle<()>, _>(format!(
                "Failed to spawn stdout reader thread: {e}"
            ))),
        };

    let server = LspServer {
        process: Mutex::new(process),
        writer: Mutex::new(BufWriter::new(stdin)),
        rx: Mutex::new(rx),
        err_rx: Mutex::new(err_rx),
        _stdout_thread,
        _stderr_thread,
        pending: Mutex::new(VecDeque::new()),
        stderr_tail,
        shutdown_initiated: std::sync::atomic::AtomicBool::new(false),
    };

    // Brief delay to allow server to fully initialize before returning
    std::thread::sleep(Duration::from_millis(100));

    server
}

#[cfg(test)]
mod stderr_tests {
    use super::{STDERR_PARTIAL_MAX_BYTES, STDERR_TAIL_LINES, record_stderr_chunk};
    use std::collections::VecDeque;

    #[test]
    fn stderr_fragments_reconstruct_across_reads() {
        let mut tail = (VecDeque::new(), String::new());
        assert!(record_stderr_chunk(&mut tail, b"warning: split ").is_empty());
        assert_eq!(
            record_stderr_chunk(&mut tail, b"across reads\nnext"),
            vec!["warning: split across reads"]
        );

        assert_eq!(tail.0.iter().collect::<Vec<_>>(), vec!["warning: split across reads"]);
        assert_eq!(tail.1, "next");
    }

    #[test]
    fn unterminated_stderr_keeps_newest_bounded_bytes() {
        let mut tail = (VecDeque::new(), String::new());
        record_stderr_chunk(
            &mut tail,
            format!("old{}new", "x".repeat(STDERR_PARTIAL_MAX_BYTES)).as_bytes(),
        );

        assert!(tail.1.len() <= STDERR_PARTIAL_MAX_BYTES);
        assert!(tail.1.ends_with("new"));
        assert!(tail.0.len() <= STDERR_TAIL_LINES);
    }

    #[test]
    fn stderr_echo_lines_survive_tail_eviction() {
        let mut tail = (VecDeque::new(), String::new());
        for index in 0..STDERR_TAIL_LINES {
            record_stderr_chunk(&mut tail, format!("line {index}\n").as_bytes());
        }

        let completed = record_stderr_chunk(&mut tail, b"new line\n");

        assert_eq!(completed, vec!["new line"]);
        assert_eq!(tail.0.len(), STDERR_TAIL_LINES);
        assert_eq!(tail.0.front().map(String::as_str), Some("line 1"));
        assert_eq!(tail.0.back().map(String::as_str), Some("new line"));
    }
}

pub fn send_request(server: &LspServer, request: Value) -> Value {
    send_request_with_response_timeout(server, request, default_timeout())
}

pub fn send_request_with_response_timeout(
    server: &LspServer,
    mut request: Value,
    timeout: Duration,
) -> Value {
    // IMPORTANT: Extract/assign ID FIRST, before any early returns.
    // This ensures error responses can include the proper request ID.
    let id = match request.get("id") {
        Some(v) => v.clone(),
        None => {
            let nid = next_id();
            request["id"] = json!(nid);
            json!(nid)
        }
    };

    let body = request.to_string();
    if let Err(e) =
        send_message_inner(&mut *server.writer.lock().unwrap_or_else(|e| e.into_inner()), &body)
    {
        // Handle write errors gracefully with proper JSON-RPC envelope
        // BrokenPipe during teardown is expected; other errors are transport failures
        return map_send_error(Some(id), e, "send_request");
    }

    // Match by ID to avoid confusion with interleaved notifications
    match &id {
        Value::Number(n) if n.as_i64().is_some() => {
            // Safe unwrap: we just checked is_some() in the match guard
            let id_num = match n.as_i64() {
                Some(num) => num,
                None => must(Err::<i64, _>(format!("ID number should be i64: {n:?}"))),
            };
            match read_response_matching_i64(server, id_num, timeout) {
                Some(resp) => resp,
                None => error_response_for_request(
                    Some(id.clone()),
                    ERR_TEST_TIMEOUT,
                    "test harness timeout",
                ),
            }
        }
        v => match read_response_matching(server, v, timeout) {
            Some(resp) => resp,
            None => error_response_for_request(
                Some(id.clone()),
                ERR_TEST_TIMEOUT,
                "test harness timeout",
            ),
        },
    }
}

pub fn send_notification(server: &LspServer, notification: Value) {
    let body = notification.to_string();
    // Ignore write errors during notification sends - BrokenPipe during teardown is expected
    let _ =
        send_message_inner(&mut *server.writer.lock().unwrap_or_else(|e| e.into_inner()), &body);
}

fn default_timeout() -> Duration {
    std::env::var("LSP_TEST_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| {
            // More nuanced adaptive timeout with exponential backoff
            let _base_timeout = Duration::from_secs(5); // Use underscore to suppress unused var warning
            let thread_count = max_concurrent_threads();

            match thread_count {
                0..=2 => Duration::from_secs(8), // Heavily constrained: reduced from 15s to 8s for faster execution
                3..=4 => Duration::from_secs(6), // Moderately constrained: reduced from 10s to 6s
                5..=8 => Duration::from_secs(4), // Lightly constrained: reduced from 7s to 4s
                _ => Duration::from_secs(3),     // Unconstrained: reduced from 5s to 3s
            }
        })
}

/// Short timeout for expected non-responses (malformed requests, etc)
pub fn short_timeout() -> Duration {
    std::env::var("LSP_TEST_SHORT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| {
            // Adaptive short timeout based on thread constraints
            let thread_count = max_concurrent_threads();
            match thread_count {
                0..=2 => Duration::from_millis(500), // Heavily constrained: reduced from 1000ms
                3..=4 => Duration::from_millis(400), // Moderately constrained: reduced from 750ms
                5..=8 => Duration::from_millis(300), // Lightly constrained: reduced from 500ms
                _ => Duration::from_millis(200),     // Unconstrained: reduced from 300ms
            }
        })
}

/// Get the maximum number of concurrent threads to use in tests
/// Respects RUST_TEST_THREADS environment variable and scales down thread counts appropriately
pub fn max_concurrent_threads() -> usize {
    std::env::var("RUST_TEST_THREADS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or_else(|| {
            // Try to detect system thread count, default to 8
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8)
        })
        .max(1) // Ensure at least 1 thread
}

/// Get adaptive timeout based on thread constraints
/// More comprehensive handling of timeout scaling with logarithmic backoff
pub fn adaptive_timeout() -> Duration {
    let base_timeout = default_timeout();
    let thread_count = max_concurrent_threads();

    // Reduced multipliers for faster test execution
    match thread_count {
        0..=2 => base_timeout, // Heavily constrained: reduced from 3x to 1x
        3..=4 => base_timeout, // Moderately constrained: reduced from 2x to 1x
        5..=8 => base_timeout, // Lightly constrained: reduced from 1.5x to 1x
        _ => base_timeout,     // Unconstrained: standard timeout
    }
}

/// Adaptive sleep duration based on thread constraints
/// More sophisticated sleep scaling with exponential strategy
pub fn adaptive_sleep_ms(base_ms: u64) -> Duration {
    let thread_count = max_concurrent_threads();
    let multiplier = match thread_count {
        0..=2 => 1, // Extremely constrained: reduced from 4x to 1x sleep
        3..=4 => 1, // Heavily constrained: reduced from 3x to 1x sleep
        5..=8 => 1, // Moderately constrained: reduced from 2x to 1x sleep
        _ => 1,     // Unconstrained: standard sleep
    };
    Duration::from_millis(base_ms * multiplier)
}

impl Drop for LspServer {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;

        // Check if already exited
        if self
            .process
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .try_wait()
            .ok()
            .flatten()
            .is_some()
        {
            return;
        }

        // Check if shutdown was already initiated by explicit call
        if self.shutdown_initiated.swap(true, Ordering::SeqCst) {
            // Already initiated, just wait for exit then force-kill if needed
            for _ in 0..50 {
                if self
                    .process
                    .get_mut()
                    .unwrap_or_else(|e| e.into_inner())
                    .try_wait()
                    .ok()
                    .flatten()
                    .is_some()
                {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = self.process.get_mut().unwrap_or_else(|e| e.into_inner()).kill();
            let _ = self.process.get_mut().unwrap_or_else(|e| e.into_inner()).wait();
            return;
        }

        // Best-effort graceful shutdown (never panic in Drop)
        // 1. Try to send shutdown request
        let shutdown_body = r#"{"jsonrpc":"2.0","id":999999,"method":"shutdown","params":{}}"#;
        let _ = send_message_inner(
            &mut *self.writer.get_mut().unwrap_or_else(|e| e.into_inner()),
            shutdown_body,
        );

        // 2. Try to send exit notification
        let exit_body = r#"{"jsonrpc":"2.0","method":"exit"}"#;
        let _ = send_message_inner(
            &mut *self.writer.get_mut().unwrap_or_else(|e| e.into_inner()),
            exit_body,
        );

        // 3. Wait briefly for graceful exit (max 500ms)
        for _ in 0..50 {
            if self
                .process
                .get_mut()
                .unwrap_or_else(|e| e.into_inner())
                .try_wait()
                .ok()
                .flatten()
                .is_some()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        // 4. Fall back to hard kill if graceful shutdown didn't work
        let _ = self.process.get_mut().unwrap_or_else(|e| e.into_inner()).kill();
        let _ = self.process.get_mut().unwrap_or_else(|e| e.into_inner()).wait();
    }
}
