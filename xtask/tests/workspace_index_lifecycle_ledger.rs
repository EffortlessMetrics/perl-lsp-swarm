//! Checked index-lifecycle proposition and ownership denominator for issue #10433.
//!
//! `policy/workspace-index-lifecycle-propositions.v1.tsv` is the machine authority
//! that selects exactly one owner for each index lifecycle proposition across the
//! three overlapping `perl-workspace` vocabularies (the operational
//! `workspace_index` coordinator, the separate `state_machine`, and the
//! `monitoring` instrumentation types).
//!
//! This ledger decides ownership only. It changes no indexing behavior, readiness
//! result, provider routing, snapshot publication, resource policy or public API.
//!
//! Regenerate the reviewer projection with:
//!
//! ```text
//! WSI_LIFECYCLE_MAP_UPDATE=1 cargo test -p xtask --locked --test workspace_index_lifecycle_ledger
//! ```
//!
//! Running that command twice must produce no second diff.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};

const LEDGER_PATH: &str = "policy/workspace-index-lifecycle-propositions.v1.tsv";
const GENERATED_PATH: &str = "docs/generated/workspace_index_lifecycle_propositions.md";
const RUNTIME_OWNERSHIP_PATH: &str = "policy/workspace-runtime-ownership.v1.tsv";
const UPDATE_ENV: &str = "WSI_LIFECYCLE_MAP_UPDATE";
const COLUMN_COUNT: usize = 18;

/// Source files whose index lifecycle declarations must all be mapped.
const COVERED_SOURCE_DIRS: &[&str] = &[
    "crates/perl-workspace/src/monitoring/mod.rs",
    "crates/perl-workspace/src/state_machine/mod.rs",
    "crates/perl-workspace/src/workspace/workspace_index.rs",
];

/// Lifecycle type names whose declarations require a ledger row.
const COVERED_TYPE_NAMES: &[&str] = &[
    "IndexState",
    "IndexStateMachine",
    "IndexStateKind",
    "IndexPhase",
    "IndexCoordinator",
    "DegradationReason",
    "ResourceKind",
    "BuildPhase",
    "IndexStateTransition",
    "IndexResourceLimits",
    "IndexPerformanceCaps",
    "InvalidationReason",
    "TransitionResult",
    "BuildPhaseTransition",
];

const ALLOWED_STATES: &[&str] = &["live", "absent_on_main", "doctrine_only"];

const ALLOWED_FAMILIES: &[&str] = &[
    "root_currentness",
    "operation_identity",
    "work_activity",
    "scan_kind",
    "snapshot_availability",
    "provider_eligibility",
    "update_accounting",
    "settlement",
    "resource_pressure",
    "degradation",
    "instrumentation",
    "observation_api",
];

const ALLOWED_REACHABILITY: &[&str] =
    &["production", "test_or_doctest_only", "doctrine_only", "public_surface_only", "absent"];

const ALLOWED_AUTHORITY_KINDS: &[&str] = &["semantic", "telemetry", "projection", "doctrine"];

const ALLOWED_PUBLIC_COMMITMENTS: &[&str] =
    &["short_path_public", "nested_public", "crate_internal", "none"];

const ALLOWED_DISPOSITIONS: &[&str] = &[
    "canonical_lifecycle_state",
    "canonical_operation_settlement",
    "canonical_payload_of_owner",
    "telemetry_projection_only",
    "provider_readiness_projection_only",
    "compatibility_forwarder_with_exit",
    "public_compatibility_requires_semver_decision",
    "test_fixture_only",
    "retire_duplicate_or_dead",
    "blocked_on_root_generation",
    "blocked_on_workspace_snapshot",
];

/// Dispositions that select the single owner of a proposition family.
const OWNER_DISPOSITIONS: &[&str] = &[
    "canonical_lifecycle_state",
    "canonical_operation_settlement",
    "provider_readiness_projection_only",
    "blocked_on_root_generation",
    "blocked_on_workspace_snapshot",
];

/// Dispositions that assert semantic lifecycle or readiness authority. A
/// telemetry type may never carry one of these.
const SEMANTIC_AUTHORITY_DISPOSITIONS: &[&str] = &[
    "canonical_lifecycle_state",
    "canonical_operation_settlement",
    "canonical_payload_of_owner",
    "provider_readiness_projection_only",
    "blocked_on_root_generation",
    "blocked_on_workspace_snapshot",
];

/// Dispositions for a row that disappears or forwards, which may therefore name
/// no owner.
const OWNERLESS_DISPOSITIONS: &[&str] = &[
    "retire_duplicate_or_dead",
    "test_fixture_only",
    "public_compatibility_requires_semver_decision",
    "compatibility_forwarder_with_exit",
    "telemetry_projection_only",
];

/// Live implementation successors named by #10433. `#10821` is deliberately
/// absent: it was closed as not planned, so no row may route work to it.
const ALLOWED_SUCCESSORS: &[&str] = &["#10784", "#10791", "#10799", "#10811", "#10828"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct PropositionRow {
    id: String,
    current_state: String,
    family: String,
    proposition: String,
    current_type: String,
    reachability: String,
    current_identity: String,
    required_target_identity: String,
    identity_sufficient_today: String,
    transition_owner: String,
    authority_kind: String,
    public_commitment: String,
    disposition: String,
    owner_row: String,
    runtime_ownership_row: String,
    successor_issue: String,
    source_path: String,
    source_marker: String,
}

impl PropositionRow {
    fn is_owner(&self) -> bool {
        self.owner_row == "self"
    }
}

fn repo_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live immediately beneath the repository root")
}

fn parse_ledger(source: &str) -> Result<Vec<PropositionRow>> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|(index, line)| parse_row(index + 1, line))
        .collect()
}

fn parse_row(line_number: usize, line: &str) -> Result<PropositionRow> {
    let columns: Vec<&str> = line.split('|').collect();
    ensure!(
        columns.len() == COLUMN_COUNT,
        "{LEDGER_PATH}:{line_number}: expected {COLUMN_COUNT} columns, found {}",
        columns.len()
    );

    Ok(PropositionRow {
        id: columns[0].to_string(),
        current_state: columns[1].to_string(),
        family: columns[2].to_string(),
        proposition: columns[3].to_string(),
        current_type: columns[4].to_string(),
        reachability: columns[5].to_string(),
        current_identity: columns[6].to_string(),
        required_target_identity: columns[7].to_string(),
        identity_sufficient_today: columns[8].to_string(),
        transition_owner: columns[9].to_string(),
        authority_kind: columns[10].to_string(),
        public_commitment: columns[11].to_string(),
        disposition: columns[12].to_string(),
        owner_row: columns[13].to_string(),
        runtime_ownership_row: columns[14].to_string(),
        successor_issue: columns[15].to_string(),
        source_path: columns[16].to_string(),
        source_marker: columns[17].to_string(),
    })
}

fn load_rows() -> Result<Vec<PropositionRow>> {
    let root = repo_root()?;
    let source = fs::read_to_string(root.join(LEDGER_PATH))
        .with_context(|| format!("read {LEDGER_PATH}"))?;
    let rows = parse_ledger(&source)?;
    ensure!(!rows.is_empty(), "{LEDGER_PATH} contains no proposition rows");
    Ok(rows)
}

/// Identities that cannot, alone, stand in for root generation, operation
/// identity, accepted snapshot generation, or provider readiness evidence.
fn identity_is_insufficient(identity: &str) -> bool {
    let normalized = identity.trim().to_ascii_lowercase();
    const INSUFFICIENT: &[&str] = &[
        "path",
        "uri",
        "document uri",
        "pending count",
        "pending counter",
        "counter",
        "count",
        "elapsed time",
        "wall-clock time",
        "instant",
        "timestamp",
        "non-empty index",
        "successful parse",
        "enum variant",
        "boolean",
        "a single process-wide boolean",
    ];
    INSUFFICIENT.contains(&normalized.as_str())
}

fn validate_rows(rows: &[PropositionRow]) -> Result<()> {
    ensure!(!rows.is_empty(), "proposition denominator must not be empty");
    let mut ids = BTreeSet::new();

    for row in rows {
        ensure!(row.id.starts_with("WSI-"), "{}: invalid stable row ID", row.id);
        ensure!(ids.insert(row.id.as_str()), "duplicate proposition row {}", row.id);

        for (field, value, allowed) in [
            ("current_state", &row.current_state, ALLOWED_STATES),
            ("family", &row.family, ALLOWED_FAMILIES),
            ("reachability", &row.reachability, ALLOWED_REACHABILITY),
            ("authority_kind", &row.authority_kind, ALLOWED_AUTHORITY_KINDS),
            ("public_commitment", &row.public_commitment, ALLOWED_PUBLIC_COMMITMENTS),
            ("disposition", &row.disposition, ALLOWED_DISPOSITIONS),
        ] {
            ensure!(allowed.contains(&value.as_str()), "{}: unknown {field} {value:?}", row.id);
        }

        ensure!(
            matches!(row.identity_sufficient_today.as_str(), "true" | "false" | "not_applicable"),
            "{}: identity_sufficient_today must be true, false or not_applicable",
            row.id
        );

        for (field, value) in [
            ("proposition", &row.proposition),
            ("current_type", &row.current_type),
            ("current_identity", &row.current_identity),
            ("required_target_identity", &row.required_target_identity),
            ("transition_owner", &row.transition_owner),
        ] {
            ensure!(!value.trim().is_empty(), "{}: proposition field {field} is empty", row.id);
        }

        // Every row names a live implementation successor. No unnamed
        // future-owner bucket is permitted.
        ensure!(
            ALLOWED_SUCCESSORS.contains(&row.successor_issue.as_str()),
            "{}: successor {:?} is not a live implementation owner; allowed: {ALLOWED_SUCCESSORS:?}",
            row.id,
            row.successor_issue
        );

        // A telemetry type may never carry semantic lifecycle or readiness
        // authority.
        if row.authority_kind == "telemetry" {
            ensure!(
                !SEMANTIC_AUTHORITY_DISPOSITIONS.contains(&row.disposition.as_str()),
                "{}: a telemetry type cannot hold semantic lifecycle or readiness authority ({})",
                row.id,
                row.disposition
            );
        }

        // A completion counter is never sufficient for a lifecycle transition.
        if row.transition_owner.to_ascii_lowercase().contains("counter") {
            ensure!(
                !OWNER_DISPOSITIONS.contains(&row.disposition.as_str()),
                "{}: a counter-driven transition cannot own a lifecycle proposition",
                row.id
            );
        }

        // Owner selection.
        if OWNER_DISPOSITIONS.contains(&row.disposition.as_str()) {
            ensure!(row.is_owner(), "{}: an owner disposition must declare owner_row=self", row.id);
        }
        if row.is_owner() {
            ensure!(
                OWNER_DISPOSITIONS.contains(&row.disposition.as_str()),
                "{}: owner_row=self requires an owner disposition, found {}",
                row.id,
                row.disposition
            );
            ensure!(
                !identity_is_insufficient(&row.required_target_identity),
                "{}: {:?} alone cannot be the target identity of a selected owner",
                row.id,
                row.required_target_identity
            );
            if row.identity_sufficient_today == "false" {
                ensure!(
                    !row.successor_issue.is_empty(),
                    "{}: an owner whose identity is insufficient today needs a cutover successor",
                    row.id
                );
            }
        } else if row.owner_row == "none" {
            ensure!(
                OWNERLESS_DISPOSITIONS.contains(&row.disposition.as_str()),
                "{}: disposition {} must name the owner that absorbs it",
                row.id,
                row.disposition
            );
        }

        // A public compatibility path needs a forwarding or removal decision.
        if row.public_commitment != "none" && row.reachability == "public_surface_only" {
            ensure!(
                matches!(
                    row.disposition.as_str(),
                    "compatibility_forwarder_with_exit"
                        | "public_compatibility_requires_semver_decision"
                        | "retire_duplicate_or_dead"
                        | "canonical_payload_of_owner"
                ),
                "{}: a public compatibility path needs a forwarding, semver or removal decision",
                row.id
            );
        }

        match row.current_state.as_str() {
            "live" => {
                ensure!(
                    !row.source_path.is_empty() && !row.source_marker.is_empty(),
                    "{}: live rows require an exact source path and marker",
                    row.id
                );
                ensure!(
                    row.reachability != "absent",
                    "{}: a live row cannot be absent from the tree",
                    row.id
                );
            }
            "absent_on_main" => {
                ensure!(
                    row.source_path.is_empty() && row.source_marker.is_empty(),
                    "{}: an absent proposition cannot cite live source reachability",
                    row.id
                );
                ensure!(
                    row.reachability == "absent",
                    "{}: an absent proposition must declare absent reachability",
                    row.id
                );
            }
            "doctrine_only" => {
                ensure!(
                    row.reachability == "doctrine_only",
                    "{}: a doctrine row must declare doctrine_only reachability",
                    row.id
                );
            }
            value => bail!("{}: unhandled current state {value}", row.id),
        }
    }

    validate_owner_selection(rows)?;
    validate_name_collisions(rows)?;
    validate_family_target_identities(rows)
}

/// Exactly one owner per proposition family, and never two canonical owners.
fn validate_owner_selection(rows: &[PropositionRow]) -> Result<()> {
    let by_id: BTreeMap<&str, &PropositionRow> =
        rows.iter().map(|row| (row.id.as_str(), row)).collect();
    let mut owners: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut families_with_semantic_rows: BTreeSet<&str> = BTreeSet::new();

    for row in rows {
        if row.is_owner() {
            owners.entry(row.family.as_str()).or_default().push(row.id.as_str());
        }
        if row.authority_kind == "semantic" {
            families_with_semantic_rows.insert(row.family.as_str());
        }
        if !row.is_owner() && row.owner_row != "none" {
            let owner = by_id.get(row.owner_row.as_str()).with_context(|| {
                format!("{}: owner_row {} does not exist", row.id, row.owner_row)
            })?;
            ensure!(
                owner.is_owner(),
                "{}: owner_row {} is not itself a selected owner",
                row.id,
                row.owner_row
            );
            ensure!(
                owner.family == row.family,
                "{}: owner_row {} belongs to family {}, not {}",
                row.id,
                row.owner_row,
                owner.family,
                row.family
            );
        }
    }

    for (family, ids) in &owners {
        ensure!(
            ids.len() == 1,
            "family {family} has {} canonical owners ({ids:?}); exactly one is allowed",
            ids.len()
        );
    }

    for family in families_with_semantic_rows {
        ensure!(
            owners.contains_key(family),
            "family {family} carries semantic lifecycle authority but selects no owner"
        );
    }

    Ok(())
}

/// Types are never unified because their names resemble each other: a bare type
/// name shared by several rows may support at most one selected owner.
fn validate_name_collisions(rows: &[PropositionRow]) -> Result<()> {
    let mut by_name: BTreeMap<String, Vec<&PropositionRow>> = BTreeMap::new();
    for row in rows {
        if let Some(name) = declared_type_name(&row.source_marker) {
            by_name.entry(name).or_default().push(row);
        }
    }

    for (name, rows) in by_name {
        let owners: Vec<&str> =
            rows.iter().filter(|row| row.is_owner()).map(|row| row.id.as_str()).collect();
        ensure!(
            owners.len() <= 1,
            "type name {name} has {} selected owners ({owners:?}); a shared name is not shared authority",
            owners.len()
        );
    }
    Ok(())
}

/// `Building` needs an operation identity and `Ready` needs an accepted snapshot
/// subject before either can be target-defined.
fn validate_family_target_identities(rows: &[PropositionRow]) -> Result<()> {
    for (family, required) in
        [("work_activity", "operation ticket"), ("snapshot_availability", "snapshot")]
    {
        let owner = rows
            .iter()
            .find(|row| row.family == family && row.is_owner())
            .with_context(|| format!("family {family} selects no owner"))?;
        ensure!(
            owner.required_target_identity.to_ascii_lowercase().contains(required),
            "{}: the {family} owner must require {required:?} in its target identity",
            owner.id
        );
    }
    Ok(())
}

fn validate_live_source_markers(rows: &[PropositionRow]) -> Result<()> {
    let root = repo_root()?;
    for row in rows.iter().filter(|row| !row.source_path.is_empty()) {
        let source = fs::read_to_string(root.join(&row.source_path))
            .with_context(|| format!("read live source {}", row.source_path))?;
        ensure!(
            source.contains(&row.source_marker),
            "{}: live marker {:?} not found in {}",
            row.id,
            row.source_marker,
            row.source_path
        );
    }
    Ok(())
}

/// Every `WRT-` row referenced must exist in the #10011 root-operation ledger.
/// The map joins that authority; it never duplicates its rows.
fn validate_runtime_ownership_join(rows: &[PropositionRow]) -> Result<()> {
    let root = repo_root()?;
    let source = fs::read_to_string(root.join(RUNTIME_OWNERSHIP_PATH))
        .with_context(|| format!("read {RUNTIME_OWNERSHIP_PATH}"))?;
    let known: BTreeSet<&str> = source
        .lines()
        .filter(|line| line.starts_with("WRT-"))
        .filter_map(|line| line.split('|').next())
        .collect();

    for row in rows.iter().filter(|row| !row.runtime_ownership_row.is_empty()) {
        ensure!(
            known.contains(row.runtime_ownership_row.as_str()),
            "{}: runtime ownership row {} is not present in {RUNTIME_OWNERSHIP_PATH}",
            row.id,
            row.runtime_ownership_row
        );
    }
    Ok(())
}

fn declared_type_name(marker: &str) -> Option<String> {
    let rest = marker.strip_prefix("pub enum ").or_else(|| marker.strip_prefix("pub struct "))?;
    let name = rest.split(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or_default();
    (!name.is_empty()).then(|| name.to_string())
}

/// Every index lifecycle declaration in the covered modules must be mapped. A new
/// lifecycle enum, state machine or coordinator cannot appear without a row.
fn declared_lifecycle_sites() -> Result<BTreeSet<(String, String)>> {
    let root = repo_root()?;
    let mut sites = BTreeSet::new();
    for path in COVERED_SOURCE_DIRS {
        let source = fs::read_to_string(root.join(path)).with_context(|| format!("read {path}"))?;
        for line in source.lines() {
            if !line.starts_with("pub enum ") && !line.starts_with("pub struct ") {
                continue;
            }
            let Some(name) = declared_type_name(line) else {
                continue;
            };
            if !COVERED_TYPE_NAMES.contains(&name.as_str()) {
                continue;
            }
            let keyword = if line.starts_with("pub enum ") { "pub enum" } else { "pub struct" };
            sites.insert(((*path).to_string(), format!("{keyword} {name}")));
        }
    }
    Ok(sites)
}

fn markdown_cell(value: &str) -> String {
    if value.is_empty() {
        return "—".to_string();
    }
    value.replace('|', "\\|").replace('\n', "<br>")
}

fn render_markdown(rows: &[PropositionRow]) -> Result<String> {
    let mut ordered = rows.to_vec();
    ordered.sort_by(|left, right| left.id.cmp(&right.id));

    let mut output = String::new();
    output.push_str("# Workspace index lifecycle propositions\n\n");
    output.push_str("Generated from `policy/workspace-index-lifecycle-propositions.v1.tsv`.\n");
    output.push_str("Edit the checked ledger, then regenerate this projection.\n\n");
    output.push_str(
        "Ownership only. This map changes no indexing behavior, readiness result, provider\n\
         routing, snapshot publication, resource policy or public API.\n\n",
    );

    output.push_str("## Selected owner by proposition family\n\n");
    output.push_str("| Family | Owner | Disposition | Required target identity | Sufficient today | Successor |\n");
    output.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for row in ordered.iter().filter(|row| row.is_owner()) {
        writeln!(
            output,
            "| {} | {} | {} | {} | {} | {} |",
            markdown_cell(&row.family),
            markdown_cell(&row.id),
            markdown_cell(&row.disposition),
            markdown_cell(&row.required_target_identity),
            markdown_cell(&row.identity_sufficient_today),
            markdown_cell(&row.successor_issue),
        )
        .context("render owner row")?;
    }

    output.push_str("\n## Every proposition row\n\n");
    output.push_str(
        "| ID | State | Family | Proposition | Current type | Reachability | Current identity | Required target identity | Sufficient today | Transition owner | Authority | Public | Disposition | Owner | Runtime row | Successor |\n",
    );
    output.push_str(
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for row in &ordered {
        writeln!(
            output,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            markdown_cell(&row.id),
            markdown_cell(&row.current_state),
            markdown_cell(&row.family),
            markdown_cell(&row.proposition),
            markdown_cell(&row.current_type),
            markdown_cell(&row.reachability),
            markdown_cell(&row.current_identity),
            markdown_cell(&row.required_target_identity),
            markdown_cell(&row.identity_sufficient_today),
            markdown_cell(&row.transition_owner),
            markdown_cell(&row.authority_kind),
            markdown_cell(&row.public_commitment),
            markdown_cell(&row.disposition),
            markdown_cell(&row.owner_row),
            markdown_cell(&row.runtime_ownership_row),
            markdown_cell(&row.successor_issue),
        )
        .context("render proposition row")?;
    }

    output.push_str("\n## Cutover handoff by successor\n\n");
    let mut by_successor: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for row in &ordered {
        by_successor.entry(row.successor_issue.as_str()).or_default().push(row.id.as_str());
    }
    output.push_str("| Successor | Rows |\n| --- | --- |\n");
    for (successor, ids) in by_successor {
        writeln!(output, "| {} | {} |", markdown_cell(successor), ids.join(", "))
            .context("render successor row")?;
    }

    Ok(output)
}

#[test]
fn proposition_rows_are_unique_complete_and_well_formed() -> Result<()> {
    let rows = load_rows()?;
    validate_rows(&rows)?;

    let observed: BTreeSet<&str> = rows.iter().map(|row| row.family.as_str()).collect();
    let expected: BTreeSet<&str> = ALLOWED_FAMILIES.iter().copied().collect();
    ensure!(
        observed == expected,
        "every required proposition family must be represented; missing {:?}",
        expected.difference(&observed).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn live_rows_are_bound_to_current_source_markers() -> Result<()> {
    let rows = load_rows()?;
    validate_rows(&rows)?;
    validate_live_source_markers(&rows)
}

#[test]
fn runtime_ownership_rows_are_joined_not_duplicated() -> Result<()> {
    let rows = load_rows()?;
    validate_runtime_ownership_join(&rows)
}

#[test]
fn every_lifecycle_declaration_is_mapped() -> Result<()> {
    let rows = load_rows()?;
    let mapped: BTreeSet<(String, String)> =
        rows.iter().map(|row| (row.source_path.clone(), row.source_marker.clone())).collect();

    let declared = declared_lifecycle_sites()?;
    ensure!(
        !declared.is_empty(),
        "lifecycle declaration scan found nothing; the coverage ratchet would be vacuous"
    );

    let missing: Vec<&(String, String)> =
        declared.iter().filter(|site| !mapped.contains(*site)).collect();
    ensure!(
        missing.is_empty(),
        "index lifecycle declarations are unmapped in {LEDGER_PATH}: {missing:?}"
    );
    Ok(())
}

#[test]
fn generated_reviewer_projection_is_current() -> Result<()> {
    let rows = load_rows()?;
    validate_rows(&rows)?;
    let expected = render_markdown(&rows)?;
    let root = repo_root()?;
    let path = root.join(GENERATED_PATH);

    if std::env::var_os(UPDATE_ENV).is_some() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        fs::write(&path, &expected).with_context(|| format!("write {GENERATED_PATH}"))?;
        return Ok(());
    }

    let actual = fs::read_to_string(&path).with_context(|| format!("read {GENERATED_PATH}"))?;
    ensure!(
        actual == expected,
        "{GENERATED_PATH} is stale; regenerate with {UPDATE_ENV}=1 cargo test -p xtask --locked --test workspace_index_lifecycle_ledger"
    );
    Ok(())
}

#[test]
fn generated_projection_is_order_independent() -> Result<()> {
    let rows = load_rows()?;
    let forward = render_markdown(&rows)?;
    let mut reversed = rows;
    reversed.reverse();
    let backward = render_markdown(&reversed)?;
    ensure!(forward == backward, "generated projection depends on ledger input order");
    Ok(())
}

// ---------------------------------------------------------------------------
// Controlled mutations. Each proves the checked authority rejects a naive merge.
//
// Every mutation asserts the *reason* it was rejected. Asserting only that
// validation failed would let a mutation pass this suite by tripping an
// unrelated rule, which would make the negative controls vacuous.
// ---------------------------------------------------------------------------

#[track_caller]
fn assert_rejected_because<T>(result: Result<T>, expected: &str) -> Result<()> {
    let error = match result {
        Ok(_) => bail!("mutation was accepted; expected rejection mentioning {expected:?}"),
        Err(error) => format!("{error:#}"),
    };
    ensure!(
        error.contains(expected),
        "mutation was rejected for the wrong reason.\n  expected substring: {expected:?}\n  actual error: {error}"
    );
    Ok(())
}

#[test]
fn duplicate_stable_row_id_is_rejected() -> Result<()> {
    let mut rows = load_rows()?;
    let duplicate = rows.first().cloned().context("proposition ledger unexpectedly empty")?;
    rows.push(duplicate);
    assert_rejected_because(validate_rows(&rows), "duplicate proposition row")?;
    Ok(())
}

#[test]
fn two_canonical_owners_for_one_family_are_rejected() -> Result<()> {
    let mut rows = load_rows()?;
    let owned_families: BTreeSet<String> =
        rows.iter().filter(|row| row.is_owner()).map(|row| row.family.clone()).collect();
    let victim = rows
        .iter_mut()
        .find(|row| !row.is_owner() && owned_families.contains(&row.family))
        .context("expected a non-owner row in a family that already selects an owner")?;
    victim.owner_row = "self".to_string();
    victim.disposition = "canonical_lifecycle_state".to_string();
    victim.authority_kind = "semantic".to_string();
    assert_rejected_because(validate_rows(&rows), "canonical owners")?;
    Ok(())
}

#[test]
fn telemetry_type_cannot_become_readiness_authority() -> Result<()> {
    let mut rows = load_rows()?;
    let victim = rows
        .iter_mut()
        .find(|row| row.authority_kind == "telemetry")
        .context("expected at least one telemetry row")?;
    victim.disposition = "provider_readiness_projection_only".to_string();
    victim.owner_row = "self".to_string();
    assert_rejected_because(
        validate_rows(&rows),
        "cannot hold semantic lifecycle or readiness authority",
    )?;
    Ok(())
}

#[test]
fn counter_driven_transition_cannot_own_a_proposition() -> Result<()> {
    let mut rows = load_rows()?;
    let victim = rows
        .iter_mut()
        .find(|row| row.transition_owner.to_ascii_lowercase().contains("counter"))
        .context("expected a counter-driven transition row")?;
    victim.disposition = "canonical_lifecycle_state".to_string();
    victim.owner_row = "self".to_string();
    victim.authority_kind = "semantic".to_string();
    assert_rejected_because(
        validate_rows(&rows),
        "counter-driven transition cannot own a lifecycle proposition",
    )?;
    Ok(())
}

#[test]
fn owner_cannot_rest_on_a_counter_or_path_identity() -> Result<()> {
    let mut rows = load_rows()?;
    let victim =
        rows.iter_mut().find(|row| row.is_owner()).context("expected at least one owner row")?;
    victim.required_target_identity = "pending count".to_string();
    assert_rejected_because(
        validate_rows(&rows),
        "alone cannot be the target identity of a selected owner",
    )?;
    Ok(())
}

#[test]
fn building_owner_without_operation_identity_is_rejected() -> Result<()> {
    let mut rows = load_rows()?;
    let victim = rows
        .iter_mut()
        .find(|row| row.family == "work_activity" && row.is_owner())
        .context("expected a work_activity owner")?;
    victim.required_target_identity = "indexed and total counts plus a timestamp".to_string();
    assert_rejected_because(validate_rows(&rows), "must require \"operation ticket\"")?;
    Ok(())
}

#[test]
fn ready_owner_without_accepted_snapshot_is_rejected() -> Result<()> {
    let mut rows = load_rows()?;
    let victim = rows
        .iter_mut()
        .find(|row| row.family == "snapshot_availability" && row.is_owner())
        .context("expected a snapshot_availability owner")?;
    victim.required_target_identity = "file and symbol counts".to_string();
    assert_rejected_because(validate_rows(&rows), "must require \"snapshot\"")?;
    Ok(())
}

#[test]
fn row_cannot_name_an_owner_in_another_family() -> Result<()> {
    let mut rows = load_rows()?;
    let foreign_owner = rows
        .iter()
        .find(|row| row.is_owner() && row.family == "degradation")
        .map(|row| row.id.clone())
        .context("expected a degradation owner")?;
    let victim = rows
        .iter_mut()
        .find(|row| row.family == "instrumentation" && row.owner_row == "none")
        .context("expected an ownerless instrumentation row")?;
    victim.owner_row = foreign_owner;
    assert_rejected_because(validate_rows(&rows), "belongs to family")?;
    Ok(())
}

#[test]
fn unnamed_future_owner_is_rejected() -> Result<()> {
    let mut rows = load_rows()?;
    let victim = rows.first_mut().context("proposition ledger unexpectedly empty")?;
    victim.successor_issue = "#10821".to_string();
    assert_rejected_because(validate_rows(&rows), "is not a live implementation owner")?;
    Ok(())
}

#[test]
fn absent_proposition_cannot_claim_live_reachability() -> Result<()> {
    let mut rows = load_rows()?;
    let victim = rows
        .iter_mut()
        .find(|row| row.current_state == "absent_on_main")
        .context("expected at least one absent proposition")?;
    victim.source_path = "crates/perl-workspace/src/workspace/workspace_index.rs".to_string();
    victim.source_marker = "pub struct IndexCoordinator".to_string();
    assert_rejected_because(validate_rows(&rows), "cannot cite live source reachability")?;
    Ok(())
}

#[test]
fn missing_live_source_marker_is_rejected() -> Result<()> {
    let mut rows = load_rows()?;
    let victim = rows
        .iter_mut()
        .find(|row| row.current_state == "live")
        .context("expected at least one live row")?;
    victim.source_marker = "__index_lifecycle_marker_that_does_not_exist__".to_string();
    assert_rejected_because(validate_live_source_markers(&rows), "not found in")?;
    Ok(())
}

#[test]
fn unmapped_lifecycle_declaration_is_rejected() -> Result<()> {
    let rows = load_rows()?;
    let mut mapped: BTreeSet<(String, String)> =
        rows.iter().map(|row| (row.source_path.clone(), row.source_marker.clone())).collect();

    let declared = declared_lifecycle_sites()?;
    let dropped = declared.iter().next().cloned().context("expected a declared lifecycle site")?;
    mapped.remove(&dropped);

    ensure!(
        declared.iter().any(|site| !mapped.contains(site)),
        "removing a mapped declaration must leave the coverage ratchet unsatisfied"
    );
    Ok(())
}

#[test]
fn unknown_runtime_ownership_row_is_rejected() -> Result<()> {
    let mut rows = load_rows()?;
    let victim = rows
        .iter_mut()
        .find(|row| !row.runtime_ownership_row.is_empty())
        .context("expected a row joined to the runtime ownership ledger")?;
    victim.runtime_ownership_row = "WRT-DOES-NOT-EXIST".to_string();
    assert_rejected_because(validate_runtime_ownership_join(&rows), "is not present in")?;
    Ok(())
}
