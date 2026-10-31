use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Vocabulary {
    pub(super) schema: String,
    pub(super) version: u64,
    pub(super) authority: Authority,
    pub(super) fragments: Fragments,
    pub(super) request_state: RequestState,
    pub(super) generic_boundary: GenericBoundary,
    pub(super) axes: Vec<Axis>,
    pub(super) identities: Vec<Identity>,
    pub(super) boundary_terms: Vec<BoundaryTerm>,
    pub(super) states: Vec<StateTerm>,
    pub(super) relations: Vec<Relationship>,
    pub(super) ambiguous_terms: Vec<AmbiguousTerm>,
    pub(super) journeys: Vec<Journey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Authority {
    pub(super) issue: u64,
    pub(super) architecture: u64,
    pub(super) train: u64,
    pub(super) consumers: Vec<u64>,
    pub(super) claim: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Fragments {
    pub(super) identities: String,
    pub(super) states: String,
    pub(super) relations: String,
    pub(super) journeys: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ContractFile {
    pub(super) schema: String,
    pub(super) version: u64,
    pub(super) authority: Authority,
    pub(super) fragments: Fragments,
    pub(super) request_state: RequestState,
    pub(super) generic_boundary: GenericBoundary,
    pub(super) axes: Vec<Axis>,
    pub(super) boundary_terms: Vec<BoundaryTerm>,
    pub(super) ambiguous_terms: Vec<AmbiguousTerm>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IdentityFragment {
    pub(super) identities: Vec<Identity>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StateFragment {
    pub(super) states: Vec<StateTerm>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RelationFragment {
    pub(super) relations: Vec<Relationship>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct JourneyFragment {
    pub(super) journeys: Vec<Journey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RequestState {
    pub(super) kind: String,
    pub(super) linear_phase_forbidden: bool,
    pub(super) axes: Vec<String>,
    pub(super) law: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GenericBoundary {
    pub(super) one_authority: OneAuthority,
    pub(super) client_consumption_claimable: bool,
    pub(super) currentness_law: String,
    pub(super) forbidden_terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OneAuthority {
    pub(super) law: String,
    pub(super) single_object: bool,
    pub(super) single_actor: bool,
    pub(super) global_lock: bool,
    pub(super) single_store: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Axis {
    pub(super) id: String,
    pub(super) proposition: String,
    pub(super) source: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) proposition: String,
    pub(super) scope: String,
    pub(super) owner: String,
    pub(super) equality: String,
    pub(super) lifetime: String,
    pub(super) variants: Vec<String>,
    pub(super) scoped_by: Vec<String>,
    pub(super) opaque: bool,
    pub(super) owner_validated: bool,
    pub(super) source: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BoundaryTerm {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) proposition: String,
    pub(super) owner: String,
    pub(super) runtime_claimable: bool,
    pub(super) source: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StateTerm {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) axis: String,
    pub(super) proposition: String,
    pub(super) runtime_claimable: bool,
    pub(super) source: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RelationKind {
    Requires,
    Permits,
    Precedes,
    IndependentOf,
    ForbidsInference,
}

impl RelationKind {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Requires => "requires",
            Self::Permits => "permits",
            Self::Precedes => "precedes",
            Self::IndependentOf => "independent_of",
            Self::ForbidsInference => "forbids_inference",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Relationship {
    pub(super) id: String,
    #[serde(rename = "from")]
    pub(super) from_id: String,
    pub(super) kind: RelationKind,
    pub(super) to: String,
    pub(super) reason: String,
    pub(super) source: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AmbiguousTerm {
    pub(super) term: String,
    pub(super) reason: String,
    pub(super) replacements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journey {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) proposition: String,
    pub(super) facts: Vec<String>,
    pub(super) relations: Vec<String>,
    pub(super) rejected: Vec<String>,
    pub(super) source: u64,
}
