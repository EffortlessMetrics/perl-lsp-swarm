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

/// Modules that exist solely to define index lifecycle vocabulary. EVERY public
/// enum or struct declared in these files must carry a ledger row — there is no
/// per-name allowlist to forget, so a lifecycle type with a novel name cannot
/// appear unmapped.
const LIFECYCLE_SCOPED_MODULES: &[&str] = &[
    "crates/perl-workspace/src/monitoring/mod.rs",
    "crates/perl-workspace/src/state_machine/mod.rs",
];

/// `workspace_index.rs` is a large mixed module: it legitimately declares symbol,
/// location and index-storage types alongside lifecycle ones. Only the lifecycle
/// declarations are mapped, so this list is explicit and deliberately tiny.
const MIXED_MODULE: &str = "crates/perl-workspace/src/workspace/workspace_index.rs";
const MIXED_MODULE_LIFECYCLE_TYPES: &[&str] = &["IndexState", "IndexCoordinator"];

/// Types in the mixed module whose names read as lifecycle-adjacent but are
/// symbol-storage, not lifecycle authority. Naming one here is a decision; the
/// point is that a lifecycle-suggestive name cannot be silently absent from both
/// lists, which is how a newly named lifecycle type would bypass the ratchet.
const MIXED_MODULE_NON_LIFECYCLE_TYPES: &[&str] = &["FileIndex", "WorkspaceIndex"];

/// Substrings that make a declaration in the mixed module require an explicit
/// lifecycle / non-lifecycle classification.
const LIFECYCLE_SUGGESTIVE: &[&str] = &["Index", "Lifecycle", "State", "Coordinator"];

const MEMBER_LEDGER_PATH: &str = "policy/workspace-index-lifecycle-members.v1.tsv";
const MEMBER_COLUMN_COUNT: usize = 8;

const ALLOWED_MEMBER_DISPOSITIONS: &[&str] = &[
    "canonical_member",
    "canonical_member_identity_gap",
    "telemetry_member",
    "duplicate_of_canonical_member",
    "absent_from_canonical",
];

/// The vocabularies whose type names collide across the three modules. #10433
/// requires a disposition for every overlapping type, variant, field and
/// transition record, so these types are inventoried at member granularity: a
/// variant or field change must not pass while the declaration stays present.
const OVERLAPPING_TYPES: &[(&str, &str)] = &[
    ("workspace_index", "IndexState"),
    ("state_machine", "IndexState"),
    ("monitoring", "IndexStateKind"),
    ("state_machine", "IndexStateKind"),
    ("monitoring", "DegradationReason"),
    ("state_machine", "DegradationReason"),
    ("monitoring", "ResourceKind"),
    ("state_machine", "ResourceKind"),
    ("monitoring", "IndexStateTransition"),
    ("state_machine", "IndexStateTransition"),
    ("monitoring", "IndexPhase"),
    ("state_machine", "BuildPhase"),
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
    "canonical_telemetry_projection",
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
    "canonical_telemetry_projection",
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
    let mut represented_families: BTreeSet<&str> = BTreeSet::new();

    for row in rows {
        represented_families.insert(row.family.as_str());
        if row.is_owner() {
            owners.entry(row.family.as_str()).or_default().push(row.id.as_str());
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

    // Every represented proposition family selects an owner, including families
    // whose rows are purely telemetry. A telemetry-only family is owned by a
    // `canonical_telemetry_projection` row, which is still barred from holding
    // semantic lifecycle or readiness authority.
    for family in represented_families {
        ensure!(owners.contains_key(family), "family {family} is represented but selects no owner");
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

/// Decide whether a marker binds to its source.
///
/// A declaration-shaped marker must match an actual declaration line, not a
/// passing mention in a comment or doc block. Identifier and prose markers (LSP
/// field names, constants, markdown status lines) keep substring matching, which
/// is the right granularity for them; that stays a weaker binding, which is
/// exactly why declarations are anchored.
fn marker_binds(source: &str, marker: &str) -> bool {
    if marker.starts_with("pub ") {
        source.lines().any(|line| line.trim_start().starts_with(marker))
    } else {
        source.contains(marker)
    }
}

fn validate_live_source_markers(rows: &[PropositionRow]) -> Result<()> {
    let root = repo_root()?;
    for row in rows.iter().filter(|row| !row.source_path.is_empty()) {
        let source = fs::read_to_string(root.join(&row.source_path))
            .with_context(|| format!("read live source {}", row.source_path))?;
        ensure!(
            marker_binds(&source, &row.source_marker),
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

/// Every index lifecycle declaration must be mapped. A new lifecycle enum, state
/// machine or coordinator cannot appear without a row.
fn declared_lifecycle_sites() -> Result<BTreeSet<(String, String)>> {
    let root = repo_root()?;
    let mut sites = BTreeSet::new();

    for path in LIFECYCLE_SCOPED_MODULES.iter().chain(std::iter::once(&MIXED_MODULE)) {
        let source = fs::read_to_string(root.join(path)).with_context(|| format!("read {path}"))?;
        let restricted = *path == MIXED_MODULE;
        for line in source.lines() {
            let keyword = if line.starts_with("pub enum ") {
                "pub enum"
            } else if line.starts_with("pub struct ") {
                "pub struct"
            } else {
                continue;
            };
            let Some(name) = declared_type_name(line) else {
                continue;
            };
            if restricted && !MIXED_MODULE_LIFECYCLE_TYPES.contains(&name.as_str()) {
                continue;
            }
            sites.insert(((*path).to_string(), format!("{keyword} {name}")));
        }
    }
    Ok(sites)
}

/// The coverage ratchet itself. Both the positive test and its controlled
/// mutation call this, so the mutation exercises the real check rather than
/// re-asserting set arithmetic it computed itself.
fn validate_declaration_coverage(mapped: &BTreeSet<(String, String)>) -> Result<()> {
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

fn mapped_sites(rows: &[PropositionRow]) -> BTreeSet<(String, String)> {
    rows.iter().map(|row| (row.source_path.clone(), row.source_marker.clone())).collect()
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
    validate_declaration_coverage(&mapped_sites(&rows))
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
fn generated_projection_is_stable_under_input_order() -> Result<()> {
    let rows = load_rows()?;

    // Reversing alone is a weak shuffle. Rotate and reverse so no row keeps its
    // original index, then require byte equality with the canonical rendering.
    let mut shuffled = rows.clone();
    shuffled.rotate_left(7);
    shuffled.reverse();
    ensure!(
        shuffled.iter().map(|row| row.id.as_str()).ne(rows.iter().map(|row| row.id.as_str())),
        "the shuffle did not reorder the ledger; this control would be vacuous"
    );
    ensure!(
        render_markdown(&rows)? == render_markdown(&shuffled)?,
        "generated projection depends on ledger input order"
    );

    // Assert the canonical ordering directly, so removing the sort fails with a
    // named cause rather than only through byte inequality above. This control
    // cannot catch nondeterminism introduced *after* the sort (for example a
    // hash-ordered section); cross-process stability of the committed artifact
    // is covered by `generated_reviewer_projection_is_current`.
    let rendered = render_markdown(&rows)?;
    let mut seen: Vec<&str> = Vec::new();
    for line in rendered.lines() {
        if let Some(id) = line.strip_prefix("| WSI-").and_then(|rest| rest.split(' ').next()) {
            seen.push(id);
        }
    }
    ensure!(!seen.is_empty(), "no rendered rows found; the ordering control would be vacuous");
    ensure!(
        seen.windows(2).all(|pair| pair[0] <= pair[1]),
        "rendered rows are not in canonical ascending order: {seen:?}"
    );
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
    let mut mapped = mapped_sites(&rows);
    let declared = declared_lifecycle_sites()?;
    let dropped = declared.iter().next().cloned().context("expected a declared lifecycle site")?;
    mapped.remove(&dropped);

    assert_rejected_because(validate_declaration_coverage(&mapped), "are unmapped in")
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

// ---------------------------------------------------------------------------
// Member-level denominator for the overlapping vocabularies.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct MemberRow {
    id: String,
    type_row: String,
    module: String,
    type_name: String,
    member: String,
    member_kind: String,
    disposition: String,
    successor_issue: String,
}

fn module_path(module: &str) -> Option<&'static str> {
    match module {
        "monitoring" => Some("crates/perl-workspace/src/monitoring/mod.rs"),
        "state_machine" => Some("crates/perl-workspace/src/state_machine/mod.rs"),
        "workspace_index" => Some("crates/perl-workspace/src/workspace/workspace_index.rs"),
        _ => None,
    }
}

fn load_member_rows() -> Result<Vec<MemberRow>> {
    let root = repo_root()?;
    let source = fs::read_to_string(root.join(MEMBER_LEDGER_PATH))
        .with_context(|| format!("read {MEMBER_LEDGER_PATH}"))?;
    let mut rows = Vec::new();
    for (index, line) in source.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let columns: Vec<&str> = line.split('|').collect();
        ensure!(
            columns.len() == MEMBER_COLUMN_COUNT,
            "{MEMBER_LEDGER_PATH}:{}: expected {MEMBER_COLUMN_COUNT} columns, found {}",
            index + 1,
            columns.len()
        );
        rows.push(MemberRow {
            id: columns[0].to_string(),
            type_row: columns[1].to_string(),
            module: columns[2].to_string(),
            type_name: columns[3].to_string(),
            member: columns[4].to_string(),
            member_kind: columns[5].to_string(),
            disposition: columns[6].to_string(),
            successor_issue: columns[7].to_string(),
        });
    }
    ensure!(!rows.is_empty(), "{MEMBER_LEDGER_PATH} contains no member rows");
    Ok(rows)
}

fn leading_ident(rest: &str) -> String {
    rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect()
}

fn variant_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix("    ")?;
    if rest.starts_with(' ') || !rest.chars().next()?.is_ascii_uppercase() {
        return None;
    }
    let name = leading_ident(rest);
    (!name.is_empty()).then_some(name)
}

fn field_name(line: &str, indent: &str, prefix: Option<&str>) -> Option<String> {
    let rest = line.strip_prefix(indent)?;
    let rest = match prefix {
        Some(prefix) => rest.strip_prefix(prefix)?,
        None => {
            if rest.starts_with(' ') {
                return None;
            }
            rest
        }
    };
    let first = rest.chars().next()?;
    if !(first.is_ascii_lowercase() || first == '_') {
        return None;
    }
    let name = leading_ident(rest);
    (rest[name.len()..].starts_with(':')).then_some(name)
}

/// Extract the declared variants, variant fields and struct fields of one type
/// from current source. This is the member denominator: it is read from the
/// tree, never from the ledger, so the ledger cannot certify its own coverage.
fn declared_members(module: &str, type_name: &str) -> Result<Vec<(String, String)>> {
    let root = repo_root()?;
    let path = module_path(module).with_context(|| format!("unknown lifecycle module {module}"))?;
    let source = fs::read_to_string(root.join(path)).with_context(|| format!("read {path}"))?;

    let mut start = None;
    let mut is_enum = false;
    for (index, line) in source.lines().enumerate() {
        let line_is_enum = if line.starts_with("pub enum ") {
            true
        } else if line.starts_with("pub struct ") {
            false
        } else {
            continue;
        };
        if declared_type_name(line).as_deref() == Some(type_name) {
            start = Some(index);
            is_enum = line_is_enum;
            break;
        }
    }
    let start =
        start.with_context(|| format!("{module}::{type_name} is not declared in {path}"))?;

    let mut members = Vec::new();
    let mut depth: i32 = 0;
    let mut opened = false;
    let mut current_variant: Option<String> = None;

    for line in source.lines().skip(start) {
        depth += line.matches('{').count() as i32;
        depth -= line.matches('}').count() as i32;
        if line.contains('{') {
            opened = true;
        }
        if opened && depth == 0 {
            break;
        }
        if is_enum {
            if let Some(name) = variant_name(line) {
                current_variant = Some(name.clone());
                members.push((name, "variant".to_string()));
            } else if let Some(field) = field_name(line, "        ", None)
                && let Some(variant) = current_variant.as_ref()
            {
                members.push((format!("{variant}.{field}"), "variant_field".to_string()));
            }
        } else if let Some(field) = field_name(line, "    ", Some("pub ")) {
            members.push((field, "struct_field".to_string()));
        }
    }
    Ok(members)
}

fn validate_member_rows(rows: &[MemberRow], type_rows: &[PropositionRow]) -> Result<()> {
    let known_type_rows: BTreeMap<&str, &PropositionRow> =
        type_rows.iter().map(|row| (row.id.as_str(), row)).collect();
    let mut ids = BTreeSet::new();
    let mut logical_keys = BTreeSet::new();

    for row in rows {
        ensure!(row.id.starts_with("WSIM-"), "{}: invalid stable member row ID", row.id);
        ensure!(ids.insert(row.id.as_str()), "duplicate member row {}", row.id);
        // A distinct WSIM id is not enough: two rows for the same declared member
        // would silently collapse during coverage and could carry conflicting
        // dispositions, breaking the one-disposition-per-member contract.
        ensure!(
            logical_keys.insert((row.module.as_str(), row.type_name.as_str(), row.member.as_str())),
            "{}: duplicate disposition for member {}::{}::{}",
            row.id,
            row.module,
            row.type_name,
            row.member
        );
        ensure!(
            ALLOWED_MEMBER_DISPOSITIONS.contains(&row.disposition.as_str()),
            "{}: unknown member disposition {:?}",
            row.id,
            row.disposition
        );
        ensure!(
            ALLOWED_SUCCESSORS.contains(&row.successor_issue.as_str()),
            "{}: successor {:?} is not a live implementation owner",
            row.id,
            row.successor_issue
        );
        let type_row = known_type_rows.get(row.type_row.as_str()).with_context(|| {
            format!("{}: type_row {} does not exist in the type ledger", row.id, row.type_row)
        })?;
        // Existence is not enough: the referenced row must describe this member's
        // own declaration, or the ledger could publish a false ownership and
        // successor relationship for a member of a different type.
        let expected_path = module_path(&row.module)
            .with_context(|| format!("{}: unknown lifecycle module {}", row.id, row.module))?;
        ensure!(
            type_row.source_path == expected_path,
            "{}: type_row {} describes {}, not {}",
            row.id,
            row.type_row,
            type_row.source_path,
            expected_path
        );
        // A member and the type it belongs to must reach the same cutover owner.
        // Two contradictory handoffs for one declaration would let #10791 and
        // #10799 each believe the other owns the member.
        ensure!(
            type_row.successor_issue == row.successor_issue,
            "{}: member routes to {} but its type row {} routes to {}",
            row.id,
            row.successor_issue,
            row.type_row,
            type_row.successor_issue
        );
        ensure!(
            declared_type_name(&type_row.source_marker).as_deref() == Some(row.type_name.as_str()),
            "{}: type_row {} declares {:?}, not {}",
            row.id,
            row.type_row,
            type_row.source_marker,
            row.type_name
        );
        ensure!(
            OVERLAPPING_TYPES.contains(&(row.module.as_str(), row.type_name.as_str())),
            "{}: {}::{} is not an overlapping vocabulary",
            row.id,
            row.module,
            row.type_name
        );
    }
    Ok(())
}

/// Every declared variant, variant field and struct field of an overlapping type
/// carries exactly one disposition, and no row survives a member that source no
/// longer declares. Changing `Ready`, adding a variant, or removing a field must
/// fail even though the enum declaration itself is untouched.
fn validate_member_coverage(rows: &[MemberRow]) -> Result<()> {
    for (module, type_name) in OVERLAPPING_TYPES {
        let declared = declared_members(module, type_name)?;
        ensure!(
            !declared.is_empty(),
            "no members extracted for {module}::{type_name}; the member ratchet would be vacuous"
        );

        let mapped: BTreeMap<&str, &MemberRow> = rows
            .iter()
            .filter(|row| row.module == *module && row.type_name == *type_name)
            .map(|row| (row.member.as_str(), row))
            .collect();

        for (member, kind) in &declared {
            let row = mapped.get(member.as_str()).with_context(|| {
                format!(
                    "{module}::{type_name} member {member} has no disposition in {MEMBER_LEDGER_PATH}"
                )
            })?;
            ensure!(
                row.member_kind == *kind,
                "{}: member {member} is a {kind} in source but recorded as {}",
                row.id,
                row.member_kind
            );
        }

        let declared_names: BTreeSet<&str> =
            declared.iter().map(|(name, _)| name.as_str()).collect();
        for member in mapped.keys() {
            ensure!(
                declared_names.contains(member),
                "{module}::{type_name} member {member} is dispositioned but source no longer declares it"
            );
        }
    }
    Ok(())
}

#[test]
fn member_rows_are_well_formed_and_joined_to_the_type_ledger() -> Result<()> {
    let type_rows = load_rows()?;
    let rows = load_member_rows()?;
    validate_member_rows(&rows, &type_rows)
}

#[test]
fn every_overlapping_member_is_dispositioned() -> Result<()> {
    let rows = load_member_rows()?;
    validate_member_coverage(&rows)
}

#[test]
fn member_extraction_finds_the_known_live_index_state_shape() -> Result<()> {
    // Guards the extractor itself: a silently-empty or shape-blind parser would
    // make the member ratchet vacuous no matter how many rows the ledger holds.
    let members = declared_members("workspace_index", "IndexState")?;
    let names: Vec<&str> = members.iter().map(|(name, _)| name.as_str()).collect();
    for expected in [
        "Building",
        "Building.phase",
        "Building.indexed_count",
        "Building.total_count",
        "Building.started_at",
        "Ready",
        "Ready.symbol_count",
        "Ready.file_count",
        "Ready.completed_at",
        "Degraded",
        "Degraded.reason",
        "Degraded.available_symbols",
        "Degraded.since",
    ] {
        ensure!(names.contains(&expected), "member extraction missed {expected}: {names:?}");
    }
    ensure!(names.len() == 13, "unexpected live IndexState member shape: {names:?}");
    Ok(())
}

#[test]
fn cancelled_is_recorded_only_on_the_live_degradation_vocabulary() -> Result<()> {
    // The two DegradationReason enums differ by exactly this variant. The map
    // must carry that as a checked member fact, not prose.
    let live = declared_members("monitoring", "DegradationReason")?;
    let duplicate = declared_members("state_machine", "DegradationReason")?;
    let live_names: BTreeSet<&str> = live.iter().map(|(name, _)| name.as_str()).collect();
    let duplicate_names: BTreeSet<&str> = duplicate.iter().map(|(name, _)| name.as_str()).collect();
    ensure!(live_names.contains("Cancelled"), "live DegradationReason lost Cancelled");
    ensure!(
        !duplicate_names.contains("Cancelled"),
        "the duplicate DegradationReason gained Cancelled; the vocabularies converged"
    );
    Ok(())
}

#[test]
fn added_member_without_a_disposition_is_rejected() -> Result<()> {
    let mut rows = load_member_rows()?;
    // Dropping a row is equivalent to source gaining a member the ledger has
    // not dispositioned — the reviewer's `Ready`-field case.
    let dropped = rows
        .iter()
        .position(|row| row.member == "Ready.completed_at" && row.module == "workspace_index")
        .context("expected a row for the live Ready.completed_at field")?;
    rows.remove(dropped);
    assert_rejected_because(validate_member_coverage(&rows), "has no disposition in")
}

#[test]
fn removed_member_leaves_no_orphan_disposition() -> Result<()> {
    let mut rows = load_member_rows()?;
    let template = rows.first().cloned().context("member ledger unexpectedly empty")?;
    rows.push(MemberRow {
        id: "WSIM-PHANTOM-001".to_string(),
        member: "VariantThatSourceDoesNotDeclare".to_string(),
        ..template
    });
    assert_rejected_because(
        validate_member_coverage(&rows),
        "is dispositioned but source no longer declares it",
    )
}

#[test]
fn member_row_cannot_reference_a_missing_type_row() -> Result<()> {
    let type_rows = load_rows()?;
    let mut rows = load_member_rows()?;
    let victim = rows.first_mut().context("member ledger unexpectedly empty")?;
    victim.type_row = "WSI-DOES-NOT-EXIST".to_string();
    assert_rejected_because(
        validate_member_rows(&rows, &type_rows),
        "does not exist in the type ledger",
    )
}

/// Lines inside a covered type's body that the member parser cannot classify.
///
/// The parser understands column-zero declarations, four-space variants or
/// public struct fields, and eight-space variant fields. Anything else — a tuple
/// variant, a macro-generated member, an unusual layout — must fail loudly here
/// rather than silently shrink the member denominator.
fn unclassified_body_lines(module: &str, type_name: &str) -> Result<Vec<String>> {
    let root = repo_root()?;
    let path = module_path(module).with_context(|| format!("unknown lifecycle module {module}"))?;
    let source = fs::read_to_string(root.join(path)).with_context(|| format!("read {path}"))?;

    let mut start = None;
    let mut is_enum = false;
    for (index, line) in source.lines().enumerate() {
        let line_is_enum = if line.starts_with("pub enum ") {
            true
        } else if line.starts_with("pub struct ") {
            false
        } else {
            continue;
        };
        if declared_type_name(line).as_deref() == Some(type_name) {
            start = Some(index);
            is_enum = line_is_enum;
            break;
        }
    }
    let start =
        start.with_context(|| format!("{module}::{type_name} is not declared in {path}"))?;

    let mut unclassified = Vec::new();
    let mut depth: i32 = 0;
    let mut opened = false;

    for (offset, line) in source.lines().skip(start).enumerate() {
        depth += line.matches('{').count() as i32;
        depth -= line.matches('}').count() as i32;
        if line.contains('{') {
            opened = true;
        }
        if opened && depth == 0 {
            break;
        }
        if offset == 0 {
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("#[")
            || trimmed == "{"
            || trimmed == "}"
            || trimmed == "},"
        {
            continue;
        }

        // A tuple variant carries positional members this parser cannot name, so
        // it must be classified deliberately rather than silently under-covered.
        if is_enum && variant_name(line).is_some() && trimmed.contains('(') {
            unclassified.push(format!("tuple variant: {trimmed}"));
            continue;
        }

        let classified = if is_enum {
            variant_name(line).is_some() || field_name(line, "        ", None).is_some()
        } else {
            field_name(line, "    ", Some("pub ")).is_some()
        };
        if !classified {
            unclassified.push(trimmed.to_string());
        }
    }
    Ok(unclassified)
}

/// Every declaration in the mixed module whose name reads as lifecycle-adjacent
/// must be explicitly classified as lifecycle or not. Without this, a newly named
/// lifecycle type in `workspace_index.rs` would bypass the ratchet by simply not
/// appearing in the hand-maintained list.
fn validate_mixed_module_classification() -> Result<()> {
    let root = repo_root()?;
    let source = fs::read_to_string(root.join(MIXED_MODULE))
        .with_context(|| format!("read {MIXED_MODULE}"))?;
    let mut unclassified = Vec::new();
    for line in source.lines() {
        if !line.starts_with("pub enum ") && !line.starts_with("pub struct ") {
            continue;
        }
        let Some(name) = declared_type_name(line) else {
            continue;
        };
        if !LIFECYCLE_SUGGESTIVE.iter().any(|hint| name.contains(hint)) {
            continue;
        }
        if MIXED_MODULE_LIFECYCLE_TYPES.contains(&name.as_str())
            || MIXED_MODULE_NON_LIFECYCLE_TYPES.contains(&name.as_str())
        {
            continue;
        }
        unclassified.push(name);
    }
    ensure!(
        unclassified.is_empty(),
        "lifecycle-suggestive types in {MIXED_MODULE} are neither mapped nor explicitly excluded: {unclassified:?}"
    );
    Ok(())
}

#[test]
fn member_parser_understands_every_covered_body() -> Result<()> {
    for (module, type_name) in OVERLAPPING_TYPES {
        let unclassified = unclassified_body_lines(module, type_name)?;
        ensure!(
            unclassified.is_empty(),
            "{module}::{type_name} contains member forms this parser cannot classify, so the member denominator would silently shrink: {unclassified:?}"
        );
    }
    Ok(())
}

#[test]
fn lifecycle_suggestive_mixed_module_types_are_classified() -> Result<()> {
    validate_mixed_module_classification()
}

#[test]
fn duplicate_member_disposition_is_rejected() -> Result<()> {
    let type_rows = load_rows()?;
    let mut rows = load_member_rows()?;
    let mut duplicate = rows.first().cloned().context("member ledger unexpectedly empty")?;
    duplicate.id = "WSIM-DUPLICATE-001".to_string();
    rows.push(duplicate);
    assert_rejected_because(
        validate_member_rows(&rows, &type_rows),
        "duplicate disposition for member",
    )
}

#[test]
fn member_cannot_join_an_unrelated_type_row() -> Result<()> {
    let type_rows = load_rows()?;

    // Same file, different type: the type-name check must reject it.
    let mut rows = load_member_rows()?;
    let victim = rows
        .iter_mut()
        .find(|row| row.type_name == "IndexState" && row.module == "workspace_index")
        .context("expected a live IndexState member row")?;
    victim.type_row = "WSI-COORD-001".to_string();
    assert_rejected_because(validate_member_rows(&rows, &type_rows), "declares")?;

    // Different file entirely: the source-path check must reject it.
    let mut rows = load_member_rows()?;
    let victim = rows
        .iter_mut()
        .find(|row| row.type_name == "IndexState" && row.module == "workspace_index")
        .context("expected a live IndexState member row")?;
    victim.type_row = "WSI-DEG-001".to_string();
    assert_rejected_because(validate_member_rows(&rows, &type_rows), "describes")
}

#[test]
fn member_successor_must_match_its_type_row() -> Result<()> {
    let type_rows = load_rows()?;
    let mut rows = load_member_rows()?;
    let victim = rows.first_mut().context("member ledger unexpectedly empty")?;
    victim.successor_issue =
        if victim.successor_issue == "#10791" { "#10799" } else { "#10791" }.to_string();
    assert_rejected_because(validate_member_rows(&rows, &type_rows), "but its type row")
}

#[test]
fn declaration_marker_in_a_comment_does_not_bind() -> Result<()> {
    // No file in the repo currently mentions a declaration only inside a comment,
    // so the discrimination is proven against a fixture rather than with a test
    // that would pass simply because the marker is absent everywhere.
    let commented_only = "\
//! Module docs.
/// Example:
///     pub fn state(&self) -> IndexState
// pub enum IndexState { Building }
struct Unrelated;
";
    ensure!(
        !marker_binds(commented_only, "pub fn state(&self) -> IndexState"),
        "a declaration mentioned only in a doc comment must not bind"
    );
    ensure!(
        !marker_binds(commented_only, "pub enum IndexState"),
        "a declaration mentioned only in a line comment must not bind"
    );

    let declared = "pub enum IndexState {\n    Building,\n}\n";
    ensure!(marker_binds(declared, "pub enum IndexState"), "a real declaration must still bind");

    // Indented declarations still bind, so the anchor is leading-whitespace
    // tolerant rather than column-zero only.
    ensure!(
        marker_binds("    pub fn helper() {}\n", "pub fn helper()"),
        "an indented declaration must still bind"
    );

    // Non-declaration markers keep substring matching by design.
    ensure!(
        marker_binds("let x = indexing_in_progress.clone();", "indexing_in_progress"),
        "identifier markers must keep substring binding"
    );
    Ok(())
}
