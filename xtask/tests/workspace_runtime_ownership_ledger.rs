//! Checked root-runtime ownership denominator for issues #10011 and #10013.
//!
//! `policy/workspace-runtime-ownership.v1.tsv` is the machine authority. Live
//! rows must resolve to exact current source markers; planned rows must remain
//! explicitly non-green and source-unreachable.

use std::{
    collections::BTreeSet,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};

const LEDGER_PATH: &str = "policy/workspace-runtime-ownership.v1.tsv";
const GENERATED_PATH: &str = "docs/generated/workspace_runtime_ownership.md";
const COLUMN_COUNT: usize = 13;

const ALLOWED_STATES: &[&str] = &["live", "planned_not_on_main"];
const ALLOWED_DISPOSITIONS: &[&str] = &[
    "root_generation_core",
    "workspace_services_cutover",
    "configuration_watcher_reload_cutover",
    "hydration_checkpoint_namespace_cutover",
    "proof_observation_closeout",
    "existing_non_root_owner",
    "retire_duplicate_or_dead",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct OwnershipRow {
    id: String,
    current_state: String,
    disposition: String,
    proposition: String,
    current_owner: String,
    identity: String,
    publication: String,
    cleanup: String,
    blocking_work_reachable: bool,
    target_issue: String,
    proof_family: String,
    source_path: String,
    source_marker: String,
}

fn repo_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live immediately beneath the repository root")
}

fn parse_ledger(source: &str) -> Result<Vec<OwnershipRow>> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.is_empty() && !line.starts_with('#'))
        .map(|(index, line)| parse_row(index + 1, line))
        .collect()
}

fn parse_row(line_number: usize, line: &str) -> Result<OwnershipRow> {
    let columns: Vec<&str> = line.split('|').collect();
    ensure!(
        columns.len() == COLUMN_COUNT,
        "{LEDGER_PATH}:{line_number}: expected {COLUMN_COUNT} columns, found {}",
        columns.len()
    );
    let blocking_work_reachable = match columns[8] {
        "true" => true,
        "false" => false,
        value => bail!(
            "{LEDGER_PATH}:{line_number}: blocking_work_reachable must be true or false, found {value:?}"
        ),
    };

    Ok(OwnershipRow {
        id: columns[0].to_string(),
        current_state: columns[1].to_string(),
        disposition: columns[2].to_string(),
        proposition: columns[3].to_string(),
        current_owner: columns[4].to_string(),
        identity: columns[5].to_string(),
        publication: columns[6].to_string(),
        cleanup: columns[7].to_string(),
        blocking_work_reachable,
        target_issue: columns[9].to_string(),
        proof_family: columns[10].to_string(),
        source_path: columns[11].to_string(),
        source_marker: columns[12].to_string(),
    })
}

fn load_rows() -> Result<Vec<OwnershipRow>> {
    let root = repo_root()?;
    let source = fs::read_to_string(root.join(LEDGER_PATH))
        .with_context(|| format!("read {LEDGER_PATH}"))?;
    let rows = parse_ledger(&source)?;
    ensure!(!rows.is_empty(), "{LEDGER_PATH} contains no ownership rows");
    Ok(rows)
}

fn expected_target_issue(disposition: &str) -> Option<&'static str> {
    match disposition {
        "root_generation_core" => Some("#10013"),
        "workspace_services_cutover" => Some("#8385"),
        "configuration_watcher_reload_cutover" => Some("#10016"),
        "hydration_checkpoint_namespace_cutover" => Some("#10017"),
        "proof_observation_closeout" => Some("#10019"),
        "existing_non_root_owner" | "retire_duplicate_or_dead" => None,
        _ => None,
    }
}

fn identity_is_path_or_uri_only(identity: &str) -> bool {
    matches!(
        identity.trim().to_ascii_lowercase().as_str(),
        "path"
            | "uri"
            | "root path"
            | "root uri"
            | "workspace path"
            | "workspace uri"
            | "display path"
            | "display uri"
    )
}

fn validate_rows(rows: &[OwnershipRow]) -> Result<()> {
    ensure!(!rows.is_empty(), "ownership denominator must not be empty");
    let mut ids = BTreeSet::new();

    for row in rows {
        ensure!(row.id.starts_with("WRT-"), "{}: invalid stable row ID", row.id);
        ensure!(
            ids.insert(row.id.as_str()),
            "duplicate ownership row {}",
            row.id
        );
        ensure!(
            ALLOWED_STATES.contains(&row.current_state.as_str()),
            "{}: unknown current state {}",
            row.id,
            row.current_state
        );
        ensure!(
            ALLOWED_DISPOSITIONS.contains(&row.disposition.as_str()),
            "{}: unknown disposition {}",
            row.id,
            row.disposition
        );
        ensure!(
            !row.proposition.trim().is_empty()
                && !row.current_owner.trim().is_empty()
                && !row.identity.trim().is_empty()
                && !row.publication.trim().is_empty()
                && !row.cleanup.trim().is_empty()
                && !row.proof_family.trim().is_empty(),
            "{}: ownership proposition is incomplete",
            row.id
        );
        ensure!(
            !identity_is_path_or_uri_only(&row.identity),
            "{}: a path or URI alone cannot be root-runtime identity",
            row.id
        );
        ensure!(
            row.target_issue.starts_with('#'),
            "{}: target issue must be an issue identity",
            row.id
        );
        if let Some(expected) = expected_target_issue(&row.disposition) {
            ensure!(
                row.target_issue == expected,
                "{}: disposition {} must target {}, found {}",
                row.id,
                row.disposition,
                expected,
                row.target_issue
            );
        }

        match row.current_state.as_str() {
            "live" => ensure!(
                !row.source_path.is_empty() && !row.source_marker.is_empty(),
                "{}: live rows require an exact source path and marker",
                row.id
            ),
            "planned_not_on_main" => {
                ensure!(
                    row.source_path.is_empty() && row.source_marker.is_empty(),
                    "{}: planned rows cannot cite live source reachability",
                    row.id
                );
                ensure!(
                    row.publication.starts_with("no "),
                    "{}: planned rows must state the absent publication claim explicitly",
                    row.id
                );
            }
            value => bail!("{}: unhandled current state {value}", row.id),
        }
    }

    Ok(())
}

fn validate_live_source_markers(rows: &[OwnershipRow]) -> Result<()> {
    let root = repo_root()?;
    for row in rows.iter().filter(|row| row.current_state == "live") {
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

fn markdown_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', "<br>")
}

fn render_markdown(rows: &[OwnershipRow]) -> Result<String> {
    let mut ordered = rows.to_vec();
    ordered.sort_by(|left, right| left.id.cmp(&right.id));

    let mut output = String::new();
    output.push_str("# Workspace runtime ownership\n\n");
    output.push_str("Generated from `policy/workspace-runtime-ownership.v1.tsv`.\n");
    output.push_str("Edit the checked ledger, then regenerate this projection.\n\n");
    output.push_str("| ID | Current state | Disposition | Proposition | Current owner | Identity | Publication | Cleanup | Blocking | Target | Proof family |\n");
    output.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");

    for row in ordered {
        writeln!(
            output,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            markdown_cell(&row.id),
            markdown_cell(&row.current_state),
            markdown_cell(&row.disposition),
            markdown_cell(&row.proposition),
            markdown_cell(&row.current_owner),
            markdown_cell(&row.identity),
            markdown_cell(&row.publication),
            markdown_cell(&row.cleanup),
            row.blocking_work_reachable,
            markdown_cell(&row.target_issue),
            markdown_cell(&row.proof_family),
        )
        .context("render workspace-runtime ownership row")?;
    }
    Ok(output)
}

#[test]
fn ownership_rows_are_unique_complete_and_well_formed() -> Result<()> {
    let rows = load_rows()?;
    validate_rows(&rows)?;

    let observed: BTreeSet<_> = rows.iter().map(|row| row.disposition.as_str()).collect();
    let expected: BTreeSet<_> = ALLOWED_DISPOSITIONS.iter().copied().collect();
    ensure!(
        observed == expected,
        "every W02-W06/existing/retire disposition must be represented; observed {observed:?}"
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
fn generated_reviewer_projection_is_current() -> Result<()> {
    let rows = load_rows()?;
    let expected = render_markdown(&rows)?;
    let root = repo_root()?;
    let actual = fs::read_to_string(root.join(GENERATED_PATH))
        .with_context(|| format!("read {GENERATED_PATH}"))?;
    ensure!(
        actual == expected,
        "{GENERATED_PATH} is stale; regenerate it from {LEDGER_PATH}"
    );
    Ok(())
}

#[test]
fn duplicate_stable_row_id_is_rejected() -> Result<()> {
    let mut rows = load_rows()?;
    let duplicate = rows
        .first()
        .cloned()
        .context("ownership ledger unexpectedly empty")?;
    rows.push(duplicate);
    ensure!(
        validate_rows(&rows).is_err(),
        "duplicate row ID must fail validation"
    );
    Ok(())
}

#[test]
fn path_or_uri_only_identity_is_rejected() -> Result<()> {
    let mut rows = load_rows()?;
    let row = rows
        .first_mut()
        .context("ownership ledger unexpectedly empty")?;
    row.identity = "root URI".to_string();
    ensure!(
        validate_rows(&rows).is_err(),
        "path/URI-only root identity must fail validation"
    );
    Ok(())
}

#[test]
fn planned_row_cannot_claim_live_source_reachability() -> Result<()> {
    let mut rows = load_rows()?;
    let row = rows
        .iter_mut()
        .find(|row| row.current_state == "planned_not_on_main")
        .context("expected at least one planned row")?;
    row.source_path = "crates/perl-lsp-rs/src/runtime/mod.rs".to_string();
    row.source_marker = "pub struct LspServer".to_string();
    ensure!(
        validate_rows(&rows).is_err(),
        "planned work must not gain live reachability from a source citation"
    );
    Ok(())
}

#[test]
fn missing_live_source_marker_is_rejected() -> Result<()> {
    let mut rows = load_rows()?;
    let row = rows
        .iter_mut()
        .find(|row| row.current_state == "live")
        .context("expected at least one live row")?;
    row.source_marker = "__workspace_runtime_marker_that_does_not_exist__".to_string();
    ensure!(
        validate_live_source_markers(&rows).is_err(),
        "missing live source marker must fail validation"
    );
    Ok(())
}

#[test]
fn later_domains_remain_non_green_until_their_owner_lands() -> Result<()> {
    let rows = load_rows()?;
    for id in [
        "WRT-CHECKPOINT-001",
        "WRT-HYDRATE-001",
        "WRT-NAMESPACE-001",
        "WRT-PROOF-001",
        "WRT-RELOAD-001",
    ] {
        let row = rows
            .iter()
            .find(|row| row.id == id)
            .with_context(|| format!("missing planned row {id}"))?;
        ensure!(
            row.current_state == "planned_not_on_main",
            "{id} cannot become green without its owning implementation leaf"
        );
    }
    Ok(())
}
