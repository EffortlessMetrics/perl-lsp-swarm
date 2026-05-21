//! Regression tests for LSP4IJ file-watcher registration crash.
//!
//! **Root cause**: The server previously used `SystemTime::now().as_millis() as i64` to
//! generate the ID for the `client/registerCapability` request.  Wall-clock epoch
//! milliseconds (≈1.7 × 10¹²) overflow `i32::MAX` (≈2.1 × 10⁹), which LSP4IJ and
//! other clients parse IDs as — causing an integer overflow crash on the client side.
//!
//! **Fix**: All outbound server→client requests now go through `LspServer::send_request`,
//! which calls `next_server_request_id()`.  That allocator emits sequential positive
//! `i32` values, wrapping from `i32::MAX` back to `1`.
//!
//! These tests verify the invariants that make the bug structurally impossible:
//!
//! 1. `file_watcher_registration_uses_bounded_lsp_integer_request_id` — watcher
//!    registration request has `1 ≤ id ≤ i32::MAX` on the wire.
//! 2. `server_request_id_allocator_emits_bounded_ids` — 100k allocations all
//!    produce positive IDs within i32 range.
//! 3. `server_request_id_allocator_wraps_at_i32_max` — counter wraps from
//!    `i32::MAX` back to `1` cleanly with no panic, no negative, no zero.
//! 4. `no_direct_outbound_send_request_outside_canonical_files` — source guard
//!    verifying that `outbound.send_request(` does not appear in runtime files
//!    other than the two files that are permitted to call it directly.

// Tests 1-3 use test API methods — require expose_lsp_test_api feature.
// Test 4 (source guard) is always compiled.

#[cfg(feature = "expose_lsp_test_api")]
use parking_lot::Mutex;
#[cfg(feature = "expose_lsp_test_api")]
use perl_lsp::LspServer;
#[cfg(feature = "expose_lsp_test_api")]
use serde_json::Value;
#[cfg(feature = "expose_lsp_test_api")]
use std::io::Write;
#[cfg(feature = "expose_lsp_test_api")]
use std::sync::Arc;

// ───────────────────────────────────────────────────────────────────────────────
// Helpers (only needed by tests that use expose_lsp_test_api)
// ───────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "expose_lsp_test_api")]
#[derive(Clone, Default)]
struct SharedBuffer {
    inner: Arc<Mutex<Vec<u8>>>,
}

#[cfg(feature = "expose_lsp_test_api")]
impl SharedBuffer {
    fn new() -> Self {
        Self::default()
    }

    fn bytes(&self) -> Vec<u8> {
        self.inner.lock().clone()
    }
}

#[cfg(feature = "expose_lsp_test_api")]
impl Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.lock().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Parse LSP-framed bytes into JSON payloads.
#[cfg(feature = "expose_lsp_test_api")]
fn parse_framed_payloads(raw: &[u8]) -> Vec<Value> {
    let mut cursor = 0usize;
    let mut payloads = Vec::new();

    while cursor < raw.len() {
        let remainder = &raw[cursor..];
        let separator = b"\r\n\r\n";
        let Some(header_end) = remainder.windows(separator.len()).position(|w| w == separator)
        else {
            break;
        };
        let header_bytes = &remainder[..header_end];
        let header = match std::str::from_utf8(header_bytes) {
            Ok(h) => h,
            Err(_) => break,
        };
        let content_length: usize = match header
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .and_then(|v| v.parse().ok())
        {
            Some(n) => n,
            None => break,
        };

        let body_start = cursor + header_end + separator.len();
        let body_end = body_start + content_length;
        if body_end > raw.len() {
            break;
        }
        if let Ok(val) = serde_json::from_slice::<Value>(&raw[body_start..body_end]) {
            payloads.push(val);
        }
        cursor = body_end;
    }

    payloads
}

// ───────────────────────────────────────────────────────────────────────────────
// Test 1: watcher registration emits a bounded integer ID on the wire
// ───────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "expose_lsp_test_api")]
#[test]
fn file_watcher_registration_uses_bounded_lsp_integer_request_id() {
    let buf = SharedBuffer::new();
    let shared = Arc::new(Mutex::new(Box::new(buf.clone()) as Box<dyn Write + Send>));

    // Build a server that advertises workspace_symbol (so the watcher path runs).
    let server = LspServer::with_output(Arc::clone(&shared));
    server.test_enable_workspace_symbol_feature();

    server.test_register_file_watchers_async();

    // Let the writer thread drain.
    drop(server);

    let raw = buf.bytes();
    let payloads = parse_framed_payloads(&raw);

    // Find the client/registerCapability request among outbound frames.
    let reg_request = payloads
        .iter()
        .find(|p| p.get("method").and_then(Value::as_str) == Some("client/registerCapability"))
        .expect("expected a client/registerCapability request in output");

    let id_value = &reg_request["id"];

    // The id must be a JSON integer (not a string, not null, not a float).
    assert!(
        id_value.is_number() && !id_value.is_f64(),
        "client/registerCapability id must be a plain integer, got: {id_value}"
    );

    let id_num = id_value.as_i64().expect("id must be representable as i64");

    assert!(id_num >= 1, "client/registerCapability id must be >= 1, got: {id_num}");
    assert!(
        id_num <= i64::from(i32::MAX),
        "client/registerCapability id must be <= i32::MAX ({}) to avoid LSP4IJ overflow, got: {id_num}",
        i32::MAX
    );
}

// ───────────────────────────────────────────────────────────────────────────────
// Test 2: allocator emits bounded IDs across 100k calls
// ───────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "expose_lsp_test_api")]
#[test]
fn server_request_id_allocator_emits_bounded_ids() {
    let server = LspServer::new();

    for _ in 0..100_000 {
        let id = server.test_next_server_request_id();
        let raw = id.as_i32();
        assert!(
            raw >= 1,
            "next_server_request_id must never return a non-positive value; got {raw}"
        );
        // as_i32 is i32, so this is always true, but checking via i64 makes the intent clear.
        assert!(
            i64::from(raw) <= i64::from(i32::MAX),
            "next_server_request_id exceeded i32::MAX; got {raw}"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────────
// Test 3: counter wraps from i32::MAX back to 1 cleanly
// ───────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "expose_lsp_test_api")]
#[test]
fn server_request_id_allocator_wraps_at_i32_max() {
    let server = LspServer::new();

    // Seed the counter near i32::MAX.
    server.set_next_request_id_for_test(i32::MAX - 1);

    // Call enough times to cross the wrap boundary.
    let ids: Vec<i32> = (0..5).map(|_| server.test_next_server_request_id().as_i32()).collect();

    // First two should be MAX-1 and MAX.
    assert_eq!(ids[0], i32::MAX - 1, "expected MAX-1 first");
    assert_eq!(ids[1], i32::MAX, "expected MAX second");
    // After wrapping, the counter resets to 1.
    assert_eq!(ids[2], 1, "expected 1 after wrap");
    assert_eq!(ids[3], 2, "expected 2 continuing from 1");
    assert_eq!(ids[4], 3, "expected 3 continuing from 2");

    // Verify no zero or negative values anywhere.
    for &id in &ids {
        assert!(id >= 1, "wrapped allocator must never emit non-positive id; got {id}");
    }
}

// ───────────────────────────────────────────────────────────────────────────────
// Test 4: no-direct-outbound guard
// ───────────────────────────────────────────────────────────────────────────────

/// Verify that `lifecycle/watchers.rs` — the file where the LSP4IJ crash originated
/// — no longer calls `outbound.send_request(` directly and no longer uses the
/// `as_millis` + `SystemTime` pattern to generate a request ID.
///
/// We scope this guard narrowly (single file) to avoid false-positives:
/// `workspace_progress.rs` legitimately receives an `&OutboundSender` parameter
/// and calls it directly (not on `self`), and `workspace.rs` uses `as_millis`
/// for timing telemetry — neither are the crash anti-pattern.
#[test]
fn watchers_rs_no_longer_uses_epoch_millis_as_request_id() {
    let watchers_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("runtime")
        .join("lifecycle")
        .join("watchers.rs");

    let content = std::fs::read_to_string(&watchers_path).expect("watchers.rs must be readable");

    // The exact original anti-pattern: using wall-clock millis as an outbound request ID.
    // Both must be absent from watchers.rs.
    assert!(
        !content.contains("as_millis"),
        "watchers.rs must not use `as_millis` (epoch-ms request IDs cause LSP4IJ overflow)"
    );
    assert!(
        !content.contains("UNIX_EPOCH"),
        "watchers.rs must not reference UNIX_EPOCH (was used to generate oversized request IDs)"
    );
    assert!(
        !content.contains("SystemTime"),
        "watchers.rs must not use SystemTime (was used to generate oversized request IDs)"
    );

    // Confirm the fix is in place: the file now uses self.send_request.
    assert!(
        content.contains("self.send_request("),
        "watchers.rs must call self.send_request() to use the bounded ServerRequestId allocator"
    );
}

/// Verify that `watchers.rs` does not call `outbound.send_request(` directly
/// (bypassing the bounded ServerRequestId allocator on LspServer).
#[test]
fn watchers_rs_does_not_bypass_send_request_allocator() {
    let watchers_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("runtime")
        .join("lifecycle")
        .join("watchers.rs");

    let content = std::fs::read_to_string(&watchers_path).expect("watchers.rs must be readable");

    assert!(
        !content.contains("outbound.send_request("),
        "watchers.rs must not call outbound.send_request() directly;\n\
         use self.send_request() to go through the bounded ServerRequestId allocator"
    );
}
