//! Reject HIR-only completion claims for `control.postfix_modifier` (#13281).
//!
//! Consumes the canonical concept ledger and proof matrix. HIR receipts can
//! satisfy only HIR cells. Full-capability closure stays blocked while required
//! semantic, PIR, oracle, composition, execution, or editor cells are missing,
//! rejected, stale, not-run, or silently omitted. Issue closure and checkboxes
//! are not evidence.
//!
//! ```text
//! cargo test -p xtask --bin postfix-capability-closure --locked
//! cargo run --locked -p xtask --bin postfix-capability-closure -- --check
//! ```

#![allow(clippy::print_stdout)]

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const CONCEPT_ID: &str = "control.postfix_modifier";
const DEFAULT_LEDGER: &str = "contracts/compiler/perl_compiler_concepts.v1.toml";
const DEFAULT_MATRIX: &str = "contracts/compiler/perl_compiler_concept_proof.v1.toml";
const DOC_SURFACES: [&str; 3] = [
    "docs/project/status/perl_compiler_concepts.md",
    "docs/project/status/perl_compiler_concept_proof.md",
    "docs/project/COMPILER_CAPABILITY_STATUS.md",
];

/// Stable required-cell order. The first unproven required cell is the
/// narrowest blocker. Effects/world is postfix-`not_applicable` and is still
/// emitted so it cannot disappear.
const CELL_ORDER: [CellId; 10] = [
    CellId::Parser,
    CellId::FlatHir,
    CellId::BodyHir,
    CellId::Semantic,
    CellId::Pir,
    CellId::Oracle,
    CellId::Composition,
    CellId::Execution,
    CellId::Editor,
    CellId::Effects,
];

#[derive(Debug, Parser)]
#[command(name = "postfix-capability-closure")]
#[command(about = "Reject HIR-only completion claims for postfix statement modifiers")]
struct Cli {
    #[arg(long, default_value = DEFAULT_LEDGER)]
    ledger: PathBuf,
    #[arg(long, default_value = DEFAULT_MATRIX)]
    matrix: PathBuf,
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    check: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CellId {
    Parser,
    FlatHir,
    BodyHir,
    Semantic,
    Pir,
    Oracle,
    Composition,
    Execution,
    Editor,
    Effects,
}

impl CellId {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Parser => "parser",
            Self::FlatHir => "flat_hir",
            Self::BodyHir => "body_hir",
            Self::Semantic => "semantic",
            Self::Pir => "pir",
            Self::Oracle => "oracle",
            Self::Composition => "composition",
            Self::Execution => "execution",
            Self::Editor => "editor",
            Self::Effects => "effects",
        }
    }

    const fn required(self) -> bool {
        !matches!(self, Self::Effects)
    }

    const fn hir_only(self) -> bool {
        matches!(self, Self::FlatHir | Self::BodyHir)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellStatus {
    Proven,
    Missing,
    NotRun,
    Rejected,
    Stale,
    Deferred,
    NotApplicable,
    Unknown,
}

impl CellStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Proven => "proven",
            Self::Missing => "missing",
            Self::NotRun => "not_run",
            Self::Rejected => "rejected",
            Self::Stale => "stale",
            Self::Deferred => "deferred",
            Self::NotApplicable => "not_applicable",
            Self::Unknown => "unknown",
        }
    }

    const fn blocks_required_closure(self) -> bool {
        !matches!(self, Self::Proven | Self::NotApplicable)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvidenceKind {
    Hir,
    Pir,
    Semantic,
    Oracle,
    Composition,
    Execution,
    IssueProse,
    Checkbox,
}

impl EvidenceKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Hir => "hir",
            Self::Pir => "pir",
            Self::Semantic => "semantic",
            Self::Oracle => "oracle",
            Self::Composition => "composition",
            Self::Execution => "execution",
            Self::IssueProse => "issue_prose",
            Self::Checkbox => "checkbox",
        }
    }

    fn admits(self, cell: CellId) -> bool {
        match (self, cell) {
            (Self::IssueProse | Self::Checkbox, _) => false,
            (Self::Hir, CellId::FlatHir | CellId::BodyHir | CellId::Parser) => true,
            (Self::Semantic, CellId::Semantic) => true,
            (Self::Pir, CellId::Pir) => true,
            (Self::Oracle, CellId::Oracle) => true,
            (Self::Composition, CellId::Composition) => true,
            (Self::Execution, CellId::Execution) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapabilityCell {
    id: CellId,
    status: CellStatus,
    reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapabilityView {
    concept_id: String,
    cells: Vec<CapabilityCell>,
}

impl CapabilityView {
    fn cell(&self, id: CellId) -> Result<&CapabilityCell> {
        self.cells
            .iter()
            .find(|cell| cell.id == id)
            .ok_or_else(|| anyhow!("derived capability view silently omitted cell {}", id.as_str()))
    }

    fn narrowest_blocker(&self) -> Result<Option<&CapabilityCell>> {
        for id in CELL_ORDER {
            if !id.required() {
                continue;
            }
            let cell = self.cell(id)?;
            if cell.status.blocks_required_closure() {
                return Ok(Some(cell));
            }
        }
        Ok(None)
    }

    fn fully_closed(&self) -> Result<bool> {
        Ok(self.narrowest_blocker()?.is_none())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ConceptLedger {
    complete: bool,
    concepts: Vec<ConceptRow>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConceptRow {
    concept_id: String,
    parser_ast: String,
    flat_hir: String,
    body_hir: String,
    pir_a: String,
    compile_effects_world: String,
    eir_profile: String,
    gold: String,
    oracle: String,
    composition: String,
    provider_eligibility: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ProofMatrix {
    complete: bool,
    defaults: ProofCellSet,
    requirements: Vec<Requirement>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
struct Requirement {
    concept_id: String,
    #[serde(default)]
    evidence_by_class: BTreeMap<String, Vec<String>>,
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

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ProofStatus {
    RequiredMissing,
    Satisfied,
    Deferred,
    NotObservable,
    NotApplicable,
}

impl Requirement {
    fn resolved(&self, defaults: ProofCellSet) -> ProofCellSet {
        ProofCellSet {
            positive_gold: self.positive_gold.unwrap_or(defaults.positive_gold),
            negative_gold: self.negative_gold.unwrap_or(defaults.negative_gold),
            boundary_gold: self.boundary_gold.unwrap_or(defaults.boundary_gold),
            recovery_gold: self.recovery_gold.unwrap_or(defaults.recovery_gold),
            hir_snapshot: self.hir_snapshot.unwrap_or(defaults.hir_snapshot),
            pir_snapshot: self.pir_snapshot.unwrap_or(defaults.pir_snapshot),
            verifier_mutation: self.verifier_mutation.unwrap_or(defaults.verifier_mutation),
            effects_world_fixture: self
                .effects_world_fixture
                .unwrap_or(defaults.effects_world_fixture),
            eir_differential: self.eir_differential.unwrap_or(defaults.eir_differential),
            real_perl_oracle: self.real_perl_oracle.unwrap_or(defaults.real_perl_oracle),
            composition_coverage: self
                .composition_coverage
                .unwrap_or(defaults.composition_coverage),
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if !cli.check {
        bail!("pass --check to evaluate postfix capability closure");
    }
    let root = cli.root.unwrap_or_else(|| PathBuf::from("."));
    let report = check_repository(&root, &cli.ledger, &cli.matrix)?;
    print!("{report}");
    Ok(())
}

fn check_repository(root: &Path, ledger_path: &Path, matrix_path: &Path) -> Result<String> {
    let ledger = load_ledger(&root.join(ledger_path))?;
    let matrix = load_matrix(&root.join(matrix_path))?;
    let view = derive_postfix_capability(&ledger, &matrix)?;
    let mut docs = Vec::new();
    for relative in DOC_SURFACES {
        let path = root.join(relative);
        let source = fs::read_to_string(&path)
            .with_context(|| format!("read capability surface {}", path.display()))?;
        docs.push((relative, source));
    }
    let doc_refs: Vec<(&str, &str)> =
        docs.iter().map(|(name, source)| (*name, source.as_str())).collect();
    evaluate_closure_gate(&view, &ledger, &matrix, &doc_refs)
}

fn load_ledger(path: &Path) -> Result<ConceptLedger> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("read compiler concept ledger {}", path.display()))?;
    toml::from_str(&source)
        .with_context(|| format!("parse compiler concept ledger {}", path.display()))
}

fn load_matrix(path: &Path) -> Result<ProofMatrix> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("read compiler concept proof matrix {}", path.display()))?;
    toml::from_str(&source)
        .with_context(|| format!("parse compiler concept proof matrix {}", path.display()))
}

fn derive_postfix_capability(
    ledger: &ConceptLedger,
    matrix: &ProofMatrix,
) -> Result<CapabilityView> {
    let concept = ledger
        .concepts
        .iter()
        .find(|row| row.concept_id == CONCEPT_ID)
        .ok_or_else(|| anyhow!("concept ledger is missing {CONCEPT_ID}"))?;
    let requirement = matrix
        .requirements
        .iter()
        .find(|row| row.concept_id == CONCEPT_ID)
        .ok_or_else(|| anyhow!("proof matrix is missing {CONCEPT_ID}"))?;
    let resolved = requirement.resolved(matrix.defaults);
    let mut cells = Vec::new();
    for id in CELL_ORDER {
        cells.push(derive_cell(id, concept, requirement, resolved)?);
    }
    let view = CapabilityView { concept_id: CONCEPT_ID.to_string(), cells };
    let present: BTreeSet<_> = view.cells.iter().map(|cell| cell.id).collect();
    for id in CELL_ORDER {
        if !present.contains(&id) {
            bail!("derived capability view silently omitted cell {}", id.as_str());
        }
    }
    Ok(view)
}

fn derive_cell(
    id: CellId,
    concept: &ConceptRow,
    requirement: &Requirement,
    resolved: ProofCellSet,
) -> Result<CapabilityCell> {
    let status_reason = match id {
        CellId::Parser => stage_cell(concept.parser_ast.as_str(), &["parsed"]),
        CellId::FlatHir => hir_cell(concept.flat_hir.as_str(), requirement, resolved.hir_snapshot),
        CellId::BodyHir => hir_cell(concept.body_hir.as_str(), requirement, resolved.hir_snapshot),
        CellId::Semantic => class_group_cell(
            CellId::Semantic,
            &[
                ("positive_gold", resolved.positive_gold),
                ("negative_gold", resolved.negative_gold),
                ("boundary_gold", resolved.boundary_gold),
                ("recovery_gold", resolved.recovery_gold),
            ],
            requirement,
        ),
        CellId::Pir => pir_cell(concept, requirement, resolved),
        CellId::Oracle => class_group_cell(
            CellId::Oracle,
            &[("real_perl_oracle", resolved.real_perl_oracle)],
            requirement,
        ),
        CellId::Composition => class_group_cell(
            CellId::Composition,
            &[("composition_coverage", resolved.composition_coverage)],
            requirement,
        ),
        CellId::Execution => execution_cell(concept, requirement, resolved.eir_differential),
        CellId::Editor => editor_cell(concept.provider_eligibility.as_str()),
        CellId::Effects => effects_cell(concept.compile_effects_world.as_str(), resolved),
    };
    Ok(CapabilityCell { id, status: status_reason.0, reason: status_reason.1 })
}

fn stage_cell(stage: &str, proving: &[&str]) -> (CellStatus, String) {
    if proving.contains(&stage) {
        (CellStatus::Proven, format!("stage is {stage}"))
    } else {
        (CellStatus::Missing, format!("stage is {stage}, not one of {proving:?}"))
    }
}

fn hir_cell(stage: &str, requirement: &Requirement, snapshot: ProofStatus) -> (CellStatus, String) {
    if stage != "modeled" {
        return (CellStatus::Missing, format!("HIR stage is {stage}, not modeled"));
    }
    admit_class(CellId::FlatHir, "hir_snapshot", snapshot, requirement)
}

fn pir_cell(
    concept: &ConceptRow,
    requirement: &Requirement,
    resolved: ProofCellSet,
) -> (CellStatus, String) {
    if concept.pir_a != "modeled" {
        return (CellStatus::Missing, format!("PIR-A stage is {}, not modeled", concept.pir_a));
    }
    let snapshot = admit_class(CellId::Pir, "pir_snapshot", resolved.pir_snapshot, requirement);
    if snapshot.0 != CellStatus::Proven {
        return snapshot;
    }
    admit_class(CellId::Pir, "verifier_mutation", resolved.verifier_mutation, requirement)
}

fn execution_cell(
    concept: &ConceptRow,
    requirement: &Requirement,
    status: ProofStatus,
) -> (CellStatus, String) {
    match status {
        ProofStatus::Deferred => {
            (CellStatus::Deferred, "eir_differential is explicitly deferred".to_string())
        }
        ProofStatus::NotApplicable => {
            (CellStatus::NotApplicable, "eir_differential is not_applicable".to_string())
        }
        ProofStatus::Satisfied if concept.eir_profile == "executable" => {
            admit_class(CellId::Execution, "eir_differential", status, requirement)
        }
        ProofStatus::Satisfied => (
            CellStatus::Rejected,
            format!("eir_differential is satisfied while eir_profile is {}", concept.eir_profile),
        ),
        ProofStatus::RequiredMissing => (
            CellStatus::Missing,
            format!(
                "eir_profile is {} and eir_differential is required_missing",
                concept.eir_profile
            ),
        ),
        ProofStatus::NotObservable => {
            (CellStatus::Unknown, "eir_differential is not_observable".to_string())
        }
    }
}

fn editor_cell(provider: &str) -> (CellStatus, String) {
    match provider {
        "exact" | "qualified" => {
            (CellStatus::Proven, format!("provider eligibility is {provider}"))
        }
        "fallback_only" | "ineligible" => {
            (CellStatus::Missing, format!("provider eligibility is {provider}"))
        }
        other => (CellStatus::Unknown, format!("provider eligibility is {other}")),
    }
}

fn effects_cell(stage: &str, resolved: ProofCellSet) -> (CellStatus, String) {
    if stage == "not_applicable" || resolved.effects_world_fixture == ProofStatus::NotApplicable {
        return (
            CellStatus::NotApplicable,
            "effects/world is explicitly not_applicable".to_string(),
        );
    }
    stage_cell(stage, &["modeled"])
}

fn class_group_cell(
    cell: CellId,
    classes: &[(&str, ProofStatus)],
    requirement: &Requirement,
) -> (CellStatus, String) {
    let mut first = None;
    for (class_id, status) in classes {
        let admitted = admit_class(cell, class_id, *status, requirement);
        if admitted.0 != CellStatus::Proven && admitted.0 != CellStatus::NotApplicable {
            return admitted;
        }
        if first.is_none() {
            first = Some(admitted);
        }
    }
    match first {
        Some(value) => value,
        None => (CellStatus::Unknown, format!("{} has no proof classes", cell.as_str())),
    }
}

fn admit_class(
    cell: CellId,
    class_id: &str,
    status: ProofStatus,
    requirement: &Requirement,
) -> (CellStatus, String) {
    match status {
        ProofStatus::NotApplicable => {
            (CellStatus::NotApplicable, format!("{class_id} is explicitly not_applicable"))
        }
        ProofStatus::Deferred => {
            (CellStatus::Deferred, format!("{class_id} is explicitly deferred"))
        }
        ProofStatus::NotObservable => {
            (CellStatus::Unknown, format!("{class_id} is not_observable"))
        }
        ProofStatus::RequiredMissing => {
            if requirement
                .evidence_by_class
                .get(class_id)
                .is_some_and(|receipts| receipts.iter().any(|receipt| looks_not_run(receipt)))
            {
                return (CellStatus::NotRun, format!("{class_id} evidence was not run"));
            }
            (CellStatus::Missing, format!("{class_id} is required_missing"))
        }
        ProofStatus::Satisfied => {
            let Some(receipts) = requirement.evidence_by_class.get(class_id) else {
                return (
                    CellStatus::Missing,
                    format!("{class_id} is satisfied without class-bound evidence"),
                );
            };
            if receipts.is_empty() {
                return (
                    CellStatus::Missing,
                    format!("{class_id} is satisfied without class-bound evidence"),
                );
            }
            let mut kinds = Vec::new();
            for receipt in receipts {
                if looks_stale(receipt) {
                    return (CellStatus::Stale, format!("{class_id} evidence is stale: {receipt}"));
                }
                if looks_not_run(receipt) {
                    return (
                        CellStatus::NotRun,
                        format!("{class_id} evidence was not run: {receipt}"),
                    );
                }
                let kind = classify_receipt(class_id, receipt);
                if !kind.admits(cell) {
                    return (
                        CellStatus::Rejected,
                        format!(
                            "{class_id} evidence {} is {} and cannot satisfy {}",
                            receipt,
                            kind.as_str(),
                            cell.as_str()
                        ),
                    );
                }
                kinds.push(kind);
            }
            if cell.hir_only() && kinds.iter().all(|kind| *kind == EvidenceKind::Hir) {
                return (CellStatus::Proven, format!("{class_id} bound to HIR receipts"));
            }
            if !cell.hir_only() && kinds.contains(&EvidenceKind::Hir) {
                return (
                    CellStatus::Rejected,
                    format!("HIR evidence cannot satisfy {}", cell.as_str()),
                );
            }
            (CellStatus::Proven, format!("{class_id} bound to compatible evidence"))
        }
    }
}

fn classify_receipt(proof_class: &str, receipt: &str) -> EvidenceKind {
    let trimmed = receipt.trim();
    if is_checkbox(trimmed) {
        return EvidenceKind::Checkbox;
    }
    if is_issue_prose(trimmed) {
        return EvidenceKind::IssueProse;
    }
    if looks_hir(trimmed) {
        return EvidenceKind::Hir;
    }
    match proof_class {
        "hir_snapshot" => EvidenceKind::Hir,
        "pir_snapshot" | "verifier_mutation" => EvidenceKind::Pir,
        "positive_gold" | "negative_gold" | "boundary_gold" | "recovery_gold" => {
            EvidenceKind::Semantic
        }
        "real_perl_oracle" => EvidenceKind::Oracle,
        "composition_coverage" => EvidenceKind::Composition,
        "eir_differential" => EvidenceKind::Execution,
        _ => EvidenceKind::Hir,
    }
}

fn is_checkbox(receipt: &str) -> bool {
    receipt.contains("[x]") || receipt.contains("[X]")
}

fn is_issue_prose(receipt: &str) -> bool {
    let trimmed = receipt.trim();
    if is_bare_issue_ref(trimmed) {
        return true;
    }
    contains_issue_closure_ref(&trimmed.to_ascii_lowercase())
}

fn is_bare_issue_ref(receipt: &str) -> bool {
    let Some(rest) = receipt.strip_prefix('#') else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
}

fn contains_issue_closure_ref(lower: &str) -> bool {
    // GitHub closing keywords plus an explicit `issue #N` form. Longer
    // spellings are listed first so `closes` is not reduced to `close`.
    const KEYWORDS: [&str; 10] = [
        "closes", "closed", "close", "fixes", "fixed", "fix", "resolves", "resolved", "resolve",
        "issue",
    ];
    for keyword in KEYWORDS {
        let mut from = 0;
        while from < lower.len() {
            let Some(rel) = lower[from..].find(keyword) else {
                break;
            };
            let idx = from + rel;
            if keyword_at_word_boundary(lower, idx, keyword.len())
                && issue_ref_after_keyword(lower, idx + keyword.len())
            {
                return true;
            }
            from = idx.saturating_add(1);
        }
    }
    false
}

fn keyword_at_word_boundary(text: &str, start: usize, len: usize) -> bool {
    let before_ok = start == 0
        || text
            .as_bytes()
            .get(start.saturating_sub(1))
            .is_some_and(|byte| !byte.is_ascii_alphanumeric());
    let end = start.saturating_add(len);
    let after_ok = end == text.len()
        || text.as_bytes().get(end).is_some_and(|byte| !byte.is_ascii_alphanumeric());
    before_ok && after_ok
}

fn issue_ref_after_keyword(lower: &str, after_keyword: usize) -> bool {
    let rest = match lower.get(after_keyword..) {
        Some(rest) => rest.trim_start_matches(|ch: char| {
            ch.is_ascii_whitespace() || matches!(ch, ':' | ',' | '-')
        }),
        None => return false,
    };
    starts_with_issue_ref(rest)
}

fn starts_with_issue_ref(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('#') else {
        return false;
    };
    rest.bytes().take_while(|byte| byte.is_ascii_digit()).count() > 0
}

fn looks_stale(receipt: &str) -> bool {
    let lower = receipt.to_ascii_lowercase();
    lower.contains("stale") || lower.contains("wrong-subject") || lower.contains("expired")
}

fn looks_not_run(receipt: &str) -> bool {
    let lower = receipt.to_ascii_lowercase();
    lower.contains("not_run")
        || lower.contains("not-run")
        || lower.contains("skipped")
        || lower.contains("cancelled")
}

fn looks_hir(receipt: &str) -> bool {
    let lower = receipt.to_ascii_lowercase();
    lower.contains("hir_snapshot")
        || lower.contains("hir-snapshot")
        || lower.contains("hir_statement_modifier")
        || lower.contains("parser.hir.")
        || lower.contains("postfix_modifiers")
        || lower.contains("statementmodifiershell")
        || lower.contains("postfixcondition")
        || lower.contains("/hir/")
        || lower.contains("\\hir\\")
        || lower.contains("flat-hir")
        || lower.contains("body-hir")
}

fn evaluate_closure_gate(
    view: &CapabilityView,
    ledger: &ConceptLedger,
    matrix: &ProofMatrix,
    docs: &[(&str, &str)],
) -> Result<String> {
    let closed = view.fully_closed()?;
    let blocker = view.narrowest_blocker()?;
    let mut report = String::new();
    writeln!(report, "concept: {}", view.concept_id)?;
    writeln!(report, "full_capability_closed: {closed}")?;
    for cell in &view.cells {
        writeln!(
            report,
            "cell {} required={} status={} reason={}",
            cell.id.as_str(),
            cell.id.required(),
            cell.status.as_str(),
            cell.reason
        )?;
    }
    if let Some(blocker) = blocker {
        writeln!(
            report,
            "narrowest_blocker: {} ({})",
            blocker.id.as_str(),
            blocker.status.as_str()
        )?;
    }

    let claims = completion_claims(view, ledger, matrix, docs)?;
    if closed {
        writeln!(report, "postfix capability closure: closed")?;
        return Ok(report);
    }
    if claims.is_empty() {
        writeln!(
            report,
            "postfix capability closure: not closed; no designated surface claims completion"
        )?;
        return Ok(report);
    }
    let blocker = blocker.ok_or_else(|| anyhow!("completion claimed without a derived blocker"))?;
    bail!(
        "postfix capability completion claimed while {} remains {}: {}\n{}",
        blocker.id.as_str(),
        blocker.status.as_str(),
        blocker.reason,
        claims.join("\n")
    );
}

fn completion_claims(
    view: &CapabilityView,
    ledger: &ConceptLedger,
    matrix: &ProofMatrix,
    docs: &[(&str, &str)],
) -> Result<Vec<String>> {
    let mut claims = Vec::new();
    if ledger.complete {
        claims.push("concept ledger complete=true".to_string());
    }
    if matrix.complete {
        claims.push("proof matrix complete=true".to_string());
    }
    let concept = ledger
        .concepts
        .iter()
        .find(|row| row.concept_id == CONCEPT_ID)
        .ok_or_else(|| anyhow!("missing {CONCEPT_ID}"))?;
    if concept.gold == "proven" && view.cell(CellId::Semantic)?.status != CellStatus::Proven {
        claims.push("ledger gold=proven is not derived from compatible semantic evidence".into());
    }
    if concept.oracle == "proven" && view.cell(CellId::Oracle)?.status != CellStatus::Proven {
        claims.push("ledger oracle=proven is not derived from compatible oracle evidence".into());
    }
    if concept.composition == "proven"
        && view.cell(CellId::Composition)?.status != CellStatus::Proven
    {
        claims.push(
            "ledger composition=proven is not derived from compatible composition evidence".into(),
        );
    }
    if matches!(concept.provider_eligibility.as_str(), "exact" | "qualified")
        && view.cell(CellId::Editor)?.status != CellStatus::Proven
    {
        claims.push(
            "ledger provider eligibility claims editor completion without derived proof".into(),
        );
    }
    if concept.gold == "proven"
        && concept.oracle == "proven"
        && concept.composition == "proven"
        && !view.fully_closed()?
    {
        claims.push(
            "ledger marks gold, oracle, and composition proven while full capability is open"
                .into(),
        );
    }
    for (name, source) in docs {
        claims.extend(doc_completion_claims(name, source, view)?);
    }
    Ok(claims)
}

fn doc_completion_claims(name: &str, source: &str, view: &CapabilityView) -> Result<Vec<String>> {
    let mut claims = Vec::new();
    for (index, line) in source.lines().enumerate() {
        if !line_mentions_postfix(line) {
            continue;
        }
        if let Some(overclaim) = table_overclaim(line, view)? {
            claims.push(format!("{name}:{} {overclaim}", index + 1));
        }
        if let Some(overclaim) = capability_status_table_overclaim(line) {
            claims.push(format!("{name}:{} {overclaim}", index + 1));
        }
        if positive_completion_phrase(line) {
            claims.push(format!(
                "{name}:{} claims postfix/statement-modifier completion: {}",
                index + 1,
                line.trim()
            ));
        }
    }
    Ok(claims)
}

fn capability_status_table_overclaim(line: &str) -> Option<String> {
    if !line.contains('|') || !line_mentions_postfix(line) {
        return None;
    }
    let cells: Vec<&str> = line.split('|').map(str::trim).filter(|part| !part.is_empty()).collect();
    if cells.len() < 2 {
        return None;
    }
    let capability = cells[0].trim_matches('`');
    let state = cells[1].trim_matches('`');
    if capability.eq_ignore_ascii_case("Capability") || capability.starts_with("---") {
        return None;
    }
    if state == "live" {
        return Some(format!(
            "capability table marks {capability} live without derived full-capability proof"
        ));
    }
    None
}

fn line_mentions_postfix(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("control.postfix_modifier")
        || lower.contains("postfix")
        || lower.contains("statement-modifier")
        || lower.contains("statement modifier")
}

fn positive_completion_phrase(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    if lower.contains("not complete")
        || lower.contains("incomplete")
        || lower.contains("complete: `false`")
        || lower.contains("inventory complete: `false`")
        || lower.contains("cannot")
        || lower.contains("remain")
        || lower.contains("missing")
    {
        return false;
    }
    [
        "is complete",
        "are complete",
        "fully supported",
        "full support",
        "capability closed",
        "umbrella is complete",
        "marked complete",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase))
}

fn table_overclaim(line: &str, view: &CapabilityView) -> Result<Option<String>> {
    if !line.contains("`control.postfix_modifier`") || !line.contains('|') {
        return Ok(None);
    }
    let cells: Vec<&str> = line.split('|').map(str::trim).filter(|part| !part.is_empty()).collect();
    // Concept, AST, Parser, Flat HIR, Body HIR, PIR-A, Effects, EIR, Gold, Oracle, Composition, Provider, Owner
    if cells.len() < 12 {
        return Ok(None);
    }
    let gold = cells[8].trim_matches('`');
    let oracle = cells[9].trim_matches('`');
    let composition = cells[10].trim_matches('`');
    let provider = cells[11].trim_matches('`');
    if gold == "proven" && view.cell(CellId::Semantic)?.status != CellStatus::Proven {
        return Ok(Some("table marks gold proven without derived semantic proof".into()));
    }
    if oracle == "proven" && view.cell(CellId::Oracle)?.status != CellStatus::Proven {
        return Ok(Some("table marks oracle proven without derived oracle proof".into()));
    }
    if composition == "proven" && view.cell(CellId::Composition)?.status != CellStatus::Proven {
        return Ok(Some("table marks composition proven without derived composition proof".into()));
    }
    if matches!(provider, "exact" | "qualified")
        && view.cell(CellId::Editor)?.status != CellStatus::Proven
    {
        return Ok(Some("table marks editor eligibility complete without derived proof".into()));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEDGER: &str = include_str!("../../../contracts/compiler/perl_compiler_concepts.v1.toml");
    const MATRIX: &str =
        include_str!("../../../contracts/compiler/perl_compiler_concept_proof.v1.toml");
    const CONCEPTS_DOC: &str =
        include_str!("../../../docs/project/status/perl_compiler_concepts.md");
    const PROOF_DOC: &str =
        include_str!("../../../docs/project/status/perl_compiler_concept_proof.md");
    const CAPABILITY_DOC: &str =
        include_str!("../../../docs/project/COMPILER_CAPABILITY_STATUS.md");

    fn apply_full_looking_hir(requirement: &mut Requirement) {
        requirement.hir_snapshot = Some(ProofStatus::Satisfied);
        requirement
            .evidence_by_class
            .insert("hir_snapshot".to_string(), full_looking_hir_receipts());
    }

    fn semantic_receipts() -> Vec<String> {
        vec!["receipt://control.postfix_modifier/gold/positive".to_string()]
    }

    fn pir_receipts() -> Vec<String> {
        vec!["receipt://control.postfix_modifier/pir/branch-edges".to_string()]
    }

    fn apply_compatible_full_capability(
        ledger: &mut ConceptLedger,
        matrix: &mut ProofMatrix,
    ) -> Result<()> {
        {
            let concept = postfix(ledger)?;
            concept.parser_ast = "parsed".to_string();
            concept.flat_hir = "modeled".to_string();
            concept.body_hir = "modeled".to_string();
            concept.pir_a = "modeled".to_string();
            concept.compile_effects_world = "not_applicable".to_string();
            concept.eir_profile = "executable".to_string();
            concept.gold = "proven".to_string();
            concept.oracle = "proven".to_string();
            concept.composition = "proven".to_string();
            concept.provider_eligibility = "qualified".to_string();
        }
        let requirement = postfix_req(matrix)?;
        apply_full_looking_hir(requirement);
        requirement.positive_gold = Some(ProofStatus::Satisfied);
        requirement.negative_gold = Some(ProofStatus::Satisfied);
        requirement.boundary_gold = Some(ProofStatus::Satisfied);
        requirement.recovery_gold = Some(ProofStatus::Satisfied);
        requirement.pir_snapshot = Some(ProofStatus::Satisfied);
        requirement.verifier_mutation = Some(ProofStatus::Satisfied);
        requirement.real_perl_oracle = Some(ProofStatus::Satisfied);
        requirement.composition_coverage = Some(ProofStatus::Satisfied);
        requirement.eir_differential = Some(ProofStatus::Satisfied);
        for class in ["positive_gold", "negative_gold", "boundary_gold", "recovery_gold"] {
            requirement.evidence_by_class.insert(class.to_string(), semantic_receipts());
        }
        requirement.evidence_by_class.insert("pir_snapshot".to_string(), pir_receipts());
        requirement.evidence_by_class.insert("verifier_mutation".to_string(), pir_receipts());
        requirement.evidence_by_class.insert(
            "real_perl_oracle".to_string(),
            vec!["receipt://control.postfix_modifier/oracle/real-perl".to_string()],
        );
        requirement.evidence_by_class.insert(
            "composition_coverage".to_string(),
            vec!["receipt://control.postfix_modifier/composition/loop-control".to_string()],
        );
        requirement.evidence_by_class.insert(
            "eir_differential".to_string(),
            vec!["receipt://control.postfix_modifier/eir/executable".to_string()],
        );
        Ok(())
    }

    fn full_looking_hir_receipts() -> Vec<String> {
        [
            "receipt://control.postfix_modifier/flat-hir/if",
            "receipt://control.postfix_modifier/flat-hir/unless",
            "receipt://control.postfix_modifier/flat-hir/while",
            "receipt://control.postfix_modifier/flat-hir/until",
            "receipt://control.postfix_modifier/flat-hir/for",
            "receipt://control.postfix_modifier/flat-hir/foreach",
            "receipt://control.postfix_modifier/body-hir/if",
            "receipt://control.postfix_modifier/body-hir/unless",
            "receipt://control.postfix_modifier/body-hir/while",
            "receipt://control.postfix_modifier/body-hir/until",
            "receipt://control.postfix_modifier/body-hir/for",
            "receipt://control.postfix_modifier/body-hir/foreach",
            "crates/perl-parser-core/tests/hir_statement_modifier_proof.rs",
            ".ci/parser-integration-targets.json#parser.hir.postfix_modifiers",
            ".ci/parser-integration-targets.lock.json",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }

    fn committed() -> Result<(ConceptLedger, ProofMatrix)> {
        Ok((toml::from_str(LEDGER)?, toml::from_str(MATRIX)?))
    }

    fn postfix<'a>(ledger: &'a mut ConceptLedger) -> Result<&'a mut ConceptRow> {
        ledger
            .concepts
            .iter_mut()
            .find(|row| row.concept_id == CONCEPT_ID)
            .ok_or_else(|| anyhow!("committed ledger missing {CONCEPT_ID}"))
    }

    fn postfix_req<'a>(matrix: &'a mut ProofMatrix) -> Result<&'a mut Requirement> {
        matrix
            .requirements
            .iter_mut()
            .find(|row| row.concept_id == CONCEPT_ID)
            .ok_or_else(|| anyhow!("committed matrix missing {CONCEPT_ID}"))
    }

    fn docs() -> [(&'static str, &'static str); 3] {
        [
            ("docs/project/status/perl_compiler_concepts.md", CONCEPTS_DOC),
            ("docs/project/status/perl_compiler_concept_proof.md", PROOF_DOC),
            ("docs/project/COMPILER_CAPABILITY_STATUS.md", CAPABILITY_DOC),
        ]
    }

    #[test]
    fn committed_state_is_open_and_does_not_claim_completion() -> Result<()> {
        let (ledger, matrix) = committed()?;
        let view = derive_postfix_capability(&ledger, &matrix)?;
        assert!(!view.fully_closed()?, "current postfix capability must not be closed");
        let report = evaluate_closure_gate(&view, &ledger, &matrix, &docs())?;
        assert!(report.contains("full_capability_closed: false"));
        assert!(report.contains("no designated surface claims completion"));
        let blocker = view.narrowest_blocker()?.ok_or_else(|| anyhow!("expected a blocker"))?;
        assert_eq!(blocker.id, CellId::FlatHir);
        assert_eq!(blocker.status, CellStatus::Missing);
        Ok(())
    }

    #[test]
    fn derived_view_emits_every_cell_including_explicit_deferred_and_not_applicable() -> Result<()>
    {
        let (ledger, matrix) = committed()?;
        let view = derive_postfix_capability(&ledger, &matrix)?;
        assert_eq!(view.cells.len(), CELL_ORDER.len());
        assert_eq!(view.cell(CellId::Execution)?.status, CellStatus::Deferred);
        assert_eq!(view.cell(CellId::Effects)?.status, CellStatus::NotApplicable);
        assert!(view.cell(CellId::Semantic)?.status.blocks_required_closure());
        Ok(())
    }

    #[test]
    fn silently_omitted_required_cell_fails_closed() -> Result<()> {
        let (ledger, matrix) = committed()?;
        let mut view = derive_postfix_capability(&ledger, &matrix)?;
        view.cells.retain(|cell| cell.id != CellId::Editor);
        let error = view.cell(CellId::Editor).expect_err("omitted editor cell must fail");
        assert!(error.to_string().contains("silently omitted"));
        Ok(())
    }

    #[test]
    fn hir_only_receipts_satisfy_only_hir_cells() -> Result<()> {
        let (mut ledger, mut matrix) = committed()?;
        postfix(&mut ledger)?.flat_hir = "modeled".to_string();
        postfix(&mut ledger)?.body_hir = "modeled".to_string();
        apply_full_looking_hir(postfix_req(&mut matrix)?);
        let view = derive_postfix_capability(&ledger, &matrix)?;
        assert_eq!(view.cell(CellId::FlatHir)?.status, CellStatus::Proven);
        assert_eq!(view.cell(CellId::BodyHir)?.status, CellStatus::Proven);
        assert_eq!(view.cell(CellId::Semantic)?.status, CellStatus::Missing);
        assert_eq!(view.cell(CellId::Pir)?.status, CellStatus::Missing);
        assert_eq!(view.cell(CellId::Oracle)?.status, CellStatus::Missing);
        assert_eq!(view.cell(CellId::Composition)?.status, CellStatus::Missing);
        assert_eq!(view.cell(CellId::Execution)?.status, CellStatus::Deferred);
        assert_eq!(view.cell(CellId::Editor)?.status, CellStatus::Missing);
        Ok(())
    }

    #[test]
    fn full_looking_hir_receipt_set_fails_full_capability_closure() -> Result<()> {
        let (ledger, mut matrix) = committed()?;
        apply_full_looking_hir(postfix_req(&mut matrix)?);
        let view = derive_postfix_capability(&ledger, &matrix)?;
        assert!(!view.fully_closed()?);
        let blocker = view.narrowest_blocker()?.ok_or_else(|| anyhow!("expected blocker"))?;
        assert_eq!(blocker.id, CellId::Semantic, "narrowest remaining layer must be semantic");
        assert_eq!(blocker.status, CellStatus::Missing);
        Ok(())
    }

    #[test]
    fn hir_receipts_cannot_satisfy_pir_or_semantic_cells() -> Result<()> {
        let (mut ledger, mut matrix) = committed()?;
        {
            let concept = postfix(&mut ledger)?;
            concept.pir_a = "modeled".to_string();
            concept.gold = "proven".to_string();
        }
        {
            let requirement = postfix_req(&mut matrix)?;
            apply_full_looking_hir(requirement);
            requirement.pir_snapshot = Some(ProofStatus::Satisfied);
            requirement.verifier_mutation = Some(ProofStatus::Satisfied);
            requirement.positive_gold = Some(ProofStatus::Satisfied);
            requirement.negative_gold = Some(ProofStatus::Satisfied);
            requirement.boundary_gold = Some(ProofStatus::Satisfied);
            requirement.recovery_gold = Some(ProofStatus::Satisfied);
            requirement
                .evidence_by_class
                .insert("pir_snapshot".to_string(), full_looking_hir_receipts());
            requirement
                .evidence_by_class
                .insert("verifier_mutation".to_string(), full_looking_hir_receipts());
            requirement
                .evidence_by_class
                .insert("positive_gold".to_string(), full_looking_hir_receipts());
            requirement
                .evidence_by_class
                .insert("negative_gold".to_string(), full_looking_hir_receipts());
            requirement
                .evidence_by_class
                .insert("boundary_gold".to_string(), full_looking_hir_receipts());
            requirement
                .evidence_by_class
                .insert("recovery_gold".to_string(), full_looking_hir_receipts());
        }
        let view = derive_postfix_capability(&ledger, &matrix)?;
        assert_eq!(view.cell(CellId::Pir)?.status, CellStatus::Rejected);
        assert_eq!(view.cell(CellId::Semantic)?.status, CellStatus::Rejected);
        assert!(view.cell(CellId::Pir)?.reason.contains("hir"));
        Ok(())
    }

    #[test]
    fn issue_closure_and_checkboxes_cannot_satisfy_cells() -> Result<()> {
        let (mut ledger, mut matrix) = committed()?;
        postfix(&mut ledger)?.gold = "proven".to_string();
        {
            let requirement = postfix_req(&mut matrix)?;
            requirement.positive_gold = Some(ProofStatus::Satisfied);
            requirement.negative_gold = Some(ProofStatus::Satisfied);
            requirement.boundary_gold = Some(ProofStatus::Satisfied);
            requirement.recovery_gold = Some(ProofStatus::Satisfied);
            for class in ["positive_gold", "negative_gold", "boundary_gold", "recovery_gold"] {
                requirement.evidence_by_class.insert(
                    class.to_string(),
                    vec!["#4886".to_string(), "Closes #13281".to_string(), "[x] done".to_string()],
                );
            }
        }
        let view = derive_postfix_capability(&ledger, &matrix)?;
        assert_eq!(view.cell(CellId::Semantic)?.status, CellStatus::Rejected);
        assert!(
            view.cell(CellId::Semantic)?.reason.contains("issue_prose")
                || view.cell(CellId::Semantic)?.reason.contains("checkbox")
        );
        Ok(())
    }

    #[test]
    fn github_closing_keywords_cannot_satisfy_semantic_cells() -> Result<()> {
        for receipt in [
            "Fixes #13281",
            "Fixed #13281",
            "Fix #13281",
            "Resolves #13281",
            "Resolved #13281",
            "This PR closes #13281",
            "close:#4886",
        ] {
            let (mut ledger, mut matrix) = committed()?;
            postfix(&mut ledger)?.gold = "proven".to_string();
            {
                let requirement = postfix_req(&mut matrix)?;
                requirement.positive_gold = Some(ProofStatus::Satisfied);
                requirement.negative_gold = Some(ProofStatus::Satisfied);
                requirement.boundary_gold = Some(ProofStatus::Satisfied);
                requirement.recovery_gold = Some(ProofStatus::Satisfied);
                for class in ["positive_gold", "negative_gold", "boundary_gold", "recovery_gold"] {
                    requirement
                        .evidence_by_class
                        .insert(class.to_string(), vec![receipt.to_string()]);
                }
            }
            let view = derive_postfix_capability(&ledger, &matrix)?;
            assert_eq!(
                view.cell(CellId::Semantic)?.status,
                CellStatus::Rejected,
                "{receipt} must be issue prose"
            );
            assert!(
                view.cell(CellId::Semantic)?.reason.contains("issue_prose"),
                "{receipt}: {}",
                view.cell(CellId::Semantic)?.reason
            );
        }
        Ok(())
    }

    #[test]
    fn stale_and_not_run_required_layers_block_with_their_status() -> Result<()> {
        let (mut ledger, mut matrix) = committed()?;
        postfix(&mut ledger)?.oracle = "proven".to_string();
        {
            let requirement = postfix_req(&mut matrix)?;
            requirement.real_perl_oracle = Some(ProofStatus::Satisfied);
            requirement.evidence_by_class.insert(
                "real_perl_oracle".to_string(),
                vec!["receipt://oracle/stale-wrong-subject".to_string()],
            );
        }
        let view = derive_postfix_capability(&ledger, &matrix)?;
        assert_eq!(view.cell(CellId::Oracle)?.status, CellStatus::Stale);

        let (ledger, mut matrix) = committed()?;
        {
            let requirement = postfix_req(&mut matrix)?;
            requirement.composition_coverage = Some(ProofStatus::RequiredMissing);
            requirement.evidence_by_class.insert(
                "composition_coverage".to_string(),
                vec!["receipt://composition/not_run".to_string()],
            );
        }
        let view = derive_postfix_capability(&ledger, &matrix)?;
        assert_eq!(view.cell(CellId::Composition)?.status, CellStatus::NotRun);
        Ok(())
    }

    #[test]
    fn completion_claim_in_docs_fails_with_narrowest_reason() -> Result<()> {
        let (ledger, mut matrix) = committed()?;
        apply_full_looking_hir(postfix_req(&mut matrix)?);
        let view = derive_postfix_capability(&ledger, &matrix)?;
        let docs = [(
            "docs/project/status/perl_compiler_concepts.md",
            "Postfix statement modifiers are fully supported.\n",
        )];
        let error = evaluate_closure_gate(&view, &ledger, &matrix, &docs)
            .expect_err("doc completion claim must fail");
        let message = error.to_string();
        assert!(message.contains("semantic"), "narrowest reason missing: {message}");
        assert!(message.contains("missing"), "{message}");
        assert!(message.contains("fully supported"), "{message}");
        Ok(())
    }

    #[test]
    fn capability_table_live_row_fails_with_narrowest_reason() -> Result<()> {
        let (ledger, mut matrix) = committed()?;
        apply_full_looking_hir(postfix_req(&mut matrix)?);
        let view = derive_postfix_capability(&ledger, &matrix)?;
        let docs = [(
            "docs/project/COMPILER_CAPABILITY_STATUS.md",
            "| Capability | State | Owner issue | Evidence | Next proof |\n\
             | --- | --- | --- | --- | --- |\n\
             | Postfix statement modifiers | `live` | #4886 | hir | none |\n\
             | Provider cutover | `partial live` | #8197 | evidence | gated |\n",
        )];
        let error = evaluate_closure_gate(&view, &ledger, &matrix, &docs)
            .expect_err("live postfix capability row must fail");
        let message = error.to_string();
        assert!(message.contains("semantic"), "narrowest reason missing: {message}");
        assert!(message.contains("live"), "{message}");
        assert!(
            !message.contains("Provider cutover"),
            "partial live on an unrelated row must not be a postfix completion claim: {message}"
        );
        Ok(())
    }

    #[test]
    fn ledger_gold_proven_with_hir_only_evidence_is_a_completion_claim() -> Result<()> {
        let (mut ledger, mut matrix) = committed()?;
        postfix(&mut ledger)?.gold = "proven".to_string();
        apply_full_looking_hir(postfix_req(&mut matrix)?);
        let view = derive_postfix_capability(&ledger, &matrix)?;
        let error = evaluate_closure_gate(&view, &ledger, &matrix, &[])
            .expect_err("gold=proven without semantic evidence must fail");
        assert!(error.to_string().contains("gold=proven"));
        Ok(())
    }

    #[test]
    fn honest_full_capability_allows_completion_claims() -> Result<()> {
        let (mut ledger, mut matrix) = committed()?;
        apply_compatible_full_capability(&mut ledger, &mut matrix)?;
        let view = derive_postfix_capability(&ledger, &matrix)?;
        assert!(view.fully_closed()?, "compatible non-HIR evidence must close required cells");
        assert_eq!(view.cell(CellId::Semantic)?.status, CellStatus::Proven);
        assert_eq!(view.cell(CellId::Pir)?.status, CellStatus::Proven);
        assert_eq!(view.cell(CellId::Oracle)?.status, CellStatus::Proven);
        assert_eq!(view.cell(CellId::Composition)?.status, CellStatus::Proven);
        assert_eq!(view.cell(CellId::Execution)?.status, CellStatus::Proven);
        assert_eq!(view.cell(CellId::Editor)?.status, CellStatus::Proven);
        assert_eq!(view.cell(CellId::Effects)?.status, CellStatus::NotApplicable);
        let docs = [(
            "docs/project/status/perl_compiler_concepts.md",
            "Postfix statement modifiers are fully supported.\n",
        )];
        let report = evaluate_closure_gate(&view, &ledger, &matrix, &docs)?;
        assert!(report.contains("full_capability_closed: true"), "{report}");
        assert!(report.contains("postfix capability closure: closed"), "{report}");
        Ok(())
    }
}
