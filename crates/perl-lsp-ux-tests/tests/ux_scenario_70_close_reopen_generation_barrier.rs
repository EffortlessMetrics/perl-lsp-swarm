// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Scenario 70 — close/reopen readiness is generation-sensitive.
//!
//! Editors routinely close and reopen the same URI while old analysis work is
//! still draining. A URI-only wait can therefore accept a buffered pre-close
//! `perl-lsp/active-document-ready` event and release the next provider request
//! before the reopened buffer is ready.
//!
//! This scenario exercises the real `perl-lsp` process over stdio and makes the
//! lifecycle barrier load-bearing:
//!
//! 1. open the document and observe generation 1;
//! 2. edit it and observe generation 2;
//! 3. snapshot all matching readiness events;
//! 4. close and reopen the same URI with different source;
//! 5. require a new generation-1 event after the snapshot;
//! 6. request document symbols and prove the reopened buffer is authoritative.
//!
//! The barrier helper deliberately ignores delayed generation-2 evidence after
//! the snapshot. A focused unit test keeps that negative control deterministic.

use anyhow::{Result, bail};
use perl_lsp_ux_tests::{
    LspEvent, ScenarioConfig, UxHarness, binary_available, document_symbol_names,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const FILE: &str = "lifecycle.pl";
const READY_METHOD: &str = "perl-lsp/active-document-ready";
const INITIAL_SYMBOL: &str = "initial_symbol";
const PRE_CLOSE_SYMBOL: &str = "pre_close_symbol";
const REOPENED_SYMBOL: &str = "reopened_symbol";
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(20);

const INITIAL_SOURCE: &str = r#"use strict;
use warnings;

sub initial_symbol {
    return "initial";
}

initial_symbol();
"#;

const PRE_CLOSE_SOURCE: &str = r#"use strict;
use warnings;

sub pre_close_symbol {
    return "pre-close";
}

pre_close_symbol();
"#;

const REOPENED_SOURCE: &str = r#"use strict;
use warnings;

sub reopened_symbol {
    return "reopened";
}

reopened_symbol();
"#;

fn ready_generation(event: &LspEvent, uri: &str) -> Option<u64> {
    let LspEvent::Other { method, params } = event else {
        return None;
    };
    if method != READY_METHOD || params.get("uri").and_then(Value::as_str) != Some(uri) {
        return None;
    }
    params.get("generation").and_then(Value::as_u64)
}

fn ready_generations(events: &[LspEvent], uri: &str) -> Vec<u64> {
    events.iter().filter_map(|event| ready_generation(event, uri)).collect()
}

fn has_generation_after(
    generations: &[u64],
    already_seen: usize,
    expected_generation: u64,
) -> bool {
    generations
        .get(already_seen..)
        .is_some_and(|new_generations| new_generations.contains(&expected_generation))
}

fn wait_for_ready_generation_after(
    harness: &UxHarness,
    uri: &str,
    expected_generation: u64,
    already_seen: usize,
    timeout: Duration,
) -> Result<Vec<u64>> {
    let deadline = Instant::now() + timeout;
    loop {
        let generations = ready_generations(&harness.peek_notifications(), uri);
        if has_generation_after(&generations, already_seen, expected_generation) {
            return Ok(generations);
        }
        if Instant::now() >= deadline {
            bail!(
                "timed out after {}ms waiting for a new {READY_METHOD} event for {uri} with \
                 generation {expected_generation} after {already_seen} prior matching events; \
                 observed matching generations: {generations:?}",
                timeout.as_millis()
            );
        }
        std::thread::sleep(READY_POLL_INTERVAL);
    }
}

#[test]
fn scenario_70_close_reopen_requires_new_generation_barrier() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_70: perl-lsp binary not found");
        return Ok(());
    }

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
            .with_file(FILE, INITIAL_SOURCE),
    )?;
    let uri = harness.workspace.uri(FILE);

    harness.open_file(FILE, INITIAL_SOURCE)?;
    let initial_generations =
        wait_for_ready_generation_after(&harness, &uri, 1, 0, READY_TIMEOUT)?;
    let initial_ready_count = initial_generations.len();

    harness.change_file_full(FILE, PRE_CLOSE_SOURCE)?;
    let pre_close_generations = wait_for_ready_generation_after(
        &harness,
        &uri,
        2,
        initial_ready_count,
        READY_TIMEOUT,
    )?;
    let pre_close_ready_count = pre_close_generations.len();

    harness.client.notify(
        "textDocument/didClose",
        json!({
            "textDocument": {
                "uri": uri.clone()
            }
        }),
    )?;
    harness.open_file(FILE, REOPENED_SOURCE)?;

    let reopened_generations = wait_for_ready_generation_after(
        &harness,
        &uri,
        1,
        pre_close_ready_count,
        READY_TIMEOUT,
    )?;
    assert!(
        reopened_generations.len() > pre_close_ready_count,
        "reopen barrier must be backed by post-snapshot readiness evidence: \
         snapshot={pre_close_ready_count}, observed={reopened_generations:?}"
    );
    let post_snapshot_generations = &reopened_generations[pre_close_ready_count..];
    assert!(
        post_snapshot_generations.contains(&1),
        "reopen barrier must observe generation 1 after close/reopen; \
         post-snapshot generations: {post_snapshot_generations:?}"
    );

    let symbols = harness.document_symbols(FILE)?;
    let names = document_symbol_names(&symbols);
    assert!(
        names.iter().any(|name| name == REOPENED_SYMBOL),
        "document symbols after the reopen barrier must come from the reopened buffer; got {names:?}"
    );
    for stale_symbol in [INITIAL_SYMBOL, PRE_CLOSE_SYMBOL] {
        assert!(
            !names.iter().any(|name| name == stale_symbol),
            "document symbols after reopen must not expose stale `{stale_symbol}`; got {names:?}"
        );
    }

    harness.assert_no_crash();
    Ok(())
}

#[cfg(test)]
mod barrier_unit_tests {
    use super::{READY_METHOD, has_generation_after, ready_generations};
    use perl_lsp_ux_tests::LspEvent;
    use serde_json::json;

    #[test]
    fn late_pre_close_generation_does_not_release_reopen_barrier() {
        let mut generations = vec![1, 2];
        let snapshot = generations.len();

        assert!(!has_generation_after(&generations, snapshot, 1));

        generations.push(2);
        assert!(
            !has_generation_after(&generations, snapshot, 1),
            "a delayed pre-close generation must not release the generation-1 reopen barrier"
        );

        generations.push(1);
        assert!(
            has_generation_after(&generations, snapshot, 1),
            "a new post-snapshot generation 1 must release the reopen barrier"
        );
    }

    #[test]
    fn readiness_filter_requires_matching_uri_method_and_numeric_generation() {
        let wanted_uri = "file:///workspace/lifecycle.pl";
        let events = vec![
            LspEvent::Other {
                method: READY_METHOD.to_string(),
                params: json!({"uri": wanted_uri, "generation": 1}),
            },
            LspEvent::Other {
                method: READY_METHOD.to_string(),
                params: json!({"uri": "file:///workspace/other.pl", "generation": 2}),
            },
            LspEvent::Other {
                method: "perl-lsp/other".to_string(),
                params: json!({"uri": wanted_uri, "generation": 3}),
            },
            LspEvent::Other {
                method: READY_METHOD.to_string(),
                params: json!({"uri": wanted_uri, "generation": "4"}),
            },
        ];

        assert_eq!(ready_generations(&events, wanted_uri), vec![1]);
    }
}
