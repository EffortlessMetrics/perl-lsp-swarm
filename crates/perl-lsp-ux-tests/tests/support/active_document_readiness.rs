#![allow(dead_code)] // Separate integration crates consume different helper subsets.

//! Generation-sensitive active-document readiness helpers for UX scenarios.
//!
//! These helpers observe the existing `perl-lsp/active-document-ready`
//! notification, bind it to one exact URI and numeric generation, and require
//! evidence after a caller-owned matching-event cursor. They do not introduce a
//! second readiness state machine or claim provider-result correlation, client
//! acknowledgement, or held-work ABA proof.
//!
//! A cursor is valid while the caller retains the event queue. Do not drain
//! matching readiness events between taking the cursor and waiting on it.
//!
//! Numeric generation is not a document-session identity: the server resets it
//! on close/reopen. Until the versioned readiness payload carries the canonical
//! document instance, a close/reopen consumer must pair this barrier with an
//! independent post-reopen result discriminator.

use anyhow::{Result, bail};
use perl_lsp_ux_tests::{LspEvent, UxHarness};
use serde_json::Value;
use std::time::{Duration, Instant};

pub(crate) const ACTIVE_DOCUMENT_READY_METHOD: &str = "perl-lsp/active-document-ready";
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// One URI-matched readiness observation after a caller-owned cursor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReadyObservation {
    pub(crate) generation: u64,
    /// One-based ordinal among valid readiness events for this exact URI.
    pub(crate) matching_ordinal: usize,
}

pub(crate) fn ready_generation(event: &LspEvent, uri: &str) -> Option<u64> {
    let LspEvent::Other { method, params } = event else {
        return None;
    };
    if method != ACTIVE_DOCUMENT_READY_METHOD
        || params.get("uri").and_then(Value::as_str) != Some(uri)
    {
        return None;
    }
    params
        .get("generation")
        .filter(|generation| generation.is_u64())
        .and_then(Value::as_u64)
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

pub(crate) fn generation_after(
    generations: &[u64],
    already_seen: usize,
    expected_generation: u64,
) -> Option<ReadyObservation> {
    let offset = generations
        .get(already_seen..)?
        .iter()
        .position(|generation| *generation == expected_generation)?;
    Some(ReadyObservation {
        generation: expected_generation,
        matching_ordinal: already_seen + offset + 1,
    })
}

pub(crate) fn wait_for_generation_after(
    harness: &UxHarness,
    uri: &str,
    expected_generation: u64,
    already_seen: usize,
    timeout: Duration,
) -> Result<ReadyObservation> {
    let deadline = Instant::now() + timeout;
    loop {
        let generations = ready_generations(&harness.peek_notifications(), uri);
        if already_seen > generations.len() {
            bail!(
                "readiness cursor {already_seen} exceeds the retained matching-event count {} for {uri}",
                generations.len()
            );
        }
        if let Some(observation) =
            generation_after(&generations, already_seen, expected_generation)
        {
            return Ok(observation);
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
