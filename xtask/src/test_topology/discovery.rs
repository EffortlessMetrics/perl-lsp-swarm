//! Live and fixture-driven discovery of compiler-critical execution subjects.
//!
//! Scaffolding commit (#12125): this file fixes the discovery CONTRACT —
//! cohort selection, manifest cross-check facts, canonical target identity,
//! and the deterministic judgment classifiers. The metadata walk itself
//! lands with the implementation commit; [`discover_from_metadata`] and
//! [`discover_live`] fail loudly until then.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail};
use serde::Deserialize;

use super::model::{
    CandidateProfileV1, CanonicalSourceIdentityV1, CompileObligationV1, DefaultProfileStateV1,
    ExecutionClaimV1, FEATURE_AUTHORITIES, FeatureSubjectV1, PARENT_CONTROLLER, ProofRoleV1,
    TargetKindV1, TestTopologyRowV1, RETIREMENT_CONDITION, REVIEW_CONDITION,
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
    /// [`Self::extra_targets`]; this list is never presented as complete.
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
    pub fn extra_targets(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::CompilerCritical => {
                &[("xtask", "gate_policy_profile_tests"), ("xtask", "perl_core_harness_workflow_policy")]
            }
        }
    }
}

/// Manifest-derived facts for one package, cross-checked against metadata.
#[derive(Debug, Default)]
pub struct ManifestFacts {
    /// `[lib] harness` override when declared.
    pub lib_harness: Option<bool>,
    /// Explicit section entries keyed by section (`test`, `bench`, `bin`,
    /// `example`).
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
    /// Pure over the observed facts: proof roles, controllers, profiles, and
    /// compile obligations derive mechanically so the checker can recompute
    /// them and reject drift.
    pub fn topology_row(&self) -> anyhow::Result<TestTopologyRowV1> {
        let default_profile_state = if self.required_features.is_empty() {
            DefaultProfileStateV1::IncludedByDefault
        } else {
            DefaultProfileStateV1::FeatureGated
        };
        let authority_refs: Vec<String> = if default_profile_state
            == DefaultProfileStateV1::FeatureGated
        {
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
                controller_refs.push("#12075".to_string());
            }
            ProofRoleV1::CompilerSemantics => controller_refs.push("#12078".to_string()),
            ProofRoleV1::Currentness => controller_refs.push("#12079".to_string()),
            ProofRoleV1::Infrastructure => {}
        }
        let candidate_profiles =
            classify_candidate_profiles(&self.cargo_target_name);
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

/// Discovers cohort subjects from a parsed-at-call-time metadata document.
///
/// Scaffolding stub: the implementation commit replaces this body.
pub fn discover_from_metadata(
    _metadata_json: &str,
    _manifests: &BTreeMap<String, ManifestFacts>,
) -> anyhow::Result<Vec<DiscoveredTarget>> {
    bail!(
        "compiler-critical topology discovery is intentionally unimplemented in the \
         scaffolding commit; the metadata walk lands with the #12125 implementation"
    );
}

/// Runs `cargo metadata` in `root` and discovers cohort subjects.
///
/// Scaffolding stub: the implementation commit replaces this body.
pub fn discover_live(_root: &std::path::Path) -> anyhow::Result<Vec<DiscoveredTarget>> {
    bail!(
        "compiler-critical topology discovery is intentionally unimplemented in the \
         scaffolding commit; live cargo-metadata discovery lands with #12125"
    );
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
        "hover", "completion", "definition", "references", "document_symbol",
        "workspace_symbol", "folding", "signature", "inlay", "semantic_token", "code_lens",
        "color", "document_link", "highlight", "moniker", "selection_range", "on_type",
        "navigation", "call_hierarchy", "codelens",
    ];
    if PROVIDER_READ.iter().any(|token| name.contains(token)) {
        return ProofRoleV1::ProviderRead;
    }
    const COMPILER_SEMANTICS: [&str; 9] = [
        "parse", "parser", "semantic_", "_semantic", "pir", "analyzer", "lexer", "ast_",
        "_ast",
    ];
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
    const PRESSURE: [&str; 7] =
        ["stress", "memory_pressure", "latency", "torture", "performance", "benchmark", "real_project"];
    PRESSURE.iter().any(|token| name.contains(token))
}

/// Deterministic candidate-profile classifier (visibility only, never
/// routing truth). Public so the checker recomputes the same judgment the
/// generator applied and rejects committed drift.
pub fn classify_candidate_profiles(target_name: &str) -> BTreeSet<CandidateProfileV1> {
    if is_pressure_subject(target_name) {
        BTreeSet::from([
            CandidateProfileV1::ScheduledPressure,
            CandidateProfileV1::ManualResearch,
        ])
    } else {
        BTreeSet::from([CandidateProfileV1::PrFocused])
    }
}
