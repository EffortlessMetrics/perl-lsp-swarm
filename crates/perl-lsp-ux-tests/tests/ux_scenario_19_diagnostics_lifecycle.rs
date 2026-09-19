// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Scenario 19 — diagnostics lifecycle during active editing.
//!
//! This scenario covers an editor-critical UX flow: a user introduces a parse
//! error, sees diagnostics, fixes the file, and expects diagnostics to clear.
//!
//! # Explicit-clear contract
//!
//! For this push-diagnostics client profile, the repaired state requires an
//! explicit `textDocument/publishDiagnostics` replacement for the repaired
//! document version with an empty diagnostic list. Silence is not an
//! alternate success path: diagnostics that an editor already retained from
//! the broken source stay on screen unless the server replaces them.
//!
//! Accepted pass shape (all required):
//!
//! 1. broken version 1 publishes non-empty diagnostics;
//! 2. the repaired version-2 change is sent;
//! 3. the latest version-2 publication for the URI is empty before the
//!    bounded deadline.
//!
//! Negative controls enforced by the oracle:
//!
//! - stale version-1 publications and unversioned publications remain
//!   visible in the event stream but can never satisfy version 2;
//! - a future version (larger than 2) cannot satisfy version 2;
//! - a transient non-empty version-2 frame does not clear: the *latest*
//!   version-2 publication must be empty, so a late regression replaces an
//!   earlier empty frame and fails the barrier;
//! - silence, queue drain, and fixed sleeps cannot construct a pass.
//!
//! # Buffer-only repair discriminator
//!
//! The backing file stays broken for the whole scenario. The fixed content
//! travels only over `textDocument/didChange`, so the repaired result also
//! exercises open-buffer authority: disk fallback cannot satisfy the empty
//! version-2 publication because the disk sentinel remains broken.

use anyhow::Context;
use perl_lsp_ux_tests::{LspEvent, ScenarioConfig, UxHarness, binary_available};
use serde_json::Value;
use std::time::Duration;

const FILE: &str = "live.pl";
const BROKEN_SOURCE: &str = "use strict;\nuse warnings;\nmy $x = ;\n";
const FIXED_SOURCE: &str = "use strict;\nuse warnings;\nmy $x = 1;\nprint $x;\n";
const FIXED_VERSION: i32 = 2;

/// One `publishDiagnostics` observation for the scenario URI.
#[derive(Clone, Debug)]
struct DiagnosticObservation {
    version: Option<i64>,
    diagnostics: Vec<Value>,
}

/// Latest publication for the exact repaired version, if any.
///
/// Stale, unversioned, and future-versioned publications are visible in the
/// event stream but can never satisfy the repaired-version barrier.
fn latest_for_version<'a>(
    observations: &'a [DiagnosticObservation],
    version: i64,
) -> Option<&'a DiagnosticObservation> {
    observations.iter().rev().find(|observation| observation.version == Some(version))
}

/// Explicit-clear barrier: the latest publication for the repaired version
/// must exist and be empty.
///
/// Taking the *latest* frame (rather than any frame) is what makes a
/// transient non-empty repaired publication fail-safe: an early empty frame
/// followed by a non-empty regression does not clear.
fn latest_cleared_for_version<'a>(
    observations: &'a [DiagnosticObservation],
    version: i64,
) -> Option<&'a DiagnosticObservation> {
    latest_for_version(observations, version)
        .filter(|observation| observation.diagnostics.is_empty())
}

/// Drain pending server events, keeping only diagnostics for `uri`.
fn observe_uri_diagnostics(
    harness: &UxHarness,
    uri: &str,
    observations: &mut Vec<DiagnosticObservation>,
) {
    for event in harness.collect_notifications() {
        if let LspEvent::Diagnostics { uri: event_uri, version, diagnostics } = event
            && event_uri == uri
        {
            observations.push(DiagnosticObservation { version, diagnostics });
        }
    }
}

/// Verifies the diagnostics edit lifecycle:
///   1. Broken content → non-empty diagnostics.
///   2. Buffer-only repaired change → explicit empty version-2 publication.
#[test]
fn scenario_19_diagnostics_clear_after_fix() -> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_19_diagnostics_clear_after_fix: perl-lsp binary not found");
        return Ok(());
    }

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
            .with_file(FILE, BROKEN_SOURCE),
    )
    .context("Failed to create UX harness")?;

    // Given: a workspace file opened with a syntax error.
    harness.open_file(FILE, BROKEN_SOURCE).context("didOpen should succeed")?;
    let uri = harness.workspace.uri(FILE);

    // When: diagnostics are first published for the broken content.
    let broken_diagnostics = harness.wait_for_diagnostics(FILE, Duration::from_secs(5));
    assert!(
        !broken_diagnostics.is_empty(),
        "Expected diagnostics for broken source, but none were published."
    );

    // Drain any follow-up broken-content batches (perltidy/perlcritic passes)
    // into the observation log before the repair.
    let mut observations: Vec<DiagnosticObservation> = Vec::new();
    observe_uri_diagnostics(&harness, &uri, &mut observations);

    // When: the user fixes the file via a full-document didChange that only
    // touches the editor buffer. The backing file stays broken on purpose so
    // disk fallback cannot construct the repaired result.
    harness
        .client
        .did_change_full(&uri, FIXED_VERSION, FIXED_SOURCE)
        .context("didChange should succeed")?;

    let disk_source = std::fs::read_to_string(harness.workspace.path(FILE))
        .context("scenario workspace file should remain readable")?;
    assert_eq!(
        disk_source, BROKEN_SOURCE,
        "test setup must keep the repaired editor buffer distinct from the backing file"
    );

    // Then: the server must explicitly replace the repaired generation's
    // diagnostics with an empty list before the deadline. Silence never
    // clears, stale/unversioned frames never satisfy version 2, and a
    // terminal non-empty version-2 publication fails.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut cleared = false;
    while std::time::Instant::now() < deadline {
        observe_uri_diagnostics(&harness, &uri, &mut observations);
        if latest_cleared_for_version(&observations, i64::from(FIXED_VERSION)).is_some() {
            cleared = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    assert!(
        cleared,
        "Expected an explicit empty publishDiagnostics replacement for version {FIXED_VERSION}; \
         silence, stale, unversioned, and non-empty repaired frames cannot clear. \
         Observations: {observations:?}"
    );
    harness.assert_no_crash();
    Ok(())
}

#[cfg(test)]
mod oracle_unit_tests {
    use super::{DiagnosticObservation, latest_cleared_for_version, latest_for_version};
    use serde_json::{Value, json};

    fn observation(version: Option<i64>, diagnostics: Vec<Value>) -> DiagnosticObservation {
        DiagnosticObservation { version, diagnostics }
    }

    #[test]
    fn stale_and_unversioned_publications_cannot_satisfy_repaired_version() {
        let observations = vec![observation(Some(1), Vec::new()), observation(None, Vec::new())];
        assert!(latest_for_version(&observations, 2).is_none());
        assert!(latest_cleared_for_version(&observations, 2).is_none());
    }

    #[test]
    fn unrequested_future_publication_cannot_satisfy_repaired_version() {
        let observations = vec![
            observation(Some(1), vec![json!({"message": "stale"})]),
            observation(Some(3), Vec::new()),
        ];

        assert!(latest_for_version(&observations, 2).is_none());
        assert!(latest_cleared_for_version(&observations, 2).is_none());
    }

    #[test]
    fn latest_current_publication_is_authoritative() {
        // An empty frame followed by a non-empty regression does not clear:
        // the editor state is what the latest publication says it is.
        let regressed = vec![
            observation(Some(2), Vec::new()),
            observation(Some(2), vec![json!({"message": "late current regression"})]),
        ];
        assert!(latest_cleared_for_version(&regressed, 2).is_none());

        // A transient non-empty repaired frame followed by an explicit empty
        // replacement satisfies the barrier.
        let transient_then_cleared = vec![
            observation(Some(2), vec![json!({"message": "residual"})]),
            observation(Some(2), Vec::new()),
        ];
        let cleared = latest_cleared_for_version(&transient_then_cleared, 2)
            .expect("the later empty replacement satisfies the barrier");
        assert_eq!(cleared.version, Some(2));
        assert!(cleared.diagnostics.is_empty());
    }
}
