//! Validate and render the compiler proof/composition policy from #6689.
//!
//! ```text
//! cargo run -p xtask --bin compiler-proof-policy -- --check
//! cargo run -p xtask --bin compiler-proof-policy -- --write-status
//! ```

#![allow(clippy::print_stdout)]

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Copy)]
struct ProofClassContract {
    authority: Authority,
    claim_stages: &'static [ClaimStage],
    circular_output_allowed: bool,
    missing_effect: MissingEffect,
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
        },
        "recovery_gold" => ProofClassContract {
            authority: Authority::IndependentGold,
            claim_stages: &[ClaimStage::Parser, ClaimStage::BodyHir, ClaimStage::Provider],
            circular_output_allowed: false,
            missing_effect: MissingEffect::BlocksClaim,
        },
        "hir_snapshot" => ProofClassContract {
            authority: Authority::CompilerSnapshot,
            claim_stages: &[ClaimStage::FlatHir, ClaimStage::BodyHir],
            circular_output_allowed: true,
            missing_effect: MissingEffect::BlocksStage,
        },
        "pir_snapshot" => ProofClassContract {
            authority: Authority::CompilerSnapshot,
            claim_stages: &[ClaimStage::PirA],
            circular_output_allowed: true,
            missing_effect: MissingEffect::BlocksStage,
        },
        "verifier_mutation" => ProofClassContract {
            authority: Authority::MutationFixture,
            claim_stages: &[ClaimStage::PirA, ClaimStage::Eir, ClaimStage::Provider],
            circular_output_allowed: false,
            missing_effect: MissingEffect::BlocksClaim,
        },
        "effects_world_fixture" => ProofClassContract {
            authority: Authority::IndependentFixture,
            claim_stages: &[ClaimStage::EffectsWorld, ClaimStage::Provider],
            circular_output_allowed: false,
            missing_effect: MissingEffect::BlocksClaim,
        },
        "eir_differential" => ProofClassContract {
            authority: Authority::EirDifferential,
            claim_stages: &[ClaimStage::Eir],
            circular_output_allowed: false,
            missing_effect: MissingEffect::BlocksExecutionClaim,
        },
        "real_perl_oracle" => ProofClassContract {
            authority: Authority::RealPerlOracle,
            claim_stages: &[ClaimStage::EffectsWorld, ClaimStage::Eir, ClaimStage::Provider],
            circular_output_allowed: false,
            missing_effect: MissingEffect::BlocksClaimWhenObservable,
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
        match self.closure_authority.as_deref() {
            Some(authority) if !self.complete => {
                bail!(
                    "incomplete compiler proof policy must not carry closure_authority {authority:?}"
                );
            }
            Some(authority) => {
                validate_closure_authority(authority, &self.controller_issue)?;
            }
            None if self.complete => {
                bail!("complete compiler proof policy requires closure_authority");
            }
            None => {}
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

        let concept_families = concepts
            .concepts
            .iter()
            .map(|concept| concept.family.as_str())
            .collect::<BTreeSet<_>>();
        let mut campaign_ids = BTreeSet::new();
        for campaign in &self.campaigns {
            campaign.validate(&class_ids, &dimension_ids, &concept_families)?;
            if !campaign_ids.insert(campaign.campaign_id.as_str()) {
                bail!("duplicate proof campaign {:?}", campaign.campaign_id);
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
        if referenced_classes != class_ids {
            bail!(
                "proof policy has unexercised proof classes: {:?}",
                class_ids.difference(&referenced_classes).copied().collect::<Vec<_>>()
            );
        }
        if referenced_dimensions != dimension_ids {
            bail!(
                "proof policy has unexercised dimensions: {:?}",
                dimension_ids.difference(&referenced_dimensions).copied().collect::<Vec<_>>()
            );
        }
        if referenced_families != concept_families {
            bail!(
                "proof policy has unexercised concept families: {:?}",
                concept_families.difference(&referenced_families).copied().collect::<Vec<_>>()
            );
        }
        Ok(())
    }

    fn canonicalized(&self) -> Self {
        let mut normalized = self.clone();
        normalized.proof_classes.sort_by(|left, right| left.class_id.cmp(&right.class_id));
        normalized.dimensions.sort_by(|left, right| left.dimension_id.cmp(&right.dimension_id));
        normalized.campaigns.sort_by(|left, right| left.campaign_id.cmp(&right.campaign_id));
        for proof_class in &mut normalized.proof_classes {
            proof_class.claim_stages.sort();
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
        line(&mut output, &format!("- Policy vocabulary closed: `{}`", normalized.complete))?;
        if let Some(authority) = &normalized.closure_authority {
            line(&mut output, &format!("- Closure authority: `{authority}`"))?;
        } else {
            line(&mut output, "- Closure authority: —")?;
        }
        line(&mut output, &format!("- Proof classes: `{}`", normalized.proof_classes.len()))?;
        line(&mut output, &format!("- Composition dimensions: `{}`", normalized.dimensions.len()))?;
        line(&mut output, &format!("- Campaigns: `{}`", normalized.campaigns.len()))?;
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
        Ok(())
    }
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
        Ok(())
    }
}

fn main() -> Result<()> {
    color_eyre::install().map_err(|error| anyhow!("install diagnostics: {error}"))?;
    let cli = Cli::parse();
    if cli.check && cli.write_status {
        bail!("--check and --write-status are mutually exclusive");
    }

    let policy = ProofPolicy::load(&cli.policy)?;
    let concepts = load_concepts(&cli.concept_ledger)?;
    let rendered = policy.render_markdown(&concepts)?;

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
            "compiler proof policy valid: {} classes, {} dimensions, {} campaigns",
            policy.proof_classes.len(),
            policy.dimensions.len(),
            policy.campaigns.len()
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

    #[test]
    fn composition_coverage_contract_is_canonical() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let composition = policy
            .proof_classes
            .iter_mut()
            .find(|proof_class| proof_class.class_id == "composition_coverage")
            .ok_or_else(|| anyhow!("committed policy has no composition class"))?;
        composition.authority = Authority::CompilerSnapshot;
        composition.circular_output_allowed = true;
        composition.claim_stages = vec![ClaimStage::Provider];
        composition.missing_effect = MissingEffect::BlocksStage;
        assert!(policy.validate(&concepts()?).is_err());
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
