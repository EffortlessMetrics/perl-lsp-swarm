//! Validate the canonical reachability fixture manifest
//! (`analysis_reachability_fixture_manifest.v1`, #10998).
//!
//! One deterministic manifest owns the declared reachability claim
//! denominator: fixture identity, metadata schema, validation, coverage
//! accounting and generated views only. It never implements analysis,
//! executes semantic or exact-process proof, selects product behavior,
//! repairs failures, changes compatibility or promotes a claim.

mod model;
mod view;

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use sha2::Digest as ShaDigest;
use sha2::Sha256;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use model::MANIFEST_RELATIVE_PATH;

const SCHEMA_PATH: &str = "schemas/analysis_reachability_fixture_manifest.v1.schema.json";

/// Claim-boundary phrases every manifest must carry verbatim.
const REQUIRED_CLAIM_PHRASES: &[&str] = &[
    "declaration only",
    "no analysis execution",
    "no semantic proof execution",
    "no exact-process proof execution",
    "no product behavior selection",
    "no claim promotion",
    "generated views derive from this manifest",
];

/// Stable row-id pattern: lowercase, digits, dots and dashes.
fn is_valid_row_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => return false,
    }
    id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
}

#[derive(Debug, Default)]
struct CoverageStats {
    rows: usize,
    declared_row_count: u64,
    families_covered: usize,
    not_proven_instruments: usize,
    deferred_slots: usize,
}

pub fn run(update_view: bool) -> Result<()> {
    let root = project_root()?;
    if update_view {
        regenerate_view(&root)?;
        println!("reachability fixture manifest coverage view regenerated");
        return Ok(());
    }
    let stats = validate(&root)?;
    println!(
        "reachability fixture manifest check passed: {} rows (declared_row_count {}), {} of {} families covered, {} deferred slots visible, {} NOT_PROVEN instruments",
        stats.rows,
        stats.declared_row_count,
        stats.families_covered,
        model::FAMILIES.len(),
        stats.deferred_slots,
        stats.not_proven_instruments,
    );
    Ok(())
}

fn regenerate_view(root: &Path) -> Result<()> {
    let manifest = load_manifest(root)?;
    // Validate the document itself; the view is the output being regenerated
    // here, so its drift rule cannot gate this path.
    let violations = validate_document(root, &manifest);
    if !violations.is_empty() {
        eprintln!("reachability fixture manifest violations:");
        for violation in &violations {
            eprintln!("  - {violation}");
        }
        bail!("reachability fixture manifest check failed with {} violation(s)", violations.len());
    }
    fs::write(root.join(model::VIEW_RELATIVE_PATH), view::render(&manifest))
        .with_context(|| format!("failed to write {}", model::VIEW_RELATIVE_PATH))?;
    Ok(())
}

fn load_manifest(root: &Path) -> Result<model::Manifest> {
    let text = read_text(root, model::MANIFEST_RELATIVE_PATH)?;
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", model::MANIFEST_RELATIVE_PATH))
}

fn validate(root: &Path) -> Result<CoverageStats> {
    validate_json_parse(root, SCHEMA_PATH)?;
    validate_schema_identity(root)?;
    let manifest = load_manifest(root)?;
    let mut violations = validate_document(root, &manifest);
    validate_generated_view(root, &manifest, &mut violations);

    if !violations.is_empty() {
        eprintln!("reachability fixture manifest violations:");
        for violation in &violations {
            eprintln!("  - {violation}");
        }
        bail!("reachability fixture manifest check failed with {} violation(s)", violations.len());
    }

    let stats = CoverageStats {
        rows: manifest.rows.len(),
        declared_row_count: manifest.declared_row_count,
        families_covered: model::FAMILIES
            .iter()
            .filter(|family| manifest.rows.iter().any(|row| row.train.family == **family))
            .count(),
        not_proven_instruments: manifest
            .rows
            .iter()
            .filter(|row| {
                row.instrument.as_ref().is_some_and(|instrument| {
                    matches!(instrument.status, model::InstrumentStatusKind::Missing)
                })
            })
            .count(),
        deferred_slots: manifest
            .denominator
            .iter()
            .map(|entry| entry.deferred_coverage.len())
            .sum(),
    };
    Ok(stats)
}

/// Validates one parsed manifest document against the repo root. The
/// generated-view drift rule is enforced by [`validate`] only, so self-fixture
/// documents can be checked without touching repository state.
pub(crate) fn validate_document(root: &Path, manifest: &model::Manifest) -> Vec<String> {
    let mut violations = Vec::new();
    validate_shape(manifest, &mut violations);
    validate_rows(root, manifest, &mut violations);
    validate_coverage(manifest, &mut violations);
    violations
}

/// Parses a manifest document, surfacing closed-schema (deny_unknown_fields)
/// violations as parse errors.
#[cfg(test)]
pub(crate) fn parse_document(text: &str) -> Result<model::Manifest> {
    serde_json::from_str(text).map_err(Into::into)
}

fn validate_json_parse(root: &Path, rel: &str) -> Result<()> {
    let text = read_text(root, rel)?;
    let _: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("failed to parse {rel} as JSON"))?;
    Ok(())
}

fn validate_schema_identity(root: &Path) -> Result<()> {
    let text = read_text(root, SCHEMA_PATH)?;
    let value: serde_json::Value = serde_json::from_str(&text)?;
    let id_ok = value.get("$id").and_then(serde_json::Value::as_str)
        == Some(
            "https://effortlessmetrics.dev/perl-lsp/schemas/analysis_reachability_fixture_manifest.v1.schema.json",
        );
    let version_ok = value.pointer("/properties/schema/const").and_then(serde_json::Value::as_str)
        == Some(model::SCHEMA_ID);
    if !id_ok || !version_ok {
        bail!("{SCHEMA_PATH} does not pin the current schema identity");
    }
    Ok(())
}

fn read_text(root: &Path, rel: &str) -> Result<String> {
    let path = root.join(rel);
    fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
}

fn validate_shape(manifest: &model::Manifest, violations: &mut Vec<String>) {
    const DOC: &str = MANIFEST_RELATIVE_PATH;
    if manifest.schema != model::SCHEMA_ID {
        violations.push(format!(
            "{DOC}: schema is {:?}; expected {:?}",
            manifest.schema,
            model::SCHEMA_ID
        ));
    }
    if manifest.schema_version != model::SCHEMA_VERSION {
        violations.push(format!("{DOC}: schema_version must be {}", model::SCHEMA_VERSION));
    }
    if manifest.manifest != model::MANIFEST_NAME {
        violations.push(format!(
            "{DOC}: manifest is {:?}; expected {:?}",
            manifest.manifest,
            model::MANIFEST_NAME
        ));
    }
    if manifest.owner_issue != 10998 {
        violations.push(format!("{DOC}: owner_issue must be 10998"));
    }
    if manifest.status != "declaration-only" {
        violations
            .push(format!("{DOC}: status is {:?}; expected \"declaration-only\"", manifest.status));
    }
    if manifest.digest_algorithm != model::DIGEST_ALGORITHM {
        violations.push(format!(
            "{DOC}: digest_algorithm is {:?}; expected {:?}",
            manifest.digest_algorithm,
            model::DIGEST_ALGORITHM
        ));
    }
    let actual_row_count = manifest.rows.len();
    if manifest.declared_row_count as usize != actual_row_count {
        // Name every row id actually present so a reviewer can diff the
        // declared population against the surviving one.
        let present_ids: BTreeSet<&str> =
            manifest.rows.iter().map(|row| row.row_id.as_str()).collect();
        violations.push(format!(
            "{DOC}: declared_row_count {} does not match the {} declared rows; row ids present: {}",
            manifest.declared_row_count,
            actual_row_count,
            present_ids.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    for phrase in REQUIRED_CLAIM_PHRASES {
        if !manifest.claim_boundary.to_ascii_lowercase().contains(phrase) {
            violations.push(format!("{DOC}: claim_boundary must include phrase {phrase:?}"));
        }
    }
    if manifest.allowed_fixture_roots.is_empty() {
        violations.push(format!("{DOC}: allowed_fixture_roots must not be empty"));
    } else {
        for entry in &manifest.allowed_fixture_roots {
            if !is_known_root(entry) {
                violations.push(format!(
                    "{DOC}: allowed_fixture_root {entry:?} is not a declared repository fixture root"
                ));
            }
        }
    }
    for family in model::FAMILIES {
        if !manifest.proof_owners.contains_key(*family) {
            violations.push(format!("{DOC}: proof_owners missing family {family:?}"));
        }
    }
    for (family, owner) in &manifest.proof_owners {
        if !model::FAMILIES.contains(&family.as_str()) {
            violations.push(format!("{DOC}: proof_owners declares unknown family {family:?}"));
        }
        if !model::PROOF_OWNER_ISSUES.contains(owner) {
            violations.push(format!(
                "{DOC}: proof_owners[{family}] = {owner} is not a declared proof-owner issue"
            ));
        }
    }
}

/// Roots this manifest version may declare; extending the set is a schema
/// revision, not a per-row decision.
fn is_known_root(root: &str) -> bool {
    matches!(root, "test_corpus" | "crates")
}

fn validate_rows(root: &Path, manifest: &model::Manifest, violations: &mut Vec<String>) {
    let mut seen_row_ids = BTreeSet::new();
    let mut fixture_identities: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    let row_ids: BTreeSet<&str> = manifest.rows.iter().map(|row| row.row_id.as_str()).collect();

    for row in &manifest.rows {
        let doc = format!("{}: row {}", MANIFEST_RELATIVE_PATH, row.row_id);
        if !is_valid_row_id(&row.row_id) {
            violations
                .push(format!("{doc}: row_id {:?} does not match [a-z0-9][a-z0-9.-]*", row.row_id));
        }
        if !seen_row_ids.insert(row.row_id.as_str()) {
            violations.push(format!("{MANIFEST_RELATIVE_PATH}: duplicate row id {:?}", row.row_id));
        }

        // Rule 1: stable fixture identities; same id must pin same bytes.
        let identity = (row.fixture.path.as_str(), row.fixture.digest_sha256_lf.as_str());
        if let Some(existing) = fixture_identities.get(row.fixture.id.as_str()) {
            if *existing != identity {
                violations.push(format!(
                    "{doc}: unstable fixture identity {:?} re-declares path/digest ({:?}, {}) after ({:?}, {})",
                    row.fixture.id, existing.0, existing.1, identity.0, identity.1,
                ));
            }
        } else {
            fixture_identities.insert(row.fixture.id.as_str(), identity);
        }

        validate_fixture_reference(root, &doc, row, &manifest.allowed_fixture_roots, violations);
        // Rule 10: implementation-authored snapshots may be retained as
        // observed output but never serve as the expected oracle of a
        // promoted row.
        if row.fixture.role.is_promoted() && row.oracle.oracle_type.is_implementation_derived() {
            violations.push(format!(
                "{doc}: implementation-derived observed output cannot serve as the expected oracle"
            ));
        }
        validate_subjects_and_roles(&doc, row, violations);
        validate_controls(&doc, row, &row_ids, violations);
        validate_expectations(&doc, row, violations);
        validate_terminal_and_limitation(&doc, row, violations);
        validate_owner(&doc, row, manifest, violations);
        validate_authority_reference(root, &doc, row, &manifest.allowed_fixture_roots, violations);
    }
}

/// A pinned authority reference must live inside the declared fixture roots
/// and point at existing bytes; byte-drift checking itself stays owned by the
/// consumer proof named in its note.
fn validate_authority_reference(
    root: &Path,
    doc: &str,
    row: &model::Row,
    allowed_roots: &[String],
    violations: &mut Vec<String>,
) {
    let Some(reference) = &row.authority_reference else {
        return;
    };
    let path = &reference.path;
    if path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || Path::new(path).is_absolute()
    {
        violations.push(format!(
            "{doc}: authority reference path must be repo-relative slash form: {path}"
        ));
        return;
    }
    let within_declared_root = allowed_roots.iter().any(|allowed| {
        path == allowed.as_str()
            || (path.starts_with(allowed.as_str())
                && path.strip_prefix(allowed.as_str()).is_some_and(|rest| rest.starts_with('/')))
    });
    if !within_declared_root {
        violations.push(format!("{doc}: authority reference escapes owned fixture roots: {path}"));
        return;
    }
    if !root.join(path).is_file() {
        violations.push(format!("{doc}: authority reference points to missing file {path}"));
    }
}

fn validate_fixture_reference(
    root: &Path,
    doc: &str,
    row: &model::Row,
    allowed_roots: &[String],
    violations: &mut Vec<String>,
) {
    let path = &row.fixture.path;
    if path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || Path::new(path).is_absolute()
    {
        violations.push(format!("{doc}: fixture path must be repo-relative slash form: {path}"));
        return;
    }
    let within_declared_root = allowed_roots.iter().any(|allowed| {
        path == allowed.as_str()
            || (path.starts_with(allowed.as_str())
                && path.strip_prefix(allowed.as_str()).is_some_and(|rest| rest.starts_with('/')))
    });
    if !within_declared_root {
        violations.push(format!(
            "{doc}: fixture source escapes owned fixture roots without disposition: {path}"
        ));
        return;
    }
    let full_path = root.join(path);
    if !full_path.is_file() {
        violations.push(format!("{doc}: fixture path points to missing file {path}"));
        return;
    }
    let actual = digest_file(&full_path).unwrap_or_else(|_| "<digest-error>".to_string());
    if actual != row.fixture.digest_sha256_lf {
        violations.push(format!(
            "{doc}: fixture digest drift for {path}: recorded {}, computed {}",
            row.fixture.digest_sha256_lf, actual
        ));
    }
}

/// [`model::DIGEST_ALGORITHM`] implementation: read the checked-out bytes,
/// normalize CRLF line endings to LF, then SHA-256 the stream. Only `\r\n`
/// sequences rewrite; a bare carriage return is semantic payload, so it moves
/// the digest and cannot silently evade fixture-identity checking.
fn digest_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    let mut bytes_iter = bytes.iter().copied().peekable();
    while let Some(byte) = bytes_iter.next() {
        match byte {
            b'\r' => {
                if bytes_iter.peek() == Some(&b'\n') {
                    bytes_iter.next();
                    hasher.update([b'\n']);
                } else {
                    hasher.update([b'\r']);
                }
            }
            other => hasher.update([other]),
        }
    }
    let digest = hasher.finalize();
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>())
}

fn validate_subjects_and_roles(doc: &str, row: &model::Row, violations: &mut Vec<String>) {
    if row.subjects.is_empty() {
        violations.push(format!("{doc}: subjects must name at least one subject identity"));
    }
    if row.subjects.iter().any(String::is_empty) {
        violations.push(format!("{doc}: subjects contains an empty identity"));
    }
    if row.source_roles.is_empty() {
        violations
            .push(format!("{doc}: source_roles must declare at least one logical-source role"));
    }
}

fn validate_controls(
    doc: &str,
    row: &model::Row,
    row_ids: &BTreeSet<&str>,
    violations: &mut Vec<String>,
) {
    let has_control = row.controls.opposite.is_some() || row.controls.near_neighbour.is_some();
    if row.fixture.role.is_promoted() && !has_control {
        // Rule 3: promoted positive rows require an opposite-direction control.
        violations
            .push(format!("{doc}: promoted positive row lacks an opposite/near-neighbour control"));
    }
    for reference in [&row.controls.opposite, &row.controls.near_neighbour].into_iter().flatten() {
        if *reference == row.row_id {
            // A self-referential control supplies no opposite-direction or
            // neighbourhood evidence, so it can never satisfy the falsifier
            // requirement a promoted row relies on.
            violations.push(format!("{doc}: control cannot reference its own row id {:?}", reference));
            continue;
        }
        if !row_ids.contains(reference.as_str()) {
            violations.push(format!("{doc}: control links unknown row {:?}", reference));
        }
    }
}

fn validate_expectations(doc: &str, row: &model::Row, violations: &mut Vec<String>) {
    if row.expectations.populated() == 0 {
        // Rule 8: at least one separated expectation object must be present.
        violations.push(format!("{doc}: expectations carry no separated proposition objects"));
        return;
    }
    if let Some(operation) = &row.expectations.operation {
        for dimension in &operation.work_dimensions {
            if dimension.trim().is_empty() {
                violations.push(format!("{doc}: operation work_dimensions contains an empty item"));
            }
        }
        if operation.stage.requires_work_dimensions() && operation.work_dimensions.is_empty() {
            violations.push(format!(
                "{doc}: operation stage {:?} requires declared work dimensions",
                operation.stage
            ));
        }
    }
    if let Some(profile) = &row.expectations.profile_budget {
        // Rule 12: profiles must disposition required work dimensions and may
        // never advertise unsafe partial support.
        if profile.required_work_dimensions.is_empty() {
            violations.push(format!("{doc}: profile_budget omits required work dimensions"));
        }
        if matches!(profile.profile, model::ProfileName::WorkspacePartial)
            && profile.partial_support_advertised
        {
            violations.push(format!(
                "{doc}: workspace_partial profile advertises unsafe partial support before safe stream commit proof"
            ));
        }
    }
    if let Some(policy) = &row.expectations.policy
        && policy.reason.trim().is_empty()
    {
        violations.push(format!("{doc}: policy expectation requires an eligibility reason"));
    }
    if let Some(transport) = &row.expectations.transport
        && transport.route.requires_client_visible_expectation()
        && transport.client_visible_expectation.as_deref().unwrap_or("").trim().is_empty()
    {
        violations.push(format!(
            "{doc}: transport route {:?} requires a client-visible expectation",
            transport.route
        ));
    }

    // Rules 7/13: bounded-view/incomplete collapse and race-row client
    // visibility.
    let incomplete_semantic =
        matches!(row.terminal, model::TerminalOutcome::IncompleteSemanticNeverBoundedComplete);
    let bounded_complete = matches!(row.terminal, model::TerminalOutcome::BoundedViewComplete)
        || row.expectations.operation.as_ref().is_some_and(|operation| {
            matches!(operation.terminal_outcome, model::TerminalOutcome::BoundedViewComplete)
        });
    if incomplete_semantic && bounded_complete {
        violations.push(format!(
            "{doc}: complete bounded view and incomplete semantic computation collapse in one row"
        ));
    }
    if let Some(barrier) = &row.race_barrier
        && barrier.kind.requires_client_visible_expectation()
        && row
            .expectations
            .transport
            .as_ref()
            .and_then(|transport| transport.client_visible_expectation.as_deref())
            .unwrap_or("")
            .trim()
            .is_empty()
    {
        violations.push(format!(
            "{doc}: race barrier {:?} lacks an exact client-visible/currentness expectation",
            barrier.kind
        ));
    }
    if let Some(currentness) = &row.expectations.currentness
        && currentness.proposition.trim().is_empty()
    {
        violations.push(format!("{doc}: currentness proposition must not be empty"));
    }
}

fn validate_terminal_and_limitation(doc: &str, row: &model::Row, violations: &mut Vec<String>) {
    let terminal = row.terminal;
    if terminal.is_exact_result() {
        // Rule 5: exact results need a named identity/completeness authority;
        // the result identity object itself carries that authority.
        if row.result_identity.is_none() {
            violations.push(format!(
                "{doc}: terminal {:?} requires named result identity authority",
                terminal
            ));
        }
        if let Some(identity) = &row.result_identity {
            if identity.identity.trim().is_empty() {
                violations.push(format!("{doc}: result identity must not be empty"));
            }
            if matches!(
                terminal,
                model::TerminalOutcome::CompleteNonempty
                    | model::TerminalOutcome::CompleteLegitimateEmpty
            ) && !matches!(identity.completeness, model::CompletenessClaim::SemanticComplete)
            {
                violations.push(format!(
                    "{doc}: complete terminal claims non-semantic completeness authority"
                ));
            }
            if matches!(terminal, model::TerminalOutcome::BoundedViewComplete)
                && !matches!(identity.completeness, model::CompletenessClaim::BoundedViewComplete)
            {
                violations.push(format!(
                    "{doc}: bounded-view terminal must claim bounded-view completeness, never semantic completeness"
                ));
            }
        }
    } else if terminal.is_non_success() && row.result_identity.is_some() {
        // Rule 6: unsupported/partial/cancelled/stale/instrument rows are
        // never empty successes carrying result identity.
        violations.push(format!(
            "{doc}: non-success terminal {:?} must not carry result identity",
            terminal
        ));
    }
    if terminal.requires_instrument_receipt() {
        // Missing instruments stay explicit NOT_PROVEN; they can never be
        // inferred zeros (rule 16).
        let instrument_ok = row.instrument.as_ref().is_some_and(|instrument| {
            instrument.disposition.trim().eq_ignore_ascii_case("not_proven")
                || matches!(instrument.status, model::InstrumentStatusKind::Present)
        });
        if !instrument_ok {
            violations.push(format!(
                "{doc}: terminal {:?} requires present instrumentation or an explicit not_proven disposition",
                terminal
            ));
        }
    }
    if let Some(instrument) = &row.instrument
        && matches!(instrument.status, model::InstrumentStatusKind::Missing)
        && !instrument.disposition.trim().eq_ignore_ascii_case("not_proven")
    {
        // Any missing instrument is NOT_PROVEN regardless of terminal class;
        // the generated view counts it that way, so a divergent disposition
        // would contradict the canonical evidence surface.
        violations.push(format!(
            "{doc}: missing instrumentation requires the explicit not_proven disposition"
        ));
    }
    if row.limitation.support_class.requires_exit_owner()
        && row.limitation.exit_owner_issue.is_none()
    {
        violations.push(format!(
            "{doc}: limitation {:?} requires a named exit owner issue",
            row.limitation.support_class
        ));
    }
    if let Some(exit_owner) = row.limitation.exit_owner_issue
        && exit_owner == 0
    {
        violations.push(format!("{doc}: exit_owner_issue must be a real issue number"));
    }
}

fn validate_owner(
    doc: &str,
    row: &model::Row,
    manifest: &model::Manifest,
    violations: &mut Vec<String>,
) {
    // Rule 11: every row maps to one declared proof owner for its family.
    let default_owner = manifest.proof_owners.get(&row.train.family).copied();
    let resolved = row.owner_issue.or(default_owner);
    match resolved {
        None => violations
            .push(format!("{doc}: no owner issue resolves from row override or family default")),
        Some(owner) => {
            if !model::PROOF_OWNER_ISSUES.contains(&owner) {
                violations.push(format!(
                    "{doc}: owner issue {owner} is not a declared proof-owner issue"
                ));
            }
        }
    }
}

/// One machine-checkable coverage slot: an operation-stage or terminal-outcome
/// vocabulary member addressed by its wire name.
#[derive(Debug, Clone, Copy)]
enum CoverageSlot {
    Stage(model::OperationStage),
    Terminal(model::TerminalOutcome),
}

/// Parses the `stage:<wire>` / `terminal:<wire>` token grammar used by
/// `required_coverage` entries and recognized deferrals. Free-text classes
/// stay allowed on deferrals but cannot satisfy a vocabulary slot.
fn parse_coverage_slot(entry: &str) -> Option<CoverageSlot> {
    let (kind, name) = entry.split_once(':')?;
    match kind {
        "stage" => model::OperationStage::ALL
            .iter()
            .copied()
            .find(|stage| stage.wire_name() == name)
            .map(CoverageSlot::Stage),
        "terminal" => model::TerminalOutcome::ALL
            .iter()
            .copied()
            .find(|terminal| terminal.wire_name() == name)
            .map(CoverageSlot::Terminal),
        _ => None,
    }
}

fn family_declares_deferral(manifest: &model::Manifest, family: &str, expected: &str) -> bool {
    manifest.denominator.iter().any(|entry| {
        entry.family == family
            && entry.deferred_coverage.iter().any(|slot| slot.coverage == expected)
    })
}

fn validate_coverage(manifest: &model::Manifest, violations: &mut Vec<String>) {
    const DOC: &str = MANIFEST_RELATIVE_PATH;
    // Rule 4: claimed families/profiles/stages keep instantiated denominator
    // rows unless the slot is explicitly deferred to a named owner.
    for family in model::FAMILIES {
        let declared = manifest.denominator.iter().find(|entry| entry.family == *family);
        let has_rows = manifest.rows.iter().any(|row| row.train.family == *family);
        // Only named deferred slots (reason + owner) excuse a missing
        // population. Required coverage strings demand instantiation; they are
        // never themselves a deferral.
        let has_deferral =
            declared.is_some_and(|entry| !entry.deferred_coverage.is_empty());
        if !has_rows && !has_deferral {
            violations.push(format!(
                "{DOC}: family {family:?} claims denominator coverage without any row"
            ));
        }
    }
    for entry in &manifest.denominator {
        if !model::FAMILIES.contains(&entry.family.as_str()) {
            violations
                .push(format!("{DOC}: denominator declares unknown family {:?}", entry.family));
        }
        // Every required slot must resolve to real row content in this family
        // or to an explicitly named deferral in the same family.
        for requirement in &entry.required_coverage {
            match parse_coverage_slot(requirement) {
                Some(CoverageSlot::Stage(stage)) => {
                    let satisfied = manifest.rows.iter().any(|row| {
                        row.train.family == entry.family
                            && row
                                .expectations
                                .operation
                                .as_ref()
                                .is_some_and(|operation| operation.stage == stage)
                    });
                    if !satisfied && !family_declares_deferral(manifest, &entry.family, requirement)
                    {
                        violations.push(format!(
                            "{DOC}: family {:?} declares required_coverage {:?} without any denominator row instantiating operation stage {:?}",
                            entry.family, requirement, stage
                        ));
                    }
                }
                Some(CoverageSlot::Terminal(terminal)) => {
                    let satisfied = manifest
                        .rows
                        .iter()
                        .any(|row| row.train.family == entry.family && row.terminal == terminal);
                    if !satisfied && !family_declares_deferral(manifest, &entry.family, requirement)
                    {
                        violations.push(format!(
                            "{DOC}: family {:?} declares required_coverage {:?} without any denominator row declaring terminal outcome {:?}",
                            entry.family, requirement, terminal
                        ));
                    }
                }
                None => {
                    violations.push(format!(
                        "{DOC}: family {:?} declares unparseable required_coverage entry {:?}; use \"stage:<name>\" or \"terminal:<name>\" with a vocabulary wire name",
                        entry.family, requirement
                    ));
                }
            }
        }
        for slot in &entry.deferred_coverage {
            if slot.reason.trim().is_empty() {
                violations.push(format!(
                    "{DOC}: deferred slot {:?} in {:?} requires a reason",
                    slot.coverage, entry.family
                ));
            }
            if slot.owner_issue == 0 {
                violations.push(format!(
                    "{DOC}: deferred slot {:?} in {:?} requires an owner issue",
                    slot.coverage, entry.family
                ));
            }
            // A mistyped vocabulary token would silently void the
            // completeness pass below; recognize-but-misname fails closed.
            if (slot.coverage.starts_with("stage:") || slot.coverage.starts_with("terminal:"))
                && parse_coverage_slot(&slot.coverage).is_none()
            {
                violations.push(format!(
                    "{DOC}: deferred slot {:?} in {:?} names an unknown vocabulary slot; use a wire name after \"stage:\"/\"terminal:\"",
                    slot.coverage, entry.family
                ));
            }
        }
    }
    // Completeness: every operation-stage and terminal-outcome vocabulary slot
    // must have at least one instantiated row or a named deferral somewhere.
    for stage in model::OperationStage::ALL {
        let has_row = manifest.rows.iter().any(|row| {
            row.expectations.operation.as_ref().is_some_and(|operation| operation.stage == *stage)
        });
        let has_deferral = manifest.denominator.iter().any(|entry| {
            entry
                .deferred_coverage
                .iter()
                .any(|slot| slot.coverage == format!("stage:{}", stage.wire_name()))
        });
        if !has_row && !has_deferral {
            violations.push(format!(
                "{DOC}: operation stage {:?} ({}) has no denominator row and no named deferral",
                stage,
                stage.wire_name()
            ));
        }
    }
    for terminal in model::TerminalOutcome::ALL {
        let has_row = manifest.rows.iter().any(|row| row.terminal == *terminal);
        let has_deferral = manifest.denominator.iter().any(|entry| {
            entry
                .deferred_coverage
                .iter()
                .any(|slot| slot.coverage == format!("terminal:{}", terminal.wire_name()))
        });
        if !has_row && !has_deferral {
            violations.push(format!(
                "{DOC}: terminal outcome {:?} ({}) has no denominator row and no named deferral",
                terminal,
                terminal.wire_name()
            ));
        }
    }
}

fn validate_generated_view(root: &Path, manifest: &model::Manifest, violations: &mut Vec<String>) {
    // Rule 14: generated coverage/status views never drift silently.
    let rendered = view::render(manifest);
    match fs::read_to_string(root.join(model::VIEW_RELATIVE_PATH)) {
        Ok(existing) => {
            if existing != rendered {
                violations.push(format!(
                    "{}: generated view drifted from the manifest; rerun `cargo xtask check-reachability-fixture-manifest --update-view`",
                    model::VIEW_RELATIVE_PATH
                ));
            }
        }
        Err(_) => violations.push(format!(
            "{}: missing generated view; rerun `cargo xtask check-reachability-fixture-manifest --update-view`",
            model::VIEW_RELATIVE_PATH
        )),
    }
}

#[cfg(test)]
mod tests;
