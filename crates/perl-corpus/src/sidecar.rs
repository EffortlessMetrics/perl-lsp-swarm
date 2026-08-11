//! Canonical versioned fixture-expectation sidecar model and validation.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

const SIDECAR_SUFFIX: &[u8] = b".meta.toml";

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
        Self { concept_ids: ids.into_iter().collect() }
    }

    /// Whether the registry contains a concept ID.
    #[must_use]
    pub fn contains(&self, concept_id: &str) -> bool {
        self.concept_ids.contains(concept_id)
    }
}

/// Parse canonical schema-v1 TOML from memory.
pub fn parse_sidecar_str(raw: &str) -> Result<FixtureExpectationSidecar> {
    toml::from_str(raw).context("deserializing fixture expectation schema fixture_expectation.v1")
}

/// Read and parse a canonical schema-v1 sidecar.
pub fn parse_sidecar(path: &Path) -> Result<FixtureExpectationSidecar> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("reading sidecar {}", path.display()))?;
    parse_sidecar_str(&raw).with_context(|| format!("parsing sidecar {}", path.display()))
}

/// Resolve the `.pl` fixture paired with a `.meta.toml` sidecar.
pub fn expected_fixture_path(sidecar_path: &Path) -> Result<PathBuf> {
    let file_name = sidecar_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid sidecar path: {}", sidecar_path.display()))?;

    if !file_name.ends_with(".meta.toml") {
        bail!("sidecar filename must end with .meta.toml: {}", sidecar_path.display());
    }

    let fixture_stem = file_name.trim_end_matches(".meta.toml");
    if fixture_stem.is_empty() {
        bail!("fixture stem must not be empty: {}", sidecar_path.display());
    }

    let parent = sidecar_path.parent().unwrap_or_else(|| Path::new("."));
    Ok(parent.join(format!("{fixture_stem}.pl")))
}

/// Validate a parsed canonical sidecar against its fixture and optional registry.
pub fn validate_sidecar(
    sidecar_path: &Path,
    sidecar: &FixtureExpectationSidecar,
    concept_registry: Option<&ConceptRegistry>,
) -> SidecarValidation {
    let mut validation = SidecarValidation::default();

    match expected_fixture_path(sidecar_path) {
        Ok(fixture_path) => match fs::symlink_metadata(&fixture_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => validation.errors.push(format!(
                "fixture file symlink is unsupported: {}",
                fixture_path.display()
            )),
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => validation.errors.push(format!(
                "fixture file is not a regular file: {}",
                fixture_path.display()
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => validation
                .errors
                .push(format!("fixture file does not exist: {}", fixture_path.display())),
            Err(error) => validation.errors.push(format!(
                "failed to inspect fixture file {}: {error}",
                fixture_path.display()
            )),
        },
        Err(error) => validation.errors.push(error.to_string()),
    }

    if sidecar.concept.id.trim().is_empty() {
        validation.errors.push("concept.id must not be empty".to_string());
    } else if let Some(registry) = concept_registry {
        if !registry.contains(&sidecar.concept.id) {
            validation.errors.push(format!(
                "concept.id '{}' is not present in the loaded concept registry",
                sidecar.concept.id
            ));
        }
    } else {
        validation.warnings.push(format!(
            "concept registry unavailable; concept resolution pending for '{}'",
            sidecar.concept.id
        ));
    }

    if sidecar.concept.tier.trim().is_empty() {
        validation.errors.push("concept.tier must not be empty".to_string());
    }

    validation
}

/// Read and validate a canonical sidecar.
pub fn load_and_validate_sidecar(
    sidecar_path: &Path,
    concept_registry: Option<&ConceptRegistry>,
) -> Result<SidecarValidation> {
    let sidecar = parse_sidecar(sidecar_path)?;
    Ok(validate_sidecar(sidecar_path, &sidecar, concept_registry))
}

/// Discover schema sidecars recursively in deterministic path order.
pub fn discover_sidecars(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut sidecars = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory)
            .with_context(|| format!("reading directory {}", directory.display()))?;
        for entry in entries {
            let entry =
                entry.with_context(|| format!("reading entry in {}", directory.display()))?;
            let path = entry.path();
            let file_name = entry.file_name();
            let is_sidecar = has_sidecar_suffix(&file_name);
            let file_type = entry
                .file_type()
                .with_context(|| format!("getting file type for {}", path.display()))?;

            if file_type.is_symlink() {
                bail!("sidecar discovery symlink is unsupported: {}", path.display());
            }
            if file_type.is_dir() {
                stack.push(path);
            } else if is_sidecar {
                if !file_type.is_file() {
                    bail!("sidecar is not a regular file: {}", path.display());
                }
                if file_name.to_str().is_none() {
                    bail!("sidecar filename is not valid UTF-8: {}", path.display());
                }
                sidecars.push(path);
            }
        }
    }

    sidecars.sort();
    Ok(sidecars)
}

fn has_sidecar_suffix(file_name: &OsStr) -> bool {
    file_name.as_encoded_bytes().ends_with(SIDECAR_SUFFIX)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    fn minimal_sidecar_toml(id: &str, mode: &str) -> String {
        format!(
            r#"
[concept]
id = "{id}"
tier = "pr"

[expect]
panic = false
timeout = false
mode = "{mode}"
"#
        )
    }

    fn write_pair(root: &Path, name: &str, raw: &str) -> Result<PathBuf> {
        let fixture = root.join(format!("{name}.pl"));
        fs::write(&fixture, "1;")?;
        let sidecar = root.join(format!("{name}.meta.toml"));
        fs::write(&sidecar, raw)?;
        Ok(sidecar)
    }

    #[test]
    fn canonical_schema_identity_is_v1() {
        assert_eq!(FixtureExpectationSidecar::SCHEMA, "fixture_expectation.v1");
    }

    #[test]
    fn all_mode_tokens_parse_through_canonical_model() -> Result<(), Box<dyn Error>> {
        let cases = [
            ("parse_clean", ExpectationMode::ParseClean),
            ("recover_without_panic", ExpectationMode::RecoverWithoutPanic),
            ("expected_error", ExpectationMode::ExpectedError),
            ("token_only", ExpectationMode::TokenOnly),
            ("span_only", ExpectationMode::SpanOnly),
        ];

        for (token, expected) in cases {
            let parsed = parse_sidecar_str(&minimal_sidecar_toml("parser.example", token))?;
            assert_eq!(parsed.expect.mode, expected);
        }
        Ok(())
    }

    #[test]
    fn canonical_parser_rejects_unknown_mode_and_fields() {
        let unknown_mode = minimal_sidecar_toml("parser.example", "unknown");
        assert!(parse_sidecar_str(&unknown_mode).is_err());

        let unknown_field =
            format!("{}\nextra = true\n", minimal_sidecar_toml("parser.example", "parse_clean"));
        assert!(parse_sidecar_str(&unknown_field).is_err());
    }

    #[test]
    fn canonical_parser_requires_concept_and_complete_snapshot_block() {
        let no_concept = r#"
[expect]
panic = false
timeout = false
mode = "parse_clean"
"#;
        assert!(parse_sidecar_str(no_concept).is_err());

        let partial_snapshots = format!(
            "{}\n[snapshots]\nast = true\n",
            minimal_sidecar_toml("parser.example", "parse_clean")
        );
        assert!(parse_sidecar_str(&partial_snapshots).is_err());
    }

    #[test]
    fn expected_fixture_path_is_strict() -> Result<(), Box<dyn Error>> {
        assert_eq!(
            expected_fixture_path(Path::new("nested/case.meta.toml"))?,
            Path::new("nested/case.pl")
        );
        assert!(expected_fixture_path(Path::new("nested/case.toml")).is_err());
        assert!(expected_fixture_path(Path::new(".meta.toml")).is_err());
        Ok(())
    }

    #[test]
    fn validation_owns_fixture_concept_and_registry_rules() -> Result<(), Box<dyn Error>> {
        let temporary = tempfile::tempdir()?;
        let raw = minimal_sidecar_toml("parser.example", "parse_clean");
        let sidecar_path = write_pair(temporary.path(), "case", &raw)?;
        let parsed = parse_sidecar(&sidecar_path)?;

        let pending = validate_sidecar(&sidecar_path, &parsed, None);
        assert!(pending.is_ok());
        assert!(pending.warnings.iter().any(|warning| warning.contains("resolution pending")));

        let registry = ConceptRegistry::from_ids(["parser.other".to_string()]);
        let rejected = validate_sidecar(&sidecar_path, &parsed, Some(&registry));
        assert!(!rejected.is_ok());
        assert!(rejected.errors.iter().any(|error| error.contains("not present")));
        Ok(())
    }

    #[test]
    fn validation_rejects_missing_fixture_and_empty_tier() -> Result<(), Box<dyn Error>> {
        let temporary = tempfile::tempdir()?;
        let sidecar_path = temporary.path().join("missing.meta.toml");
        let mut parsed = parse_sidecar_str(&minimal_sidecar_toml("parser.example", "parse_clean"))?;
        parsed.concept.tier.clear();

        let validation = validate_sidecar(&sidecar_path, &parsed, None);
        assert!(!validation.is_ok());
        assert!(validation.errors.iter().any(|error| error.contains("fixture file")));
        assert!(validation.errors.iter().any(|error| error.contains("concept.tier")));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn validation_rejects_paired_fixture_symlink() -> Result<(), Box<dyn Error>> {
        use std::os::unix::fs::symlink;

        let corpus = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let target = outside.path().join("case.pl");
        fs::write(&target, "1;")?;
        let fixture_link = corpus.path().join("case.pl");
        symlink(&target, &fixture_link)?;
        let sidecar_path = corpus.path().join("case.meta.toml");
        fs::write(
            &sidecar_path,
            minimal_sidecar_toml("parser.example", "parse_clean"),
        )?;
        let parsed = parse_sidecar(&sidecar_path)?;

        let validation = validate_sidecar(&sidecar_path, &parsed, None);
        assert!(!validation.is_ok());
        assert!(validation.errors.iter().any(|error| error.contains("symlink")));
        Ok(())
    }

    #[test]
    fn discovery_is_recursive_and_sorted() -> Result<(), Box<dyn Error>> {
        let temporary = tempfile::tempdir()?;
        fs::create_dir_all(temporary.path().join("z"))?;
        fs::create_dir_all(temporary.path().join("a"))?;
        fs::write(temporary.path().join("z/second.meta.toml"), "")?;
        fs::write(temporary.path().join("a/first.meta.toml"), "")?;
        fs::write(temporary.path().join("a/ignore.toml"), "")?;

        let discovered = discover_sidecars(temporary.path())?;
        assert_eq!(
            discovered,
            vec![
                temporary.path().join("a/first.meta.toml"),
                temporary.path().join("z/second.meta.toml"),
            ]
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn discovery_rejects_non_utf8_sidecar_names() -> Result<(), Box<dyn Error>> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let temporary = tempfile::tempdir()?;
        let mut bytes = vec![b'c', b'a', b's', b'e', 0xff];
        bytes.extend_from_slice(SIDECAR_SUFFIX);
        let path = temporary.path().join(OsString::from_vec(bytes));
        fs::write(&path, "")?;

        let error = discover_sidecars(temporary.path())
            .expect_err("selected non-UTF-8 sidecar names must fail closed");
        assert!(error.to_string().contains("not valid UTF-8"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn discovery_ignores_unselected_non_utf8_metadata() -> Result<(), Box<dyn Error>> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let temporary = tempfile::tempdir()?;
        let path = temporary
            .path()
            .join(OsString::from_vec(vec![b'n', b'o', b't', b'e', 0xff, b'.', b't', b'x', b't']));
        fs::write(path, "")?;

        assert!(discover_sidecars(temporary.path())?.is_empty());
        Ok(())
    }

    #[test]
    fn registry_loader_collects_nested_ids() -> Result<(), Box<dyn Error>> {
        let temporary = tempfile::tempdir()?;
        let registry_path = temporary.path().join("concepts.toml");
        fs::write(
            &registry_path,
            r#"
[[concepts]]
id = "parser.one"

[groups.nested]
id = "parser.two"
"#,
        )?;

        let registry = ConceptRegistry::load(&registry_path)?;
        assert!(registry.contains("parser.one"));
        assert!(registry.contains("parser.two"));
        Ok(())
    }
}
