use perl_source_identity::{
    ContentDigest, LogicalSourceId, ProjectId, RootRelativeLogicalPath, SourceGeneration,
    WorkspaceRootId,
};
use perl_test_must::{must_err_with, must_with};

use crate::features::policy::FeatureProfile;
use crate::providers::semantic_port::{
    ProviderCancellationState, ProviderIdentity, ProviderQueryDeadline,
    ProviderReadinessRequirement, ProviderReadinessState,
};
use crate::semantic_snapshot::DocumentInstanceId;

use super::{
    DiskObservationError, DiskObservationFacts, DocumentSourceKind, DocumentSourceOffer,
    LocalHostFacts, LspSessionFacts, OpenDocumentFacts, SourceSelection, SuppliedDocumentFacts,
    UnsupportedRoots, WorkspaceRootBinding, WorkspaceSession, WorkspaceSessionError,
};

const PROJECT: &str = "acme/widget";
const ROOT_KEY: &str = "root-a";
const PATH: &str = "lib/Widget.pm";
const OPEN_TEXT: &str = "package Widget;\nour $OPEN = 1;\n1;\n";
const DISK_TEXT: &str = "package Widget;\nour $DISK = 1;\n1;\n";
const SUPPLIED_TEXT: &str = "package Widget;\nour $SUPPLIED = 1;\n1;\n";

fn project() -> ProjectId {
    ProjectId::from_canonical_name(PROJECT)
}

fn root() -> WorkspaceRootId {
    WorkspaceRootId::from_project_and_root_key(&project(), ROOT_KEY)
}

fn logical_source() -> LogicalSourceId {
    let path = must_with(RootRelativeLogicalPath::parse(PATH), "valid logical path");
    LogicalSourceId::from_root_and_logical_path(&root(), &path)
}

fn instance(key: &str) -> DocumentInstanceId {
    DocumentInstanceId::from_logical_source_and_instance_key(&logical_source(), key)
}

fn open_facts(text: &str, key: &str, generation: &str) -> OpenDocumentFacts {
    OpenDocumentFacts {
        instance: instance(key),
        generation: SourceGeneration::known(generation),
        text: text.to_string(),
    }
}

fn disk_ok(text: &str) -> Result<DiskObservationFacts, DiskObservationError> {
    Ok(DiskObservationFacts {
        generation: SourceGeneration::known("disk-4"),
        text: text.to_string(),
        metadata_identity: "len:32;mtime:7".to_string(),
    })
}

fn offer_open() -> DocumentSourceOffer {
    DocumentSourceOffer {
        selection: Some(SourceSelection::OpenRetained),
        document_required: true,
        open: Some(open_facts(OPEN_TEXT, "open-1", "doc-3")),
        supplied: None,
        disk: disk_ok(DISK_TEXT),
        expected_instance: Some(instance("open-1")),
        expected_generation: Some(SourceGeneration::known("doc-3")),
    }
}

fn local_facts(document: DocumentSourceOffer) -> LocalHostFacts {
    LocalHostFacts {
        project_name: PROJECT.to_string(),
        root_key: ROOT_KEY.to_string(),
        root_generation: SourceGeneration::known("root-2"),
        expected_root_generation: None,
        config_generation: SourceGeneration::known("cfg-5"),
        expected_config_generation: None,
        feature_profile: FeatureProfile::Production,
        workspace_generation: SourceGeneration::known("ws-9"),
        index_generation: SourceGeneration::known("idx-8"),
        readiness_requirement: ProviderReadinessRequirement::ActiveDocument,
        readiness_state: ProviderReadinessState::Ready,
        deadline: ProviderQueryDeadline::RemainingMillis(250),
        cancellation: ProviderCancellationState::Active,
        logical_path: Some(PATH.to_string()),
        document,
    }
}

fn lsp_facts(document: DocumentSourceOffer) -> LspSessionFacts {
    LspSessionFacts {
        project: project(),
        roots: Ok(WorkspaceRootBinding::single(root())),
        root_generation: SourceGeneration::known("root-2"),
        expected_root_generation: None,
        config_generation: SourceGeneration::known("cfg-5"),
        expected_config_generation: None,
        feature_profile: FeatureProfile::Production,
        workspace_generation: SourceGeneration::known("ws-9"),
        index_generation: SourceGeneration::known("idx-8"),
        readiness_requirement: ProviderReadinessRequirement::ActiveDocument,
        readiness_state: ProviderReadinessState::Ready,
        deadline: ProviderQueryDeadline::RemainingMillis(250),
        cancellation: ProviderCancellationState::Active,
        logical_path: Some(PATH.to_string()),
        document,
    }
}

#[test]
fn open_snapshot_differs_from_disk() {
    let open =
        must_with(WorkspaceSession::from_local_host(&local_facts(offer_open())), "open session");
    let mut disk_offer = offer_open();
    disk_offer.selection = Some(SourceSelection::DiskObservation);
    disk_offer.expected_instance = None;
    disk_offer.expected_generation = None;
    let disk =
        must_with(WorkspaceSession::from_local_host(&local_facts(disk_offer)), "disk session");

    let open_source =
        must_with(open.document_source().cloned().ok_or("missing open source"), "open source");
    let disk_source =
        must_with(disk.document_source().cloned().ok_or("missing disk source"), "disk source");
    assert_eq!(open_source.kind(), DocumentSourceKind::OpenRetained);
    assert_eq!(disk_source.kind(), DocumentSourceKind::DiskObservation);
    assert_ne!(open_source.digest(), disk_source.digest());
    assert_eq!(open_source.digest(), &ContentDigest::of_bytes(OPEN_TEXT.as_bytes()));
    assert_eq!(disk_source.digest(), &ContentDigest::of_bytes(DISK_TEXT.as_bytes()));
    assert!(open_source.document_instance().is_some());
    assert!(disk_source.document_instance().is_none());
    assert!(disk_source.disk_metadata_identity().is_some());
    assert_ne!(open.session_id(), disk.session_id());
}

#[test]
fn supplied_snapshot_differs_from_open_and_disk() {
    let mut offer = offer_open();
    offer.selection = Some(SourceSelection::CallerSupplied);
    offer.supplied = Some(SuppliedDocumentFacts {
        generation: SourceGeneration::known("caller-1"),
        text: SUPPLIED_TEXT.to_string(),
    });
    offer.expected_instance = None;
    offer.expected_generation = None;
    let supplied =
        must_with(WorkspaceSession::from_local_host(&local_facts(offer)), "supplied session");
    let source = must_with(
        supplied.document_source().cloned().ok_or("missing supplied source"),
        "supplied source",
    );
    assert_eq!(source.kind(), DocumentSourceKind::CallerSupplied);
    assert_eq!(source.digest(), &ContentDigest::of_bytes(SUPPLIED_TEXT.as_bytes()));
    assert_ne!(source.digest(), &ContentDigest::of_bytes(OPEN_TEXT.as_bytes()));
    assert_ne!(source.digest(), &ContentDigest::of_bytes(DISK_TEXT.as_bytes()));
    assert!(source.document_instance().is_none());
}

#[test]
fn caller_supplied_does_not_fall_back_to_disk() {
    let mut offer = offer_open();
    offer.selection = Some(SourceSelection::CallerSupplied);
    offer.supplied = None;
    let error = must_err_with(
        WorkspaceSession::from_local_host(&local_facts(offer)),
        "caller-supplied must not use disk",
    );
    assert_eq!(error, WorkspaceSessionError::CallerSuppliedUnavailable);
}

#[test]
fn stale_document_generation_is_a_construction_error() {
    let mut offer = offer_open();
    offer.expected_generation = Some(SourceGeneration::known("doc-2"));
    let error =
        must_err_with(WorkspaceSession::from_lsp_facts(&lsp_facts(offer)), "stale generation");
    assert!(matches!(error, WorkspaceSessionError::StaleDocumentGeneration { .. }));
}

#[test]
fn replaced_document_instance_is_a_construction_error() {
    let mut offer = offer_open();
    offer.expected_instance = Some(instance("open-2"));
    let error =
        must_err_with(WorkspaceSession::from_lsp_facts(&lsp_facts(offer)), "replaced instance");
    assert!(matches!(error, WorkspaceSessionError::ReplacedDocumentInstance { .. }));
}

#[test]
fn root_generation_change_mints_a_new_session_identity() {
    let first = must_with(
        WorkspaceSession::from_local_host(&local_facts(offer_open())),
        "first root generation",
    );
    let mut moved = local_facts(offer_open());
    moved.root_generation = SourceGeneration::known("root-3");
    let second = must_with(WorkspaceSession::from_local_host(&moved), "moved root generation");
    assert_ne!(first.session_id(), second.session_id());
}

#[test]
fn config_generation_change_mints_a_new_session_and_rejects_stale_bind() {
    let first = must_with(
        WorkspaceSession::from_local_host(&local_facts(offer_open())),
        "first config generation",
    );
    let mut moved = local_facts(offer_open());
    moved.config_generation = SourceGeneration::known("cfg-6");
    let second = must_with(WorkspaceSession::from_local_host(&moved), "moved config generation");
    assert_ne!(first.session_id(), second.session_id());

    moved.expected_config_generation = Some(SourceGeneration::known("cfg-5"));
    let error = must_err_with(WorkspaceSession::from_local_host(&moved), "stale config bind");
    assert!(matches!(error, WorkspaceSessionError::StaleConfigGeneration { .. }));
}

#[test]
fn stale_root_bind_is_a_construction_error() {
    let mut facts = local_facts(offer_open());
    facts.expected_root_generation = Some(SourceGeneration::known("root-1"));
    let error = must_err_with(WorkspaceSession::from_local_host(&facts), "stale root bind");
    assert!(matches!(error, WorkspaceSessionError::StaleRootGeneration { .. }));
}

#[test]
fn missing_unreadable_ambiguous_and_escaped_root_fail_closed() {
    let mut missing = offer_open();
    missing.open = None;
    missing.disk = Err(DiskObservationError::Missing);
    missing.supplied = None;
    let error =
        must_err_with(WorkspaceSession::from_local_host(&local_facts(missing)), "missing document");
    assert_eq!(error, WorkspaceSessionError::MissingDocument);

    let mut unreadable = offer_open();
    unreadable.selection = Some(SourceSelection::DiskObservation);
    unreadable.disk = Err(DiskObservationError::Unreadable);
    unreadable.expected_instance = None;
    unreadable.expected_generation = None;
    let error = must_err_with(
        WorkspaceSession::from_local_host(&local_facts(unreadable)),
        "unreadable disk",
    );
    assert_eq!(error, WorkspaceSessionError::UnreadableDocument);

    let mut ambiguous = offer_open();
    ambiguous.selection = None;
    let error = must_err_with(
        WorkspaceSession::from_local_host(&local_facts(ambiguous)),
        "ambiguous sources",
    );
    assert_eq!(error, WorkspaceSessionError::AmbiguousDocumentSource);

    let mut escaped = local_facts(offer_open());
    escaped.logical_path = Some("../secret.pm".to_string());
    let error = must_err_with(WorkspaceSession::from_local_host(&escaped), "escaped root");
    assert!(matches!(error, WorkspaceSessionError::EscapedRoot { .. }));
}

#[test]
fn single_root_succeeds_and_unsupported_multi_root_fails() {
    let session =
        must_with(WorkspaceSession::from_lsp_facts(&lsp_facts(offer_open())), "single root");
    assert!(matches!(session.roots(), WorkspaceRootBinding::Single { .. }));

    let mut unsupported = lsp_facts(offer_open());
    unsupported.roots = Err(UnsupportedRoots);
    let error =
        must_err_with(WorkspaceSession::from_lsp_facts(&unsupported), "unsupported multi-root");
    assert_eq!(error, WorkspaceSessionError::UnsupportedMultiRoot);
}

#[test]
fn reviewed_multi_root_uses_the_canonical_root_in_query_context() {
    let extra = WorkspaceRootId::from_project_and_root_key(&project(), "root-b");
    let binding = must_with(
        WorkspaceRootBinding::reviewed_multi_root(root(), vec![extra]),
        "reviewed multi-root",
    );
    let mut facts = lsp_facts(offer_open());
    facts.roots = Ok(binding);
    let session = must_with(WorkspaceSession::from_lsp_facts(&facts), "reviewed session");
    let context = session.provider_query_context();
    assert_eq!(context.root_identity, ProviderIdentity::known(root().as_wire()));
    assert!(matches!(session.roots(), WorkspaceRootBinding::ReviewedMultiRoot { .. }));
}

#[test]
fn cancellation_and_deadline_propagate_into_operation_identity_and_query_context() {
    let active =
        must_with(WorkspaceSession::from_local_host(&local_facts(offer_open())), "active controls");
    let mut cancelled = local_facts(offer_open());
    cancelled.cancellation = ProviderCancellationState::Cancelled;
    cancelled.deadline = ProviderQueryDeadline::Expired;
    let terminal = must_with(WorkspaceSession::from_local_host(&cancelled), "terminal controls");
    assert_eq!(active.session_id(), terminal.session_id());
    assert_ne!(active.operation_id(), terminal.operation_id());
    assert_eq!(active.provider_query_context().cancellation, ProviderCancellationState::Active);
    assert_eq!(terminal.provider_query_context().deadline, ProviderQueryDeadline::Expired);
    assert_eq!(
        terminal.provider_query_context().cancellation,
        ProviderCancellationState::Cancelled
    );
}

#[test]
fn identical_inputs_mint_identical_session_and_request_identities() {
    let first = must_with(WorkspaceSession::from_local_host(&local_facts(offer_open())), "first");
    let second = must_with(WorkspaceSession::from_local_host(&local_facts(offer_open())), "second");
    assert_eq!(first, second);
    assert_eq!(first.session_id().as_wire(), second.session_id().as_wire());
    assert_eq!(first.operation_id().as_wire(), second.operation_id().as_wire());
    assert_eq!(first.provider_query_context(), second.provider_query_context());
}

#[test]
fn construction_does_not_mutate_input_facts() {
    let facts = local_facts(offer_open());
    let snapshot = facts.clone();
    let _session = must_with(WorkspaceSession::from_local_host(&facts), "construct");
    assert_eq!(facts, snapshot, "constructing a session must not write lifecycle facts");
}

#[test]
fn lsp_and_local_constructors_agree_on_equivalent_facts() {
    let local =
        must_with(WorkspaceSession::from_local_host(&local_facts(offer_open())), "local host");
    let lsp =
        must_with(WorkspaceSession::from_lsp_facts(&lsp_facts(offer_open())), "lsp projection");
    assert_eq!(local.session_id(), lsp.session_id());
    assert_eq!(local.operation_id(), lsp.operation_id());
    assert_eq!(local.provider_query_context(), lsp.provider_query_context());
    assert_eq!(local.document_source(), lsp.document_source());
}

#[test]
fn query_context_consumes_the_6816_port_types() {
    let session = must_with(WorkspaceSession::from_lsp_facts(&lsp_facts(offer_open())), "session");
    let context = session.provider_query_context();
    assert_eq!(context.project_identity, ProviderIdentity::known(project().as_wire()));
    assert_eq!(context.root_identity, ProviderIdentity::known(root().as_wire()));
    assert_eq!(
        context.document_generation,
        crate::providers::semantic_port::ProviderQueryContext::new(
            context.project_identity.clone(),
            context.root_identity.clone(),
            context.document_generation.clone(),
            context.workspace_generation.clone(),
            context.readiness_requirement,
            context.readiness_state,
            context.deadline,
            context.cancellation,
        )
        .document_generation
    );
    assert!(matches!(
        session.document_source().map(super::DocumentSource::kind),
        Some(DocumentSourceKind::OpenRetained)
    ));
}

#[test]
fn facade_does_not_import_transport_sdks() {
    for source in [include_str!("mod.rs"), include_str!("model.rs"), include_str!("construct.rs")] {
        let production = must_with(
            source.split("#[cfg(test)]").next().ok_or("missing production region"),
            "production region",
        );
        for forbidden in [
            "use lsp_types",
            "use crate::protocol",
            "use crate::transport",
            "use crate::uri",
            "use tower_lsp",
        ] {
            assert!(
                !production.contains(forbidden),
                "session facade must stay below transport SDKs: found `{forbidden}`"
            );
        }
    }
}
