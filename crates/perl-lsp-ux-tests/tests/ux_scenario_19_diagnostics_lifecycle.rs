// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Scenario 19 — diagnostics lifecycle during active editing.
//!
//! This scenario covers an editor-critical UX flow: a user introduces a parse
//! error, sees diagnostics, fixes the file, and expects diagnostics to clear.
//!
//! For this push-diagnostics client profile, active-document readiness is
//! projected only after a current diagnostics publication and current document
//! symbols have committed. The repaired state therefore requires the explicit
//! post-edit diagnostics frame that precedes readiness; silence is not an
//! alternate success path.
//!
//! Stale lower-version and unversioned publications remain visible in the
//! captured evidence but cannot satisfy the repaired-generation barrier.

#[path = "support/active_document_readiness.rs"]
mod active_document_readiness;

use active_document_readiness::{ready_event_count, wait_for_generation_after};
use anyhow::{Result, bail};
use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{LspEvent, ScenarioConfig, UxHarness};
use serde_json::Value;
use std::time::{Duration, Instant};

const FILE: &str = "live.pl";
const BROKEN_SOURCE: &str = "use strict;\nuse warnings;\nmy $x = ;\n";
const FIXED_SOURCE: &str = "use strict;\nuse warnings;\nmy $x = 1;\nprint $x;\n";
const BROKEN_VERSION: i64 = 1;
const FIXED_VERSION: i64 = 2;
const DIAGNOSTICS_TIMEOUT: Duration = Duration::from_secs(10);
const READY_TIMEOUT: Duration = Duration::from_secs(10);
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

fn latest_for_version(
    observations: &[DiagnosticObservation],
    minimum_version: i64,
) -> Option<&DiagnosticObservation> {
    observations.iter().rev().find(|observation| {
        observation
            .version
            .is_some_and(|version| version == minimum_version)
    })
}

fn wait_for_versioned_diagnostics_after(
    harness: &UxHarness,
    uri: &str,
    already_seen: usize,
    minimum_version: i64,
    timeout: Duration,
) -> Result<Vec<DiagnosticObservation>> {
    let deadline = Instant::now() + timeout;
    loop {
        let observations = diagnostic_observations_after(
            &harness.peek_notifications(),
            uri,
            already_seen,
        );
        if latest_for_version(&observations, minimum_version).is_some() {
            return Ok(observations);
        }
        if Instant::now() >= deadline {
            bail!(
                "timed out after {}ms waiting for diagnostics for {uri} with version {minimum_version} after {already_seen} prior URI-matched publications; observed: \
                 {observations:?}",
                timeout.as_millis()
            );
        }
        std::thread::sleep(OBSERVATION_POLL_INTERVAL);
    }
}

/// Verifies the diagnostics edit lifecycle:
///   1. Broken version 1 publishes non-empty diagnostics.
///   2. Fixed version 2 reaches parser-core readiness.
///   3. An explicit post-cursor version-2-or-newer publication is empty.
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
    let broken_observations = wait_for_versioned_diagnostics_after(
        &harness,
        &uri,
        0,
        BROKEN_VERSION,
        DIAGNOSTICS_TIMEOUT,
    )?;
    let broken = latest_for_version(&broken_observations, BROKEN_VERSION)
        .ok_or_else(|| anyhow::anyhow!("version-1 diagnostics disappeared after the wait"))?;
    assert_eq!(
        broken.version,
        Some(BROKEN_VERSION),
        "initial broken publication must identify editor version 1; observed {broken_observations:?}"
    );
    assert!(
        !broken.diagnostics.is_empty(),
        "expected non-empty diagnostics for broken version 1; observed {broken_observations:?}"
    );

    let diagnostics_seen_before_fix = harness.diagnostics_event_count(FILE);
    let readiness_seen_before_fix = ready_event_count(&harness, &uri);

    harness.client.did_change_full(&uri, FIXED_VERSION as i32, FIXED_SOURCE)?;
    let readiness = wait_for_generation_after(
        &harness,
        &uri,
        FIXED_VERSION as u64,
        readiness_seen_before_fix,
        READY_TIMEOUT,
    )?;

    // The current push-diagnostics sink commits before active-document
    // readiness is projected. Require that explicit post-edit publication
    // rather than treating the absence of a later frame as clean.
    let repaired_observations = wait_for_versioned_diagnostics_after(
        &harness,
        &uri,
        diagnostics_seen_before_fix,
        FIXED_VERSION,
        DIAGNOSTICS_TIMEOUT,
    )?;
    let repaired = latest_for_version(&repaired_observations, FIXED_VERSION)
        .ok_or_else(|| anyhow::anyhow!("current diagnostics disappeared after the wait"))?;
    assert!(
        repaired.diagnostics.is_empty(),
        "generation {} reached readiness at matching ordinal {}, but the latest explicit \
         repaired-generation diagnostics publication was non-empty: {repaired_observations:?}",
        readiness.generation,
        readiness.matching_ordinal
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
    use super::{DiagnosticObservation, latest_for_version};
    use serde_json::json;

    #[test]
    fn stale_and_unversioned_publications_cannot_satisfy_repaired_version() {
        let observations = vec![
            DiagnosticObservation {
                version: Some(1),
                diagnostics: Vec::new(),
            },
            DiagnosticObservation {
                version: None,
                diagnostics: Vec::new(),
            },
        ];
        assert_eq!(latest_for_version(&observations, 2), None);
    }

    #[test]
    fn unrequested_future_publication_cannot_satisfy_repaired_version() {
        let observations = vec![
            DiagnosticObservation {
                version: Some(1),
                diagnostics: vec![json!({"message": "stale"})],
            },
            DiagnosticObservation {
                version: Some(3),
                diagnostics: Vec::new(),
            },
        ];

        assert_eq!(latest_for_version(&observations, 2), None);
    }

    #[test]
    fn latest_current_publication_is_authoritative() {
        let observations = vec![
            DiagnosticObservation {
                version: Some(2),
                diagnostics: Vec::new(),
            },
            DiagnosticObservation {
                version: Some(2),
                diagnostics: vec![json!({"message": "late current regression"})],
            },
        ];
        let latest = latest_for_version(&observations, 2)
            .expect("a current publication should be selected");
        assert_eq!(latest.version, Some(2));
        assert!(!latest.diagnostics.is_empty());
    }

    #[test]
    fn explicit_repaired_generation_empty_is_selected() {
        let observations = vec![
            DiagnosticObservation {
                version: Some(1),
                diagnostics: vec![json!({"message": "stale"})],
            },
            DiagnosticObservation {
                version: Some(2),
                diagnostics: Vec::new(),
            },
        ];
        let latest = latest_for_version(&observations, 2)
            .expect("a current empty publication should be selected");
        assert_eq!(latest.version, Some(2));
        assert!(latest.diagnostics.is_empty());
    }
}
