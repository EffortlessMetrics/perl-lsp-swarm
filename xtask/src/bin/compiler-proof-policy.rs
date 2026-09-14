//! Validate and render the compiler proof/composition policy from #6689.
//!
//! ```text
//! cargo run -p xtask --bin compiler-proof-policy -- --check
//! cargo run -p xtask --bin compiler-proof-policy -- --write-status
//! ```

#![allow(clippy::print_stdout)]

use anyhow::{Context, Result, anyhow, bail};
use chrono::{NaiveDate, Utc};
use clap::Parser;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const POLICY_SCHEMA: &str = "perl_compiler_proof_policy.v1";
const DEFAULT_POLICY: &str = "contracts/compiler/perl_compiler_proof_policy.v1.toml";
const DEFAULT_CONCEPT_LEDGER: &str = "contracts/compiler/perl_compiler_concepts.v1.toml";
const DEFAULT_STATUS: &str = "docs/project/status/perl_compiler_proof_policy.md";

#[derive(Debug, Parser)]
#[command(name = "compiler-proof-policy")]
#[command(about = "Validate and render compiler concept proof policy")]
struct Cli {
    #[arg(long, default_value = DEFAULT_POLICY)]
    policy: PathBuf,

    #[arg(long, default_value = DEFAULT_CONCEPT_LEDGER)]
    concept_ledger: PathBuf,

    #[arg(long, default_value = DEFAULT_STATUS)]
    status: PathBuf,

    #[arg(long)]
    check: bool,

    #[arg(long)]
    write_status: bool,

    /// Date used to resolve disposition expiry. Defaults to the current UTC date.
    ///
    /// Expiry is deliberately excluded from the generated projection so the
    /// checked-in status stays deterministic; it is resolved here instead.
    #[arg(long, value_name = "YYYY-MM-DD")]
    as_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ProofPolicy {
    schema_version: String,
    policy_id: String,
    controller_issue: String,
    concept_ledger_schema: String,
    coverage_scope: String,
    claim_boundary: String,
    complete: bool,
    #[serde(default)]
    closure_authority: Option<String>,
    proof_classes: Vec<ProofClass>,
    dimensions: Vec<Dimension>,
    campaigns: Vec<Campaign>,
    /// Vocabulary deliberately retained without a campaign exercising it.
    ///
    /// Empty is the closed state. Every entry is an unresolved obligation, so a
    /// non-empty set makes derived closure `false` however the source spells
    /// `complete`.
    #[serde(default)]
    dispositions: Vec<Disposition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ProofClass {
    class_id: String,
    purpose: String,
    authority: Authority,
    claim_stages: Vec<ClaimStage>,
    circular_output_allowed: bool,
    missing_effect: MissingEffect,
    owner_issue: String,
    /// Roles the evidence under this class is allowed to play.
    ///
    /// Separate from `authority`: authority says who produced the evidence,
    /// role says what it may be read as. Independently authored structural
    /// expectations, compiler-generated snapshots, and execution receipts are
    /// not interchangeable even when they serialize alike.
    evidence_roles: Vec<EvidenceRole>,
    /// Required when, and only when, more than one role is admitted.
    #[serde(default)]
    multi_role_claim_ceiling: Option<String>,
}

/// One typed record retaining a declared vocabulary item that no campaign exercises.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Disposition {
    item_kind: ItemKind,
    item_id: String,
    status: DispositionStatus,
    reason: String,
    owner_issue: String,
    basis: DispositionBasis,
    /// ISO-8601 date after which the disposition must be re-decided.
    review_after: String,
    /// The condition that ends the disposition, independent of the calendar.
    exit_condition: String,
    claim_effect: ClaimEffect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Dimension {
    dimension_id: String,
    values: Vec<String>,
    owner_issue: String,
    claim_boundary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Campaign {
    campaign_id: String,
    concept_families: Vec<String>,
    dimensions: Vec<String>,
    proof_classes: Vec<String>,
    owner_issue: String,
    claim_boundary: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Authority {
    IndependentGold,
    CompilerSnapshot,
    MutationFixture,
    IndependentFixture,
    EirDifferential,
    RealPerlOracle,
    CompositionHarness,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum ClaimStage {
    Parser,
    FlatHir,
    BodyHir,
    PirA,
    EffectsWorld,
    Eir,
    Provider,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)]
#[serde(rename_all = "snake_case")]
enum MissingEffect {
    BlocksClaim,
    BlocksStage,
    BlocksExecutionClaim,
    BlocksClaimWhenObservable,
}

/// What a piece of evidence may be read as, independent of who produced it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum EvidenceRole {
    /// Expectations authored against the specification rather than the compiler.
    IndependentStructuralExpectation,
    /// Output the compiler itself produced, accepted as a change detector only.
    CompilerGeneratedSnapshot,
    /// The observed result of running something.
    ExecutionReceipt,
    /// Behavior observed from real Perl.
    RealPerlOracleObservation,
    /// The result of injecting one defect and requiring a verifier to reject it.
    VerifierMutationResult,
    /// Source-order, identity, and invalidation fixtures for effects and world state.
    EffectsWorldFixture,
    /// Profile-stamped execution compared against declared reference behavior.
    EirDifferentialResult,
    /// Cross-family interaction output from the composition harness.
    CompositionResult,
    /// A diagnostic the implementation emitted about itself.
    ImplementationDiagnosticReceipt,
}

/// Which declared vocabulary a disposition retains.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum ItemKind {
    ProofClass,
    Dimension,
    ConceptFamily,
}

/// Why the item is retained without being exercised.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum DispositionStatus {
    /// A named owner issue must supply the campaign.
    DeferredToOwner,
    /// A prerequisite must land before the item can be exercised.
    SequencingBlocked,
    /// The item is scheduled for removal once its consumers move.
    SupersededPendingRemoval,
}

/// Whether the retention is a semantic or an ordering decision.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum DispositionBasis {
    Semantic,
    Sequencing,
}

/// What the retention does to claims that would otherwise rely on the item.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum ClaimEffect {
    /// No claim may rest on the item while it is retained.
    ExcludedFromClaims,
    /// Claims may proceed under an explicitly narrowed ceiling.
    ClaimsLimited,
}

#[derive(Debug, Clone, Deserialize)]
struct ConceptLedgerIndex {
    schema_version: String,
    concepts: Vec<ConceptIndexRow>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConceptIndexRow {
    family: String,
}

trait StableName {
    fn stable_name(self) -> &'static str;
}

impl StableName for Authority {
    fn stable_name(self) -> &'static str {
        match self {
            Self::IndependentGold => "independent_gold",
            Self::CompilerSnapshot => "compiler_snapshot",
            Self::MutationFixture => "mutation_fixture",
            Self::IndependentFixture => "independent_fixture",
            Self::EirDifferential => "eir_differential",
            Self::RealPerlOracle => "real_perl_oracle",
            Self::CompositionHarness => "composition_harness",
        }
    }
}

impl StableName for ClaimStage {
    fn stable_name(self) -> &'static str {
        match self {
            Self::Parser => "parser",
            Self::FlatHir => "flat_hir",
            Self::BodyHir => "body_hir",
            Self::PirA => "pir_a",
            Self::EffectsWorld => "effects_world",
            Self::Eir => "eir",
            Self::Provider => "provider",
        }
    }
}

impl StableName for MissingEffect {
    fn stable_name(self) -> &'static str {
        match self {
            Self::BlocksClaim => "blocks_claim",
            Self::BlocksStage => "blocks_stage",
            Self::BlocksExecutionClaim => "blocks_execution_claim",
            Self::BlocksClaimWhenObservable => "blocks_claim_when_observable",
        }
    }
}

impl StableName for EvidenceRole {
    fn stable_name(self) -> &'static str {
        match self {
            Self::IndependentStructuralExpectation => "independent_structural_expectation",
            Self::CompilerGeneratedSnapshot => "compiler_generated_snapshot",
            Self::ExecutionReceipt => "execution_receipt",
            Self::RealPerlOracleObservation => "real_perl_oracle_observation",
            Self::VerifierMutationResult => "verifier_mutation_result",
            Self::EffectsWorldFixture => "effects_world_fixture",
            Self::EirDifferentialResult => "eir_differential_result",
            Self::CompositionResult => "composition_result",
            Self::ImplementationDiagnosticReceipt => "implementation_diagnostic_receipt",
        }
    }
}

impl StableName for ItemKind {
    fn stable_name(self) -> &'static str {
        match self {
            Self::ProofClass => "proof_class",
            Self::Dimension => "dimension",
            Self::ConceptFamily => "concept_family",
        }
    }
}

impl StableName for DispositionStatus {
    fn stable_name(self) -> &'static str {
        match self {
            Self::DeferredToOwner => "deferred_to_owner",
            Self::SequencingBlocked => "sequencing_blocked",
            Self::SupersededPendingRemoval => "superseded_pending_removal",
        }
    }
}

impl StableName for DispositionBasis {
    fn stable_name(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::Sequencing => "sequencing",
        }
    }
}

impl StableName for ClaimEffect {
    fn stable_name(self) -> &'static str {
        match self {
            Self::ExcludedFromClaims => "excluded_from_claims",
            Self::ClaimsLimited => "claims_limited",
        }
    }
}

/// Roles an authority is allowed to produce.
///
/// This is the compatibility rule the issue requires to be explicit: an
/// `independent_gold` class can never admit compiler output or an execution
/// receipt, however its rows are spelled.
fn authority_admissible_roles(authority: Authority) -> &'static [EvidenceRole] {
    match authority {
        Authority::IndependentGold => &[EvidenceRole::IndependentStructuralExpectation],
        Authority::CompilerSnapshot => &[
            EvidenceRole::CompilerGeneratedSnapshot,
            EvidenceRole::ImplementationDiagnosticReceipt,
        ],
        Authority::MutationFixture => &[EvidenceRole::VerifierMutationResult],
        Authority::IndependentFixture => &[EvidenceRole::EffectsWorldFixture],
        Authority::EirDifferential => {
            &[EvidenceRole::EirDifferentialResult, EvidenceRole::ExecutionReceipt]
        }
        Authority::RealPerlOracle => {
            &[EvidenceRole::RealPerlOracleObservation, EvidenceRole::ExecutionReceipt]
        }
        Authority::CompositionHarness => &[EvidenceRole::CompositionResult],
    }
}

#[derive(Debug, Clone, Copy)]
struct ProofClassContract {
    authority: Authority,
    claim_stages: &'static [ClaimStage],
    circular_output_allowed: bool,
    missing_effect: MissingEffect,
    evidence_roles: &'static [EvidenceRole],
}

fn proof_class_contract(class_id: &str) -> Result<ProofClassContract> {
    let contract = match class_id {
        "positive_gold" => ProofClassContract {
            authority: Authority::IndependentGold,
            claim_stages: &[
                ClaimStage::BodyHir,
                ClaimStage::PirA,
                ClaimStage::EffectsWorld,
                ClaimStage::Eir,
                ClaimStage::Provider,
            ],
            circular_output_allowed: false,
            missing_effect: MissingEffect::BlocksClaim,
            evidence_roles: &[EvidenceRole::IndependentStructuralExpectation],
        },
        "negative_gold" => ProofClassContract {
            authority: Authority::IndependentGold,
            claim_stages: &[
                ClaimStage::BodyHir,
                ClaimStage::PirA,
                ClaimStage::EffectsWorld,
                ClaimStage::Eir,
                ClaimStage::Provider,
            ],
            circular_output_allowed: false,
            missing_effect: MissingEffect::BlocksClaim,
            evidence_roles: &[EvidenceRole::IndependentStructuralExpectation],
        },
        "boundary_gold" => ProofClassContract {
            authority: Authority::IndependentGold,
            claim_stages: &[
                ClaimStage::BodyHir,
                ClaimStage::PirA,
                ClaimStage::EffectsWorld,
                ClaimStage::Eir,
                ClaimStage::Provider,
            ],
            circular_output_allowed: false,
            missing_effect: MissingEffect::BlocksClaim,
            evidence_roles: &[EvidenceRole::IndependentStructuralExpectation],
        },
        "recovery_gold" => ProofClassContract {
            authority: Authority::IndependentGold,
            claim_stages: &[ClaimStage::Parser, ClaimStage::BodyHir, ClaimStage::Provider],
            circular_output_allowed: false,
            missing_effect: MissingEffect::BlocksClaim,
            evidence_roles: &[EvidenceRole::IndependentStructuralExpectation],
        },
        "hir_snapshot" => ProofClassContract {
            authority: Authority::CompilerSnapshot,
            claim_stages: &[ClaimStage::FlatHir, ClaimStage::BodyHir],
            circular_output_allowed: true,
            missing_effect: MissingEffect::BlocksStage,
            evidence_roles: &[EvidenceRole::CompilerGeneratedSnapshot],
        },
        "pir_snapshot" => ProofClassContract {
            authority: Authority::CompilerSnapshot,
            claim_stages: &[ClaimStage::PirA],
            circular_output_allowed: true,
            missing_effect: MissingEffect::BlocksStage,
            evidence_roles: &[EvidenceRole::CompilerGeneratedSnapshot],
        },
        "verifier_mutation" => ProofClassContract {
            authority: Authority::MutationFixture,
            claim_stages: &[ClaimStage::PirA, ClaimStage::Eir, ClaimStage::Provider],
            circular_output_allowed: false,
            missing_effect: MissingEffect::BlocksClaim,
            evidence_roles: &[EvidenceRole::VerifierMutationResult],
        },
        "effects_world_fixture" => ProofClassContract {
            authority: Authority::IndependentFixture,
            claim_stages: &[ClaimStage::EffectsWorld, ClaimStage::Provider],
            circular_output_allowed: false,
            missing_effect: MissingEffect::BlocksClaim,
            evidence_roles: &[EvidenceRole::EffectsWorldFixture],
        },
        "eir_differential" => ProofClassContract {
            authority: Authority::EirDifferential,
            claim_stages: &[ClaimStage::Eir],
            circular_output_allowed: false,
            missing_effect: MissingEffect::BlocksExecutionClaim,
            evidence_roles: &[EvidenceRole::EirDifferentialResult, EvidenceRole::ExecutionReceipt],
        },
        "real_perl_oracle" => ProofClassContract {
            authority: Authority::RealPerlOracle,
            claim_stages: &[ClaimStage::EffectsWorld, ClaimStage::Eir, ClaimStage::Provider],
            circular_output_allowed: false,
            missing_effect: MissingEffect::BlocksClaimWhenObservable,
            evidence_roles: &[
                EvidenceRole::RealPerlOracleObservation,
                EvidenceRole::ExecutionReceipt,
            ],
        },
        "composition_coverage" => ProofClassContract {
            authority: Authority::CompositionHarness,
            claim_stages: &[
                ClaimStage::BodyHir,
                ClaimStage::PirA,
                ClaimStage::EffectsWorld,
                ClaimStage::Eir,
                ClaimStage::Provider,
            ],
            circular_output_allowed: false,
            missing_effect: MissingEffect::BlocksClaim,
            evidence_roles: &[EvidenceRole::CompositionResult],
        },
        _ => bail!("unknown compiler proof class {:?}", class_id),
    };
    Ok(contract)
}

impl ProofPolicy {
    fn from_str(source: &str) -> Result<Self> {
        let policy: Self = toml::from_str(source).context("parse compiler proof policy")?;
        Ok(policy)
    }

    fn load(path: &Path) -> Result<Self> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("read compiler proof policy {}", path.display()))?;
        Self::from_str(&source)
            .with_context(|| format!("parse compiler proof policy {}", path.display()))
    }

    fn validate(&self, concepts: &ConceptLedgerIndex) -> Result<()> {
        if self.schema_version != POLICY_SCHEMA {
            bail!(
                "unsupported compiler proof policy schema {:?}; expected {:?}",
                self.schema_version,
                POLICY_SCHEMA
            );
        }
        if self.concept_ledger_schema != concepts.schema_version {
            bail!(
                "proof policy expects concept schema {:?}, but ledger uses {:?}",
                self.concept_ledger_schema,
                concepts.schema_version
            );
        }
        for (name, value) in [
            ("policy_id", self.policy_id.as_str()),
            ("controller_issue", self.controller_issue.as_str()),
            ("coverage_scope", self.coverage_scope.as_str()),
            ("claim_boundary", self.claim_boundary.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("compiler proof policy field {name} must not be empty");
            }
        }
        validate_issue("controller_issue", &self.controller_issue)?;
        if let Some(authority) = self.closure_authority.as_deref() {
            validate_closure_authority(authority, &self.controller_issue)?;
        }
        if self.proof_classes.is_empty() || self.dimensions.is_empty() || self.campaigns.is_empty()
        {
            bail!("proof classes, dimensions, and campaigns must all be non-empty");
        }

        let mut class_ids = BTreeSet::new();
        for proof_class in &self.proof_classes {
            proof_class.validate()?;
            if !class_ids.insert(proof_class.class_id.as_str()) {
                bail!("duplicate proof class {:?}", proof_class.class_id);
            }
        }

        let composition = self
            .proof_classes
            .iter()
            .find(|proof_class| proof_class.class_id == "composition_coverage")
            .ok_or_else(|| anyhow!("proof policy must define composition_coverage"))?;
        let expected_composition_stages = [
            ClaimStage::BodyHir,
            ClaimStage::PirA,
            ClaimStage::EffectsWorld,
            ClaimStage::Eir,
            ClaimStage::Provider,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        let actual_composition_stages =
            composition.claim_stages.iter().copied().collect::<BTreeSet<_>>();
        if composition.authority != Authority::CompositionHarness
            || composition.circular_output_allowed
            || composition.missing_effect != MissingEffect::BlocksClaim
            || actual_composition_stages != expected_composition_stages
        {
            bail!(
                "composition_coverage must retain composition_harness authority, non-circular output, blocks_claim semantics, and canonical claim stages"
            );
        }

        let mut dimension_ids = BTreeSet::new();
        for dimension in &self.dimensions {
            dimension.validate()?;
            if !dimension_ids.insert(dimension.dimension_id.as_str()) {
                bail!("duplicate composition dimension {:?}", dimension.dimension_id);
            }
        }

        let concept_families = concept_family_index(concepts)?;
        let mut campaign_ids = BTreeSet::new();
        let mut campaign_member_sets = BTreeMap::<CampaignMembers, &str>::new();
        for campaign in &self.campaigns {
            campaign.validate(&class_ids, &dimension_ids, &concept_families)?;
            if !campaign_ids.insert(campaign.campaign_id.as_str()) {
                bail!("duplicate proof campaign {:?}", campaign.campaign_id);
            }
            if let Some(existing) =
                campaign_member_sets.insert(campaign.members(), campaign.campaign_id.as_str())
            {
                bail!(
                    "campaigns {:?} and {:?} declare the same exact member set",
                    existing,
                    campaign.campaign_id
                );
            }
        }

        let referenced_classes = self
            .campaigns
            .iter()
            .flat_map(|campaign| campaign.proof_classes.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();
        let referenced_dimensions = self
            .campaigns
            .iter()
            .flat_map(|campaign| campaign.dimensions.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();
        let referenced_families = self
            .campaigns
            .iter()
            .flat_map(|campaign| campaign.concept_families.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();

        self.validate_dispositions(&class_ids, &dimension_ids, &concept_families)?;
        let dispositioned = self.dispositioned_ids();
        for (kind, declared, exercised) in [
            (ItemKind::ProofClass, &class_ids, &referenced_classes),
            (ItemKind::Dimension, &dimension_ids, &referenced_dimensions),
            (ItemKind::ConceptFamily, &concept_families, &referenced_families),
        ] {
            let retained = dispositioned.get(&kind).cloned().unwrap_or_default();
            let contradictory =
                exercised.iter().filter(|item| retained.contains(**item)).collect::<Vec<_>>();
            if !contradictory.is_empty() {
                bail!(
                    "{} dispositions contradict campaign coverage: {:?}",
                    kind.stable_name(),
                    contradictory
                );
            }
            let covered = exercised
                .union(&retained.iter().copied().collect::<BTreeSet<_>>())
                .copied()
                .collect::<BTreeSet<_>>();
            if &covered != declared {
                bail!(
                    "proof policy has {} entries that are neither exercised nor dispositioned: {:?}",
                    kind.stable_name(),
                    declared.difference(&covered).copied().collect::<Vec<_>>()
                );
            }
        }

        let derived = self.derived_complete();
        if self.complete != derived {
            bail!(
                "compiler proof policy declares complete={} but closed state derives complete={}; \
                 closure requires a controller-bound closure_authority and zero retained dispositions",
                self.complete,
                derived
            );
        }
        Ok(())
    }

    /// Closure is derived, never asserted.
    ///
    /// A disposition is by construction an unresolved obligation, so any
    /// retained row keeps the policy open no matter what the source says.
    fn derived_complete(&self) -> bool {
        self.closure_authority.is_some() && self.dispositions.is_empty()
    }

    fn dispositioned_ids(&self) -> BTreeMap<ItemKind, BTreeSet<&str>> {
        let mut index = BTreeMap::<ItemKind, BTreeSet<&str>>::new();
        for disposition in &self.dispositions {
            index.entry(disposition.item_kind).or_default().insert(disposition.item_id.as_str());
        }
        index
    }

    fn validate_dispositions(
        &self,
        proof_classes: &BTreeSet<&str>,
        dimensions: &BTreeSet<&str>,
        concept_families: &BTreeSet<&str>,
    ) -> Result<()> {
        let mut seen = BTreeSet::new();
        for disposition in &self.dispositions {
            disposition.validate(proof_classes, dimensions, concept_families)?;
            if !seen.insert((disposition.item_kind, disposition.item_id.as_str())) {
                bail!(
                    "duplicate disposition for {} {:?}",
                    disposition.item_kind.stable_name(),
                    disposition.item_id
                );
            }
        }
        Ok(())
    }

    /// Reject a disposition whose review date has passed at `as_of`.
    ///
    /// Kept out of `validate` so the generated projection stays deterministic:
    /// an expired row is a command failure, not a doc that can be regenerated
    /// back to green.
    fn check_currentness(&self, as_of: NaiveDate) -> Result<()> {
        for disposition in &self.dispositions {
            let review_after = parse_date("review_after", &disposition.review_after)?;
            if review_after < as_of {
                bail!(
                    "disposition for {} {:?} expired on {} (as of {}); re-decide it against {:?} \
                     rather than advancing the date",
                    disposition.item_kind.stable_name(),
                    disposition.item_id,
                    disposition.review_after,
                    as_of,
                    disposition.exit_condition
                );
            }
        }
        Ok(())
    }

    fn canonicalized(&self) -> Self {
        let mut normalized = self.clone();
        normalized.proof_classes.sort_by(|left, right| left.class_id.cmp(&right.class_id));
        normalized.dimensions.sort_by(|left, right| left.dimension_id.cmp(&right.dimension_id));
        normalized.campaigns.sort_by(|left, right| left.campaign_id.cmp(&right.campaign_id));
        normalized.dispositions.sort_by(|left, right| {
            (left.item_kind, &left.item_id).cmp(&(right.item_kind, &right.item_id))
        });
        for proof_class in &mut normalized.proof_classes {
            proof_class.claim_stages.sort();
            proof_class.evidence_roles.sort();
        }
        for dimension in &mut normalized.dimensions {
            dimension.values.sort();
        }
        for campaign in &mut normalized.campaigns {
            campaign.concept_families.sort();
            campaign.dimensions.sort();
            campaign.proof_classes.sort();
        }
        normalized
    }

    /// Digest every load-bearing policy field, including the ones the tables omit.
    ///
    /// Comparing rendered text alone cannot see a changed `purpose` or campaign
    /// `claim_boundary`, so an edit to either used to leave the status current.
    fn policy_digest(&self) -> Result<String> {
        let canonical = toml::to_string(&self.canonicalized())
            .context("serialize canonical compiler proof policy for digest")?;
        Ok(digest(&canonical))
    }

    fn render_markdown(&self, concepts: &ConceptLedgerIndex) -> Result<String> {
        self.validate(concepts)?;
        let normalized = self.canonicalized();
        let mut output = String::new();

        line(&mut output, "# Perl Compiler Proof Policy")?;
        line(&mut output, "")?;
        line(
            &mut output,
            "> Generated by `cargo run -p xtask --bin compiler-proof-policy -- --write-status`.",
        )?;
        line(
            &mut output,
            "> Check with `cargo run -p xtask --bin compiler-proof-policy -- --check`.",
        )?;
        line(&mut output, "")?;
        line(&mut output, &normalized.coverage_scope)?;
        line(&mut output, "")?;
        line(&mut output, &format!("- Schema: `{}`", normalized.schema_version))?;
        line(&mut output, &format!("- Policy: `{}`", normalized.policy_id))?;
        line(&mut output, &format!("- Controller: {}", normalized.controller_issue))?;
        line(&mut output, &format!("- Concept schema: `{}`", normalized.concept_ledger_schema))?;
        line(
            &mut output,
            &format!("- Policy vocabulary closed: `{}` (derived)", normalized.derived_complete()),
        )?;
        if let Some(authority) = &normalized.closure_authority {
            line(&mut output, &format!("- Closure authority: `{authority}`"))?;
        } else {
            line(&mut output, "- Closure authority: —")?;
        }
        line(&mut output, &format!("- Proof classes: `{}`", normalized.proof_classes.len()))?;
        line(&mut output, &format!("- Composition dimensions: `{}`", normalized.dimensions.len()))?;
        line(&mut output, &format!("- Campaigns: `{}`", normalized.campaigns.len()))?;
        line(
            &mut output,
            &format!("- Retained dispositions: `{}`", normalized.dispositions.len()),
        )?;
        line(&mut output, &format!("- Policy digest: `{}`", normalized.policy_digest()?))?;
        line(
            &mut output,
            &format!("- Concept projection digest: `{}`", concept_projection_digest(concepts)),
        )?;
        line(&mut output, "")?;
        line(&mut output, &format!("**Claim boundary:** {}", normalized.claim_boundary))?;
        line(&mut output, "")?;

        line(&mut output, "## Proof classes")?;
        line(&mut output, "")?;
        line(
            &mut output,
            "| Proof class | Authority | Claim stages | Circular output | Missing effect | Owner |",
        )?;
        line(&mut output, "| --- | --- | --- | --- | --- | --- |")?;
        for proof_class in &normalized.proof_classes {
            let stages = proof_class
                .claim_stages
                .iter()
                .map(|stage| format!("`{}`", stage.stable_name()))
                .collect::<Vec<_>>()
                .join(", ");
            line(
                &mut output,
                &format!(
                    "| `{}` | `{}` | {} | `{}` | `{}` | {} |",
                    proof_class.class_id,
                    proof_class.authority.stable_name(),
                    stages,
                    proof_class.circular_output_allowed,
                    proof_class.missing_effect.stable_name(),
                    proof_class.owner_issue
                ),
            )?;
        }
        line(&mut output, "")?;

        line(&mut output, "## Evidence-role compatibility")?;
        line(&mut output, "")?;
        line(
            &mut output,
            "Authority says who produced the evidence; role says what it may be read as.",
        )?;
        line(&mut output, "")?;
        line(
            &mut output,
            "| Proof class | Admitted roles | Roles the authority allows | Claim ceiling |",
        )?;
        line(&mut output, "| --- | --- | --- | --- |")?;
        for proof_class in &normalized.proof_classes {
            let admitted = proof_class
                .evidence_roles
                .iter()
                .map(|role| format!("`{}`", role.stable_name()))
                .collect::<Vec<_>>()
                .join(", ");
            let allowed = authority_admissible_roles(proof_class.authority)
                .iter()
                .map(|role| format!("`{}`", role.stable_name()))
                .collect::<Vec<_>>()
                .join(", ");
            line(
                &mut output,
                &format!(
                    "| `{}` | {} | {} | {} |",
                    proof_class.class_id,
                    admitted,
                    allowed,
                    proof_class.multi_role_claim_ceiling.as_deref().unwrap_or("—")
                ),
            )?;
        }
        line(&mut output, "")?;

        line(&mut output, "## Composition dimensions")?;
        line(&mut output, "")?;
        line(&mut output, "| Dimension | Values | Owner | Claim boundary |")?;
        line(&mut output, "| --- | --- | --- | --- |")?;
        for dimension in &normalized.dimensions {
            let values = dimension
                .values
                .iter()
                .map(|value| format!("`{value}`"))
                .collect::<Vec<_>>()
                .join(", ");
            line(
                &mut output,
                &format!(
                    "| `{}` | {} | {} | {} |",
                    dimension.dimension_id, values, dimension.owner_issue, dimension.claim_boundary
                ),
            )?;
        }
        line(&mut output, "")?;

        line(&mut output, "## Campaigns")?;
        line(&mut output, "")?;
        line(&mut output, "| Campaign | Concept families | Dimensions | Proof classes | Owner |")?;
        line(&mut output, "| --- | --- | --- | --- | --- |")?;
        for campaign in &normalized.campaigns {
            line(
                &mut output,
                &format!(
                    "| `{}` | {} | {} | {} | {} |",
                    campaign.campaign_id,
                    code_list(&campaign.concept_families),
                    code_list(&campaign.dimensions),
                    code_list(&campaign.proof_classes),
                    campaign.owner_issue
                ),
            )?;
        }
        line(&mut output, "")?;

        line(&mut output, "## Reverse coverage")?;
        line(&mut output, "")?;
        line(
            &mut output,
            "Every declared item is exercised by a campaign or retained under a typed disposition.",
        )?;
        line(&mut output, "")?;
        line(&mut output, "| Vocabulary | Declared | Exercised | Dispositioned |")?;
        line(&mut output, "| --- | ---: | ---: | ---: |")?;
        let dispositioned = normalized.dispositioned_ids();
        let ledger_families = concept_family_index(concepts)?;
        for (kind, declared, exercised) in [
            (
                ItemKind::ProofClass,
                normalized.proof_classes.len(),
                normalized
                    .campaigns
                    .iter()
                    .flat_map(|campaign| campaign.proof_classes.iter())
                    .collect::<BTreeSet<_>>()
                    .len(),
            ),
            (
                ItemKind::Dimension,
                normalized.dimensions.len(),
                normalized
                    .campaigns
                    .iter()
                    .flat_map(|campaign| campaign.dimensions.iter())
                    .collect::<BTreeSet<_>>()
                    .len(),
            ),
            (
                ItemKind::ConceptFamily,
                ledger_families.len(),
                normalized
                    .campaigns
                    .iter()
                    .flat_map(|campaign| campaign.concept_families.iter())
                    .collect::<BTreeSet<_>>()
                    .len(),
            ),
        ] {
            let retained = dispositioned.get(&kind).map_or(0, BTreeSet::len);
            line(
                &mut output,
                &format!("| `{}` | {declared} | {exercised} | {retained} |", kind.stable_name()),
            )?;
        }
        line(&mut output, "")?;

        line(&mut output, "## Retained dispositions")?;
        line(&mut output, "")?;
        if normalized.dispositions.is_empty() {
            line(
                &mut output,
                "None. Every declared item is exercised, so the policy retains no unresolved obligation.",
            )?;
        } else {
            line(
                &mut output,
                "| Item | Kind | Status | Basis | Owner | Review after | Exit condition | Claim effect | Reason |",
            )?;
            line(&mut output, "| --- | --- | --- | --- | --- | --- | --- | --- | --- |")?;
            for disposition in &normalized.dispositions {
                line(
                    &mut output,
                    &format!(
                        "| `{}` | `{}` | `{}` | `{}` | {} | `{}` | {} | `{}` | {} |",
                        disposition.item_id,
                        disposition.item_kind.stable_name(),
                        disposition.status.stable_name(),
                        disposition.basis.stable_name(),
                        disposition.owner_issue,
                        disposition.review_after,
                        disposition.exit_condition,
                        disposition.claim_effect.stable_name(),
                        disposition.reason
                    ),
                )?;
            }
        }
        line(&mut output, "")?;

        line(&mut output, "## Coverage counts")?;
        line(&mut output, "")?;
        line(&mut output, "| Item | Count |")?;
        line(&mut output, "| --- | ---: |")?;
        let mut stage_counts = BTreeMap::<&str, usize>::new();
        for proof_class in &normalized.proof_classes {
            for stage in &proof_class.claim_stages {
                *stage_counts.entry(stage.stable_name()).or_default() += 1;
            }
        }
        for (stage, count) in stage_counts {
            line(&mut output, &format!("| Proof classes applying to `{stage}` | {count} |"))?;
        }
        let referenced_families = normalized
            .campaigns
            .iter()
            .flat_map(|campaign| campaign.concept_families.iter())
            .collect::<BTreeSet<_>>()
            .len();
        line(
            &mut output,
            &format!("| Distinct concept families in campaigns | {referenced_families} |"),
        )?;
        line(
            &mut output,
            &format!("| Concept rows in consumed seed | {} |", concepts.concepts.len()),
        )?;
        Ok(output)
    }
}

impl ProofClass {
    fn validate(&self) -> Result<()> {
        validate_id("proof class", &self.class_id)?;
        let contract = proof_class_contract(&self.class_id)?;
        if self.purpose.trim().is_empty() {
            bail!("proof class {} has an empty purpose", self.class_id);
        }
        validate_issue("owner_issue", &self.owner_issue)?;
        validate_unique("claim_stages", &self.class_id, &self.claim_stages)?;
        if self.claim_stages.is_empty() {
            bail!("proof class {} must name at least one claim stage", self.class_id);
        }
        let actual_stages = self.claim_stages.iter().copied().collect::<BTreeSet<_>>();
        let expected_stages = contract.claim_stages.iter().copied().collect::<BTreeSet<_>>();
        if self.authority != contract.authority {
            bail!(
                "proof class {} has authority {:?}; expected {:?}",
                self.class_id,
                self.authority,
                contract.authority
            );
        }
        if actual_stages != expected_stages {
            bail!(
                "proof class {} has claim stages {:?}; expected {:?}",
                self.class_id,
                actual_stages,
                expected_stages
            );
        }
        if self.circular_output_allowed != contract.circular_output_allowed {
            bail!(
                "proof class {} has circular_output_allowed={}; expected {}",
                self.class_id,
                self.circular_output_allowed,
                contract.circular_output_allowed
            );
        }
        if self.missing_effect != contract.missing_effect {
            bail!(
                "proof class {} has missing effect {:?}; expected {:?}",
                self.class_id,
                self.missing_effect,
                contract.missing_effect
            );
        }
        self.validate_evidence_roles(&contract)
    }

    /// Enforce the evidence-role compatibility rule and its claim ceiling.
    ///
    /// Two independent guards, because they fail on different mistakes: the
    /// authority rule rejects a role the producer could never play, and the
    /// pinned set rejects a reviewed row drifting to a different one.
    fn validate_evidence_roles(&self, contract: &ProofClassContract) -> Result<()> {
        validate_unique("evidence_roles", &self.class_id, &self.evidence_roles)?;
        if self.evidence_roles.is_empty() {
            bail!("proof class {} must name at least one evidence role", self.class_id);
        }
        let admissible =
            authority_admissible_roles(self.authority).iter().copied().collect::<BTreeSet<_>>();
        for role in &self.evidence_roles {
            if !admissible.contains(role) {
                bail!(
                    "proof class {} admits evidence role {:?}, which authority {:?} cannot produce",
                    self.class_id,
                    role.stable_name(),
                    self.authority.stable_name()
                );
            }
        }
        let actual = self.evidence_roles.iter().copied().collect::<BTreeSet<_>>();
        let expected = contract.evidence_roles.iter().copied().collect::<BTreeSet<_>>();
        if actual != expected {
            bail!(
                "proof class {} has evidence roles {:?}; expected {:?}",
                self.class_id,
                stable_names(&actual),
                stable_names(&expected)
            );
        }
        match (&self.multi_role_claim_ceiling, self.evidence_roles.len() > 1) {
            (Some(ceiling), true) if ceiling.trim().is_empty() => {
                bail!(
                    "proof class {} admits several evidence roles and needs a non-empty multi_role_claim_ceiling",
                    self.class_id
                );
            }
            (None, true) => {
                bail!(
                    "proof class {} admits evidence roles {:?} and must declare a multi_role_claim_ceiling",
                    self.class_id,
                    stable_names(&actual)
                );
            }
            (Some(_), false) => {
                bail!(
                    "proof class {} admits one evidence role and must not declare a multi_role_claim_ceiling",
                    self.class_id
                );
            }
            _ => Ok(()),
        }
    }
}

fn stable_names<T: StableName + Copy>(values: &BTreeSet<T>) -> Vec<&'static str> {
    values.iter().map(|value| value.stable_name()).collect()
}

impl Dimension {
    fn validate(&self) -> Result<()> {
        validate_id("dimension", &self.dimension_id)?;
        validate_issue("owner_issue", &self.owner_issue)?;
        if self.claim_boundary.trim().is_empty() {
            bail!("dimension {} has an empty claim boundary", self.dimension_id);
        }
        validate_unique("values", &self.dimension_id, &self.values)?;
        if self.values.len() < 2 {
            bail!("dimension {} must contain at least two values", self.dimension_id);
        }
        for value in &self.values {
            validate_id("dimension value", value)?;
        }
        Ok(())
    }
}

impl Campaign {
    fn validate(
        &self,
        proof_classes: &BTreeSet<&str>,
        dimensions: &BTreeSet<&str>,
        concept_families: &BTreeSet<&str>,
    ) -> Result<()> {
        validate_id("campaign", &self.campaign_id)?;
        validate_issue("owner_issue", &self.owner_issue)?;
        if self.claim_boundary.trim().is_empty() {
            bail!("campaign {} has an empty claim boundary", self.campaign_id);
        }
        validate_unique("concept_families", &self.campaign_id, &self.concept_families)?;
        validate_unique("dimensions", &self.campaign_id, &self.dimensions)?;
        validate_unique("proof_classes", &self.campaign_id, &self.proof_classes)?;
        if self.concept_families.len() < 2
            || self.dimensions.len() < 2
            || self.proof_classes.is_empty()
        {
            bail!(
                "campaign {} needs at least two concept families, at least two dimensions, and proof classes",
                self.campaign_id
            );
        }
        for family in &self.concept_families {
            if !concept_families.contains(family.as_str()) {
                bail!(
                    "campaign {} references unknown concept family {:?}",
                    self.campaign_id,
                    family
                );
            }
        }
        for dimension in &self.dimensions {
            if !dimensions.contains(dimension.as_str()) {
                bail!("campaign {} references unknown dimension {:?}", self.campaign_id, dimension);
            }
        }
        for proof_class in &self.proof_classes {
            if !proof_classes.contains(proof_class.as_str()) {
                bail!(
                    "campaign {} references unknown proof class {:?}",
                    self.campaign_id,
                    proof_class
                );
            }
        }
        if !self.proof_classes.iter().any(|value| value == "composition_coverage") {
            bail!("campaign {} must include composition_coverage", self.campaign_id);
        }
        let distinct_families =
            self.concept_families.iter().map(|family| family_key(family)).collect::<BTreeSet<_>>();
        if distinct_families.len() < 2 {
            bail!(
                "campaign {} names {} concept families that resolve to one family; an alias cannot satisfy the cross-family requirement",
                self.campaign_id,
                self.concept_families.len()
            );
        }
        Ok(())
    }

    fn members(&self) -> CampaignMembers {
        CampaignMembers {
            concept_families: self.concept_families.iter().cloned().collect(),
            dimensions: self.dimensions.iter().cloned().collect(),
            proof_classes: self.proof_classes.iter().cloned().collect(),
        }
    }
}

/// The exact member sets that make a campaign a distinct proof obligation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CampaignMembers {
    concept_families: BTreeSet<String>,
    dimensions: BTreeSet<String>,
    proof_classes: BTreeSet<String>,
}

impl Disposition {
    fn validate(
        &self,
        proof_classes: &BTreeSet<&str>,
        dimensions: &BTreeSet<&str>,
        concept_families: &BTreeSet<&str>,
    ) -> Result<()> {
        validate_id("disposition item", &self.item_id)?;
        validate_issue("owner_issue", &self.owner_issue)?;
        for (name, value) in
            [("reason", self.reason.as_str()), ("exit_condition", self.exit_condition.as_str())]
        {
            if value.trim().is_empty() {
                bail!(
                    "disposition for {} {:?} has an empty {name}",
                    self.item_kind.stable_name(),
                    self.item_id
                );
            }
        }
        parse_date("review_after", &self.review_after)?;
        let declared = match self.item_kind {
            ItemKind::ProofClass => proof_classes,
            ItemKind::Dimension => dimensions,
            ItemKind::ConceptFamily => concept_families,
        };
        if !declared.contains(self.item_id.as_str()) {
            bail!("disposition names unknown {} {:?}", self.item_kind.stable_name(), self.item_id);
        }
        Ok(())
    }
}

/// Collapse the spellings that would let one concept family appear twice.
fn family_key(family: &str) -> String {
    family.replace('-', "_")
}

/// Index the ledger families, refusing a ledger that presents one family twice.
fn concept_family_index(concepts: &ConceptLedgerIndex) -> Result<BTreeSet<&str>> {
    let mut by_key = BTreeMap::<String, &str>::new();
    for concept in &concepts.concepts {
        let family = concept.family.as_str();
        if let Some(existing) = by_key.insert(family_key(family), family)
            && existing != family
        {
            bail!(
                "concept ledger presents {existing:?} and {family:?} as separate families, but they are aliases"
            );
        }
    }
    Ok(concepts.concepts.iter().map(|concept| concept.family.as_str()).collect())
}

/// Digest only the ledger facts this policy consumes.
///
/// Hashing the whole ledger would churn this status on every unrelated concept
/// edit; the load-bearing projection is the schema and the family set.
fn concept_projection_digest(concepts: &ConceptLedgerIndex) -> String {
    let families = concepts
        .concepts
        .iter()
        .map(|concept| concept.family.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(",");
    digest(&format!("{}|{}|{}", concepts.schema_version, concepts.concepts.len(), families))
}

fn parse_date(name: &str, value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .with_context(|| format!("{name} must be an ISO-8601 date like 2026-12-01; got {value:?}"))
}

fn digest(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn main() -> Result<()> {
    color_eyre::install().map_err(|error| anyhow!("install diagnostics: {error}"))?;
    let cli = Cli::parse();
    if cli.check && cli.write_status {
        bail!("--check and --write-status are mutually exclusive");
    }

    let as_of = match cli.as_of.as_deref() {
        Some(value) => parse_date("--as-of", value)?,
        None => Utc::now().date_naive(),
    };

    let policy = ProofPolicy::load(&cli.policy)?;
    let concepts = load_concepts(&cli.concept_ledger)?;
    let rendered = policy.render_markdown(&concepts)?;
    policy.check_currentness(as_of)?;

    if cli.write_status {
        write_status(&cli.status, &rendered)?;
        println!("wrote {}", cli.status.display());
        return Ok(());
    }
    if cli.check {
        let current = fs::read_to_string(&cli.status)
            .with_context(|| format!("read generated status {}", cli.status.display()))?;
        if current != rendered {
            bail!(
                "generated compiler proof policy status is stale: run `cargo run -p xtask --bin compiler-proof-policy -- --write-status`"
            );
        }
        println!(
            "compiler proof policy valid: {} classes, {} dimensions, {} campaigns, {} retained dispositions, closed={}",
            policy.proof_classes.len(),
            policy.dimensions.len(),
            policy.campaigns.len(),
            policy.dispositions.len(),
            policy.derived_complete()
        );
        return Ok(());
    }

    print!("{rendered}");
    Ok(())
}

fn load_concepts(path: &Path) -> Result<ConceptLedgerIndex> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("read compiler concept ledger {}", path.display()))?;
    toml::from_str(&source)
        .with_context(|| format!("parse compiler concept ledger index {}", path.display()))
}

fn write_status(path: &Path, rendered: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create status directory {}", parent.display()))?;
    }
    fs::write(path, rendered).with_context(|| format!("write status {}", path.display()))
}

fn code_list(values: &[String]) -> String {
    values.iter().map(|value| format!("`{value}`")).collect::<Vec<_>>().join(", ")
}

fn line(output: &mut String, value: &str) -> Result<()> {
    writeln!(output, "{value}").map_err(|_| anyhow!("render compiler proof policy status"))
}

fn validate_issue(name: &str, value: &str) -> Result<()> {
    let digits = value.strip_prefix('#').unwrap_or_default();
    if digits.is_empty()
        || digits.starts_with('0')
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        bail!("{name} must be a GitHub issue reference like #6689; got {value:?}");
    }
    Ok(())
}

fn validate_closure_authority(value: &str, controller_issue: &str) -> Result<()> {
    let authority = value.strip_prefix("issue:").ok_or_else(|| {
        anyhow!("closure_authority must use issue:#<number>/<revision> syntax; got {value:?}")
    })?;
    let (issue, revision) = authority.split_once('/').ok_or_else(|| {
        anyhow!("closure_authority must use issue:#<number>/<revision> syntax; got {value:?}")
    })?;
    validate_issue("closure_authority issue", issue)?;
    if issue != controller_issue {
        bail!("closure_authority issue {issue:?} must match controller_issue {controller_issue:?}");
    }
    validate_id("closure authority revision", revision)
}

fn validate_id(kind: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.starts_with('-')
        || value.starts_with('_')
        || value.ends_with('-')
        || value.ends_with('_')
        || value.bytes().any(|byte| {
            !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-'))
        })
    {
        bail!("invalid {kind} id {value:?}");
    }
    Ok(())
}

fn validate_unique<T: Ord>(name: &str, owner: &str, values: &[T]) -> Result<()> {
    let unique = values.iter().collect::<BTreeSet<_>>();
    if unique.len() != values.len() {
        bail!("{owner} {name} contains duplicate entries");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: &str =
        include_str!("../../../contracts/compiler/perl_compiler_proof_policy.v1.toml");
    const CONCEPTS: &str =
        include_str!("../../../contracts/compiler/perl_compiler_concepts.v1.toml");
    const STATUS: &str = include_str!("../../../docs/project/status/perl_compiler_proof_policy.md");

    fn concepts() -> Result<ConceptLedgerIndex> {
        toml::from_str(CONCEPTS).context("parse committed concept ledger")
    }

    #[test]
    fn committed_policy_validates_and_status_is_current() -> Result<()> {
        let policy = ProofPolicy::from_str(POLICY)?;
        let concepts = concepts()?;
        policy.validate(&concepts)?;
        assert!(!policy.complete);
        assert_eq!(policy.render_markdown(&concepts)?, STATUS);
        Ok(())
    }

    #[test]
    fn unknown_campaign_dimension_fails_closed() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let campaign = policy
            .campaigns
            .first_mut()
            .ok_or_else(|| anyhow!("committed policy unexpectedly has no campaign"))?;
        campaign.dimensions.push("unknown_dimension".to_string());
        assert!(policy.validate(&concepts()?).is_err());
        Ok(())
    }

    #[test]
    fn independent_gold_cannot_be_circular() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let proof_class = policy
            .proof_classes
            .iter_mut()
            .find(|proof_class| matches!(proof_class.authority, Authority::IndependentGold))
            .ok_or_else(|| anyhow!("committed policy has no independent gold class"))?;
        proof_class.circular_output_allowed = true;
        assert!(policy.validate(&concepts()?).is_err());
        Ok(())
    }

    #[test]
    fn positive_gold_authority_mutation_fails_closed() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let proof_class = policy
            .proof_classes
            .iter_mut()
            .find(|proof_class| proof_class.class_id == "positive_gold")
            .ok_or_else(|| anyhow!("committed policy has no positive gold class"))?;
        proof_class.authority = Authority::CompilerSnapshot;
        assert!(policy.validate(&concepts()?).is_err());
        Ok(())
    }

    #[test]
    fn positive_gold_stage_mutation_fails_closed() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let proof_class = policy
            .proof_classes
            .iter_mut()
            .find(|proof_class| proof_class.class_id == "positive_gold")
            .ok_or_else(|| anyhow!("committed policy has no positive gold class"))?;
        proof_class.claim_stages = vec![ClaimStage::Provider];
        assert!(policy.validate(&concepts()?).is_err());
        Ok(())
    }

    #[test]
    fn positive_gold_missing_effect_mutation_fails_closed() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let proof_class = policy
            .proof_classes
            .iter_mut()
            .find(|proof_class| proof_class.class_id == "positive_gold")
            .ok_or_else(|| anyhow!("committed policy has no positive gold class"))?;
        proof_class.missing_effect = MissingEffect::BlocksStage;
        assert!(policy.validate(&concepts()?).is_err());
        Ok(())
    }

    #[test]
    fn unknown_concept_family_fails_closed() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let campaign = policy
            .campaigns
            .first_mut()
            .ok_or_else(|| anyhow!("committed policy unexpectedly has no campaign"))?;
        campaign.concept_families.push("unknown_family".to_string());
        assert!(policy.validate(&concepts()?).is_err());
        Ok(())
    }

    #[test]
    fn complete_policy_requires_controller_bound_closure_authority() -> Result<()> {
        let concepts = concepts()?;
        let mut policy = ProofPolicy::from_str(POLICY)?;
        policy.complete = true;
        assert!(policy.validate(&concepts).is_err());

        policy.closure_authority = Some("arbitrary".to_string());
        assert!(policy.validate(&concepts).is_err());

        policy.closure_authority = Some("issue:#6657/policy_closure_v1".to_string());
        assert!(policy.validate(&concepts).is_err());

        policy.closure_authority = Some("issue:#6689/policy_closure_v1".to_string());
        policy.validate(&concepts)?;
        Ok(())
    }

    #[test]
    fn incomplete_policy_rejects_stale_closure_authority() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        policy.closure_authority = Some("issue:#6689/policy_closure_v1".to_string());
        assert!(policy.validate(&concepts()?).is_err());
        Ok(())
    }

    #[test]
    fn single_family_campaign_fails_closed() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let campaign = policy
            .campaigns
            .first_mut()
            .ok_or_else(|| anyhow!("committed policy unexpectedly has no campaign"))?;
        campaign.concept_families.truncate(1);
        assert!(policy.validate(&concepts()?).is_err());
        Ok(())
    }

    #[test]
    fn orphaned_policy_vocabulary_fails_closed() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let mut dimension = policy
            .dimensions
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("committed policy unexpectedly has no dimension"))?;
        dimension.dimension_id = "unexercised_axis".to_string();
        dimension.values = vec!["first".to_string(), "second".to_string()];
        policy.dimensions.push(dimension);
        assert!(policy.validate(&concepts()?).is_err());

        let policy = ProofPolicy::from_str(POLICY)?;
        let mut concept_index = concepts()?;
        concept_index.concepts.push(ConceptIndexRow { family: "unexercised_family".to_string() });
        assert!(policy.validate(&concept_index).is_err());
        Ok(())
    }

    /// Mutate exactly one field of the committed `composition_coverage` class and
    /// return the resulting validation error.
    ///
    /// One field at a time is the point: a mutation that flips authority, stages,
    /// circularity, and missing-effect together passes as soon as *any* one of the
    /// four is guarded, which is precisely the gap that let compiler-generated
    /// output stand as its own composition proof.
    fn composition_mutation_error(mutate: impl FnOnce(&mut ProofClass)) -> Result<String> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let composition = policy
            .proof_classes
            .iter_mut()
            .find(|proof_class| proof_class.class_id == "composition_coverage")
            .ok_or_else(|| anyhow!("committed policy has no composition class"))?;
        mutate(composition);
        Ok(policy
            .validate(&concepts()?)
            .expect_err("composition_coverage mutation must fail closed")
            .to_string())
    }

    #[test]
    fn composition_coverage_authority_mutation_fails_closed() -> Result<()> {
        let error = composition_mutation_error(|class| {
            class.authority = Authority::CompilerSnapshot;
        })?;
        assert!(error.contains("composition_coverage"), "unexpected error: {error}");
        assert!(error.contains("authority"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn composition_coverage_circular_output_mutation_fails_closed() -> Result<()> {
        let error = composition_mutation_error(|class| {
            class.circular_output_allowed = true;
        })?;
        assert!(error.contains("composition_coverage"), "unexpected error: {error}");
        assert!(error.contains("circular"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn composition_coverage_stage_mutation_fails_closed() -> Result<()> {
        let error = composition_mutation_error(|class| {
            class.claim_stages = vec![ClaimStage::Provider];
        })?;
        assert!(error.contains("composition_coverage"), "unexpected error: {error}");
        assert!(error.contains("claim stages"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn composition_coverage_missing_effect_mutation_fails_closed() -> Result<()> {
        let error = composition_mutation_error(|class| {
            class.missing_effect = MissingEffect::BlocksStage;
        })?;
        assert!(error.contains("composition_coverage"), "unexpected error: {error}");
        assert!(error.contains("missing effect"), "unexpected error: {error}");
        Ok(())
    }

    /// A dimension no campaign names, plus the disposition that legitimately
    /// retains it. Callers mutate one field to prove each guard.
    fn retained_dimension(policy: &mut ProofPolicy) -> Disposition {
        policy.dimensions.push(Dimension {
            dimension_id: "retained_axis".to_string(),
            values: vec!["first".to_string(), "second".to_string()],
            owner_issue: "#6689".to_string(),
            claim_boundary: "Retained pending its owning campaign.".to_string(),
        });
        Disposition {
            item_kind: ItemKind::Dimension,
            item_id: "retained_axis".to_string(),
            status: DispositionStatus::SequencingBlocked,
            reason: "The owning campaign lands with its concept slice.".to_string(),
            owner_issue: "#6689".to_string(),
            basis: DispositionBasis::Sequencing,
            review_after: "2099-01-01".to_string(),
            exit_condition: "A campaign names retained_axis.".to_string(),
            claim_effect: ClaimEffect::ExcludedFromClaims,
        }
    }

    fn as_of(value: &str) -> Result<NaiveDate> {
        parse_date("as_of", value)
    }

    #[test]
    fn unexercised_proof_class_fails_closed() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        for campaign in &mut policy.campaigns {
            campaign.proof_classes.retain(|class| class != "recovery_gold");
        }
        let error = policy
            .validate(&concepts()?)
            .expect_err("an unexercised proof class must fail closed")
            .to_string();
        assert!(error.contains("recovery_gold"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn retained_vocabulary_needs_a_typed_disposition() -> Result<()> {
        let concepts = concepts()?;

        // Unexercised and undispositioned is the closed-policy failure.
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let disposition = retained_dimension(&mut policy);
        let error = policy
            .validate(&concepts)
            .expect_err("retained vocabulary without a disposition must fail closed")
            .to_string();
        assert!(error.contains("retained_axis"), "unexpected error: {error}");

        // A complete, owned, current disposition is the honest representation.
        policy.dispositions.push(disposition);
        policy.validate(&concepts)?;
        policy.check_currentness(as_of("2026-09-13")?)?;
        Ok(())
    }

    #[test]
    fn ownerless_disposition_fails_closed() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let mut disposition = retained_dimension(&mut policy);
        disposition.owner_issue = String::new();
        policy.dispositions.push(disposition);
        let error = policy
            .validate(&concepts()?)
            .expect_err("an ownerless disposition must fail closed")
            .to_string();
        assert!(error.contains("owner_issue"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn expired_disposition_fails_closed_without_rendering_a_date() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let mut disposition = retained_dimension(&mut policy);
        disposition.review_after = "2026-01-01".to_string();
        policy.dispositions.push(disposition);
        let concepts = concepts()?;

        // Structural validation stays date-independent, so the projection the
        // gate compares against is deterministic: the row and its own
        // review_after are visible, but no evaluation date is baked in...
        policy.validate(&concepts)?;
        let rendered = policy.render_markdown(&concepts)?;
        assert!(rendered.contains("retained_axis"), "the retained row must stay visible");
        assert!(rendered.contains("2026-01-01"), "the row must carry its own review date");

        // ...but the expired row is still a command failure, so regenerating
        // the status cannot return the policy to green.
        let error = policy
            .check_currentness(as_of("2026-09-13")?)
            .expect_err("an expired disposition must fail closed")
            .to_string();
        assert!(error.contains("expired"), "unexpected error: {error}");
        policy.check_currentness(as_of("2025-12-31")?)?;
        Ok(())
    }

    #[test]
    fn disposition_cannot_contradict_campaign_coverage() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        policy.dispositions.push(Disposition {
            item_kind: ItemKind::ProofClass,
            item_id: "positive_gold".to_string(),
            status: DispositionStatus::DeferredToOwner,
            reason: "Claims an exercised class is retained.".to_string(),
            owner_issue: "#6689".to_string(),
            basis: DispositionBasis::Semantic,
            review_after: "2099-01-01".to_string(),
            exit_condition: "Never; the class is already exercised.".to_string(),
            claim_effect: ClaimEffect::ExcludedFromClaims,
        });
        let error = policy
            .validate(&concepts()?)
            .expect_err("dispositioning an exercised item must fail closed")
            .to_string();
        assert!(error.contains("contradict"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn disposition_naming_unknown_vocabulary_fails_closed() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let mut disposition = retained_dimension(&mut policy);
        disposition.item_id = "no_such_axis".to_string();
        policy.dispositions.push(disposition);
        let error = policy
            .validate(&concepts()?)
            .expect_err("a disposition for unknown vocabulary must fail closed")
            .to_string();
        assert!(error.contains("no_such_axis"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn closure_cannot_be_declared_while_a_disposition_is_retained() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let disposition = retained_dimension(&mut policy);
        policy.dispositions.push(disposition);
        policy.complete = true;
        policy.closure_authority = Some("issue:#6689/policy_closure_v1".to_string());
        let error = policy
            .validate(&concepts()?)
            .expect_err("a retained disposition must keep the policy open")
            .to_string();
        assert!(error.contains("derives complete=false"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn independent_gold_cannot_admit_compiler_output_or_execution_receipts() -> Result<()> {
        for intruder in [EvidenceRole::CompilerGeneratedSnapshot, EvidenceRole::ExecutionReceipt] {
            let mut policy = ProofPolicy::from_str(POLICY)?;
            let proof_class = policy
                .proof_classes
                .iter_mut()
                .find(|proof_class| proof_class.class_id == "positive_gold")
                .ok_or_else(|| anyhow!("committed policy has no positive gold class"))?;
            proof_class.evidence_roles.push(intruder);
            proof_class.multi_role_claim_ceiling = Some("Treated as equivalent.".to_string());
            let error = policy
                .validate(&concepts()?)
                .expect_err("independent gold must not admit this role")
                .to_string();
            assert!(error.contains("positive_gold"), "unexpected error: {error}");
            assert!(error.contains(intruder.stable_name()), "unexpected error: {error}");
        }
        Ok(())
    }

    #[test]
    fn multi_role_class_requires_an_explicit_claim_ceiling() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let proof_class = policy
            .proof_classes
            .iter_mut()
            .find(|proof_class| proof_class.class_id == "real_perl_oracle")
            .ok_or_else(|| anyhow!("committed policy has no real Perl oracle class"))?;
        proof_class.multi_role_claim_ceiling = None;
        let error = policy
            .validate(&concepts()?)
            .expect_err("a multi-role class without a ceiling must fail closed")
            .to_string();
        assert!(error.contains("multi_role_claim_ceiling"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn single_role_class_rejects_a_claim_ceiling() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let proof_class = policy
            .proof_classes
            .iter_mut()
            .find(|proof_class| proof_class.class_id == "positive_gold")
            .ok_or_else(|| anyhow!("committed policy has no positive gold class"))?;
        proof_class.multi_role_claim_ceiling = Some("Unnecessary ceiling.".to_string());
        assert!(policy.validate(&concepts()?).is_err());
        Ok(())
    }

    #[test]
    fn evidence_role_drift_from_the_reviewed_set_fails_closed() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let proof_class = policy
            .proof_classes
            .iter_mut()
            .find(|proof_class| proof_class.class_id == "hir_snapshot")
            .ok_or_else(|| anyhow!("committed policy has no hir snapshot class"))?;
        proof_class.evidence_roles = vec![EvidenceRole::ImplementationDiagnosticReceipt];
        let error = policy
            .validate(&concepts()?)
            .expect_err("silently retyping a reviewed role must fail closed")
            .to_string();
        assert!(error.contains("evidence roles"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn aliased_concept_families_cannot_be_two_families() -> Result<()> {
        let policy = ProofPolicy::from_str(POLICY)?;
        let mut concept_index = concepts()?;
        concept_index.concepts.push(ConceptIndexRow { family: "place-s".to_string() });
        concept_index.concepts.push(ConceptIndexRow { family: "place_s".to_string() });
        let error = policy
            .validate(&concept_index)
            .expect_err("aliased ledger families must fail closed")
            .to_string();
        assert!(error.contains("aliases"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn campaigns_cannot_share_an_exact_member_set() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let mut duplicate = policy
            .campaigns
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("committed policy unexpectedly has no campaign"))?;
        duplicate.campaign_id = "renamed_but_identical".to_string();
        policy.campaigns.push(duplicate);
        let error = policy
            .validate(&concepts()?)
            .expect_err("two campaigns with one member set must fail closed")
            .to_string();
        assert!(error.contains("same exact member set"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn unrendered_policy_fields_still_move_the_status_digest() -> Result<()> {
        let concepts = concepts()?;
        let policy = ProofPolicy::from_str(POLICY)?;
        let baseline = policy.render_markdown(&concepts)?;

        // `claim_boundary` and `purpose` carry policy meaning but appear in no
        // rendered table, so before the digest an edit to either left the
        // generated status byte-identical and the gate green.
        let mut edited = policy.clone();
        let campaign = edited
            .campaigns
            .first_mut()
            .ok_or_else(|| anyhow!("committed policy unexpectedly has no campaign"))?;
        campaign.claim_boundary = "Materially different boundary.".to_string();
        assert_ne!(edited.render_markdown(&concepts)?, baseline);

        let mut edited = policy.clone();
        let proof_class = edited
            .proof_classes
            .first_mut()
            .ok_or_else(|| anyhow!("committed policy unexpectedly has no proof class"))?;
        proof_class.purpose = "Materially different purpose.".to_string();
        assert_ne!(edited.render_markdown(&concepts)?, baseline);
        Ok(())
    }

    #[test]
    fn concept_projection_change_moves_the_consumed_digest() -> Result<()> {
        let baseline = concept_projection_digest(&concepts()?);
        let mut concept_index = concepts()?;
        concept_index.concepts.push(ConceptIndexRow { family: "calls".to_string() });
        assert_ne!(concept_projection_digest(&concept_index), baseline);
        Ok(())
    }

    #[test]
    fn rendering_is_independent_of_input_order() -> Result<()> {
        let policy = ProofPolicy::from_str(POLICY)?;
        let concepts = concepts()?;
        let expected = policy.render_markdown(&concepts)?;
        let mut reversed = policy.clone();
        reversed.proof_classes.reverse();
        reversed.dimensions.reverse();
        reversed.campaigns.reverse();
        assert_eq!(reversed.render_markdown(&concepts)?, expected);
        Ok(())
    }
}
