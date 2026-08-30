//! `public_release_claims.v2` — deterministic install-claim inventory derivative (#11548).
//!
//! Scoping authority: the shape-(a) receipt on #11549 (comment 5458545004).
//! The catalog consumes the landed inventory
//! `docs/distribution/INSTALL_CLAIM_SURFACES.md` (#11575) as its only join
//! input: every surface row (`S01`-`S13`) and claim row (`C101`-`C1309`)
//! becomes one v2 record. This derivative is explicitly NON-authoritative: it
//! carries no route IDs and no projection contexts, and it does not claim the
//! #10333/#10334 true-v2 route-contract authority.
//!
//! Defect-clearing mechanics from the closed #12858 review (contract:
//! `.spec/11548-inventory-derivative/acceptance.md`):
//!
//! - D1: `dimensions.windows_arm64.published_receipt_v0_17_0` is derived from
//!   the mirrored release-asset manifest
//!   (`distribution/release_receipts/v0.17.0.assets.json`), never asserted
//!   from prose; the validator binds that manifest to the inventory release
//!   anchor and cross-checks every receipt field against it.
//! - D2: the crates.io four-name collision is a first-class closed-schema
//!   `identity_anti_claims` record, not omission-caveat prose; rows whose raw
//!   inventory text states the collision or asserts the crates.io registry
//!   route must carry it.
//! - D3: code-span delimiters are trimmed symmetrically (one matched boundary
//!   pair, else verbatim) and unbalanced delimiters are rejected.
//! - D4: finding-claim relations derive from the inventory-cited file:line to
//!   claim-location join (same-cited-file mapping), not from literal `FND-n`
//!   token scans; the regression assertion is FND-1 relates C401.
//! - D5: the Python oracle (`scripts/validate_public_release_claims_v2.py`)
//!   re-derives every row from the inventory source and probes tampering per
//!   row; this module keeps the byte-canonical regeneration gate on the Rust
//!   side.
//! - D6: every nested object is schema-closed; the validator walks the schema
//!   and asserts `additionalProperties: false` on every object node.

use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeMap, fmt, fs, path::Path};

pub const DOC_PATH: &str = "docs/distribution/INSTALL_CLAIM_SURFACES.md";
pub const SCHEMA_PATH: &str = "schemas/public_release_claims.v2.schema.json";
pub const RECEIPT_MANIFEST_PATH: &str = "distribution/release_receipts/v0.17.0.assets.json";
pub const ARTIFACT_PATH: &str = "distribution/public_release_claims.v2.json";
pub const SCHEMA_VERSION: &str = "public_release_claims.v2";
pub const GENERATOR_COMMAND: &str = "cargo xtask public-release-claims-v2 build --write";

/// Complete surface denominator from the landed inventory.
const EXPECTED_SURFACES: [&str; 13] =
    ["S01", "S02", "S03", "S04", "S05", "S06", "S07", "S08", "S09", "S10", "S11", "S12", "S13"];

/// Complete claim-row denominator. A missing or renamed inventory row fails
/// the check (missing-producer-omits-route falsifier); adding rows upstream is
/// a sanctioned regeneration, never a silent pass.
const EXPECTED_CLAIM_IDS: [&str; 70] = [
    "C101", "C102", "C103", "C104", "C105", "C106", "C107", "C108", "C201", "C202", "C203", "C204",
    "C205", "C206", "C207", "C208", "C209", "C210", "C211", "C212", "C213", "C214", "C215", "C216",
    "C301", "C302", "C303", "C401", "C402", "C403", "C404", "C405", "C406", "C501", "C502", "C503",
    "C601", "C701", "C702", "C703", "C801", "C901", "C902", "C1001", "C1002", "C1003", "C1004",
    "C1005", "C1006", "C1007", "C1008", "C1101", "C1102", "C1201", "C1202", "C1203", "C1204",
    "C1205", "C1206", "C1207", "C1208", "C1301", "C1302", "C1303", "C1304", "C1305", "C1306",
    "C1307", "C1308", "C1309",
];

const DRIFT_STATUSES: [&str; 8] = [
    "current",
    "pending",
    "stale_example",
    "future_example",
    "mutable_pin",
    "cross_surface_drift",
    "source_drift",
    "volatile_number",
];

const FINDING_IDS: [&str; 12] = [
    "FND-1", "FND-2", "FND-3", "FND-4", "FND-5", "FND-6", "FND-7", "FND-8", "FND-9", "FND-10",
    "FND-11", "FND-12",
];

/// Recorded disposition pointers, exactly as stated in the inventory text.
const FINDING_OWNERS: [(&str, &str); 3] = [
    ("FND-4", "#11549-classifier"),
    ("FND-10", "#10342-ci-cutover"),
    ("FND-11", "distribution-docs-sync"),
];

/// D4 regression pair required by the scoping receipt: FND-1 cites
/// `action.yml:3`, whose claim row is C401; the join must relate them.
pub const REGRESSION_FINDING: &str = "FND-1";
pub const REGRESSION_CLAIM: &str = "C401";

/// D2 anti-claim record: closed, const-valued; every derived row carries
/// exactly this shape.
const ANTI_CLAIM_KIND: &str = "crates_io_name_collision";
const ANTI_CLAIM_FOREIGN: &str = "perl-lsp";
const ANTI_CLAIM_OWNED: &str = "perllsp";
const ANTI_CLAIM_DISPOSITION: &str = "do_not_install";

#[derive(Debug)]
pub struct CatalogError(String);

impl CatalogError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CatalogError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogStats {
    pub surfaces: usize,
    pub claims: usize,
    pub findings: usize,
    pub dimensioned_rows: usize,
    pub anti_claimed_rows: usize,
    pub derived_relations: usize,
}

#[derive(Debug)]
struct ParsedSurface {
    surface_id: String,
    path: String,
    role: String,
    claim_class: String,
    registry_cross_ref: String,
}

#[derive(Debug)]
struct ParsedClaim {
    claim_id: String,
    surface_id: String,
    location: String,
    summary: String,
    drift_status: String,
    notes: String,
    raw_row: String,
}

#[derive(Debug)]
struct ParsedFinding {
    finding_id: String,
    title: String,
    cited_files: Vec<String>,
}

#[derive(Debug)]
pub struct ParsedInventory {
    audited_commit: String,
    audited_date: String,
    release_anchor: String,
    surfaces: Vec<ParsedSurface>,
    claims: Vec<ParsedClaim>,
    findings: Vec<ParsedFinding>,
}

/// Mirrored release-asset manifest (receipt authority, D1).
#[derive(Debug)]
pub struct ParsedReceiptManifest {
    pub release: String,
    pub source: String,
    pub verified_date: String,
    pub asset_names: Vec<String>,
}

fn read_repo_bytes(root: &Path, rel: &str) -> Result<Vec<u8>, CatalogError> {
    fs::read(root.join(rel))
        .map_err(|error| CatalogError::new(format!("{rel}: cannot read: {error}")))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn canonical_bytes(value: &Value) -> Result<Vec<u8>, CatalogError> {
    let mut text = serde_json::to_string_pretty(value).map_err(|error| {
        CatalogError::new(format!("catalog: cannot serialize canonical form: {error}"))
    })?;
    text.push('\n');
    Ok(text.into_bytes())
}

// ---------------------------------------------------------------------------
// D3: symmetric code-span cell extraction
// ---------------------------------------------------------------------------

/// Markdown links keep their display label; everything else stays verbatim.
fn resolve_link_label(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with('[')
        && let Some(close) = trimmed.find(']')
    {
        return trimmed[1..close].to_string();
    }
    trimmed.to_string()
}

/// D3: drop the boundary backticks only when they form a matched pair
/// delimiting one whole-cell code span (the closing delimiter of that same
/// span is the final character); anything else stays verbatim. Interior
/// delimiters are never touched, so markdown structure cannot re-pair.
fn trim_code_span_pair(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('`') && trimmed.ends_with('`') {
        let inner = &trimmed[1..];
        if let Some(second) = inner.find('`')
            && second + 1 == inner.len()
        {
            return inner[..second].to_string();
        }
    }
    trimmed.to_string()
}

/// D3: a derived cell is rejected when its code-span delimiters are
/// unbalanced (odd backtick count).
fn backticks_balanced(text: &str) -> bool {
    text.matches('`').count().is_multiple_of(2)
}

fn extract_cell_text(cell: &str, context: &str) -> Result<String, CatalogError> {
    let normalized = trim_code_span_pair(&resolve_link_label(cell));
    if !backticks_balanced(&normalized) {
        return Err(CatalogError::new(format!(
            "{DOC_PATH}: {context}: unbalanced code-span delimiters in derived cell \
             (D3 rejects malformed rows): `{normalized}`"
        )));
    }
    Ok(normalized)
}

/// Code-span contents of a raw table row, in order (segments between
/// alternating backticks). Rows with unbalanced delimiters yield the
/// well-formed prefix; the per-cell balance check fails them separately.
fn code_spans(raw: &str) -> Vec<String> {
    let mut spans = Vec::new();
    let mut parts = raw.split('`');
    parts.next();
    while let Some(span) = parts.next() {
        spans.push(span.to_string());
        if parts.next().is_none() {
            break;
        }
    }
    spans
}

// ---------------------------------------------------------------------------
// D2: crates.io name-collision anti-claim derivation
// ---------------------------------------------------------------------------

/// The row's raw inventory text states the four-name collision: it names the
/// backticked foreign crates.io package in a crates.io context, or talks
/// about the collision directly.
fn states_crates_io_collision(raw_row: &str) -> bool {
    let foreign_backticked = format!("`{ANTI_CLAIM_FOREIGN}`");
    (raw_row.contains("crates.io") && raw_row.contains(&foreign_backticked))
        || raw_row.contains("collision")
}

/// The row asserts a crates.io registry route: some code span is a
/// `cargo install ... perllsp` invocation that is neither `--path` (local
/// checkout) nor `--git` (unpinned source route).
fn asserts_registry_route(raw_row: &str) -> bool {
    code_spans(raw_row).iter().any(|span| {
        let span = span.trim();
        span.starts_with("cargo install")
            && span.contains(ANTI_CLAIM_OWNED)
            && !span.contains("--path")
            && !span.contains("--git")
    })
}

fn identity_anti_claim_record() -> Value {
    json!({
        "kind": ANTI_CLAIM_KIND,
        "foreign_name": ANTI_CLAIM_FOREIGN,
        "owned_name": ANTI_CLAIM_OWNED,
        "disposition": ANTI_CLAIM_DISPOSITION,
    })
}

/// `C<n>`-style claim-ID tokens referenced inside a raw inventory row.
fn extract_claim_refs(raw_row: &str) -> Vec<String> {
    let bytes = raw_row.as_bytes();
    let mut refs = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'C' || !bytes.get(index + 1).is_some_and(|byte| byte.is_ascii_digit()) {
            index += 1;
            continue;
        }
        // The ID must not continue an alphanumeric token (e.g. `SEC401`).
        if index > 0 && (bytes[index - 1].is_ascii_alphanumeric() || bytes[index - 1] == b'_') {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() && end - index <= 4 {
            end += 1;
        }
        let digits = end - index - 1;
        if (3..=4).contains(&digits) {
            let id = raw_row[index..end].to_string();
            if !refs.contains(&id) {
                refs.push(id);
            }
        }
        index = end;
    }
    refs
}

/// D2 derivation: rows whose raw inventory text states the collision or
/// asserts the crates.io registry route carry the anti-claim, and a row that
/// explicitly references an anti-claimed row's route set by claim ID (e.g.
/// C1305 deferring to C1304) inherits it. The set is mechanical; the Python
/// oracle re-derives it independently.
fn derive_anti_claim_ids_from_claims(claims: &[ParsedClaim]) -> Vec<String> {
    let direct: Vec<&str> = claims
        .iter()
        .filter(|claim| {
            states_crates_io_collision(&claim.raw_row) || asserts_registry_route(&claim.raw_row)
        })
        .map(|claim| claim.claim_id.as_str())
        .collect();
    let mut ids = Vec::new();
    for claim in claims {
        let referenced = extract_claim_refs(&claim.raw_row)
            .iter()
            .any(|reference| direct.contains(&reference.as_str()));
        if direct.contains(&claim.claim_id.as_str()) || referenced {
            ids.push(claim.claim_id.clone());
        }
    }
    ids
}

/// Claim IDs whose raw inventory text participates in the collision identity
/// (D2 derivation; the oracle re-derives the same set independently).
pub fn derive_anti_claim_ids(doc: &str) -> Result<Vec<String>, CatalogError> {
    let inventory = parse_inventory(doc)?;
    Ok(derive_anti_claim_ids_from_claims(&inventory.claims))
}

// ---------------------------------------------------------------------------
// D4: cited-file join for finding-claim relations
// ---------------------------------------------------------------------------

fn location_file(location: &str) -> String {
    let file = location.split(':').next().unwrap_or(location);
    file.rsplit('/').next().unwrap_or(file).to_string()
}

/// Extract `file.ext:line` citation tokens from finding prose. A citation
/// needs an explicit line reference (`:digit`) so prose mentions like
/// "the install.ps1 header" stay uncited.
fn extract_cited_files(body: &str) -> Vec<String> {
    let bytes = body.as_bytes();
    let mut files: Vec<String> = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if !bytes[index].is_ascii_alphanumeric() && bytes[index] != b'_' {
            index += 1;
            continue;
        }
        let start = index;
        let mut end = index;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric()
                || matches!(bytes[end], b'_' | b'.' | b'/' | b'-'))
        {
            end += 1;
        }
        let token = &body[start..end];
        index = end.max(start + 1);
        let Some(dot) = token.rfind('.') else {
            continue;
        };
        let extension = &token[dot + 1..];
        if !matches!(extension, "md" | "yml" | "yaml" | "sh" | "ps1" | "json" | "toml") {
            continue;
        }
        if bytes.get(end) != Some(&b':') {
            continue;
        }
        let Some(first_line_digit) = bytes.get(end + 1) else {
            continue;
        };
        if !first_line_digit.is_ascii_digit() {
            continue;
        }
        let base = token.rsplit('/').next().unwrap_or(token).to_string();
        if !files.contains(&base) {
            files.push(base);
        }
    }
    files.sort();
    files.dedup();
    files
}

/// Same-cited-file mapping: a finding relates to every claim row whose
/// Location cell cites a file the finding also cites (basename granularity,
/// deterministic superset).
fn derive_relations(
    claims: &[ParsedClaim],
    findings: &[ParsedFinding],
) -> BTreeMap<String, Vec<String>> {
    let mut relations: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for finding in findings {
        let mut related: Vec<String> = Vec::new();
        for claim in claims {
            let claim_file = location_file(&claim.location);
            if finding.cited_files.contains(&claim_file) {
                related.push(claim.claim_id.clone());
            }
        }
        related.sort_by_key(|id| numeric_claim_key(id));
        relations.insert(finding.finding_id.clone(), related);
    }
    relations
}

// ---------------------------------------------------------------------------
// Inventory parsing
// ---------------------------------------------------------------------------

fn numeric_claim_key(claim_id: &str) -> u32 {
    claim_id.trim_start_matches('C').parse().unwrap_or(u32::MAX)
}

/// Parse the landed inventory document into its structured denominator.
pub fn parse_inventory(doc: &str) -> Result<ParsedInventory, CatalogError> {
    let mut surfaces = Vec::new();
    let mut claims = Vec::new();
    let mut in_surface_index = false;
    let mut current_section: Option<String> = None;

    // Anchors wrap across markdown lines; parse them over the joined text.
    let joined = doc.replace('\n', " ");
    let Some((audited_commit, audited_date)) = parse_audited_anchor(&joined) else {
        return Err(CatalogError::new(format!(
            "{DOC_PATH}: could not locate the `**Audited against:**` commit/date anchor"
        )));
    };
    if audited_date.is_empty() {
        return Err(CatalogError::new(format!(
            "{DOC_PATH}: could not locate the audit date next to the audited commit"
        )));
    }
    let release_anchor = parse_release_anchor_after_marker(&joined, "**Drift anchor:**")
        .ok_or_else(|| {
            CatalogError::new(format!(
                "{DOC_PATH}: could not locate the drift-anchor release receipt"
            ))
        })?;

    for line in doc.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            in_surface_index = trimmed == "## Surface index";
            continue;
        }
        if trimmed.starts_with("### ") {
            let heading = trimmed.trim_start_matches("### ").trim();
            current_section = heading
                .split([' ', '\u{2014}', '-'])
                .next()
                .filter(|token| token.len() == 3 && token.starts_with('S'))
                .map(str::to_string);
            continue;
        }

        if in_surface_index {
            if trimmed.starts_with("| S") {
                surfaces.extend(parse_surface_row(trimmed)?);
            }
            continue;
        }
        if trimmed.starts_with("| C") {
            claims.extend(parse_claim_row(trimmed, current_section.as_deref())?);
        }
    }

    surfaces.sort_by(|left, right| left.surface_id.cmp(&right.surface_id));
    claims.sort_by(|left, right| {
        numeric_claim_key(&left.claim_id).cmp(&numeric_claim_key(&right.claim_id))
    });

    let findings = parse_findings(doc)?;
    for finding_id in FINDING_IDS {
        if !findings.iter().any(|finding| finding.finding_id == finding_id) {
            return Err(CatalogError::new(format!(
                "{DOC_PATH}: findings section is missing `{finding_id}`"
            )));
        }
    }

    Ok(ParsedInventory { audited_commit, audited_date, release_anchor, surfaces, claims, findings })
}

/// Extract each `- **FND-N — title.** body` bullet (multi-line safe), plus the
/// file:line citations in the body (the D4 join input).
fn parse_findings(doc: &str) -> Result<Vec<ParsedFinding>, CatalogError> {
    let joined = doc.replace('\n', " ");
    let mut findings = Vec::new();
    for number in 1..=12u32 {
        let marker = format!("**FND-{number} \u{2014} ");
        let Some(start) = joined.find(&marker) else {
            continue;
        };
        let after_marker = start + marker.len();
        let Some(title_end) = joined[after_marker..].find(".**") else {
            return Err(CatalogError::new(format!(
                "{DOC_PATH}: finding FND-{number} has no `.**` title terminator"
            )));
        };
        let title = joined[after_marker..after_marker + title_end]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if title.is_empty() {
            return Err(CatalogError::new(format!(
                "{DOC_PATH}: finding FND-{number} has an empty title"
            )));
        }
        // Body runs until the next finding bullet or the next section heading.
        let after_title = after_marker + title_end + 3;
        let next_bullet = joined[after_title..].find(&format!("- **FND-{}", number + 1));
        let next_section = joined[after_title..].find("## ");
        let body_end = [next_bullet, next_section]
            .into_iter()
            .flatten()
            .min()
            .map_or(joined.len(), |offset| after_title + offset);
        let cited_files = extract_cited_files(&joined[after_title..body_end]);
        findings.push(ParsedFinding { finding_id: format!("FND-{number}"), title, cited_files });
    }
    Ok(findings)
}

fn parse_audited_anchor(line: &str) -> Option<(String, String)> {
    let mut commit = None;
    let mut date = None;
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'`' => {
                let start = index + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end] != b'`' {
                    end += 1;
                }
                if end < bytes.len() {
                    let candidate = &line[start..end];
                    let hexy = !candidate.is_empty()
                        && candidate.chars().all(|c| c.is_ascii_hexdigit())
                        && (7..=40).contains(&candidate.len());
                    if hexy {
                        commit.get_or_insert(candidate.to_ascii_lowercase());
                    }
                    index = end + 1;
                    continue;
                }
            }
            b'(' => {
                let inner_end = line[index..].find(')').map(|offset| index + offset)?;
                let inner = &line[index + 1..inner_end];
                if inner.len() == 10
                    && inner.as_bytes()[4] == b'-'
                    && inner.as_bytes()[7] == b'-'
                    && inner.chars().enumerate().all(|(position, c)| {
                        ([4usize, 7].contains(&position)) || c.is_ascii_digit()
                    })
                {
                    date.get_or_insert_with(|| inner.to_string());
                }
                index = inner_end;
            }
            _ => {}
        }
        index += 1;
    }
    Some((commit?, date.unwrap_or_default()))
}

fn parse_release_anchor_after_marker(text: &str, marker: &str) -> Option<String> {
    let start = text.find(marker).map(|offset| offset + marker.len())?;
    let bytes = text.as_bytes();
    let mut index = start;
    while index < bytes.len() {
        if bytes[index] == b'v'
            && index > 0
            && !bytes[index - 1].is_ascii_alphanumeric()
            && bytes.get(index - 1) != Some(&b'.')
        {
            let mut end = index + 1;
            while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
                end += 1;
            }
            let candidate = &text[index..end];
            let numeric_parts = candidate.trim_start_matches('v');
            let parts: Vec<&str> = numeric_parts.split('.').collect();
            let shaped = parts.len() == 3
                && parts
                    .iter()
                    .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
            if shaped {
                return Some(candidate.to_string());
            }
        }
        index += 1;
    }
    None
}

fn split_table_row(row: &str) -> Vec<String> {
    let trimmed = row.trim();
    let inner =
        trimmed.strip_prefix('|').and_then(|rest| rest.strip_suffix('|')).unwrap_or(trimmed);
    inner.split('|').map(|cell| cell.trim().to_string()).collect()
}

fn parse_surface_row(row: &str) -> Result<Option<ParsedSurface>, CatalogError> {
    let cells = split_table_row(row);
    if cells.len() < 5 {
        return Err(CatalogError::new(format!(
            "{DOC_PATH}: malformed surface row (expected >=5 cells): {row}"
        )));
    }
    let surface_id = cells[0].clone();
    if !surface_id.starts_with('S') {
        return Ok(None);
    }
    let registry_cross_ref =
        extract_cell_text(&cells[4], &format!("{surface_id}.registry_cross_ref"))?
            .replace('\u{2014}', "")
            .trim()
            .to_string();
    let path = extract_cell_text(&cells[1], &format!("{surface_id}.path"))?;
    Ok(Some(ParsedSurface {
        surface_id,
        path,
        role: cells[2].clone(),
        claim_class: cells[3].clone(),
        registry_cross_ref,
    }))
}

fn parse_claim_row(row: &str, section: Option<&str>) -> Result<Option<ParsedClaim>, CatalogError> {
    let cells = split_table_row(row);
    if cells.len() < 4 {
        return Err(CatalogError::new(format!(
            "{DOC_PATH}: malformed claim row (expected >=4 cells): {row}"
        )));
    }
    let claim_id = cells[0].clone();
    if !claim_id.starts_with('C') {
        return Ok(None);
    }
    let surface_id = section
        .ok_or_else(|| {
            CatalogError::new(format!(
                "{DOC_PATH}: claim row {claim_id} appeared before any `### Sxx` heading"
            ))
        })?
        .to_string();
    let notes_cell = cells.get(4).cloned().unwrap_or_default();
    let summary = extract_cell_text(&cells[2], &format!("{claim_id}.summary"))?;
    let drift_status = extract_cell_text(&cells[3], &format!("{claim_id}.drift_status"))?;
    let notes = extract_cell_text(&notes_cell, &format!("{claim_id}.notes"))?;
    Ok(Some(ParsedClaim {
        claim_id,
        surface_id,
        location: cells[1].clone(),
        summary,
        drift_status,
        notes,
        raw_row: row.to_string(),
    }))
}

// ---------------------------------------------------------------------------
// D1: release-receipt manifest parsing and derivation
// ---------------------------------------------------------------------------

/// Parse and shape-check the mirrored release-asset manifest.
pub fn parse_receipt_manifest(bytes: &[u8]) -> Result<ParsedReceiptManifest, CatalogError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| {
        CatalogError::new(format!("{RECEIPT_MANIFEST_PATH}: invalid JSON: {error}"))
    })?;
    let object = value.as_object().ok_or_else(|| {
        CatalogError::new(format!("{RECEIPT_MANIFEST_PATH}: root must be an object"))
    })?;
    let expected_keys = ["release", "source", "verified_date", "assets"];
    for key in object.keys() {
        if !expected_keys.contains(&key.as_str()) {
            return Err(CatalogError::new(format!(
                "{RECEIPT_MANIFEST_PATH}: unknown root key `{key}`"
            )));
        }
    }
    let release = value
        .get("release")
        .and_then(Value::as_str)
        .ok_or_else(|| CatalogError::new(format!("{RECEIPT_MANIFEST_PATH}: release: missing")))?
        .to_string();
    if !valid_release_shape(&release) {
        return Err(CatalogError::new(format!(
            "{RECEIPT_MANIFEST_PATH}: release `{release}` must match v<major>.<minor>.<patch>"
        )));
    }
    let source = value
        .get("source")
        .and_then(Value::as_str)
        .filter(|source| !source.is_empty())
        .ok_or_else(|| {
            CatalogError::new(format!("{RECEIPT_MANIFEST_PATH}: source: missing or empty"))
        })?
        .to_string();
    let verified_date = value
        .get("verified_date")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            CatalogError::new(format!("{RECEIPT_MANIFEST_PATH}: verified_date: missing"))
        })?
        .to_string();
    if !valid_date_shape(&verified_date) {
        return Err(CatalogError::new(format!(
            "{RECEIPT_MANIFEST_PATH}: verified_date `{verified_date}` must match YYYY-MM-DD"
        )));
    }
    let asset_objects = value.get("assets").and_then(Value::as_array).ok_or_else(|| {
        CatalogError::new(format!("{RECEIPT_MANIFEST_PATH}: assets: missing array"))
    })?;
    let mut assets: Vec<String> = Vec::new();
    for asset in asset_objects {
        let object = asset.as_object().ok_or_else(|| {
            CatalogError::new(format!("{RECEIPT_MANIFEST_PATH}: assets[]: expected object"))
        })?;
        if let Some(key) = object.keys().find(|key| key.as_str() != "name") {
            return Err(CatalogError::new(format!(
                "{RECEIPT_MANIFEST_PATH}: assets[]: unknown key `{key}`"
            )));
        }
        let name = asset
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                CatalogError::new(format!("{RECEIPT_MANIFEST_PATH}: assets[]: missing name"))
            })?
            .to_string();
        assets.push(name);
    }
    if assets.is_empty() {
        return Err(CatalogError::new(format!(
            "{RECEIPT_MANIFEST_PATH}: assets must be non-empty"
        )));
    }
    let mut sorted = assets.clone();
    sorted.sort();
    sorted.dedup();
    if sorted != assets {
        return Err(CatalogError::new(format!(
            "{RECEIPT_MANIFEST_PATH}: asset names must be unique and stored in sorted order"
        )));
    }
    Ok(ParsedReceiptManifest { release, source, verified_date, asset_names: assets })
}

fn valid_release_shape(release: &str) -> bool {
    let Some(version) = release.strip_prefix('v') else {
        return false;
    };
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.chars().all(|character| character.is_ascii_digit())
        })
}

fn valid_date_shape(date: &str) -> bool {
    let bytes = date.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

/// D1 derivation: the receipt field is a fact about the published asset list,
/// not about prose. `present` only when the release actually shipped a
/// Windows ARM64 archive.
fn derive_windows_arm64_receipt(manifest: &ParsedReceiptManifest) -> &'static str {
    if manifest.asset_names.iter().any(|name| name.contains("aarch64-pc-windows-msvc")) {
        "present"
    } else {
        "absent"
    }
}

// ---------------------------------------------------------------------------
// Dimension assembly
// ---------------------------------------------------------------------------

/// Conjunctive route dimensions sourced from the inventory's own family
/// handoff notes (#11549 dimensions (a)/(b)/(c)). Receipt fields are NOT
/// recorded here: they are derived from the mirrored asset manifest (D1).
fn dimension_facts(claim_id: &str) -> Value {
    match claim_id {
        "C210" => json!({
            "windows_arm64": {
                "user_prose": "x64_fallback_build_from_source",
                "tracked_source": "built",
                "finding_refs": ["FND-4", "FND-11"]
            },
            "product_units": {
                "build_from_source_units": ["perllsp"],
                "tracked_installer_ships_adapter": true,
                "finding_refs": ["FND-11"]
            }
        }),
        "C1204" => json!({
            "windows_arm64": {
                "user_prose": "x64_fallback_build_from_source",
                "tracked_source": "built",
                "finding_refs": ["FND-4", "FND-11"]
            }
        }),
        "C405" => json!({
            "windows_arm64": {
                "user_prose": "unspecified",
                "tracked_source": "built",
                "finding_refs": ["FND-4"]
            }
        }),
        "C501" => json!({
            "windows_arm64": {
                "user_prose": "supported",
                "tracked_source": "built",
                "finding_refs": ["FND-4"]
            }
        }),
        "C207" => json!({
            "sha256sums_enforcement": {"mode": "fail_closed_required"}
        }),
        "C1005" => json!({
            "sha256sums_enforcement": {
                "mode": "fail_open_conditional",
                "finding_refs": ["FND-7"]
            }
        }),
        "C302" | "C406" => json!({
            "sha256sums_enforcement": {"mode": "verify_present_no_mode"}
        }),
        "C208" => json!({
            "product_units": {
                "build_from_source_units": ["perllsp"],
                "archive_units_claimed": ["perllsp", "perl-dap"]
            }
        }),
        "C209" => json!({
            "product_units": {
                "build_from_source_units": [],
                "archive_units_claimed": ["perllsp", "perl-dap"]
            }
        }),
        _ => Value::Null,
    }
}

/// Insert the manifest-derived receipt value into every windows_arm64
/// dimension (D1: receipt fields bind to the live asset manifest).
fn bind_receipt_to_dimensions(dimensions: &mut Map<String, Value>, receipt_value: &str) {
    if let Some(windows_arm64) = dimensions.get_mut("windows_arm64").and_then(Value::as_object_mut)
    {
        windows_arm64.insert("published_receipt_v0_17_0".to_string(), json!(receipt_value));
    }
}

/// Curated annotations sourced from the inventory's own Family handoff notes;
/// no preference, fragment choice, or ranking is invented here.
fn restatement_group(claim_id: &str) -> Option<&'static str> {
    match claim_id {
        "C103" | "C204" | "C1004" | "C1206" => Some("bootstrap_identity"),
        "C106" | "C216" | "C1008" | "C801" => Some("verification_probes"),
        _ => None,
    }
}

/// Caveat omissions recorded by the inventory itself. The crates.io name
/// collision is deliberately NOT here: it is a first-class anti-claim
/// identity (D2), not an omission caveat.
fn omitted_caveats(claim_id: &str) -> &'static [&'static str] {
    match claim_id {
        "C1304" | "C1305" => &["homebrew_tap_version_unproven"],
        _ => &[],
    }
}

// ---------------------------------------------------------------------------
// Catalog assembly
// ---------------------------------------------------------------------------

fn finding_sort_key(finding_id: &str) -> u32 {
    finding_id.trim_start_matches("FND-").parse().unwrap_or(u32::MAX)
}

/// Assemble the canonical catalog value from the parsed inventory.
pub fn build_catalog_value(
    inventory: &ParsedInventory,
    receipt_manifest: &ParsedReceiptManifest,
) -> Value {
    let receipt_value = derive_windows_arm64_receipt(receipt_manifest);
    let relations = derive_relations(&inventory.claims, &inventory.findings);
    let anti_claim_ids = derive_anti_claim_ids_from_claims(&inventory.claims);

    let claims: Vec<Value> = inventory
        .claims
        .iter()
        .map(|claim| {
            // D4: claim-level finding refs are the inverted join, not a scan.
            let mut finding_refs: Vec<String> = relations
                .iter()
                .filter(|(_, related)| related.contains(&claim.claim_id))
                .map(|(finding_id, _)| finding_id.clone())
                .collect();
            let mut dimension_refs: Vec<String> = Vec::new();
            if let Some(object) = dimension_facts(&claim.claim_id).as_object() {
                for dimension in object.values() {
                    if let Some(list) = dimension.get("finding_refs").and_then(Value::as_array) {
                        for value in list {
                            if let Some(reference) = value.as_str() {
                                dimension_refs.push(reference.to_string());
                            }
                        }
                    }
                }
            }
            for reference in dimension_refs {
                finding_refs.push(reference);
            }
            finding_refs.sort_by_key(|id| finding_sort_key(id));
            finding_refs.dedup();

            let mut dimensions = Map::new();
            if let Some(object) = dimension_facts(&claim.claim_id).as_object() {
                for (key, value) in object {
                    dimensions.insert(key.clone(), value.clone());
                }
            }
            bind_receipt_to_dimensions(&mut dimensions, receipt_value);
            let has_dimensions = !dimensions.is_empty();

            let mut claim_value = json!({
                "claim_id": claim.claim_id,
                "surface_id": claim.surface_id,
                "location": claim.location,
                "summary": claim.summary,
                "drift_status": claim.drift_status,
                "notes": claim.notes,
                "finding_refs": finding_refs,
                "restatement_group": restatement_group(&claim.claim_id),
                "omitted_caveats": omitted_caveats(&claim.claim_id),
                "identity_anti_claims": if anti_claim_ids.contains(&claim.claim_id) {
                    vec![identity_anti_claim_record()]
                } else {
                    Vec::new()
                },
            });
            if has_dimensions {
                claim_value["dimensions"] = Value::Object(dimensions);
            }
            claim_value
        })
        .collect();

    let findings: Vec<Value> = inventory
        .findings
        .iter()
        .map(|finding| {
            let related_claims = relations.get(&finding.finding_id).cloned().unwrap_or_default();
            let owner_route = FINDING_OWNERS
                .iter()
                .find(|(id, _)| *id == finding.finding_id)
                .map(|(_, route)| *route)
                .unwrap_or("none_recorded");
            json!({
                "finding_id": finding.finding_id,
                "title": finding.title,
                "cited_files": finding.cited_files,
                "related_claims": related_claims,
                "owner_route": owner_route,
            })
        })
        .collect();

    let assets: Vec<Value> =
        receipt_manifest.asset_names.iter().map(|name| json!({"name": name})).collect();

    json!({
        "schema_version": SCHEMA_VERSION,
        "status": "generated",
        "generator": GENERATOR_COMMAND,
        "issue": 11548,
        "source_inventory": {
            "path": DOC_PATH,
            "audited_commit": inventory.audited_commit,
            "audited_date": inventory.audited_date,
            "release_anchor": inventory.release_anchor,
            "track": "public-beta",
        },
        "input_digests": {},
        "release_receipts": [{
            "release": receipt_manifest.release,
            "source": receipt_manifest.source,
            "verified_date": receipt_manifest.verified_date,
            "assets": assets,
        }],
        "surfaces": inventory.surfaces.iter().map(|surface| json!({
            "surface_id": surface.surface_id,
            "path": surface.path,
            "role": surface.role,
            "claim_class": surface.claim_class,
            "registry_cross_ref": surface.registry_cross_ref,
        })).collect::<Vec<_>>(),
        "claims": claims,
        "findings": findings,
    })
}

/// Attach input digests; separated so tests can diff structure before binding.
pub fn with_input_digests(
    catalog: &Value,
    doc_sha: &str,
    schema_sha: &str,
    receipt_sha: &str,
) -> Value {
    let mut updated = catalog.clone();
    updated["input_digests"] = json!({
        "inventory_document": format!("sha256:{doc_sha}"),
        "schema": format!("sha256:{schema_sha}"),
        "release_receipt_manifest": format!("sha256:{receipt_sha}"),
    });
    updated
}

/// Generate the exact artifact bytes for the repository state under `root`.
pub fn generate_artifact_bytes(root: &Path) -> Result<Vec<u8>, CatalogError> {
    let doc_bytes = read_repo_bytes(root, DOC_PATH)?;
    let schema_bytes = read_repo_bytes(root, SCHEMA_PATH)?;
    let manifest_bytes = read_repo_bytes(root, RECEIPT_MANIFEST_PATH)?;
    let doc = std::str::from_utf8(&doc_bytes)
        .map_err(|error| CatalogError::new(format!("{DOC_PATH}: not UTF-8: {error}")))?;
    let inventory = parse_inventory(doc)?;
    validate_denominator(&inventory)?;

    let manifest = parse_receipt_manifest(&manifest_bytes)?;
    validate_receipt_manifest_anchor(&inventory, &manifest)?;
    let catalog = build_catalog_value(&inventory, &manifest);
    let catalog = with_input_digests(
        &catalog,
        &sha256_hex(&doc_bytes),
        &sha256_hex(&schema_bytes),
        &sha256_hex(&manifest_bytes),
    );
    canonical_bytes(&catalog)
}

// ---------------------------------------------------------------------------
// Validation (Rust-side gate; the Python oracle is the row-level authority)
// ---------------------------------------------------------------------------

fn validate_denominator(inventory: &ParsedInventory) -> Result<(), CatalogError> {
    let surface_ids: Vec<&str> =
        inventory.surfaces.iter().map(|surface| surface.surface_id.as_str()).collect();
    for expected in EXPECTED_SURFACES {
        if !surface_ids.contains(&expected) {
            return Err(CatalogError::new(format!(
                "{DOC_PATH}: missing denominator surface {expected}"
            )));
        }
    }
    if surface_ids.len() != EXPECTED_SURFACES.len() {
        return Err(CatalogError::new(format!(
            "{DOC_PATH}: unexpected extra surfaces: found {} surface rows",
            surface_ids.len()
        )));
    }

    let claim_ids: Vec<&str> =
        inventory.claims.iter().map(|claim| claim.claim_id.as_str()).collect();
    for expected in EXPECTED_CLAIM_IDS {
        if !claim_ids.contains(&expected) {
            return Err(CatalogError::new(format!(
                "{DOC_PATH}: missing denominator claim row {expected}"
            )));
        }
    }
    if claim_ids.len() != EXPECTED_CLAIM_IDS.len() {
        let extras: Vec<&str> =
            claim_ids.iter().copied().filter(|id| !EXPECTED_CLAIM_IDS.contains(id)).collect();
        return Err(CatalogError::new(format!(
            "{DOC_PATH}: {} claim row(s) outside the recorded denominator \
             (sanctioned regen required, never a silent pass): {extras:?}",
            extras.len()
        )));
    }

    for claim in &inventory.claims {
        if !DRIFT_STATUSES.contains(&claim.drift_status.as_str()) {
            return Err(CatalogError::new(format!(
                "{DOC_PATH}: {} uses unknown drift status `{}` (vocabulary moved? regenerate)",
                claim.claim_id, claim.drift_status
            )));
        }
        if !EXPECTED_SURFACES.contains(&claim.surface_id.as_str()) {
            return Err(CatalogError::new(format!(
                "{DOC_PATH}: {} references unknown surface `{}`",
                claim.claim_id, claim.surface_id
            )));
        }
    }

    // D4 regression: the join must relate FND-1 to C401 (cited `action.yml:3`).
    let relations = derive_relations(&inventory.claims, &inventory.findings);
    let regression_related = relations
        .get(REGRESSION_FINDING)
        .ok_or_else(|| CatalogError::new(format!("{DOC_PATH}: missing {REGRESSION_FINDING}")))?;
    if !regression_related.iter().any(|id| id == REGRESSION_CLAIM) {
        return Err(CatalogError::new(format!(
            "{DOC_PATH}: cited-file join lost the {REGRESSION_FINDING} \u{2194} {REGRESSION_CLAIM} \
             relation (D4 regression)"
        )));
    }
    Ok(())
}

/// Validate artifact bytes: structural closure checks plus deterministic
/// canonical-byte identity.
pub fn validate_artifact_bytes(bytes: &[u8]) -> Result<CatalogStats, CatalogError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| CatalogError::new(format!("catalog: not UTF-8: {error}")))?;
    let value: Value = serde_json::from_str(text)
        .map_err(|error| CatalogError::new(format!("catalog: invalid JSON: {error}")))?;
    let stats = validate_catalog_value(&value)?;
    let canonical = canonical_bytes(&value)?;
    if bytes != canonical.as_slice() {
        return Err(CatalogError::new(
            "catalog: bytes are not the deterministic canonical form \
             (canonical JSON = sorted keys, two-space indent, single trailing LF)",
        ));
    }
    Ok(stats)
}

fn string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

fn validate_closed_keys(
    object: &Map<String, Value>,
    allowed: &[&str],
    name: &str,
) -> Result<(), CatalogError> {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(CatalogError::new(format!(
                "catalog.{name}: schema-forbidden key `{key}` (D6 closed-object violation)"
            )));
        }
    }
    Ok(())
}

fn validate_identity_anti_claim(value: &Value, name: &str) -> Result<(), CatalogError> {
    let object = value.as_object().ok_or_else(|| {
        CatalogError::new(format!("catalog.{name}: anti-claim must be an object"))
    })?;
    validate_closed_keys(object, &["kind", "foreign_name", "owned_name", "disposition"], name)?;
    for (key, expected) in [
        ("kind", ANTI_CLAIM_KIND),
        ("foreign_name", ANTI_CLAIM_FOREIGN),
        ("owned_name", ANTI_CLAIM_OWNED),
        ("disposition", ANTI_CLAIM_DISPOSITION),
    ] {
        if object.get(key).and_then(Value::as_str) != Some(expected) {
            return Err(CatalogError::new(format!(
                "catalog.{name}.{key}: must be `{expected}` (closed anti-claim shape, D2)"
            )));
        }
    }
    Ok(())
}

fn validate_dimension_family(
    family: &Map<String, Value>,
    allowed: &[&str],
    name: &str,
) -> Result<(), CatalogError> {
    validate_closed_keys(family, allowed, name)?;
    if let Some(refs) = family.get("finding_refs").and_then(Value::as_array) {
        for reference in refs {
            let id = reference.as_str().ok_or_else(|| {
                CatalogError::new(format!("catalog.{name}.finding_refs: non-string entry"))
            })?;
            if !FINDING_IDS.contains(&id) {
                return Err(CatalogError::new(format!(
                    "catalog.{name}.finding_refs: unknown finding `{id}`"
                )));
            }
        }
    }
    Ok(())
}

fn validate_catalog_value(value: &Value) -> Result<CatalogStats, CatalogError> {
    let object = value.as_object().ok_or_else(|| CatalogError::new("catalog: expected object"))?;
    validate_closed_keys(
        object,
        &[
            "schema_version",
            "status",
            "generator",
            "issue",
            "source_inventory",
            "input_digests",
            "release_receipts",
            "surfaces",
            "claims",
            "findings",
        ],
        "",
    )?;

    let version = object
        .get("schema_version")
        .and_then(Value::as_str)
        .ok_or_else(|| CatalogError::new("catalog.schema_version: missing"))?;
    if version != SCHEMA_VERSION {
        return Err(CatalogError::new(format!(
            "catalog.schema_version: expected `{SCHEMA_VERSION}`, found `{version}` \
             (a v1 document must go to the v1 validator, which this slice leaves untouched)"
        )));
    }

    let surfaces_array = value
        .get("surfaces")
        .and_then(Value::as_array)
        .ok_or_else(|| CatalogError::new("catalog.surfaces: missing array"))?;
    for surface in surfaces_array {
        let surface_object = surface
            .as_object()
            .ok_or_else(|| CatalogError::new("catalog.surfaces[]: expected object"))?;
        validate_closed_keys(
            surface_object,
            &["surface_id", "path", "role", "claim_class", "registry_cross_ref"],
            "surfaces[]",
        )?;
    }

    let input_digests = value
        .get("input_digests")
        .and_then(Value::as_object)
        .ok_or_else(|| CatalogError::new("catalog.input_digests: missing object"))?;
    validate_closed_keys(
        input_digests,
        &["inventory_document", "schema", "release_receipt_manifest"],
        "input_digests",
    )?;

    let receipts = value
        .get("release_receipts")
        .and_then(Value::as_array)
        .ok_or_else(|| CatalogError::new("catalog.release_receipts: missing array"))?;
    if receipts.is_empty() {
        return Err(CatalogError::new("catalog.release_receipts: must be non-empty"));
    }
    for receipt in receipts {
        let receipt_object = receipt
            .as_object()
            .ok_or_else(|| CatalogError::new("catalog.release_receipts[]: expected object"))?;
        validate_closed_keys(
            receipt_object,
            &["release", "source", "verified_date", "assets"],
            "release_receipts[]",
        )?;
        let assets = receipt
            .get("assets")
            .and_then(Value::as_array)
            .ok_or_else(|| CatalogError::new("catalog.release_receipts[].assets: missing array"))?;
        for asset in assets {
            let asset_object = asset.as_object().ok_or_else(|| {
                CatalogError::new("catalog.release_receipts[].assets[]: expected object")
            })?;
            validate_closed_keys(asset_object, &["name"], "release_receipts[].assets[]")?;
        }
    }

    let claims_array = value
        .get("claims")
        .and_then(Value::as_array)
        .ok_or_else(|| CatalogError::new("catalog.claims: missing array"))?;
    let mut seen: std::collections::BTreeSet<String> = Default::default();
    let mut dimensioned_rows = 0usize;
    let mut anti_claimed_rows = 0usize;
    for claim in claims_array {
        let claim_object = claim
            .as_object()
            .ok_or_else(|| CatalogError::new("catalog.claims[]: expected object"))?;
        validate_closed_keys(
            claim_object,
            &[
                "claim_id",
                "surface_id",
                "location",
                "summary",
                "drift_status",
                "notes",
                "finding_refs",
                "restatement_group",
                "omitted_caveats",
                "identity_anti_claims",
                "dimensions",
            ],
            "claims[]",
        )?;
        let id = claim
            .get("claim_id")
            .and_then(Value::as_str)
            .ok_or_else(|| CatalogError::new("catalog.claims[].claim_id: missing"))?;
        if !seen.insert(id.to_string()) {
            return Err(CatalogError::new(format!(
                "catalog.claims: duplicate route authority `{id}`"
            )));
        }
        let drift_status = claim.get("drift_status").and_then(Value::as_str).ok_or_else(|| {
            CatalogError::new(format!("catalog.claims[{id}].drift_status: missing"))
        })?;
        if !DRIFT_STATUSES.contains(&drift_status) {
            return Err(CatalogError::new(format!(
                "catalog.claims[{id}]: unknown drift status `{drift_status}`"
            )));
        }
        for field in ["summary", "notes"] {
            let text = claim.get(field).and_then(Value::as_str).unwrap_or("");
            if !backticks_balanced(text) {
                return Err(CatalogError::new(format!(
                    "catalog.claims[{id}].{field}: unbalanced code-span delimiters (D3)"
                )));
            }
        }
        let anti_claims =
            claim.get("identity_anti_claims").and_then(Value::as_array).ok_or_else(|| {
                CatalogError::new(format!(
                    "catalog.claims[{id}].identity_anti_claims: missing array"
                ))
            })?;
        for anti_claim in anti_claims {
            validate_identity_anti_claim(
                anti_claim,
                &format!("claims[{id}].identity_anti_claims[]"),
            )?;
        }
        if !anti_claims.is_empty() {
            anti_claimed_rows += 1;
        }
        if let Some(dimensions) = claim.get("dimensions").and_then(Value::as_object) {
            if !dimensions.is_empty() {
                dimensioned_rows += 1;
            }
            for (family, value) in dimensions {
                let family_object = value.as_object().ok_or_else(|| {
                    CatalogError::new(format!(
                        "catalog.claims[{id}].dimensions.{family}: expected object"
                    ))
                })?;
                match family.as_str() {
                    "windows_arm64" => {
                        validate_dimension_family(
                            family_object,
                            &[
                                "user_prose",
                                "tracked_source",
                                "published_receipt_v0_17_0",
                                "finding_refs",
                            ],
                            &format!("claims[{id}].dimensions.windows_arm64"),
                        )?;
                        if let Some(receipt) =
                            family_object.get("published_receipt_v0_17_0").and_then(Value::as_str)
                            && !matches!(receipt, "absent" | "present" | "unverified")
                        {
                            return Err(CatalogError::new(format!(
                                "catalog.claims[{id}]: invalid published_receipt_v0_17_0 `{receipt}`"
                            )));
                        }
                    }
                    "sha256sums_enforcement" => {
                        validate_dimension_family(
                            family_object,
                            &["mode", "finding_refs"],
                            &format!("claims[{id}].dimensions.sha256sums_enforcement"),
                        )?;
                    }
                    "product_units" => {
                        validate_dimension_family(
                            family_object,
                            &[
                                "build_from_source_units",
                                "archive_units_claimed",
                                "tracked_installer_ships_adapter",
                                "finding_refs",
                            ],
                            &format!("claims[{id}].dimensions.product_units"),
                        )?;
                    }
                    other => {
                        return Err(CatalogError::new(format!(
                            "catalog.claims[{id}].dimensions: schema-forbidden dimension family \
                             `{other}` (D6 closed-object violation)"
                        )));
                    }
                }
            }
        }
    }

    let findings_array = value
        .get("findings")
        .and_then(Value::as_array)
        .ok_or_else(|| CatalogError::new("catalog.findings: missing array"))?;
    for finding in findings_array {
        let finding_object = finding
            .as_object()
            .ok_or_else(|| CatalogError::new("catalog.findings[]: expected object"))?;
        validate_closed_keys(
            finding_object,
            &["finding_id", "title", "cited_files", "related_claims", "owner_route"],
            "findings[]",
        )?;
        let related = string_array(finding, "related_claims");
        for claim_id in related {
            if !EXPECTED_CLAIM_IDS.contains(&claim_id.as_str()) {
                return Err(CatalogError::new(format!(
                    "catalog.findings[].related_claims: unknown claim `{claim_id}`"
                )));
            }
        }
    }

    // Hard denominator guard: works even when only the artifact (not the
    // source document) is tampered with, so a dropped row can never read as a
    // clean pass.
    if claims_array.len() != EXPECTED_CLAIM_IDS.len() {
        return Err(CatalogError::new(format!(
            "catalog.claims: {} row(s) but the denominator holds {}",
            claims_array.len(),
            EXPECTED_CLAIM_IDS.len()
        )));
    }
    if surfaces_array.len() != EXPECTED_SURFACES.len() {
        return Err(CatalogError::new(format!(
            "catalog.surfaces: {} row(s) but the denominator holds {}",
            surfaces_array.len(),
            EXPECTED_SURFACES.len()
        )));
    }
    if findings_array.len() != FINDING_IDS.len() {
        return Err(CatalogError::new(format!(
            "catalog.findings: {} row(s) but the denominator holds {}",
            findings_array.len(),
            FINDING_IDS.len()
        )));
    }

    // D4 regression inside the committed artifact.
    let regression = findings_array.iter().find(|finding| {
        finding.get("finding_id").and_then(Value::as_str) == Some(REGRESSION_FINDING)
    });
    let regression_related = regression
        .and_then(|finding| finding.get("related_claims").and_then(Value::as_array).cloned())
        .unwrap_or_default();
    if !regression_related.iter().any(|id| id.as_str() == Some(REGRESSION_CLAIM)) {
        return Err(CatalogError::new(format!(
            "catalog.findings[{REGRESSION_FINDING}]: relation to {REGRESSION_CLAIM} missing \
             (D4 regression)"
        )));
    }

    let derived_relations = findings_array
        .iter()
        .map(|finding| {
            finding.get("related_claims").and_then(Value::as_array).map(Vec::len).unwrap_or(0)
        })
        .sum();

    Ok(CatalogStats {
        surfaces: surfaces_array.len(),
        claims: claims_array.len(),
        findings: findings_array.len(),
        dimensioned_rows,
        anti_claimed_rows,
        derived_relations,
    })
}

/// D1 checks: the mirrored manifest is bound to the inventory release anchor,
/// and every committed receipt field equals the value derived from that
/// manifest — receipt authority, not the inventory digest.
fn validate_receipt_manifest_anchor(
    inventory: &ParsedInventory,
    manifest: &ParsedReceiptManifest,
) -> Result<(), CatalogError> {
    if manifest.release != inventory.release_anchor {
        return Err(CatalogError::new(format!(
            "{RECEIPT_MANIFEST_PATH}: release `{}` does not match the {DOC_PATH} drift anchor \
             `{}` (D1 receipt binding)",
            manifest.release, inventory.release_anchor
        )));
    }
    Ok(())
}

fn validate_receipt_binding(
    catalog: &Value,
    manifest: &ParsedReceiptManifest,
) -> Result<(), CatalogError> {
    let derived = derive_windows_arm64_receipt(manifest);
    let claims = catalog
        .get("claims")
        .and_then(Value::as_array)
        .ok_or_else(|| CatalogError::new("catalog.claims: missing array"))?;
    for claim in claims {
        let Some(id) = claim.get("claim_id").and_then(Value::as_str) else {
            continue;
        };
        let Some(dimensions) = claim.get("dimensions").and_then(Value::as_object) else {
            continue;
        };
        let Some(windows_arm64) = dimensions.get("windows_arm64").and_then(Value::as_object) else {
            continue;
        };
        let recorded = windows_arm64
            .get("published_receipt_v0_17_0")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                CatalogError::new(format!(
                    "catalog.claims[{id}]: windows_arm64 dimension missing \
                     published_receipt_v0_17_0 (D1 receipt binding)"
                ))
            })?;
        if recorded != derived {
            return Err(CatalogError::new(format!(
                "catalog.claims[{id}]: published_receipt_v0_17_0 `{recorded}` contradicts the \
                 release-asset manifest (`{derived}` for {}; D1 receipt binding)",
                manifest.release
            )));
        }
    }
    Ok(())
}

/// D6 schema-side walk: every object-typed node (any node with `properties` or
/// `type: object`) must close its key set.
pub fn validate_schema_closure(schema: &Value) -> Result<usize, CatalogError> {
    let mut closed_objects = 0usize;
    let mut stack = vec![schema];
    while let Some(node) = stack.pop() {
        let Some(object) = node.as_object() else {
            continue;
        };
        let is_object_schema = object.get("type").and_then(Value::as_str) == Some("object")
            || object.contains_key("properties");
        if is_object_schema {
            if object.get("additionalProperties") != Some(&Value::Bool(false)) {
                return Err(CatalogError::new(
                    "schema: object node without `additionalProperties: false` (D6 closure violation)",
                ));
            }
            closed_objects += 1;
        }
        for child in object.values() {
            stack.push(child);
        }
        // Array items inside JSON arrays (e.g. enum lists) carry no schemas.
        if let Some(items) = object.get("items") {
            stack.push(items);
        }
        if let Some(defs) = object.get("$defs").and_then(Value::as_object) {
            for def in defs.values() {
                stack.push(def);
            }
        }
        if let Some(properties) = object.get("properties").and_then(Value::as_object) {
            for property in properties.values() {
                stack.push(property);
            }
        }
    }
    Ok(closed_objects)
}

/// Full in-repo gate: regenerate from the live sources and compare with the
/// committed artifact byte-for-byte, then apply the structural, receipt, and
/// closure checks.
pub fn validate_repository_catalog(root: &Path) -> Result<CatalogStats, CatalogError> {
    let expected = generate_artifact_bytes(root)?;
    let actual = read_repo_bytes(root, ARTIFACT_PATH)?;

    if actual != expected {
        let first_difference = actual
            .iter()
            .zip(expected.iter())
            .position(|(actual_byte, expected_byte)| actual_byte != expected_byte)
            .unwrap_or(actual.len().min(expected.len()));
        return Err(CatalogError::new(format!(
            "{}: stale or tampered relative to {DOC_PATH}; regenerate via `{GENERATOR_COMMAND}` \
             (first differing byte at offset {first_difference})",
            ARTIFACT_PATH
        )));
    }

    let stats = validate_artifact_bytes(&actual)?;

    let schema_text = fs::read_to_string(root.join(SCHEMA_PATH))
        .map_err(|error| CatalogError::new(format!("{SCHEMA_PATH}: cannot read: {error}")))?;
    let schema: Value = serde_json::from_str(&schema_text)
        .map_err(|error| CatalogError::new(format!("{SCHEMA_PATH}: invalid JSON: {error}")))?;
    validate_schema_closure(&schema)?;

    let manifest_bytes = read_repo_bytes(root, RECEIPT_MANIFEST_PATH)?;
    let manifest = parse_receipt_manifest(&manifest_bytes)?;
    let catalog: Value = serde_json::from_slice(&actual)
        .map_err(|error| CatalogError::new(format!("{}: invalid JSON: {error}", ARTIFACT_PATH)))?;
    validate_receipt_binding(&catalog, &manifest)?;

    // D2: the committed anti-claim set must equal the derivation from the
    // raw inventory text.
    let doc_bytes = read_repo_bytes(root, DOC_PATH)?;
    let doc = std::str::from_utf8(&doc_bytes)
        .map_err(|error| CatalogError::new(format!("{DOC_PATH}: not UTF-8: {error}")))?;
    let derived_anti_claim_ids = derive_anti_claim_ids(doc)?;
    let committed_anti_claim_ids: Vec<String> = catalog
        .get("claims")
        .and_then(Value::as_array)
        .map(|claims| {
            claims
                .iter()
                .filter(|claim| {
                    claim
                        .get("identity_anti_claims")
                        .and_then(Value::as_array)
                        .is_some_and(|items| !items.is_empty())
                })
                .filter_map(|claim| claim.get("claim_id").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if committed_anti_claim_ids != derived_anti_claim_ids {
        return Err(CatalogError::new(format!(
            "catalog: anti-claim identity set {committed_anti_claim_ids:?} does not match the \
             inventory-derived set {derived_anti_claim_ids:?} (D2)"
        )));
    }

    Ok(stats)
}

/// List stable claim IDs in catalog order.
pub fn list_claim_ids(manifest: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(claims) = manifest.get("claims").and_then(Value::as_array) {
        for claim in claims {
            if let Some(id) = claim.get("claim_id").and_then(Value::as_str) {
                ids.push(id.to_string());
            }
        }
    }
    ids
}

/// Render one claim row as pretty JSON for `explain`.
pub fn explain_claim(manifest: &Value, claim_id: &str) -> Option<String> {
    let claims = manifest.get("claims")?.as_array()?;
    for claim in claims {
        if claim.get("claim_id").and_then(Value::as_str) == Some(claim_id) {
            return serde_json::to_string_pretty(claim).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|parent| parent.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    }

    fn committed_artifact() -> Vec<u8> {
        read_repo_bytes(&repo_root(), ARTIFACT_PATH).expect("test: committed artifact readable")
    }

    fn committed_doc() -> String {
        let bytes = read_repo_bytes(&repo_root(), DOC_PATH).expect("test: inventory readable");
        String::from_utf8(bytes).expect("test: inventory is UTF-8")
    }

    fn committed_catalog() -> Value {
        serde_json::from_slice(&committed_artifact()).expect("test: artifact parses")
    }

    fn claim_row(catalog: &Value, claim_id: &str) -> Value {
        catalog
            .get("claims")
            .and_then(Value::as_array)
            .and_then(|claims| {
                claims
                    .iter()
                    .find(|claim| claim.get("claim_id").and_then(Value::as_str) == Some(claim_id))
                    .cloned()
            })
            .unwrap_or_else(|| panic!("test: claim row {claim_id} present"))
    }

    fn finding_row(catalog: &Value, finding_id: &str) -> Value {
        catalog
            .get("findings")
            .and_then(Value::as_array)
            .and_then(|findings| {
                findings
                    .iter()
                    .find(|finding| {
                        finding.get("finding_id").and_then(Value::as_str) == Some(finding_id)
                    })
                    .cloned()
            })
            .unwrap_or_else(|| panic!("test: finding row {finding_id} present"))
    }

    /// Falsifier: a second generation changes no bytes.
    #[test]
    fn generation_is_byte_stable_across_runs() {
        let root = repo_root();
        let first = generate_artifact_bytes(&root).expect("first generation");
        let second = generate_artifact_bytes(&root).expect("second generation");
        assert_eq!(first, second);
    }

    /// D5 (Rust-side half): the committed artifact is the canonical
    /// regeneration of the live sources.
    #[test]
    fn committed_artifact_matches_regeneration() {
        let root = repo_root();
        let expected = generate_artifact_bytes(&root).expect("generation");
        assert_eq!(committed_artifact(), expected);
    }

    #[test]
    fn repository_validation_passes() {
        let stats = validate_repository_catalog(&repo_root()).expect("repository validation");
        assert_eq!(stats.surfaces, 13);
        assert_eq!(stats.claims, 70);
        assert_eq!(stats.findings, 12);
        assert!(stats.dimensioned_rows >= 8);
        assert!(stats.anti_claimed_rows >= 4);
    }

    /// D1: the receipt value for every windows_arm64 row is `absent` because
    /// the mirrored v0.17.0 manifest ships no Windows ARM64 asset; a manifest
    /// that does ship one must flip the derivation.
    #[test]
    fn d1_receipt_values_derive_from_asset_manifest() {
        let catalog = committed_catalog();
        for claim_id in ["C210", "C1204", "C405", "C501"] {
            let row = claim_row(&catalog, claim_id);
            let receipt = row
                .get("dimensions")
                .and_then(|dimensions| dimensions.get("windows_arm64"))
                .and_then(|windows| windows.get("published_receipt_v0_17_0"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("test: {claim_id} carries a receipt field"));
            assert_eq!(receipt, "absent", "D1: {claim_id} receipt must be absent for v0.17.0");
        }

        let doc = committed_doc();
        let inventory = parse_inventory(&doc).expect("inventory parses");
        let without_arm64 =
            parse_receipt_manifest(br#"{"release":"v0.17.0","source":"https://example.invalid/v0.17.0","verified_date":"2026-08-28","assets":[{"name":"SHA256SUMS"},{"name":"perllsp-0.17.0-x86_64-pc-windows-msvc.zip"}]}"#)
                .expect("synthetic manifest parses");
        let with_arm64 =
            parse_receipt_manifest(br#"{"release":"v0.18.0","source":"https://example.invalid/v0.18.0","verified_date":"2026-08-28","assets":[{"name":"SHA256SUMS"},{"name":"perllsp-0.18.0-aarch64-pc-windows-msvc.zip"}]}"#)
                .expect("synthetic manifest parses");
        assert_eq!(derive_windows_arm64_receipt(&without_arm64), "absent");
        assert_eq!(derive_windows_arm64_receipt(&with_arm64), "present");

        let without = build_catalog_value(&inventory, &without_arm64);
        let with = build_catalog_value(&inventory, &with_arm64);
        let c210 = claim_row(&without, "C210");
        assert_eq!(
            c210.pointer("/dimensions/windows_arm64/published_receipt_v0_17_0")
                .and_then(Value::as_str),
            Some("absent")
        );
        let c210_with = claim_row(&with, "C210");
        assert_eq!(
            c210_with
                .pointer("/dimensions/windows_arm64/published_receipt_v0_17_0")
                .and_then(Value::as_str),
            Some("present")
        );
    }

    /// D1: the manifest digest participates in the artifact binding, so
    /// changing the manifest changes the artifact.
    #[test]
    fn d1_manifest_digest_binds_into_artifact() {
        let catalog = committed_catalog();
        let digest = catalog
            .pointer("/input_digests/release_receipt_manifest")
            .and_then(Value::as_str)
            .expect("test: manifest digest present");
        let manifest_bytes =
            read_repo_bytes(&repo_root(), RECEIPT_MANIFEST_PATH).expect("manifest readable");
        assert_eq!(digest, format!("sha256:{}", sha256_hex(&manifest_bytes)));
    }

    #[test]
    fn d1_rust_manifest_release_mismatch_is_rejected() {
        let root = repo_root();
        let inventory = parse_inventory(&committed_doc()).expect("inventory parses");
        let manifest_bytes =
            read_repo_bytes(&root, RECEIPT_MANIFEST_PATH).expect("manifest readable");
        let manifest = parse_receipt_manifest(&manifest_bytes).expect("manifest parses");
        assert!(validate_receipt_manifest_anchor(&inventory, &manifest).is_ok());

        let mut mismatched = manifest;
        mismatched.release = "v9.9.9".to_string();
        let error = validate_receipt_manifest_anchor(&inventory, &mismatched)
            .expect_err("mismatched release must fail");
        let message = format!("{error}");
        assert!(message.contains("release `v9.9.9`"), "{message}");
        assert!(message.contains(&inventory.release_anchor), "{message}");
        assert!(message.contains(DOC_PATH), "{message}");

        let temp = tempfile::tempdir().expect("temporary root");
        for relative_path in [DOC_PATH, SCHEMA_PATH, RECEIPT_MANIFEST_PATH] {
            let destination = temp.path().join(relative_path);
            fs::create_dir_all(destination.parent().expect("input parent")).expect("input dirs");
            fs::write(&destination, read_repo_bytes(&root, relative_path).expect("input readable"))
                .expect("input copied");
        }
        assert!(generate_artifact_bytes(temp.path()).is_ok());
        let manifest_path = temp.path().join(RECEIPT_MANIFEST_PATH);
        let mut manifest_value: Value =
            serde_json::from_slice(&fs::read(&manifest_path).expect("manifest copied"))
                .expect("manifest JSON");
        manifest_value["release"] = json!("v9.9.9");
        fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest_value).expect("manifest serialized"),
        )
        .expect("manifest mutated");
        let error =
            generate_artifact_bytes(temp.path()).expect_err("mismatched manifest must fail");
        let message = format!("{error}");
        assert!(message.contains("release `v9.9.9`"), "{message}");
        assert!(message.contains(DOC_PATH), "{message}");
    }

    #[test]
    fn d3_surface_paths_are_code_span_trimmed() {
        let inventory = parse_inventory(&committed_doc()).expect("inventory parses");
        for surface in &inventory.surfaces {
            assert!(!surface.path.contains('`'), "backtick in {}", surface.surface_id);
        }
        for (surface_id, expected_path) in [
            ("S04", ".github/actions/setup-perl-lsp/action.yml"),
            ("S06", "docs/examples/github-actions/setup-perl-lsp-consumer.yml"),
            ("S11", "vscode-extension/package.json"),
        ] {
            let surface = inventory
                .surfaces
                .iter()
                .find(|surface| surface.surface_id == surface_id)
                .expect("surface present");
            assert_eq!(surface.path, expected_path);
        }
    }

    #[test]
    fn d3_unbalanced_surface_path_is_rejected() {
        let error = parse_surface_row("| S04 | `foo/bar.yml | role | claims | — |")
            .expect_err("unbalanced surface path must fail");
        assert!(format!("{error}").contains("unbalanced"));
    }

    /// D2: the four receipt-named rows carry the first-class anti-claim, and
    /// the committed set equals the derivation from raw inventory text.
    #[test]
    fn d2_anti_claim_set_is_derived_and_covers_receipt_rows() {
        let catalog = committed_catalog();
        let anti_claimed: Vec<String> = catalog
            .get("claims")
            .and_then(Value::as_array)
            .map(|claims| {
                claims
                    .iter()
                    .filter(|claim| {
                        claim
                            .get("identity_anti_claims")
                            .and_then(Value::as_array)
                            .is_some_and(|items| !items.is_empty())
                    })
                    .filter_map(|claim| claim.get("claim_id").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        for claim_id in ["C1302", "C1306", "C1307", "C1308"] {
            assert!(
                anti_claimed.iter().any(|id| id == claim_id),
                "D2: {claim_id} must carry the anti-claim identity"
            );
        }
        let derived = derive_anti_claim_ids(&committed_doc()).expect("derivation");
        assert_eq!(anti_claimed, derived);

        // The anti-claim is not an omission caveat anywhere.
        for claim in catalog.get("claims").and_then(Value::as_array).expect("claims") {
            let caveats = claim.get("omitted_caveats").and_then(Value::as_array).expect("caveats");
            assert!(
                caveats.iter().all(|caveat| caveat.as_str() != Some("crates_io_name_collision")),
                "D2: collision must never be an omission caveat"
            );
        }
    }

    /// D3: every derived summary/notes cell has balanced delimiters; a cell
    /// with a matched boundary pair is trimmed exactly one pair, and a cell
    /// whose boundary backticks do not match stays verbatim (opening
    /// delimiters of leading code spans are content, not boundary residue).
    #[test]
    fn d3_all_summary_cells_are_pair_trimmed_and_balanced() {
        let catalog = committed_catalog();
        let mut code_span_led = 0usize;
        for claim in catalog.get("claims").and_then(Value::as_array).expect("claims") {
            let id = claim.get("claim_id").and_then(Value::as_str).expect("claim id");
            for field in ["summary", "notes"] {
                let text = claim.get(field).and_then(Value::as_str).unwrap_or("");
                assert!(
                    backticks_balanced(text),
                    "D3: {id}.{field} has unbalanced delimiters: {text}"
                );
            }
        }
        // Whole-cell code spans trim their matched pair; multi-span cells
        // stay fully verbatim so interior markdown never re-pairs.
        let c1302 = claim_row(&catalog, "C1302");
        let verbatim = c1302.get("summary").and_then(Value::as_str).expect("summary");
        assert!(verbatim.starts_with("`cargo install perllsp --locked`"));
        assert!(verbatim.ends_with("`cargo install --path crates/perllsp --locked`"));
        assert_eq!(verbatim.matches('`').count(), 4);
        let c1306 = claim_row(&catalog, "C1306");
        let whole_span = c1306.get("summary").and_then(Value::as_str).expect("summary");
        assert_eq!(whole_span, "cargo install perllsp");
        // Unmatched boundary kept verbatim: the leading code span survives.
        let c1003 = claim_row(&catalog, "C1003");
        let leading = c1003.get("summary").and_then(Value::as_str).expect("summary");
        assert!(leading.starts_with("`code --install-extension"));
        let doc = committed_doc();
        for line in doc.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("| C") && trimmed.contains("| `") {
                code_span_led += 1;
            }
        }
        assert!(code_span_led > 0, "expected code-span-led rows in the inventory");
    }

    /// D3 negative: an unbalanced cell fails generation instead of producing
    /// a malformed row.
    #[test]
    fn d3_unbalanced_cell_is_rejected() {
        let doc = committed_doc();
        let doctored = doc.replace("| `cargo install perllsp` |", "| `cargo install perllsp |");
        if doctored == doc {
            // The exact probe cell moved upstream; fall back to a structural
            // mutation that always applies.
            let doctored = doc.replace("| `current` |", "| `current |");
            assert_ne!(doctored, doc, "test: probe row must exist");
            let error = parse_inventory(&doctored).expect_err("unbalanced cell must fail");
            assert!(format!("{error}").contains("unbalanced"));
            return;
        }
        let error = parse_inventory(&doctored).expect_err("unbalanced cell must fail");
        assert!(format!("{error}").contains("unbalanced"));
    }

    /// D4: FND-1 relates C401 through the cited-file join.
    #[test]
    fn d4_fnd1_relates_c401() {
        let catalog = committed_catalog();
        let related = finding_row(&catalog, "FND-1")
            .get("related_claims")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(
            related.iter().any(|id| id.as_str() == Some("C401")),
            "D4 regression: FND-1 must relate C401"
        );
        let cited = finding_row(&catalog, "FND-1")
            .get("cited_files")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(cited.iter().any(|file| file.as_str() == Some("action.yml")));
    }

    /// D4: committed relations are exactly the cited-file join (no token
    /// scan residue), and claim-level refs are the inverted join.
    #[test]
    fn d4_relations_equal_the_cited_file_join() {
        let doc = committed_doc();
        let inventory = parse_inventory(&doc).expect("inventory parses");
        let relations = derive_relations(&inventory.claims, &inventory.findings);
        let catalog = committed_catalog();
        for finding in catalog.get("findings").and_then(Value::as_array).expect("findings") {
            let id = finding.get("finding_id").and_then(Value::as_str).expect("finding id");
            let expected: Vec<String> = relations.get(id).cloned().unwrap_or_default();
            let actual: Vec<String> = finding
                .get("related_claims")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            assert_eq!(expected, actual, "D4: relation list for {id}");
        }
        for claim in catalog.get("claims").and_then(Value::as_array).expect("claims") {
            let id = claim.get("claim_id").and_then(Value::as_str).expect("claim id");
            // Mirror the generator: inverted join plus dimension fact refs.
            let mut expected: Vec<String> = relations
                .iter()
                .filter(|(_, related)| related.iter().any(|related_id| related_id == id))
                .map(|(finding_id, _)| finding_id.clone())
                .collect();
            if let Some(object) = dimension_facts(id).as_object() {
                for dimension in object.values() {
                    if let Some(list) = dimension.get("finding_refs").and_then(Value::as_array) {
                        for value in list {
                            if let Some(reference) = value.as_str() {
                                expected.push(reference.to_string());
                            }
                        }
                    }
                }
            }
            expected.sort_by_key(|finding_id| finding_sort_key(finding_id));
            expected.dedup();
            let actual: Vec<String> = claim
                .get("finding_refs")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            assert_eq!(expected, actual, "D4: inverted join for {id}");
        }
    }

    /// D6: the committed schema is fully closed.
    #[test]
    fn d6_schema_is_fully_closed() {
        let schema_bytes = read_repo_bytes(&repo_root(), SCHEMA_PATH).expect("schema readable");
        let schema: Value = serde_json::from_slice(&schema_bytes).expect("schema parses");
        let closed_objects = validate_schema_closure(&schema).expect("closure walk");
        assert!(closed_objects >= 12, "expected the full object graph to be walked");
    }

    /// D6 negative: a schema that opens any object node fails the walk.
    #[test]
    fn d6_open_schema_node_is_rejected() {
        let schema_bytes = read_repo_bytes(&repo_root(), SCHEMA_PATH).expect("schema readable");
        let mut schema: Value = serde_json::from_slice(&schema_bytes).expect("schema parses");
        schema
            .pointer_mut("/$defs/dimensions/properties/sha256sums_enforcement")
            .and_then(|node| node.as_object_mut())
            .expect("test: sha256sums_enforcement node")
            .remove("additionalProperties");
        let error = validate_schema_closure(&schema).expect_err("open node must fail");
        assert!(format!("{error}").contains("closure violation"));
    }

    /// D5 (Rust-side half): any tampered catalog byte fails validation.
    #[test]
    fn tampered_catalog_bytes_are_rejected() {
        let mut bytes = committed_artifact();
        let position = bytes
            .windows(9)
            .position(|window| window == b"\"summary\"")
            .expect("test: a summary field exists");
        bytes[position + 10] = bytes[position + 10].wrapping_add(1);
        let error = validate_artifact_bytes(&bytes).expect_err("tampered bytes must fail");
        let message = format!("{error}");
        assert!(
            message.contains("canonical") || message.contains("invalid"),
            "unexpected rejection reason: {message}"
        );
    }

    #[test]
    fn list_and_explain_smoke() {
        let catalog = committed_catalog();
        let ids = list_claim_ids(&catalog);
        assert_eq!(ids.len(), 70);
        assert_eq!(ids[0], "C101");
        assert!(explain_claim(&catalog, "C207").is_some());
        assert!(explain_claim(&catalog, "C9999").is_none());
    }

    // -----------------------------------------------------------------------
    // RIPR seam reveal: every error path in this module is executed by a
    // focused test that asserts a discriminating value. Each test names the
    // seam it reveals.
    // -----------------------------------------------------------------------

    fn assert_err_containing(error: Result<(), CatalogError>, fragment: &str) {
        let message = format!("{}", error.expect_err("test: expected an error"));
        assert!(message.contains(fragment), "error `{message}` must name `{fragment}`");
    }

    fn doctored(doc: &str, from: &str, to: &str) -> String {
        let result = doc.replace(from, to);
        assert_ne!(result, doc, "test: probe `{from}` must exist in the inventory");
        result
    }

    /// Seam: extract_cell_text rejects unbalanced cells (direct call).
    #[test]
    fn seam_extract_cell_text_rejects_unbalanced_directly() {
        let error = extract_cell_text("`open only", "probe.summary");
        assert_err_containing(error.map(|_| ()), "unbalanced code-span delimiters");
        assert_err_containing(
            extract_cell_text("close only`", "probe.notes").map(|_| ()),
            "probe.notes",
        );
        assert_eq!(extract_cell_text("plain text", "probe").expect("plain cell ok"), "plain text");
        assert_eq!(
            extract_cell_text("`whole span`", "probe").expect("whole span trims"),
            "whole span"
        );
        assert_eq!(
            extract_cell_text("`a` between `b`", "probe").expect("multi-span stays verbatim"),
            "`a` between `b`"
        );
    }

    /// Seam: trim_code_span_pair symmetric rules (direct table).
    #[test]
    fn seam_trim_code_span_pair_cases() {
        assert_eq!(trim_code_span_pair("  `whole`  "), "whole");
        assert_eq!(trim_code_span_pair("`a` b `c`"), "`a` b `c`");
        assert_eq!(trim_code_span_pair("`open"), "`open");
        assert_eq!(trim_code_span_pair("close`"), "close`");
        assert_eq!(trim_code_span_pair("plain"), "plain");
        assert_eq!(trim_code_span_pair("``"), "");
    }

    /// Seam: resolve_link_label keeps display labels and plain text verbatim.
    #[test]
    fn seam_resolve_link_label_cases() {
        assert_eq!(resolve_link_label("[README.md](../README.md)"), "README.md");
        assert_eq!(resolve_link_label("no link"), "no link");
        assert_eq!(resolve_link_label("[unclosed"), "[unclosed");
    }

    /// Seam: parse_inventory fails when the audited-commit anchor is absent.
    #[test]
    fn seam_parse_inventory_missing_audited_anchor() {
        let error = parse_inventory(
            "# Install Claim Surface Inventory

no anchors here",
        )
        .expect_err("anchor-less doc must fail");
        assert!(format!("{error}").contains("could not locate the `**Audited against:**`"));
    }

    /// Seam: parse_inventory fails when the audit date is absent.
    #[test]
    fn seam_parse_inventory_missing_audited_date() {
        let doc = committed_doc();
        let doctored = doctored(&doc, "(2026-08-26)", "no-date").replace("(2026-06-28)", "no-date");
        assert_err_containing(
            parse_inventory(&doctored).map(|_| ()),
            "could not locate the audit date",
        );
    }

    /// Seam: parse_inventory fails when the drift anchor release is absent.
    #[test]
    fn seam_parse_inventory_missing_release_anchor() {
        let doc = committed_doc();
        let doctored = doc.replace("**Drift anchor:**", "**Drift notes:**");
        assert_err_containing(
            parse_inventory(&doctored).map(|_| ()),
            "could not locate the drift-anchor release receipt",
        );
    }

    /// Seam: parse_inventory fails when a claim row precedes any S-heading.
    #[test]
    fn seam_parse_inventory_claim_row_before_section() {
        let doc = committed_doc();
        let doctored = doctored(
            &doc,
            "## Claim rows",
            "## Claim rows\n\n| C0000 | probe.md:1 | probe | `current` | probe |",
        );
        assert_err_containing(
            parse_inventory(&doctored).map(|_| ()),
            "appeared before any `### Sxx` heading",
        );
    }

    /// Seam: parse_surface_row rejects malformed surface rows.
    #[test]
    fn seam_parse_surface_row_malformed() {
        let doc = committed_doc();
        let doctored = doctored(
            &doc,
            "| S01 | [README.md](../../README.md) | root landing prose | commands + boundaries | \u{2014} |",
            "| S01 | too-few-cells |",
        );
        assert_err_containing(parse_inventory(&doctored).map(|_| ()), "malformed surface row");
    }

    /// Seam: parse_claim_row rejects malformed claim rows.
    #[test]
    fn seam_parse_claim_row_malformed() {
        let doc = committed_doc();
        let doctored = doctored(
            &doc,
            "| C801 | TROUBLESHOOTING.md:3-16 | If basic probes fail, fix binary installation and `PATH` first before deeper debugging | `current` | Route-recommendation claim (diagnostic-surface class) |",
            "| C801 | only-two-cells |",
        );
        assert_err_containing(parse_inventory(&doctored).map(|_| ()), "malformed claim row");
    }

    /// Seam: parse_findings rejects a finding without its `.**` terminator.
    #[test]
    fn seam_parse_findings_missing_title_terminator() {
        let doc = committed_doc();
        let doctored = doctored(
            &doc,
            "unpinned release-archive download outside S02.**",
            "unpinned release-archive download outside S02**",
        );
        assert_err_containing(
            parse_inventory(&doctored).map(|_| ()),
            "has no `.**` title terminator",
        );
    }

    /// Seam: parse_findings rejects an empty finding title.
    #[test]
    fn seam_parse_findings_empty_title() {
        let doc = committed_doc();
        let doctored = doctored(
            &doc,
            "**FND-1 \u{2014} `@master` literal action pins (consumer-facing).**",
            "**FND-1 \u{2014} .**",
        );
        assert_err_containing(parse_inventory(&doctored).map(|_| ()), "has an empty title");
    }

    /// Seam: parse_inventory fails when a finding bullet is missing.
    #[test]
    fn seam_parse_findings_missing_bullet() {
        let doc = committed_doc();
        let doctored = doctored(
            &doc,
            "- **FND-12 \u{2014} unpinned release-archive download outside S02.**",
            "- (finding withheld)",
        );
        assert_err_containing(
            parse_inventory(&doctored).map(|_| ()),
            "findings section is missing `FND-12`",
        );
    }

    /// Seam: validate_denominator fails on unknown drift vocabulary.
    #[test]
    fn seam_validate_denominator_unknown_drift_status() {
        let doc = committed_doc();
        let doctored =
            doctored(&doc, "| `current` | Matches S02/S12 |", "| `banana` | Matches S02/S12 |");
        let inventory = parse_inventory(&doctored).expect("doctored doc parses");
        assert_err_containing(
            validate_denominator(&inventory).map(|_| ()),
            "uses unknown drift status `banana`",
        );
    }

    /// Seam: validate_denominator fails when a denominator surface row is dropped.
    #[test]
    fn seam_validate_denominator_missing_surface() {
        let doc = committed_doc();
        let doctored = doctored(
            &doc,
            "| S13 | docs/EDITORS/*_SETUP.md (7 guides) | editor integration guides | acquisition commands | \u{2014} |",
            "",
        );
        let inventory = parse_inventory(&doctored).expect("doctored doc parses");
        assert_err_containing(
            validate_denominator(&inventory).map(|_| ()),
            "missing denominator surface S13",
        );
    }

    /// Seam: validate_denominator fails when an unexpected claim row appears.
    #[test]
    fn seam_validate_denominator_extra_claim_row() {
        let doc = committed_doc();
        let with_extra = doc.replace(
            "## Findings",
            "| C9999 | CODEX_CLI_SETUP.md:31 | probe row | `current` | probe |

## Findings",
        );
        let inventory = parse_inventory(&with_extra).expect("extra row parses");
        assert_err_containing(
            validate_denominator(&inventory).map(|_| ()),
            "outside the recorded denominator",
        );
        // The undisturbed document still passes the denominator (sanity).
        let inventory = parse_inventory(&doc).expect("doc parses");
        validate_denominator(&inventory).expect("denominator holds");
    }

    /// Seam: validate_denominator fails when the FND-1 rel C401 join is lost.
    #[test]
    fn seam_validate_denominator_d4_regression() {
        let doc = committed_doc();
        let doctored = doctored(
            &doc,
            ".github/actions/setup-perl-lsp/action.yml:3",
            "elsewhere/renamed-file.md:3",
        );
        let inventory = parse_inventory(&doctored).expect("doctored doc still parses");
        let error = validate_denominator(&inventory).expect_err("D4 regression must trip");
        assert!(format!("{error}").contains("relation"), "{error}");
    }

    /// Seam: parse_findings citations resolve from the finding body.
    #[test]
    fn seam_parse_findings_cited_files() {
        let doc = committed_doc();
        let inventory = parse_inventory(&doc).expect("inventory parses");
        let fnd9 = inventory
            .findings
            .iter()
            .find(|finding| finding.finding_id == "FND-9")
            .expect("FND-9 present");
        assert!(fnd9.cited_files.contains(&"README.md".to_string()));
        assert!(fnd9.cited_files.contains(&"INSTALLATION.md".to_string()));
        assert!(!fnd9.cited_files.contains(&"install.ps1".to_string()));
    }

    /// Seam: validate_artifact_bytes rejects non-UTF-8 bytes.
    #[test]
    fn seam_validate_artifact_bytes_not_utf8() {
        let error = validate_artifact_bytes(&[0xFF, 0xFE, 0x00]).expect_err("non-UTF-8 must fail");
        assert!(format!("{error}").contains("not UTF-8"));
    }

    /// Seam: validate_artifact_bytes rejects invalid JSON.
    #[test]
    fn seam_validate_artifact_bytes_invalid_json() {
        let error = validate_artifact_bytes(b"{not json").expect_err("invalid JSON must fail");
        assert!(format!("{error}").contains("invalid JSON"));
    }

    /// Seam: validate_artifact_bytes rejects non-canonical byte forms.
    #[test]
    fn seam_validate_artifact_bytes_non_canonical() {
        let value = committed_catalog();
        let text = serde_json::to_string(&value).expect("compact serialization");
        let error = validate_artifact_bytes(text.as_bytes()).expect_err("compact form must fail");
        assert!(format!("{error}").contains("canonical"));
    }

    /// Seam: validate_catalog_value rejects schema-forbidden root keys.
    #[test]
    fn seam_validate_catalog_value_rogue_root_key() {
        let mut catalog = committed_catalog();
        catalog["rogue"] = Value::Bool(true);
        let error = validate_catalog_value(&catalog).expect_err("rogue root key must fail");
        assert!(format!("{error}").contains("schema-forbidden key"));
    }

    /// Seam: validate_catalog_value rejects a wrong schema version.
    #[test]
    fn seam_validate_catalog_value_wrong_version() {
        let mut catalog = committed_catalog();
        catalog["schema_version"] = json!("public_release_claims.v1");
        let error = validate_catalog_value(&catalog).expect_err("wrong version must fail");
        assert!(format!("{error}").contains("v1 document must go to the v1 validator"));
    }

    /// Seam: validate_catalog_value rejects duplicate claim authority.
    #[test]
    fn seam_validate_catalog_value_duplicate_claim_ids() {
        let mut catalog = committed_catalog();
        if let Some(claims) = catalog.get_mut("claims").and_then(Value::as_array_mut) {
            let clone = claims[0].clone();
            claims.insert(1, clone);
        }
        let error = validate_catalog_value(&catalog).expect_err("duplicate ids must fail");
        assert!(format!("{error}").contains("duplicate route authority"));
    }

    /// Seam: validate_catalog_value rejects unknown drift statuses.
    #[test]
    fn seam_validate_catalog_value_unknown_drift() {
        let mut catalog = committed_catalog();
        if let Some(claims) = catalog.get_mut("claims").and_then(Value::as_array_mut) {
            claims[0]["drift_status"] = json!("banana");
        }
        let error = validate_catalog_value(&catalog).expect_err("unknown drift must fail");
        assert!(format!("{error}").contains("unknown drift status"));
    }

    /// Seam: validate_catalog_value rejects unbalanced summary delimiters.
    #[test]
    fn seam_validate_catalog_value_unbalanced_summary() {
        let mut catalog = committed_catalog();
        if let Some(claims) = catalog.get_mut("claims").and_then(Value::as_array_mut) {
            claims[0]["summary"] = json!("`dangling delimiter");
        }
        let error = validate_catalog_value(&catalog).expect_err("unbalanced summary must fail");
        assert!(format!("{error}").contains("unbalanced code-span delimiters"));
    }

    /// Seam: validate_identity_anti_claim rejects mutated anti-claim consts.
    #[test]
    fn seam_validate_anti_claim_shape() {
        let mut catalog = committed_catalog();
        let index = catalog
            .get("claims")
            .and_then(Value::as_array)
            .map(|claims| {
                claims
                    .iter()
                    .position(|claim| {
                        claim
                            .get("identity_anti_claims")
                            .and_then(Value::as_array)
                            .is_some_and(|items| !items.is_empty())
                    })
                    .expect("test: an anti-claimed row exists")
            })
            .expect("test: claims array");
        catalog["claims"][index]["identity_anti_claims"][0]["disposition"] = json!("install_me");
        let error = validate_catalog_value(&catalog).expect_err("mutated const must fail");
        assert!(format!("{error}").contains("must be `do_not_install`"));
        catalog["claims"][index]["identity_anti_claims"][0]["rogue"] = Value::Bool(true);
        let error = validate_catalog_value(&catalog).expect_err("rogue key must fail");
        assert!(format!("{error}").contains("schema-forbidden key"));
    }

    /// Seam: dimension family validation rejects rogue families and keys.
    #[test]
    fn seam_validate_dimension_families() {
        let mut catalog = committed_catalog();
        let index = catalog
            .get("claims")
            .and_then(Value::as_array)
            .map(|claims| {
                claims
                    .iter()
                    .position(|claim| claim.get("dimensions").is_some())
                    .expect("test: a dimensioned row exists")
            })
            .expect("test: claims array");
        catalog["claims"][index]["dimensions"]["rogue_family"] = json!({});
        let error = validate_catalog_value(&catalog).expect_err("rogue family must fail");
        assert!(format!("{error}").contains("schema-forbidden dimension family"));
        catalog["claims"][index]["dimensions"] =
            json!({"sha256sums_enforcement": {"mode": "fail_closed_required", "rogue": true}});
        let error = validate_catalog_value(&catalog).expect_err("rogue key must fail");
        assert!(format!("{error}").contains("schema-forbidden key"));
        catalog["claims"][index]["dimensions"] = json!({"windows_arm64": {"user_prose": "unsupported", "tracked_source": "built", "published_receipt_v0_17_0": "banana"}});
        let error = validate_catalog_value(&catalog).expect_err("bad receipt enum must fail");
        assert!(format!("{error}").contains("invalid published_receipt_v0_17_0"));
        catalog["claims"][index]["dimensions"] = json!({"windows_arm64": {"user_prose": "unsupported", "tracked_source": "built", "published_receipt_v0_17_0": "absent", "finding_refs": ["FND-99"]}});
        let error = validate_catalog_value(&catalog).expect_err("unknown finding ref must fail");
        assert!(format!("{error}").contains("unknown finding `FND-99`"));
    }

    /// Seam: findings validation rejects unknown related claims and the
    /// D4 regression absence.
    #[test]
    fn seam_validate_findings_rows() {
        let mut catalog = committed_catalog();
        if let Some(findings) = catalog.get_mut("findings").and_then(Value::as_array_mut) {
            for finding in findings.iter_mut() {
                if finding.get("finding_id").and_then(Value::as_str) == Some("FND-1") {
                    finding["rogue"] = Value::Bool(true);
                }
            }
        }
        let error = validate_catalog_value(&catalog).expect_err("rogue finding key must fail");
        assert!(format!("{error}").contains("schema-forbidden key"));
        let mut catalog = committed_catalog();
        if let Some(findings) = catalog.get_mut("findings").and_then(Value::as_array_mut) {
            for finding in findings.iter_mut() {
                if finding.get("finding_id").and_then(Value::as_str) == Some("FND-1") {
                    finding["related_claims"] = json!(["C9999"]);
                }
            }
        }
        let error = validate_catalog_value(&catalog).expect_err("unknown claim must fail");
        assert!(format!("{error}").contains("unknown claim `C9999`"));
        let mut catalog = committed_catalog();
        if let Some(findings) = catalog.get_mut("findings").and_then(Value::as_array_mut) {
            for finding in findings.iter_mut() {
                if finding.get("finding_id").and_then(Value::as_str) == Some(REGRESSION_FINDING) {
                    finding["related_claims"] = json!([]);
                }
            }
        }
        let error = validate_catalog_value(&catalog).expect_err("missing regression must fail");
        assert!(format!("{error}").contains("relation to C401 missing"));
    }

    /// Seam: receipt-manifest parsing rejects every malformed shape.
    #[test]
    fn seam_parse_receipt_manifest_shapes() {
        let error = parse_receipt_manifest(b"{not json").expect_err("invalid JSON must fail");
        assert!(format!("{error}").contains("invalid JSON"));
        for (probe, fragment) in [
            (br#"{"verified_date":"2026-08-28","assets":[{"name":"a"}]}"#.as_slice(), "release: missing"),
            (br#"{"release":"v0.17.0","assets":[{"name":"a"}]}"#.as_slice(), "source: missing"),
            (br#"{"release":"v0.17.0","source":"","verified_date":"2026-08-28","assets":[{"name":"a"}]}"#.as_slice(), "source: missing or empty"),
            (br#"{"release":"v0.17.0","source":"https://example.invalid/v0.17.0"}"#.as_slice(), "verified_date: missing"),
            (br#"{"release":"v0.17.0","source":"https://example.invalid/v0.17.0","verified_date":"2026-08-28"}"#.as_slice(), "assets: missing array"),
            (br#"{"release":"v0.17.0","source":"https://example.invalid/v0.17.0","verified_date":"2026-08-28","assets":[]}"#.as_slice(), "assets must be non-empty"),
            (br#"{"release":"v0.17.0","source":"https://example.invalid/v0.17.0","verified_date":"2026-08-28","assets":[{}]}"#.as_slice(), "missing name"),
            (br#"{"release":"v0.17.0","source":"https://example.invalid/v0.17.0","verified_date":"2026-08-28","assets":[{"name":"a","rogue":true}]}"#.as_slice(), "unknown key"),
            (br#"{"release":"v0.17.0","source":"https://example.invalid/v0.17.0","verified_date":"2026-08-28","assets":[{"name":"b"},{"name":"a"}]}"#.as_slice(), "unique and stored in sorted order"),
            (br#"{"release":"v0.17.0","source":"https://example.invalid/v0.17.0","verified_date":"2026-08-28","assets":[{"name":"a"},{"name":"a"}]}"#.as_slice(), "unique and stored in sorted order"),
            (br#"{"release":"v0.17.0","source":"https://example.invalid/v0.17.0","verified_date":"2026-08-28","assets":[{"name":"a"}],"rogue":true}"#.as_slice(), "unknown root key"),
            (br#"{"release":"0.17.0","source":"https://example.invalid/v0.17.0","verified_date":"2026-08-28","assets":[{"name":"a"}]}"#.as_slice(), "must match v<major>.<minor>.<patch>"),
            (br#"{"release":"v0.17.0","source":"https://example.invalid/v0.17.0","verified_date":"2026/08/28","assets":[{"name":"a"}]}"#.as_slice(), "must match YYYY-MM-DD"),
        ] {
            let error = parse_receipt_manifest(probe).expect_err("malformed manifest must fail");
            assert!(format!("{error}").contains(fragment), "{error}");
        }
        let ok = parse_receipt_manifest(
            br#"{"release":"v0.17.0","source":"https://example.invalid/v0.17.0","verified_date":"2026-08-28","assets":[{"name":"a"},{"name":"b"}]}"#,
        )
        .expect("sorted manifest parses");
        assert_eq!(ok.asset_names, vec!["a", "b"]);
        assert_eq!(ok.source, "https://example.invalid/v0.17.0");
    }

    /// Seam: validate_receipt_binding rejects a receipt field that contradicts
    /// the manifest, and a windows_arm64 row missing the field.
    #[test]
    fn seam_validate_receipt_binding() {
        let manifest = parse_receipt_manifest(
            br#"{"release":"v0.17.0","source":"https://example.invalid/v0.17.0","verified_date":"2026-08-28","assets":[{"name":"perllsp-0.17.0-x86_64-pc-windows-msvc.zip"}]}"#,
        )
        .expect("manifest parses");
        let mut catalog = committed_catalog();
        let index = catalog
            .get("claims")
            .and_then(Value::as_array)
            .map(|claims| {
                claims
                    .iter()
                    .position(|claim| claim.pointer("/dimensions/windows_arm64").is_some())
                    .expect("test: a windows_arm64 row exists")
            })
            .expect("test: claims array");
        catalog["claims"][index]["dimensions"]["windows_arm64"]["published_receipt_v0_17_0"] =
            json!("present");
        let error = validate_receipt_binding(&catalog, &manifest)
            .expect_err("contradicting receipt must fail");
        assert!(format!("{error}").contains("contradicts the release-asset manifest"));
        catalog["claims"][index]["dimensions"]["windows_arm64"]
            .as_object_mut()
            .expect("test: object")
            .remove("published_receipt_v0_17_0");
        let error = validate_receipt_binding(&catalog, &manifest)
            .expect_err("missing receipt field must fail");
        assert!(format!("{error}").contains("missing published_receipt_v0_17_0"));
    }

    /// Seam: the regenerated artifact binds exact digests of all three live
    /// inputs; swapping any input changes the artifact.
    #[test]
    fn seam_input_digests_binding() {
        let root = repo_root();
        let doc_bytes = read_repo_bytes(&root, DOC_PATH).expect("doc readable");
        let schema_bytes = read_repo_bytes(&root, SCHEMA_PATH).expect("schema readable");
        let manifest_bytes =
            read_repo_bytes(&root, RECEIPT_MANIFEST_PATH).expect("manifest readable");
        let catalog = committed_catalog();
        let expected: [(&str, &[u8]); 3] = [
            ("inventory_document", &doc_bytes),
            ("schema", &schema_bytes),
            ("release_receipt_manifest", &manifest_bytes),
        ];
        for (key, bytes) in expected {
            let digest = catalog
                .pointer(&format!("/input_digests/{key}"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("test: digest {key} present"));
            assert_eq!(digest, format!("sha256:{}", sha256_hex(bytes)), "digest {key}");
        }
        let mutated = with_input_digests(
            &catalog,
            &sha256_hex(b"other doc"),
            &sha256_hex(&schema_bytes),
            &sha256_hex(&manifest_bytes),
        );
        assert_ne!(mutated, catalog, "a swapped digest changes the artifact");
    }

    /// Seam: validate_schema_closure counts closed object nodes.
    #[test]
    fn seam_schema_closure_walk_count() {
        let schema_bytes = read_repo_bytes(&repo_root(), SCHEMA_PATH).expect("schema readable");
        let schema: Value = serde_json::from_slice(&schema_bytes).expect("schema parses");
        let closed = validate_schema_closure(&schema).expect("closure walk");
        assert!(closed >= 13, "walked {closed} objects");
    }
}
