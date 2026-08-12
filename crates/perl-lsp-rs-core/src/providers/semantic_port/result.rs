use perl_semantic_facts::{
    BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, ProviderFactTrace,
    ProviderFallbackState, SemanticConfidence, SemanticFactEnvelope, SemanticFactKind,
    SemanticFactStatus, SemanticFreshness, SemanticProducer, SemanticProvenance,
    SemanticReasonCode, SourceAnchor, SourceGeneration,
};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::BTreeSet;

use super::{
    ProviderCancellationState, ProviderCompletenessAuthorityReceipt, ProviderCompletenessGrant,
    ProviderQueryContractError, ProviderQueryControl, ProviderQueryDeadline, ProviderQueryFact,
    ProviderQueryKind, ProviderQueryRequest, ProviderQuerySubject, facts_are_related,
    semantic_provenance_is_exact,
};

include!("result/types.rs");
include!("result/checked.rs");
include!("result/execute.rs");
include!("result/evidence.rs");
include!("result/validate.rs");
include!("result/summarize.rs");
