//! Fixtures and negative controls required by #16895.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use perl_test_must::{must_err_with, must_with};
use serde_json::{Value, json};

fn token(raw: &str) -> LogicalToken {
    must_with(LogicalToken::parse(raw), format!("token `{raw}`"))
}

fn subject(root: &str, runtime: &str, configuration: &str) -> WorkspaceLifecycleSubject {
    WorkspaceLifecycleSubject::new(
        token("session-1"),
        token("workspace-1"),
        token(root),
        token(runtime),
        token(configuration),
        token("trust-1"),
        token("env-1"),
    )
}

fn provenance(
    subject: &WorkspaceLifecycleSubject,
    source: &str,
    domain: &str,
) -> DimensionProvenance {
    DimensionProvenance::new(
        subject.session_id().clone(),
        subject.root_id().clone(),
        subject.runtime_generation().clone(),
        token(source),
        token(domain),
    )
}

fn observed<T>(
    subject: &WorkspaceLifecycleSubject,
    source: &str,
    domain: &str,
    value: T,
) -> StatusDimension<T> {
    StatusDimension::Observed { provenance: provenance(subject, source, domain), value }
}

fn metadata_complete() -> StatusMetadata {
    must_with(
        StatusMetadata::new(
            Completeness::Complete,
            InstrumentState::Ok,
            PrivacyClass::PublicSafe,
            0,
        ),
        "complete metadata",
    )
}

fn metadata_partial() -> StatusMetadata {
    must_with(
        StatusMetadata::new(
            Completeness::Partial,
            InstrumentState::Ok,
            PrivacyClass::PublicSafe,
            0,
        ),
        "partial metadata",
    )
}

fn repair_none() -> RepairProjection {
    RepairProjection::new(
        token("reason.none"),
        ResponsibleOwnerRole::RootLifecycle,
        PrimaryActionKind::NoneRequired,
        None,
    )
}

fn readiness_all(
    subject: &WorkspaceLifecycleSubject,
    state: ReadinessState,
) -> ReadinessObservation {
    ReadinessObservation::new(
        observed(subject, "readiness.active", "ready-gen-1", state),
        observed(subject, "readiness.neighborhood", "ready-gen-1", state),
        observed(subject, "readiness.workspace", "ready-gen-1", state),
    )
}

fn current_ready_parts() -> WorkspaceLifecycleStatusRowParts {
    let subject = subject("root-a", "runtime-1", "config-1");
    WorkspaceLifecycleStatusRowParts {
        root_lifecycle: observed(
            &subject,
            "root.lifecycle",
            "runtime-1",
            RootLifecycleState::Active,
        ),
        configuration: observed(
            &subject,
            "config.authority",
            "config-1",
            ConfigurationState::Valid,
        ),
        watch_reload: observed(&subject, "watch.transport", "reload-1", WatchReloadState::Accepted),
        readiness: readiness_all(&subject, ReadinessState::Ready),
        storage: observed(&subject, "store.backend", "store-1", StorageState::Available),
        hydration: observed(
            &subject,
            "hydration.owner",
            "hydrate-1",
            HydrationState::AdoptedComplete,
        ),
        provider_posture: observed(
            &subject,
            "provider.exact",
            "provider-1",
            ProviderPosture::ExactCurrent,
        ),
        repair: repair_none(),
        reasons: vec![token("reason.ready")],
        limitations: Vec::new(),
        operation_links: vec![token("op.1")],
        metadata: metadata_complete(),
        subject,
    }
}

fn row_from(parts: WorkspaceLifecycleStatusRowParts) -> WorkspaceLifecycleStatusRow {
    must_with(WorkspaceLifecycleStatusRow::from_parts(parts), "row constructs")
}

fn envelope_from(rows: Vec<WorkspaceLifecycleStatusRow>) -> WorkspaceLifecycleStatusV1 {
    must_with(
        WorkspaceLifecycleStatusV1::from_parts(WorkspaceLifecycleStatusParts {
            producer: ProducerIdentity::new(token("perl-lsp-rs-core")),
            primary_action_priority_policy: PrimaryActionPriorityPolicy::CanonicalV1,
            rows,
        }),
        "envelope constructs",
    )
}

fn round_trip(status: &WorkspaceLifecycleStatusV1) -> WorkspaceLifecycleStatusV1 {
    let json = must_with(serde_json::to_string(status), "serialize");
    must_with(serde_json::from_str(&json), format!("round-trip\n{json}"))
}

fn healthy_cold_startup() -> WorkspaceLifecycleStatusRowParts {
    let mut parts = current_ready_parts();
    parts.root_lifecycle =
        observed(&parts.subject, "root.lifecycle", "runtime-1", RootLifecycleState::Creating);
    parts.watch_reload =
        observed(&parts.subject, "watch.transport", "reload-1", WatchReloadState::Unavailable);
    parts.readiness = ReadinessObservation::new(
        observed(&parts.subject, "readiness.active", "ready-gen-1", ReadinessState::Pending),
        observed(&parts.subject, "readiness.neighborhood", "ready-gen-1", ReadinessState::NotReady),
        observed(&parts.subject, "readiness.workspace", "ready-gen-1", ReadinessState::NotReady),
    );
    parts.storage = observed(&parts.subject, "store.backend", "store-1", StorageState::Disabled);
    parts.hydration =
        observed(&parts.subject, "hydration.owner", "hydrate-1", HydrationState::NotAttempted);
    parts.provider_posture =
        observed(&parts.subject, "provider.exact", "provider-1", ProviderPosture::NotReady);
    parts.repair = RepairProjection::new(
        token("reason.cold_start"),
        ResponsibleOwnerRole::Storage,
        PrimaryActionKind::NoneRequired,
        None,
    );
    parts.reasons = vec![token("reason.cache_disabled")];
    parts.metadata = metadata_partial();
    parts
}

fn ready_limited_parts() -> WorkspaceLifecycleStatusRowParts {
    let mut parts = current_ready_parts();
    parts.root_lifecycle =
        observed(&parts.subject, "root.lifecycle", "runtime-1", RootLifecycleState::Limited);
    parts.readiness = readiness_all(&parts.subject, ReadinessState::ReadyLimited);
    parts.provider_posture =
        observed(&parts.subject, "provider.exact", "provider-1", ProviderPosture::BoundedCurrent);
    parts.repair = RepairProjection::new(
        token("reason.ready_limited"),
        ResponsibleOwnerRole::Readiness,
        PrimaryActionKind::NoneRequired,
        None,
    );
    parts.limitations = vec![token("limit.bounded_index")];
    parts.metadata = metadata_partial();
    parts
}

fn invalid_config_not_ready_parts() -> WorkspaceLifecycleStatusRowParts {
    let mut parts = current_ready_parts();
    parts.configuration = observed(
        &parts.subject,
        "config.authority",
        "config-1",
        ConfigurationState::InvalidCurrent,
    );
    parts.watch_reload =
        observed(&parts.subject, "watch.transport", "reload-1", WatchReloadState::Invalidated);
    parts.provider_posture =
        observed(&parts.subject, "provider.exact", "provider-1", ProviderPosture::NotReady);
    parts.readiness = readiness_all(&parts.subject, ReadinessState::NotReady);
    parts.repair = RepairProjection::new(
        token("reason.config.invalid_current"),
        ResponsibleOwnerRole::Configuration,
        PrimaryActionKind::RepairConfiguration,
        None,
    );
    parts.reasons = vec![token("reason.config.invalid_current")];
    parts.metadata = metadata_partial();
    parts
}

fn reload_pending_invalidated_parts() -> WorkspaceLifecycleStatusRowParts {
    let mut parts = current_ready_parts();
    parts.configuration =
        observed(&parts.subject, "config.authority", "config-1", ConfigurationState::Reloading);
    parts.watch_reload =
        observed(&parts.subject, "watch.transport", "reload-1", WatchReloadState::Pending);
    parts.provider_posture =
        observed(&parts.subject, "provider.exact", "provider-1", ProviderPosture::NotReady);
    parts.repair = RepairProjection::new(
        token("reason.reload.pending"),
        ResponsibleOwnerRole::Reload,
        PrimaryActionKind::WaitForReload,
        None,
    );
    parts.metadata = metadata_partial();
    parts
}

fn store_incompatible_parts() -> WorkspaceLifecycleStatusRowParts {
    let mut parts = current_ready_parts();
    parts.storage = observed(&parts.subject, "store.backend", "store-1", StorageState::Available);
    parts.hydration =
        observed(&parts.subject, "hydration.owner", "hydrate-1", HydrationState::Rejected);
    parts.readiness = ReadinessObservation::new(
        observed(&parts.subject, "readiness.active", "ready-gen-1", ReadinessState::Ready),
        observed(&parts.subject, "readiness.neighborhood", "ready-gen-1", ReadinessState::Pending),
        observed(&parts.subject, "readiness.workspace", "ready-gen-1", ReadinessState::NotReady),
    );
    parts.limitations = vec![token("limit.snapshot_incompatible")];
    parts.repair = RepairProjection::new(
        token("reason.hydration.rejected"),
        ResponsibleOwnerRole::Hydration,
        PrimaryActionKind::WaitForIndex,
        None,
    );
    parts.metadata = metadata_partial();
    parts
}

fn partial_hydration_indexing_parts() -> WorkspaceLifecycleStatusRowParts {
    let mut parts = current_ready_parts();
    parts.hydration =
        observed(&parts.subject, "hydration.owner", "hydrate-1", HydrationState::AdoptedPartial);
    parts.readiness = ReadinessObservation::new(
        observed(&parts.subject, "readiness.active", "ready-gen-1", ReadinessState::Ready),
        observed(&parts.subject, "readiness.neighborhood", "ready-gen-1", ReadinessState::Pending),
        observed(&parts.subject, "readiness.workspace", "ready-gen-1", ReadinessState::Pending),
    );
    parts.repair = RepairProjection::new(
        token("reason.index.pending"),
        ResponsibleOwnerRole::Readiness,
        PrimaryActionKind::WaitForIndex,
        None,
    );
    parts.metadata = metadata_partial();
    parts
}

fn busy_store_cold_fallback_parts() -> WorkspaceLifecycleStatusRowParts {
    let mut parts = current_ready_parts();
    parts.storage = observed(&parts.subject, "store.backend", "store-1", StorageState::Busy);
    parts.hydration =
        observed(&parts.subject, "hydration.owner", "hydrate-1", HydrationState::ColdFallback);
    parts.repair = RepairProjection::new(
        token("reason.storage.busy_cold"),
        ResponsibleOwnerRole::Storage,
        PrimaryActionKind::NoneRequired,
        Some(LocalDiagnosticAction::InspectStorage),
    );
    parts.metadata = metadata_partial();
    parts
}

fn quarantined_rebuild_parts() -> WorkspaceLifecycleStatusRowParts {
    let mut parts = current_ready_parts();
    parts.storage = observed(&parts.subject, "store.backend", "store-1", StorageState::Quarantined);
    parts.hydration =
        observed(&parts.subject, "hydration.owner", "hydrate-1", HydrationState::ColdFallback);
    parts.repair = RepairProjection::new(
        token("reason.storage.quarantined"),
        ResponsibleOwnerRole::Storage,
        PrimaryActionKind::ExportQuarantinedCache,
        Some(LocalDiagnosticAction::ExportDiagnostic),
    );
    parts.metadata = metadata_partial();
    parts
}

fn root_removal_pending_parts() -> WorkspaceLifecycleStatusRowParts {
    let mut parts = current_ready_parts();
    parts.root_lifecycle =
        observed(&parts.subject, "root.lifecycle", "runtime-1", RootLifecycleState::Removing);
    parts.watch_reload =
        observed(&parts.subject, "watch.transport", "reload-1", WatchReloadState::Pending);
    parts.operation_links = vec![token("op.reload"), token("op.index")];
    parts.repair = RepairProjection::new(
        token("reason.root.removing"),
        ResponsibleOwnerRole::RootLifecycle,
        PrimaryActionKind::NoneRequired,
        None,
    );
    parts.metadata = metadata_partial();
    parts
}

fn limited_root_parts() -> WorkspaceLifecycleStatusRowParts {
    let mut parts = ready_limited_parts();
    parts.subject = subject("root-b", "runtime-1", "config-1");
    parts.root_lifecycle =
        observed(&parts.subject, "root.lifecycle", "runtime-1", RootLifecycleState::Limited);
    parts.configuration =
        observed(&parts.subject, "config.authority", "config-1", ConfigurationState::Valid);
    parts.watch_reload =
        observed(&parts.subject, "watch.transport", "reload-1", WatchReloadState::Accepted);
    parts.readiness = readiness_all(&parts.subject, ReadinessState::ReadyLimited);
    parts.storage = observed(&parts.subject, "store.backend", "store-1", StorageState::Available);
    parts.hydration =
        observed(&parts.subject, "hydration.owner", "hydrate-1", HydrationState::AdoptedComplete);
    parts.provider_posture =
        observed(&parts.subject, "provider.exact", "provider-1", ProviderPosture::BoundedCurrent);
    parts
}

fn provider_posture_parts(posture: ProviderPosture) -> WorkspaceLifecycleStatusRowParts {
    let mut parts = current_ready_parts();
    parts.provider_posture = observed(&parts.subject, "provider.exact", "provider-1", posture);
    if posture == ProviderPosture::ExactCurrent {
        parts.metadata = metadata_complete();
    } else {
        parts.metadata = metadata_partial();
        parts.readiness = readiness_all(&parts.subject, ReadinessState::NotReady);
    }
    parts
}

fn instrument_failed_parts() -> WorkspaceLifecycleStatusRowParts {
    let mut parts = current_ready_parts();
    parts.hydration = StatusDimension::InstrumentFailed { instrument_id: token("hydrate.probe") };
    parts.metadata = must_with(
        StatusMetadata::new(
            Completeness::Partial,
            InstrumentState::Failed,
            PrivacyClass::PublicSafe,
            0,
        ),
        "instrument-failed metadata",
    );
    parts
}

fn serialize(status: &WorkspaceLifecycleStatusV1) -> Value {
    must_with(serde_json::to_value(status), "json value")
}

// ── Required fixtures ──────────────────────────────────────────────────────

#[test]
fn fixture_healthy_cold_startup_with_cache_disabled() {
    let status = envelope_from(vec![row_from(healthy_cold_startup())]);
    let row = &status.rows()[0];
    assert_eq!(row.storage().observed_value(), Some(StorageState::Disabled));
    assert_eq!(row.hydration().observed_value(), Some(HydrationState::NotAttempted));
    assert_eq!(row.provider_posture().observed_value(), Some(ProviderPosture::NotReady));
    assert_eq!(row.repair().primary_action(), PrimaryActionKind::NoneRequired);
    assert_eq!(round_trip(&status), status);
}

#[test]
fn fixture_current_ready_state() {
    let status = envelope_from(vec![row_from(current_ready_parts())]);
    let row = &status.rows()[0];
    assert_eq!(row.root_lifecycle().observed_value(), Some(RootLifecycleState::Active));
    assert_eq!(row.configuration().observed_value(), Some(ConfigurationState::Valid));
    assert_eq!(row.readiness().active_document().observed_value(), Some(ReadinessState::Ready));
    assert_eq!(row.provider_posture().observed_value(), Some(ProviderPosture::ExactCurrent));
    assert_eq!(row.metadata().completeness(), Completeness::Complete);
    assert_eq!(round_trip(&status), status);
}

#[test]
fn fixture_ready_limited_state() {
    let status = envelope_from(vec![row_from(ready_limited_parts())]);
    let row = &status.rows()[0];
    assert_eq!(row.root_lifecycle().observed_value(), Some(RootLifecycleState::Limited));
    assert_eq!(
        row.readiness().whole_workspace().observed_value(),
        Some(ReadinessState::ReadyLimited)
    );
    assert_eq!(row.provider_posture().observed_value(), Some(ProviderPosture::BoundedCurrent));
    assert_eq!(round_trip(&status), status);
}

#[test]
fn fixture_invalid_configuration_with_provider_not_ready() {
    let status = envelope_from(vec![row_from(invalid_config_not_ready_parts())]);
    let row = &status.rows()[0];
    assert_eq!(row.configuration().observed_value(), Some(ConfigurationState::InvalidCurrent));
    assert_eq!(row.provider_posture().observed_value(), Some(ProviderPosture::NotReady));
    assert_ne!(row.provider_posture().observed_value(), Some(ProviderPosture::ExactCurrent));
    assert_eq!(row.repair().primary_action(), PrimaryActionKind::RepairConfiguration);
    assert_eq!(round_trip(&status), status);
}

#[test]
fn fixture_reload_pending_with_old_exact_output_invalidated() {
    let status = envelope_from(vec![row_from(reload_pending_invalidated_parts())]);
    let row = &status.rows()[0];
    assert_eq!(row.watch_reload().observed_value(), Some(WatchReloadState::Pending));
    assert_ne!(row.watch_reload().observed_value(), Some(WatchReloadState::Accepted));
    assert_eq!(row.provider_posture().observed_value(), Some(ProviderPosture::NotReady));
    assert_eq!(round_trip(&status), status);
}

#[test]
fn fixture_store_available_but_candidate_incompatible() {
    let status = envelope_from(vec![row_from(store_incompatible_parts())]);
    let row = &status.rows()[0];
    assert_eq!(row.storage().observed_value(), Some(StorageState::Available));
    assert_eq!(row.hydration().observed_value(), Some(HydrationState::Rejected));
    assert_ne!(row.hydration().observed_value(), Some(HydrationState::AdoptedComplete));
    assert_eq!(round_trip(&status), status);
}

#[test]
fn fixture_partial_hydration_plus_indexing_work() {
    let status = envelope_from(vec![row_from(partial_hydration_indexing_parts())]);
    let row = &status.rows()[0];
    assert_eq!(row.hydration().observed_value(), Some(HydrationState::AdoptedPartial));
    assert_eq!(row.readiness().active_document().observed_value(), Some(ReadinessState::Ready));
    assert_eq!(row.readiness().whole_workspace().observed_value(), Some(ReadinessState::Pending));
    assert_eq!(round_trip(&status), status);
}

#[test]
fn fixture_busy_store_with_healthy_cold_fallback() {
    let status = envelope_from(vec![row_from(busy_store_cold_fallback_parts())]);
    let row = &status.rows()[0];
    assert_eq!(row.storage().observed_value(), Some(StorageState::Busy));
    assert_eq!(row.hydration().observed_value(), Some(HydrationState::ColdFallback));
    assert_eq!(row.repair().primary_action(), PrimaryActionKind::NoneRequired);
    assert_eq!(round_trip(&status), status);
}

#[test]
fn fixture_quarantined_cache_with_source_safe_rebuild() {
    let status = envelope_from(vec![row_from(quarantined_rebuild_parts())]);
    let row = &status.rows()[0];
    assert_eq!(row.storage().observed_value(), Some(StorageState::Quarantined));
    assert_eq!(row.hydration().observed_value(), Some(HydrationState::ColdFallback));
    assert_eq!(row.repair().primary_action(), PrimaryActionKind::ExportQuarantinedCache);
    assert_eq!(round_trip(&status), status);
}

#[test]
fn fixture_root_removal_with_pending_operations() {
    let status = envelope_from(vec![row_from(root_removal_pending_parts())]);
    let row = &status.rows()[0];
    assert_eq!(row.root_lifecycle().observed_value(), Some(RootLifecycleState::Removing));
    assert_eq!(row.operation_links().len(), 2);
    assert_eq!(round_trip(&status), status);
}

#[test]
fn fixture_one_healthy_and_one_limited_root() {
    let status =
        envelope_from(vec![row_from(current_ready_parts()), row_from(limited_root_parts())]);
    assert_eq!(status.rows().len(), 2);
    assert_eq!(status.rows()[0].subject().root_id().as_str(), "root-a");
    assert_eq!(status.rows()[1].subject().root_id().as_str(), "root-b");
    assert_eq!(
        status.rows()[0].root_lifecycle().observed_value(),
        Some(RootLifecycleState::Active)
    );
    assert_eq!(
        status.rows()[1].root_lifecycle().observed_value(),
        Some(RootLifecycleState::Limited)
    );
    assert_eq!(round_trip(&status), status);
}

#[test]
fn fixture_legitimate_empty_versus_not_ready_versus_refusal() {
    let empty =
        envelope_from(vec![row_from(provider_posture_parts(ProviderPosture::LegitimateEmpty))]);
    let not_ready =
        envelope_from(vec![row_from(provider_posture_parts(ProviderPosture::NotReady))]);
    let refusal =
        envelope_from(vec![row_from(provider_posture_parts(ProviderPosture::SafeRefusal))]);
    assert_ne!(empty.fingerprint(), not_ready.fingerprint());
    assert_ne!(empty.fingerprint(), refusal.fingerprint());
    assert_ne!(not_ready.fingerprint(), refusal.fingerprint());
    assert_eq!(
        empty.rows()[0].provider_posture().observed_value(),
        Some(ProviderPosture::LegitimateEmpty)
    );
}

#[test]
fn fixture_unknown_instrument_failed_dimension() {
    let status = envelope_from(vec![row_from(instrument_failed_parts())]);
    assert!(status.rows()[0].hydration().is_instrument_failed());
    assert_eq!(status.rows()[0].metadata().instrument_state(), InstrumentState::Failed);
    assert_ne!(
        status.rows()[0].hydration().observed_value(),
        Some(HydrationState::AdoptedComplete)
    );
    assert_eq!(round_trip(&status), status);
}

#[test]
fn fixture_mismatched_root_runtime_configuration_generations_fail_closed() {
    let mut parts = current_ready_parts();
    parts.configuration = StatusDimension::Observed {
        provenance: DimensionProvenance::new(
            parts.subject.session_id().clone(),
            parts.subject.root_id().clone(),
            parts.subject.runtime_generation().clone(),
            token("config.authority"),
            token("config-other"),
        ),
        value: ConfigurationState::Valid,
    };
    let err = must_err_with(WorkspaceLifecycleStatusRow::from_parts(parts), "mismatch must fail");
    assert_eq!(err.as_str(), "cross_generation_join");
}

#[test]
fn fixture_deterministic_construction_under_shuffled_input() {
    let first =
        envelope_from(vec![row_from(limited_root_parts()), row_from(current_ready_parts())]);
    let second =
        envelope_from(vec![row_from(current_ready_parts()), row_from(limited_root_parts())]);
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(first, second);
    assert_eq!(first.rows()[0].subject().root_id().as_str(), "root-a");
}

#[test]
fn fixture_hostile_path_source_secret_log_canaries() {
    for hostile in [
        "/home/user/.ssh/id_rsa",
        r"C:\Users\secret\token.txt",
        "C:",
        "package Foo;",
        "sub leak {",
        "password=hunter2",
        "Password123",
        "SECRET",
        "-----BEGIN RSA PRIVATE KEY-----",
        "PATH=/usr/bin",
        "$HOME",
        "line1\nERROR secret",
        "AKIAIOSFODNN7EXAMPLE",
    ] {
        let err = must_err_with(LogicalToken::parse(hostile), format!("canary `{hostile}`"));
        assert!(
            matches!(
                err,
                ValidationError::HostilePublicContent | ValidationError::InvalidLogicalTokenCharset
            ),
            "canary `{hostile}` produced {err:?}"
        );
    }
}

// ── Negative controls ──────────────────────────────────────────────────────

#[test]
fn negative_one_status_enum_does_not_collapse_independent_dimensions() {
    let value = serialize(&envelope_from(vec![row_from(store_incompatible_parts())]));
    let row = &value["rows"][0];
    for key in [
        "root_lifecycle",
        "configuration",
        "watch_reload",
        "readiness",
        "storage",
        "hydration",
        "provider_posture",
    ] {
        assert!(
            row.get(key).is_some(),
            "independent dimension `{key}` must remain a distinct field"
        );
    }
    assert_ne!(row["storage"], row["hydration"]);
    assert_ne!(row["watch_reload"], row["provider_posture"]);
    assert!(row["readiness"].get("active_document").is_some());
    assert!(row["readiness"].get("whole_workspace").is_some());
}

#[test]
fn negative_absent_fields_do_not_deserialize_as_ready_current_or_zero() {
    let mut value = serialize(&envelope_from(vec![row_from(current_ready_parts())]));
    value["rows"][0].as_object_mut().unwrap().remove("provider_posture");
    assert!(serde_json::from_value::<WorkspaceLifecycleStatusV1>(value.clone()).is_err());

    value = serialize(&envelope_from(vec![row_from(current_ready_parts())]));
    value["rows"][0].as_object_mut().unwrap().remove("reasons");
    assert!(serde_json::from_value::<WorkspaceLifecycleStatusV1>(value).is_err());
}

#[test]
fn negative_cache_availability_does_not_imply_hydration_or_readiness() {
    let available_rejected = envelope_from(vec![row_from(store_incompatible_parts())]);
    assert_eq!(
        available_rejected.rows()[0].storage().observed_value(),
        Some(StorageState::Available)
    );
    assert_eq!(
        available_rejected.rows()[0].hydration().observed_value(),
        Some(HydrationState::Rejected)
    );
    assert_ne!(
        available_rejected.rows()[0].readiness().whole_workspace().observed_value(),
        Some(ReadinessState::Ready)
    );
}

#[test]
fn negative_watch_observation_does_not_imply_successful_reload() {
    let mut parts = current_ready_parts();
    parts.watch_reload =
        observed(&parts.subject, "watch.transport", "reload-1", WatchReloadState::Registered);
    parts.metadata = metadata_partial();
    let status = envelope_from(vec![row_from(parts)]);
    assert_eq!(
        status.rows()[0].watch_reload().observed_value(),
        Some(WatchReloadState::Registered)
    );
    assert_ne!(status.rows()[0].watch_reload().observed_value(), Some(WatchReloadState::Accepted));
}

#[test]
fn negative_provider_empty_does_not_become_legitimate_empty() {
    let mut value = serialize(&envelope_from(vec![row_from(current_ready_parts())]));
    value["rows"][0]["provider_posture"] = json!("");
    assert!(serde_json::from_value::<WorkspaceLifecycleStatusV1>(value.clone()).is_err());
    value = serialize(&envelope_from(vec![row_from(current_ready_parts())]));
    value["rows"][0].as_object_mut().unwrap().remove("provider_posture");
    let decoded = serde_json::from_value::<WorkspaceLifecycleStatusV1>(value);
    assert!(decoded.is_err(), "missing provider must not default to legitimate_empty");
}

#[test]
fn negative_cross_root_or_cross_generation_observations_join() {
    let mut parts = current_ready_parts();
    parts.storage = StatusDimension::Observed {
        provenance: DimensionProvenance::new(
            parts.subject.session_id().clone(),
            token("root-other"),
            parts.subject.runtime_generation().clone(),
            token("store.backend"),
            token("store-1"),
        ),
        value: StorageState::Available,
    };
    let err = must_err_with(WorkspaceLifecycleStatusRow::from_parts(parts), "cross-root");
    assert_eq!(err.as_str(), "cross_root_join");

    let mut parts = current_ready_parts();
    parts.watch_reload = StatusDimension::Observed {
        provenance: DimensionProvenance::new(
            parts.subject.session_id().clone(),
            parts.subject.root_id().clone(),
            token("runtime-other"),
            token("watch.transport"),
            token("reload-1"),
        ),
        value: WatchReloadState::Accepted,
    };
    let err = must_err_with(WorkspaceLifecycleStatusRow::from_parts(parts), "cross-generation");
    assert_eq!(err.as_str(), "cross_generation_join");
}

#[test]
fn negative_free_form_strings_are_not_reason_or_repair_authority() {
    let err = must_err_with(LogicalToken::parse("please fix the config"), "prose reason");
    assert_eq!(err.as_str(), "invalid_logical_token_charset");

    let mut value = serialize(&envelope_from(vec![row_from(current_ready_parts())]));
    value["rows"][0]["repair"]["primary_action"] = json!("do whatever you like");
    assert!(serde_json::from_value::<WorkspaceLifecycleStatusV1>(value).is_err());
}

#[test]
fn negative_unknown_mandatory_schema_is_not_accepted_optimistically() {
    let mut value = serialize(&envelope_from(vec![row_from(current_ready_parts())]));
    value["schema_version"] = json!(2);
    assert!(serde_json::from_value::<WorkspaceLifecycleStatusV1>(value.clone()).is_err());
    value["schema_version"] = json!("workspace_lifecycle_status.v1");
    assert!(serde_json::from_value::<WorkspaceLifecycleStatusV1>(value).is_err());
}

#[test]
fn negative_source_secrets_private_paths_env_or_unbounded_history_enter_public_output() {
    assert!(LogicalToken::parse("/var/log/syslog").is_err());
    assert!(LogicalToken::parse("AKIAIOSFODNN7EXAMPLE").is_err());
    let err = must_err_with(
        StatusMetadata::new(
            Completeness::Complete,
            InstrumentState::Ok,
            PrivacyClass::PublicSafe,
            99,
        ),
        "unbounded history",
    );
    assert_eq!(err.as_str(), "bound_exceeded");

    let json = must_with(
        serde_json::to_string(&envelope_from(vec![row_from(current_ready_parts())])),
        "serialize public",
    );
    assert!(!json.contains("/home/"));
    assert!(!json.contains("BEGIN "));
    assert!(!json.contains("password"));
}

#[test]
fn negative_token_list_membership_changes_semantic_identity() {
    let mut reasons_and_limitations = current_ready_parts();
    reasons_and_limitations.reasons = vec![token("a")];
    reasons_and_limitations.limitations = vec![token("b")];
    reasons_and_limitations.operation_links = Vec::new();

    let mut limitations_only = current_ready_parts();
    limitations_only.reasons = Vec::new();
    limitations_only.limitations = vec![token("a"), token("b")];
    limitations_only.operation_links = Vec::new();

    let left = envelope_from(vec![row_from(reasons_and_limitations)]);
    let right = envelope_from(vec![row_from(limitations_only)]);
    assert_ne!(left.fingerprint(), right.fingerprint());
    assert_ne!(left, right);
}

#[test]
fn negative_timestamps_host_paths_or_insertion_order_change_semantic_identity() {
    let left = envelope_from(vec![row_from(current_ready_parts()), row_from(limited_root_parts())]);
    let right =
        envelope_from(vec![row_from(limited_root_parts()), row_from(current_ready_parts())]);
    assert_eq!(left.fingerprint(), right.fingerprint());

    let json = must_with(serde_json::to_string(&left), "serialize");
    assert!(!json.contains("timestamp"));
    assert!(!json.contains("created_at"));
    assert!(!json.contains("/tmp/"));
}

#[test]
fn schema_version_v1_is_supported_and_displayed() {
    assert!(WorkspaceLifecycleStatusSchemaVersion::V1.is_supported());
    assert!(!WorkspaceLifecycleStatusSchemaVersion(2).is_supported());
    assert_eq!(
        WorkspaceLifecycleStatusSchemaVersion::V1.to_string(),
        "workspace_lifecycle_status.v1"
    );
}

#[test]
fn status_metadata_deserialize_enforces_history_bound() {
    let err = serde_json::from_value::<StatusMetadata>(json!({
        "completeness": "complete",
        "instrument_state": "ok",
        "privacy_class": "public_safe",
        "history_retained": 99
    }));
    assert!(err.is_err(), "standalone StatusMetadata must not bypass history bound");
}

#[test]
fn raw_duplicate_reason_count_cannot_evade_row_bound() {
    let mut parts = current_ready_parts();
    parts.reasons = (0..17).map(|_| token("reason.ready")).collect();
    let err = must_err_with(
        WorkspaceLifecycleStatusRow::from_parts(parts),
        "seventeen duplicate reasons",
    );
    assert_eq!(err.as_str(), "bound_exceeded");
}

#[test]
fn complete_cannot_hide_not_proven_or_instrument_failure() {
    let mut parts = current_ready_parts();
    parts.provider_posture = StatusDimension::NotProven;
    let err = must_err_with(WorkspaceLifecycleStatusRow::from_parts(parts), "complete+not_proven");
    assert_eq!(err.as_str(), "completeness_contradiction");
}

#[test]
fn stale_configuration_allows_mismatched_generation() {
    let mut parts = current_ready_parts();
    parts.configuration = StatusDimension::Observed {
        provenance: DimensionProvenance::new(
            parts.subject.session_id().clone(),
            parts.subject.root_id().clone(),
            token("runtime-old"),
            token("config.authority"),
            token("config-old"),
        ),
        value: ConfigurationState::Stale,
    };
    parts.metadata = metadata_partial();
    let row = row_from(parts);
    assert_eq!(row.configuration().observed_value(), Some(ConfigurationState::Stale));
}

#[test]
fn unknown_envelope_fields_fail_closed() {
    let mut value = serialize(&envelope_from(vec![row_from(current_ready_parts())]));
    value["future_field"] = json!(true);
    assert!(serde_json::from_value::<WorkspaceLifecycleStatusV1>(value).is_err());
}

#[test]
fn fingerprint_mismatch_fails_closed() {
    let mut value = serialize(&envelope_from(vec![row_from(current_ready_parts())]));
    value["fingerprint"] =
        json!("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let err = serde_json::from_value::<WorkspaceLifecycleStatusV1>(value);
    assert!(err.is_err(), "forged fingerprint must fail");
}

#[test]
fn module_references_no_live_workspace_cli_protocol_or_editor_surfaces() {
    let production = include_str!("mod.rs").split("#[cfg(test)]").next().unwrap();
    let ids = include_str!("ids.rs");
    let vocab = include_str!("vocab.rs");
    let envelope = include_str!("envelope.rs");
    for source in [production, ids, vocab, envelope] {
        for forbidden in [
            "use perl_workspace",
            "use crate::providers",
            "use crate::protocol",
            "use crate::runtime",
            "use crate::config",
            "std::fs",
            "std::env",
            "vscode",
            "perllsp doctor",
        ] {
            assert!(
                !source.contains(forbidden),
                "contract must not read live state or sibling surfaces: found `{forbidden}`"
            );
        }
    }
}
