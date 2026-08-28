//! Independent static validation of the stable `issue_controller_train.v1`
//! manifest (T02 `#11765`).
//!
//! This validator is deliberately independent of manifest construction: it
//! re-derives every structural law, re-computes title fingerprints and the
//! canonical semantic digest from the parsed document, and refuses to warn —
//! any violated invariant is a fail-closed diagnostic. It never reads live
//! GitHub state, never inspects the current tree, and never mutates anything;
//! those planes are owned by T03/T06 and friends. It also never rewrites the
//! manifest to make it pass; corrections route through a T02R (`#11767`)
//! classified semantic revision.

use std::collections::{BTreeMap, BTreeSet};

use color_eyre::eyre::{Result, bail};

use super::digest::{canonical_digest, title_fingerprint};
use super::model::{
    AUTHORITY_PLANES, DEP_CLASSES, DISPOSITIONS, EXPECTED_NODES, HOME_PROGRAMME, LAW_EDGES,
    Manifest, OPEN_DECISION_OWNERS, REQUIRED_AUTHORITIES, SCHEMA_NAME, SCHEMA_VERSION, TRAIN_ROLES,
    WRITER_NAMESPACE,
};

/// Owner line printed with every diagnostic: stable-contract corrections are
/// classified by T02R and the manifest belongs to T01's stable contract.
pub const CORRECTION_OWNER: &str =
    "T02R #11767 (semantic train revision; manifest owner T01 #11764)";
pub const CORRECTION_ACTION: &str = "repair the manifest through a T02R-classified revision, \
     then regenerate the projection with `cargo xtask issue-controllers train graph`; never \
     edit the manifest to satisfy the validator and never edit generated artifacts by hand";

/// One fail-closed finding. Every diagnostic names the exact node/edge/field,
/// the violated invariant, the owning correction node and a safe next action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub check: &'static str,
    pub subject: String,
    pub invariant: String,
}

impl Diagnostic {
    fn new(check: &'static str, subject: String, invariant: String) -> Self {
        Diagnostic { check, subject, invariant }
    }

    /// Render the diagnostic in the stable multi-line envelope.
    pub fn render(&self) -> String {
        format!(
            "error[static.{}] {}\n  invariant: {}\n  owner: {}\n  action: {}",
            self.check, self.subject, self.invariant, CORRECTION_OWNER, CORRECTION_ACTION
        )
    }
}

/// Result of a static validation run.
pub struct StaticReport {
    pub diagnostics: Vec<Diagnostic>,
    /// The canonical semantic digest, present only when validation is clean.
    pub semantic_digest: Option<String>,
    /// The typed manifest, present only when validation is clean.
    pub manifest: Option<Manifest>,
}

impl StaticReport {
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Structural summary of the validated graph, used by the projection.
pub struct GraphSummary {
    pub node_count: usize,
    pub edge_count: usize,
    pub class_counts: BTreeMap<String, usize>,
}

impl GraphSummary {
    pub fn of(manifest: &Manifest) -> Self {
        let mut class_counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut edge_count = 0usize;
        for node in &manifest.nodes {
            for dep in &node.dependencies {
                *class_counts.entry(dep.class.clone()).or_insert(0) += 1;
                edge_count += 1;
            }
        }
        GraphSummary { node_count: manifest.nodes.len(), edge_count, class_counts }
    }
}

/// Validate raw manifest bytes: byte hygiene, strict typed parse, and every
/// static law. Collects all diagnostics instead of stopping at the first so
/// the owning corrector sees the full defect set.
pub fn validate_static_bytes(raw: &[u8]) -> StaticReport {
    let mut diagnostics = Vec::new();

    byte_hygiene(raw, &mut diagnostics);
    raw_bytes_live_state_scan(raw, &mut diagnostics);

    let parsed: Option<serde_json::Value> = match serde_json::from_slice::<serde_json::Value>(raw) {
        Ok(value) => Some(value),
        Err(error) => {
            diagnostics.push(Diagnostic::new(
                "serialization",
                "manifest root".to_owned(),
                format!("manifest is not valid JSON: {error}"),
            ));
            None
        }
    };

    if let Some(value) = parsed.as_ref() {
        live_state_scan(value, &mut diagnostics);
        reject_floats(value, "manifest root", &mut diagnostics);
    }

    let Some(value) = parsed else {
        return StaticReport { diagnostics, semantic_digest: None, manifest: None };
    };

    top_level_key_set(&value, &mut diagnostics);

    let manifest = match serde_json::from_value::<Manifest>(value.clone()) {
        Ok(manifest) => manifest,
        Err(error) => {
            let subject = serde_path_subject(&error.to_string(), &value);
            diagnostics.push(Diagnostic::new(
                "schema",
                subject,
                format!("manifest violates the issue_controller_train.v1 typed schema: {error}"),
            ));
            return StaticReport { diagnostics, semantic_digest: None, manifest: None };
        }
    };

    validate_static(&manifest, &value, &mut diagnostics);

    let semantic_digest = canonical_digest(&value).ok();
    if diagnostics.is_empty() {
        StaticReport { diagnostics, semantic_digest, manifest: Some(manifest) }
    } else {
        StaticReport { diagnostics, semantic_digest: None, manifest: None }
    }
}

/// Attribute a serde error path to the offending node when possible.
fn serde_path_subject(message: &str, value: &serde_json::Value) -> String {
    let node_ids: Vec<String> = value
        .get("nodes")
        .and_then(|nodes| nodes.as_array())
        .map(|nodes| {
            nodes
                .iter()
                .map(|n| {
                    n.get("node_id")
                        .and_then(|id| id.as_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| "<unnamed>".to_owned())
                })
                .collect()
        })
        .unwrap_or_default();
    for (index, node_id) in node_ids.iter().enumerate() {
        if message.contains(&format!("nodes[{index}]")) {
            return format!("node {node_id} (nodes[{index}])");
        }
    }
    "manifest root".to_owned()
}

fn byte_hygiene(raw: &[u8], diagnostics: &mut Vec<Diagnostic>) {
    if raw.len() >= 3 && raw[0] == 0xEF && raw[1] == 0xBB && raw[2] == 0xBF {
        diagnostics.push(Diagnostic::new(
            "byte-hygiene",
            "manifest bytes".to_owned(),
            "manifest must not start with a UTF-8 BOM".to_owned(),
        ));
    }
    if raw.contains(&b'\r') {
        diagnostics.push(Diagnostic::new(
            "byte-hygiene",
            "manifest bytes".to_owned(),
            "manifest must contain no CR bytes (LF line endings only)".to_owned(),
        ));
    }
    if raw.contains(&b'\t') {
        diagnostics.push(Diagnostic::new(
            "byte-hygiene",
            "manifest bytes".to_owned(),
            "manifest must contain no tab bytes".to_owned(),
        ));
    }
    let ends_with_single_lf = raw.ends_with(b"\n") && !raw.ends_with(b"\n\n");
    if !ends_with_single_lf {
        diagnostics.push(Diagnostic::new(
            "byte-hygiene",
            "manifest bytes".to_owned(),
            "manifest must end with exactly one trailing LF".to_owned(),
        ));
    }
}

/// Mirror the parsed-value scan over the exact bytes: SHA-like runs and
/// timestamps must fail closed even when they hide outside string values
/// (for example inside a key name).
fn raw_bytes_live_state_scan(raw: &[u8], diagnostics: &mut Vec<Diagnostic>) {
    let text = String::from_utf8_lossy(raw);
    for line in text.lines() {
        if looks_like_live_sha(line) {
            diagnostics.push(Diagnostic::new(
                "live-state",
                "manifest bytes".to_owned(),
                format!("possible live SHA token in manifest bytes: {line}"),
            ));
        }
        if looks_like_timestamp(line) {
            diagnostics.push(Diagnostic::new(
                "live-state",
                "manifest bytes".to_owned(),
                "possible live timestamp in manifest bytes".to_owned(),
            ));
        }
    }
}

const LIVE_STATE_TOKENS: [&str; 6] =
    ["origin/", "refs/heads/", "pull/", "PR #", "merge-base", "worktrees/"];

fn is_any_hex(byte: u8) -> bool {
    byte.is_ascii_hexdigit()
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn looks_like_live_sha(text: &str) -> bool {
    // A run of seven or more [0-9a-f] characters whose neighboring characters
    // are not hex in any case (mirrors `(?<![0-9A-Fa-f])[0-9a-f]{7,}(?![0-9A-Fa-f])`).
    // Uppercase-hex neighbors (title fingerprints) guard a run instead of
    // bounding it.
    let mut run = 0usize;
    let mut run_start_guarded = false;
    for &byte in text.as_bytes() {
        if is_lower_hex(byte) {
            run += 1;
            continue;
        }
        if run >= 7 && !run_start_guarded && !is_any_hex(byte) {
            return true;
        }
        run = 0;
        run_start_guarded = is_any_hex(byte);
    }
    run >= 7 && !run_start_guarded
}

fn looks_like_timestamp(text: &str) -> bool {
    // Shape: YYYY-MM-DDT...
    let bytes = text.as_bytes();
    bytes.len() > 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5].is_ascii_digit()
        && bytes[6].is_ascii_digit()
        && bytes[7] == b'-'
        && bytes[8].is_ascii_digit()
        && bytes[9].is_ascii_digit()
        && bytes[10] == b'T'
}

fn live_state_scan(value: &serde_json::Value, diagnostics: &mut Vec<Diagnostic>) {
    match value {
        serde_json::Value::String(text) => {
            if looks_like_live_sha(text) {
                diagnostics.push(Diagnostic::new(
                    "live-state",
                    "manifest string value".to_owned(),
                    format!("possible live SHA/state token in stable bytes: {text}"),
                ));
            }
            if looks_like_timestamp(text) {
                diagnostics.push(Diagnostic::new(
                    "live-state",
                    "manifest string value".to_owned(),
                    format!("possible live timestamp in stable bytes: {text}"),
                ));
            }
            for token in LIVE_STATE_TOKENS {
                if text.contains(token) {
                    diagnostics.push(Diagnostic::new(
                        "live-state",
                        "manifest string value".to_owned(),
                        format!("possible live-state token '{token}' in stable bytes"),
                    ));
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                live_state_scan(item, diagnostics);
            }
        }
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                live_state_scan(item, diagnostics);
                let _ = key;
            }
        }
        _ => {}
    }
}

fn reject_floats(value: &serde_json::Value, where_: &str, diagnostics: &mut Vec<Diagnostic>) {
    match value {
        serde_json::Value::Number(number) if number.is_f64() => {
            diagnostics.push(Diagnostic::new(
                "serialization",
                where_.to_owned(),
                format!("non-integer JSON number is a schema defect: {number}"),
            ));
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                reject_floats(item, &format!("{where_}[{index}]"), diagnostics);
            }
        }
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                reject_floats(item, &format!("{where_}.{key}"), diagnostics);
            }
        }
        _ => {}
    }
}

const TOP_KEYS: [&str; 12] = [
    "schema",
    "schema_version",
    "programme",
    "authority_planes",
    "train_role_vocabulary",
    "evidence_semantics",
    "external_authorities",
    "open_decisions_routed_elsewhere",
    "nodes",
    "supersessions",
    "revision_governance",
    "limitations",
];

fn top_level_key_set(value: &serde_json::Value, diagnostics: &mut Vec<Diagnostic>) {
    let Some(map) = value.as_object() else {
        diagnostics.push(Diagnostic::new(
            "schema",
            "manifest root".to_owned(),
            "manifest root must be a JSON object".to_owned(),
        ));
        return;
    };
    let actual: BTreeSet<&str> = map.keys().map(String::as_str).collect();
    let expected: BTreeSet<&str> = TOP_KEYS.into_iter().collect();
    let missing: Vec<&str> = expected.difference(&actual).copied().collect();
    let extra: Vec<&str> = actual.difference(&expected).copied().collect();
    if !missing.is_empty() || !extra.is_empty() {
        diagnostics.push(Diagnostic::new(
            "schema",
            "manifest root key set".to_owned(),
            format!(
                "exact key set required: missing=[{}] extra=[{}]",
                missing.join(","),
                extra.join(",")
            ),
        ));
    }
}

/// The full static law set over the typed manifest.
pub fn validate_static(
    manifest: &Manifest,
    value: &serde_json::Value,
    diagnostics: &mut Vec<Diagnostic>,
) {
    programme_anchors(manifest, diagnostics);
    authority_plane_laws(manifest, diagnostics);
    role_vocabulary_laws(manifest, diagnostics);
    evidence_semantics_laws(manifest, diagnostics);
    external_authority_laws(manifest, diagnostics);
    open_decision_laws(manifest, diagnostics);
    revision_governance_laws(manifest, diagnostics);

    let by_id = node_map(manifest, diagnostics);
    node_set_laws(manifest, &by_id, diagnostics);
    supersession_laws(manifest, &by_id, diagnostics);

    for node in &manifest.nodes {
        node_contract_laws(node, manifest, diagnostics);
        edge_laws(node, &by_id, manifest, diagnostics);
    }

    role_assignability_laws(manifest, diagnostics);
    writer_parallelism_laws(manifest, diagnostics);
    successor_derivation_laws(manifest, diagnostics);
    law_edge_laws(manifest, &by_id, diagnostics);
    acyclicity_laws(manifest, &by_id, diagnostics);
    orphan_and_route_laws(manifest, diagnostics);

    // The digest must be computable over the exact parsed document; if the
    // walk refuses (non-integer number), surface it as a diagnostic.
    if let Err(error) = canonical_digest(value) {
        diagnostics.push(Diagnostic::new(
            "serialization",
            "manifest root".to_owned(),
            format!("canonical semantic digest is not computable: {error}"),
        ));
    }
}

fn programme_anchors(manifest: &Manifest, diagnostics: &mut Vec<Diagnostic>) {
    if manifest.schema != SCHEMA_NAME {
        diagnostics.push(Diagnostic::new(
            "schema",
            "root.schema".to_owned(),
            format!("schema name must be exactly '{SCHEMA_NAME}'"),
        ));
    }
    if manifest.schema_version != SCHEMA_VERSION {
        diagnostics.push(Diagnostic::new(
            "schema",
            "root.schema_version".to_owned(),
            format!("schema_version must be exactly {SCHEMA_VERSION}"),
        ));
    }
    let programme = &manifest.programme;
    if programme.controller_issue != 11681 {
        diagnostics.push(Diagnostic::new(
            "anchor",
            "programme.controller_issue".to_owned(),
            format!(
                "programme controller issue must be 11681, found {}",
                programme.controller_issue
            ),
        ));
    }
    if programme.home_programme != HOME_PROGRAMME {
        diagnostics.push(Diagnostic::new(
            "anchor",
            "programme.home_programme".to_owned(),
            format!(
                "home programme must be '{HOME_PROGRAMME}', found '{}'",
                programme.home_programme
            ),
        ));
    }
    if programme.durable_architecture_issue != 11763 {
        diagnostics.push(Diagnostic::new(
            "anchor",
            "programme.durable_architecture_issue".to_owned(),
            format!(
                "durable architecture issue must be 11763, found {}",
                programme.durable_architecture_issue
            ),
        ));
    }
}

fn authority_plane_laws(manifest: &Manifest, diagnostics: &mut Vec<Diagnostic>) {
    if manifest.authority_planes.len() != AUTHORITY_PLANES.len() {
        diagnostics.push(Diagnostic::new(
            "planes",
            "authority_planes".to_owned(),
            format!(
                "expected exactly {} authority planes, found {}",
                AUTHORITY_PLANES.len(),
                manifest.authority_planes.len()
            ),
        ));
    }
    for (index, plane) in manifest.authority_planes.iter().enumerate() {
        let Some(&expected) = AUTHORITY_PLANES.get(index) else { break };
        if plane.plane != expected {
            diagnostics.push(Diagnostic::new(
                "planes",
                format!("authority_planes[{index}]"),
                format!(
                    "authority plane order broken: expected '{expected}', found '{}'",
                    plane.plane
                ),
            ));
        }
        if plane.owns.trim().is_empty() || plane.never_substitutes.trim().is_empty() {
            diagnostics.push(Diagnostic::new(
                "planes",
                format!("authority_planes[{index}].owns/never_substitutes"),
                "authority plane ownership statements must be non-empty".to_owned(),
            ));
        }
    }
}

fn role_vocabulary_laws(manifest: &Manifest, diagnostics: &mut Vec<Diagnostic>) {
    if manifest.train_role_vocabulary.len() != TRAIN_ROLES.len() {
        diagnostics.push(Diagnostic::new(
            "roles",
            "train_role_vocabulary".to_owned(),
            format!(
                "expected exactly {} train-execution roles, found {}",
                TRAIN_ROLES.len(),
                manifest.train_role_vocabulary.len()
            ),
        ));
    }
    for (index, role) in manifest.train_role_vocabulary.iter().enumerate() {
        let Some(&expected) = TRAIN_ROLES.get(index) else { break };
        if role.role != expected {
            diagnostics.push(Diagnostic::new(
                "roles",
                format!("train_role_vocabulary[{index}]"),
                format!("train role order broken: expected '{expected}', found '{}'", role.role),
            ));
        }
        if role.owns.trim().is_empty() {
            diagnostics.push(Diagnostic::new(
                "roles",
                format!("train_role_vocabulary[{index}].owns"),
                "train role ownership statement must be non-empty".to_owned(),
            ));
        }
    }
}

fn evidence_semantics_laws(manifest: &Manifest, diagnostics: &mut Vec<Diagnostic>) {
    let semantics = &manifest.evidence_semantics;
    if semantics.not_proven_law.trim().is_empty() {
        diagnostics.push(Diagnostic::new(
            "evidence",
            "evidence_semantics.not_proven_law".to_owned(),
            "the not_proven law must stay explicit; missing/partial/stale evidence may never silently pass".to_owned(),
        ));
    }
    if semantics.optional_visibility.trim().is_empty() {
        diagnostics.push(Diagnostic::new(
            "evidence",
            "evidence_semantics.optional_visibility".to_owned(),
            "optional/unavailable rows must remain explicit and never disappear".to_owned(),
        ));
    }
}

fn external_authority_laws(manifest: &Manifest, diagnostics: &mut Vec<Diagnostic>) {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for authority in &manifest.external_authorities {
        if !authority.id.starts_with('#') {
            diagnostics.push(Diagnostic::new(
                "authorities",
                format!("external_authorities[{}].id", authority.id),
                "external authority id must start with '#'".to_owned(),
            ));
        }
        if !seen.insert(&authority.id) {
            diagnostics.push(Diagnostic::new(
                "authorities",
                format!("external_authorities[{}]", authority.id),
                "duplicate external authority id".to_owned(),
            ));
        }
        if authority.subject.trim().is_empty() {
            diagnostics.push(Diagnostic::new(
                "authorities",
                format!("external_authorities[{}].subject", authority.id),
                "external authority subject must be non-empty".to_owned(),
            ));
        }
    }
    for required in REQUIRED_AUTHORITIES {
        if !seen.contains(required) {
            diagnostics.push(Diagnostic::new(
                "authorities",
                format!("external_authorities[{required}]"),
                "required external authority missing from the registry".to_owned(),
            ));
        }
    }
}

fn open_decision_laws(manifest: &Manifest, diagnostics: &mut Vec<Diagnostic>) {
    if manifest.open_decisions_routed_elsewhere.len() != OPEN_DECISION_OWNERS.len() {
        diagnostics.push(Diagnostic::new(
            "open-decisions",
            "open_decisions_routed_elsewhere".to_owned(),
            format!(
                "expected exactly {} routed open decisions, found {}",
                OPEN_DECISION_OWNERS.len(),
                manifest.open_decisions_routed_elsewhere.len()
            ),
        ));
    }
    for (index, decision) in manifest.open_decisions_routed_elsewhere.iter().enumerate() {
        let Some(&(expected_id, expected_node, expected_issue)) = OPEN_DECISION_OWNERS.get(index)
        else {
            break;
        };
        if decision.id != expected_id
            || decision.owning_node != expected_node
            || decision.owning_issue != expected_issue
        {
            diagnostics.push(Diagnostic::new(
                "open-decisions",
                format!("open_decisions[{index}]"),
                format!(
                    "open decision must stay routed to its owner: expected {expected_id} -> \
                     {expected_node} #{expected_issue}, found {} -> {} #{}",
                    decision.id, decision.owning_node, decision.owning_issue
                ),
            ));
        }
        if decision.subject.trim().is_empty() {
            diagnostics.push(Diagnostic::new(
                "open-decisions",
                format!("open_decisions[{index}].subject"),
                "open decision subject must be non-empty".to_owned(),
            ));
        }
    }
}

fn revision_governance_laws(manifest: &Manifest, diagnostics: &mut Vec<Diagnostic>) {
    let governance = &manifest.revision_governance;
    if governance.owner_node != "T02R" || governance.owner_issue != 11767 {
        diagnostics.push(Diagnostic::new(
            "revision",
            "revision_governance.owner".to_owned(),
            "semantic revision/invalidation must stay owned by T02R #11767".to_owned(),
        ));
    }
    if governance.invalidates.trim().is_empty() || governance.never.trim().is_empty() {
        diagnostics.push(Diagnostic::new(
            "revision",
            "revision_governance.invalidates/never".to_owned(),
            "revision invalidation scope and prohibitions must stay explicit".to_owned(),
        ));
    }
}

fn node_map(manifest: &Manifest, diagnostics: &mut Vec<Diagnostic>) -> BTreeMap<String, usize> {
    let mut by_id: BTreeMap<String, usize> = BTreeMap::new();
    for (index, node) in manifest.nodes.iter().enumerate() {
        if by_id.insert(node.node_id.clone(), index).is_some() {
            diagnostics.push(Diagnostic::new(
                "uniqueness",
                format!("node {}", node.node_id),
                "duplicate node_id".to_owned(),
            ));
        }
    }
    by_id
}

fn node_set_laws(
    manifest: &Manifest,
    by_id: &BTreeMap<String, usize>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let expected: BTreeMap<&str, u64> =
        EXPECTED_NODES.iter().map(|(id, issue)| (*id, *issue)).collect();
    if manifest.nodes.len() != expected.len() {
        diagnostics.push(Diagnostic::new(
            "node-set",
            "nodes".to_owned(),
            format!(
                "the stable graph freezes exactly {} nodes, found {}",
                expected.len(),
                manifest.nodes.len()
            ),
        ));
    }
    let mut seen_issues: BTreeMap<u64, &str> = BTreeMap::new();
    let mut seen_aliases: BTreeMap<&str, &str> = BTreeMap::new();
    let mut seen_keys: BTreeMap<&str, &str> = BTreeMap::new();
    let mut seen_authority_after: BTreeMap<&str, &str> = BTreeMap::new();

    for node in &manifest.nodes {
        let Some(&expected_issue) = expected.get(node.node_id.as_str()) else {
            diagnostics.push(Diagnostic::new(
                "node-set",
                format!("node {}", node.node_id),
                format!(
                    "unexpected node_id '{}' is not part of the frozen stable node set; a node-set \
                     change is a T02R-classified semantic revision",
                    node.node_id
                ),
            ));
            continue;
        };
        if node.issue != expected_issue {
            diagnostics.push(Diagnostic::new(
                "node-set",
                format!("node {}", node.node_id),
                format!(
                    "node/issue mismatch: {0} must own #{1}, found #{2}",
                    node.node_id, expected_issue, node.issue
                ),
            ));
        }
        if let Some(previous) = seen_issues.insert(node.issue, &node.node_id) {
            diagnostics.push(Diagnostic::new(
                "uniqueness",
                format!("nodes {previous}/{}", node.node_id),
                format!("issue #{} is assigned to more than one node", node.issue),
            ));
        }
        for alias in &node.aliases {
            if let Some(previous) = seen_aliases.insert(alias.as_str(), &node.node_id) {
                diagnostics.push(Diagnostic::new(
                    "uniqueness",
                    format!("nodes {previous}/{}", node.node_id),
                    format!("duplicate alias '{alias}'"),
                ));
            }
        }
        if let Some(previous) = seen_keys.insert(node.writer.conflict_key.as_str(), &node.node_id) {
            diagnostics.push(Diagnostic::new(
                "conflict",
                format!("nodes {previous}/{}", node.node_id),
                format!(
                    "two nodes advertise the same semantic writer slot '{}' — path non-overlap \
                     cannot override one semantic owner",
                    node.writer.conflict_key
                ),
            ));
        }
        if let Some(previous) =
            seen_authority_after.insert(node.authority_after.as_str(), &node.node_id)
        {
            diagnostics.push(Diagnostic::new(
                "uniqueness",
                format!("nodes {previous}/{}", node.node_id),
                "duplicate authority-after proposition: two nodes would own the same \
                     post-condition"
                    .to_owned(),
            ));
        }
    }
    for expected_id in expected.keys() {
        if !by_id.contains_key(*expected_id) {
            diagnostics.push(Diagnostic::new(
                "node-set",
                format!("node {expected_id}"),
                "expected node of the frozen stable set is missing".to_owned(),
            ));
        }
    }
}

fn supersession_laws(
    manifest: &Manifest,
    by_id: &BTreeMap<String, usize>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for supersession in &manifest.supersessions {
        let subject = format!("supersession of {}", supersession.superseded_node);
        if !seen.insert(&supersession.superseded_node) {
            diagnostics.push(Diagnostic::new(
                "supersession",
                subject.clone(),
                "duplicate supersession for one node".to_owned(),
            ));
        }
        let Some(&index) = by_id.get(supersession.superseded_node.as_str()) else {
            diagnostics.push(Diagnostic::new(
                "supersession",
                subject.clone(),
                "supersession names an unknown node".to_owned(),
            ));
            continue;
        };
        let superseded_issue = manifest.nodes[index].issue;
        if supersession.successor_issue == superseded_issue {
            diagnostics.push(Diagnostic::new(
                "supersession",
                subject.clone(),
                "successor issue must differ from the superseded node's own issue".to_owned(),
            ));
        }
        let successor_nodes: Vec<&str> = manifest
            .nodes
            .iter()
            .filter(|n| {
                n.issue == supersession.successor_issue && n.node_id != supersession.superseded_node
            })
            .map(|n| n.node_id.as_str())
            .collect();
        let Some(successor) = successor_nodes.first().copied() else {
            diagnostics.push(Diagnostic::new(
                "supersession",
                subject.clone(),
                "supersession names an unknown successor issue".to_owned(),
            ));
            continue;
        };
        // A superseded node must be drained: only its successor may still
        // hard-require it; anyone else keeping the edge leaves the superseded
        // node active beside its successor.
        let mut active_dependents: Vec<String> = Vec::new();
        for node in &manifest.nodes {
            let still_hard = node.dependencies.iter().any(|dep| {
                dep.target == supersession.superseded_node
                    && matches!(dep.class.as_str(), "hard" | "evidence")
            });
            if still_hard && node.node_id != successor {
                active_dependents.push(node.node_id.clone());
            }
        }
        if !active_dependents.is_empty() {
            diagnostics.push(Diagnostic::new(
                "supersession",
                subject.clone(),
                format!(
                    "superseded node remains active beside its successor: non-successor \
                     dependents still hold hard/evidence edges: [{}]",
                    active_dependents.join(",")
                ),
            ));
        }
        if supersession.reason.trim().is_empty() {
            diagnostics.push(Diagnostic::new(
                "supersession",
                subject,
                "supersession reason must be non-empty".to_owned(),
            ));
        }
    }
}

fn node_contract_laws(
    node: &super::model::TrainNode,
    manifest: &Manifest,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let subject = format!("node {}", node.node_id);
    let registry: BTreeSet<&str> =
        manifest.external_authorities.iter().map(|a| a.id.as_str()).collect();

    let expected_fingerprint = title_fingerprint(&node.title);
    if node.title_fingerprint != expected_fingerprint {
        diagnostics.push(Diagnostic::new(
            "fingerprint",
            format!("{subject}.title_fingerprint"),
            format!(
                "title fingerprint mismatch: recomputed {expected_fingerprint} from the exact title, found {}",
                node.title_fingerprint
            ),
        ));
    }
    if !TRAIN_ROLES.contains(&node.train_role.as_str()) {
        diagnostics.push(Diagnostic::new(
            "roles",
            format!("{subject}.train_role"),
            format!(
                "unknown train role '{}' — issue-plane role vocabularies must not leak into \
                 train-execution roles",
                node.train_role
            ),
        ));
    }
    if node.chain.home != HOME_PROGRAMME || node.chain.controller != "CTRL" {
        diagnostics.push(Diagnostic::new(
            "import",
            format!("{subject}.chain"),
            format!(
                "every node belongs to exactly one home programme under CTRL: expected \
                 home='{HOME_PROGRAMME}' controller='CTRL', found home='{}' controller='{}'; an \
                 imported authority copied under another home programme requires an explicit \
                 import edge, not a second home",
                node.chain.home, node.chain.controller
            ),
        ));
    }
    if !node.writer.conflict_key.starts_with(WRITER_NAMESPACE) {
        diagnostics.push(Diagnostic::new(
            "conflict",
            format!("{subject}.writer.conflict_key"),
            format!(
                "conflict key '{}' must live in the '{WRITER_NAMESPACE}' semantic writer namespace",
                node.writer.conflict_key
            ),
        ));
    }
    if node.writer.conflict_key.trim().is_empty() {
        diagnostics.push(Diagnostic::new(
            "conflict",
            format!("{subject}.writer.conflict_key"),
            "conflict key must be non-empty".to_owned(),
        ));
    }
    if !DISPOSITIONS.contains(&node.spec.disposition.as_str()) {
        diagnostics.push(Diagnostic::new(
            "spec",
            format!("{subject}.spec.disposition"),
            format!("unknown spec disposition '{}'", node.spec.disposition),
        ));
    }
    if node.spec.owner != node.node_id {
        diagnostics.push(Diagnostic::new(
            "spec",
            format!("{subject}.spec.owner"),
            "each node owns its own spec disposition".to_owned(),
        ));
    }
    for (field, value) in [
        ("spec.stale_policy", &node.spec.stale_policy),
        ("controls.positive", &node.controls.positive),
        ("controls.opposite", &node.controls.opposite),
        ("controls.stale", &node.controls.stale),
        ("controls.wrong_subject", &node.controls.wrong_subject),
        ("controls.fault", &node.controls.fault),
        ("controls.mutation", &node.controls.mutation),
        ("proof.focused", &node.proof.focused),
        ("obligations.schema", &node.obligations.schema),
        ("obligations.generated", &node.obligations.generated),
        ("obligations.docs", &node.obligations.docs),
        ("obligations.changelog", &node.obligations.changelog),
        ("obligations.receipt", &node.obligations.receipt),
        ("exits.old_path", &node.exits.old_path),
        ("exits.compatibility", &node.exits.compatibility),
        ("exits.supersession", &node.exits.supersession),
        ("exits.transfer", &node.exits.transfer),
        ("rollback.rollback", &node.rollback.rollback),
        ("rollback.return_to_issue", &node.rollback.return_to_issue),
        ("rollback.not_proven", &node.rollback.not_proven),
        ("rollback.stop", &node.rollback.stop),
        ("one_pr_outcome", &node.one_pr_outcome),
        ("authority_before", &node.authority_before),
        ("authority_after", &node.authority_after),
        ("claim_ceiling", &node.claim_ceiling),
        ("first_falsifier", &node.first_falsifier),
        ("title", &node.title),
    ] {
        if value.trim().is_empty() {
            diagnostics.push(Diagnostic::new(
                "contract",
                format!("{subject}.{field}"),
                "required contract field is empty or blank — missing falsifier/review/docs/\
                 rollback/stop contracts are errors, never warnings"
                    .to_owned(),
            ));
        }
    }
    if node.review_forward.questions.is_empty() || node.review_forward.lenses.is_empty() {
        diagnostics.push(Diagnostic::new(
            "contract",
            format!("{subject}.review_forward"),
            "each node carries at least one review question and one review lens".to_owned(),
        ));
    }
    if node.identity_fields.is_empty()
        || node.allowed_components.is_empty()
        || node.forbidden_adjacent_owners.is_empty()
    {
        diagnostics.push(Diagnostic::new(
            "contract",
            format!("{subject}.identity_fields/allowed_components/forbidden_adjacent_owners"),
            "identity fields, allowed components and forbidden adjacent owners are required"
                .to_owned(),
        ));
    }
    for authority in &node.consumed_authorities {
        if !registry.contains(authority.as_str()) {
            diagnostics.push(Diagnostic::new(
                "authorities",
                format!("{subject}.consumed_authorities"),
                format!(
                    "node references command/spec/generator authority '{authority}' with no \
                     owner or explicit planned state in the external authority registry"
                ),
            ));
        }
    }
}

fn provenance_is_well_formed(provenance: &str) -> bool {
    if let Some(rest) = provenance.strip_prefix('#') {
        let body = rest
            .strip_suffix(" body references")
            .or_else(|| rest.strip_suffix(" body fan-in consumers"));
        if let Some(digits) = body {
            return !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
        }
    }
    matches!(
        provenance,
        "S00 plan.md node row"
            | "S00 plan.md ordering boundaries"
            | "S00 plan.md programme shape"
            | "#11681 dependency graph"
    )
}

fn edge_laws(
    node: &super::model::TrainNode,
    by_id: &BTreeMap<String, usize>,
    manifest: &Manifest,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let subject = format!("node {}", node.node_id);
    let registry: BTreeSet<&str> =
        manifest.external_authorities.iter().map(|a| a.id.as_str()).collect();
    let mut seen_targets: BTreeSet<&str> = BTreeSet::new();

    for dep in &node.dependencies {
        let edge_subject = format!("{subject} edge -> {}", dep.target);
        if !DEP_CLASSES.contains(&dep.class.as_str()) {
            diagnostics.push(Diagnostic::new(
                "edge-class",
                format!("{edge_subject} (class='{}')", dep.class),
                format!(
                    "dependency class '{}' collapses the typed hard/evidence/optional/external \
                     vocabulary; a generic 'depends_on' erases evidence-stage distinctions",
                    dep.class
                ),
            ));
        }
        if !provenance_is_well_formed(&dep.provenance) {
            diagnostics.push(Diagnostic::new(
                "edge-class",
                format!("{edge_subject}.provenance"),
                format!("unrecognized provenance '{}'", dep.provenance),
            ));
        }
        if dep.provenance.trim().is_empty() {
            diagnostics.push(Diagnostic::new(
                "edge-class",
                format!("{edge_subject}.provenance"),
                "provenance must be non-empty".to_owned(),
            ));
        }
        if !seen_targets.insert(dep.target.as_str()) {
            diagnostics.push(Diagnostic::new(
                "edge-identity",
                edge_subject.clone(),
                "more than one dependency edge to the same target: conflicting identities, not a \
                     richer contract"
                    .to_owned(),
            ));
        }
        if dep.target.starts_with('#') {
            if !registry.contains(dep.target.as_str()) {
                diagnostics.push(Diagnostic::new(
                    "authorities",
                    edge_subject.clone(),
                    format!("depends on unknown external authority '{}'", dep.target),
                ));
            }
        } else {
            match by_id.get(dep.target.as_str()) {
                Some(_) => {}
                None => diagnostics.push(Diagnostic::new(
                    "edge-target",
                    edge_subject.clone(),
                    "depends on unknown node".to_owned(),
                )),
            }
            if dep.target == node.node_id {
                diagnostics.push(Diagnostic::new(
                    "edge-target",
                    edge_subject,
                    "self-dependency".to_owned(),
                ));
            }
        }
    }
}

fn role_assignability_laws(manifest: &Manifest, diagnostics: &mut Vec<Diagnostic>) {
    for node in &manifest.nodes {
        let subject = format!("node {} (role={})", node.node_id, node.train_role);
        match node.train_role.as_str() {
            "controller" | "fan_in" | "external_gate" => {
                if node.buildable {
                    diagnostics.push(Diagnostic::new(
                        "assignability",
                        subject.clone(),
                        format!(
                            "{} nodes are not buildable as ordinary whole-product PRs; they never \
                             enter builder frontier eligibility",
                            node.train_role
                        ),
                    ));
                }
            }
            _ => {
                if !node.buildable {
                    diagnostics.push(Diagnostic::new(
                        "assignability",
                        subject.clone(),
                        "ordinary nodes are exactly one reviewable one-PR proposition and must be \
                             buildable"
                            .to_owned(),
                    ));
                }
            }
        }
        if matches!(node.train_role.as_str(), "proof" | "fan_in")
            && !node.claim_ceiling.contains("repair")
        {
            diagnostics.push(Diagnostic::new(
                "assignability",
                subject,
                "proof/fan-in nodes may report and transfer product defects but never repair \
                     them; the claim ceiling must bound repair authority"
                    .to_owned(),
            ));
        }
    }
    let ctrl = manifest.nodes.iter().find(|n| n.node_id == "CTRL");
    match ctrl {
        Some(node) if node.train_role == "controller" => {}
        Some(node) => diagnostics.push(Diagnostic::new(
            "assignability",
            "node CTRL".to_owned(),
            format!("CTRL must carry train role 'controller', found '{}'", node.train_role),
        )),
        None => diagnostics.push(Diagnostic::new(
            "assignability",
            "node CTRL".to_owned(),
            "the programme controller node CTRL is missing".to_owned(),
        )),
    }

    let r05b = manifest.nodes.iter().find(|n| n.node_id == "R05B");
    if let Some(r05b) = r05b {
        let auth_edges = r05b
            .dependencies
            .iter()
            .filter(|dep| dep.target == "#EXPLICIT-AUTHORIZATION" && dep.class == "external")
            .count();
        if auth_edges != 1 {
            diagnostics.push(Diagnostic::new(
                "assignability",
                "node R05B".to_owned(),
                format!(
                    "the external gate must carry exactly one external #EXPLICIT-AUTHORIZATION \
                     dependency, found {auth_edges}; authorization is never inferred"
                ),
            ));
        }
    } else {
        diagnostics.push(Diagnostic::new(
            "assignability",
            "node R05B".to_owned(),
            "the external-gate node R05B is missing from the functional rail".to_owned(),
        ));
    }

    let missing_heuristic_exit = manifest
        .nodes
        .iter()
        .find(|n| n.node_id == "I01")
        .is_some_and(|i01| !i01.exits.old_path.contains("heuristic"));
    if missing_heuristic_exit {
        diagnostics.push(Diagnostic::new(
            "exits",
            "node I01.exits.old_path".to_owned(),
            "the generic entry cutover must keep an explicit old-heuristic retirement exit"
                .to_owned(),
        ));
    }
}

fn writer_parallelism_laws(manifest: &Manifest, diagnostics: &mut Vec<Diagnostic>) {
    for node in &manifest.nodes {
        let subject = format!("node {}", node.node_id);
        if node.writer.stack_relation != "none" && node.writer.parallel_group != "none" {
            diagnostics.push(Diagnostic::new(
                "parallelism",
                format!("{subject}.writer"),
                "explicit stack edges and parallel groups are distinct planes: a stacked node \
                     cannot simultaneously advertise parallel safety"
                    .to_owned(),
            ));
        }
        if node.writer.parallel_group == "none" {
            continue;
        }
        let Some(gate) = node
            .writer
            .parallel_group
            .strip_prefix("post-")
            .and_then(|rest| rest.strip_suffix("-parallel"))
        else {
            diagnostics.push(Diagnostic::new(
                "parallelism",
                format!("{subject}.writer.parallel_group"),
                format!(
                    "parallel group '{}' must be named 'post-<GATE>-parallel' after its common \
                     hard gate",
                    node.writer.parallel_group
                ),
            ));
            continue;
        };
        let hard_on_gate =
            node.dependencies.iter().any(|dep| dep.target == gate && dep.class == "hard");
        if !hard_on_gate {
            diagnostics.push(Diagnostic::new(
                "parallelism",
                format!("{subject}.writer.parallel_group"),
                format!(
                    "parallel group '{}' requires a hard dependency on gate '{gate}'; nodes are \
                     not parallel-safe merely by path non-overlap",
                    node.writer.parallel_group
                ),
            ));
        }
    }

    // Members of one parallel group must be mutually independent for
    // implementation: a hard edge between them forces serial execution
    // inside an advertised parallel group (falsifier: disjoint writers
    // forced serial only by issue order). Evidence/optional inputs may flow
    // between parallel members without serializing their implementation.
    let groups: BTreeMap<&str, Vec<&str>> = {
        let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for node in &manifest.nodes {
            if node.writer.parallel_group != "none" {
                groups
                    .entry(node.writer.parallel_group.as_str())
                    .or_default()
                    .push(node.node_id.as_str());
            }
        }
        groups
    };
    for (group, members) in groups {
        for left in &members {
            for right in &members {
                if left == right {
                    continue;
                }
                let serial = manifest.nodes.iter().find(|n| n.node_id == *right).is_some_and(|n| {
                    n.dependencies.iter().any(|dep| dep.target == *left && dep.class == "hard")
                });
                if serial {
                    diagnostics.push(Diagnostic::new(
                        "parallelism",
                        format!("group {group}: {right} -> {left}"),
                        format!(
                            "a hard dependency edge between two members of parallel group \
                             '{group}' forces them serial despite disjoint writers; issue order \
                             is not a semantic dependency"
                        ),
                    ));
                }
            }
        }
    }
}

fn successor_derivation_laws(manifest: &Manifest, diagnostics: &mut Vec<Diagnostic>) {
    let mut derived: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for node in &manifest.nodes {
        derived.entry(node.node_id.as_str()).or_default();
    }
    for node in &manifest.nodes {
        for dep in &node.dependencies {
            if !dep.target.starts_with('#') {
                derived.entry(dep.target.as_str()).or_default().insert(node.node_id.clone());
            }
        }
    }
    for node in &manifest.nodes {
        // Compare as sorted unique sets: presentation order is not semantics.
        let mut actual = node.successors.clone();
        actual.sort();
        actual.dedup();
        let derived_set: Vec<String> =
            derived.get(node.node_id.as_str()).cloned().unwrap_or_default().into_iter().collect();
        if actual != derived_set {
            diagnostics.push(Diagnostic::new(
                "successors",
                format!("node {}", node.node_id),
                format!(
                    "successor set must be exactly the derived reverse-edge set: actual=[{}] \
                     derived=[{}]",
                    node.successors.join(","),
                    derived_set.join(",")
                ),
            ));
        }
    }
}

fn law_edge_laws(
    manifest: &Manifest,
    by_id: &BTreeMap<String, usize>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (source, target, class) in LAW_EDGES {
        let Some(&index) = by_id.get(*target) else {
            // The missing node itself is already reported by the node-set law.
            continue;
        };
        let node = &manifest.nodes[index];
        let matched = node
            .dependencies
            .iter()
            .filter(|dep| dep.target == *source)
            .filter(|dep| dep.class == *class)
            .count();
        if matched != 1 {
            let actual_class = node
                .dependencies
                .iter()
                .find(|dep| dep.target == *source)
                .map(|dep| dep.class.as_str())
                .unwrap_or("<missing>");
            diagnostics.push(Diagnostic::new(
                "law-edge",
                format!("edge {source} -> {target}"),
                format!(
                    "frozen graph-law edge must be exactly '{class}', found '{actual_class}'; \
                     substituting an evidence/optional stage for a hard prerequisite (or the \
                     reverse) is an evidence-stage substitution"
                ),
            ));
        }
    }
}

fn acyclicity_laws(
    manifest: &Manifest,
    by_id: &BTreeMap<String, usize>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Depth-first colouring over hard/evidence edges. 0 = unvisited,
    // 1 = on stack, 2 = done. The node set is small and frozen, so the
    // recursion depth is bounded by the node count.
    let mut colour: BTreeMap<&str, u8> = BTreeMap::new();
    for node in &manifest.nodes {
        colour.insert(node.node_id.as_str(), 0);
    }
    let mut reported: BTreeSet<String> = BTreeSet::new();

    fn visit<'a>(
        current: &'a str,
        manifest: &'a Manifest,
        by_id: &BTreeMap<String, usize>,
        colour: &mut BTreeMap<&'a str, u8>,
        reported: &mut BTreeSet<String>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        match colour.get(current).copied().unwrap_or(2) {
            1 => {
                if !reported.contains(current) {
                    reported.insert(current.to_owned());
                    diagnostics.push(Diagnostic::new(
                        "cycle",
                        format!("edge {current} -> ... -> {current}"),
                        "dependency cycle over hard/evidence edges: an impossible dependency \
                             ordering"
                            .to_owned(),
                    ));
                }
            }
            2 => {}
            _ => {
                colour.insert(current, 1);
                let Some(&index) = by_id.get(current) else { return };
                for dep in &manifest.nodes[index].dependencies {
                    if dep.target.starts_with('#') {
                        continue;
                    }
                    if !matches!(dep.class.as_str(), "hard" | "evidence") {
                        continue;
                    }
                    visit(&dep.target, manifest, by_id, colour, reported, diagnostics);
                }
                colour.insert(current, 2);
            }
        }
    }

    for node in &manifest.nodes {
        visit(&node.node_id, manifest, by_id, &mut colour, &mut reported, diagnostics);
    }
}

fn orphan_and_route_laws(manifest: &Manifest, diagnostics: &mut Vec<Diagnostic>) {
    // Orphans: no incoming and no outgoing node edges (the controller root is
    // the parenting root and is exempt).
    for node in &manifest.nodes {
        if node.node_id == "CTRL" {
            continue;
        }
        let has_deps = node.dependencies.iter().any(|dep| !dep.target.starts_with('#'));
        let has_successors = !node.successors.is_empty();
        if !has_deps && !has_successors {
            diagnostics.push(Diagnostic::new(
                "orphan",
                format!("node {}", node.node_id),
                format!(
                    "orphaned {} node: no dependency and no successor connects it to the train",
                    node.train_role
                ),
            ));
        }
    }

    // Terminal route: exactly one fan_in closeout node with no successors;
    // every node except the controller root must reach it over
    // hard/evidence/optional forward edges.
    let fan_ins: Vec<&str> = manifest
        .nodes
        .iter()
        .filter(|n| n.train_role == "fan_in")
        .map(|n| n.node_id.as_str())
        .collect();
    if fan_ins.len() != 1 {
        diagnostics.push(Diagnostic::new(
            "terminal",
            "fan_in nodes".to_owned(),
            format!(
                "the train expects exactly one terminal fan-in closeout node, found [{}]",
                fan_ins.join(",")
            ),
        ));
        return;
    }
    let terminal = fan_ins[0];
    let Some(terminal_node) = manifest.nodes.iter().find(|n| n.node_id == terminal) else {
        return;
    };
    if !terminal_node.successors.is_empty() {
        diagnostics.push(Diagnostic::new(
            "terminal",
            format!("node {terminal}"),
            "the terminal fan-in closeout node has successors".to_owned(),
        ));
    }

    // Forward reachability to the terminal: a node reaches the fan-in when
    // one of its successors (nodes that depend on it) is already known to
    // reach it, over hard/evidence/optional edges.
    let mut reach: BTreeSet<&str> = BTreeSet::new();
    reach.insert(terminal);
    let mut changed = true;
    while changed {
        changed = false;
        for node in &manifest.nodes {
            if reach.contains(node.node_id.as_str()) {
                continue;
            }
            let reaches_terminal =
                node.successors.iter().any(|successor| reach.contains(successor.as_str()));
            if reaches_terminal {
                reach.insert(node.node_id.as_str());
                changed = true;
            }
        }
    }
    for node in &manifest.nodes {
        if node.node_id == "CTRL" || node.node_id == terminal {
            continue;
        }
        if !reach.contains(node.node_id.as_str()) {
            diagnostics.push(Diagnostic::new(
                "terminal",
                format!("node {}", node.node_id),
                format!(
                    "no route to the terminal fan-in closeout '{terminal}' over \
                     hard/evidence/optional edges"
                ),
            ));
        }
    }
}

/// Render a report for humans: every diagnostic plus a stable pass line.
pub fn render_report(report: &StaticReport) -> Result<String> {
    if !report.is_valid() {
        let mut out = String::new();
        for diagnostic in &report.diagnostics {
            out.push_str(&diagnostic.render());
            out.push('\n');
        }
        out.push_str(&format!(
            "ISSUE_CONTROLLER_TRAIN_STATIC_CHECK=FAIL diagnostics={}\n",
            report.diagnostics.len()
        ));
        return Ok(out);
    }
    let digest = report
        .semantic_digest
        .as_deref()
        .ok_or_else(|| color_eyre::eyre::eyre!("valid manifest must carry a semantic digest"))?;
    let summary = report
        .manifest
        .as_ref()
        .map(GraphSummary::of)
        .ok_or_else(|| color_eyre::eyre::eyre!("valid report must carry the typed manifest"))?;
    let mut line = format!(
        "ISSUE_CONTROLLER_TRAIN_STATIC_CHECK=PASS nodes={} edges={} semantic_sha256={digest}",
        summary.node_count, summary.edge_count
    );
    let classes: Vec<String> =
        summary.class_counts.iter().map(|(k, v)| format!("{k}:{v}")).collect();
    line.push_str(&format!(" classes={}", classes.join(",")));
    Ok(line)
}

/// Convenience for callers that need a hard failure on invalid manifests.
pub fn require_valid(report: &StaticReport) -> Result<(&Manifest, &str)> {
    if !report.is_valid() {
        for diagnostic in &report.diagnostics {
            eprintln!("{}", diagnostic.render());
        }
        bail!(
            "issue_controller_train.v1 manifest failed static validation with {} diagnostic(s); \
             failing closed — no projection is emitted",
            report.diagnostics.len()
        );
    }
    let manifest = report
        .manifest
        .as_ref()
        .ok_or_else(|| color_eyre::eyre::eyre!("valid report must carry the typed manifest"))?;
    let digest = report
        .semantic_digest
        .as_deref()
        .ok_or_else(|| color_eyre::eyre::eyre!("valid report must carry the semantic digest"))?;
    Ok((manifest, digest))
}
