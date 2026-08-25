//! Live and fixture-driven discovery of compiler-critical execution subjects.
//!
//! Discovery shells out to `cargo metadata --format-version 1`, parses the
//! document into local resilience-first structs (`#[serde(default)]`), and
//! cross-checks package manifests through the `toml` crate for the facts
//! metadata does not carry (notably harness settings). Nothing here infers
//! execution from existence: a discovered target only ever yields an
//! obligation-bearing row, never a pass/fail verdict.
//!
//! Determinism contract: rows are canonically sorted by
//! [`DiscoveredTarget::target_id`], feature lists are sorted, every stored
//! path is workspace-relative with forward slashes, and no field depends on
//! `CARGO_TARGET_DIR`, metadata array order, or filesystem iteration order.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, bail};
use serde::Deserialize;

use super::model::{
    CandidateProfileV1, CanonicalSourceIdentityV1, CompileObligationV1, DefaultProfileStateV1,
    ExecutionClaimV1, FEATURE_AUTHORITIES, FeatureSubjectV1, PARENT_CONTROLLER, ProofRoleV1,
    RETIREMENT_CONDITION, REVIEW_CONDITION, TargetKindV1, TestTopologyRowV1,
};

/// Cohort selector understood by the CLI surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cohort {
    /// The compiler-convergence critical path fixed by issue #12125.
    CompilerCritical,
}

impl Cohort {
    /// Canonical cohort name used in artifacts and commands.
    pub fn as_slug(self) -> &'static str {
        match self {
            Self::CompilerCritical => "compiler-critical",
        }
    }

    /// Seed package list. Evidence-based extensions live in
    /// [`Self::extra_targets`]; this list is never presented as complete —
    /// the issue directs discovery to follow Cargo metadata and current
    /// repository ownership rather than an assumed full enumeration.
    pub fn packages(self) -> &'static [&'static str] {
        match self {
            Self::CompilerCritical => &[
                "perl-core-harness",
                "perl-core-harness-types",
                "perl-core-test-runner",
                "perl-parser-core",
                "perl-semantic-analyzer",
                "perl-workspace",
                "perl-lsp-rs-core",
                "perl-lsp-rs",
            ],
        }
    }

    /// Named targets outside the seed packages that route or police the
    /// cohort (xtask gate-policy/workflow-policy proof subjects).
    /// `(package, target name)` pairs; extend only with evidence.
    pub fn extra_targets(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::CompilerCritical => &[
                ("xtask", "gate_policy_profile_tests"),
                ("xtask", "perl_core_harness_workflow_policy"),
            ],
        }
    }
}

/// Raw `cargo metadata` document (only the fields discovery consumes).
#[derive(Debug, Deserialize)]
struct MetadataDocument {
    #[serde(default)]
    packages: Vec<MetadataPackage>,
    #[serde(default)]
    workspace_root: String,
}

/// Raw workspace member entry.
#[derive(Debug, Deserialize)]
struct MetadataPackage {
    name: String,
    #[serde(default)]
    manifest_path: String,
    #[serde(default)]
    targets: Vec<MetadataTarget>,
}

/// Raw Cargo target entry. Unknown fields are ignored on purpose: the
/// metadata format gains keys over time and resilience is required here
/// (unlike the committed inventory schema, which is closed).
#[derive(Debug, Deserialize)]
struct MetadataTarget {
    name: String,
    #[serde(default)]
    kind: Vec<String>,
    #[serde(default)]
    src_path: String,
    #[serde(rename = "required-features", default)]
    required_features: Vec<String>,
    doctest: Option<bool>,
}

/// Manifest-derived facts for one package, cross-checked against metadata.
#[derive(Debug, Default)]
pub struct ManifestFacts {
    /// `[lib] harness` override when declared.
    pub lib_harness: Option<bool>,
    /// Explicit section entries keyed by section (`test`, `bench`, `bin`,
    /// `example`); each maps target name to declared facts.
    pub sections: BTreeMap<String, BTreeMap<String, SectionEntry>>,
    /// `[package] autotests`; `None` when unset (Cargo default: enabled).
    pub autotests: Option<bool>,
    /// `[package] autobenches`; `None` when unset (Cargo default: enabled).
    pub autobenches: Option<bool>,
}

/// One explicit manifest target declaration.
#[derive(Debug, Default)]
pub struct SectionEntry {
    /// `harness = ...` when declared.
    pub harness: Option<bool>,
    /// `required-features = [...]` when declared.
    pub required_features: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawManifest {
    package: RawPackageSection,
    lib: RawLibSection,
    test: Vec<RawSectionTarget>,
    bench: Vec<RawSectionTarget>,
    bin: Vec<RawSectionTarget>,
    example: Vec<RawSectionTarget>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawPackageSection {
    autotests: Option<bool>,
    autobenches: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawLibSection {
    harness: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawSectionTarget {
    name: String,
    harness: Option<bool>,
    #[serde(rename = "required-features")]
    required_features: Option<Vec<String>>,
}

impl RawManifest {
    fn facts(&self) -> ManifestFacts {
        let mut sections: BTreeMap<String, BTreeMap<String, SectionEntry>> = BTreeMap::new();
        for (section, targets) in [
            ("test", &self.test),
            ("bench", &self.bench),
            ("bin", &self.bin),
            ("example", &self.example),
        ] {
            for target in targets {
                if target.name.is_empty() {
                    continue;
                }
                sections.entry(section.to_string()).or_default().insert(
                    target.name.clone(),
                    SectionEntry {
                        harness: target.harness,
                        required_features: target.required_features.clone().unwrap_or_default(),
                    },
                );
            }
        }
        ManifestFacts {
            lib_harness: self.lib.harness,
            sections,
            autotests: self.package.autotests,
            autobenches: self.package.autobenches,
        }
    }
}

/// Parses manifest text into cross-check facts.
pub fn parse_manifest_facts(manifest_text: &str) -> anyhow::Result<ManifestFacts> {
    let raw: RawManifest = toml::from_str(manifest_text)
        .map_err(|error| anyhow::Error::new(error).context("parsing package manifest"))?;
    Ok(raw.facts())
}

/// A discovered execution subject with canonical, root-independent facts.
///
/// This is the raw observation layer; judgment surfaces (proof role,
/// controllers, profiles) are applied by [`DiscoveredTarget::topology_row`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredTarget {
    /// Workspace package name.
    pub package_name: String,
    /// Cargo target name.
    pub cargo_target_name: String,
    /// Stable identity `<package>/<target>/<kind-token>`.
    pub target_id: String,
    /// Target source path, workspace-relative, forward slashes.
    pub path: String,
    /// Package manifest path, workspace-relative, forward slashes.
    pub manifest_path: String,
    /// Closed target kind derived from metadata kind tokens.
    pub kind: TargetKindV1,
    /// Harness setting resolved from manifests (metadata omits it).
    pub harness: bool,
    /// Doctest marker for libraries, sourced from metadata.
    pub doctest: Option<bool>,
    /// Union of metadata and manifest required features. Sorted, deduped.
    pub required_features: Vec<String>,
}

impl DiscoveredTarget {
    /// Applies the deterministic v1 judgment layer and produces a row.
    ///
    /// Proof-role classification is a pure function of package and target
    /// name so the checker can recompute and reject drift (falsifier 7);
    /// profiles are visibility-only labels and never routing claims.
    pub fn topology_row(&self) -> anyhow::Result<TestTopologyRowV1> {
        let default_profile_state = if self.required_features.is_empty() {
            DefaultProfileStateV1::IncludedByDefault
        } else {
            DefaultProfileStateV1::FeatureGated
        };
        let authority_refs: Vec<String> =
            if default_profile_state == DefaultProfileStateV1::FeatureGated {
                vec![FEATURE_AUTHORITIES[0].to_string()]
            } else {
                Vec::new()
            };
        let feature_subject = FeatureSubjectV1::new(
            self.required_features.clone(),
            default_profile_state,
            Vec::new(),
            authority_refs,
        )?;
        let compile_obligation = match default_profile_state {
            DefaultProfileStateV1::IncludedByDefault => {
                CompileObligationV1::IncludedInCheckAllTargets
            }
            DefaultProfileStateV1::FeatureGated => {
                CompileObligationV1::ExplicitFeatureBuildRequired
            }
        };
        let proof_role = classify_proof_role(&self.package_name, &self.cargo_target_name);
        let mut controller_refs = vec![PARENT_CONTROLLER.to_string()];
        match proof_role {
            ProofRoleV1::Compatibility | ProofRoleV1::ProviderRead | ProofRoleV1::RefactorEdit => {
                controller_refs.push("#12075".to_string())
            }
            ProofRoleV1::CompilerSemantics => controller_refs.push("#12078".to_string()),
            ProofRoleV1::Currentness => controller_refs.push("#12079".to_string()),
            ProofRoleV1::Infrastructure => {}
        }
        let mut candidate_profiles = BTreeSet::new();
        if is_pressure_subject(&self.cargo_target_name) {
            candidate_profiles.insert(CandidateProfileV1::ScheduledPressure);
            candidate_profiles.insert(CandidateProfileV1::ManualResearch);
        } else {
            candidate_profiles.insert(CandidateProfileV1::PrFocused);
        }
        let mut row = TestTopologyRowV1 {
            target_id: self.target_id.clone(),
            package_id: self.package_name.clone(),
            cargo_target_name: self.cargo_target_name.clone(),
            path: self.path.clone(),
            target_kind: self.kind,
            harness: self.harness,
            doctest: self.doctest,
            feature_subject,
            proof_role,
            controller_refs,
            candidate_profiles,
            minimum_nonzero_work: 1,
            canonical_source_identity: CanonicalSourceIdentityV1 {
                manifest_path: self.manifest_path.clone(),
                source_path: self.path.clone(),
            },
            compile_obligation,
            execution_claim: ExecutionClaimV1::default(),
            review_condition: REVIEW_CONDITION.to_string(),
            retirement_condition: RETIREMENT_CONDITION.to_string(),
            subject_fingerprint: String::new(),
        };
        row.subject_fingerprint = row.compute_fingerprint();
        row.validate()?;
        Ok(row)
    }
}

/// Deterministic proof-role classifier (v1 heuristic denominator owner).
///
/// Ordered rules, first match wins; refinement happens through later
/// #8437 leaves, never by editing committed rows out from under the checker.
pub fn classify_proof_role(package: &str, target_name: &str) -> ProofRoleV1 {
    let name = target_name.to_ascii_lowercase();
    if name.contains("compat") {
        return ProofRoleV1::Compatibility;
    }
    const CURRENTNESS: [&str; 6] =
        ["fresh", "stale", "staleness", "generation_counter", "currentness", "pending_parse"];
    if CURRENTNESS.iter().any(|token| name.contains(token)) {
        return ProofRoleV1::Currentness;
    }
    const REFACTOR_EDIT: [&str; 5] = ["rename", "code_action", "formatting", "_edit", "refactor"];
    if REFACTOR_EDIT.iter().any(|token| name.contains(token)) {
        return ProofRoleV1::RefactorEdit;
    }
    const PROVIDER_READ: [&str; 20] = [
        "hover",
        "completion",
        "definition",
        "references",
        "document_symbol",
        "workspace_symbol",
        "folding",
        "signature",
        "inlay",
        "semantic_token",
        "code_lens",
        "color",
        "document_link",
        "highlight",
        "moniker",
        "selection_range",
        "on_type",
        "navigation",
        "call_hierarchy",
        "codelens",
    ];
    if PROVIDER_READ.iter().any(|token| name.contains(token)) {
        return ProofRoleV1::ProviderRead;
    }
    const COMPILER_SEMANTICS: [&str; 9] =
        ["parse", "parser", "semantic_", "_semantic", "pir", "analyzer", "lexer", "ast_", "_ast"];
    if COMPILER_SEMANTICS.iter().any(|token| name.contains(token)) {
        return ProofRoleV1::CompilerSemantics;
    }
    match package {
        "perl-parser-core" | "perl-semantic-analyzer" | "perl-workspace" | "perl-lsp-rs-core" => {
            ProofRoleV1::CompilerSemantics
        }
        "perl-lsp-rs" => ProofRoleV1::ProviderRead,
        _ => ProofRoleV1::Infrastructure,
    }
}

/// Visibility-only profile rule: heavy/latency/stress subjects are marked for
/// scheduled pressure and manual research instead of PR-focused ride-along.
fn is_pressure_subject(target_name: &str) -> bool {
    let name = target_name.to_ascii_lowercase();
    const PRESSURE: [&str; 7] = [
        "stress",
        "memory_pressure",
        "latency",
        "torture",
        "performance",
        "benchmark",
        "real_project",
    ];
    PRESSURE.iter().any(|token| name.contains(token))
}

/// Deterministic candidate-profile classifier (visibility only, never
/// routing truth). Public so the checker recomputes the same judgment the
/// generator applied and rejects committed drift.
pub fn classify_candidate_profiles(target_name: &str) -> BTreeSet<CandidateProfileV1> {
    if is_pressure_subject(target_name) {
        BTreeSet::from([CandidateProfileV1::ScheduledPressure, CandidateProfileV1::ManualResearch])
    } else {
        BTreeSet::from([CandidateProfileV1::PrFocused])
    }
}

/// Resolves harness for one discovered target from manifest facts.
fn resolve_harness(kind: TargetKindV1, target_name: &str, manifest: &ManifestFacts) -> bool {
    match kind {
        TargetKindV1::Library => manifest.lib_harness.unwrap_or(true),
        TargetKindV1::IntegrationTest => manifest
            .sections
            .get("test")
            .and_then(|entries| entries.get(target_name))
            .and_then(|entry| entry.harness)
            .unwrap_or(true),
        TargetKindV1::Bench => manifest
            .sections
            .get("bench")
            .and_then(|entries| entries.get(target_name))
            .and_then(|entry| entry.harness)
            .unwrap_or(true),
        TargetKindV1::Binary => manifest
            .sections
            .get("bin")
            .and_then(|entries| entries.get(target_name))
            .and_then(|entry| entry.harness)
            .unwrap_or(true),
        TargetKindV1::Example => manifest
            .sections
            .get("example")
            .and_then(|entries| entries.get(target_name))
            .and_then(|entry| entry.harness)
            .unwrap_or(true),
        // Inline module subjects have no independent manifest declaration.
        TargetKindV1::UnitTestModule => true,
    }
}

/// Converts an absolute-ish path into a workspace-relative forward-slash string.
fn relativize(workspace_root: &str, raw: &str) -> anyhow::Result<String> {
    let root = Path::new(workspace_root);
    let candidate = Path::new(raw);
    let relative = candidate.strip_prefix(root).with_context(|| {
        format!(
            "path {raw} is not under workspace root {workspace_root}; stored paths must be \
             workspace-relative so identity survives changed roots"
        )
    })?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

/// Classifies one metadata kind-token list into the closed enum.
///
/// Build scripts (`custom-build`) yield `Ok(None)` and are skipped: they host
/// no governed test subject. Any unknown token combination is an error —
/// kinds are never coerced.
fn classify_kind(tokens: &[String]) -> anyhow::Result<Option<TargetKindV1>> {
    if tokens.is_empty() {
        bail!("metadata target carries no kind tokens; refusing to guess");
    }
    if tokens.iter().any(|token| token == "custom-build") {
        return Ok(None);
    }
    for (candidate, token) in [
        (TargetKindV1::IntegrationTest, "test"),
        (TargetKindV1::Library, "lib"),
        (TargetKindV1::Binary, "bin"),
        (TargetKindV1::Bench, "bench"),
        (TargetKindV1::Example, "example"),
    ] {
        if tokens.iter().any(|present| present == token) {
            return Ok(Some(candidate));
        }
    }
    bail!(
        "unknown metadata target kind {tokens:?}; kinds are closed and never coerced \
         (falsifier 10)"
    );
}

/// Discovers cohort subjects from a parsed-at-call-time metadata document.
///
/// `manifests` maps each package's `manifest_path` (exactly as it appears in
/// the metadata document) to its manifest text or pre-parsed facts provider
/// input. Tests pass synthetic documents and manifests; production passes
/// real ones via [`discover_live`].
pub fn discover_from_metadata(
    metadata_json: &str,
    manifests: &BTreeMap<String, ManifestFacts>,
) -> anyhow::Result<Vec<DiscoveredTarget>> {
    let document: MetadataDocument = serde_json::from_str(metadata_json)
        .map_err(|error| anyhow::Error::new(error).context("parsing cargo metadata JSON"))?;
    if document.workspace_root.is_empty() {
        bail!("cargo metadata document lacks workspace_root");
    }
    let cohort = Cohort::CompilerCritical;
    let mut cohort_packages: BTreeSet<&str> = cohort.packages().iter().copied().collect();
    let mut selected_xtask_targets: BTreeSet<&str> = BTreeSet::new();
    for (package, target) in cohort.extra_targets() {
        if *package == "xtask" {
            cohort_packages.insert("xtask");
            selected_xtask_targets.insert(target);
        }
    }
    let mut discovered = Vec::new();
    for package in &document.packages {
        let in_seed = cohort_packages.contains(package.name.as_str());
        let is_xtask_selection = package.name == "xtask";
        if !in_seed && !is_xtask_selection {
            continue;
        }
        let manifest = manifests.get(package.manifest_path.as_str()).ok_or_else(|| {
            anyhow::anyhow!(
                "manifest facts missing for {} at {}; discovery requires the manifest \
                     cross-check surface even when metadata already answers",
                package.name,
                package.manifest_path
            )
        })?;
        let mut metadata_test_names: BTreeSet<&str> = BTreeSet::new();
        let mut metadata_bench_names: BTreeSet<&str> = BTreeSet::new();
        for target in &package.targets {
            let Some(kind) = classify_kind(&target.kind).with_context(|| {
                format!("classifying target {} of {}", target.name, package.name)
            })?
            else {
                continue;
            };
            if let TargetKindV1::IntegrationTest = kind {
                metadata_test_names.insert(target.name.as_str());
            }
            if let TargetKindV1::Bench = kind {
                metadata_bench_names.insert(target.name.as_str());
            }
            if is_xtask_selection
                && (!selected_xtask_targets.contains(target.name.as_str())
                    || kind != TargetKindV1::IntegrationTest)
            {
                continue;
            }
            let mut required: Vec<String> = target.required_features.clone();
            let section_key = match kind {
                TargetKindV1::IntegrationTest => "test",
                TargetKindV1::Bench => "bench",
                TargetKindV1::Binary => "bin",
                TargetKindV1::Example => "example",
                TargetKindV1::Library | TargetKindV1::UnitTestModule => "",
            };
            if let Some(entry) = (!section_key.is_empty())
                .then(|| manifest.sections.get(section_key))
                .and_then(|entries| entries.and_then(|entries| entries.get(target.name.as_str())))
            {
                required.extend(entry.required_features.iter().cloned());
            }
            required.sort();
            required.dedup();
            let path =
                relativize(&document.workspace_root, &target.src_path).with_context(|| {
                    format!("relativizing target {} of {}", target.name, package.name)
                })?;
            let manifest_rel = relativize(&document.workspace_root, &package.manifest_path)
                .with_context(|| format!("relativizing manifest of {}", package.name))?;
            discovered.push(DiscoveredTarget {
                target_id: format!("{}/{}/{}", package.name, target.name, kind.as_token()),
                package_name: package.name.clone(),
                cargo_target_name: target.name.clone(),
                path,
                manifest_path: manifest_rel,
                kind,
                harness: resolve_harness(kind, &target.name, manifest),
                doctest: match kind {
                    TargetKindV1::Library => Some(target.doctest.unwrap_or(true)),
                    _ => None,
                },
                required_features: required,
            });
        }
        if in_seed {
            cross_check_explicit_sections(
                package.name.as_str(),
                manifest,
                "test",
                &metadata_test_names,
                manifest.autotests,
            )?;
            cross_check_explicit_sections(
                package.name.as_str(),
                manifest,
                "bench",
                &metadata_bench_names,
                manifest.autobenches,
            )?;
        }
    }
    discovered.sort_by(|left, right| left.target_id.cmp(&right.target_id));
    Ok(discovered)
}

/// Fails discovery when explicit manifest declarations disagree with what
/// Cargo metadata reports (stale [[test]]/[[bench]] rows or auto-globs
/// disabled while targets still appear).
fn cross_check_explicit_sections(
    package: &str,
    manifest: &ManifestFacts,
    section: &str,
    metadata_names: &BTreeSet<&str>,
    auto_flag: Option<bool>,
) -> anyhow::Result<()> {
    let Some(entries) = manifest.sections.get(section) else {
        return Ok(());
    };
    let auto_enabled = auto_flag.unwrap_or(true);
    for name in entries.keys() {
        if !auto_enabled && !metadata_names.contains(name.as_str()) {
            bail!(
                "{package} declares [{section}] {name} while {section} auto-discovery is \
                 disabled and cargo metadata does not report it; stale manifest row"
            );
        }
    }
    if !auto_enabled {
        for name in metadata_names {
            if !entries.contains_key(*name) {
                bail!(
                    "{package} disables auto-{section}s but cargo metadata reports {name} \
                     without an explicit declaration; refusing to guess its harness/facts"
                );
            }
        }
    }
    Ok(())
}

/// Runs `cargo metadata --format-version 1 --no-deps` in `root` and reads the
/// selected package manifests from disk.
pub fn discover_live(root: &Path) -> anyhow::Result<Vec<DiscoveredTarget>> {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .context("spawning cargo metadata")?;
    if !output.status.success() {
        bail!("cargo metadata failed:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    let metadata_json =
        String::from_utf8(output.stdout).context("cargo metadata produced non-UTF-8 output")?;
    let document: MetadataDocument = serde_json::from_str(&metadata_json)
        .map_err(|error| anyhow::Error::new(error).context("parsing live cargo metadata"))?;
    let mut manifests = BTreeMap::new();
    for package in &document.packages {
        if !Cohort::CompilerCritical.packages().contains(&package.name.as_str())
            && package.name != "xtask"
        {
            continue;
        }
        let text = std::fs::read_to_string(&package.manifest_path)
            .with_context(|| format!("reading manifest {}", package.manifest_path))?;
        manifests.insert(package.manifest_path.clone(), parse_manifest_facts(&text)?);
    }
    discover_from_metadata(&metadata_json, &manifests)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_metadata_kinds_are_refused_not_coerced() -> anyhow::Result<()> {
        let error = classify_kind(&["mystery-kind".to_string()])
            .err()
            .ok_or_else(|| anyhow::anyhow!("unknown metadata kind must be refused"))?;
        assert!(format!("{error:#}").contains("never coerced"), "unexpected error: {error:#}");
        Ok(())
    }

    #[test]
    fn build_scripts_are_skipped_silently() -> anyhow::Result<()> {
        assert_eq!(classify_kind(&["custom-build".to_string()])?, None);
        Ok(())
    }
}
