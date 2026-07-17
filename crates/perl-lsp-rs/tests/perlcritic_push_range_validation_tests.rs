//! Deterministic push-diagnostics proof for external Perl::Critic range filtering.
//!
//! The server owns an asynchronous outbound writer. Dropping `LspServer` closes
//! its sender and joins that writer, so the captured buffer is complete without
//! sleeps or polling before assertions run.

#![cfg(all(not(target_arch = "wasm32"), feature = "expose_lsp_test_api"))]
#![allow(clippy::expect_used)]

use parking_lot::Mutex;
use perl_lsp::LspServer;
use perl_lsp_rs_core::config::CriticEngine;
use perl_subprocess_runtime::mock::{MockResponse, MockSubprocessRuntime};
use serde_json::json;
use std::io::Write;
use std::sync::Arc;

struct CapturingWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl Write for CapturingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.buffer.lock().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn server_with_capture() -> (LspServer, Arc<Mutex<Vec<u8>>>) {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let writer = CapturingWriter { buffer: Arc::clone(&buffer) };
    let output: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(Box::new(writer)));
    (LspServer::with_output(output), buffer)
}

#[test]
fn push_perlcritic_keeps_valid_range_and_drops_malformed_ranges() {
    let (server, buffer) = server_with_capture();
    server.test_configure_perlcritic(true, 3, None);
    server.test_configure_critic_engine(CriticEngine::Legacy);

    let runtime = Arc::new(MockSubprocessRuntime::new());
    runtime.add_response(MockResponse::success(
        b"test.pl:1:1:3:TestingAndDebugging::RequireUseStrict:valid range\n\
          test.pl:99:1:3:TestingAndDebugging::RequireUseStrict:bad line range\n\
          test.pl:1:99:3:TestingAndDebugging::RequireUseStrict:bad column range\n"
            .to_vec(),
    ));
    server.test_install_mock_critic_runtime(runtime);
    server.test_bypass_perlcritic_command_check();

    #[cfg(windows)]
    let uri = "file:///C:/tmp/critic_range_push.pl";
    #[cfg(not(windows))]
    let uri = "file:///tmp/critic_range_push.pl";

    server
        .test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "print 'hello';\n"
            }
        })))
        .expect("didOpen should succeed");

    // `Drop` closes the outbound sender and joins the writer thread.
    drop(server);

    let output = String::from_utf8(buffer.lock().clone()).expect("captured output is UTF-8");
    assert!(
        output.contains("valid range"),
        "valid external critic range must publish: {output:?}"
    );
    assert!(
        !output.contains("bad line range") && !output.contains("bad column range"),
        "malformed external critic ranges must not publish: {output:?}"
    );
}
