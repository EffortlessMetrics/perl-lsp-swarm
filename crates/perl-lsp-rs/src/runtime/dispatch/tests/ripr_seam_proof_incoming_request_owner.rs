use super::*;
use serde_json::json;
use std::io;
use std::sync::Barrier;
use std::thread;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

fn join<T>(handle: thread::JoinHandle<T>) -> TestResult<T> {
    handle.join().map_err(|_| io::Error::other("request lifecycle race thread panicked").into())
}

fn error(code: i32, message: &str) -> JsonRpcError {
    JsonRpcError { code, message: message.to_string(), data: None }
}

#[test]
fn ripr_seam_proof_notifications_bypass_without_consuming_capacity() -> TestResult {
    let registry = IncomingRequestRegistry::new(1)?;
    let notification = registry.admit_optional(None, "textDocument/didOpen")?;
    assert!(notification.is_none());
    assert_eq!(registry.active_count(), 0);
    assert_eq!(registry.counters().notifications_bypassed, 1);
    Ok(())
}

#[test]
fn ripr_seam_proof_numeric_and_string_ids_are_distinct() -> TestResult {
    let registry = IncomingRequestRegistry::new(2)?;
    let numeric = registry.admit(JsonRpcId::Integer(1), "textDocument/hover")?;
    let string = registry.admit(JsonRpcId::String("1".to_string()), "textDocument/hover")?;
    assert_eq!(registry.active_count(), 2);
    assert_ne!(numeric.id, string.id);

    let (numeric_disposition, _) = registry.select_result(&numeric.id, Value::Null);
    let (string_disposition, _) = registry.select_error(&string.id, error(-32800, "cancelled"));
    assert_eq!(numeric_disposition, TerminalSelectionDisposition::Selected);
    assert_eq!(string_disposition, TerminalSelectionDisposition::Selected);
    assert_eq!(registry.response_written(&numeric.id), ResponseWriteDisposition::WrittenAndCleaned);
    assert_eq!(registry.response_written(&string.id), ResponseWriteDisposition::WrittenAndCleaned);
    assert_eq!(registry.active_count(), 0);
    Ok(())
}

#[test]
fn ripr_seam_proof_supported_identities_round_trip_without_the_unsupported_path() -> TestResult {
    for id in [JsonRpcId::Integer(-7), JsonRpcId::String("7".to_string())] {
        let key = IncomingRequestKey::from_id(&id)
            .ok_or("supported wire identity must project into a registry key")?;
        assert_eq!(key.to_id(), id, "registry key must round-trip its wire identity");
    }

    // A legitimate request must never travel the fail-closed identity path,
    // so the counter stays at its baseline across a whole lifecycle.
    let registry = IncomingRequestRegistry::new(2)?;
    let handle = registry.admit(JsonRpcId::Integer(-7), "textDocument/hover")?;
    let (disposition, _) = registry.select_result(&handle.id, Value::Null);
    assert_eq!(disposition, TerminalSelectionDisposition::Selected);
    assert_eq!(registry.response_written(&handle.id), ResponseWriteDisposition::WrittenAndCleaned);
    assert_eq!(registry.counters().unsupported_id, 0);
    assert!(
        registry
            .anomalies()
            .iter()
            .all(|anomaly| anomaly.kind != IncomingRequestAnomalyKind::UnsupportedId),
        "a supported identity must not raise an unsupported-identity anomaly"
    );
    Ok(())
}

#[test]
fn ripr_seam_proof_capacity_rejects_before_an_unowned_request_is_accepted() -> TestResult {
    let registry = IncomingRequestRegistry::new(1)?;
    let first = registry.admit(JsonRpcId::Integer(1), "textDocument/completion")?;
    let second = registry.admit(JsonRpcId::Integer(2), "textDocument/hover");
    assert!(matches!(second, Err(IncomingRequestRegistryError::CapacityExhausted { capacity: 1 })));
    assert_eq!(registry.counters().capacity_rejected, 1);
    let _ = registry.select_error(&first.id, error(-32099, "overload test cleanup"));
    assert_eq!(registry.response_written(&first.id), ResponseWriteDisposition::WrittenAndCleaned);
    Ok(())
}

#[test]
fn ripr_seam_proof_every_lifecycle_step_moves_the_observable_counters_and_clock() -> TestResult {
    // The dispositions alone are a weak oracle: a registry could return the
    // right disposition while mis-accounting its own state. This pins the
    // gauges, the cumulative totals, and the phase clock at each step, so a
    // transition that forgets to move one of them fails here.
    let registry = IncomingRequestRegistry::new(4)?;
    let before = registry.counters();
    assert_eq!(before.admitted_total, 0);
    assert_eq!(before.active, 0);
    assert_eq!(before.capacity, 4);

    let handle = registry.admit(JsonRpcId::Integer(41), "textDocument/references")?;
    let accepted = registry.counters();
    assert_eq!(accepted.admitted_total, 1, "admission is cumulative");
    assert_eq!(accepted.active, 1);
    assert_eq!(accepted.accepted, 1, "a fresh request sits in Accepted");
    assert_eq!(accepted.queued, 0);
    assert_eq!(accepted.running, 0);

    let admitted_at =
        registry.snapshots().into_iter().next().ok_or("missing snapshot after admission")?;
    assert_eq!(admitted_at.phase, IncomingRequestPhase::Accepted);
    assert_eq!(
        admitted_at.accepted_at, admitted_at.phase_changed_at,
        "phase clock starts at the admission instant"
    );
    assert!(admitted_at.terminal_kind.is_none());

    assert_eq!(registry.mark_queued(&handle.id), PhaseTransitionDisposition::Advanced);
    let queued = registry.counters();
    assert_eq!(queued.accepted, 0, "the gauge moves rather than accumulating");
    assert_eq!(queued.queued, 1);
    assert_eq!(queued.phase_advanced, 1);

    assert_eq!(registry.mark_running(&handle.id), PhaseTransitionDisposition::Advanced);
    let running = registry.counters();
    assert_eq!(running.queued, 0);
    assert_eq!(running.running, 1);
    assert_eq!(running.phase_advanced, 2);

    let advanced =
        registry.snapshots().into_iter().next().ok_or("missing snapshot after advancing")?;
    assert_eq!(advanced.accepted_at, admitted_at.accepted_at, "admission instant is stable");
    assert!(
        advanced.phase_changed_at >= admitted_at.phase_changed_at,
        "the phase clock must not run backwards across a transition"
    );

    let (disposition, selected) = registry.select_result(&handle.id, json!({"ok": true}));
    assert_eq!(disposition, TerminalSelectionDisposition::Selected);
    let selected = selected.ok_or("selected terminal must be returned to the caller")?;
    assert_eq!(selected.method, "textDocument/references");
    assert!(
        selected.selected_at >= selected.accepted_at,
        "a terminal cannot be selected before its request was accepted"
    );
    let terminal = registry.counters();
    assert_eq!(terminal.terminal_selected_total, 1);
    assert_eq!(terminal.result_selected, 1);
    assert_eq!(terminal.error_selected, 0);
    assert_eq!(terminal.shutdown_selected, 0);
    assert_eq!(terminal.terminal_selected_active, 1, "still owned until the response is written");
    assert_eq!(terminal.running, 0);
    assert_eq!(terminal.active, 1);

    assert_eq!(registry.response_written(&handle.id), ResponseWriteDisposition::WrittenAndCleaned);
    let done = registry.counters();
    assert_eq!(done.responses_written, 1);
    assert_eq!(done.response_write_failed, 0);
    assert_eq!(done.terminal_selected_active, 0);
    assert_eq!(done.active, 0);
    assert_eq!(done.admitted_total, 1, "cumulative totals survive cleanup");
    assert_eq!(done.duplicate_terminal, 0);
    assert_eq!(done.unknown_request, 0);
    assert_eq!(done.transport_cleaned, 0);
    assert!(registry.snapshots().is_empty(), "cleanup removes the active entry");
    assert!(
        registry.anomalies().is_empty(),
        "a clean lifecycle must raise no anomaly: {:?}",
        registry.anomalies()
    );
    Ok(())
}

#[test]
fn ripr_seam_proof_phase_progression_is_monotonic() -> TestResult {
    let registry = IncomingRequestRegistry::new(2)?;
    let handle = registry.admit(JsonRpcId::Integer(7), "workspace/symbol")?;
    assert_eq!(registry.mark_queued(&handle.id), PhaseTransitionDisposition::Advanced);
    assert_eq!(registry.mark_running(&handle.id), PhaseTransitionDisposition::Advanced);
    assert_eq!(registry.mark_queued(&handle.id), PhaseTransitionDisposition::Unchanged);
    let snapshot = registry.snapshots().into_iter().next().ok_or("missing snapshot")?;
    assert_eq!(snapshot.phase, IncomingRequestPhase::Running);
    assert!(
        registry
            .anomalies()
            .iter()
            .any(|anomaly| { anomaly.kind == IncomingRequestAnomalyKind::InvalidPhaseRegression })
    );
    let _ = registry.select_result(&handle.id, json!([]));
    assert_eq!(registry.response_written(&handle.id), ResponseWriteDisposition::WrittenAndCleaned);
    Ok(())
}

#[test]
fn ripr_seam_proof_terminal_is_selected_once_and_cleanup_is_explicit() -> TestResult {
    let registry = IncomingRequestRegistry::new(2)?;
    let handle = registry.admit(JsonRpcId::Integer(11), "textDocument/definition")?;
    let (first, selected) = registry.select_result(&handle.id, json!({"uri": "file:///a.pm"}));
    assert_eq!(first, TerminalSelectionDisposition::Selected);
    assert!(selected.is_some());
    let (second, duplicate) = registry.select_error(&handle.id, error(-32800, "too late"));
    assert_eq!(second, TerminalSelectionDisposition::AlreadyTerminal);
    assert!(duplicate.is_none());
    assert_eq!(registry.active_count(), 1, "selection alone must not hide write leaks");
    assert_eq!(registry.response_written(&handle.id), ResponseWriteDisposition::WrittenAndCleaned);
    let (third, _) = registry.select_result(&handle.id, Value::Null);
    assert_eq!(third, TerminalSelectionDisposition::AlreadyTerminal);
    assert_eq!(registry.counters().duplicate_terminal, 2);
    Ok(())
}

#[test]
fn ripr_seam_proof_response_before_terminal_does_not_drop_the_request() -> TestResult {
    let registry = IncomingRequestRegistry::new(1)?;
    let handle = registry.admit(JsonRpcId::Integer(14), "textDocument/references")?;
    assert_eq!(
        registry.response_written(&handle.id),
        ResponseWriteDisposition::TerminalNotSelected
    );
    assert_eq!(registry.active_count(), 1);
    let _ = registry.select_result(&handle.id, json!([]));
    assert_eq!(registry.response_written(&handle.id), ResponseWriteDisposition::WrittenAndCleaned);
    Ok(())
}

#[test]
fn ripr_seam_proof_response_write_failure_is_distinct_and_cleans_once() -> TestResult {
    let registry = IncomingRequestRegistry::new(1)?;
    let handle =
        registry.admit(JsonRpcId::String("write-failure".to_string()), "textDocument/rename")?;
    let _ = registry.select_error(&handle.id, error(-32603, "internal error"));
    assert_eq!(
        registry.response_write_failed(&handle.id, "broken pipe"),
        ResponseWriteDisposition::WriteFailedAndCleaned
    );
    assert_eq!(registry.active_count(), 0);
    assert_eq!(registry.counters().response_write_failed, 1);
    assert_eq!(registry.response_written(&handle.id), ResponseWriteDisposition::AlreadyCleaned);
    Ok(())
}

#[test]
fn ripr_seam_proof_result_and_shutdown_race_select_exactly_one_terminal() -> TestResult {
    let registry = IncomingRequestRegistry::new(2)?;
    let handle = registry.admit(JsonRpcId::Integer(21), "workspace/symbol")?;
    let barrier = Arc::new(Barrier::new(3));

    let result_registry = registry.clone();
    let result_barrier = Arc::clone(&barrier);
    let result_id = handle.id.clone();
    let result_thread = thread::spawn(move || {
        result_barrier.wait();
        result_registry.select_result(&result_id, json!([])).0
    });

    let shutdown_registry = registry.clone();
    let shutdown_barrier = Arc::clone(&barrier);
    let shutdown_thread = thread::spawn(move || {
        shutdown_barrier.wait();
        shutdown_registry.select_shutdown_errors(-32097, "server shutdown")
    });

    barrier.wait();
    let result_disposition = join(result_thread)?;
    let shutdown_terminals = join(shutdown_thread)?;
    let selected_count = usize::from(result_disposition == TerminalSelectionDisposition::Selected)
        + shutdown_terminals.len();
    assert_eq!(selected_count, 1);
    assert_eq!(registry.counters().terminal_selected_total, 1);
    assert_eq!(registry.response_written(&handle.id), ResponseWriteDisposition::WrittenAndCleaned);
    Ok(())
}

#[test]
fn ripr_seam_proof_transport_loss_cleans_every_active_phase() -> TestResult {
    let registry = IncomingRequestRegistry::new(4)?;
    let accepted = registry.admit(JsonRpcId::Integer(31), "accepted")?;
    let queued = registry.admit(JsonRpcId::Integer(32), "queued")?;
    let running = registry.admit(JsonRpcId::Integer(33), "running")?;
    let terminal = registry.admit(JsonRpcId::Integer(34), "terminal")?;
    let _ = registry.mark_queued(&queued.id);
    let _ = registry.mark_running(&running.id);
    let _ = registry.select_error(&terminal.id, error(-32800, "cancelled"));
    assert_eq!(registry.transport_lost(), 4);
    assert_eq!(registry.active_count(), 0);
    assert_eq!(registry.counters().transport_cleaned, 4);
    assert!(
        registry
            .anomalies()
            .iter()
            .filter(|anomaly| { anomaly.kind == IncomingRequestAnomalyKind::TransportCleanup })
            .count()
            >= 4
    );
    let _ = accepted;
    Ok(())
}

#[test]
fn ripr_seam_proof_duplicate_admission_is_rejected_without_replacing_owner() -> TestResult {
    let registry = IncomingRequestRegistry::new(2)?;
    let handle = registry.admit(JsonRpcId::String("same".to_string()), "first")?;
    let duplicate = registry.admit(JsonRpcId::String("same".to_string()), "second");
    assert!(matches!(duplicate, Err(IncomingRequestRegistryError::DuplicateId { .. })));
    let snapshot = registry.snapshots().into_iter().next().ok_or("missing owner")?;
    assert_eq!(snapshot.method, "first");
    let _ = registry.select_result(&handle.id, Value::Null);
    let _ = registry.response_written(&handle.id);
    Ok(())
}

#[test]
fn ripr_seam_proof_anomaly_storage_is_bounded() -> TestResult {
    let registry = IncomingRequestRegistry::new(1)?;
    for index in 0..(MAX_ANOMALIES + 9) {
        let id = JsonRpcId::Integer(i64::try_from(index)? + 10_000);
        assert_eq!(registry.mark_running(&id), PhaseTransitionDisposition::Unknown);
    }
    assert_eq!(registry.anomalies().len(), MAX_ANOMALIES);
    assert_eq!(registry.counters().anomalies_dropped, 9);
    Ok(())
}

#[test]
fn ripr_seam_proof_invalid_capacity_is_rejected_before_any_owner() -> TestResult {
    for requested in [0, MAX_INCOMING_REQUEST_CAPACITY + 1] {
        let Err(error) = IncomingRequestRegistry::new(requested) else {
            return Err("invalid capacity must fail closed before an owner exists".into());
        };
        assert!(matches!(
            error,
            IncomingRequestRegistryError::InvalidCapacity {
                requested: got,
                maximum: MAX_INCOMING_REQUEST_CAPACITY,
            } if got == requested
        ));
        assert!(error.to_string().contains("outside 1..="));
    }
    Ok(())
}

#[test]
fn ripr_seam_proof_unknown_id_does_not_create_an_owner() -> TestResult {
    let registry = IncomingRequestRegistry::new(1)?;
    let missing = JsonRpcId::Integer(99);
    assert_eq!(registry.mark_running(&missing), PhaseTransitionDisposition::Unknown);
    let (disposition, selected) = registry.select_result(&missing, Value::Null);
    assert_eq!(disposition, TerminalSelectionDisposition::Unknown);
    assert!(selected.is_none());
    assert_eq!(registry.response_written(&missing), ResponseWriteDisposition::Unknown);
    assert_eq!(registry.active_count(), 0);
    assert_eq!(registry.counters().unknown_request, 3);
    Ok(())
}

#[test]
fn ripr_seam_proof_method_text_is_bounded() -> TestResult {
    let registry = IncomingRequestRegistry::new(1)?;
    let oversized = "x".repeat(MAX_METHOD_BYTES + 40);
    let handle = registry.admit(JsonRpcId::Integer(3), oversized)?;
    assert_eq!(handle.method.len(), MAX_METHOD_BYTES);
    assert!(handle.method.ends_with("..."));
    let _ = registry.select_result(&handle.id, Value::Null);
    let _ = registry.response_written(&handle.id);
    Ok(())
}
