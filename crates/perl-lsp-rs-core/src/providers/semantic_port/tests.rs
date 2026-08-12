use super::model::VerifiedProviderCompletenessSnapshot;
use super::*;
use perl_semantic_facts::{
    AnchorId, BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, EntityId, FactId,
    FileId, LifecyclePhase, Provenance, ProviderFactFreshness, ProviderFactSourceKind,
    ProviderFallbackState, ProviderSurface, ScopeId, SemanticConfidence, SemanticFactEnvelope,
    SemanticFactKind, SemanticFreshness, SemanticProducer, SemanticProvenance,
    SemanticReasonCode, SourceAnchor, SourceGeneration,
};
use std::error::Error;

include!("tests/helpers.rs");
include!("tests/subjects.rs");
include!("tests/authority.rs");
include!("tests/ambiguity.rs");
include!("tests/terminal.rs");
include!("tests/outcomes.rs");
include!("tests/contracts.rs");
