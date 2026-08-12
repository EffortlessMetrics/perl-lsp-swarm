use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Canonical schema identity for fixture-expectation sidecars.
pub const FIXTURE_EXPECTATION_SCHEMA: &str = "fixture_expectation.v1";

/// Accepted expectation modes for schema v1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExpectationMode {
    /// The fixture should parse without errors.
    ParseClean,
    /// Recovery may emit errors but must complete without panic.
    RecoverWithoutPanic,
    /// The fixture is expected to produce a parser error.
    ExpectedError,
    /// Only token output is authoritative for the fixture.
    TokenOnly,
    /// Only source spans are authoritative for the fixture.
    SpanOnly,
}

/// Concept metadata attached to a fixture expectation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SidecarConcept {
    /// Stable concept identifier.
    pub id: String,
    /// Execution tier such as `pr` or `nightly`.
    pub tier: String,
}

/// Required execution expectations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SidecarExpect {
    /// Whether a panic is expected.
    pub panic: bool,
    /// Whether a timeout is expected.
    pub timeout: bool,
    /// Expected parser disposition.
    pub mode: ExpectationMode,
}

/// Optional numeric and structural constraints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SidecarMetrics {
    /// Maximum accepted error-node count.
    pub max_error_nodes: Option<u32>,
    /// Node kinds that must be emitted.
    pub must_emit_node_kinds: Option<Vec<String>>,
}

/// Optional snapshot-surface selection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SidecarSnapshots {
    /// Record or compare token snapshots.
    pub tokens: bool,
    /// Record or compare AST snapshots.
    pub ast: bool,
    /// Record or compare span snapshots.
    pub spans: bool,
}

/// Canonical schema-v1 fixture expectation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FixtureExpectationSidecar {
    /// Required concept identity and tier.
    pub concept: SidecarConcept,
    /// Required execution expectation.
    pub expect: SidecarExpect,
    /// Optional metric constraints.
    pub metrics: Option<SidecarMetrics>,
    /// Optional snapshot selection. When present, all three booleans are explicit.
    pub snapshots: Option<SidecarSnapshots>,
}

/// Version-bearing name for the canonical sidecar model.
pub type FixtureExpectationV1 = FixtureExpectationSidecar;

impl FixtureExpectationSidecar {
    /// Schema identity owned by this model.
    pub const SCHEMA: &'static str = FIXTURE_EXPECTATION_SCHEMA;
}

/// Canonical sidecar validation result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SidecarValidation {
    /// Blocking validation failures.
    pub errors: Vec<String>,
    /// Non-blocking validation findings.
    pub warnings: Vec<String>,
}

impl SidecarValidation {
    /// Whether validation found no blocking errors.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Loaded set of valid concept IDs.
#[derive(Debug, Clone)]
pub struct ConceptRegistry {
    concept_ids: HashSet<String>,
}

impl ConceptRegistry {
    /// Load concept IDs recursively from a TOML registry.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("reading concept registry {}", path.display()))?;
        let value = toml::from_str::<toml::Value>(&raw)
            .with_context(|| format!("parsing concept registry TOML {}", path.display()))?;
        let mut concept_ids = HashSet::new();
        collect_concept_ids(&value, &mut concept_ids);
        Ok(Self { concept_ids })
    }

    /// Build a registry from an existing ID population.
    pub fn from_ids(ids: impl IntoIterator<Item = String>) -> Self {
        Self {
            concept_ids: ids.into_iter().collect(),
        }
    }

    /// Whether the registry contains a concept ID.
    #[must_use]
    pub fn contains(&self, concept_id: &str) -> bool {
        self.concept_ids.contains(concept_id)
    }
}

fn collect_concept_ids(value: &toml::Value, concept_ids: &mut HashSet<String>) {
    match value {
        toml::Value::Table(table) => {
            for (key, item) in table {
                if key == "id"
                    && let toml::Value::String(id) = item
                    && !id.trim().is_empty()
                {
                    concept_ids.insert(id.clone());
                }
                collect_concept_ids(item, concept_ids);
            }
        }
        toml::Value::Array(items) => {
            for item in items {
                collect_concept_ids(item, concept_ids);
            }
        }
        _ => {}
    }
}
