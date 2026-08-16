use super::rows::canonical_rows;
use super::sources::inspect_sources;
use super::{
    CheckRow, Disposition, Finding, Inventory, MutationPosture, ResultClass, SCHEMA, SourceDigest,
    sha256_hex,
};
use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Serialize)]
struct InventoryBody<'a> {
    schema: u32,
    status: &'a str,
    doctor_check_headings: &'a [super::DoctorHeading],
    rows: &'a [CheckRow],
    findings: &'a [Finding],
    active_mutations: &'a [super::ActiveMutation],
    sources: &'a BTreeMap<String, SourceDigest>,
}

pub fn build_inventory(root: &Path) -> Result<Inventory> {
    let source_facts = inspect_sources(root)?;
    let rows = canonical_rows();
    validate_rows(&rows)?;
    let findings = current_findings();
    let status = "DEBT_INVENTORIED".to_string();
    let inventory_digest = inventory_digest(
        &status,
        &source_facts.headings,
        &rows,
        &findings,
        &source_facts.active_mutations,
        &source_facts.sources,
    )?;
    Ok(Inventory {
        schema: SCHEMA,
        status,
        doctor_check_headings: source_facts.headings,
        rows,
        findings,
        active_mutations: source_facts.active_mutations,
        sources: source_facts.sources,
        inventory_digest,
    })
}

pub fn validate_inventory(root: &Path, inventory: &Inventory) -> Result<()> {
    let current = build_inventory(root)?;
    if inventory != &current {
        bail!("workspace doctor inventory is stale or contradictory");
    }
    Ok(())
}

pub fn validate_rows(rows: &[CheckRow]) -> Result<()> {
    if rows.len() != 22 {
        bail!("workspace doctor inventory must contain 22 rows; found {}", rows.len());
    }
    let mut ids = BTreeSet::new();
    let mut facts = BTreeSet::new();
    for row in rows {
        if !ids.insert(row.check_id.as_str()) {
            bail!("duplicate check_id {}", row.check_id);
        }
        if !facts.insert(row.fact_key.as_str()) {
            bail!("fact {} has multiple canonical rows", row.fact_key);
        }
        if row.current_mutation == MutationPosture::AutomaticMutation
            && row.disposition != Disposition::MoveToExplicitRepair
        {
            bail!("automatic mutation {} is not moved to explicit repair", row.check_id);
        }
    }
    require_row(
        rows,
        "core-bare",
        MutationPosture::AutomaticMutation,
        ResultClass::RepairAvailable,
        Disposition::MoveToExplicitRepair,
    )?;
    require_row(
        rows,
        "worktree-file-overlap",
        MutationPosture::ReadOnly,
        ResultClass::Blocked,
        Disposition::ReuseAuthority,
    )?;
    require_row(
        rows,
        "orphaned-worktree-directory",
        MutationPosture::ReadOnly,
        ResultClass::NotProven,
        Disposition::NotProven,
    )?;
    require_row(
        rows,
        "branch-worktree-collision",
        MutationPosture::NotObserved,
        ResultClass::BlockedOrNotProven,
        Disposition::ReuseAuthority,
    )?;
    require_row(
        rows,
        "default-base-behind",
        MutationPosture::ReadOnly,
        ResultClass::Advisory,
        Disposition::ReviseSemantics,
    )?;
    require_row(
        rows,
        "default-base-unresolved",
        MutationPosture::ReadOnly,
        ResultClass::NotProven,
        Disposition::ReuseAuthority,
    )?;
    require_row(
        rows,
        "doctor-aggregate-exit",
        MutationPosture::InheritsDoctorMutation,
        ResultClass::BlockedOrNotProven,
        Disposition::ReviseSemantics,
    )?;
    require_row(
        rows,
        "ready-composition",
        MutationPosture::InheritsDoctorMutation,
        ResultClass::TypedReadiness,
        Disposition::ReviseSemantics,
    )?;
    Ok(())
}

pub fn render_human(inventory: &Inventory) -> String {
    let mut lines = vec![
        format!("workspace-doctor-inventory: {}", inventory.status),
        format!("doctor checks: {}", inventory.doctor_check_headings.len()),
        format!("inventory rows: {}", inventory.rows.len()),
        format!("active mutations: {}", inventory.active_mutations.len()),
        format!("findings: {}", inventory.findings.len()),
    ];
    for finding in &inventory.findings {
        lines.push(format!(
            "{} [{}]: {} -> {}",
            finding.finding_id, finding.check_id, finding.current, finding.required_disposition
        ));
    }
    lines.join("\n")
}

fn require_row(
    rows: &[CheckRow],
    check_id: &str,
    mutation: MutationPosture,
    result: ResultClass,
    disposition: Disposition,
) -> Result<()> {
    let row = rows
        .iter()
        .find(|row| row.check_id == check_id)
        .ok_or_else(|| anyhow::anyhow!("required row {check_id} is missing"))?;
    if row.current_mutation != mutation
        || row.target_result != result
        || row.disposition != disposition
    {
        bail!("required row {check_id} has an invalid classification");
    }
    Ok(())
}

fn current_findings() -> Vec<Finding> {
    vec![
        finding(
            "AUTO_MUTATION_IN_DIAGNOSIS",
            "core-bare",
            "automatic_mutation",
            "MOVE_TO_EXPLICIT_REPAIR",
        ),
        finding(
            "REQUIRED_FINDINGS_EXIT_ZERO",
            "doctor-aggregate-exit",
            "always_exit_0_after_findings",
            "REVISE_SEMANTICS",
        ),
        finding(
            "READY_AFTER_UNRESOLVED_DOCTOR",
            "ready-composition",
            "doctor exit zero can satisfy dependency",
            "REVISE_SEMANTICS",
        ),
        finding(
            "BEHIND_ONLY_BRANCH_MOVEMENT",
            "default-base-behind",
            "prescribes git pull --ff-only",
            "REVISE_SEMANTICS",
        ),
        finding(
            "UNTRACKED_STATE_OMITTED",
            "workspace-untracked",
            "--untracked-files=no",
            "REVISE_SEMANTICS",
        ),
        finding(
            "ADMISSION_VERDICT_EXIT_ZERO",
            "branch-worktree-collision",
            "writer-admission returns a typed verdict but the command always exits zero",
            "CONSUME_TYPED_REPORT",
        ),
        finding(
            "WORKTREE_DRY_RUN_PRUNES_METADATA",
            "orphaned-worktree-directory",
            "worktree-cleanup invokes git worktree prune before dry-run classification",
            "DO_NOT_INVOKE_FROM_READ_ONLY_DOCTOR",
        ),
    ]
}

fn finding(id: &str, check_id: &str, current: &str, required: &str) -> Finding {
    Finding {
        finding_id: id.to_string(),
        check_id: check_id.to_string(),
        current: current.to_string(),
        required_disposition: required.to_string(),
    }
}

fn inventory_digest(
    status: &str,
    headings: &[super::DoctorHeading],
    rows: &[CheckRow],
    findings: &[Finding],
    mutations: &[super::ActiveMutation],
    sources: &BTreeMap<String, SourceDigest>,
) -> Result<String> {
    let body = InventoryBody {
        schema: SCHEMA,
        status,
        doctor_check_headings: headings,
        rows,
        findings,
        active_mutations: mutations,
        sources,
    };
    let bytes = serde_json::to_vec(&body)?;
    Ok(sha256_hex(&bytes))
}
