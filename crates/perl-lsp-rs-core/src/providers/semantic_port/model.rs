use perl_semantic_facts::{
    Confidence, EntityId, FactId, FileId, Provenance, ProviderSurface, SemanticConfidence,
    SemanticFactEnvelope, SemanticFreshness, SemanticProducer, SemanticProvenance,
    SourceGeneration,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::ProviderQueryContractError;

include!("model/context.rs");
include!("model/fact.rs");
include!("model/completeness.rs");
include!("model/validation.rs");
