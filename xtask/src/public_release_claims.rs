//! `public_release_claims.v2` — deterministic install-claim catalog (#11548).
//!
//! The catalog consumes the landed inventory
//! `docs/distribution/INSTALL_CLAIM_SURFACES.md` (#11575) as its join input:
//! every surface row (`S01`-`S13`) and claim row (`C101`-`C1309`) becomes one
//! v2 record. Conjunctive route contradictions (Windows ARM64 receipt-binding,
//! SHA256SUMS enforcement mode, product-unit membership) and caveat omissions
//! stay independent per-row dimensions instead of collapsing into a scalar
//! status, and the FND findings are carried verbatim so downstream consumers
//! (#11549 classifier) never re-read producers, issues, docs, or workflows.
//!
//! Coexistence: `schemas/public_release_claims.v1.schema.json` and its
//! validator remain untouched; v2 is additive (`#10333` current-versus-
//! historical rule). The generated artifact is byte-canonical (sorted keys,
//! two-space indent, single trailing LF) and rejected when tampered.

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{fmt, fs, path::Path};

pub const DOC_PATH: &str = "docs/distribution/INSTALL_CLAIM_SURFACES.md";
pub const SCHEMA_PATH: &str = "schemas/public_release_claims.v2.schema.json";
pub const ARTIFACT_PATH: &str = "distribution/public_release_claims.v2.json";
pub const SCHEMA_VERSION: &str = "public_release_claims.v2";
pub const GENERATOR_COMMAND: &str = "cargo xtask public-release-claims-v2 build --write";

/// Complete surface denominator from the landed inventory.
const EXPECTED_SURFACES: [&str; 13] =
    ["S01", "S02", "S03", "S04", "S05", "S06", "S07", "S08", "S09", "S10", "S11", "S12", "S13"];

/// Complete claim-row denominator. A missing or renamed inventory row fails
/// the check (missing-producer-omits-route falsifier); adding rows upstream is
/// a sanctioned regeneration, not a silent pass.
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
}

struct ParsedSurface {
    surface_id: String,
    path: String,
    role: String,
    claim_class: String,
    registry_cross_ref: String,
}

struct ParsedClaim {
    claim_id: String,
    surface_id: String,
    location: String,
    summary: String,
    drift_status: String,
    notes: String,
    finding_refs: Vec<String>,
}

pub struct ParsedInventory {
    audited_commit: String,
    audited_date: String,
    release_anchor: String,
    surfaces: Vec<ParsedSurface>,
    claims: Vec<ParsedClaim>,
    finding_titles: Vec<(String, String)>,
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

fn take_while_digits(text: &[u8], start: usize) -> (String, usize) {
    let mut end = start;
    while end < text.len() && text[end].is_ascii_digit() {
        end += 1;
    }
    (String::from_utf8_lossy(&text[start..end]).into_owned(), end)
}

/// Extract every `FND-N` reference (1..=12) from a raw table row line.
fn scan_finding_refs(line: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let bytes = line.as_bytes();
    let mut index = 0;
    while index + 4 <= bytes.len() {
        if &bytes[index..index + 4] == b"FND-" {
            let (digits, next) = take_while_digits(bytes, index + 4);
            if !digits.is_empty() {
                let number: u32 = digits.parse().unwrap_or(0);
                if (1..=12).contains(&number) {
                    let id = format!("FND-{number}");
                    if !refs.contains(&id) {
                        refs.push(id);
                    }
                }
            }
            index = next;
        } else {
            index += 1;
        }
    }
    refs.sort();
    refs.dedup();
    refs
}

fn strip_markdown(cell: &str) -> String {
    let trimmed = cell.trim();
    // Markdown links keep their display label; backticks drop away entirely.
    let unlinked = if trimmed.starts_with('[') && trimmed.contains(']') {
        match trimmed.find(']') {
            Some(close) => trimmed[1..close].to_string(),
            None => trimmed.to_string(),
        }
    } else {
        trimmed.to_string()
    };
    let mut result = unlinked.trim().trim_matches('`').trim().to_string();
    while result.starts_with('`') || result.ends_with('`') {
        result = result.trim_matches('`').trim().to_string();
    }
    result
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

    let finding_titles = parse_finding_titles(doc);
    for finding_id in FINDING_IDS {
        if !finding_titles.iter().any(|(id, _)| id == finding_id) {
            return Err(CatalogError::new(format!(
                "{DOC_PATH}: findings section is missing `{finding_id}`"
            )));
        }
    }

    Ok(ParsedInventory {
        audited_commit,
        audited_date,
        release_anchor,
        surfaces,
        claims,
        finding_titles,
    })
}

/// Extract each `- **FND-N — title.**` heading (multi-line safe).
fn parse_finding_titles(doc: &str) -> Vec<(String, String)> {
    let mut titles = Vec::new();
    let joined = doc.replace('\n', " ");
    for number in 1..=12u32 {
        let marker = format!("**FND-{number} — ");
        let Some(start) = joined.find(&marker) else {
            continue;
        };
        let after_marker = start + marker.len();
        let Some(end) = joined[after_marker..].find(".**") else {
            continue;
        };
        let raw = &joined[after_marker..after_marker + end];
        let mut cleaned = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        cleaned = cleaned.trim().to_string();
        if !cleaned.is_empty() {
            titles.push((format!("FND-{number}"), cleaned));
        }
    }
    titles
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
    row.trim()
        .strip_prefix('|')
        .unwrap_or(row.trim())
        .strip_suffix('|')
        .unwrap_or(row.trim())
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn parse_surface_row(row: &str) -> Result<Option<ParsedSurface>, CatalogError> {
    let cells = split_table_row(row);
    if cells.len() < 5 {
        return Err(CatalogError::new(format!(
            "{DOC_PATH}: malformed surface row (expected >=5 cells): {row}"
        )));
    }
    let surface_id = cells[0].clone();
    if !surface_id.starts_with("S") {
        return Ok(None);
    }
    let registry_cross_ref = strip_markdown(&cells[4]).replace('\u{2014}', "");
    Ok(Some(ParsedSurface {
        surface_id,
        path: strip_markdown(&cells[1]),
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
    let notes = cells.get(4).cloned().unwrap_or_default();
    Ok(Some(ParsedClaim {
        claim_id,
        surface_id,
        location: cells[1].clone(),
        summary: strip_markdown(&cells[2]),
        drift_status: strip_markdown(&cells[3]),
        notes,
        finding_refs: scan_finding_refs(row),
    }))
}

fn numeric_claim_key(claim_id: &str) -> u32 {
    claim_id.trim_start_matches('C').parse().unwrap_or(u32::MAX)
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

fn omitted_caveats(claim_id: &str) -> &'static [&'static str] {
    match claim_id {
        "C1304" | "C1305" => &["homebrew_tap_version_unproven"],
        "C1306" | "C1307" | "C1308" | "C1302" => &["crates_io_name_collision"],
        _ => &[],
    }
}

fn dimension_overrides(claim_id: &str) -> Value {
    match claim_id {
        "C210" => json!({
            "windows_arm64": {
                "user_prose": "x64_fallback_build_from_source",
                "tracked_source": "built",
                "published_receipt_v0_17_0": "present",
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
                "published_receipt_v0_17_0": "present",
                "finding_refs": ["FND-4", "FND-11"]
            }
        }),
        "C405" => json!({
            "windows_arm64": {
                "user_prose": "unspecified",
                "tracked_source": "built",
                "published_receipt_v0_17_0": "absent",
                "finding_refs": ["FND-4"]
            }
        }),
        "C501" => json!({
            "windows_arm64": {
                "user_prose": "supported",
                "tracked_source": "built",
                "published_receipt_v0_17_0": "absent",
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

fn insert_dimension_overrides(dimensions: &mut serde_json::Map<String, Value>, overrides: Value) {
    let Some(object) = overrides.as_object() else {
        return;
    };
    for (key, value) in object {
        dimensions.insert(key.clone(), value.clone());
    }
}

/// Assemble the canonical catalog value from the parsed inventory.
pub fn build_catalog_value(inventory: &ParsedInventory) -> Value {
    let claims: Vec<Value> = inventory
        .claims
        .iter()
        .map(|claim| {
            let mut finding_refs = claim.finding_refs.clone();
            let mut dimensions = serde_json::Map::new();
            insert_dimension_overrides(&mut dimensions, dimension_overrides(&claim.claim_id));
            if let Some(value) = dimensions.get("windows_arm64") {
                merge_dim_refs(value, &mut finding_refs);
            }
            if let Some(value) = dimensions.get("sha256sums_enforcement") {
                merge_dim_refs(value, &mut finding_refs);
            }
            if let Some(value) = dimensions.get("product_units") {
                merge_dim_refs(value, &mut finding_refs);
            }
            finding_refs.sort();
            finding_refs.dedup();

            let group = restatement_group(&claim.claim_id);
            let caveats = omitted_caveats(&claim.claim_id);
            let has_dimensions = !dimensions.is_empty();

            let mut claim_value = json!({
                "claim_id": claim.claim_id,
                "surface_id": claim.surface_id,
                "location": claim.location,
                "summary": claim.summary,
                "drift_status": claim.drift_status,
                "notes": claim.notes,
                "finding_refs": finding_refs,
                "restatement_group": group,
                "omitted_caveats": caveats,
            });
            if has_dimensions {
                claim_value["dimensions"] = Value::Object(dimensions);
            }
            claim_value
        })
        .collect();

    let mut related: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for claim in &inventory.claims {
        for finding in &claim.finding_refs {
            related.entry(finding.clone()).or_default().push(claim.claim_id.clone());
        }
    }

    let findings: Vec<Value> = FINDING_IDS
        .iter()
        .map(|finding_id| {
            let related_claims = related.remove(*finding_id).unwrap_or_default();
            let owner_route = FINDING_OWNERS
                .iter()
                .find(|(id, _)| id == finding_id)
                .map(|(_, route)| *route)
                .unwrap_or("none_recorded");
            let title = inventory
                .finding_titles
                .iter()
                .find(|(id, _)| id == finding_id)
                .map(|(_, title)| title.clone())
                .unwrap_or_else(|| finding_id.to_string());
            json!({
                "finding_id": finding_id,
                "title": title,
                "related_claims": related_claims,
                "owner_route": owner_route,
            })
        })
        .collect();

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

fn merge_dim_refs(dimension: &Value, refs: &mut Vec<String>) {
    if let Some(list) = dimension.get("finding_refs").and_then(Value::as_array) {
        for value in list {
            if let Some(reference) = value.as_str() {
                refs.push(reference.to_string());
            }
        }
    }
}

/// Attach input digests; separated so tests can diff structure before binding.
pub fn with_input_digests(catalog: &Value, doc_sha: &str, schema_sha: &str) -> Value {
    let mut updated = catalog.clone();
    updated["input_digests"] = json!({
        "inventory_document": format!("sha256:{doc_sha}"),
        "schema": format!("sha256:{schema_sha}"),
    });
    updated
}

/// Generate the exact artifact bytes for the repository state under `root`.
pub fn generate_artifact_bytes(root: &Path) -> Result<Vec<u8>, CatalogError> {
    let doc_bytes = read_repo_bytes(root, DOC_PATH)?;
    let schema_bytes = read_repo_bytes(root, SCHEMA_PATH)?;
    let doc = std::str::from_utf8(&doc_bytes)
        .map_err(|error| CatalogError::new(format!("{DOC_PATH}: not UTF-8: {error}")))?;
    let inventory = parse_inventory(doc)?;
    validate_denominator(&inventory)?;

    let mut catalog = build_catalog_value(&inventory);
    catalog = with_input_digests(&catalog, &sha256_hex(&doc_bytes), &sha256_hex(&schema_bytes));
    canonical_bytes(&catalog)
}

fn validate_denominator(inventory: &ParsedInventory) -> Result<(), CatalogError> {
    let surface_ids: Vec<&str> = inventory.surfaces.iter().map(|s| s.surface_id.as_str()).collect();
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

    let claim_ids: Vec<&str> = inventory.claims.iter().map(|c| c.claim_id.as_str()).collect();
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
        for reference in &claim.finding_refs {
            if !FINDING_IDS.contains(&reference.as_str()) {
                return Err(CatalogError::new(format!(
                    "{DOC_PATH}: {} references unknown finding `{reference}`",
                    claim.claim_id
                )));
            }
        }
    }
    Ok(())
}

/// Validate artifact bytes: structural checks plus deterministic canonical-byte
/// identity against the recorded denominator expectations.
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

fn validate_catalog_value(value: &Value) -> Result<CatalogStats, CatalogError> {
    let object = value.as_object().ok_or_else(|| CatalogError::new("catalog: expected object"))?;

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

    let surfaces = count_array(value, "surfaces", "catalog.surfaces")?;
    let claims_array = value
        .get("claims")
        .and_then(Value::as_array)
        .ok_or_else(|| CatalogError::new("catalog.claims: missing array"))?;
    let mut seen: std::collections::BTreeSet<String> = Default::default();
    let mut dimensioned_rows = 0usize;
    for claim in claims_array {
        let id = claim
            .get("claim_id")
            .and_then(Value::as_str)
            .ok_or_else(|| CatalogError::new("catalog.claims[].claim_id: missing"))?;
        if !seen.insert(id.to_string()) {
            return Err(CatalogError::new(format!(
                "catalog.claims: duplicate route authority `{id}`"
            )));
        }
        if claim.get("dimensions").and_then(Value::as_object).is_some_and(|d| !d.is_empty()) {
            dimensioned_rows += 1;
        }
    }
    let findings = count_array(value, "findings", "catalog.findings")?;
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
    if surfaces != EXPECTED_SURFACES.len() {
        return Err(CatalogError::new(format!(
            "catalog.surfaces: {} row(s) but the denominator holds {}",
            surfaces,
            EXPECTED_SURFACES.len()
        )));
    }
    if findings != FINDING_IDS.len() {
        return Err(CatalogError::new(format!(
            "catalog.findings: {} row(s) but the denominator holds {}",
            findings,
            FINDING_IDS.len()
        )));
    }
    Ok(CatalogStats { surfaces, claims: claims_array.len(), findings, dimensioned_rows })
}

fn count_array(value: &Value, key: &str, name: &str) -> Result<usize, CatalogError> {
    let array = value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| CatalogError::new(format!("{name}: missing array")))?;
    Ok(array.len())
}

/// Full in-repo gate: regenerate from the live sources and compare with the
/// committed artifact byte-for-byte, then apply the JSON schema as the
/// structural authority (closed schema: unknown fields are violations).
pub fn validate_repository_catalog(root: &Path) -> Result<CatalogStats, CatalogError> {
    let expected = generate_artifact_bytes(root)?;
    let actual = read_repo_bytes(root, ARTIFACT_PATH)?;

    if actual.len() != expected.len() || actual != expected {
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
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| CatalogError::new(format!("{SCHEMA_PATH}: invalid schema: {error}")))?;
    let catalog: Value = serde_json::from_slice(&actual)
        .map_err(|error| CatalogError::new(format!("{}: invalid JSON: {error}", ARTIFACT_PATH)))?;
    let mut violations = Vec::new();
    for error in validator.iter_errors(&catalog) {
        violations.push(format!("{}: schema violation: {error}", ARTIFACT_PATH));
    }
    if let Some(first) = violations.first() {
        return Err(CatalogError::new(first.clone()));
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
        // Mirrors `crate::utils::project_root`, which lives in the binary-only
        // module; the manifest-dir parent is the repository root either way.
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("test: xtask lives in a subdirectory")
            .to_path_buf()
    }

    fn artifact_bytes(root: &Path) -> Vec<u8> {
        read_repo_bytes(root, ARTIFACT_PATH).expect("test: committed artifact readable")
    }

    /// Falsifier 15: a second generation changes no bytes.
    #[test]
    fn generation_is_byte_stable_across_runs() {
        let root = repo_root();
        let first = generate_artifact_bytes(&root).expect("first generation");
        let second = generate_artifact_bytes(&root).expect("second generation");
        assert_eq!(first, second);
    }

    /// The committed artifact is exactly what the landed document generates.
    #[test]
    fn committed_artifact_matches_regeneration() {
        let root = repo_root();
        let regenerated = generate_artifact_bytes(&root).expect("regeneration");
        assert_eq!(artifact_bytes(&root), regenerated);
    }

    #[test]
    fn parsed_anchors_match_the_landed_inventory() {
        let root = repo_root();
        let doc_bytes = read_repo_bytes(&root, DOC_PATH).expect("inventory doc");
        let doc = std::str::from_utf8(&doc_bytes).expect("UTF-8 inventory");
        let inventory = parse_inventory(doc).expect("parse");
        assert_eq!(inventory.audited_commit, "20174d50c");
        assert_eq!(inventory.audited_date, "2026-08-26");
        assert_eq!(inventory.release_anchor, "v0.17.0");
        assert_eq!(inventory.surfaces.len(), EXPECTED_SURFACES.len());
        assert_eq!(inventory.claims.len(), EXPECTED_CLAIM_IDS.len());
    }

    #[test]
    fn claim_rows_carry_conjunctive_dimensions_without_collapse() {
        let root = repo_root();
        let value: Value = serde_json::from_slice(&artifact_bytes(&root)).expect("artifact JSON");
        let claims = value.get("claims").and_then(Value::as_array).expect("claims array");

        let by_id = |id: &str| {
            claims
                .iter()
                .find(|claim| claim.get("claim_id").and_then(Value::as_str) == Some(id))
                .unwrap_or_else(|| panic!("test: {id} present"))
                .clone()
        };

        // (a) Windows ARM64 three-way split stays independent per field.
        let c210 = by_id("C210");
        let arm210 = c210
            .pointer("/dimensions/windows_arm64")
            .cloned()
            .expect("C210 windows_arm64 dimension");
        assert_eq!(
            arm210.get("user_prose").and_then(Value::as_str),
            Some("x64_fallback_build_from_source")
        );
        assert_eq!(arm210.get("tracked_source").and_then(Value::as_str), Some("built"));
        assert_eq!(
            arm210.get("published_receipt_v0_17_0").and_then(Value::as_str),
            Some("present")
        );
        let c501 = by_id("C501");
        assert_eq!(
            c501.pointer("/dimensions/windows_arm64/published_receipt_v0_17_0")
                .and_then(Value::as_str),
            Some("absent")
        );

        // (b) checksum enforcement mode conflict stays visible on both rows.
        assert_eq!(
            by_id("C207")
                .pointer("/dimensions/sha256sums_enforcement/mode")
                .and_then(Value::as_str),
            Some("fail_closed_required")
        );
        assert_eq!(
            by_id("C1005")
                .pointer("/dimensions/sha256sums_enforcement/mode")
                .and_then(Value::as_str),
            Some("fail_open_conditional")
        );

        // (c) product-unit membership under BUILD_FROM_SOURCE stays server-only
        // while the tracked-installer divergence is recorded, not merged in.
        assert_eq!(
            by_id("C208")
                .pointer("/dimensions/product_units/build_from_source_units")
                .and_then(|v| v.as_array())
                .map(|units| units.len()),
            Some(1)
        );
        assert_eq!(
            by_id("C210")
                .pointer("/dimensions/product_units/tracked_installer_ships_adapter")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            by_id("C209").pointer("/dimensions/product_units/tracked_installer_ships_adapter"),
            None,
            "unrecorded cells stay absent instead of inventing a direction"
        );
    }

    #[test]
    fn caveat_omissions_are_explicit_rows_not_prose_loss() {
        let root = repo_root();
        let value: Value = serde_json::from_slice(&artifact_bytes(&root)).expect("artifact JSON");
        let claims = value.get("claims").and_then(Value::as_array).expect("claims");
        let find = |id: &str| {
            claims.iter().find(|c| c.get("claim_id").and_then(Value::as_str) == Some(id)).cloned()
        };
        for id in ["C1304", "C1305"] {
            let claim = find(id).unwrap_or_else(|| panic!("test: {id} present"));
            let caveats = claim.get("omitted_caveats").and_then(Value::as_array).expect("caveats");
            assert!(caveats.iter().any(|v| v.as_str() == Some("homebrew_tap_version_unproven")));
        }
        // Restatement groups keep the dedup join without choosing a fragment.
        assert_eq!(
            find("C204").expect("C204").get("restatement_group").and_then(Value::as_str),
            Some("bootstrap_identity")
        );
        assert_eq!(
            find("C801").expect("C801").get("restatement_group").and_then(Value::as_str),
            Some("verification_probes")
        );
    }

    /// Tamper rejection. A single flipped character must fail the gate:
    /// * formatting-preserving value changes are caught only by the
    ///   regenerate-and-compare path (`validate_repository_catalog`), which is
    ///   why that comparison, not shape checks alone, is load-bearing;
    /// * raw byte damage (broken JSON/non-canonical spacing) is caught by
    ///   `validate_artifact_bytes` before regeneration even runs.
    #[test]
    fn tampered_artifact_is_rejected() {
        let root = repo_root();
        let mut bytes = artifact_bytes(&root);
        let marker = b"volatile_number";
        let position = bytes
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("marker present");
        // Formatting-preserving drift: same length, same canonical shape.
        bytes[position + 2] = if bytes[position + 2] == b'o' { b'0' } else { b'X' };
        let regenerated = generate_artifact_bytes(&root).expect("regeneration");
        assert_ne!(bytes, regenerated, "gate compares artifact bytes to regeneration");
        validate_artifact_bytes(&bytes)
            .expect("shape checks alone cannot see a re-encoded value — by design");

        // Raw damage: same-length reindentation stays valid JSON but must fail
        // the canonical-form check immediately.
        let mut indented = bytes.clone();
        let start = indented
            .windows(2)
            .position(|window| window == b"  ")
            .expect("pretty indentation present");
        indented[start] = b'\t';
        let error = validate_artifact_bytes(&indented).err().expect("non-canonical form rejected");
        assert!(format!("{error}").contains("canonical form"), "{error}");
    }

    /// Unknown-field rejection and structural authority of the closed schema.
    #[test]
    fn unknown_top_level_field_fails_schema_validation() {
        let root = repo_root();
        let mut value: Value =
            serde_json::from_slice(&artifact_bytes(&root)).expect("artifact JSON");
        value["rogue_dimension"] = serde_json::json!(true);
        let bytes = canonical_bytes(&value).expect("re-serialize");

        // Structure/count guards alone still hold; only the closed schema sees
        // the unknown field. That is exactly why both layers run in the gate.
        validate_artifact_bytes(&bytes).expect("structure consistent");

        let schema_text = fs::read_to_string(root.join(SCHEMA_PATH)).expect("schema readable");
        let schema: Value = serde_json::from_str(&schema_text).expect("schema json");
        let validator = jsonschema::validator_for(&schema).expect("schema compiles");
        let catalog: Value = serde_json::from_slice(&bytes).expect("valid json");
        let violations: Vec<String> =
            validator.iter_errors(&catalog).map(|error| error.to_string()).collect();
        assert!(
            violations.iter().any(|violation| violation.contains("rogue_dimension")),
            "closed schema must reject unknown fields, got {violations:?}"
        );
    }

    /// Dropping a row from the artifact alone cannot produce a clean pass.
    #[test]
    fn shrunk_catalog_fails_even_when_canonical() {
        let root = repo_root();
        let mut value: Value =
            serde_json::from_slice(&artifact_bytes(&root)).expect("artifact JSON");
        let claims = value.get_mut("claims").and_then(Value::as_array_mut).expect("claims array");
        claims.pop();
        let bytes = canonical_bytes(&value).expect("canonical re-serialize");
        let error = validate_artifact_bytes(&bytes).err().expect("shrink rejected");
        assert!(format!("{error}").contains("denominator holds"), "{error}");
    }

    /// A v1-shaped document is refused with the coexistence pointer intact.
    #[test]
    fn v1_document_directs_to_v1_validator() {
        let v1 = serde_json::json!({
            "schema_version": "public_release_claims.v1",
            "release": "0.18.0",
            "track": "public-beta",
            "subject_sha": "0".repeat(40),
            "topology_digest": format!("sha256:{}", "0".repeat(64)),
            "claims": [{"id": "install.x"}],
        });
        let error = validate_catalog_value(&v1).err().expect("v1 rejected");
        let message = format!("{error}");
        assert!(message.contains("public_release_claims.v1"), "{message}");
        assert!(message.contains("v1 validator"), "{message}");
    }

    #[test]
    fn list_and_explain_surface_stable_ids() {
        let root = repo_root();
        let manifest: Value =
            serde_json::from_slice(&artifact_bytes(&root)).expect("artifact JSON");
        let ids = list_claim_ids(&manifest);
        assert_eq!(ids.first().map(String::as_str), Some("C101"));
        assert_eq!(ids.last().map(String::as_str), Some("C1309"));
        let explained = explain_claim(&manifest, "C207").expect("C207 exists");
        assert!(explained.contains("\"claim_id\": \"C207\""));
        assert!(explain_claim(&manifest, "C9999").is_none());
    }
}
