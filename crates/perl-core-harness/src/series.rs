//! Immutable comparison-series identity for the upstream Perl core harness.
//!
//! A series manifest pins the denominator of every later comparison: the
//! normalized file membership, the measured subject identities, and the hash
//! that binds them together. The manifest is immutable once written, so
//! replacing one requires an explicit new series id and a change reason.

use crate::normalization::hex_lower;
use crate::{normalize_test_path, project_root, read_discovery_report};
use chrono::Utc;
use color_eyre::eyre::{Context, Result, bail};
use perl_core_harness_types::{
    DISCOVERY_SCHEMA_VERSION, DiscoveredTest, DiscoveryReport, HarnessProfile, HarnessRunner,
    SERIES_MANIFEST_NORMALIZATION_VERSION, SERIES_MANIFEST_SCHEMA_VERSION, SeriesManifest,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Configuration for generating or checking a comparison-series manifest.
#[derive(Debug, Clone)]
pub struct SeriesManifestConfig {
    pub discovery: PathBuf,
    pub output: Option<PathBuf>,
    pub series_id: String,
    pub profile: HarnessProfile,
    pub perl_requested_ref: String,
    pub perl_resolved_ref: String,
    pub preparation_receipt_id: String,
    pub preparation_receipt_digest: String,
    pub compiler_subject_identity: String,
    pub invocation_identity: String,
    pub capability_identity: String,
    pub environment_identity: String,
    pub replaces_series_id: Option<String>,
    pub change_reason: Option<String>,
    pub check: bool,
}

/// Generate or check the immutable identity manifest for a comparison series.
pub fn series_manifest(config: SeriesManifestConfig) -> Result<()> {
    let discovery_path = config.discovery.clone();
    let output_path =
        config.output.clone().unwrap_or_else(|| default_series_manifest_path(config.profile));
    let discovery = read_discovery_report(&discovery_path)?;

    if config.check {
        let existing = read_series_manifest(&output_path)?;
        validate_series_manifest(&existing)?;
        let expected = build_series_manifest(&discovery, &config, existing.created_at.clone())?;
        if existing != expected {
            bail!(
                "comparison-series manifest drift detected for {}: regenerate an explicit new series",
                existing.series_id
            );
        }
        tracing::info!("perl-core-harness: series manifest check passed");
        return Ok(());
    }

    let manifest = build_series_manifest(
        &discovery,
        &config,
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    )?;
    validate_series_manifest(&manifest)?;
    if output_path.is_file() {
        let existing = read_series_manifest(&output_path)?;
        validate_series_manifest(&existing)?;
        if config.replaces_series_id.as_deref() != Some(existing.series_id.as_str())
            || config.change_reason.as_deref().is_none_or(|reason| reason.trim().is_empty())
        {
            bail!(
                "comparison-series manifest is immutable; provide --replaces-series-id matching {} and a non-empty --change-reason to replace it",
                existing.series_id
            );
        }
        if manifest.series_id == existing.series_id {
            bail!(
                "a replacement comparison series must declare a new --series-id, not reuse {}",
                existing.series_id
            );
        }
    }
    write_series_manifest(&output_path, &manifest)?;
    tracing::info!(
        "perl-core-harness: wrote {} comparison series with {} files",
        manifest.series_id,
        manifest.normalized_manifest.len()
    );
    Ok(())
}

pub(crate) fn build_series_manifest(
    discovery: &DiscoveryReport,
    config: &SeriesManifestConfig,
    created_at: String,
) -> Result<SeriesManifest> {
    if discovery.profile != config.profile {
        bail!(
            "discovery profile {} does not match requested series profile {}",
            discovery.profile,
            config.profile
        );
    }
    if config.series_id.trim().is_empty()
        || config.perl_requested_ref.trim().is_empty()
        || config.perl_resolved_ref.trim().is_empty()
        || config.preparation_receipt_id.trim().is_empty()
        || config.preparation_receipt_digest.trim().is_empty()
        || config.compiler_subject_identity.trim().is_empty()
        || config.invocation_identity.trim().is_empty()
        || config.capability_identity.trim().is_empty()
        || config.environment_identity.trim().is_empty()
    {
        bail!("series identity fields must not be empty");
    }
    validate_replacement_metadata(
        &config.series_id,
        config.replaces_series_id.as_deref(),
        config.change_reason.as_deref(),
    )?;
    if discovery.perl_ref != config.perl_resolved_ref {
        bail!(
            "resolved Perl ref {} does not match discovery receipt {}",
            config.perl_resolved_ref,
            discovery.perl_ref
        );
    }

    let normalized_manifest = normalize_discovered_tests(discovery, config.profile)?;
    let profile_roots: Vec<String> =
        config.profile.roots().iter().map(|root| (*root).to_string()).collect();
    let manifest_hash = series_manifest_hash(&SeriesManifestHashInput {
        series_id: &config.series_id,
        repository_commit: &discovery.commit,
        perl_requested_ref: &config.perl_requested_ref,
        perl_resolved_ref: &config.perl_resolved_ref,
        runner: discovery.runner,
        profile: discovery.profile,
        roots: &profile_roots,
        files: &normalized_manifest,
        preparation_receipt_id: &config.preparation_receipt_id,
        preparation_receipt_digest: &config.preparation_receipt_digest,
        harness_schema_version: &discovery.schema_version,
        compiler_subject_identity: &config.compiler_subject_identity,
        invocation_identity: &config.invocation_identity,
        capability_identity: &config.capability_identity,
        environment_identity: &config.environment_identity,
        replaces_series_id: config.replaces_series_id.as_deref(),
        change_reason: config.change_reason.as_deref(),
    });

    Ok(SeriesManifest {
        schema_version: SERIES_MANIFEST_SCHEMA_VERSION.to_string(),
        series_id: config.series_id.clone(),
        profile: discovery.profile,
        profile_roots,
        repository_commit: discovery.commit.clone(),
        perl_requested_ref: config.perl_requested_ref.clone(),
        perl_resolved_ref: config.perl_resolved_ref.clone(),
        runner: discovery.runner,
        normalized_manifest,
        manifest_hash,
        preparation_receipt_id: config.preparation_receipt_id.clone(),
        preparation_receipt_digest: config.preparation_receipt_digest.clone(),
        harness_schema_version: discovery.schema_version.clone(),
        compiler_subject_identity: config.compiler_subject_identity.clone(),
        invocation_identity: config.invocation_identity.clone(),
        capability_identity: config.capability_identity.clone(),
        environment_identity: config.environment_identity.clone(),
        normalization_version: SERIES_MANIFEST_NORMALIZATION_VERSION.to_string(),
        created_at,
        replaces_series_id: config.replaces_series_id.clone(),
        change_reason: config.change_reason.clone(),
    })
}

pub(crate) fn normalize_discovered_tests(
    discovery: &DiscoveryReport,
    profile: HarnessProfile,
) -> Result<Vec<String>> {
    let mut files = Vec::with_capacity(discovery.tests.len());
    let allowed_roots = profile.roots().iter().copied().collect::<BTreeSet<_>>();
    for test in &discovery.tests {
        let normalized = normalize_test_path(&test.path).ok_or_else(|| {
            color_eyre::eyre::eyre!("invalid discovered test path: {}", test.path)
        })?;
        if normalized.contains("..") || normalized.starts_with('/') {
            bail!("discovered test path escapes the profile roots: {normalized}");
        }
        let Some((root, _)) = normalized.split_once('/') else {
            bail!("discovered test must include a profile root: {normalized}");
        };
        if !allowed_roots.contains(root) {
            bail!("discovered test {normalized} is outside core profile roots");
        }
        if test.root != root {
            bail!("discovered test {} has mismatched root {}", test.path, test.root);
        }
        files.push(normalized);
    }
    files.sort();
    for pair in files.windows(2) {
        if pair[0] == pair[1] {
            bail!("duplicate discovered test path: {}", pair[0]);
        }
    }
    if files.is_empty() {
        bail!("comparison series cannot have an empty file list");
    }
    Ok(files)
}

pub(crate) fn validate_series_manifest(manifest: &SeriesManifest) -> Result<()> {
    if manifest.schema_version != SERIES_MANIFEST_SCHEMA_VERSION {
        bail!("unsupported series manifest schema: {}", manifest.schema_version);
    }
    if manifest.series_id.trim().is_empty()
        || manifest.repository_commit.trim().is_empty()
        || manifest.perl_requested_ref.trim().is_empty()
        || manifest.perl_resolved_ref.trim().is_empty()
        || manifest.preparation_receipt_id.trim().is_empty()
        || manifest.preparation_receipt_digest.trim().is_empty()
        || manifest.compiler_subject_identity.trim().is_empty()
        || manifest.invocation_identity.trim().is_empty()
        || manifest.capability_identity.trim().is_empty()
        || manifest.environment_identity.trim().is_empty()
        || manifest.created_at.trim().is_empty()
    {
        bail!("comparison-series identity fields must not be empty");
    }
    validate_replacement_metadata(
        &manifest.series_id,
        manifest.replaces_series_id.as_deref(),
        manifest.change_reason.as_deref(),
    )?;
    let expected_roots =
        manifest.profile.roots().iter().map(|root| (*root).to_string()).collect::<Vec<_>>();
    if manifest.profile_roots != expected_roots {
        bail!("comparison-series roots do not match the declared profile");
    }
    if manifest.normalized_manifest.is_empty() {
        bail!("comparison series cannot have an empty file list");
    }
    if manifest.normalization_version != SERIES_MANIFEST_NORMALIZATION_VERSION {
        bail!("unsupported series manifest normalization: {}", manifest.normalization_version);
    }
    for pair in manifest.normalized_manifest.windows(2) {
        if pair[0] >= pair[1] {
            bail!("series manifest files must be strictly sorted and unique");
        }
    }
    let tests = manifest
        .normalized_manifest
        .iter()
        .map(|path| {
            let root = path.split('/').next().ok_or_else(|| {
                color_eyre::eyre::eyre!("series manifest path has no root: {path}")
            })?;
            Ok(DiscoveredTest { path: path.clone(), root: root.to_string() })
        })
        .collect::<Result<Vec<_>>>()?;
    let discovery = DiscoveryReport {
        schema_version: DISCOVERY_SCHEMA_VERSION.to_string(),
        commit: manifest.repository_commit.clone(),
        timestamp: manifest.created_at.clone(),
        perl_ref: manifest.perl_resolved_ref.clone(),
        prepared_tree: manifest.preparation_receipt_id.clone(),
        host_perl: "manifest".to_string(),
        runner: manifest.runner,
        profile: manifest.profile,
        tests,
    };
    if discovery.schema_version != manifest.harness_schema_version {
        bail!("comparison-series harness schema does not match discovery schema");
    }
    let files = normalize_discovered_tests(&discovery, manifest.profile)?;
    let expected_hash = series_manifest_hash(&SeriesManifestHashInput {
        series_id: &manifest.series_id,
        repository_commit: &manifest.repository_commit,
        perl_requested_ref: &manifest.perl_requested_ref,
        perl_resolved_ref: &manifest.perl_resolved_ref,
        runner: manifest.runner,
        profile: manifest.profile,
        roots: &manifest.profile_roots,
        files: &files,
        preparation_receipt_id: &manifest.preparation_receipt_id,
        preparation_receipt_digest: &manifest.preparation_receipt_digest,
        harness_schema_version: &manifest.harness_schema_version,
        compiler_subject_identity: &manifest.compiler_subject_identity,
        invocation_identity: &manifest.invocation_identity,
        capability_identity: &manifest.capability_identity,
        environment_identity: &manifest.environment_identity,
        replaces_series_id: manifest.replaces_series_id.as_deref(),
        change_reason: manifest.change_reason.as_deref(),
    });
    if manifest.manifest_hash != expected_hash {
        bail!("series manifest hash does not match its identity and file list");
    }
    Ok(())
}

fn validate_replacement_metadata(
    series_id: &str,
    replaces_series_id: Option<&str>,
    change_reason: Option<&str>,
) -> Result<()> {
    if let Some(replaced) = replaces_series_id {
        if replaced.trim().is_empty() || change_reason.is_none_or(|reason| reason.trim().is_empty())
        {
            bail!("replacement comparison series require a non-empty --change-reason");
        }
        if replaced == series_id {
            bail!("a replacement comparison series must declare a new --series-id");
        }
    }
    Ok(())
}

struct SeriesManifestHashInput<'a> {
    series_id: &'a str,
    repository_commit: &'a str,
    perl_requested_ref: &'a str,
    perl_resolved_ref: &'a str,
    runner: HarnessRunner,
    profile: HarnessProfile,
    roots: &'a [String],
    files: &'a [String],
    preparation_receipt_id: &'a str,
    preparation_receipt_digest: &'a str,
    harness_schema_version: &'a str,
    compiler_subject_identity: &'a str,
    invocation_identity: &'a str,
    capability_identity: &'a str,
    environment_identity: &'a str,
    replaces_series_id: Option<&'a str>,
    change_reason: Option<&'a str>,
}

fn series_manifest_hash(input: &SeriesManifestHashInput<'_>) -> String {
    let mut hasher = Sha256::new();
    for value in [
        SERIES_MANIFEST_SCHEMA_VERSION,
        SERIES_MANIFEST_NORMALIZATION_VERSION,
        input.series_id,
        input.repository_commit,
        input.perl_requested_ref,
        input.perl_resolved_ref,
        input.runner.as_str(),
        input.profile.as_str(),
        input.preparation_receipt_id,
        input.preparation_receipt_digest,
        input.harness_schema_version,
        input.compiler_subject_identity,
        input.invocation_identity,
        input.capability_identity,
        input.environment_identity,
    ] {
        hash_manifest_value(&mut hasher, value);
    }
    hash_manifest_value(&mut hasher, "profile-roots");
    hash_manifest_length(&mut hasher, input.roots.len());
    for value in input.roots {
        hash_manifest_value(&mut hasher, value);
    }
    hash_manifest_value(&mut hasher, "normalized-files");
    hash_manifest_length(&mut hasher, input.files.len());
    for value in input.files {
        hash_manifest_value(&mut hasher, value);
    }
    hash_manifest_optional_value(&mut hasher, input.replaces_series_id);
    hash_manifest_optional_value(&mut hasher, input.change_reason);
    hex_lower(&hasher.finalize())
}

fn hash_manifest_value(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

fn hash_manifest_length(hasher: &mut Sha256, length: usize) {
    hasher.update((length as u64).to_le_bytes());
}

fn hash_manifest_optional_value(hasher: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            hash_manifest_value(hasher, "some");
            hash_manifest_value(hasher, value);
        }
        None => hash_manifest_value(hasher, "none"),
    }
}

fn default_series_manifest_path(profile: HarnessProfile) -> PathBuf {
    let root = project_root().unwrap_or_else(|_| PathBuf::from("."));
    root.join(".ci").join("perl-core-harness").join(format!("{profile}-series-manifest.json"))
}

pub(crate) fn write_series_manifest(path: &Path, manifest: &SeriesManifest) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating series manifest directory {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(manifest).context("serializing series manifest")?;
    fs::write(path, format!("{json}\n"))
        .with_context(|| format!("writing series manifest {}", path.display()))
}

pub(crate) fn read_series_manifest(path: &Path) -> Result<SeriesManifest> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading series manifest {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("decoding series manifest {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed discovery receipt used by the byte-parity fixtures below.
    fn fixture_discovery() -> DiscoveryReport {
        DiscoveryReport {
            schema_version: DISCOVERY_SCHEMA_VERSION.to_string(),
            commit: "1111111111111111111111111111111111111111".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            perl_ref: "2222222222222222222222222222222222222222".to_string(),
            prepared_tree: "prepare-fixture".to_string(),
            host_perl: "/usr/bin/perl".to_string(),
            runner: HarnessRunner::Test,
            profile: HarnessProfile::Base,
            tests: vec![
                DiscoveredTest { path: "base/if.t".to_string(), root: "base".to_string() },
                DiscoveredTest { path: "base/cond.t".to_string(), root: "base".to_string() },
            ],
        }
    }

    /// Fixed series configuration used by the byte-parity fixtures below.
    fn fixture_config() -> SeriesManifestConfig {
        SeriesManifestConfig {
            discovery: PathBuf::from("discovery.json"),
            output: None,
            series_id: "series-fixture-1".to_string(),
            profile: HarnessProfile::Base,
            perl_requested_ref: "v5.40.0".to_string(),
            perl_resolved_ref: "2222222222222222222222222222222222222222".to_string(),
            preparation_receipt_id: "prepare-fixture".to_string(),
            preparation_receipt_digest: "sha256:3333".to_string(),
            compiler_subject_identity: "compiler-fixture".to_string(),
            invocation_identity: "invocation-fixture".to_string(),
            capability_identity: "capability-fixture".to_string(),
            environment_identity: "environment-fixture".to_string(),
            replaces_series_id: None,
            change_reason: None,
            check: false,
        }
    }

    #[test]
    fn series_manifest_hash_is_pinned_for_a_fixed_fixture() -> Result<()> {
        let manifest = build_series_manifest(
            &fixture_discovery(),
            &fixture_config(),
            "2026-01-01T00:00:00Z".to_string(),
        )?;

        assert_eq!(
            manifest.manifest_hash,
            "d29b42a1b65010359920c1d000cd27933681fbd9f3859295bfa46afbb0faeb9e"
        );
        Ok(())
    }

    #[test]
    fn series_manifest_serializes_to_pinned_bytes() -> Result<()> {
        let manifest = build_series_manifest(
            &fixture_discovery(),
            &fixture_config(),
            "2026-01-01T00:00:00Z".to_string(),
        )?;

        let json = serde_json::to_string_pretty(&manifest)?;
        assert_eq!(json, PINNED_MANIFEST_JSON);
        Ok(())
    }

    #[test]
    fn series_manifest_rejects_duplicate_membership() {
        let mut discovery = fixture_discovery();
        discovery
            .tests
            .push(DiscoveredTest { path: "base/if.t".to_string(), root: "base".to_string() });

        let error = build_series_manifest(
            &discovery,
            &fixture_config(),
            "2026-01-01T00:00:00Z".to_string(),
        )
        .expect_err("duplicate selected files must fail closed");
        assert!(error.to_string().contains("duplicate discovered test path"));
    }

    #[test]
    fn series_manifest_rejects_wrong_profile_root() {
        let mut discovery = fixture_discovery();
        discovery.tests[0].root = "comp".to_string();

        let error = build_series_manifest(
            &discovery,
            &fixture_config(),
            "2026-01-01T00:00:00Z".to_string(),
        )
        .expect_err("mismatched roots must fail closed");
        assert!(error.to_string().contains("mismatched root"));
    }

    #[test]
    fn series_manifest_rejects_paths_that_escape_profile_roots() {
        let mut discovery = fixture_discovery();
        discovery.tests[0].path = "base/../outside.t".to_string();

        let error = build_series_manifest(
            &discovery,
            &fixture_config(),
            "2026-01-01T00:00:00Z".to_string(),
        )
        .expect_err("path escapes must fail closed");
        assert!(error.to_string().contains("escapes the profile roots"));
    }

    const PINNED_MANIFEST_JSON: &str = r#"{
  "schema_version": "perl_core_harness.comparison_series.v1",
  "series_id": "series-fixture-1",
  "profile": "base",
  "profile_roots": [
    "base"
  ],
  "repository_commit": "1111111111111111111111111111111111111111",
  "perl_requested_ref": "v5.40.0",
  "perl_resolved_ref": "2222222222222222222222222222222222222222",
  "runner": "test",
  "normalized_manifest": [
    "base/cond.t",
    "base/if.t"
  ],
  "manifest_hash": "d29b42a1b65010359920c1d000cd27933681fbd9f3859295bfa46afbb0faeb9e",
  "preparation_receipt_id": "prepare-fixture",
  "preparation_receipt_digest": "sha256:3333",
  "harness_schema_version": "perl_core_harness.discovery.v1",
  "compiler_subject_identity": "compiler-fixture",
  "invocation_identity": "invocation-fixture",
  "capability_identity": "capability-fixture",
  "environment_identity": "environment-fixture",
  "normalization_version": "path-normalization.v1",
  "created_at": "2026-01-01T00:00:00Z",
  "replaces_series_id": null,
  "change_reason": null
}"#;
}
