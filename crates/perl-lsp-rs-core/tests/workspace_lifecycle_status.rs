//! Public-API integration for `workspace_lifecycle_status.v1` (#16895).
//!
//! In-crate fixtures own the discriminator matrix. This file proves the
//! contract is a public, transport-neutral type that later assembler and
//! presentation leaves can consume without defining a sibling vocabulary.

use perl_lsp_rs_core::workspace_lifecycle_status::{
    Completeness, ConfigurationState, DimensionProvenance, HydrationState, InstrumentState,
    LogicalToken, PrimaryActionKind, PrimaryActionPriorityPolicy, PrivacyClass, ProducerIdentity,
    ProviderPosture, ReadinessObservation, ReadinessState, RepairProjection, ResponsibleOwnerRole,
    RootLifecycleState, StatusDimension, StatusMetadata, StorageState, WatchReloadState,
    WorkspaceLifecycleStatusParts, WorkspaceLifecycleStatusRow, WorkspaceLifecycleStatusRowParts,
    WorkspaceLifecycleStatusSchemaVersion, WorkspaceLifecycleStatusV1, WorkspaceLifecycleSubject,
};
use perl_test_must::must_with;

fn token(raw: &str) -> LogicalToken {
    must_with(LogicalToken::parse(raw), format!("token `{raw}`"))
}

fn observed<T>(
    subject: &WorkspaceLifecycleSubject,
    source: &str,
    domain: &str,
    value: T,
) -> StatusDimension<T> {
    StatusDimension::Observed {
        provenance: DimensionProvenance::new(
            subject.session_id().clone(),
            subject.root_id().clone(),
            subject.runtime_generation().clone(),
            token(source),
            token(domain),
        ),
        value,
    }
}

#[test]
fn public_api_constructs_and_round_trips_workspace_lifecycle_status_v1() {
    let subject = WorkspaceLifecycleSubject::new(
        token("session-public"),
        token("workspace-public"),
        token("root-public"),
        token("runtime-1"),
        token("config-1"),
        token("trust-1"),
        token("env-1"),
    );
    let row = must_with(
        WorkspaceLifecycleStatusRow::from_parts(WorkspaceLifecycleStatusRowParts {
            subject: subject.clone(),
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
            watch_reload: observed(
                &subject,
                "watch.transport",
                "reload-1",
                WatchReloadState::Registered,
            ),
            readiness: ReadinessObservation::new(
                observed(&subject, "readiness.active", "ready-1", ReadinessState::Ready),
                observed(&subject, "readiness.neighborhood", "ready-1", ReadinessState::Ready),
                observed(&subject, "readiness.workspace", "ready-1", ReadinessState::Ready),
            ),
            storage: observed(&subject, "store.backend", "store-1", StorageState::Disabled),
            hydration: observed(
                &subject,
                "hydration.owner",
                "hydrate-1",
                HydrationState::NotAttempted,
            ),
            provider_posture: observed(
                &subject,
                "provider.exact",
                "provider-1",
                ProviderPosture::NotReady,
            ),
            repair: RepairProjection::new(
                token("reason.cold_start"),
                ResponsibleOwnerRole::Storage,
                PrimaryActionKind::NoneRequired,
                None,
            ),
            reasons: vec![token("reason.cache_disabled")],
            limitations: Vec::new(),
            operation_links: Vec::new(),
            metadata: must_with(
                StatusMetadata::new(
                    Completeness::Partial,
                    InstrumentState::Ok,
                    PrivacyClass::PublicSafe,
                    0,
                ),
                "metadata",
            ),
        }),
        "public row",
    );
    let status = must_with(
        WorkspaceLifecycleStatusV1::from_parts(WorkspaceLifecycleStatusParts {
            producer: ProducerIdentity::new(token("perl-lsp-rs-core")),
            primary_action_priority_policy: PrimaryActionPriorityPolicy::CanonicalV1,
            rows: vec![row],
        }),
        "public envelope",
    );
    assert_eq!(status.schema_version().to_string(), "workspace_lifecycle_status.v1");
    assert!(WorkspaceLifecycleStatusSchemaVersion::V1.is_supported());
    let json = must_with(serde_json::to_string(&status), "serialize");
    let back: WorkspaceLifecycleStatusV1 = must_with(serde_json::from_str(&json), "deserialize");
    assert_eq!(status, back);
    assert_eq!(status.fingerprint(), back.fingerprint());
}
