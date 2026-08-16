//! Validate and render one-to-one compiler concept proof obligations from #6689.
//!
//! ```text
//! cargo run -p xtask --bin compiler-concept-proof -- --check
//! cargo run -p xtask --bin compiler-concept-proof -- --write-status
//! ```

#![allow(clippy::print_stdout)]

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const MATRIX_SCHEMA: &str = "perl_compiler_concept_proof.v1";
const DEFAULT_MATRIX: &str = "contracts/compiler/perl_compiler_concept_proof.v1.toml";
const DEFAULT_CONCEPT_LEDGER: &str = "contracts/compiler/perl_compiler_concepts.v1.toml";
const DEFAULT_PROOF_POLICY: &str = "contracts/compiler/perl_compiler_proof_policy.v1.toml";
const DEFAULT_STATUS: &str = "docs/project/status/perl_compiler_concept_proof.md";

#[derive(Debug, Parser)]
#[command(name = "compiler-concept-proof")]
#[command(about = "Validate and render compiler concept proof obligations")]
struct Cli {
    #[arg(long, default_value = DEFAULT_MATRIX)]
    matrix: PathBuf,

    #[arg(long, default_value = DEFAULT_CONCEPT_LEDGER)]
    concept_ledger: PathBuf,

    #[arg(long, default_value = DEFAULT_PROOF_POLICY)]
    proof_policy: PathBuf,

    #[arg(long, default_value = DEFAULT_STATUS)]
    status: PathBuf,

    #[arg(long)]
    check: bool,

    #[arg(long)]
    write_status: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ProofMatrix {
    schema_version: String,
    matrix_id: String,
    controller_issue: String,
    concept_ledger_schema: String,
    proof_policy_schema: String,
    coverage_scope: String,
    claim_boundary: String,
    complete: bool,
    defaults: ProofCellSet,
    requirements: Vec<Requirement>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ProofCellSet {
    positive_gold: ProofStatus,
    negative_gold: ProofStatus,
    boundary_gold: ProofStatus,
    recovery_gold: ProofStatus,
    hir_snapshot: ProofStatus,
    pir_snapshot: ProofStatus,
    verifier_mutation: ProofStatus,
    effects_world_fixture: ProofStatus,
    eir_differential: ProofStatus,
    real_perl_oracle: ProofStatus,
    composition_coverage: ProofStatus,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Requirement {
    concept_id: String,
    owner_issue: String,
    composition_dimensions: Vec<String>,
    basis: Vec<String>,
    evidence: Vec<String>,
    #[serde(default)]
    positive_gold: Option<ProofStatus>,
    #[serde(default)]
    negative_gold: Option<ProofStatus>,
    #[serde(default)]
    boundary_gold: Option<ProofStatus>,
    #[serde(default)]
    recovery_gold: Option<ProofStatus>,
    #[serde(default)]
    hir_snapshot: Option<ProofStatus>,
    #[serde(default)]
    pir_snapshot: Option<ProofStatus>,
    #[serde(default)]
    verifier_mutation: Option<ProofStatus>,
    #[serde(default)]
    effects_world_fixture: Option<ProofStatus>,
    #[serde(default)]
    eir_differential: Option<ProofStatus>,
    #[serde(default)]
    real_perl_oracle: Option<ProofStatus>,
    #[serde(default)]
    composition_coverage: Option<ProofStatus>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum ProofStatus {
    RequiredMissing,
    Satisfied,
    Deferred,
    NotObservable,
    NotApplicable,
}

#[derive(Debug, Clone, Deserialize)]
struct ConceptLedgerIndex {
    schema_version: String,
    concepts: Vec<ConceptIndexRow>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConceptIndexRow {
    concept_id: String,
    owner_issue: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ProofPolicyIndex {
    schema_version: String,
    proof_classes: Vec<ProofClassIndex>,
    dimensions: Vec<DimensionIndex>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProofClassIndex {
    class_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DimensionIndex {
    dimension_id: String,
}

impl ProofStatus {
    const fn ordered() -> [Self; 5] {
        [
            Self::RequiredMissing,
            Self::Satisfied,
            Self::Deferred,
            Self::NotObservable,
            Self::NotApplicable,
        ]
    }

    const fn stable_name(self) -> &'static str {
        match self {
            Self::RequiredMissing => "required_missing",
            Self::Satisfied => "satisfied",
            Self::Deferred => "deferred",
            Self::NotObservable => "not_observable",
            Self::NotApplicable => "not_applicable",
        }
    }
}

impl ProofCellSet {
    const fn class_ids() -> [&'static str; 11] {
        [
            "positive_gold",
            "negative_gold",
            "boundary_gold",
            "recovery_gold",
            "hir_snapshot",
            "pir_snapshot",
            "verifier_mutation",
            "effects_world_fixture",
            "eir_differential",
            "real_perl_oracle",
            "composition_coverage",
        ]
    }

    const fn entries(self) -> [(&'static str, ProofStatus); 11] {
        [
            ("positive_gold", self.positive_gold),
            ("negative_gold", self.negative_gold),
            ("boundary_gold", self.boundary_gold),
            ("recovery_gold", self.recovery_gold),
            ("hir_snapshot", self.hir_snapshot),
            ("pir_snapshot", self.pir_snapshot),
            ("verifier_mutation", self.verifier_mutation),
            ("effects_world_fixture", self.effects_world_fixture),
            ("eir_differential", self.eir_differential),
            ("real_perl_oracle", self.real_perl_oracle),
            ("composition_coverage", self.composition_coverage),
        ]
    }

    fn count(self, status: ProofStatus) -> usize {
        self.entries().into_iter().filter(|(_, value)| *value == status).count()
    }

    fn all_not_applicable(self) -> bool {
        self.entries().into_iter().all(|(_, status)| status == ProofStatus::NotApplicable)
    }
}

impl Requirement {
    const fn resolved(&self, defaults: ProofCellSet) -> ProofCellSet {
        ProofCellSet {
            positive_gold: match self.positive_gold {
                Some(value) => value,
                None => defaults.positive_gold,
            },
            negative_gold: match self.negative_gold {
                Some(value) => value,
                None => defaults.negative_gold,
            },
            boundary_gold: match self.boundary_gold {
                Some(value) => value,
                None => defaults.boundary_gold,
            },
            recovery_gold: match self.recovery_gold {
                Some(value) => value,
                None => defaults.recovery_gold,
            },
            hir_snapshot: match self.hir_snapshot {
                Some(value) => value,
                None => defaults.hir_snapshot,
            },
            pir_snapshot: match self.pir_snapshot {
                Some(value) => value,
                None => defaults.pir_snapshot,
            },
            verifier_mutation: match self.verifier_mutation {
                Some(value) => value,
                None => defaults.verifier_mutation,
            },
            effects_world_fixture: match self.effects_world_fixture {
                Some(value) => value,
                None => defaults.effects_world_fixture,
            },
            eir_differential: match self.eir_differential {
                Some(value) => value,
                None => defaults.eir_differential,
            },
            real_perl_oracle: match self.real_perl_oracle {
                Some(value) => value,
                None => defaults.real_perl_oracle,
            },
            composition_coverage: match self.composition_coverage {
                Some(value) => value,
                None => defaults.composition_coverage,
            },
        }
    }

    fn validate(
        &self,
        defaults: ProofCellSet,
        controller_issue: &str,
        dimensions: &BTreeSet<&str>,
    ) -> Result<()> {
        validate_concept_id(&self.concept_id)?;
        validate_issue("owner_issue", &self.owner_issue)?;
        validate_unique_nonempty(
            "composition_dimensions",
            &self.concept_id,
            &self.composition_dimensions,
        )?;
        if self.composition_dimensions.len() < 2 {
            bail!(
                "compiler concept {} must name at least two composition dimensions",
                self.concept_id
            );
        }
        for dimension in &self.composition_dimensions {
            if !dimensions.contains(dimension.as_str()) {
                bail!(
                    "compiler concept {} references unknown composition dimension {:?}",
                    self.concept_id,
                    dimension
                );
            }
        }

        validate_unique_nonempty("basis", &self.concept_id, &self.basis)?;
        if self.basis.len() < 2 {
            bail!(
                "compiler concept {} must name its owner and proof controller as basis",
                self.concept_id
            );
        }
        for basis in &self.basis {
            validate_issue("basis", basis)?;
        }
        if !self.basis.contains(&self.owner_issue) {
            bail!(
                "compiler concept {} basis must include owner {}",
                self.concept_id,
                self.owner_issue
            );
        }
        if !self.basis.iter().any(|value| value == controller_issue) {
            bail!(
                "compiler concept {} basis must include controller {}",
                self.concept_id,
                controller_issue
            );
        }

        validate_unique_nonempty("evidence", &self.concept_id, &self.evidence)?;
        let resolved = self.resolved(defaults);
        if resolved.all_not_applicable() {
            bail!(
                "compiler concept {} cannot make every proof class not_applicable",
                self.concept_id
            );
        }
        if resolved.entries().into_iter().any(|(_, status)| status == ProofStatus::Satisfied)
            && self.evidence.is_empty()
        {
            bail!("compiler concept {} marks proof satisfied without evidence", self.concept_id);
        }
        for (proof_class, status) in resolved.entries() {
            if status == ProofStatus::NotObservable
                && !matches!(proof_class, "real_perl_oracle" | "eir_differential")
            {
                bail!(
                    "compiler concept {} marks non-oracle proof class {} not_observable",
                    self.concept_id,
                    proof_class
                );
            }
        }
        Ok(())
    }
}

impl ProofMatrix {
    fn from_str(source: &str) -> Result<Self> {
        toml::from_str(source).context("parse compiler concept proof matrix")
    }

    fn load(path: &Path) -> Result<Self> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("read compiler concept proof matrix {}", path.display()))?;
        Self::from_str(&source)
            .with_context(|| format!("parse compiler concept proof matrix {}", path.display()))
    }

    fn validate(&self, concepts: &ConceptLedgerIndex, policy: &ProofPolicyIndex) -> Result<()> {
        if self.schema_version != MATRIX_SCHEMA {
            bail!(
                "unsupported concept proof schema {:?}; expected {:?}",
                self.schema_version,
                MATRIX_SCHEMA
            );
        }
        if self.concept_ledger_schema != concepts.schema_version {
            bail!(
                "concept proof matrix expects concept schema {:?}, but ledger uses {:?}",
                self.concept_ledger_schema,
                concepts.schema_version
            );
        }
        if self.proof_policy_schema != policy.schema_version {
            bail!(
                "concept proof matrix expects proof policy {:?}, but policy uses {:?}",
                self.proof_policy_schema,
                policy.schema_version
            );
        }
        for (name, value) in [
            ("matrix_id", self.matrix_id.as_str()),
            ("controller_issue", self.controller_issue.as_str()),
            ("coverage_scope", self.coverage_scope.as_str()),
            ("claim_boundary", self.claim_boundary.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("compiler concept proof field {name} must not be empty");
            }
        }
        validate_issue("controller_issue", &self.controller_issue)?;
        if self.requirements.is_empty() {
            bail!("compiler concept proof matrix must contain requirements");
        }

        let expected_classes = ProofCellSet::class_ids().into_iter().collect::<BTreeSet<_>>();
        let policy_classes = policy
            .proof_classes
            .iter()
            .map(|proof_class| proof_class.class_id.as_str())
            .collect::<BTreeSet<_>>();
        if policy_classes != expected_classes {
            bail!(
                "proof policy classes do not match concept proof matrix vocabulary: expected {:?}, got {:?}",
                expected_classes,
                policy_classes
            );
        }

        let dimensions = policy
            .dimensions
            .iter()
            .map(|dimension| dimension.dimension_id.as_str())
            .collect::<BTreeSet<_>>();
        let mut concepts_by_id = BTreeMap::new();
        for concept in &concepts.concepts {
            if concepts_by_id.insert(concept.concept_id.as_str(), concept).is_some() {
                bail!("duplicate concept id {:?} in concept ledger", concept.concept_id);
            }
        }

        let mut requirement_ids = BTreeSet::new();
        for requirement in &self.requirements {
            requirement.validate(self.defaults, &self.controller_issue, &dimensions)?;
            if !requirement_ids.insert(requirement.concept_id.as_str()) {
                bail!("duplicate proof requirement for concept {:?}", requirement.concept_id);
            }
            let concept = concepts_by_id.get(requirement.concept_id.as_str()).ok_or_else(|| {
                anyhow!("proof requirement references unknown concept {:?}", requirement.concept_id)
            })?;
            if requirement.owner_issue != concept.owner_issue {
                bail!(
                    "proof requirement {} owner {} does not match concept ledger owner {}",
                    requirement.concept_id,
                    requirement.owner_issue,
                    concept.owner_issue
                );
            }
        }

        let concept_ids = concepts_by_id.keys().copied().collect::<BTreeSet<_>>();
        if requirement_ids != concept_ids {
            let missing = concept_ids.difference(&requirement_ids).copied().collect::<Vec<_>>();
            let extra = requirement_ids.difference(&concept_ids).copied().collect::<Vec<_>>();
            bail!(
                "proof requirement set does not match concept ledger; missing {:?}, extra {:?}",
                missing,
                extra
            );
        }

        if self.complete
            && self.requirements.iter().any(|requirement| {
                requirement.resolved(self.defaults).entries().into_iter().any(|(_, status)| {
                    matches!(status, ProofStatus::RequiredMissing | ProofStatus::Deferred)
                })
            })
        {
            bail!(
                "complete compiler concept proof matrix cannot contain required_missing or deferred cells"
            );
        }
        Ok(())
    }

    fn canonicalized(&self) -> Self {
        let mut normalized = self.clone();
        normalized.requirements.sort_by(|left, right| left.concept_id.cmp(&right.concept_id));
        for requirement in &mut normalized.requirements {
            requirement.composition_dimensions.sort();
            requirement.basis.sort();
            requirement.evidence.sort();
        }
        normalized
    }

    fn render_markdown(
        &self,
        concepts: &ConceptLedgerIndex,
        policy: &ProofPolicyIndex,
    ) -> Result<String> {
        self.validate(concepts, policy)?;
        let normalized = self.canonicalized();
        let mut output = String::new();
        let mut overall_counts = BTreeMap::<ProofStatus, usize>::new();
        let mut class_counts = BTreeMap::<&'static str, BTreeMap<ProofStatus, usize>>::new();
        for requirement in &normalized.requirements {
            for (proof_class, status) in requirement.resolved(normalized.defaults).entries() {
                *overall_counts.entry(status).or_default() += 1;
                *class_counts.entry(proof_class).or_default().entry(status).or_default() += 1;
            }
        }

        line(&mut output, "# Perl Compiler Concept Proof Obligations")?;
        line(&mut output, "")?;
        line(
            &mut output,
            "> Generated by `cargo run -p xtask --bin compiler-concept-proof -- --write-status`.",
        )?;
        line(
            &mut output,
            "> Check with `cargo run -p xtask --bin compiler-concept-proof -- --check`.",
        )?;
        line(&mut output, "")?;
        line(&mut output, &normalized.coverage_scope)?;
        line(&mut output, "")?;
        line(&mut output, &format!("- Schema: `{}`", normalized.schema_version))?;
        line(&mut output, &format!("- Matrix: `{}`", normalized.matrix_id))?;
        line(&mut output, &format!("- Controller: {}", normalized.controller_issue))?;
        line(&mut output, &format!("- Concept schema: `{}`", normalized.concept_ledger_schema))?;
        line(&mut output, &format!("- Proof policy schema: `{}`", normalized.proof_policy_schema))?;
        line(&mut output, &format!("- Complete: `{}`", normalized.complete))?;
        line(&mut output, &format!("- Concept obligations: `{}`", normalized.requirements.len()))?;
        line(
            &mut output,
            &format!(
                "- Proof cells: `{}`",
                normalized.requirements.len() * ProofCellSet::class_ids().len()
            ),
        )?;
        line(&mut output, "")?;
        line(&mut output, &format!("**Claim boundary:** {}", normalized.claim_boundary))?;
        line(&mut output, "")?;

        line(&mut output, "## Status counts")?;
        line(&mut output, "")?;
        line(&mut output, "| Status | Count |")?;
        line(&mut output, "| --- | ---: |")?;
        for status in ProofStatus::ordered() {
            let count = overall_counts.get(&status).copied().unwrap_or(0);
            line(&mut output, &format!("| `{}` | {count} |", status.stable_name()))?;
        }
        line(&mut output, "")?;

        line(&mut output, "## Proof-class counts")?;
        line(&mut output, "")?;
        line(
            &mut output,
            "| Proof class | Required missing | Satisfied | Deferred | Not observable | Not applicable | Total |",
        )?;
        line(&mut output, "| --- | ---: | ---: | ---: | ---: | ---: | ---: |")?;
        for proof_class in ProofCellSet::class_ids() {
            let counts = class_counts.get(proof_class);
            let required_missing = count_status(counts, ProofStatus::RequiredMissing);
            let satisfied = count_status(counts, ProofStatus::Satisfied);
            let deferred = count_status(counts, ProofStatus::Deferred);
            let not_observable = count_status(counts, ProofStatus::NotObservable);
            let not_applicable = count_status(counts, ProofStatus::NotApplicable);
            let total = required_missing + satisfied + deferred + not_observable + not_applicable;
            line(
                &mut output,
                &format!(
                    "| `{proof_class}` | {required_missing} | {satisfied} | {deferred} | {not_observable} | {not_applicable} | {total} |"
                ),
            )?;
        }
        line(&mut output, "")?;

        line(&mut output, "## Concept obligations")?;
        line(&mut output, "")?;
        line(
            &mut output,
            "| Concept | Owner | Composition dimensions | Required missing | Deferred | Not observable | Not applicable | Satisfied | Basis |",
        )?;
        line(&mut output, "| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |")?;
        for requirement in &normalized.requirements {
            let resolved = requirement.resolved(normalized.defaults);
            line(
                &mut output,
                &format!(
                    "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} |",
                    requirement.concept_id,
                    requirement.owner_issue,
                    code_list(&requirement.composition_dimensions),
                    resolved.count(ProofStatus::RequiredMissing),
                    resolved.count(ProofStatus::Deferred),
                    resolved.count(ProofStatus::NotObservable),
                    resolved.count(ProofStatus::NotApplicable),
                    resolved.count(ProofStatus::Satisfied),
                    requirement.basis.join(", ")
                ),
            )?;
        }
        line(&mut output, "")?;

        line(&mut output, "## Missing proof by concept")?;
        line(&mut output, "")?;
        for requirement in &normalized.requirements {
            let missing = requirement
                .resolved(normalized.defaults)
                .entries()
                .into_iter()
                .filter_map(|(proof_class, status)| {
                    (status == ProofStatus::RequiredMissing).then_some(proof_class.to_string())
                })
                .collect::<Vec<_>>();
            line(
                &mut output,
                &format!(
                    "- **`{}` ({}):** {}",
                    requirement.concept_id,
                    requirement.owner_issue,
                    code_list(&missing)
                ),
            )?;
        }
        Ok(output)
    }
}

fn main() -> Result<()> {
    color_eyre::install().map_err(|error| anyhow!("install diagnostics: {error}"))?;
    let cli = Cli::parse();
    if cli.check && cli.write_status {
        bail!("--check and --write-status are mutually exclusive");
    }

    let matrix = ProofMatrix::load(&cli.matrix)?;
    let concepts = load_concepts(&cli.concept_ledger)?;
    let policy = load_policy(&cli.proof_policy)?;
    let rendered = matrix.render_markdown(&concepts, &policy)?;

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
                "generated concept proof status is stale: run `cargo run -p xtask --bin compiler-concept-proof -- --write-status`"
            );
        }
        println!(
            "compiler concept proof matrix valid: {} concepts, {} proof cells",
            matrix.requirements.len(),
            matrix.requirements.len() * ProofCellSet::class_ids().len()
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
        .with_context(|| format!("parse compiler concept ledger {}", path.display()))
}

fn load_policy(path: &Path) -> Result<ProofPolicyIndex> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("read compiler proof policy {}", path.display()))?;
    toml::from_str(&source)
        .with_context(|| format!("parse compiler proof policy {}", path.display()))
}

fn write_status(path: &Path, rendered: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create status directory {}", parent.display()))?;
    }
    fs::write(path, rendered).with_context(|| format!("write status {}", path.display()))
}

fn count_status(counts: Option<&BTreeMap<ProofStatus, usize>>, status: ProofStatus) -> usize {
    counts.and_then(|values| values.get(&status)).copied().unwrap_or(0)
}

fn code_list(values: &[String]) -> String {
    values.iter().map(|value| format!("`{value}`")).collect::<Vec<_>>().join(", ")
}

fn line(output: &mut String, value: &str) -> Result<()> {
    writeln!(output, "{value}").map_err(|_| anyhow!("render compiler concept proof status"))
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

fn validate_concept_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || value.bytes().any(|byte| {
            !(byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'-'))
        })
    {
        bail!("invalid compiler concept id {value:?}");
    }
    Ok(())
}

fn validate_unique_nonempty(name: &str, owner: &str, values: &[String]) -> Result<()> {
    if values.iter().any(|value| value.trim().is_empty()) {
        bail!("{owner} {name} contains an empty entry");
    }
    let unique = values.iter().collect::<BTreeSet<_>>();
    if unique.len() != values.len() {
        bail!("{owner} {name} contains duplicate entries");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MATRIX: &str =
        include_str!("../../../contracts/compiler/perl_compiler_concept_proof.v1.toml");
    const CONCEPTS: &str =
        include_str!("../../../contracts/compiler/perl_compiler_concepts.v1.toml");
    const POLICY: &str =
        include_str!("../../../contracts/compiler/perl_compiler_proof_policy.v1.toml");
    const STATUS: &str =
        include_str!("../../../docs/project/status/perl_compiler_concept_proof.md");

    fn concepts() -> Result<ConceptLedgerIndex> {
        toml::from_str(CONCEPTS).context("parse committed concept ledger")
    }

    fn policy() -> Result<ProofPolicyIndex> {
        toml::from_str(POLICY).context("parse committed proof policy")
    }

    #[test]
    fn committed_matrix_validates_and_status_is_current() -> Result<()> {
        let matrix = ProofMatrix::from_str(MATRIX)?;
        let concepts = concepts()?;
        let policy = policy()?;
        matrix.validate(&concepts, &policy)?;
        assert!(!matrix.complete);
        assert_eq!(matrix.render_markdown(&concepts, &policy)?, STATUS);
        Ok(())
    }

    #[test]
    fn missing_requirement_fails_closed() -> Result<()> {
        let mut matrix = ProofMatrix::from_str(MATRIX)?;
        matrix.requirements.pop();
        assert!(matrix.validate(&concepts()?, &policy()?).is_err());
        Ok(())
    }

    #[test]
    fn mismatched_owner_fails_closed() -> Result<()> {
        let mut matrix = ProofMatrix::from_str(MATRIX)?;
        let first = matrix
            .requirements
            .first_mut()
            .ok_or_else(|| anyhow!("committed matrix unexpectedly empty"))?;
        first.owner_issue = "#6657".to_string();
        first.basis = vec!["#6657".to_string(), "#6689".to_string()];
        assert!(matrix.validate(&concepts()?, &policy()?).is_err());
        Ok(())
    }

    #[test]
    fn unknown_dimension_fails_closed() -> Result<()> {
        let mut matrix = ProofMatrix::from_str(MATRIX)?;
        let first = matrix
            .requirements
            .first_mut()
            .ok_or_else(|| anyhow!("committed matrix unexpectedly empty"))?;
        first.composition_dimensions.push("unknown_dimension".to_string());
        assert!(matrix.validate(&concepts()?, &policy()?).is_err());
        Ok(())
    }

    #[test]
    fn satisfied_without_evidence_fails_closed() -> Result<()> {
        let mut matrix = ProofMatrix::from_str(MATRIX)?;
        let first = matrix
            .requirements
            .first_mut()
            .ok_or_else(|| anyhow!("committed matrix unexpectedly empty"))?;
        first.positive_gold = Some(ProofStatus::Satisfied);
        assert!(matrix.validate(&concepts()?, &policy()?).is_err());
        Ok(())
    }

    #[test]
    fn untracked_requirement_fails_closed() -> Result<()> {
        let mut matrix = ProofMatrix::from_str(MATRIX)?;
        let mut extra = matrix
            .requirements
            .first()
            .ok_or_else(|| anyhow!("committed matrix unexpectedly empty"))?
            .clone();
        extra.concept_id = "runtime.untracked_concept".to_string();
        matrix.requirements.push(extra);
        let error = matrix
            .validate(&concepts()?, &policy()?)
            .expect_err("untracked requirement must fail closed")
            .to_string();
        assert!(error.contains("unknown concept"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn all_not_applicable_row_fails_closed() -> Result<()> {
        let mut matrix = ProofMatrix::from_str(MATRIX)?;
        let first = matrix
            .requirements
            .first_mut()
            .ok_or_else(|| anyhow!("committed matrix unexpectedly empty"))?;
        for cell in [
            &mut first.positive_gold,
            &mut first.negative_gold,
            &mut first.boundary_gold,
            &mut first.recovery_gold,
            &mut first.hir_snapshot,
            &mut first.pir_snapshot,
            &mut first.verifier_mutation,
            &mut first.effects_world_fixture,
            &mut first.eir_differential,
            &mut first.real_perl_oracle,
            &mut first.composition_coverage,
        ] {
            *cell = Some(ProofStatus::NotApplicable);
        }
        let error = matrix
            .validate(&concepts()?, &policy()?)
            .expect_err("all-not_applicable row must fail closed")
            .to_string();
        assert!(error.contains("not_applicable"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn non_oracle_not_observable_fails_closed() -> Result<()> {
        let mut matrix = ProofMatrix::from_str(MATRIX)?;
        let first = matrix
            .requirements
            .first_mut()
            .ok_or_else(|| anyhow!("committed matrix unexpectedly empty"))?;
        first.positive_gold = Some(ProofStatus::NotObservable);
        let error = matrix
            .validate(&concepts()?, &policy()?)
            .expect_err("non-oracle not_observable must fail closed")
            .to_string();
        assert!(error.contains("non-oracle proof class"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn proof_class_vocabulary_drift_fails_closed() -> Result<()> {
        let matrix = ProofMatrix::from_str(MATRIX)?;
        let mut policy = policy()?;
        policy.proof_classes.pop();
        let error = matrix
            .validate(&concepts()?, &policy)
            .expect_err("proof-class vocabulary drift must fail closed")
            .to_string();
        assert!(error.contains("proof policy classes do not match"), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn rendering_is_independent_of_input_order() -> Result<()> {
        let matrix = ProofMatrix::from_str(MATRIX)?;
        let concepts = concepts()?;
        let policy = policy()?;
        let expected = matrix.render_markdown(&concepts, &policy)?;
        let mut reversed = matrix.clone();
        reversed.requirements.reverse();
        assert_eq!(reversed.render_markdown(&concepts, &policy)?, expected);
        Ok(())
    }
}
