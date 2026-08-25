//! Fail-closed corpus invariant checks (#11649 first-falsifier list).
//!
//! Every violation is collected and reported together so reviewers see the
//! whole broken surface instead of only the first defect.

use super::model::{
    AuthorityKind, AuthorityStatus, CORPUS_NAME, CorpusManifest, LoadedCases, RepairFalsifierCase,
    SCHEMA_VERSION,
};
use color_eyre::eyre::{Result, WrapErr, bail};
use regex::Regex;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

pub(crate) const FIXTURE_DIR: &str = "fixtures/clippy_repair_falsifiers";
const MANIFEST_FILE: &str = "manifest.v1.json";
const TOOLCHAIN_FILE: &str = "rust-toolchain.toml";
const GATE_POLICY_FILE: &str = ".ci/gate-policy.yaml";
const CARGO_MANIFEST: &str = "Cargo.toml";
const LINT_FRAGMENT_DIR: &str = "policy/clippy-lints.d";

/// The frozen denominator. A deleted case breaks validation instead of making
/// a downstream readiness check green (#11649 falsifier 10).
pub(crate) const REQUIRED_CASE_IDS: &[&str] = &[
    "A01-file-wide-suppression-carveout",
    "A02-dead-code-baseline-absorption",
    "A03-cfg-test-attr-general-carveout",
    "A04-exact-lint-group-substitution",
    "A05-command-missing-docs-reintroduction",
    "A06-required-target-omission",
    "A07-required-feature-profile-reduction",
    "A08-platform-substitution-linux-for-hosted",
    "A09-zero-work-or-malformed-as-success",
    "A10-candidate-refresh-baseline-absorption",
    "B11-same-total-finding-swap",
    "B12-accepted-finding-copy-to-other-path",
    "B13-consumed-finding-identity-reintroduction",
    "B14-suppression-displacement-count-equal",
    "B15-stale-cross-toolchain-receipt",
    "B16-open-world-item-as-closed-cleanup",
    "C17-ok-erasure-of-result",
    "C18-let-underscore-must-use-discard",
    "C19-uncontracted-underscore-binding",
    "C20-log-only-error-consumption",
    "C21-panic-assertion-flow-substitution",
    "C22-redaction-weakening-for-cause-retention",
    "C23-renamed-error-variable-still-ignored",
    "D24-unchecked-byte-slicing-swap",
    "D25-get-unwrap-indexing-substitution",
    "D26-clamp-default-range-semantics-swap",
    "D27-ascii-only-unicode-evidence",
    "D28-numeric-helper-semantics-drift",
    "D29-atomic-mutex-substitution-unproved",
    "D30-await-structure-change-unproofed",
    "D31-unsafe-boundary-widening-unowned",
    "E32-parameter-bag-without-owner",
    "E33-type-alias-only-hiding",
    "E34-trampoline-ordering-split",
    "E35-ownership-theater-clone-wrapper",
    "E36-invariant-free-accessor",
    "E37-api-shape-change-as-compliance",
    "E38-generated-output-edited-generator-stale",
    "F39-lib-only-helper-deletion",
    "F40-default-feature-import-deletion",
    "F41-auto-suggestion-reexport-deletion",
    "F42-unbounded-clippy-fix-scope",
    "F43-machine-applicable-crossing-authorities",
    "F44-malformed-suggestion-auto-application",
    "G45-restating-documentation",
    "G46-invented-guarantee-documentation",
    "G47-test-proof-weakening-for-green",
    "G48-cargo-feature-surface-compliance-change",
    "G49-dependency-upgrade-as-duplicate-fix",
    "G50-private-evidence-for-product-package",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CorpusReport {
    pub(crate) case_count: usize,
    pub(crate) bound_count: usize,
    pub(crate) pending_count: usize,
}

struct CorpusContext {
    repo_root: std::path::PathBuf,
    gate_names: BTreeSet<String>,
    lint_fragment_text: String,
    workspace_lint_names: BTreeSet<String>,
}

impl CorpusContext {
    fn load(repo_root: &Path) -> Result<Self> {
        let gate_policy = read_repo_file(repo_root, GATE_POLICY_FILE)?;
        let gate_names = parse_gate_names(&gate_policy)
            .wrap_err_with(|| format!("parsing gate names from {GATE_POLICY_FILE}"))?;
        let mut lint_fragment_text = String::new();
        let fragment_dir = repo_root.join(LINT_FRAGMENT_DIR);
        let mut fragments: Vec<_> = fs::read_dir(&fragment_dir)
            .wrap_err_with(|| format!("reading {}", fragment_dir.display()))?
            .collect::<std::io::Result<Vec<_>>>()
            .wrap_err_with(|| format!("listing {}", fragment_dir.display()))?;
        fragments.sort_by_key(|entry| entry.file_name());
        for fragment in fragments {
            let path = fragment.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            lint_fragment_text.push_str(
                &fs::read_to_string(&path).wrap_err_with(|| {
                    format!("reading lint catalog fragment {}", path.display())
                })?,
            );
            lint_fragment_text.push('\n');
        }
        let cargo = read_repo_file(repo_root, CARGO_MANIFEST)?;
        let workspace_lint_names = parse_workspace_lint_names(&cargo)
            .wrap_err_with(|| format!("parsing workspace lints from {CARGO_MANIFEST}"))?;
        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            gate_names,
            lint_fragment_text,
            workspace_lint_names,
        })
    }

    fn lint_is_governed(&self, qualified_name: &str) -> bool {
        let quoted = format!("\"{qualified_name}\"");
        if self.lint_fragment_text.contains(&quoted) {
            return true;
        }
        let bare = qualified_name.rsplit("::").next().unwrap_or(qualified_name);
        self.workspace_lint_names.contains(bare)
    }

    fn gate_exists(&self, gate: &str) -> bool {
        self.gate_names.contains(gate)
    }
}

fn read_repo_file(repo_root: &Path, relative: &str) -> Result<String> {
    fs::read_to_string(repo_root.join(relative))
        .wrap_err_with(|| format!("reading repository authority file {relative}"))
}

fn parse_gate_names(raw: &str) -> Result<BTreeSet<String>> {
    #[derive(Deserialize)]
    struct GateEntry {
        name: String,
    }
    let value: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(raw).wrap_err("parsing gate policy yaml")?;
    let mut names = BTreeSet::new();
    if let Some(gates) = value.get("gates").and_then(|gates| gates.as_sequence()) {
        for gate in gates {
            let parsed: GateEntry = serde_yaml_ng::from_value(gate.clone())
                .wrap_err("parsing one gate-policy entry")?;
            names.insert(parsed.name);
        }
    }
    Ok(names)
}

/// Collect the exact lint names configured under `[workspace.lints.*]` so
/// governed-lint resolution reads every tool section (not just the first) and
/// can never match a partial or commented name.
fn parse_workspace_lint_names(cargo_toml: &str) -> Result<BTreeSet<String>> {
    let value: toml::Value = toml::from_str(cargo_toml).wrap_err("parsing Cargo.toml")?;
    let mut names = BTreeSet::new();
    let Some(lints) = value
        .get("workspace")
        .and_then(|workspace| workspace.get("lints"))
        .and_then(|l| l.as_table())
    else {
        return Ok(names);
    };
    for (tool, entries) in lints {
        let Some(entries) = entries.as_table() else {
            bail!("{CARGO_MANIFEST}: [workspace.lints.{tool}] is not a table");
        };
        for name in entries.keys() {
            names.insert(name.clone());
        }
    }
    Ok(names)
}

fn sha256_hex(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    digest.iter().fold(String::new(), |mut out, byte| {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn looks_like_mutable_identity(field: &str, value: &str) -> Option<String> {
    // Any long embedded hex run is commit/artifact identity, never case identity.
    let mut run = 0usize;
    for c in value.chars() {
        if c.is_ascii_hexdigit() {
            run += 1;
        } else {
            if run >= 32 {
                return Some(format!("{field} embeds a raw long hash as identity"));
            }
            run = 0;
        }
    }
    if run >= 32 {
        return Some(format!("{field} embeds a raw long hash as identity"));
    }
    for banned in ["refs/", "origin/", "remotes/", "/heads/", "localhost"] {
        if value.contains(banned) {
            return Some(format!("{field} embeds mutable location token `{banned}`"));
        }
    }
    None
}

fn push_evidence(
    violations: &mut Vec<String>,
    case_id: &str,
    label: &str,
    needles: &[String],
    haystack: &str,
    must_contain: bool,
) {
    for needle in needles {
        if needle.is_empty() {
            violations.push(format!("{case_id}: empty {label} entry"));
            continue;
        }
        let present = haystack.contains(needle.as_str());
        if must_contain && !present {
            violations.push(format!(
                "{case_id}: {label} evidence `{needle}` absent from its own content"
            ));
        }
        if !must_contain && present {
            violations.push(format!(
                "{case_id}: sanctioned control evidence `{needle}` leaked into dishonest mutation"
            ));
        }
    }
}

fn validate_case(case: &RepairFalsifierCase, ctx: &CorpusContext) -> Vec<String> {
    let mut violations = Vec::new();
    let id = &case.case_id;

    // F1: stable identifier shape agreeing with the declared family.
    if !stable_case_id_ok(id) {
        violations.push(format!("{id}: case ID violates the stable-id grammar"));
    }
    let prefix = id.chars().next();
    if prefix != Some(case.family.id_prefix()) {
        violations.push(format!("{id}: ID prefix disagrees with family {:?}", case.family));
    }
    if case.purpose.trim().len() < 20 {
        violations.push(format!("{id}: purpose too short to carry meaning"));
    }
    if case.claim_boundary.trim().len() < 20 {
        violations.push(format!("{id}: claim_boundary missing"));
    }

    // F8: mutable state can never be stable case identity.
    for (field, value) in [
        ("case_id", id.as_str()),
        ("purpose", case.purpose.as_str()),
        ("claim_boundary", case.claim_boundary.as_str()),
    ] {
        if let Some(reason) = looks_like_mutable_identity(field, value) {
            violations.push(format!("{id}: {reason}"));
        }
    }

    // F2: recorded digests must match recomputed content digests.
    if sha256_hex(&case.dishonest_mutation.content) != case.dishonest_mutation.sha256 {
        violations.push(format!("{id}: mutation digest does not match mutation bytes"));
    }
    if sha256_hex(&case.positive_counterpart.content) != case.positive_counterpart.sha256 {
        violations.push(format!("{id}: counterpart digest does not match counterpart bytes"));
    }

    // F6: positive counterpart required, distinct, discriminating both ways.
    if case.dishonest_mutation.sha256 == case.positive_counterpart.sha256 {
        violations.push(format!("{id}: negative case lacks a distinct positive counterpart"));
    }
    if case.positive_counterpart.sanctioned_evidence.is_empty() {
        violations.push(format!("{id}: positive counterpart carries no sanctioned evidence"));
    }
    push_evidence(
        &mut violations,
        id,
        "mutation trigger",
        &case.dishonest_mutation.trigger_evidence,
        &case.dishonest_mutation.content,
        true,
    );
    push_evidence(
        &mut violations,
        id,
        "counterpart sanction",
        &case.positive_counterpart.sanctioned_evidence,
        &case.positive_counterpart.content,
        true,
    );
    push_evidence(
        &mut violations,
        id,
        "control leak",
        &case.positive_counterpart.sanctioned_evidence,
        &case.dishonest_mutation.content,
        false,
    );

    // Governed-lint references must resolve to current authorities.
    if let Some(lint_ref) = &case.governed_lint
        && !ctx.lint_is_governed(&lint_ref.lint)
    {
        violations.push(format!(
            "{id}: governed lint {} is absent from Cargo workspace lints and the policy catalog",
            lint_ref.lint
        ));
    }

    // Authority status laws: bound references resolve on main; pending owners
    // name a real issue and stay non-packet-ready (no fabricated bindings).
    match &case.rejecting_authority {
        AuthorityStatus::Bound { authority_kind, reference } => {
            if let Some(reason) = looks_like_mutable_identity("authority.reference", reference) {
                violations.push(format!("{id}: {reason}"));
            }
            if let Err(message) =
                resolve_authority(*authority_kind, reference, ctx, repo_relative_ban(id, reference))
            {
                violations.push(format!("{id}: {message}"));
            }
        }
        AuthorityStatus::PendingOwner { owner_issue, unresolved_reason } => {
            if *owner_issue == 0 {
                violations.push(format!("{id}: pending authority lacks a owning issue"));
            }
            if unresolved_reason.trim().len() < 20 {
                violations
                    .push(format!("{id}: pending authority lacks a precise unresolved-reason"));
            }
        }
    }

    if case.delta_provenance.change_reason.trim().is_empty()
        || case.delta_provenance.owning_issue == 0
    {
        violations
            .push(format!("{id}: delta provenance must name a change reason and owning issue"));
    }
    if case.applicability.packet_classes.is_empty() || case.applicability.domains.is_empty() {
        violations.push(format!("{id}: applicability must name packet classes and domains"));
    }
    if case.schema_version != SCHEMA_VERSION {
        violations.push(format!("{}: unsupported case schema version {}", id, case.schema_version));
    }

    violations
}

/// The corpus may never cite itself as its own rejecting authority (falsifier 5:
/// expected results generated from the implementation under test).
fn repo_relative_ban(case_id: &str, reference: &str) -> Option<String> {
    let lower = reference.to_ascii_lowercase();
    for banned in ["fixtures/clippy_repair_falsifiers", "schemas/clippy_repair_falsifiers"] {
        if lower.contains(banned) {
            return Some(format!("{case_id}: authority reference cites the corpus itself"));
        }
    }
    None
}

fn resolve_authority(
    kind: AuthorityKind,
    reference: &str,
    ctx: &CorpusContext,
    self_reference: Option<String>,
) -> std::result::Result<(), String> {
    if let Some(message) = self_reference {
        return Err(message);
    }
    match kind {
        AuthorityKind::CargoLints => {
            let name = reference.strip_prefix("lint:").ok_or_else(|| {
                format!("cargo-lints authority `{reference}` must start with `lint:`")
            })?;
            if !ctx.lint_is_governed(name) {
                return Err(format!(
                    "cargo-lints authority `{name}` resolves to no current workspace lint or catalog row"
                ));
            }
            Ok(())
        }
        AuthorityKind::GateCommand => {
            let gate = reference.strip_prefix("gate:").ok_or_else(|| {
                format!("gate-command authority `{reference}` must start with `gate:`")
            })?;
            if !ctx.gate_exists(gate) {
                return Err(format!(
                    "gate-command authority `{gate}` names no gate in {GATE_POLICY_FILE}"
                ));
            }
            Ok(())
        }
        AuthorityKind::FileContract | AuthorityKind::ReceiptContract => {
            let (path_part, needle) = split_symbol_reference(reference)?;
            if needle.is_empty() && kind == AuthorityKind::ReceiptContract {
                return Err(format!("receipt-contract authority `{reference}` must pin a symbol"));
            }
            let absolute = ctx.repo_root.join(path_part);
            if !absolute.is_file() {
                return Err(format!(
                    "authority file `{path_part}` is absent from the working tree"
                ));
            }
            if !needle.is_empty() {
                let text = fs::read_to_string(&absolute)
                    .map_err(|error| format!("reading authority file `{path_part}`: {error}"))?;
                if !text.contains(needle) {
                    return Err(format!(
                        "authority file `{path_part}` no longer contains pinned text `{needle}`"
                    ));
                }
            }
            Ok(())
        }
    }
}

fn split_symbol_reference(reference: &str) -> std::result::Result<(&str, &str), String> {
    let payload = reference.split_once(':').map_or(reference, |(_, rest)| rest);
    match payload.split_once('#') {
        Some((path, symbol)) => Ok((path, symbol)),
        None => Ok((payload, "")),
    }
}

/// The stable case-id grammar, mirroring the schema pattern
/// `^[A-G][0-9]{2}-[a-z0-9]+(-[a-z0-9]+)*$` from
/// schemas/clippy_repair_falsifiers.v1.schema.json so the validator and the
/// schema cannot drift apart.
#[allow(clippy::expect_used, reason = "static LazyLock regex with known-good pattern")]
static STABLE_CASE_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-G][0-9]{2}-[a-z0-9]+(-[a-z0-9]+)*$")
        .expect("static case-id grammar regex is valid")
});

fn stable_case_id_ok(id: &str) -> bool {
    STABLE_CASE_ID.is_match(id)
}

/// Validate the checked-in corpus against every structural invariant.
pub(crate) fn validate_corpus(repo_root: &Path) -> Result<CorpusReport> {
    let fixture_dir = repo_root.join(FIXTURE_DIR);
    if !fixture_dir.is_dir() {
        bail!("corpus fixture directory {} is missing", fixture_dir.display());
    }
    let manifest_path = fixture_dir.join(MANIFEST_FILE);
    let raw_manifest = fs::read_to_string(&manifest_path)
        .wrap_err_with(|| format!("reading {}", manifest_path.display()))?;
    let manifest: CorpusManifest = super::parse_json(&raw_manifest, MANIFEST_FILE)?;

    let mut violations = Vec::new();

    // Manifest header invariants.
    if manifest.schema_version != SCHEMA_VERSION || manifest.corpus != CORPUS_NAME {
        violations.push(format!(
            "manifest identity mismatch: {} v{} ({})",
            manifest.corpus, manifest.schema_version, CORPUS_NAME
        ));
    }
    if manifest.case_count != manifest.cases.len() {
        violations.push(format!(
            "manifest case_count {} disagrees with {} entries",
            manifest.case_count,
            manifest.cases.len()
        ));
    }
    let toolchain_raw = read_repo_file(repo_root, TOOLCHAIN_FILE)?;
    if !toolchain_raw.contains(manifest.producer_toolchain.trim()) {
        violations.push(format!(
            "producer_toolchain {} disagrees with {}",
            manifest.producer_toolchain, TOOLCHAIN_FILE
        ));
    }

    let ctx = CorpusContext::load(repo_root)?;
    let loaded = load_cases(repo_root, &manifest, &mut violations);

    // Stable-identity law applies to every manifest entry even when its case
    // document cannot load, so mutable hashes can never masquerade as identity.
    for entry in &manifest.cases {
        if !stable_case_id_ok(&entry.case_id) {
            violations.push(format!("{}: case ID violates the stable-id grammar", entry.case_id));
        }
        if let Some(reason) = looks_like_mutable_identity("case_id", &entry.case_id) {
            violations.push(format!("{}: {reason}", entry.case_id));
        }
    }

    // F3/F10: denominator equality in both directions.
    let required: BTreeSet<&str> = REQUIRED_CASE_IDS.iter().copied().collect();
    let present: BTreeSet<&str> = loaded.keys().map(String::as_str).collect();
    for missing in required.difference(&present) {
        violations.push(format!("required corpus case {missing} is missing"));
    }
    for unexpected in present.difference(&required) {
        violations.push(format!(
            "corpus case {unexpected} is not part of the frozen denominator; \
             extend REQUIRED_CASE_IDS through a reviewed delta"
        ));
    }

    // Manifest ordering is deterministic so semantic output cannot depend on
    // case order (update-law determinism clause).
    let ids: Vec<&str> = manifest.cases.iter().map(|entry| entry.case_id.as_str()).collect();
    if !ids.windows(2).all(|pair| pair[0] < pair[1]) {
        violations.push("manifest entries are not strictly ordered by case_id".to_owned());
    }

    // Per-case structural + authority invariants, plus cross-case uniqueness.
    let mut reason_codes: BTreeMap<String, String> = BTreeMap::new();
    let mut bound_count = 0usize;
    let mut pending_count = 0usize;
    for (case_id, case) in &loaded {
        violations.extend(validate_case(case, &ctx));
        let code = format!("{:?}", case.expected_result.reason_code);
        if let Some(previous) = reason_codes.insert(code.clone(), case_id.clone()) {
            violations.push(format!(
                "{case_id}: reason code {code} already owned by {previous}; \
                 a copied mechanism needs a reviewed new delta, not a drifting copy"
            ));
        }
        match &case.rejecting_authority {
            AuthorityStatus::Bound { .. } => bound_count += 1,
            AuthorityStatus::PendingOwner { .. } => pending_count += 1,
        }
        let ready = manifest
            .cases
            .iter()
            .find(|entry| &entry.case_id == case_id)
            .map(|entry| entry.packet_ready)
            .unwrap_or_default();
        let should_be_ready = case.rejecting_authority.is_bound();
        if ready != should_be_ready {
            violations.push(format!(
                "{case_id}: manifest packet_ready={ready} contradicts authority binding (expected {should_be_ready})"
            ));
        }
    }

    if violations.is_empty() {
        Ok(CorpusReport { case_count: loaded.len(), bound_count, pending_count })
    } else {
        violations.sort();
        violations.dedup();
        bail!(
            "clippy repair falsifier corpus failed {} invariant(s):\n  - {}",
            violations.len(),
            violations.join("\n  - ")
        )
    }
}

fn load_cases(
    repo_root: &Path,
    manifest: &CorpusManifest,
    violations: &mut Vec<String>,
) -> LoadedCases {
    let mut cases = BTreeMap::new();
    let mut seen_ids: BTreeSet<&str> = BTreeSet::new();
    for entry in &manifest.cases {
        if !seen_ids.insert(entry.case_id.as_str()) {
            violations.push(format!("duplicate case id {} in manifest", entry.case_id));
            continue;
        }
        let expected_file = format!("cases/{}.json", entry.case_id);
        if entry.file != expected_file {
            violations.push(format!(
                "{}: manifest file `{}` does not follow the canonical layout",
                entry.case_id, entry.file
            ));
            continue;
        }
        let path = repo_root.join(FIXTURE_DIR).join(&entry.file);
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) => {
                violations.push(format!("{}: reading case file failed: {error}", entry.case_id));
                continue;
            }
        };
        match super::parse_json::<RepairFalsifierCase>(&raw, &entry.file) {
            Ok(case) => {
                if case.case_id != entry.case_id {
                    violations.push(format!(
                        "{}: embedded case_id disagrees with manifest entry",
                        entry.case_id
                    ));
                    continue;
                }
                if case.family != entry.family {
                    violations.push(format!(
                        "{}: embedded family disagrees with manifest entry",
                        entry.case_id
                    ));
                    continue;
                }
                cases.insert(entry.case_id.clone(), case);
            }
            Err(error) => violations
                .push(format!("{}: parsing case document failed: {error:#}", entry.case_id)),
        }
    }
    cases
}

/// Test-visible helper that mirrors [`validate_corpus`] but returns violations
/// without failing, so probes can assert designated error classes.
#[cfg(test)]
pub(crate) fn violations_for(repo_root: &Path) -> Result<Vec<String>> {
    let result = validate_corpus(repo_root);
    match result {
        Ok(_) => Ok(Vec::new()),
        Err(error) => Ok(vec![format!("{error:#}")]),
    }
}

#[cfg(test)]
mod stable_id_and_lint_parsing_tests {
    use super::{parse_workspace_lint_names, stable_case_id_ok};

    #[test]
    fn case_id_grammar_accepts_schema_valid_shapes() {
        for id in [
            "A01-file-wide-suppression-carveout",
            "G50-private-evidence-for-product-package",
            "B11-same-total-finding-swap",
            // Digits are legal inside the tail segments.
            "A01-v2-parsing",
        ] {
            assert!(stable_case_id_ok(id), "schema-valid id rejected: {id}");
        }
    }

    #[test]
    fn case_id_grammar_rejects_schema_invalid_shapes() {
        for id in [
            // digits not immediately after the family letter
            "A-01-file",
            // family letter outside A..=G
            "Z01-file",
            // three digits where exactly two are required
            "A011-file",
            // too few / missing digits
            "A1-file",
            "A01",
            // wrong case
            "a01-file",
            // dangling or doubled separators
            "A01-",
            "A01--x",
            "",
            "-A01-file",
        ] {
            assert!(!stable_case_id_ok(id), "schema-invalid id accepted: {id}");
        }
    }

    #[test]
    fn workspace_lint_names_cover_every_tool_section_exactly() {
        let cargo = r#"
[workspace.lints.clippy]
unwrap_used = "deny"
await_holding_lock = "deny"
collapsible_match = { level = "allow", priority = 1 }

[workspace.lints.rust]
unexpected_cfgs = { level = "warn" }

[package]
name = "probe"
"#;
        let names = parse_workspace_lint_names(cargo).expect("test toml parses");
        assert_eq!(
            names,
            ["await_holding_lock", "collapsible_match", "unexpected_cfgs", "unwrap_used"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        );
        // Partial-name substrings must never satisfy governance lookups.
        assert!(names.contains("unwrap_used"));
        assert!(!names.contains("wrap_use"));
    }

    #[test]
    fn workspace_lint_names_fail_closed_on_non_table_sections() {
        let cargo = "[workspace.lints]\nclippy = \"oops\"\n";
        assert!(parse_workspace_lint_names(cargo).is_err());
    }
}
