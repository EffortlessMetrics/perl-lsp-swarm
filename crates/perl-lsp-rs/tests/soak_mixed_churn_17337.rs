//! Sustained mixed open/change/query churn must not terminate the server (#17337).
//!
//! The #17337 soak drove `didOpen` → `didChange` → `completion` → `hover` →
//! `references` → `didClose` cycles across a 10-file workspace and observed the
//! stdio process die mid-session with exit code 1 and only the startup banner
//! on stderr. The exit-path audit behind the diagnostic repair found exactly
//! one mid-session `process::exit` in the binary (`handle_exit_dispatch`, an
//! `exit` notification the soak never sends) and no other in-binary silent
//! route, so the deaths were not attributable to any instrumented path on
//! current main — but a longevity regression guard is still owed.
//!
//! This test drives the same mixed cycle through the production serving stack
//! (`serve_async` ingress → scheduler queues → handlers → outbound writer
//! thread) for a bounded number of cycles. Every query must settle with a
//! response frame, and the loop must end only on input EOF. A mid-session
//! `process::exit` reachable from this mix kills the test process itself; a
//! wedge, a silent teardown, or a dropped response fails the per-request
//! deadline asserts below.

use parking_lot::Mutex;
use perl_lsp::LspServer;
use serde_json::{Value, json};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

/// Number of full open/change/query/close cycles per document round-robin.
///
/// 100 cycles × 6 messages mirrors the #17337 message mix (300 settled
/// requests) while staying well inside a normal unit-test budget.
const CHURN_CYCLES: usize = 100;
const WORKSPACE_DOCS: usize = 10;
/// Client request ids start far above the server→client request id counter
/// (`ServerRequestId` allocates from 1), so response-marker scans cannot
/// collide with server-issued requests.
const FIRST_REQUEST_ID: i64 = 1_000_000;
const PER_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CHURN_DEADLINE: Duration = Duration::from_mins(4);
const EOF_DRAIN_TIMEOUT: Duration = Duration::from_mins(1);

/// Append-only capture of everything the outbound writer thread emits.
#[derive(Clone)]
struct CaptureSink(Arc<Mutex<Vec<u8>>>);

impl Write for CaptureSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn source(index: usize, version_tag: &str) -> String {
    let peer = (index + 1) % WORKSPACE_DOCS;
    format!(
        "package App::Module{index};\n\
         use strict; use warnings;\n\
         our $VERSION = '{version_tag}';\n\
         sub new {{ my ($class, %a) = @_; bless {{ name => $a{{name}} // \"m{index}\" }}, $class }}\n\
         sub add_item_{index} {{ my ($self, $x) = @_; push @{{ $self->{{items}} }}, $x; \
         scalar @{{ $self->{{items}} }} }}\n\
         sub helper_{index} {{ my ($self, $x) = @_; return $x + 1; }}\n\
         sub describe_{index} {{ my ($self) = @_; my $p = App::Module{peer}->new(); \
         $p->add_item_{peer}(\"x\"); }}\n\
         1;\n"
    )
}

/// Byte offset → LSP line/character, matching the #17337 driver's `pos`.
///
/// A missing needle degrades to the document start rather than panicking:
/// this test asserts that queries *settle*, not that a specific result is
/// returned, and the needles come from the same fixed template as the text.
fn position(text: &str, needle: &str) -> Value {
    let idx = text.find(needle).unwrap_or(0);
    let line = text[..idx].matches('\n').count() as u32;
    let character = (idx - (text[..idx].rfind('\n').map_or(0, |p| p + 1))) as u32;
    json!({"line": line, "character": character})
}

/// Wait until `marker` appears in the captured outbound bytes.
///
/// The marker embeds the trailing comma of the response envelope, so a
/// 7-digit client id can never prefix-match a longer number.
async fn wait_for_response(
    captured: &Arc<Mutex<Vec<u8>>>,
    marker: String,
    what: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = tokio::time::Instant::now() + PER_REQUEST_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        {
            let buf = captured.lock();
            if buf.windows(marker.len()).any(|window| window == marker.as_bytes()) {
                return Ok(());
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    Err(format!(
        "request never settled within {PER_REQUEST_TIMEOUT:?}: {what} \
         (marker {marker:?} absent from outbound stream) — ingress stalled, \
         tore down silently, or exited mid-session"
    )
    .into())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sustained_mixed_churn_keeps_serving_until_eof() -> Result<(), Box<dyn std::error::Error>> {
    let captured = Arc::new(Mutex::new(Vec::<u8>::new()));
    let sink =
        Arc::new(Mutex::new(Box::new(CaptureSink(Arc::clone(&captured))) as Box<dyn Write + Send>));
    let server = Arc::new(LspServer::with_output(sink));

    let (tx, rx) = tokio::sync::mpsc::channel(64);
    let serving_task = tokio::spawn(Arc::clone(&server).serve_async(rx));

    let mut next_id = FIRST_REQUEST_ID;
    let send = |tx: &tokio::sync::mpsc::Sender<perl_lsp::JsonRpcRequest>,
                id: Option<i64>,
                method: &str,
                params: Value| {
        let request = perl_lsp::JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: id.map(perl_lsp::JsonRpcId::Integer),
            method: method.to_string(),
            params: Some(params),
        };
        tx.try_send(request).map_err(|error| format!("ingress channel refused {method}: {error}"))
    };

    // Lifecycle: initialize → initialized, exactly like the #17337 driver.
    send(
        &tx,
        Some(next_id),
        "initialize",
        json!({"processId": 0, "rootUri": "file:///workspace", "capabilities": {}}),
    )?;
    let initialize_marker = format!("\"id\":{next_id},");
    next_id += 1;
    wait_for_response(&captured, initialize_marker, "initialize").await?;
    send(&tx, None, "initialized", json!({}))?;

    // Mixed churn: the exact #17337 cycle over 10 virtual documents.
    let churn = async {
        for cycle in 0..CHURN_CYCLES {
            let index = cycle % WORKSPACE_DOCS;
            let uri = format!("file:///workspace/Module{index}.pl");
            let text = source(index, &format!("0.1.{cycle}"));

            send(
                &tx,
                None,
                "textDocument/didOpen",
                json!({"textDocument": {"uri": uri, "languageId": "perl",
                                         "version": (cycle + 1) as i64, "text": text}}),
            )?;
            send(
                &tx,
                None,
                "textDocument/didChange",
                json!({"textDocument": {"uri": uri, "version": (cycle + 2) as i64},
                        "contentChanges": [{"text": text}]}),
            )?;

            let completion_position = position(&text, "add_item_");
            let hover_position = position(&text, "helper_");
            for (method, params, what) in [
                (
                    "textDocument/completion",
                    json!({"textDocument": {"uri": uri}, "position": completion_position}),
                    "completion",
                ),
                (
                    "textDocument/hover",
                    json!({"textDocument": {"uri": uri}, "position": hover_position}),
                    "hover",
                ),
                (
                    "textDocument/references",
                    json!({"textDocument": {"uri": uri}, "position": completion_position,
                        "context": {"includeDeclaration": true}}),
                    "references",
                ),
            ] {
                let id = next_id;
                next_id += 1;
                send(&tx, Some(id), method, params)?;
                wait_for_response(
                    &captured,
                    format!("\"id\":{id},"),
                    &format!("cycle {cycle} {what}"),
                )
                .await?;
            }

            send(&tx, None, "textDocument/didClose", json!({"textDocument": {"uri": uri}}))?;
        }
        Ok::<(), Box<dyn std::error::Error>>(())
    };

    tokio::time::timeout(CHURN_DEADLINE, churn).await.map_err(|_| {
        format!(
            "mixed churn did not finish {CHURN_CYCLES} cycles within {CHURN_DEADLINE:?} — \
                 the serving stack wedged or stopped responding mid-session"
        )
    })??;

    if !server.is_initialized() {
        return Err("server never reached the initialized state".into());
    }

    // Post-close ingress probe. The final `didClose` above is only queued —
    // every response assertion happens before it — and a failed scheduler
    // hand-off there ends `serve_async`'s receive loop early while the task
    // still returns normally after worker shutdown, so the successful join
    // after `drop(tx)` below cannot distinguish that teardown from input EOF.
    // Reopen the final document and prove ingress still settles a request
    // before the EOF teardown is exercised.
    let probe_index = (CHURN_CYCLES - 1) % WORKSPACE_DOCS;
    let probe_uri = format!("file:///workspace/Module{probe_index}.pl");
    let probe_text = source(probe_index, "0.1.probe");
    let probe_position = position(&probe_text, "helper_");
    send(
        &tx,
        None,
        "textDocument/didOpen",
        json!({"textDocument": {"uri": probe_uri.clone(), "languageId": "perl",
                                 "version": (CHURN_CYCLES + 2) as i64, "text": probe_text}}),
    )?;
    let probe_id = next_id;
    send(
        &tx,
        Some(probe_id),
        "textDocument/hover",
        json!({"textDocument": {"uri": probe_uri}, "position": probe_position}),
    )?;
    wait_for_response(&captured, format!("\"id\":{probe_id},"), "post-close ingress probe").await?;

    // EOF teardown: dropping ingress must end `serve_async` cooperatively —
    // a hang here would be the wedge half of the #17337 wedge-then-exit shape.
    drop(tx);
    let served_until_eof =
        tokio::time::timeout(EOF_DRAIN_TIMEOUT, serving_task).await.map_err(|_| {
            format!("serve_async did not finish within {EOF_DRAIN_TIMEOUT:?} after ingress EOF")
        })?;
    served_until_eof.map_err(|join_error| {
        format!("serve_async task failed during EOF teardown: {join_error}")
    })?;

    let settled_bytes = captured.lock().len();
    if settled_bytes == 0 {
        return Err("outbound writer produced no frames for the whole session".into());
    }

    Ok(())
}
