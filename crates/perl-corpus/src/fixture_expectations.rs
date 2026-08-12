//! Compatibility adapter for the canonical root-bound fixture-expectation authority.

use anyhow::Result;
use serde::{Deserialize, Deserializer};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::sidecar;

/// Compatibility expectation-mode type retained as a distinct public identity.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
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

impl From<sidecar::ExpectationMode> for ExpectationMode {
    fn from(value: sidecar::ExpectationMode) -> Self {
        match value {
            sidecar::ExpectationMode::ParseClean => Self::ParseClean,
            sidecar::ExpectationMode::RecoverWithoutPanic => Self::RecoverWithoutPanic,
            sidecar::ExpectationMode::ExpectedError => Self::ExpectedError,
            sidecar::ExpectationMode::TokenOnly => Self::TokenOnly,
            sidecar::ExpectationMode::SpanOnly => Self::SpanOnly,
        }
    }
}

impl From<ExpectationMode> for sidecar::ExpectationMode {
    fn from(value: ExpectationMode) -> Self {
        match value {
            ExpectationMode::ParseClean => Self::ParseClean,
            ExpectationMode::RecoverWithoutPanic => Self::RecoverWithoutPanic,
            ExpectationMode::ExpectedError => Self::ExpectedError,
            ExpectationMode::TokenOnly => Self::TokenOnly,
            ExpectationMode::SpanOnly => Self::SpanOnly,
        }
    }
}

/// Compatibility representation of a fixture expectation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureExpectation {
    /// Concept metadata. Canonically parsed sidecars always produce `Some`.
    pub concept: Option<ConceptInfo>,
    /// Required execution expectation.
    pub expect: ExpectBlock,
    /// Optional metric constraints.
    pub metrics: Option<MetricsBlock>,
    /// Optional snapshot selection.
    pub snapshots: Option<SnapshotBlock>,
}

impl<'de> Deserialize<'de> for FixtureExpectation {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        sidecar::FixtureExpectationSidecar::deserialize(deserializer).map(Self::from)
    }
}

impl From<sidecar::FixtureExpectationSidecar> for FixtureExpectation {
    fn from(value: sidecar::FixtureExpectationSidecar) -> Self {
        Self {
            concept: Some(value.concept.into()),
            expect: value.expect.into(),
            metrics: value.metrics.map(Into::into),
            snapshots: value.snapshots.map(Into::into),
        }
    }
}

impl TryFrom<FixtureExpectation> for sidecar::FixtureExpectationSidecar {
    type Error = anyhow::Error;

    fn try_from(value: FixtureExpectation) -> Result<Self> {
        let concept = value.concept.ok_or_else(|| {
            anyhow::anyhow!(
                "concept block is required by canonical schema {}",
                sidecar::FIXTURE_EXPECTATION_SCHEMA
            )
        })?;
        Ok(Self {
            concept: concept.into(),
            expect: value.expect.into(),
            metrics: value.metrics.map(Into::into),
            snapshots: value.snapshots.map(TryInto::try_into).transpose()?,
        })
    }
}

/// Compatibility concept metadata.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConceptInfo {
    /// Stable concept identifier.
    pub id: String,
    /// Execution tier.
    pub tier: String,
}

impl From<sidecar::SidecarConcept> for ConceptInfo {
    fn from(value: sidecar::SidecarConcept) -> Self {
        Self {
            id: value.id,
            tier: value.tier,
        }
    }
}

impl From<ConceptInfo> for sidecar::SidecarConcept {
    fn from(value: ConceptInfo) -> Self {
        Self {
            id: value.id,
            tier: value.tier,
        }
    }
}

/// Compatibility execution expectation.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExpectBlock {
    /// Whether a panic is expected.
    pub panic: bool,
    /// Whether a timeout is expected.
    pub timeout: bool,
    /// Expected parser disposition.
    pub mode: ExpectationMode,
}

impl From<sidecar::SidecarExpect> for ExpectBlock {
    fn from(value: sidecar::SidecarExpect) -> Self {
        Self {
            panic: value.panic,
            timeout: value.timeout,
            mode: value.mode.into(),
        }
    }
}

impl From<ExpectBlock> for sidecar::SidecarExpect {
    fn from(value: ExpectBlock) -> Self {
        Self {
            panic: value.panic,
            timeout: value.timeout,
            mode: value.mode.into(),
        }
    }
}

/// Compatibility metric constraints.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct MetricsBlock {
    /// Maximum accepted error-node count.
    pub max_error_nodes: Option<u32>,
    /// Node kinds that must be emitted.
    pub must_emit_node_kinds: Option<Vec<String>>,
}

impl From<sidecar::SidecarMetrics> for MetricsBlock {
    fn from(value: sidecar::SidecarMetrics) -> Self {
        Self {
            max_error_nodes: value.max_error_nodes,
            must_emit_node_kinds: value.must_emit_node_kinds,
        }
    }
}

impl From<MetricsBlock> for sidecar::SidecarMetrics {
    fn from(value: MetricsBlock) -> Self {
        Self {
            max_error_nodes: value.max_error_nodes,
            must_emit_node_kinds: value.must_emit_node_kinds,
        }
    }
}

/// Compatibility snapshot selection.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct SnapshotBlock {
    /// Token snapshot selection.
    pub tokens: Option<bool>,
    /// AST snapshot selection.
    pub ast: Option<bool>,
    /// Span snapshot selection.
    pub spans: Option<bool>,
}

impl From<sidecar::SidecarSnapshots> for SnapshotBlock {
    fn from(value: sidecar::SidecarSnapshots) -> Self {
        Self {
            tokens: Some(value.tokens),
            ast: Some(value.ast),
            spans: Some(value.spans),
        }
    }
}

impl TryFrom<SnapshotBlock> for sidecar::SidecarSnapshots {
    type Error = anyhow::Error;

    fn try_from(value: SnapshotBlock) -> Result<Self> {
        Ok(Self {
            tokens: value.tokens.ok_or_else(|| {
                anyhow::anyhow!("snapshots.tokens is required by canonical schema")
            })?,
            ast: value
                .ast
                .ok_or_else(|| anyhow::anyhow!("snapshots.ast is required by canonical schema"))?,
            spans: value.spans.ok_or_else(|| {
                anyhow::anyhow!("snapshots.spans is required by canonical schema")
            })?,
        })
    }
}

/// Compatibility validation result with portable resolved identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarValidation {
    /// Root-relative sidecar path that was inspected.
    pub sidecar_path: PathBuf,
    /// Root-relative paired fixture path, when path authority succeeded.
    pub fixture_path: Option<PathBuf>,
    /// Blocking validation failures.
    pub errors: Vec<String>,
    /// Non-blocking validation findings.
    pub warnings: Vec<String>,
}

impl SidecarValidation {
    /// Whether validation found no blocking errors.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Parse through the canonical root-bound schema authority.
pub fn parse_sidecar(
    context: &sidecar::SidecarValidationContext,
    path: &Path,
) -> Result<FixtureExpectation> {
    sidecar::parse_sidecar(context, path).map(Into::into)
}

/// Discover one exact root-bound sidecar population.
pub fn discover_sidecars(root: &Path) -> Result<sidecar::SidecarValidationContext> {
    sidecar::SidecarValidationContext::discover(root)
}

/// Validate through the canonical parser, path authority, and semantic validator.
pub fn validate_sidecar(
    context: &sidecar::SidecarValidationContext,
    path: &Path,
    concept_registry: Option<&HashSet<String>>,
) -> SidecarValidation {
    let registry =
        concept_registry.map(|values| sidecar::ConceptRegistry::from_ids(values.iter().cloned()));
    let pair = context.resolve_pair(path);
    let (fixture_path, path_error) = match pair {
        Ok(pair) => (Some(pair.identity().fixture_path.clone()), None),
        Err(error) => (None, Some(error.to_string())),
    };

    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    match sidecar::parse_sidecar(context, path) {
        Ok(parsed) => {
            let validation = sidecar::validate_sidecar(context, path, &parsed, registry.as_ref());
            errors = validation.errors;
            warnings = validation.warnings;
        }
        Err(error) => errors.push(error.to_string()),
    }
    if let Some(path_error) = path_error
        && !errors.contains(&path_error)
    {
        errors.insert(0, path_error);
    }

    let sidecar_path = if path.is_absolute() {
        path.strip_prefix(context.root())
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| PathBuf::from("<outside-root>"))
    } else {
        path.to_path_buf()
    };
    SidecarValidation {
        sidecar_path,
        fixture_path,
        errors,
        warnings,
    }
}

/// Validate every member of one exact discovered population.
pub fn validate_sidecars_in_dir(
    root: &Path,
    concept_registry: Option<&HashSet<String>>,
) -> Result<(sidecar::SidecarValidationContext, Vec<SidecarValidation>)> {
    let context = discover_sidecars(root)?;
    let sidecars = context
        .sidecars()
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    let validations = sidecars
        .iter()
        .map(|path| validate_sidecar(&context, path, concept_registry))
        .collect();
    Ok((context, validations))
}
