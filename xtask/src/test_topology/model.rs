//! Versioned topology schema for compiler-critical Cargo execution subjects.
//!
//! The schema ([`SCHEMA_ID`] `test_topology_inventory.v1`) is deliberately
//! closed: unknown target kinds, proof roles, candidate profiles, authority
//! references, and struct fields are rejected during deserialization instead
//! of being coerced into something plausible. Identity is stable across
//! filesystem roots, `CARGO_TARGET_DIR`, and Cargo metadata ordering; every
//! stored path is workspace-relative with forward slashes.
//!
//! Authority boundary (issue #12125, parent #8437): rows own target identity,
//! package/path/kind facts, required feature/build subjects, proof roles,
//! candidate visibility profiles, work-count requirements, and owner
//! references. They do NOT own workflow routing, execution results, current
//! pass/fail verdicts, gate activation, or the feature taxonomy — rows
//! *reference* the #3790/#8121 feature authorities rather than redefining
//! any supported-feature combination.

use std::collections::BTreeSet;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Stable identifier of the only schema version this module understands.
pub const SCHEMA_ID: &str = "test_topology_inventory.v1";

/// Numeric schema version recorded alongside [`SCHEMA_ID`].
pub const SCHEMA_VERSION: u32 = 1;

/// Closed set of feature identities rows may reference.
///
/// `#3790` owns supported Cargo build-feature combinations;
/// `#8121` classifies governed Cargo features by role. Rows must reference
/// these authorities; embedding an inline feature matrix is a schema
/// violation (`deny_unknown_fields` keeps definition-shaped payloads out).
pub const FEATURE_AUTHORITIES: [&str; 2] = ["#3790", "#8121"];

/// Parent controller every row names.
pub const PARENT_CONTROLLER: &str = "#8437";

/// Boilerplate review condition stamped on generated rows.
pub const REVIEW_CONDITION: &str =
    "review whenever regeneration reports fact drift or the target identity changes";

/// Boilerplate retirement condition stamped on generated rows.
pub const RETIREMENT_CONDITION: &str =
    "retire the row by regenerating the inventory after the target disappears from Cargo metadata";

/// Cargo target kind of a topology row.
///
/// The enum is closed: an unknown kind in serialized input is a hard
/// deserialization error, never a coercion into a default.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKindV1 {
    /// Integration test target declared under `tests/` (or explicit `[[test]]`).
    IntegrationTest,
    /// Inline `#[cfg(test)]` module subject attached to a library or binary.
    /// Never emitted by discovery; present so kind confusion is representable
    /// and therefore rejectable.
    UnitTestModule,
    /// Library target hosting unit-test modules and (optionally) doctests.
    Library,
    /// Binary target.
    Binary,
    /// Benchmark target (`[[bench]]`).
    Bench,
    /// Example target (`[[example]]`).
    Example,
}

impl TargetKindV1 {
    /// Canonical lowercase token used inside [`crate::test_topology::model`]
    /// identifiers and projections.
    pub fn as_token(self) -> &'static str {
        match self {
            Self::IntegrationTest => "integration-test",
            Self::UnitTestModule => "unit-test-module",
            Self::Library => "library",
            Self::Binary => "binary",
            Self::Bench => "bench",
            Self::Example => "example",
        }
    }

    /// Canonical Cargo metadata kind tokens that map to this kind.
    pub fn metadata_tokens(self) -> &'static [&'static str] {
        match self {
            Self::IntegrationTest => &["test"],
            Self::UnitTestModule => &[],
            Self::Library => &["lib"],
            Self::Binary => &["bin"],
            Self::Bench => &["bench"],
            Self::Example => &["example"],
        }
    }

    /// Parses a canonical token produced by [`Self::as_token`].
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "integration-test" => Some(Self::IntegrationTest),
            "unit-test-module" => Some(Self::UnitTestModule),
            "library" => Some(Self::Library),
            "binary" => Some(Self::Binary),
            "bench" => Some(Self::Bench),
            "example" => Some(Self::Example),
            _ => None,
        }
    }
}

/// Proof role assigned to an execution subject.
///
/// Exactly the six roles fixed by issue #12125. Unknown roles fail
/// deserialization; they are never mapped onto a default role.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofRoleV1 {
    /// Public compatibility proof surface.
    Compatibility,
    /// Parser/analyzer/workspace semantics proof surface.
    CompilerSemantics,
    /// Freshness/current-tree proof surface.
    Currentness,
    /// Provider read-path proof surface (hover/completion/navigation/...).
    ProviderRead,
    /// Edit/refactor proof surface (rename/format/code actions).
    RefactorEdit,
    /// Tooling/harness plumbing that routes or hosts other proofs.
    Infrastructure,
}

impl ProofRoleV1 {
    /// Canonical token used in projections and reports.
    pub fn as_token(self) -> &'static str {
        match self {
            Self::Compatibility => "compatibility",
            Self::CompilerSemantics => "compiler_semantics",
            Self::Currentness => "currentness",
            Self::ProviderRead => "provider_read",
            Self::RefactorEdit => "refactor_edit",
            Self::Infrastructure => "infrastructure",
        }
    }
}

/// Candidate maintenance-profile visibility label.
///
/// Profiles are visibility only: they never claim current workflow routing,
/// requiredness, or scheduling truth.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateProfileV1 {
    /// Cheap enough to ride along with focused PR proof.
    PrFocused,
    /// Expected on the merge-required lane.
    MergeRequired,
    /// Deferred to scheduled pressure runs.
    ScheduledPressure,
    /// Reproducible locally on demand.
    LocalReproduce,
    /// Requires manual research before any automation claim.
    ManualResearch,
}

impl CandidateProfileV1 {
    /// Canonical token used in projections and reports.
    pub fn as_token(self) -> &'static str {
        match self {
            Self::PrFocused => "pr_focused",
            Self::MergeRequired => "merge_required",
            Self::ScheduledPressure => "scheduled_pressure",
            Self::LocalReproduce => "local_reproduce",
            Self::ManualResearch => "manual_research",
        }
    }
}

/// Whether a subject participates in the default feature profile.
///
/// Feature-gated subjects stay explicit even when they compile to zero test
/// cases under the default profile; omission is never an option.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultProfileStateV1 {
    /// Built and executed by default-profile invocations.
    IncludedByDefault,
    /// Compiled to zero under the default profile until its features are selected.
    FeatureGated,
}

/// What must compile before the subject can run.
///
/// This is an obligation identity only — never a satisfaction claim.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompileObligationV1 {
    /// Covered by a `cargo check --all-targets` style compile pass.
    IncludedInCheckAllTargets,
    /// Requires an explicit feature-selected build.
    ExplicitFeatureBuildRequired,
}

/// Execution claim carrier.
///
/// Version 1 of the schema cannot record execution evidence: the inventory is
/// a denominator of governed subjects, not a verdict ledger. Any attempt to
/// mark [`ExecutionClaimV1::claimed`] is refused by validation, including
/// attempts that cite `cargo check --all-targets` success as execution proof.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionClaimV1 {
    /// Always `false` in schema v1; `true` is a validation error.
    #[serde(default)]
    pub claimed: bool,
    /// Optional provenance string for a claim; meaningless while `claimed`
    /// is `false` and never accepted as execution evidence.
    #[serde(default)]
    pub evidence_ref: Option<String>,
}

/// Required/default/forbidden feature subject of one target.
///
/// `required` and `forbidden_under` hold observed Cargo feature NAMES only.
/// Any statement about which combinations are *supported* must be carried by
/// `authority_refs` pointing at the #3790/#8121 identities. Inline feature
/// definitions (matrices, powersets, per-feature role labels) have no field
/// to live in and are rejected by the schema.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureSubjectV1 {
    /// Feature names Cargo requires before this target builds. Sorted.
    #[serde(default)]
    pub required: Vec<String>,
    /// Default-profile participation state.
    pub default_profile_state: DefaultProfileStateV1,
    /// Feature selections under which this subject is excluded. Sorted.
    #[serde(default)]
    pub forbidden_under: Vec<String>,
    /// References into the #3790/#8121 authority space. Sorted, deduped.
    #[serde(default)]
    pub authority_refs: Vec<String>,
}

impl FeatureSubjectV1 {
    /// Builds a validated feature subject.
    ///
    /// Errors when an authority reference is outside
    /// [`FEATURE_AUTHORITIES`] or when a non-trivial subject (any required or
    /// forbidden feature, or a gated default state) carries no `#3790`
    /// reference.
    pub fn new(
        mut required: Vec<String>,
        default_profile_state: DefaultProfileStateV1,
        mut forbidden_under: Vec<String>,
        authority_refs: Vec<String>,
    ) -> anyhow::Result<Self> {
        for reference in &authority_refs {
            if !FEATURE_AUTHORITIES.contains(&reference.as_str()) {
                bail!(
                    "unknown feature authority reference {reference}; rows must reference {:?}",
                    FEATURE_AUTHORITIES
                );
            }
        }
        required.sort();
        required.dedup();
        forbidden_under.sort();
        forbidden_under.dedup();
        let nontrivial = default_profile_state == DefaultProfileStateV1::FeatureGated
            || !required.is_empty()
            || !forbidden_under.is_empty();
        if nontrivial && !authority_refs.iter().any(|reference| reference == "#3790") {
            bail!(
                "feature subject with required={required:?} forbidden={forbidden_under:?} \
                 must reference the #3790 supported-combination authority"
            );
        }
        let mut authority_refs = authority_refs;
        authority_refs.sort();
        authority_refs.dedup();
        Ok(Self { required, default_profile_state, forbidden_under, authority_refs })
    }
}

/// Canonical, root-independent source identity of one row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalSourceIdentityV1 {
    /// Package manifest path, workspace-relative, forward slashes.
    pub manifest_path: String,
    /// Target source path, workspace-relative, forward slashes.
    pub source_path: String,
}

/// One governed execution subject.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TestTopologyRowV1 {
    /// Stable canonical identity: `<package>/<target>/<kind-token>`.
    /// Independent of filesystem root, metadata order, and target dirs.
    pub target_id: String,
    /// Workspace package name (stable package identity; Cargo's own package
    /// id embeds an absolute registry/path root and is therefore not used).
    pub package_id: String,
    /// Cargo target name.
    pub cargo_target_name: String,
    /// Target source path, workspace-relative, forward slashes.
    pub path: String,
    /// Closed target kind.
    pub target_kind: TargetKindV1,
    /// Whether the subject runs under the standard test harness.
    pub harness: bool,
    /// Doctest marker; `Some` only on [`TargetKindV1::Library`] rows.
    pub doctest: Option<bool>,
    /// Required/default/forbidden feature subject.
    pub feature_subject: FeatureSubjectV1,
    /// Assigned proof role.
    pub proof_role: ProofRoleV1,
    /// Semantic owner / controller references (e.g. `#8437`). Sorted, deduped.
    pub controller_refs: Vec<String>,
    /// Candidate visibility profiles. Never empty; never routing truth.
    pub candidate_profiles: BTreeSet<CandidateProfileV1>,
    /// Minimum number of distinct work items the subject must contribute.
    /// Strictly positive; zero is rejected.
    pub minimum_nonzero_work: u32,
    /// Canonical source identity.
    pub canonical_source_identity: CanonicalSourceIdentityV1,
    /// Compile/build obligation identity (never a satisfaction claim).
    pub compile_obligation: CompileObligationV1,
    /// Execution claim; must remain unclaimed in schema v1.
    pub execution_claim: ExecutionClaimV1,
    /// Review condition.
    pub review_condition: String,
    /// Retirement condition.
    pub retirement_condition: String,
    /// SHA-256 over the canonical Cargo-observable subject facts. Detects
    /// subject drift under an unchanged identity (fingerprint/identity drift).
    pub subject_fingerprint: String,
}

impl TestTopologyRowV1 {
    /// Validates intrinsic row invariants independent of discovery.
    ///
    /// Structural checks that need the live tree (existence, kind agreement
    /// with the filesystem) belong to the checker; everything decidable from
    /// the row alone lives here so both deserialization consumers and
    /// generators share one definition of "well formed".
    pub fn validate(&self) -> anyhow::Result<()> {
        let expected_identity = format!(
            "{}/{}/{}",
            self.package_id,
            self.cargo_target_name,
            self.target_kind.as_token()
        );
        if self.target_id != expected_identity {
            // When package and target agree but the kind token diverges, the
            // row is a kind-confused representation of the same subject
            // (e.g. an integration test relabeled as a library-test module).
            let subject_prefix = format!("{}/{}/", self.package_id, self.cargo_target_name);
            if self.target_id.starts_with(&subject_prefix) {
                bail!(
                    "kind confusion: identity {} declares kind {} but the row carries kind {}",
                    self.target_id,
                    self.target_id.strip_prefix(&subject_prefix).unwrap_or("<none>"),
                    self.target_kind.as_token()
                );
            }
            bail!(
                "target_id {} does not follow the canonical <package>/<target>/<kind> form {}",
                self.target_id,
                expected_identity
            );
        }
        if self.path.contains('\\') || self.path.starts_with('/') {
            bail!("path {} must be workspace-relative with forward slashes", self.path);
        }
        let in_tests_directory = self.path.split('/').any(|segment| segment == "tests");
        if in_tests_directory && self.target_kind != TargetKindV1::IntegrationTest {
            bail!(
                "kind confusion: {} lives under tests/ but is represented as {} instead of {}",
                self.target_id,
                self.target_kind.as_token(),
                TargetKindV1::IntegrationTest.as_token()
            );
        }
        if matches!(self.target_kind, TargetKindV1::Library) && self.doctest.is_none() {
            bail!("library row {} must carry an explicit doctest marker", self.target_id);
        }
        if !matches!(self.target_kind, TargetKindV1::Library) && self.doctest.is_some() {
            bail!(
                "doctest marker is a library-only field but row {} kind is {}",
                self.target_id,
                self.target_kind.as_token()
            );
        }
        if self.minimum_nonzero_work == 0 {
            bail!("minimum_nonzero_work must be strictly positive for {}", self.target_id);
        }
        if self.execution_claim.claimed {
            bail!(
                "execution claim refused for {}: the topology inventory records governed \
                 subjects, not execution evidence; compile obligation and execution claims \
                 are represented separately and v1 never claims execution",
                self.target_id
            );
        }
        if self.controller_refs.is_empty() {
            bail!("row {} must name at least the parent controller", self.target_id);
        }
        if self.candidate_profiles.is_empty() {
            bail!("row {} must expose at least one candidate profile", self.target_id);
        }
        if self.subject_fingerprint.is_empty() {
            bail!("row {} must carry a subject fingerprint", self.target_id);
        }
        Ok(())
    }

    /// Computes the subject fingerprint over the canonical Cargo-observable
    /// facts (identity, paths, kind, harness, doctest, feature subject, work
    /// floor, compile obligation). Proof roles, controllers, and profiles are
    /// judgment surfaces compared explicitly by the checker and intentionally
    /// excluded from the fingerprint.
    pub fn compute_fingerprint(&self) -> String {
        let mut canonical = String::new();
        canonical.push_str("id=");
        canonical.push_str(&self.target_id);
        canonical.push('\n');
        canonical.push_str("package=");
        canonical.push_str(&self.package_id);
        canonical.push('\n');
        canonical.push_str("target=");
        canonical.push_str(&self.cargo_target_name);
        canonical.push('\n');
        canonical.push_str("path=");
        canonical.push_str(&self.path);
        canonical.push('\n');
        canonical.push_str("kind=");
        canonical.push_str(self.target_kind.as_token());
        canonical.push('\n');
        canonical.push_str("harness=");
        canonical.push_str(if self.harness { "true" } else { "false" });
        canonical.push('\n');
        canonical.push_str("doctest=");
        match self.doctest {
            Some(value) => canonical.push_str(if value { "true" } else { "false" }),
            None => canonical.push_str("unset"),
        }
        canonical.push('\n');
        canonical.push_str("required=");
        canonical.push_str(&self.feature_subject.required.join(","));
        canonical.push('\n');
        canonical.push_str("default_state=");
        canonical.push_str(match self.feature_subject.default_profile_state {
            DefaultProfileStateV1::IncludedByDefault => "included_by_default",
            DefaultProfileStateV1::FeatureGated => "feature_gated",
        });
        canonical.push('\n');
        canonical.push_str("forbidden=");
        canonical.push_str(&self.feature_subject.forbidden_under.join(","));
        canonical.push('\n');
        canonical.push_str("work_floor=");
        canonical.push_str(&self.minimum_nonzero_work.to_string());
        canonical.push('\n');
        canonical.push_str("compile=");
        canonical.push_str(match self.compile_obligation {
            CompileObligationV1::IncludedInCheckAllTargets => "check-all-targets",
            CompileObligationV1::ExplicitFeatureBuildRequired => "explicit-feature-build",
        });
        canonical.push('\n');
        let digest = Sha256::digest(canonical.as_bytes());
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex
    }
}

/// Versioned inventory of governed execution subjects for one cohort.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TestTopologyInventoryV1 {
    /// Schema identifier; must equal [`SCHEMA_ID`].
    pub schema_id: String,
    /// Schema version; must equal [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Cohort name (e.g. `compiler-critical`).
    pub cohort: String,
    /// Command that produced this artifact.
    pub generated_by: String,
    /// Command that regenerates this artifact (mirrors the generated-files ledger).
    pub regenerate_command: String,
    /// Feature authority references; must equal [`FEATURE_AUTHORITIES`].
    pub feature_authorities: Vec<String>,
    /// Convergence controllers governing this inventory. Sorted, deduped.
    pub controllers: Vec<String>,
    /// Rows, canonically sorted by [`TestTopologyRowV1::target_id`].
    pub rows: Vec<TestTopologyRowV1>,
}

impl TestTopologyInventoryV1 {
    /// Builds an inventory from rows, enforcing schema constants, canonical
    /// ordering, uniqueness, and per-row validity.
    pub fn new(
        cohort: &str,
        generated_by: &str,
        controllers: &[String],
        mut rows: Vec<TestTopologyRowV1>,
    ) -> anyhow::Result<Self> {
        rows.sort_by(|left, right| left.target_id.cmp(&right.target_id));
        let mut seen = BTreeSet::new();
        for row in &rows {
            if row.target_id.is_empty() {
                bail!("inventory rows must carry a non-empty target_id");
            }
            if !seen.insert(row.target_id.as_str()) {
                bail!(
                    "duplicate canonical identity {} (path aliases or changed workspace roots \
                     must collapse to one row, never duplicate)",
                    row.target_id
                );
            }
            row.validate().with_context(|| format!("validating row {}", row.target_id))?;
        }
        Ok(Self {
            schema_id: SCHEMA_ID.to_string(),
            schema_version: SCHEMA_VERSION,
            cohort: cohort.to_string(),
            generated_by: generated_by.to_string(),
            regenerate_command: generated_by.to_string(),
            feature_authorities: FEATURE_AUTHORITIES
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            controllers: {
                let mut controllers = controllers.to_vec();
                controllers.sort();
                controllers.dedup();
                controllers
            },
            rows,
        })
    }

    /// Validates intrinsic inventory invariants (schema constants, ordering,
    /// duplicates, row validity). Cross-checking against live discovery is a
    /// checker concern, not a schema invariant.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.schema_id != SCHEMA_ID || self.schema_version != SCHEMA_VERSION {
            bail!(
                "unsupported topology schema {} v{}; expected {} v{}",
                self.schema_id,
                self.schema_version,
                SCHEMA_ID,
                SCHEMA_VERSION
            );
        }
        let authorities: Vec<String> = self.feature_authorities.clone();
        let expected: Vec<String> =
            FEATURE_AUTHORITIES.iter().map(|value| (*value).to_string()).collect();
        if authorities != expected {
            bail!(
                "feature_authorities must be exactly {expected:?}; found {authorities:?}. \
                 Rows reference the #3790/#8121 authorities; they never redefine or copy \
                 the feature matrix."
            );
        }
        if self.controllers.is_empty() {
            bail!("inventory must name at least one convergence controller");
        }
        let mut previous: Option<&str> = None;
        let mut seen = BTreeSet::new();
        for row in &self.rows {
            if !seen.insert(row.target_id.as_str()) {
                bail!(
                    "duplicate canonical identity {} (path aliases or changed workspace roots \
                     must collapse to one row)",
                    row.target_id
                );
            }
            if let Some(previous) = previous
                && row.target_id.as_str() <= previous
            {
                bail!(
                    "canonical ordering drift: {} follows {}; rows must be strictly \
                     sorted by target_id",
                    row.target_id,
                    previous
                );
            }
            row.validate().with_context(|| format!("validating row {}", row.target_id))?;
            previous = Some(row.target_id.as_str());
        }
        Ok(())
    }
}

/// Deserializes an inventory from JSON with context-rich errors.
pub fn inventory_from_json(json: &str) -> anyhow::Result<TestTopologyInventoryV1> {
    let inventory: TestTopologyInventoryV1 = serde_json::from_str(json).map_err(|error| {
        anyhow::Error::new(error).context(
            "rejected topology inventory JSON: the schema is closed, so unknown fields, \
                 unknown kinds, unknown proof roles, and unknown profiles are hard errors",
        )
    })?;
    inventory.validate()?;
    Ok(inventory)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_row(target_id: &str, kind: TargetKindV1) -> TestTopologyRowV1 {
        // target_id form under test: <package>/<target>/<kind-token>.
        let mut segments = target_id.split('/');
        let package = segments.next().unwrap_or_default().to_string();
        let target_name = segments.next().unwrap_or_default().to_string();
        let mut row = TestTopologyRowV1 {
            target_id: target_id.to_string(),
            package_id: package,
            cargo_target_name: target_name,
            path: "crates/sample/src/lib.rs".to_string(),
            target_kind: kind,
            harness: true,
            doctest: None,
            feature_subject: FeatureSubjectV1 {
                required: Vec::new(),
                default_profile_state: DefaultProfileStateV1::IncludedByDefault,
                forbidden_under: Vec::new(),
                authority_refs: Vec::new(),
            },
            proof_role: ProofRoleV1::Infrastructure,
            controller_refs: vec![PARENT_CONTROLLER.to_string()],
            candidate_profiles: BTreeSet::from([CandidateProfileV1::PrFocused]),
            minimum_nonzero_work: 1,
            canonical_source_identity: CanonicalSourceIdentityV1 {
                manifest_path: "crates/sample/Cargo.toml".to_string(),
                source_path: "crates/sample/src/lib.rs".to_string(),
            },
            compile_obligation: CompileObligationV1::IncludedInCheckAllTargets,
            execution_claim: ExecutionClaimV1::default(),
            review_condition: REVIEW_CONDITION.to_string(),
            retirement_condition: RETIREMENT_CONDITION.to_string(),
            subject_fingerprint: String::new(),
        };
        if kind == TargetKindV1::Library {
            row.doctest = Some(false);
        }
        row.subject_fingerprint = row.compute_fingerprint();
        row
    }

    #[test]
    fn unknown_proof_role_is_never_coerced() -> anyhow::Result<()> {
        let payload = r##"{
            "schema_id": "test_topology_inventory.v1",
            "schema_version": 1,
            "cohort": "compiler-critical",
            "generated_by": "fixture",
            "regenerate_command": "fixture",
            "feature_authorities": ["#3790", "#8121"],
            "controllers": ["#8437"],
            "rows": [{
                "target_id": "pkg/t/library",
                "package_id": "pkg",
                "cargo_target_name": "t",
                "path": "crates/pkg/src/lib.rs",
                "target_kind": "library",
                "harness": true,
                "doctest": false,
                "feature_subject": {
                    "required": [],
                    "default_profile_state": "included_by_default",
                    "forbidden_under": [],
                    "authority_refs": []
                },
                "proof_role": "ordinary_compile_only",
                "controller_refs": ["#8437"],
                "candidate_profiles": ["pr_focused"],
                "minimum_nonzero_work": 1,
                "canonical_source_identity": {
                    "manifest_path": "crates/pkg/Cargo.toml",
                    "source_path": "crates/pkg/src/lib.rs"
                },
                "compile_obligation": "included_in_check_all_targets",
                "execution_claim": {},
                "review_condition": "r",
                "retirement_condition": "r",
                "subject_fingerprint": "x"
            }]
        }"##;
        let error = inventory_from_json(payload)
            .err()
            .ok_or_else(|| anyhow::anyhow!("unknown proof_role must be rejected"))?;
        assert!(
            format!("{error:#}").contains("rejected topology inventory"),
            "error should come from the closed schema, got: {error:#}"
        );
        Ok(())
    }

    #[test]
    fn execution_claims_are_refused_even_when_citing_check_all_targets() -> anyhow::Result<()> {
        let mut row = sample_row("pkg/t/library", TargetKindV1::Library);
        row.execution_claim = ExecutionClaimV1 {
            claimed: true,
            evidence_ref: Some("cargo check --all-targets exit 0".into()),
        };
        let error = row
            .validate()
            .err()
            .ok_or_else(|| anyhow::anyhow!("claimed execution must be refused"))?;
        assert!(
            format!("{error:#}").contains("execution claim refused"),
            "unexpected error: {error:#}"
        );
        Ok(())
    }

    #[test]
    fn kind_confusion_between_tests_dir_and_module_rows_is_rejected() -> anyhow::Result<()> {
        let mut row = sample_row("pkg/t/unit-test-module", TargetKindV1::UnitTestModule);
        row.path = "crates/pkg/tests/t.rs".to_string();
        let error = row.validate().err().ok_or_else(|| {
            anyhow::anyhow!("tests/ path with unit-test-module kind must be rejected")
        })?;
        assert!(format!("{error:#}").contains("kind confusion"), "unexpected error: {error:#}");
        Ok(())
    }

    #[test]
    fn zero_work_requirement_is_rejected() -> anyhow::Result<()> {
        let mut row = sample_row("pkg/t/library", TargetKindV1::Library);
        row.minimum_nonzero_work = 0;
        let error = row
            .validate()
            .err()
            .ok_or_else(|| anyhow::anyhow!("zero work floor must be rejected"))?;
        assert!(format!("{error:#}").contains("strictly positive"), "unexpected error: {error:#}");
        Ok(())
    }

    #[test]
    fn inline_feature_definition_payload_is_rejected_by_the_closed_schema() -> anyhow::Result<()> {
        let payload = r##"{
            "target_id": "pkg/t/library",
            "package_id": "pkg",
            "cargo_target_name": "t",
            "path": "crates/pkg/src/lib.rs",
            "target_kind": "library",
            "harness": true,
            "doctest": false,
            "feature_definitions": { "supported": [["a", "b"]] },
            "feature_subject": {
                "required": [],
                "default_profile_state": "included_by_default",
                "forbidden_under": [],
                "authority_refs": []
            },
            "proof_role": "infrastructure",
            "controller_refs": ["#8437"],
            "candidate_profiles": ["pr_focused"],
            "minimum_nonzero_work": 1,
            "canonical_source_identity": {
                "manifest_path": "crates/pkg/Cargo.toml",
                "source_path": "crates/pkg/src/lib.rs"
            },
            "compile_obligation": "included_in_check_all_targets",
            "execution_claim": {},
            "review_condition": "r",
            "retirement_condition": "r",
            "subject_fingerprint": "x"
        }"##;
        let result: Result<TestTopologyRowV1, _> = serde_json::from_str(payload);
        let error = result
            .err()
            .ok_or_else(|| anyhow::anyhow!("inline feature definitions must be rejected"))?;
        assert!(
            format!("{error}").contains("unknown field"),
            "expected deny_unknown_fields rejection, got: {error}"
        );
        Ok(())
    }

    #[test]
    fn invented_feature_authority_references_are_rejected() -> anyhow::Result<()> {
        let error = FeatureSubjectV1::new(
            vec!["lsp-compat".to_string()],
            DefaultProfileStateV1::FeatureGated,
            Vec::new(),
            vec!["#9999-local-matrix-copy".to_string()],
        )
        .err()
        .ok_or_else(|| anyhow::anyhow!("invented authority reference must be rejected"))?;
        assert!(
            format!("{error:#}").contains("unknown feature authority reference"),
            "unexpected error: {error:#}"
        );
        Ok(())
    }
}
