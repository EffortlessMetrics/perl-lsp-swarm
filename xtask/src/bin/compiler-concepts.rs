//! Standalone validator and deterministic status generator for #6657.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p xtask --bin compiler-concepts -- --check
//! cargo run -p xtask --bin compiler-concepts -- --write-status
//! ```

#![allow(clippy::print_stdout)]

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: &str = "perl_compiler_concepts.v1";
const DEFAULT_LEDGER: &str = "contracts/compiler/perl_compiler_concepts.v1.toml";
const DEFAULT_STATUS: &str = "docs/project/status/perl_compiler_concepts.md";

#[derive(Debug, Parser)]
#[command(name = "compiler-concepts")]
#[command(about = "Validate and render the canonical Perl compiler concept ledger")]
struct Cli {
    /// Machine-readable compiler concept ledger.
    #[arg(long, default_value = DEFAULT_LEDGER)]
    ledger: PathBuf,

    /// Generated Markdown status path.
    #[arg(long, default_value = DEFAULT_STATUS)]
    status: PathBuf,

    /// Validate the ledger and fail when the checked-in status is stale.
    #[arg(long)]
    check: bool,

    /// Validate the ledger and rewrite the generated status.
    #[arg(long)]
    write_status: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ConceptLedger {
    schema_version: String,
    ledger_id: String,
    controller_issue: String,
    coverage_scope: String,
    claim_boundary: String,
    complete: bool,
    #[serde(default)]
    inventory_authority: Option<String>,
    concepts: Vec<ConceptRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ConceptRow {
    concept_id: String,
    family: String,
    title: String,
    ast_kinds: Vec<String>,
    parser_ast: ParserState,
    flat_hir: RepresentationState,
    body_hir: RepresentationState,
    pir_a: RepresentationState,
    compile_effects_world: RepresentationState,
    eir_profile: EirState,
    gold: ProofState,
    oracle: ProofState,
    composition: ProofState,
    provider_eligibility: ProviderEligibility,
    ownership: OwnershipState,
    owner_issue: String,
    claim_boundary: String,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum ParserState {
    Parsed,
    Recovered,
    Absent,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum RepresentationState {
    Modeled,
    MigrationAdapter,
    Bridge,
    Opaque,
    Boundary,
    Skipped,
    Absent,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum EirState {
    Executable,
    Boundary,
    Absent,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum ProofState {
    Proven,
    Missing,
    NotObservable,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum ProviderEligibility {
    Exact,
    Qualified,
    FallbackOnly,
    Ineligible,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum OwnershipState {
    Active,
    Deferred,
    Boundary,
    NotApplicable,
}

trait StableName {
    fn stable_name(self) -> &'static str;
}

impl StableName for ParserState {
    fn stable_name(self) -> &'static str {
        match self {
            Self::Parsed => "parsed",
            Self::Recovered => "recovered",
            Self::Absent => "absent",
            Self::NotApplicable => "not_applicable",
        }
    }
}

impl StableName for RepresentationState {
    fn stable_name(self) -> &'static str {
        match self {
            Self::Modeled => "modeled",
            Self::MigrationAdapter => "migration_adapter",
            Self::Bridge => "bridge",
            Self::Opaque => "opaque",
            Self::Boundary => "boundary",
            Self::Skipped => "skipped",
            Self::Absent => "absent",
            Self::NotApplicable => "not_applicable",
        }
    }
}

impl StableName for EirState {
    fn stable_name(self) -> &'static str {
        match self {
            Self::Executable => "executable",
            Self::Boundary => "boundary",
            Self::Absent => "absent",
            Self::NotApplicable => "not_applicable",
        }
    }
}

impl StableName for ProofState {
    fn stable_name(self) -> &'static str {
        match self {
            Self::Proven => "proven",
            Self::Missing => "missing",
            Self::NotObservable => "not_observable",
            Self::NotApplicable => "not_applicable",
        }
    }
}

impl StableName for ProviderEligibility {
    fn stable_name(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Qualified => "qualified",
            Self::FallbackOnly => "fallback_only",
            Self::Ineligible => "ineligible",
        }
    }
}

impl StableName for OwnershipState {
    fn stable_name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Deferred => "deferred",
            Self::Boundary => "boundary",
            Self::NotApplicable => "not_applicable",
        }
    }
}

impl ConceptLedger {
    fn from_str(source: &str) -> Result<Self> {
        let ledger: Self = toml::from_str(source).context("parse compiler concept ledger")?;
        ledger.validate()?;
        Ok(ledger)
    }

    fn load(path: &Path) -> Result<Self> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("read compiler concept ledger {}", path.display()))?;
        Self::from_str(&source)
            .with_context(|| format!("validate compiler concept ledger {}", path.display()))
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            bail!(
                "unsupported compiler concept schema {:?}; expected {:?}",
                self.schema_version,
                SCHEMA_VERSION
            );
        }

        for (name, value) in [
            ("ledger_id", self.ledger_id.as_str()),
            ("controller_issue", self.controller_issue.as_str()),
            ("coverage_scope", self.coverage_scope.as_str()),
            ("claim_boundary", self.claim_boundary.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("compiler concept ledger field {name} must not be empty");
            }
        }

        validate_issue("controller_issue", &self.controller_issue)?;

        match self.inventory_authority.as_deref() {
            Some(authority) if authority.trim().is_empty() => {
                bail!("compiler concept inventory authority must not be empty");
            }
            None if self.complete => {
                bail!("complete compiler concept inventory requires inventory_authority");
            }
            Some(_) | None => {}
        }

        if self.concepts.is_empty() {
            bail!("compiler concept ledger must contain at least one concept");
        }

        let mut ids = BTreeSet::new();
        for concept in &self.concepts {
            concept.validate()?;
            if !ids.insert(concept.concept_id.as_str()) {
                bail!("duplicate compiler concept id {:?}", concept.concept_id);
            }
        }

        if self.complete {
            let unresolved_ownership = self
                .concepts
                .iter()
                .filter(|concept| {
                    matches!(concept.ownership, OwnershipState::Active | OwnershipState::Deferred)
                })
                .map(|concept| concept.concept_id.as_str())
                .collect::<Vec<_>>();
            if !unresolved_ownership.is_empty() {
                bail!(
                    "complete compiler concept inventory has unresolved ownership: {:?}",
                    unresolved_ownership
                );
            }

            let missing_proof = self
                .concepts
                .iter()
                .filter(|concept| {
                    matches!(concept.gold, ProofState::Missing)
                        || matches!(concept.oracle, ProofState::Missing)
                        || matches!(concept.composition, ProofState::Missing)
                })
                .map(|concept| concept.concept_id.as_str())
                .collect::<Vec<_>>();
            if !missing_proof.is_empty() {
                bail!("complete compiler concept inventory has missing proof: {:?}", missing_proof);
            }
        }

        Ok(())
    }

    fn canonicalized(&self) -> Self {
        let mut normalized = self.clone();
        normalized.concepts.sort_by(|left, right| left.concept_id.cmp(&right.concept_id));
        for concept in &mut normalized.concepts {
            concept.ast_kinds.sort();
            concept.evidence.sort();
        }
        normalized
    }

    fn render_markdown(&self) -> Result<String> {
        self.validate()?;
        let normalized = self.canonicalized();
        let mut output = String::new();

        line(&mut output, "# Perl Compiler Concepts")?;
        line(&mut output, "")?;
        line(
            &mut output,
            "> Generated by `cargo run -p xtask --bin compiler-concepts -- --write-status`.",
        )?;
        line(&mut output, "> Check with `cargo run -p xtask --bin compiler-concepts -- --check`.")?;
        line(&mut output, "")?;
        line(&mut output, &normalized.coverage_scope)?;
        line(&mut output, "")?;
        line(&mut output, &format!("- Schema: `{}`", normalized.schema_version))?;
        line(&mut output, &format!("- Ledger: `{}`", normalized.ledger_id))?;
        line(&mut output, &format!("- Controller: {}", normalized.controller_issue))?;
        line(&mut output, &format!("- Inventory complete: `{}`", normalized.complete))?;
        if let Some(authority) = &normalized.inventory_authority {
            line(&mut output, &format!("- Inventory authority: `{authority}`"))?;
        } else {
            line(&mut output, "- Inventory authority: —")?;
        }
        line(&mut output, &format!("- Concepts in this seed: `{}`", normalized.concepts.len()))?;
        line(&mut output, "")?;
        line(&mut output, &format!("**Claim boundary:** {}", normalized.claim_boundary))?;
        line(&mut output, "")?;

        line(&mut output, "## Family counts")?;
        line(&mut output, "")?;
        line(&mut output, "| Family | Concepts |")?;
        line(&mut output, "| --- | ---: |")?;
        let mut family_counts = BTreeMap::<&str, usize>::new();
        for concept in &normalized.concepts {
            *family_counts.entry(concept.family.as_str()).or_default() += 1;
        }
        for (family, count) in family_counts {
            line(&mut output, &format!("| `{family}` | {count} |"))?;
        }
        line(&mut output, "")?;

        line(&mut output, "## Stage counts")?;
        line(&mut output, "")?;
        line(&mut output, "| Stage | State | Count |")?;
        line(&mut output, "| --- | --- | ---: |")?;
        let mut stage_counts = BTreeMap::<(&'static str, &'static str), usize>::new();
        for concept in &normalized.concepts {
            for (stage, state) in concept.stage_states() {
                *stage_counts.entry((stage, state)).or_default() += 1;
            }
        }
        for stage in ConceptRow::stage_order() {
            for ((row_stage, state), count) in &stage_counts {
                if *row_stage == stage {
                    line(&mut output, &format!("| {stage} | `{state}` | {count} |"))?;
                }
            }
        }
        line(&mut output, "")?;

        line(&mut output, "## Concept rows")?;
        line(&mut output, "")?;
        line(
            &mut output,
            "| Concept | AST kinds | Parser | Flat HIR | Body HIR | PIR-A | Effects/world | EIR | Gold | Oracle | Composition | Provider | Owner |",
        )?;
        line(
            &mut output,
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        )?;
        for concept in &normalized.concepts {
            let ast_kinds = if concept.ast_kinds.is_empty() {
                "—".to_string()
            } else {
                concept
                    .ast_kinds
                    .iter()
                    .map(|kind| format!("`{kind}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            line(
                &mut output,
                &format!(
                    "| `{}` | {} | `{}` | `{}` | `{}` | `{}` | `{}` | `{}` | `{}` | `{}` | `{}` | `{}` | {} |",
                    concept.concept_id,
                    ast_kinds,
                    concept.parser_ast.stable_name(),
                    concept.flat_hir.stable_name(),
                    concept.body_hir.stable_name(),
                    concept.pir_a.stable_name(),
                    concept.compile_effects_world.stable_name(),
                    concept.eir_profile.stable_name(),
                    concept.gold.stable_name(),
                    concept.oracle.stable_name(),
                    concept.composition.stable_name(),
                    concept.provider_eligibility.stable_name(),
                    concept.owner_issue,
                ),
            )?;
        }
        line(&mut output, "")?;

        line(&mut output, "## Claim boundaries")?;
        line(&mut output, "")?;
        for concept in &normalized.concepts {
            line(
                &mut output,
                &format!(
                    "- **`{}` ({}):** {}",
                    concept.concept_id, concept.owner_issue, concept.claim_boundary
                ),
            )?;
        }
        Ok(output)
    }
}

impl ConceptRow {
    fn validate(&self) -> Result<()> {
        if !valid_concept_id(&self.concept_id) {
            bail!("invalid compiler concept id {:?}", self.concept_id);
        }
        if self.family.trim().is_empty() {
            bail!("compiler concept {} has an empty family", self.concept_id);
        }
        let expected_family = self.concept_id.split('.').next().unwrap_or_default();
        if self.family != expected_family {
            bail!(
                "compiler concept {} family {:?} must match id prefix {:?}",
                self.concept_id,
                self.family,
                expected_family
            );
        }
        for (name, value) in
            [("title", self.title.as_str()), ("claim_boundary", self.claim_boundary.as_str())]
        {
            if value.trim().is_empty() {
                bail!("compiler concept {} has an empty {name}", self.concept_id);
            }
        }

        validate_unique_nonempty("ast_kinds", &self.concept_id, &self.ast_kinds)?;
        validate_unique_nonempty("evidence", &self.concept_id, &self.evidence)?;
        if self.evidence.is_empty() {
            bail!("compiler concept {} must name at least one evidence item", self.concept_id);
        }
        validate_issue("owner_issue", &self.owner_issue)?;

        if matches!(self.provider_eligibility, ProviderEligibility::Exact)
            && (!matches!(self.body_hir, RepresentationState::Modeled)
                || !matches!(self.pir_a, RepresentationState::Modeled)
                || !matches!(self.eir_profile, EirState::Executable))
        {
            bail!(
                "compiler concept {} cannot be provider-exact without modeled body HIR, modeled PIR-A, and executable EIR",
                self.concept_id
            );
        }

        if matches!(self.provider_eligibility, ProviderEligibility::Exact)
            && (!matches!(self.gold, ProofState::Proven)
                || !matches!(self.composition, ProofState::Proven))
        {
            bail!(
                "compiler concept {} cannot be provider-exact without proven gold and composition evidence",
                self.concept_id
            );
        }

        if matches!(self.eir_profile, EirState::Executable)
            && !matches!(self.pir_a, RepresentationState::Modeled)
        {
            bail!(
                "compiler concept {} cannot be executable from unverified PIR-A state {}",
                self.concept_id,
                self.pir_a.stable_name()
            );
        }

        Ok(())
    }

    fn stage_order() -> [&'static str; 11] {
        [
            "Parser / AST",
            "Flat HIR",
            "Body HIR",
            "PIR-A",
            "Effects / world",
            "EIR profile",
            "Gold",
            "Oracle",
            "Composition",
            "Provider eligibility",
            "Ownership",
        ]
    }

    fn stage_states(&self) -> [(&'static str, &'static str); 11] {
        [
            ("Parser / AST", self.parser_ast.stable_name()),
            ("Flat HIR", self.flat_hir.stable_name()),
            ("Body HIR", self.body_hir.stable_name()),
            ("PIR-A", self.pir_a.stable_name()),
            ("Effects / world", self.compile_effects_world.stable_name()),
            ("EIR profile", self.eir_profile.stable_name()),
            ("Gold", self.gold.stable_name()),
            ("Oracle", self.oracle.stable_name()),
            ("Composition", self.composition.stable_name()),
            ("Provider eligibility", self.provider_eligibility.stable_name()),
            ("Ownership", self.ownership.stable_name()),
        ]
    }
}

fn main() -> Result<()> {
    color_eyre::install().map_err(|error| anyhow!("install diagnostics: {error}"))?;
    let cli = Cli::parse();
    if cli.check && cli.write_status {
        bail!("--check and --write-status are mutually exclusive");
    }

    let ledger = ConceptLedger::load(&cli.ledger)?;
    let rendered = ledger.render_markdown()?;

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
                "generated compiler concept status is stale: run `cargo run -p xtask --bin compiler-concepts -- --write-status`"
            );
        }
        println!(
            "compiler concept ledger valid: {} concepts; status current",
            ledger.concepts.len()
        );
        return Ok(());
    }

    print!("{rendered}");
    Ok(())
}

fn write_status(path: &Path, rendered: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create status directory {}", parent.display()))?;
    }
    fs::write(path, rendered).with_context(|| format!("write status {}", path.display()))
}

fn line(output: &mut String, value: &str) -> Result<()> {
    writeln!(output, "{value}").map_err(|_| anyhow!("render compiler concept status"))
}

fn validate_issue(name: &str, value: &str) -> Result<()> {
    let digits = value.strip_prefix('#').unwrap_or_default();
    if digits.is_empty()
        || digits.starts_with('0')
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        bail!("{name} must be a GitHub issue reference like #6657; got {value:?}");
    }
    Ok(())
}

fn valid_concept_id(value: &str) -> bool {
    value.split('.').all(valid_concept_segment)
}

fn valid_concept_segment(value: &str) -> bool {
    let bytes = value.as_bytes();
    let Some(&first) = bytes.first() else {
        return false;
    };
    let last = bytes.last().copied().unwrap_or(first);
    let is_boundary = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    is_boundary(first)
        && is_boundary(last)
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

fn validate_unique_nonempty(name: &str, concept_id: &str, values: &[String]) -> Result<()> {
    if values.iter().any(|value| value.trim().is_empty()) {
        bail!("compiler concept {concept_id} {name} contains an empty entry");
    }
    let unique = values.iter().collect::<BTreeSet<_>>();
    if unique.len() != values.len() {
        bail!("compiler concept {concept_id} {name} contains duplicate entries");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMITTED_LEDGER: &str =
        include_str!("../../../contracts/compiler/perl_compiler_concepts.v1.toml");
    const COMMITTED_STATUS: &str =
        include_str!("../../../docs/project/status/perl_compiler_concepts.md");

    #[test]
    fn committed_ledger_validates_and_status_is_current() -> Result<()> {
        let ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        assert_eq!(ledger.schema_version, SCHEMA_VERSION);
        assert!(!ledger.complete);
        assert_eq!(ledger.render_markdown()?, COMMITTED_STATUS);
        Ok(())
    }

    #[test]
    fn duplicate_concept_ids_fail_closed() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        let duplicate = ledger
            .concepts
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("committed ledger unexpectedly empty"))?;
        ledger.concepts.push(duplicate);
        assert!(ledger.validate().is_err());
        Ok(())
    }

    #[test]
    fn invalid_owner_issue_fails_closed() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        let first = ledger
            .concepts
            .first_mut()
            .ok_or_else(|| anyhow!("committed ledger unexpectedly empty"))?;
        first.owner_issue = "6657".to_string();
        assert!(ledger.validate().is_err());
        Ok(())
    }

    #[test]
    fn opaque_body_hir_cannot_authorize_exact_provider_output() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        let first = ledger
            .concepts
            .first_mut()
            .ok_or_else(|| anyhow!("committed ledger unexpectedly empty"))?;
        first.body_hir = RepresentationState::Opaque;
        first.provider_eligibility = ProviderEligibility::Exact;
        first.gold = ProofState::Proven;
        first.composition = ProofState::Proven;
        assert!(ledger.validate().is_err());
        Ok(())
    }

    #[test]
    fn inventory_complete_requires_named_authority() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        ledger.complete = true;
        assert!(ledger.validate().is_err());
        Ok(())
    }

    #[test]
    fn inventory_authority_does_not_hide_unresolved_ownership() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        ledger.complete = true;
        ledger.inventory_authority = Some("issue:#6657/full-taxonomy-v1".to_string());
        for concept in &mut ledger.concepts {
            concept.provider_eligibility = ProviderEligibility::Ineligible;
            if matches!(concept.gold, ProofState::Missing) {
                concept.gold = ProofState::NotApplicable;
            }
            if matches!(concept.oracle, ProofState::Missing) {
                concept.oracle = ProofState::NotApplicable;
            }
            if matches!(concept.composition, ProofState::Missing) {
                concept.composition = ProofState::NotApplicable;
            }
        }
        assert!(ledger.validate().is_err());
        for concept in &mut ledger.concepts {
            concept.ownership = OwnershipState::Boundary;
        }
        ledger.validate()?;
        Ok(())
    }

    #[test]
    fn inventory_authority_does_not_hide_missing_proof() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        ledger.complete = true;
        ledger.inventory_authority = Some("issue:#6657/full-taxonomy-v1".to_string());
        for concept in &mut ledger.concepts {
            concept.provider_eligibility = ProviderEligibility::Ineligible;
            concept.ownership = OwnershipState::Boundary;
        }
        assert!(ledger.validate().is_err());
        for concept in &mut ledger.concepts {
            if matches!(concept.gold, ProofState::Missing) {
                concept.gold = ProofState::NotApplicable;
            }
            if matches!(concept.oracle, ProofState::Missing) {
                concept.oracle = ProofState::NotApplicable;
            }
            if matches!(concept.composition, ProofState::Missing) {
                concept.composition = ProofState::NotApplicable;
            }
        }
        ledger.validate()?;
        Ok(())
    }

    #[test]
    fn exact_provider_requires_canonical_executable_pipeline_regardless_of_owner() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        let first = ledger
            .concepts
            .first_mut()
            .ok_or_else(|| anyhow!("committed ledger unexpectedly empty"))?;
        first.provider_eligibility = ProviderEligibility::Exact;
        first.body_hir = RepresentationState::Opaque;
        first.pir_a = RepresentationState::Modeled;
        first.eir_profile = EirState::Executable;
        first.gold = ProofState::Proven;
        first.composition = ProofState::Proven;
        first.ownership = OwnershipState::Deferred;
        assert!(ledger.validate().is_err());
        Ok(())
    }

    #[test]
    fn executable_eir_requires_modeled_pir_a() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        let first = ledger
            .concepts
            .first_mut()
            .ok_or_else(|| anyhow!("committed ledger unexpectedly empty"))?;
        first.eir_profile = EirState::Executable;
        first.pir_a = RepresentationState::Skipped;
        assert!(ledger.validate().is_err());
        Ok(())
    }

    #[test]
    fn ast_kinds_is_required_by_typed_parser() {
        let source = COMMITTED_LEDGER.replacen("ast_kinds = [\"AmperCall\"]", "", 1);
        assert_ne!(source, COMMITTED_LEDGER);
        assert!(toml::from_str::<ConceptLedger>(&source).is_err());
    }

    #[test]
    fn concept_id_grammar_matches_segment_contract() {
        for value in ["a", "calls.normalized_semantics", "x1.a-b_c"] {
            assert!(valid_concept_id(value));
        }
        for value in ["", ".calls", "calls.", "calls..ampersand", "-calls.x", "calls.x-"] {
            assert!(!valid_concept_id(value));
        }
    }

    #[test]
    fn rendering_is_independent_of_input_order() -> Result<()> {
        let ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        let expected = ledger.render_markdown()?;
        let mut reversed = ledger.clone();
        reversed.concepts.reverse();
        assert_eq!(reversed.render_markdown()?, expected);
        Ok(())
    }
}
