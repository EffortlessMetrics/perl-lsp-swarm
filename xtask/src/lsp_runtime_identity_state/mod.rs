//! Checked causal identity, state-axis, and lifecycle vocabulary for the reusable LSP runtime.
//!
//! Issue #11045 owns this normative vocabulary. It is stable repository data,
//! not current-tree, candidate, support, or release state. Downstream contracts
//! should reference these stable ids rather than minting private synonyms.

use std::collections::BTreeSet;

use color_eyre::eyre::{Context, Result};

mod constants;
mod model;
mod render;
mod validate;

use constants::*;
use model::*;

/// Normative contract root. It names the checked data fragments below.
pub const MANIFEST_RELATIVE_PATH: &str =
    ".spec/11045-lsp-runtime-vocabulary/contract.v1.json";

/// Human reference containing the checked generated index.
pub const DOCUMENT_RELATIVE_PATH: &str =
    "docs/architecture/lsp-runtime/identity-and-state.md";

/// Closed schema identity.
pub const SCHEMA_NAME: &str = "lsp_runtime_identity_state.v1";

/// Closed schema version.
pub const SCHEMA_VERSION: u64 = 1;

const EMBEDDED_CONTRACT: &str =
    include_str!("../../../.spec/11045-lsp-runtime-vocabulary/contract.v1.json");
const EMBEDDED_IDENTITIES: &str =
    include_str!("../../../.spec/11045-lsp-runtime-vocabulary/identities.v1.json");
const EMBEDDED_STATES: &str =
    include_str!("../../../.spec/11045-lsp-runtime-vocabulary/states.v1.json");
const EMBEDDED_RELATIONS: &str =
    include_str!("../../../.spec/11045-lsp-runtime-vocabulary/relations.v1.json");
const EMBEDDED_JOURNEYS: &str =
    include_str!("../../../.spec/11045-lsp-runtime-vocabulary/journeys.v1.json");
const EMBEDDED_DOCUMENT: &str =
    include_str!("../../../docs/architecture/lsp-runtime/identity-and-state.md");

/// Embedded human reference.
#[must_use]
pub const fn embedded_document() -> &'static str {
    EMBEDDED_DOCUMENT
}

/// Return the normalized embedded bundle as one JSON document for tests and downstream readers.
pub fn embedded_bundle_json() -> Result<String> {
    let vocabulary = parse_embedded()?;
    serde_json::to_string(&vocabulary).wrap_err("failed to serialize embedded vocabulary bundle")
}

/// Validate an arbitrary assembled vocabulary document without granting it repository authority.
pub fn validate_str(raw: &str) -> Result<()> {
    parse(raw).map(|_| ())
}

/// Validate all repository-owned fragments and the checked documentation index.
pub fn validate_embedded() -> Result<()> {
    let vocabulary = parse_embedded()?;
    render::verify_document(&vocabulary, EMBEDDED_DOCUMENT)
}

/// Return a deterministic semantic digest. Array and object order do not matter.
pub fn semantic_digest_str(raw: &str) -> Result<String> {
    let vocabulary = parse(raw)?;
    render::semantic_digest(&vocabulary)
}

/// Render the deterministic checked index used by the human reference.
pub fn render_index_str(raw: &str) -> Result<String> {
    let vocabulary = parse(raw)?;
    render::render_index(&vocabulary)
}

/// Return the sorted stable concept ids available to downstream contract code.
pub fn concept_ids() -> Result<Vec<String>> {
    let vocabulary = parse_embedded()?;
    Ok(concept_id_set(&vocabulary).into_iter().map(str::to_string).collect())
}

fn parse_embedded() -> Result<Vocabulary> {
    let contract: ContractFile =
        serde_json::from_str(EMBEDDED_CONTRACT).wrap_err("invalid vocabulary contract root")?;
    let identities: IdentityFragment =
        serde_json::from_str(EMBEDDED_IDENTITIES).wrap_err("invalid identity fragment")?;
    let states: StateFragment =
        serde_json::from_str(EMBEDDED_STATES).wrap_err("invalid state fragment")?;
    let relations: RelationFragment =
        serde_json::from_str(EMBEDDED_RELATIONS).wrap_err("invalid relationship fragment")?;
    let journeys: JourneyFragment =
        serde_json::from_str(EMBEDDED_JOURNEYS).wrap_err("invalid journey fragment")?;

    let mut vocabulary = Vocabulary {
        schema: contract.schema,
        version: contract.version,
        authority: contract.authority,
        fragments: contract.fragments,
        request_state: contract.request_state,
        generic_boundary: contract.generic_boundary,
        axes: contract.axes,
        identities: identities.identities,
        boundary_terms: contract.boundary_terms,
        states: states.states,
        relations: relations.relations,
        ambiguous_terms: contract.ambiguous_terms,
        journeys: journeys.journeys,
    };
    vocabulary.normalize();
    vocabulary.validate()?;
    Ok(vocabulary)
}

fn parse(raw: &str) -> Result<Vocabulary> {
    let mut vocabulary: Vocabulary =
        serde_json::from_str(raw).wrap_err("invalid lsp runtime identity/state vocabulary")?;
    vocabulary.normalize();
    vocabulary.validate()?;
    Ok(vocabulary)
}

impl Vocabulary {
    fn normalize(&mut self) {
        self.authority.consumers.sort_unstable();
        self.request_state.axes.sort();
        self.generic_boundary.forbidden_terms.sort();

        self.axes.sort_by(|a, b| a.id.cmp(&b.id));
        self.identities.sort_by(|a, b| a.id.cmp(&b.id));
        self.boundary_terms.sort_by(|a, b| a.id.cmp(&b.id));
        self.states.sort_by(|a, b| a.id.cmp(&b.id));
        self.relations.sort_by(|a, b| a.id.cmp(&b.id));
        self.ambiguous_terms.sort_by(|a, b| a.term.cmp(&b.term));
        self.journeys.sort_by(|a, b| a.id.cmp(&b.id));

        for identity in &mut self.identities {
            identity.variants.sort();
            identity.scoped_by.sort();
        }
        for term in &mut self.ambiguous_terms {
            term.replacements.sort();
        }
        for journey in &mut self.journeys {
            journey.facts.sort();
            journey.relations.sort();
            journey.rejected.sort();
        }
    }
}

fn concept_id_set(vocabulary: &Vocabulary) -> BTreeSet<&str> {
    vocabulary
        .identities
        .iter()
        .map(|row| row.id.as_str())
        .chain(vocabulary.boundary_terms.iter().map(|row| row.id.as_str()))
        .chain(vocabulary.states.iter().map(|row| row.id.as_str()))
        .collect()
}
