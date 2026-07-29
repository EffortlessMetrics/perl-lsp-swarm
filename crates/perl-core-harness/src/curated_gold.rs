//! Read-only loading and comparison for independently authored semantic gold.
//!
//! This module deliberately has no accept, bless, or update operation. A gold
//! fixture is valid only when its source and expectation hashes match the
//! independently reviewed file on disk. The comparator consumes facts from a
//! typed adapter and emits a deterministic receipt; it never writes fixtures.

use color_eyre::eyre::{Context, Result, bail};
use perl_core_harness_types::{
    CURATED_GOLD_COMPARISON_SCHEMA_VERSION, CURATED_GOLD_SCHEMA_VERSION, CuratedGoldComparisonItem,
    CuratedGoldComparisonKind, CuratedGoldComparisonReceipt, CuratedGoldComparisonStatus,
    CuratedGoldFact, CuratedGoldFactClass, CuratedGoldFixture, CuratedGoldSource,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Input paths and revision constraints for the read-only gold loader.
#[derive(Debug, Clone)]
pub struct CuratedGoldLoadConfig {
    /// JSON fixture to load.
    pub fixture_path: PathBuf,
    /// Repository root used by repository-backed sources.
    pub repository_root: Option<PathBuf>,
    /// Optional revision that a repository-backed source must declare.
    pub expected_revision: Option<String>,
}

/// A validated fixture together with the exact source bytes it declares.
#[derive(Debug, Clone)]
pub struct LoadedCuratedGold {
    /// Validated fixture metadata and independent expectations.
    pub fixture: CuratedGoldFixture,
    /// Source text whose content hash was validated against the fixture.
    pub source_text: String,
}

/// Adapter boundary for a concrete compiler fact extractor.
pub trait CuratedGoldFactAdapter {
    /// Fact class implemented by this adapter.
    fn fact_class(&self) -> CuratedGoldFactClass;

    /// Extract normalized facts from the declared source.
    fn extract_facts(&self, source: &str) -> Result<Vec<CuratedGoldFact>>;
}

#[derive(Debug, Serialize)]
struct ExpectationDigest<'a> {
    expected_facts: Vec<&'a CuratedGoldFact>,
    allowed_dynamic_boundaries: Vec<String>,
}

/// Return a lower-case SHA-256 digest in the repository's `sha256:` format.
pub fn sha256_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let hex = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    format!("sha256:{hex}")
}

/// Calculate the canonical expectation digest used by `curated_gold.v1`.
pub fn curated_gold_expectation_hash(
    expected_facts: &[CuratedGoldFact],
    allowed_dynamic_boundaries: &[String],
) -> Result<String> {
    let mut facts = expected_facts.iter().collect::<Vec<_>>();
    facts.sort_by(|left, right| left.fact_id.cmp(&right.fact_id));
    let mut boundaries = allowed_dynamic_boundaries.to_vec();
    boundaries.sort();
    serde_json::to_vec(&ExpectationDigest {
        expected_facts: facts,
        allowed_dynamic_boundaries: boundaries,
    })
    .map(|bytes| sha256_digest(&bytes))
    .context("serializing curated-gold expectation digest input")
}

/// Load and validate one curated-gold fixture without changing it.
pub fn load_curated_gold(config: &CuratedGoldLoadConfig) -> Result<LoadedCuratedGold> {
    let fixture_bytes = fs::read(&config.fixture_path).with_context(|| {
        format!("reading curated-gold fixture {}", config.fixture_path.display())
    })?;
    let fixture: CuratedGoldFixture =
        serde_json::from_slice(&fixture_bytes).with_context(|| {
            format!("decoding curated-gold fixture {}", config.fixture_path.display())
        })?;
    validate_fixture(&fixture)?;
    let source_bytes = source_bytes(&fixture.source, config)?;
    let source_text = String::from_utf8(source_bytes.clone())
        .context("curated-gold source must be valid UTF-8")?;
    if fixture.source_content_hash != sha256_digest(&source_bytes) {
        bail!("curated-gold source content hash does not match fixture");
    }
    let expectation_hash = curated_gold_expectation_hash(
        &fixture.expected_facts,
        &fixture.allowed_dynamic_boundaries,
    )?;
    if fixture.expectation_hash != expectation_hash {
        bail!("curated-gold expectation hash does not match fixture");
    }
    Ok(LoadedCuratedGold { fixture, source_text })
}

/// Compare a validated fixture against a typed adapter and return a deterministic receipt.
pub fn compare_curated_gold(
    loaded: &LoadedCuratedGold,
    adapter: Option<&dyn CuratedGoldFactAdapter>,
    compiler_capability: &str,
) -> Result<CuratedGoldComparisonReceipt> {
    let fixture = &loaded.fixture;
    if compiler_capability != fixture.minimum_compiler_capability {
        return Ok(receipt(
            fixture,
            compiler_capability,
            CuratedGoldComparisonStatus::StaleFixture,
            vec![CuratedGoldComparisonItem {
                kind: CuratedGoldComparisonKind::StaleFixture,
                fact_id: None,
                detail: format!(
                    "fixture requires compiler capability {}, observed {compiler_capability}",
                    fixture.minimum_compiler_capability
                ),
            }],
        ));
    }
    let Some(adapter) = adapter else {
        return Ok(receipt(
            fixture,
            compiler_capability,
            CuratedGoldComparisonStatus::UnimplementedFactClass,
            vec![unimplemented_item(fixture.fact_class)],
        ));
    };
    if adapter.fact_class() != fixture.fact_class {
        return Ok(receipt(
            fixture,
            compiler_capability,
            CuratedGoldComparisonStatus::UnimplementedFactClass,
            vec![unimplemented_item(fixture.fact_class)],
        ));
    }
    let actual_facts = adapter.extract_facts(&loaded.source_text)?;
    validate_facts(&actual_facts, "observed")?;
    let comparisons = compare_facts(fixture, &actual_facts)?;
    let status = if comparisons.iter().all(|item| {
        matches!(
            item.kind,
            CuratedGoldComparisonKind::ExactAgreement
                | CuratedGoldComparisonKind::ExpectedDynamicBoundary
        )
    }) {
        CuratedGoldComparisonStatus::ExactAgreement
    } else {
        CuratedGoldComparisonStatus::HasMismatches
    };
    Ok(receipt(fixture, compiler_capability, status, comparisons))
}

fn receipt(
    fixture: &CuratedGoldFixture,
    compiler_capability: &str,
    status: CuratedGoldComparisonStatus,
    mut comparisons: Vec<CuratedGoldComparisonItem>,
) -> CuratedGoldComparisonReceipt {
    comparisons.sort_by(|left, right| {
        left.fact_id
            .cmp(&right.fact_id)
            .then(left.kind.cmp(&right.kind))
            .then(left.detail.cmp(&right.detail))
    });
    CuratedGoldComparisonReceipt {
        schema_version: CURATED_GOLD_COMPARISON_SCHEMA_VERSION.to_string(),
        fixture_id: fixture.fixture_id.clone(),
        fact_class: fixture.fact_class,
        source_content_hash: fixture.source_content_hash.clone(),
        expectation_hash: fixture.expectation_hash.clone(),
        compiler_capability: compiler_capability.to_string(),
        status,
        comparisons,
    }
}

fn unimplemented_item(fact_class: CuratedGoldFactClass) -> CuratedGoldComparisonItem {
    CuratedGoldComparisonItem {
        kind: CuratedGoldComparisonKind::UnimplementedFactClass,
        fact_id: None,
        detail: format!("no adapter is implemented for {fact_class}"),
    }
}

fn source_bytes(source: &CuratedGoldSource, config: &CuratedGoldLoadConfig) -> Result<Vec<u8>> {
    match source {
        CuratedGoldSource::Inline { text } => Ok(text.as_bytes().to_vec()),
        CuratedGoldSource::Repository { path, revision } => {
            if path.is_empty() || !is_safe_relative_path(Path::new(path)) {
                bail!("curated-gold repository source path is not safe: {path}");
            }
            if let Some(expected_revision) = &config.expected_revision
                && revision != expected_revision
            {
                bail!("curated-gold source revision does not match the requested revision");
            }
            let Some(root) = &config.repository_root else {
                bail!("repository root is required for a repository-backed gold source");
            };
            fs::read(root.join(path)).with_context(|| format!("reading curated-gold source {path}"))
        }
    }
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::ParentDir | Component::Prefix(_)))
}

fn validate_fixture(fixture: &CuratedGoldFixture) -> Result<()> {
    if fixture.schema_version != CURATED_GOLD_SCHEMA_VERSION {
        bail!("unsupported curated-gold schema version {}", fixture.schema_version);
    }
    for (name, value) in [
        ("fixture_id", fixture.fixture_id.as_str()),
        ("author_identity", fixture.author_identity.as_str()),
        ("reviewer_identity", fixture.reviewer_identity.as_str()),
        ("review_receipt", fixture.review_receipt.as_str()),
        ("rationale", fixture.rationale.as_str()),
        ("coverage_intent", fixture.coverage_intent.as_str()),
        ("minimum_compiler_capability", fixture.minimum_compiler_capability.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("curated-gold {name} must not be empty");
        }
    }
    if fixture.author_identity == fixture.reviewer_identity {
        bail!("curated-gold reviewer must be independent from the author");
    }
    validate_digest(&fixture.source_content_hash, "source_content_hash")?;
    validate_digest(&fixture.expectation_hash, "expectation_hash")?;
    validate_facts(&fixture.expected_facts, "expected")?;
    let mut boundaries = fixture.allowed_dynamic_boundaries.clone();
    boundaries.sort();
    if boundaries.windows(2).any(|pair| pair[0] == pair[1]) {
        bail!("curated-gold dynamic boundaries must be unique");
    }
    Ok(())
}

fn validate_digest(value: &str, field: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        bail!("curated-gold {field} must use sha256:<64 lowercase hex digits>");
    };
    if hex.len() != 64
        || !hex.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("curated-gold {field} must use sha256:<64 lowercase hex digits>");
    }
    Ok(())
}

fn validate_facts(facts: &[CuratedGoldFact], label: &str) -> Result<()> {
    let mut ids = BTreeSet::new();
    for fact in facts {
        if fact.fact_id.trim().is_empty() {
            bail!("curated-gold {label} fact id must not be empty");
        }
        if !ids.insert(&fact.fact_id) {
            bail!("curated-gold {label} fact ids must be unique: {}", fact.fact_id);
        }
        if let Some(range) = fact.range
            && range.start_byte > range.end_byte
        {
            bail!("curated-gold {label} fact range is inverted: {}", fact.fact_id);
        }
        if fact.provenance.trim().is_empty() {
            bail!("curated-gold {label} fact provenance must not be empty: {}", fact.fact_id);
        }
    }
    Ok(())
}

fn compare_facts(
    fixture: &CuratedGoldFixture,
    actual_facts: &[CuratedGoldFact],
) -> Result<Vec<CuratedGoldComparisonItem>> {
    let expected = fixture
        .expected_facts
        .iter()
        .map(|fact| (fact.fact_id.clone(), fact))
        .collect::<BTreeMap<_, _>>();
    let actual =
        actual_facts.iter().map(|fact| (fact.fact_id.clone(), fact)).collect::<BTreeMap<_, _>>();
    let ids = expected.keys().chain(actual.keys()).cloned().collect::<BTreeSet<_>>();
    let mut comparisons = Vec::new();
    for fact_id in ids {
        match (expected.get(&fact_id), actual.get(&fact_id)) {
            (Some(_), None) => comparisons.push(CuratedGoldComparisonItem {
                kind: CuratedGoldComparisonKind::MissingFact,
                fact_id: Some(fact_id),
                detail: "expected fact was not observed".to_string(),
            }),
            (None, Some(_)) => comparisons.push(CuratedGoldComparisonItem {
                kind: CuratedGoldComparisonKind::ExtraFact,
                fact_id: Some(fact_id),
                detail: "observed fact was not independently expected".to_string(),
            }),
            (Some(expected), Some(actual)) => {
                compare_fact_pair(fixture, expected, actual, &mut comparisons)
            }
            (None, None) => bail!("curated-gold comparison encountered an empty fact pair"),
        }
    }
    Ok(comparisons)
}

fn compare_fact_pair(
    fixture: &CuratedGoldFixture,
    expected: &CuratedGoldFact,
    actual: &CuratedGoldFact,
    comparisons: &mut Vec<CuratedGoldComparisonItem>,
) {
    let fact_id = Some(expected.fact_id.clone());
    let mut matched = true;
    if expected.value != actual.value {
        matched = false;
        comparisons.push(CuratedGoldComparisonItem {
            kind: CuratedGoldComparisonKind::ValueMismatch,
            fact_id: fact_id.clone(),
            detail: "expected value differs from observed value".to_string(),
        });
    }
    if expected.range != actual.range {
        matched = false;
        comparisons.push(CuratedGoldComparisonItem {
            kind: CuratedGoldComparisonKind::RangeMismatch,
            fact_id: fact_id.clone(),
            detail: "expected source range differs from observed range".to_string(),
        });
    }
    if expected.provenance != actual.provenance {
        matched = false;
        comparisons.push(CuratedGoldComparisonItem {
            kind: CuratedGoldComparisonKind::ProvenanceMismatch,
            fact_id: fact_id.clone(),
            detail: "expected provenance differs from observed provenance".to_string(),
        });
    }
    if expected.confidence != actual.confidence || expected.freshness != actual.freshness {
        matched = false;
        comparisons.push(CuratedGoldComparisonItem {
            kind: CuratedGoldComparisonKind::ConfidenceOrFreshnessMismatch,
            fact_id: fact_id.clone(),
            detail: "expected confidence or freshness differs from observed fact".to_string(),
        });
    }
    if let Some(boundary) = &actual.dynamic_boundary {
        matched = false;
        let allowed = fixture.allowed_dynamic_boundaries.iter().any(|item| item == boundary);
        let expected_boundary = expected.dynamic_boundary.as_deref() == Some(boundary.as_str());
        comparisons.push(CuratedGoldComparisonItem {
            kind: if allowed && expected_boundary {
                CuratedGoldComparisonKind::ExpectedDynamicBoundary
            } else {
                CuratedGoldComparisonKind::UnexpectedDynamicBoundary
            },
            fact_id: fact_id.clone(),
            detail: if allowed && expected_boundary {
                format!("dynamic boundary {boundary} is explicitly allowed")
            } else {
                format!("dynamic boundary {boundary} is not an expected fixture boundary")
            },
        });
    } else if expected.dynamic_boundary.is_some() {
        matched = false;
        comparisons.push(CuratedGoldComparisonItem {
            kind: CuratedGoldComparisonKind::ValueMismatch,
            fact_id: fact_id.clone(),
            detail: "expected dynamic boundary was not observed".to_string(),
        });
    }
    if matched {
        comparisons.push(CuratedGoldComparisonItem {
            kind: CuratedGoldComparisonKind::ExactAgreement,
            fact_id: fact_id.clone(),
            detail: "expected and observed fact agree".to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::Result;
    use perl_core_harness_types::{CuratedGoldConfidence, CuratedGoldFreshness, CuratedGoldRange};
    use serde_json::json;
    use std::time::SystemTime;
    use tempfile::tempdir;

    struct PackageAdapter {
        facts: Vec<CuratedGoldFact>,
    }

    impl CuratedGoldFactAdapter for PackageAdapter {
        fn fact_class(&self) -> CuratedGoldFactClass {
            CuratedGoldFactClass::PackageSubTable
        }

        fn extract_facts(&self, _source: &str) -> Result<Vec<CuratedGoldFact>> {
            Ok(self.facts.clone())
        }
    }

    fn fact() -> CuratedGoldFact {
        CuratedGoldFact {
            fact_id: "package:main".to_string(),
            value: json!({"name": "main", "members": ["run"]}),
            range: Some(CuratedGoldRange { start_byte: 0, end_byte: 12 }),
            provenance: "ExplicitSource".to_string(),
            confidence: CuratedGoldConfidence::High,
            freshness: CuratedGoldFreshness::Fresh,
            dynamic_boundary: None,
        }
    }

    fn fixture() -> Result<CuratedGoldFixture> {
        let source = "package main;\n".to_string();
        let expected_facts = vec![fact()];
        Ok(CuratedGoldFixture {
            schema_version: CURATED_GOLD_SCHEMA_VERSION.to_string(),
            fixture_id: "gold.package.main".to_string(),
            fact_class: CuratedGoldFactClass::PackageSubTable,
            source: CuratedGoldSource::Inline { text: source.clone() },
            source_content_hash: sha256_digest(source.as_bytes()),
            expected_facts: expected_facts.clone(),
            expectation_hash: curated_gold_expectation_hash(&expected_facts, &[])?,
            author_identity: "author@example.test".to_string(),
            reviewer_identity: "reviewer@example.test".to_string(),
            review_receipt: "review-123".to_string(),
            rationale: "package declaration has a source-backed package fact".to_string(),
            coverage_intent: "package table basics".to_string(),
            confidence: CuratedGoldConfidence::High,
            perl_references: vec!["perlfunc/package".to_string()],
            allowed_dynamic_boundaries: Vec::new(),
            minimum_compiler_capability: "compiler.facts.v1".to_string(),
        })
    }

    fn write_fixture(dir: &Path, fixture: &CuratedGoldFixture) -> Result<PathBuf> {
        let path = dir.join("fixture.json");
        let bytes = serde_json::to_vec_pretty(fixture)?;
        fs::write(&path, bytes)?;
        Ok(path)
    }

    fn load_fixture(dir: &Path) -> Result<(PathBuf, LoadedCuratedGold)> {
        let path = write_fixture(dir, &fixture()?)?;
        let loaded = load_curated_gold(&CuratedGoldLoadConfig {
            fixture_path: path.clone(),
            repository_root: None,
            expected_revision: None,
        })?;
        Ok((path, loaded))
    }

    fn has_kind(receipt: &CuratedGoldComparisonReceipt, kind: CuratedGoldComparisonKind) -> bool {
        receipt.comparisons.iter().any(|item| item.kind == kind)
    }

    #[test]
    fn valid_fixture_loads_without_mutation() -> Result<()> {
        let dir = tempdir()?;
        let (path, loaded) = load_fixture(dir.path())?;
        let before = fs::read(&path)?;
        let modified_before = fs::metadata(&path)?.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let adapter = PackageAdapter { facts: vec![fact()] };
        let receipt = compare_curated_gold(&loaded, Some(&adapter), "compiler.facts.v1")?;
        let after = fs::read(&path)?;
        let modified_after = fs::metadata(&path)?.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if before != after || modified_before != modified_after {
            bail!("curated-gold comparison modified its fixture");
        }
        if receipt.status != CuratedGoldComparisonStatus::ExactAgreement
            || !has_kind(&receipt, CuratedGoldComparisonKind::ExactAgreement)
        {
            bail!("valid curated-gold fixture did not produce exact agreement");
        }
        Ok(())
    }

    #[test]
    fn missing_reviewer_metadata_is_rejected() -> Result<()> {
        let dir = tempdir()?;
        let mut fixture = fixture()?;
        fixture.reviewer_identity.clear();
        let path = write_fixture(dir.path(), &fixture)?;
        let error = load_curated_gold(&CuratedGoldLoadConfig {
            fixture_path: path,
            repository_root: None,
            expected_revision: None,
        });
        if error.is_ok() {
            bail!("missing reviewer metadata was accepted");
        }
        Ok(())
    }

    #[test]
    fn source_and_expectation_hash_drift_is_rejected() -> Result<()> {
        for drift in ["source", "expectation"] {
            let dir = tempdir()?;
            let mut fixture = fixture()?;
            if drift == "source" {
                fixture.source_content_hash = sha256_digest(b"different");
            } else {
                fixture.expectation_hash = sha256_digest(b"different");
            }
            let path = write_fixture(dir.path(), &fixture)?;
            let result = load_curated_gold(&CuratedGoldLoadConfig {
                fixture_path: path,
                repository_root: None,
                expected_revision: None,
            });
            if result.is_ok() {
                bail!("{drift} hash drift was accepted");
            }
        }
        Ok(())
    }

    #[test]
    fn repository_sources_require_declared_revision_and_safe_path() -> Result<()> {
        let dir = tempdir()?;
        let source_path = dir.path().join("package.t");
        let source = b"package main;\n";
        fs::write(&source_path, source)?;
        let expected_facts = vec![fact()];
        let mut fixture = fixture()?;
        fixture.source = CuratedGoldSource::Repository {
            path: "package.t".to_string(),
            revision: "source-rev-1".to_string(),
        };
        fixture.source_content_hash = sha256_digest(source);
        fixture.expectation_hash = curated_gold_expectation_hash(&expected_facts, &[])?;
        let path = write_fixture(dir.path(), &fixture)?;
        let loaded = load_curated_gold(&CuratedGoldLoadConfig {
            fixture_path: path.clone(),
            repository_root: Some(dir.path().to_path_buf()),
            expected_revision: Some("source-rev-1".to_string()),
        })?;
        if loaded.source_text != "package main;\n" {
            bail!("repository-backed source was not loaded");
        }

        fixture.source = CuratedGoldSource::Repository {
            path: "../package.t".to_string(),
            revision: "source-rev-1".to_string(),
        };
        fixture.source_content_hash = sha256_digest(source);
        let unsafe_path = write_fixture(dir.path(), &fixture)?;
        let result = load_curated_gold(&CuratedGoldLoadConfig {
            fixture_path: unsafe_path,
            repository_root: Some(dir.path().to_path_buf()),
            expected_revision: Some("source-rev-1".to_string()),
        });
        if result.is_ok() {
            bail!("repository source path traversal was accepted");
        }
        Ok(())
    }

    #[test]
    fn unsupported_fact_class_fails_closed() -> Result<()> {
        let dir = tempdir()?;
        let mut value = serde_json::to_value(fixture()?)?;
        value["fact_class"] = json!("Unsupported");
        let path = dir.path().join("unsupported.json");
        fs::write(&path, serde_json::to_vec(&value)?)?;
        let result = load_curated_gold(&CuratedGoldLoadConfig {
            fixture_path: path,
            repository_root: None,
            expected_revision: None,
        });
        if result.is_ok() {
            bail!("unsupported fact class was accepted");
        }
        Ok(())
    }

    #[test]
    fn comparison_distinguishes_required_outcomes() -> Result<()> {
        let dir = tempdir()?;
        let (_path, loaded) = load_fixture(dir.path())?;
        let mut changed = fact();
        changed.value = json!({"name": "other"});
        changed.range = Some(CuratedGoldRange { start_byte: 1, end_byte: 12 });
        changed.provenance = "GeneratedNoSource".to_string();
        changed.confidence = CuratedGoldConfidence::Low;
        changed.freshness = CuratedGoldFreshness::Stale;
        let receipt = compare_curated_gold(
            &loaded,
            Some(&PackageAdapter { facts: vec![changed] }),
            "compiler.facts.v1",
        )?;
        for kind in [
            CuratedGoldComparisonKind::ValueMismatch,
            CuratedGoldComparisonKind::RangeMismatch,
            CuratedGoldComparisonKind::ProvenanceMismatch,
            CuratedGoldComparisonKind::ConfidenceOrFreshnessMismatch,
        ] {
            if !has_kind(&receipt, kind) {
                bail!("comparison omitted {kind:?}");
            }
        }
        let extra = CuratedGoldFact { fact_id: "extra".to_string(), ..fact() };
        let receipt = compare_curated_gold(
            &loaded,
            Some(&PackageAdapter { facts: vec![extra] }),
            "compiler.facts.v1",
        )?;
        if !has_kind(&receipt, CuratedGoldComparisonKind::MissingFact)
            || !has_kind(&receipt, CuratedGoldComparisonKind::ExtraFact)
        {
            bail!("comparison did not distinguish missing and extra facts");
        }
        Ok(())
    }

    #[test]
    fn dynamic_unimplemented_and_stale_outcomes_are_distinct() -> Result<()> {
        let dir = tempdir()?;
        let mut fixture = fixture()?;
        fixture.allowed_dynamic_boundaries = vec!["runtime_eval".to_string()];
        fixture.expected_facts[0].dynamic_boundary = Some("runtime_eval".to_string());
        fixture.expectation_hash = curated_gold_expectation_hash(
            &fixture.expected_facts,
            &fixture.allowed_dynamic_boundaries,
        )?;
        let path = write_fixture(dir.path(), &fixture)?;
        let loaded = load_curated_gold(&CuratedGoldLoadConfig {
            fixture_path: path,
            repository_root: None,
            expected_revision: None,
        })?;
        let receipt = compare_curated_gold(
            &loaded,
            Some(&PackageAdapter { facts: vec![fixture.expected_facts[0].clone()] }),
            "compiler.facts.v1",
        )?;
        if !has_kind(&receipt, CuratedGoldComparisonKind::ExpectedDynamicBoundary) {
            bail!("expected dynamic boundary was not classified separately");
        }
        let mut unexpected = fixture.expected_facts[0].clone();
        unexpected.dynamic_boundary = Some("unlisted_boundary".to_string());
        let receipt = compare_curated_gold(
            &loaded,
            Some(&PackageAdapter { facts: vec![unexpected] }),
            "compiler.facts.v1",
        )?;
        if !has_kind(&receipt, CuratedGoldComparisonKind::UnexpectedDynamicBoundary) {
            bail!("unexpected dynamic boundary was not classified separately");
        }
        let receipt = compare_curated_gold(&loaded, None, "compiler.facts.v1")?;
        if receipt.status != CuratedGoldComparisonStatus::UnimplementedFactClass
            || !has_kind(&receipt, CuratedGoldComparisonKind::UnimplementedFactClass)
        {
            bail!("unimplemented fact class was not classified separately");
        }
        let receipt = compare_curated_gold(&loaded, None, "compiler.facts.v0")?;
        if receipt.status != CuratedGoldComparisonStatus::StaleFixture
            || !has_kind(&receipt, CuratedGoldComparisonKind::StaleFixture)
        {
            bail!("stale capability was not classified separately");
        }
        Ok(())
    }

    #[test]
    fn comparison_receipt_is_deterministic() -> Result<()> {
        let dir = tempdir()?;
        let (_path, loaded) = load_fixture(dir.path())?;
        let first = compare_curated_gold(
            &loaded,
            Some(&PackageAdapter { facts: vec![fact()] }),
            "compiler.facts.v1",
        )?;
        let second = compare_curated_gold(
            &loaded,
            Some(&PackageAdapter { facts: vec![fact()] }),
            "compiler.facts.v1",
        )?;
        if first != second {
            bail!("curated-gold comparison receipt was not deterministic");
        }
        Ok(())
    }
}
