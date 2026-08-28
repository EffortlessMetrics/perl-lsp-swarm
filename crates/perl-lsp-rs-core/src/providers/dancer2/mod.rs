//! Canonical Dancer2 read-only provider slice (#8928).
//!
//! This module promotes the first read-only Dancer2 provider cells
//! (completion, signature help, hover, definition, document/workspace
//! symbols, bounded diagnostics) from the canonical activation, import,
//! route, context, and hook facts minted by `perl-semantic-facts` and
//! extracted by `perl-semantic-analyzer` (#8914, #8918, #8921, #8924).
//!
//! # One selected authority
//!
//! Every promoted Dancer2 decision selects exactly one authority per request:
//!
//! - [`Dancer2Decision::PromoteCanonical`] — canonical facts answered;
//! - [`Dancer2Decision::FallbackExisting`] — the request class is not
//!   admitted by #8928 and the existing generic provider path owns it;
//! - [`Dancer2Decision::RefuseTyped`] — a dynamic/unsupported/ambiguous
//!   boundary was met; the cell returns a typed refusal with a reason
//!   instead of a union of canonical and legacy answers.
//!
//! Canonical and legacy framework answers are never merged into one result.
//!
//! # No new Dancer2 grammar
//!
//! The slice adds no new Dancer2 source grammar: every fact is produced by
//! the canonical producers (`extract_dancer2_*` plus the registry-activated
//! `dancer2_*_facts` minting). A missing fact keeps the cell on typed
//! fallback and is reported to the producer issue tracker rather than being
//! re-derived here.
//!
//! # Activation evidence boundary
//!
//! Exact activation requires the #8914 registry seam: a detected framework
//! plus a resolved `Dancer2` module with observed version evidence. The
//! runtime supplies that evidence through [`RuntimeDancer2Module`]; without
//! it the activation is `NotActivated` and every cell returns zero
//! framework output (never name-only synthesis).

mod activation;
mod completion;
mod decision;
mod diagnostics;
mod facts;
mod hover;
mod signature;
mod symbols;
mod targets;

pub use activation::{
    Dancer2FileActivations, Dancer2PackageActivation, RuntimeDancer2Module,
    activation_state_reason, file_activations, first_activation_site_offset, has_activation_site,
    read_declared_module_version,
};
pub use completion::{
    Dancer2CompletionCandidate, keyword_completion_candidates, keyword_completion_rank_penalty,
};
pub use decision::Dancer2Decision;
pub use diagnostics::{Dancer2BoundedDiagnostic, bounded_diagnostics};
pub use facts::{CanonicalDancer2FileFacts, canonical_file_facts};
pub use hover::{RouteHoverProjection, hover_projection_at};
/// Package-scope helper re-exported for runtime wiring (the package walk
/// lives in the analyzer's declaration module).
pub use perl_semantic_analyzer::declaration::current_package_at;
pub use signature::{RouteSignatureForm, route_keyword_signature_forms};
pub use symbols::{
    DANCER2_HOOK_LABEL, DANCER2_ROUTE_LABEL, Dancer2DocumentSymbol, dancer2_document_symbols,
    dancer2_request_kind, dancer2_workspace_entities,
};
pub use targets::{Dancer2DefinitionTarget, definition_target_at};
