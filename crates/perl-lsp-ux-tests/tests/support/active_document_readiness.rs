//! Generation-sensitive active-document readiness helpers for UX scenarios.
//!
//! These helpers keep the test oracle narrower than the runtime contract: they
//! observe the existing `perl-lsp/active-document-ready` notification, bind it
//! to one exact URI and numeric generation, and require evidence after a caller
//! supplied event-count snapshot. They do not claim client acknowledgement,
//! provider-result correlation, or deep held-work lifecycle proof.

use anyhow::{Result, bail};
use perl_lsp_ux_tests::{LspEvent, UxHarness};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub(crate) const ACTIVE_DOCUMENT_READY_METHOD: &str = "perl-lsp/active-document-ready";
const POLL_INTERVAL: Duration = Duration::from_millis(20);

pub(crate) fn ready_generation(event: &LspEvent, uri: &str) -> Option<u64> {
    let LspEvent::Other { method, params } = event else {
        return None;
    };
    if method != ACTIVE_DOCUMENT_READY_METHOD
        || params.get("uri").and_then(Value::as_str) != Some(uri)
    {
        return None;
    }
    params.get("generation").and_then(Value::as_u64)
}

pub(crate) fn ready_generations(events: &[LspEvent], uri: &str) -> Vec<u64> {
    events
        .iter()
        .filter_map(|event| ready_generation(event, uri))
        .collect()
}

pub(crate) fn ready_event_count(harness: &UxHarness, uri: &str) -> usize {
    ready_generations(&harness.peek_notifications(), uri).len()
}

pub(crate) fn has_generation_after(
    generations: &[u64],
    already_seen: usize,
    expected_generation: u64,
) -> bool {
    generations
        .get(already_seen..)
        .is_some_and(|new_generations| new_generations.contains(&expected_generation))
}

pub(crate) fn wait_for_generation_after(
    harness: &UxHarness,
    uri: &str,
    expected_generation: u64,
    already_seen: usize,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let generations = ready_generations(&harness.peek_notifications(), uri);
        if has_generation_after(&generations, already_seen, expected_generation) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "timed out after {}ms waiting for a new {ACTIVE_DOCUMENT_READY_METHOD} event for \
                 {uri} with generation {expected_generation} after {already_seen} prior matching \
                 events; observed matching generations: {generations:?}",
                timeout.as_millis()
            );
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

pub(crate) fn notify_close(harness: &UxHarness, relative_path: &str) -> Result<()> {
    let uri = harness.workspace.uri(relative_path);
    harness.client.notify(
        "textDocument/didClose",
        json!({
            "textDocument": {
                "uri": uri
            }
        }),
    )
}
