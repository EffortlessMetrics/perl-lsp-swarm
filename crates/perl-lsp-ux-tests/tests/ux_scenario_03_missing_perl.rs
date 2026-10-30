//! Scenario 03 — Missing perl interpreter.
//!
//! Simulates perl-lsp running without `perl` on PATH.
//!
//! Acceptance criteria:
//! - Server MUST start (it is a Rust binary).
//! - Server MUST accept `initialize` and `textDocument/didOpen`.
//! - Server MUST NOT crash during initialization.
//! - Hover and completion may return null/empty — that is acceptable.
//! - The interpreter resolution must reach the user exactly once per session,
//!   with actionable content: either one `window/showMessage` Error naming the
//!   missing interpreter with the canonical install remediation, or one
//!   `window/logMessage` Info naming the fallback interpreter that was found.
//!
//! # Environment note
//!
//! With an empty PATH the server may still resolve an interpreter through its
//! canonical fallback probes (for example `/usr/bin/perl` on the CI runner or
//! `C:\Strawberry\perl\bin\perl.exe` on a Windows workstation), in which case
//! the not-found Error legitimately never fires and the fallback log arm is
//! the one under observation. Both arms are pinned so the test is
//! discriminating in every environment.

use anyhow::Context;
use perl_lsp_ux_tests::{
    LspEvent, ScenarioConfig, UxCiTier, UxComponent, UxEvidenceClass, UxHarness, binary_available,
    missing_binary_skip, run_ux_scenario_with_evidence_class,
};
use std::time::Duration;

const WORKFLOW_ID: &str = "missing_perl_graceful_degradation";
const SCENARIO_FILE: &str = "ux_scenario_03_missing_perl.rs";

/// Distinctive head of the once-per-session not-found message
/// (`perl_not_found_message`, generic arm).
const PERL_MISSING_ON_PATH: &str = "Perl missing on PATH";
/// Distinctive substring of the canonical remediation sentence
/// (`PERL_REMEDIATION`), which the server's own unit tests pin.
const STRAWBERRY_PERL: &str = "strawberryperl.com";
/// Distinctive head of the fallback log message (`perl_fallback_message`).
const PERL_FALLBACK_IN_USE: &str = "not found on PATH; using the";

/// Bound on the event-driven wait for the resolution message. The message is
/// emitted during `initialize` handling, so this only needs to cover transport
/// latency, never product work.
const RESOLUTION_WAIT: Duration = Duration::from_secs(15);

fn config_without_perl() -> ScenarioConfig {
    ScenarioConfig::with_empty_path()
}

/// Count resolution messages on either channel at any severity.
///
/// The two phrases are deliberately mutually exclusive: `perl_not_found_message`
/// says "Perl missing on PATH" while `perl_fallback_message` says "Perl not
/// found on PATH; using the … installation", so one session can satisfy at most
/// one arm. Channel and severity are checked after counting, so a malformed
/// duplicate cannot evade the once-per-session assertion.
fn interpreter_resolution_counts(events: &[LspEvent]) -> (usize, usize) {
    let mut not_found = 0;
    let mut fallback = 0;
    for event in events {
        if let LspEvent::WindowMessage { message, .. } | LspEvent::LogMessage { message, .. } =
            event
        {
            not_found += usize::from(message.contains(PERL_MISSING_ON_PATH));
            fallback += usize::from(message.contains(PERL_FALLBACK_IN_USE));
        }
    }
    (not_found, fallback)
}

#[test]
fn scenario_03_duplicate_fallback_at_wrong_severity_still_counts_twice() -> anyhow::Result<()> {
    let message = format!("Perl {PERL_FALLBACK_IN_USE} fallback installation");
    let events = [
        LspEvent::LogMessage { message_type: 3, message: message.clone() },
        LspEvent::LogMessage { message_type: 2, message },
    ];
    let (_, fallback) = interpreter_resolution_counts(&events);
    anyhow::ensure!(fallback == 2, "a wrong-severity duplicate evaded the once-per-session count");
    Ok(())
}

#[test]
fn scenario_03_duplicate_not_found_at_wrong_severity_still_counts_twice() -> anyhow::Result<()> {
    let message = format!("{PERL_MISSING_ON_PATH}; see {STRAWBERRY_PERL}");
    let events = [
        LspEvent::WindowMessage { message_type: 1, message: message.clone() },
        LspEvent::WindowMessage { message_type: 2, message },
    ];
    let (not_found, _) = interpreter_resolution_counts(&events);
    anyhow::ensure!(not_found == 2, "a wrong-severity not-found duplicate evaded the count");
    Ok(())
}

#[test]
fn scenario_03_duplicate_fallback_on_wrong_channel_still_counts_twice() -> anyhow::Result<()> {
    let message = format!("Perl {PERL_FALLBACK_IN_USE} fallback installation");
    let events = [
        LspEvent::LogMessage { message_type: 3, message: message.clone() },
        LspEvent::WindowMessage { message_type: 3, message },
    ];
    let (_, fallback) = interpreter_resolution_counts(&events);
    anyhow::ensure!(fallback == 2, "a wrong-channel fallback duplicate evaded the count");
    Ok(())
}

#[test]
fn scenario_03_server_starts_without_perl() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_03_server_starts_without_perl",
        UxCiTier::Pr,
        Some(UxComponent::Infra),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let source = "use strict;\nmy $x = 1;\n";
            let harness = UxHarness::new(config_without_perl())
                .context("Failed to create UX harness without perl")?;

            let open_result = harness.open_file("no_perl.pl", source);
            recorder.check("didOpen accepted without perl", open_result.is_ok())?;
            open_result.context("didOpen should succeed without perl")?;

            harness.assert_no_crash();
            Ok(())
        },
    );
}

#[test]
fn scenario_03_degraded_mode_hover_does_not_crash() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_03_degraded_mode_hover_does_not_crash",
        UxCiTier::Pr,
        Some(UxComponent::Hover),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let source = "my $x = 42;\n";
            let harness = UxHarness::new(config_without_perl())
                .context("Failed to create UX harness without perl")?;

            let open_result = harness.open_file("degraded.pl", source);
            recorder.check("didOpen accepted without perl", open_result.is_ok())?;
            open_result.context("didOpen should succeed")?;
            let hover_result = harness.hover("degraded.pl", 0, 3);
            recorder.check("hover transport completed without perl", hover_result.is_ok())?;
            hover_result.context("hover should not return a transport error in degraded mode")?;

            harness.assert_no_crash();
            Ok(())
        },
    );
}

#[test]
fn scenario_03_interpreter_resolution_message_is_once_per_session_with_remediation() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_03_interpreter_resolution_message_is_once_per_session_with_remediation",
        UxCiTier::Pr,
        Some(UxComponent::Infra),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness = UxHarness::new(config_without_perl())
                .context("Failed to create UX harness without perl")?;

            // Two harness opens in ONE server session. The resolution message is
            // emitted during `initialize` (before the initialize response), so it
            // is already buffered by the time `UxHarness::new` returns; the opens
            // exist to prove a per-open re-emission cannot happen.
            harness
                .open_file("session_first.pl", "my $x = 1;\n")
                .context("first didOpen should succeed without perl")?;
            harness
                .open_file("session_second.pl", "my $y = 2;\n")
                .context("second didOpen should succeed without perl")?;

            // Event-driven settle: wait until the resolution message has actually
            // been observed, so the count below reads a completed emission.
            let (not_found, fallback) = harness
                .client
                .wait_for_events(RESOLUTION_WAIT, |events| {
                    let (not_found, fallback) = interpreter_resolution_counts(events);
                    (not_found + fallback > 0).then_some((not_found, fallback))
                })
                .map_err(|end| {
                    anyhow::anyhow!(
                        "no perl interpreter resolution message arrived in session: {end:?}"
                    )
                })?;

            // Hover round-trip proves the server processed both didOpens before
            // the final count, so a per-open duplicate would be in the buffer.
            harness
                .hover("session_second.pl", 0, 3)
                .context("hover round-trip after both opens should complete")?;

            let events = harness.client.peek_events();
            let (not_found, fallback) = {
                let (after_hover_not_found, after_hover_fallback) =
                    interpreter_resolution_counts(&events);
                (after_hover_not_found.max(not_found), after_hover_fallback.max(fallback))
            };

            recorder.check(
                "interpreter resolution reached the user in the session",
                not_found + fallback > 0,
            )?;
            recorder
                .check("not-found message fired at most once across two opens", not_found <= 1)?;
            recorder
                .check("fallback message fired at most once across two opens", fallback <= 1)?;
            recorder.check(
                "exactly one resolution arm fired, exactly once",
                not_found + fallback == 1,
            )?;

            recorder.check(
                "every fallback resolution message is an Info log",
                events.iter().all(|event| match event {
                    LspEvent::LogMessage { message_type, message }
                        if message.contains(PERL_FALLBACK_IN_USE) =>
                    {
                        *message_type == 3
                    }
                    LspEvent::WindowMessage { message, .. }
                        if message.contains(PERL_FALLBACK_IN_USE) =>
                    {
                        false
                    }
                    _ => true,
                }),
            )?;

            let mut remediation_present = true;
            for event in &events {
                if let LspEvent::WindowMessage { message, .. }
                | LspEvent::LogMessage { message, .. } = event
                    && message.contains(PERL_MISSING_ON_PATH)
                {
                    remediation_present &= message.contains(STRAWBERRY_PERL);
                }
            }
            recorder.check(
                "every not-found resolution message is an Error popup",
                events.iter().all(|event| match event {
                    LspEvent::WindowMessage { message_type, message }
                        if message.contains(PERL_MISSING_ON_PATH) =>
                    {
                        *message_type == 1
                    }
                    LspEvent::LogMessage { message, .. }
                        if message.contains(PERL_MISSING_ON_PATH) =>
                    {
                        false
                    }
                    _ => true,
                }),
            )?;
            recorder.check(
                "every not-found warning carries the canonical remediation substring",
                remediation_present,
            )?;

            harness.assert_no_crash();
            Ok(())
        },
    );
}

#[test]
fn scenario_03_degraded_mode_completion_does_not_crash() {
    run_ux_scenario_with_evidence_class(
        WORKFLOW_ID,
        SCENARIO_FILE,
        "scenario_03_degraded_mode_completion_does_not_crash",
        UxCiTier::Pr,
        Some(UxComponent::Completion),
        UxEvidenceClass::TransportCharacterization,
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let source = "use str\n";
            let harness = UxHarness::new(config_without_perl())
                .context("Failed to create UX harness without perl")?;

            let open_result = harness.open_file("complete.pl", source);
            recorder.check("didOpen accepted without perl", open_result.is_ok())?;
            open_result.context("didOpen should succeed")?;
            let completion_result = harness.completion("complete.pl", 0, 7);
            recorder
                .check("completion transport completed without perl", completion_result.is_ok())?;
            completion_result
                .context("completion should not return a transport error in degraded mode")?;

            harness.assert_no_crash();
            Ok(())
        },
    );
}
