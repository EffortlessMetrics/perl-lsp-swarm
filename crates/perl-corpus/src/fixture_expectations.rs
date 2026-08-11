//! Compatibility adapter for the canonical fixture-expectation sidecar authority.

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
        Self { id: value.id, tier: value.tier }
    }
}

impl From<ConceptInfo> for sidecar::SidecarConcept {
    fn from(value: ConceptInfo) -> Self {
        Self { id: value.id, tier: value.tier }
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
        Self { panic: value.panic, timeout: value.timeout, mode: value.mode.into() }
    }
}

impl From<ExpectBlock> for sidecar::SidecarExpect {
    fn from(value: ExpectBlock) -> Self {
        Self { panic: value.panic, timeout: value.timeout, mode: value.mode.into() }
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
        Self { tokens: Some(value.tokens), ast: Some(value.ast), spans: Some(value.spans) }
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

/// Compatibility validation result with resolved paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarValidation {
    /// Sidecar path that was inspected.
    pub sidecar_path: PathBuf,
    /// Expected paired fixture path.
    pub fixture_path: PathBuf,
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

/// Parse through the canonical schema-v1 authority and convert to the adapter type.
pub fn parse_sidecar(path: &Path) -> Result<FixtureExpectation> {
    sidecar::parse_sidecar(path).map(Into::into)
}

/// Discover sidecars through the canonical deterministic traversal.
pub fn discover_sidecars(root: &Path) -> Result<Vec<PathBuf>> {
    sidecar::discover_sidecars(root)
}

/// Validate through the canonical parser and validator.
pub fn validate_sidecar(
    path: &Path,
    concept_registry: Option<&HashSet<String>>,
) -> SidecarValidation {
    let fixture_path =
        sidecar::expected_fixture_path(path).unwrap_or_else(|_| path.with_extension("pl"));
    let registry = concept_registry
        .map(|values| sidecar::ConceptRegistry::from_ids(values.iter().cloned()));

    let (errors, warnings) = match sidecar::parse_sidecar(path) {
        Ok(parsed) => {
            let validation = sidecar::validate_sidecar(path, &parsed, registry.as_ref());
            (validation.errors, validation.warnings)
        }
        Err(error) => (vec![error.to_string()], Vec::new()),
    };

    SidecarValidation {
        sidecar_path: path.to_path_buf(),
        fixture_path,
        errors,
        warnings,
    }
}

/// Validate every discovered sidecar through the canonical authority.
pub fn validate_sidecars_in_dir(
    root: &Path,
    concept_registry: Option<&HashSet<String>>,
) -> Result<Vec<SidecarValidation>> {
    let sidecars = discover_sidecars(root)?;
    Ok(sidecars.iter().map(|path| validate_sidecar(path, concept_registry)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context as _;
    use std::any::TypeId;
    use std::error::Error;
    use std::fs;

    fn write_fixture_pair(root: &Path, name: &str, raw: &str) -> Result<PathBuf> {
        fs::write(root.join(format!("{name}.pl")), "1;")
            .with_context(|| format!("writing fixture {name}"))?;
        let path = root.join(format!("{name}.meta.toml"));
        fs::write(&path, raw).with_context(|| format!("writing sidecar {name}"))?;
        Ok(path)
    }

    fn valid_toml() -> &'static str {
        r#"
[concept]
id = "parser.example"
tier = "pr"

[expect]
panic = false
timeout = false
mode = "recover_without_panic"

[metrics]
max_error_nodes = 1
must_emit_node_kinds = ["Error"]

[snapshots]
tokens = true
ast = false
spans = true
"#
    }

    #[test]
    fn compatibility_mode_identity_remains_distinct() {
        assert_ne!(TypeId::of::<ExpectationMode>(), TypeId::of::<sidecar::ExpectationMode>());
    }

    #[test]
    fn conversions_preserve_every_mode() {
        let modes = [
            ExpectationMode::ParseClean,
            ExpectationMode::RecoverWithoutPanic,
            ExpectationMode::ExpectedError,
            ExpectationMode::TokenOnly,
            ExpectationMode::SpanOnly,
        ];

        for mode in modes {
            let canonical: sidecar::ExpectationMode = mode.clone().into();
            assert_eq!(ExpectationMode::from(canonical), mode);
        }
    }

    #[test]
    fn direct_deserialization_and_file_parsing_share_canonical_authority(
    ) -> Result<(), Box<dyn Error>> {
        let canonical: sidecar::FixtureExpectationSidecar = toml::from_str(valid_toml())?;
        let adapter: FixtureExpectation = toml::from_str(valid_toml())?;
        assert_eq!(adapter, canonical.clone().into());

        let temporary = tempfile::tempdir()?;
        let path = write_fixture_pair(temporary.path(), "case", valid_toml())?;
        assert_eq!(parse_sidecar(&path)?, canonical.into());
        Ok(())
    }

    #[test]
    fn both_public_models_reject_the_same_invalid_documents() {
        let unknown_field = format!("{}\nunknown = true\n", valid_toml());
        let partial_snapshots = r#"
[concept]
id = "parser.example"
tier = "pr"

[expect]
panic = false
timeout = false
mode = "parse_clean"

[snapshots]
ast = true
"#;
        let missing_concept = r#"
[expect]
panic = false
timeout = false
mode = "parse_clean"
"#;

        for raw in [&unknown_field, partial_snapshots, missing_concept] {
            assert!(toml::from_str::<sidecar::FixtureExpectationSidecar>(raw).is_err());
            assert!(toml::from_str::<FixtureExpectation>(raw).is_err());
        }
    }

    #[test]
    fn adapter_validation_matches_canonical_validation() -> Result<(), Box<dyn Error>> {
        let temporary = tempfile::tempdir()?;
        let path = write_fixture_pair(temporary.path(), "case", valid_toml())?;
        let parsed = sidecar::parse_sidecar(&path)?;
        let canonical = sidecar::validate_sidecar(&path, &parsed, None);
        let adapter = validate_sidecar(&path, None);

        assert_eq!(adapter.errors, canonical.errors);
        assert_eq!(adapter.warnings, canonical.warnings);
        assert_eq!(adapter.fixture_path, sidecar::expected_fixture_path(&path)?);
        Ok(())
    }

    #[test]
    fn adapter_to_canonical_rejects_unrepresentable_legacy_states() {
        let without_concept = FixtureExpectation {
            concept: None,
            expect: ExpectBlock {
                panic: false,
                timeout: false,
                mode: ExpectationMode::ParseClean,
            },
            metrics: None,
            snapshots: None,
        };
        assert!(sidecar::FixtureExpectationSidecar::try_from(without_concept).is_err());

        let partial_snapshots = FixtureExpectation {
            concept: Some(ConceptInfo {
                id: "parser.example".to_string(),
                tier: "pr".to_string(),
            }),
            expect: ExpectBlock {
                panic: false,
                timeout: false,
                mode: ExpectationMode::ParseClean,
            },
            metrics: None,
            snapshots: Some(SnapshotBlock {
                tokens: None,
                ast: Some(true),
                spans: Some(true),
            }),
        };
        assert!(sidecar::FixtureExpectationSidecar::try_from(partial_snapshots).is_err());
    }

    #[test]
    fn validation_uses_registry_semantics_from_canonical_authority(
    ) -> Result<(), Box<dyn Error>> {
        let temporary = tempfile::tempdir()?;
        let path = write_fixture_pair(temporary.path(), "case", valid_toml())?;

        let pending = validate_sidecar(&path, None);
        assert!(pending.is_valid());
        assert!(pending.warnings.iter().any(|warning| warning.contains("resolution pending")));

        let registry = HashSet::from(["parser.other".to_string()]);
        let rejected = validate_sidecar(&path, Some(&registry));
        assert!(!rejected.is_valid());
        assert!(rejected.errors.iter().any(|error| error.contains("not present")));
        Ok(())
    }
}
