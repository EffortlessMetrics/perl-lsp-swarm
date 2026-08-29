// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Scenario 19 — diagnostics lifecycle during active editing.
//!
//! This scenario covers an editor-critical UX flow: a user introduces a parse
//! error, sees diagnostics, fixes the file, and expects diagnostics to clear.
//!
//! The repaired source is accepted only after a generation-sensitive
//! `perl-lsp/active-document-ready` barrier proves that generation 2 and its
//! required parser-core effects are current. An explicit version-2 empty
//! publication is the strongest observed result. Silence is accepted only
//! after that barrier and a bounded post-ready observation window; wall-clock
//! quiet by itself is never the correctness oracle.
//!
//! Stale lower-version diagnostic publications remain visible in the captured
//! evidence but cannot determine the repaired source's verdict. A non-empty
//! current or unversioned publication after the repair remains a regression.

#[path = "support/active_document_readiness.rs"]
mod active_document_readiness;

use active_document_readiness::{ready_event_count, wait_for_generation_after};
use anyhow::Result;
use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{LspEvent, ScenarioConfig, UxHarness};
use serde_json::Value;
use std::time::{Duration, Instant};

const FILE: &str = "live.pl";
const BROKEN_SOURCE: &str = "use strict;\nuse warnings;\nmy $x = ;\n";
const FIXED_SOURCE: &str = "use strict;\nuse warnings;\nmy $x = 1;\nprint $x;\n";
const FIXED_VERSION: i64 = 2;
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const POST_READY_OBSERVATION_WINDOW: Duration = Duration::from_secs(1);
const OBSERVATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, PartialEq)]
struct DiagnosticObservation {
    version: Option<i64>,
    diagnostics: Vec<Value>,
}

fn diagnostic_observations_after(
    events: &[LspEvent],
    uri: &str,
    already_seen: usize,
) -> Vec<DiagnosticObservation> {
    events
        .iter()
        .filter_map(|event| {
            let LspEvent::Diagnostics {
                uri: event_uri,
                version,
                diagnostics,
            } = event
            else {
                return None;
            };
            (event_uri == uri).then(|| DiagnosticObservation {
                version: *version,
                diagnostics: diagnostics.clone(),
            })
        })
        .skip(already_seen)
        .collect()
}

fn is_stale(observation: &DiagnosticObservation) -> bool {
    observation.version.is_some_and(|version| version < FIXED_VERSION)
}

fn has_current_or_unversioned_non_empty(observations: &[DiagnosticObservation]) -> bool {
    observations
        .iter()
        .any(|observation| !is_stale(observation) && !observation.diagnostics.is_empty())
}

fn has_explicit_current_empty(observations: &[DiagnosticObservation]) -> bool {
    observations.iter().any(|observation| {
        observation.version.is_some_and(|version| version >= FIXED_VERSION)
            && observation.diagnostics.is_empty()
    })
}

/// Verifies the diagnostics edit lifecycle:
///   1. Broken content → diagnostics appear.
///   2. Fixed generation → parser-core readiness becomes current.
///   3. Fixed diagnostics are explicitly empty or remain quiet after readiness.
#[test]
fn scenario_19_diagnostics_clear_after_fix() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_19_diagnostics_clear_after_fix: perl-lsp binary not found");
        return Ok(());
    }

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
            .env("PERL_LSP_WORKSPACE", "1")
            .env("PERL_LSP_E2E", "1")
            .with_file(FILE, BROKEN_SOURCE),
    )?;
    let uri = harness.workspace.uri(FILE);

    // Keep the backing file broken. Both editor generations travel only over
    // LSP so the post-fix result also exercises open-buffer authority.
    harness.client.did_open(&uri, BROKEN_SOURCE)?;
    let diagnostics = harness.wait_for_diagnostics(FILE, Duration::from_secs(5));
    assert!(
        !diagnostics.is_empty(),
        "expected diagnostics for broken source, but none were published"
    );

    let diagnostics_seen_before_fix = harness.diagnostics_event_count(FILE);
    let readiness_seen_before_fix = ready_event_count(&harness, &uri);

    harness.client.did_change_full(&uri, FIXED_VERSION as i32, FIXED_SOURCE)?;
    wait_for_generation_after(
        &harness,
        &uri,
        FIXED_VERSION as u64,
        readiness_seen_before_fix,
        READY_TIMEOUT,
    )?;

    // The generation barrier establishes currentness. This bounded window is
    // only a regression detector for late current/non-versioned non-empty
    // publications; it does not manufacture the clean verdict.
    let observation_deadline = Instant::now() + POST_READY_OBSERVATION_WINDOW;
    loop {
        let observations = diagnostic_observations_after(
            &harness.peek_notifications(),
            &uri,
            diagnostics_seen_before_fix,
        );
        if has_current_or_unversioned_non_empty(&observations)
            || Instant::now() >= observation_deadline
        {
            break;
        }
        std::thread::sleep(OBSERVATION_POLL_INTERVAL);
    }

    let observations = diagnostic_observations_after(
        &harness.peek_notifications(),
        &uri,
        diagnostics_seen_before_fix,
    );
    let explicit_current_empty = has_explicit_current_empty(&observations);
    assert!(
        !has_current_or_unversioned_non_empty(&observations),
        "fixed generation reached parser-core readiness but retained/published current diagnostics; \
         explicit_current_empty={explicit_current_empty}, observations={observations:?}"
    );

    let disk_source = std::fs::read_to_string(harness.workspace.path(FILE))?;
    assert_eq!(
        disk_source, BROKEN_SOURCE,
        "test setup must keep the repaired editor buffer distinct from the backing file"
    );

    harness.assert_no_crash();
    Ok(())
}

#[cfg(test)]
mod oracle_unit_tests {
    use super::{
        DiagnosticObservation, has_current_or_unversioned_non_empty,
        has_explicit_current_empty,
    };
    use serde_json::json;

    #[test]
    fn stale_non_empty_does_not_control_fixed_generation() {
        let observations = vec![DiagnosticObservation {
            version: Some(1),
            diagnostics: vec![json!({"message": "stale"})],
        }];
        assert!(!has_current_or_unversioned_non_empty(&observations));
        assert!(!has_explicit_current_empty(&observations));
    }

    #[test]
    fn current_and_unversioned_non_empty_remain_failures() {
        for version in [Some(2), None] {
            let observations = vec![DiagnosticObservation {
                version,
                diagnostics: vec![json!({"message": "not cleared"})],
            }];
            assert!(has_current_or_unversioned_non_empty(&observations));
        }
    }

    #[test]
    fn explicit_fixed_generation_empty_is_detected() {
        let observations = vec![DiagnosticObservation {
            version: Some(2),
            diagnostics: Vec::new(),
        }];
        assert!(has_explicit_current_empty(&observations));
        assert!(!has_current_or_unversioned_non_empty(&observations));
    }
}
