// The hermetic runner substrate is consumed progressively by the sibling
// actual-host verdict targets (#7126/#7721/#7727); this contract target
// exercises only the schema/event/receipt core, so under-consumed substrate
// items are expected rather than drift.
#![allow(dead_code)]
#![allow(unused_imports)]

#[path = "support/emacs_host_runner.rs"]
mod emacs_host_runner;

use anyhow::{Result, ensure};
use emacs_host_runner::{
    DRIVER_SCHEMA_VERSION, DriverEvent, DriverEventKind, RUN_PLAN_SCHEMA_VERSION,
    default_not_proven_diagnostics, validate_driver_events,
};
use std::collections::BTreeMap;
use xtask::editor_client_compat::{DiagnosticMode, DiagnosticsIdentity};

fn event(sequence: u64, kind: DriverEventKind) -> DriverEvent {
    DriverEvent {
        schema_version: DRIVER_SCHEMA_VERSION.to_string(),
        sequence,
        kind,
        details: BTreeMap::new(),
    }
}

/// `validate_driver_events` pairs host actions by their `action_id` detail, so
/// an action event without one is rejected before any ordering rule is reached.
/// Building the accepted trace without these details did not merely fail the
/// positive test: it also made every negative test below pass for the wrong
/// reason, since a missing `action_id` rejects the trace on its own.
fn action_event(sequence: u64, kind: DriverEventKind, action_id: &str) -> DriverEvent {
    let mut observation = event(sequence, kind);
    observation.details.insert("action_id".to_string(), action_id.to_string());
    observation
}

fn complete_events() -> Vec<DriverEvent> {
    vec![
        event(1, DriverEventKind::HostStarted),
        event(2, DriverEventKind::ClientLoaded),
        event(3, DriverEventKind::RegistrationSelected),
        event(4, DriverEventKind::InitializeObserved),
        event(5, DriverEventKind::WorkspaceReady),
        event(6, DriverEventKind::BufferOpened),
        action_event(7, DriverEventKind::HostActionStarted, "rename_module"),
        action_event(8, DriverEventKind::HostActionCompleted, "rename_module"),
        event(9, DriverEventKind::EditApplied),
        event(10, DriverEventKind::ShutdownStarted),
        event(11, DriverEventKind::ShutdownCompleted),
    ]
}

#[test]
fn runner_contract_uses_versioned_run_and_driver_schemas() {
    assert_eq!(RUN_PLAN_SCHEMA_VERSION, "emacs_host_run_plan.v1");
    assert_eq!(DRIVER_SCHEMA_VERSION, "emacs_host_driver.v1");
}

#[test]
fn diagnostics_contract_uses_the_canonical_type() -> Result<()> {
    let diagnostics: DiagnosticsIdentity = default_not_proven_diagnostics();
    ensure!(
        diagnostics.advertised_mode == DiagnosticMode::NotProven,
        "default diagnostics must fail closed"
    );
    ensure!(
        diagnostics.observed_messages.is_empty(),
        "not-proven diagnostics cannot manufacture observations"
    );
    Ok(())
}

#[test]
fn ordered_complete_driver_trace_is_accepted() -> Result<()> {
    validate_driver_events(&complete_events(), true)
}

#[test]
fn unidentified_and_mismatched_host_actions_are_rejected() {
    let mut missing_action_id = complete_events();
    missing_action_id[6].details.clear();
    assert!(validate_driver_events(&missing_action_id, true).is_err());

    let mut mismatched_action_id = complete_events();
    mismatched_action_id[7].details.insert("action_id".to_string(), "other_action".to_string());
    assert!(validate_driver_events(&mismatched_action_id, true).is_err());
}

#[test]
fn sequence_gap_and_missing_shutdown_are_rejected() {
    let mut sequence_gap = complete_events();
    sequence_gap[3].sequence = 99;
    assert!(validate_driver_events(&sequence_gap, true).is_err());

    let mut missing_shutdown = complete_events();
    missing_shutdown.pop();
    assert!(validate_driver_events(&missing_shutdown, true).is_err());
}

#[test]
fn lifecycle_reordering_and_unclosed_action_are_rejected() {
    let mut reordered = complete_events();
    reordered.swap(1, 2);
    for (index, observation) in reordered.iter_mut().enumerate() {
        observation.sequence = (index + 1) as u64;
    }
    assert!(validate_driver_events(&reordered, true).is_err());

    let mut unclosed_action = complete_events();
    unclosed_action.remove(7);
    for (index, observation) in unclosed_action.iter_mut().enumerate() {
        observation.sequence = (index + 1) as u64;
    }
    assert!(validate_driver_events(&unclosed_action, true).is_err());
}

#[test]
fn schema_drift_is_rejected() {
    let mut drifted = complete_events();
    drifted[0].schema_version = "emacs_host_driver.v2".to_string();
    assert!(validate_driver_events(&drifted, true).is_err());
}
