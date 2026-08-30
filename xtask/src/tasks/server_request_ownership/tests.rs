//! Proof for the server-request ownership matrix (#13223).
//!
//! The live matrix is the positive control; every other test names a plausible
//! wrong implementation and requires the checker to reject it. Each negative
//! control mutates exactly one field away from a passing row so a failure
//! localizes to the rule under test.

use super::check::check;
use super::discover::{parse_direction_registry, parse_feature_catalog, scan_emission};
use super::model::{CatalogRow, Discovered, Matrix, Meta, RegistryKind, RequestRow};
use super::{evaluate, fingerprint, load, render};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    match crate::utils::project_root() {
        Ok(root) => root,
        Err(error) => unreachable!("xtask always resolves a project root: {error}"),
    }
}

fn meta() -> Meta {
    Meta {
        schema: "server_request_ownership.v1".to_string(),
        owner_issue: 13223,
        direction_registry: "crates/perl-lsp-rs/src/protocol/method_direction.rs".to_string(),
        feature_catalog: "features.toml".to_string(),
        emission_scan_root: "crates/perl-lsp-rs/src/runtime".to_string(),
        allowed_protocol_baselines: vec!["stable_3_17".to_string(), "selected_3_18".to_string()],
        allowed_emission_states: vec!["emitted".to_string(), "not_emitted".to_string()],
        allowed_response_decoders: vec!["generic_shape".to_string(), "per_method".to_string()],
        allowed_dispositions: vec![
            "supported".to_string(),
            "advertised_not_proven".to_string(),
            "helper_only_unadvertised".to_string(),
            "not_proven".to_string(),
        ],
    }
}

/// One row that passes every rule, used as the base for single-field mutations.
fn passing_row() -> RequestRow {
    RequestRow {
        id: "srq-example".to_string(),
        method: "workspace/codeLens/refresh".to_string(),
        spec: "LSP 3.16".to_string(),
        protocol_baseline: "stable_3_17".to_string(),
        emission: "emitted".to_string(),
        emitters: vec![
            "crates/perl-lsp-rs/src/runtime/client_requests.rs#request_code_lens_refresh"
                .to_string(),
        ],
        feature_catalog_row: "lsp.code_lens_refresh".to_string(),
        capability_gate: "workspace.codeLens.refreshSupport".to_string(),
        capability_gate_owner: "#6735".to_string(),
        ux_default_response_owner: "missing:#13220".to_string(),
        programmable_actions_owner: "missing:#13221".to_string(),
        response_decoder: "generic_shape".to_string(),
        terminal_state_owner: "missing:#6724".to_string(),
        timeout_cleanup_policy: "server_request_registry_default".to_string(),
        exact_process_proof: "missing:#7016".to_string(),
        schema_evidence: "#7116".to_string(),
        disposition: "advertised_not_proven".to_string(),
        limitations: "Fire-and-forget.".to_string(),
    }
}

/// Discovery that agrees with [`passing_row`].
fn agreeing_discovery() -> Discovered {
    let mut registry = BTreeMap::new();
    registry.insert("workspace/codeLens/refresh".to_string(), RegistryKind::ServerToClientRequest);
    let mut emitted = BTreeMap::new();
    emitted.insert(
        "workspace/codeLens/refresh".to_string(),
        vec![
            "crates/perl-lsp-rs/src/runtime/client_requests.rs#request_code_lens_refresh"
                .to_string(),
        ],
    );
    let mut catalog_rows = BTreeMap::new();
    catalog_rows.insert("lsp.code_lens_refresh".to_string(), catalog("LSP 3.16", "workspace"));
    Discovered { registry, emitted, catalog_rows, ambiguous_symbols: BTreeSet::new() }
}

fn catalog(spec: &str, area: &str) -> CatalogRow {
    CatalogRow {
        spec: spec.to_string(),
        area: area.to_string(),
        advertised: true,
        maturity: "not_proven".to_string(),
        state_owner: "missing".to_string(),
    }
}

fn matrix_of(rows: Vec<RequestRow>) -> Matrix {
    Matrix { meta: meta(), request: rows }
}

fn rules(rows: Vec<RequestRow>, discovered: &Discovered) -> Vec<&'static str> {
    let matrix = matrix_of(rows);
    check(&repo_root(), &matrix, discovered, Vec::new())
        .into_iter()
        .map(|violation| violation.rule)
        .collect()
}

// ── Positive control ────────────────────────────────────────────────────

#[test]
fn synthetic_agreeing_row_has_no_findings() {
    assert!(
        rules(vec![passing_row()], &agreeing_discovery()).is_empty(),
        "the base row must pass so each mutation below isolates one rule"
    );
}

/// The claim itself: the committed matrix agrees with current `main`.
#[test]
fn live_matrix_matches_current_main() -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    let matrix = load(&root, Path::new("policy/server-request-ownership.v1.toml"))?;
    let violations = evaluate(&root, &matrix)?;
    assert!(
        violations.is_empty(),
        "live matrix drifted from current main:\n{}",
        violations
            .iter()
            .map(|v| format!("  {}: {} — {}", v.rule, v.subject, v.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
    Ok(())
}

/// Every server-to-client request the registry classifies has a row, and the
/// matrix invents none. This is the coverage claim in both directions.
#[test]
fn live_matrix_covers_exactly_the_registry_request_set() -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    let matrix = load(&root, Path::new("policy/server-request-ownership.v1.toml"))?;
    let source = std::fs::read_to_string(root.join(&matrix.meta.direction_registry))?;
    let (registry, _findings) = parse_direction_registry(&source, &BTreeMap::new());

    let mut expected: Vec<&String> = registry
        .iter()
        .filter(|(_, kind)| **kind == RegistryKind::ServerToClientRequest)
        .map(|(method, _)| method)
        .collect();
    expected.sort();
    let mut actual: Vec<&String> = matrix.request.iter().map(|row| &row.method).collect();
    actual.sort();

    assert!(!expected.is_empty(), "registry parse returned nothing; the instrument failed");
    assert_eq!(actual, expected, "matrix rows and registry requests must be the same set");
    Ok(())
}

// ── Negative controls: coverage and identity ────────────────────────────

#[test]
fn a_registry_request_with_no_row_fails() {
    let mut discovered = agreeing_discovery();
    discovered
        .registry
        .insert("workspace/inlayHint/refresh".to_string(), RegistryKind::ServerToClientRequest);
    assert!(
        rules(vec![passing_row()], &discovered).contains(&"missing-row"),
        "a newly emitted refresh request omitted from the matrix must fail"
    );
}

#[test]
fn a_duplicated_method_fails() {
    let mut second = passing_row();
    second.id = "srq-example-copy".to_string();
    assert!(
        rules(vec![passing_row(), second], &agreeing_discovery()).contains(&"duplicate-method")
    );
}

#[test]
fn the_registration_pair_may_not_share_one_row() {
    let mut discovered = agreeing_discovery();
    for method in ["client/registerCapability", "client/unregisterCapability"] {
        discovered.registry.insert(method.to_string(), RegistryKind::ServerToClientRequest);
    }
    let mut row = passing_row();
    row.method = "client/registerCapability".to_string();
    row.emitters.clear();
    row.emission = "not_emitted".to_string();
    row.disposition = "not_proven".to_string();
    row.feature_catalog_row = "none".to_string();

    let found = rules(vec![row], &discovered);
    assert!(
        found.contains(&"registration-pair-split") || found.contains(&"missing-row"),
        "covering only registerCapability must not satisfy unregisterCapability: {found:?}"
    );
}

// ── Negative controls: direction and envelope ───────────────────────────

#[test]
fn a_notification_row_fails() {
    let mut discovered = agreeing_discovery();
    discovered
        .registry
        .insert("window/logMessage".to_string(), RegistryKind::ServerToClientNotification);
    let mut row = passing_row();
    row.method = "window/logMessage".to_string();
    row.emitters.clear();
    row.emission = "not_emitted".to_string();
    row.disposition = "not_proven".to_string();
    row.feature_catalog_row = "none".to_string();

    assert!(
        rules(vec![row], &discovered).contains(&"wrong-envelope"),
        "a notification expects no response and can never satisfy server-request coverage"
    );
}

#[test]
fn an_opposite_direction_row_fails() {
    let mut discovered = agreeing_discovery();
    discovered
        .registry
        .insert("workspace/textDocumentContent".to_string(), RegistryKind::ClientToServer);
    let mut row = passing_row();
    row.method = "workspace/textDocumentContent".to_string();
    row.emitters.clear();
    row.emission = "not_emitted".to_string();
    row.disposition = "not_proven".to_string();
    row.feature_catalog_row = "none".to_string();

    assert!(
        rules(vec![row], &discovered).contains(&"wrong-direction"),
        "the client-to-server request of a similar name is a different method"
    );
}

// ── Negative controls: emission ─────────────────────────────────────────

#[test]
fn claiming_emission_without_a_call_site_fails() {
    let mut discovered = agreeing_discovery();
    discovered.emitted.clear();
    assert!(rules(vec![passing_row()], &discovered).contains(&"emission-mismatch"));
}

#[test]
fn calling_a_live_emitter_dormant_fails() {
    let mut row = passing_row();
    row.emission = "not_emitted".to_string();
    row.emitters.clear();
    row.disposition = "not_proven".to_string();
    assert!(rules(vec![row], &agreeing_discovery()).contains(&"emission-mismatch"));
}

#[test]
fn a_stale_emitter_path_fails() {
    let mut row = passing_row();
    row.emitters = vec!["crates/perl-lsp-rs/src/runtime/deleted_module.rs#gone".to_string()];
    assert!(rules(vec![row], &agreeing_discovery()).contains(&"emitter-path-stale"));
}

#[test]
fn a_stale_emitter_symbol_fails() {
    let mut row = passing_row();
    row.emitters =
        vec!["crates/perl-lsp-rs/src/runtime/client_requests.rs#renamed_away_symbol".to_string()];
    assert!(rules(vec![row], &agreeing_discovery()).contains(&"emitter-symbol-stale"));
}

/// Citing a symbol that really exists in the file but emits a different method
/// must not satisfy the row. Path-level presence is not ownership.
#[test]
fn a_real_but_wrong_emitter_symbol_fails() {
    let mut row = passing_row();
    row.emitters = vec![
        "crates/perl-lsp-rs/src/runtime/client_requests.rs#request_inlay_hint_refresh".to_string(),
    ];
    let found = rules(vec![row], &agreeing_discovery());
    assert!(
        found.contains(&"emitter-not-discovered"),
        "a real symbol emitting another method must not satisfy this row: {found:?}"
    );
}

/// A second path that emits the method must be cited; omitting it silently
/// leaves the row's ownership claim incomplete.
#[test]
fn an_uncited_second_emitter_fails() {
    let mut discovered = agreeing_discovery();
    if let Some(paths) = discovered.emitted.get_mut("workspace/codeLens/refresh") {
        paths.push("crates/perl-lsp-rs/src/runtime/window.rs#another_emitter".to_string());
    }
    let found = rules(vec![passing_row()], &discovered);
    assert!(found.contains(&"emitter-uncited"), "an uncited emitting path must fail: {found:?}");
}

// ── Negative controls: credit and proof ─────────────────────────────────

#[test]
fn support_credit_without_terminal_owner_or_process_proof_fails() {
    let mut row = passing_row();
    row.disposition = "supported".to_string();
    assert!(
        rules(vec![row], &agreeing_discovery()).contains(&"support-without-proof"),
        "missing evidence must never be rendered as support"
    );
}

#[test]
fn a_dormant_method_may_not_keep_credit() {
    let mut discovered = agreeing_discovery();
    discovered.emitted.clear();
    let mut row = passing_row();
    row.emission = "not_emitted".to_string();
    row.emitters.clear();
    // disposition stays advertised_not_proven, which is credit-bearing.
    assert!(
        rules(vec![row], &discovered).contains(&"dormant-credit"),
        "a helper-only or dead method must not retain support credit"
    );
}

#[test]
fn a_selected_318_surface_may_not_claim_stable_317() {
    let mut discovered = agreeing_discovery();
    discovered
        .registry
        .insert("workspace/foldingRange/refresh".to_string(), RegistryKind::ServerToClientRequest);
    discovered
        .catalog_rows
        .insert("lsp.folding_range_refresh".to_string(), catalog("LSP 3.18", "workspace"));
    discovered.emitted.insert(
        "workspace/foldingRange/refresh".to_string(),
        vec![
            "crates/perl-lsp-rs/src/runtime/client_requests.rs#request_folding_range_refresh"
                .to_string(),
        ],
    );

    let mut row = passing_row();
    row.method = "workspace/foldingRange/refresh".to_string();
    row.spec = "LSP 3.18".to_string();
    row.protocol_baseline = "stable_3_17".to_string();
    row.feature_catalog_row = "lsp.folding_range_refresh".to_string();
    row.emitters = vec![
        "crates/perl-lsp-rs/src/runtime/client_requests.rs#request_folding_range_refresh"
            .to_string(),
    ];

    assert!(
        rules(vec![row], &discovered).contains(&"baseline-understated"),
        "3.18-selected evidence must not be silently counted as stable 3.17"
    );
}

/// Editing only the matrix side must not demote a 3.18 surface. The catalog's
/// spec is authoritative, so changing `spec` AND `protocol_baseline` together
/// still fails — this is the evasion an existence-only catalog join allowed.
#[test]
fn demoting_a_318_surface_on_the_matrix_side_alone_fails() {
    let mut discovered = agreeing_discovery();
    discovered
        .registry
        .insert("workspace/foldingRange/refresh".to_string(), RegistryKind::ServerToClientRequest);
    discovered
        .catalog_rows
        .insert("lsp.folding_range_refresh".to_string(), catalog("LSP 3.18", "workspace"));
    discovered.emitted.insert(
        "workspace/foldingRange/refresh".to_string(),
        vec![
            "crates/perl-lsp-rs/src/runtime/client_requests.rs#request_folding_range_refresh"
                .to_string(),
        ],
    );

    let mut row = passing_row();
    row.method = "workspace/foldingRange/refresh".to_string();
    // Both matrix-side fields say 3.17 while the catalog still says 3.18.
    row.spec = "LSP 3.17".to_string();
    row.protocol_baseline = "stable_3_17".to_string();
    row.feature_catalog_row = "lsp.folding_range_refresh".to_string();
    row.emitters = vec![
        "crates/perl-lsp-rs/src/runtime/client_requests.rs#request_folding_range_refresh"
            .to_string(),
    ];

    let found = rules(vec![row], &discovered);
    assert!(
        found.contains(&"catalog-spec-mismatch"),
        "the catalog's spec must be consumed, not just its key: {found:?}"
    );
    assert!(
        found.contains(&"baseline-understated"),
        "the 3.18 boundary must derive from the catalog spec: {found:?}"
    );
}

/// Pointing a row at an unrelated but existing server-to-client catalog row
/// must fail on the spec it actually records.
#[test]
fn citing_an_unrelated_catalog_row_fails() {
    let mut discovered = agreeing_discovery();
    discovered
        .catalog_rows
        .insert("lsp.inlay_hint_refresh".to_string(), catalog("LSP 3.17", "workspace"));
    let mut row = passing_row();
    row.feature_catalog_row = "lsp.inlay_hint_refresh".to_string();
    assert!(rules(vec![row], &discovered).contains(&"catalog-spec-mismatch"));
}

#[test]
fn claiming_a_per_method_decoder_that_does_not_exist_fails() {
    let mut row = passing_row();
    row.response_decoder = "per_method".to_string();
    assert!(
        rules(vec![row], &agreeing_discovery()).contains(&"decoder-overclaim"),
        "decoding is generic shape-only; a per-method decoder claim must be refuted"
    );
}

#[test]
fn an_unknown_cell_value_fails() {
    let mut row = passing_row();
    row.disposition = "probably_fine".to_string();
    assert!(
        rules(vec![row], &agreeing_discovery()).contains(&"value-not-allowed"),
        "an unknown proof cell must not render as passing"
    );
}

#[test]
fn an_unknown_catalog_row_fails() {
    let mut row = passing_row();
    row.feature_catalog_row = "lsp.invented_row".to_string();
    assert!(rules(vec![row], &agreeing_discovery()).contains(&"catalog-row-unknown"));
}

/// The matrix must not define its own vocabulary. Widening the meta allow-list
/// and using the invented value in a row has to fail on the allow-list itself,
/// otherwise the file under validation validates itself.
#[test]
fn a_matrix_that_widens_its_own_vocabulary_fails() {
    let mut row = passing_row();
    row.disposition = "definitely_fine".to_string();
    let mut matrix = matrix_of(vec![row]);
    matrix.meta.allowed_dispositions.push("definitely_fine".to_string());

    let found: Vec<&str> = check(&repo_root(), &matrix, &agreeing_discovery(), Vec::new())
        .into_iter()
        .map(|violation| violation.rule)
        .collect();

    assert!(
        found.contains(&"vocabulary-not-authoritative"),
        "an extended allow-list must be rejected: {found:?}"
    );
    assert!(
        found.contains(&"value-not-allowed"),
        "the invented value must still fail against the schema: {found:?}"
    );
}

/// Spec equality alone would accept a swap between two catalog rows sharing a
/// version, so the catalog's area must own the method's wire segment.
#[test]
fn citing_a_catalog_row_from_another_area_fails() {
    let mut discovered = agreeing_discovery();
    // `lsp.show_message_request` is a real server-to-client row that shares no
    // area with a `workspace/` method.
    discovered
        .catalog_rows
        .insert("lsp.show_message_request".to_string(), catalog("LSP 3.16", "window"));
    let mut row = passing_row();
    row.feature_catalog_row = "lsp.show_message_request".to_string();

    assert!(
        rules(vec![row], &discovered).contains(&"catalog-area-mismatch"),
        "a workspace method may not claim a window catalog row"
    );
}

/// `path#symbol` cannot distinguish two same-named functions in one file, so an
/// ambiguous attribution is refused rather than standing for both.
#[test]
fn an_ambiguous_emitter_symbol_fails() {
    let mut discovered = agreeing_discovery();
    discovered.ambiguous_symbols.insert(
        "crates/perl-lsp-rs/src/runtime/client_requests.rs#request_code_lens_refresh".to_string(),
    );
    assert!(rules(vec![passing_row()], &discovered).contains(&"emitter-ambiguous"));
}

/// A registry entry naming its method by constant must be resolved, not
/// skipped. Skipping shrank the coverage denominator, so a newly classified
/// request would have needed no row.
#[test]
fn a_constant_named_registry_entry_is_resolved() {
    let mut constants = BTreeMap::new();
    constants.insert("WORKSPACE_APPLY_EDIT".to_string(), "workspace/applyEdit".to_string());
    let (parsed, findings) = parse_direction_registry(
        r"
        pub(crate) const REGISTRY: &[MethodDescriptor] = &[
            s2c(WORKSPACE_APPLY_EDIT, EnvelopeKind::Request),
        ];
    ",
        &constants,
    );
    assert_eq!(parsed.get("workspace/applyEdit"), Some(&RegistryKind::ServerToClientRequest));
    assert!(findings.is_empty(), "{findings:?}");
}

/// An unresolvable registry entry is an instrument finding, never a silent
/// shrink of the denominator.
#[test]
fn an_unresolvable_registry_entry_fails_closed() {
    let (parsed, findings) = parse_direction_registry(
        r"
        pub(crate) const REGISTRY: &[MethodDescriptor] = &[
            s2c(compute_method(), EnvelopeKind::Request),
        ];
    ",
        &BTreeMap::new(),
    );
    assert!(parsed.is_empty());
    assert!(
        findings.iter().any(|finding| finding.rule == "registry-entry-unresolved"),
        "an unreadable classification entry must be a finding: {findings:?}"
    );
}

/// Coverage must not rest on the registry parse alone: a method the emission
/// scan found needs a row even if its classification entry was unreadable.
#[test]
fn an_emitted_method_with_no_row_fails() {
    let mut discovered = agreeing_discovery();
    discovered.emitted.insert(
        "workspace/inlayHint/refresh".to_string(),
        vec![
            "crates/perl-lsp-rs/src/runtime/client_requests.rs#request_inlay_hint_refresh"
                .to_string(),
        ],
    );
    assert!(
        rules(vec![passing_row()], &discovered).contains(&"emitted-without-row"),
        "an emitted method absent from the matrix must fail even when unclassified"
    );
}

/// Support credit requires a positively shaped owner reference. Rejecting only
/// the `missing` sentinel would accept an empty cell, `none`, or a typo.
#[test]
fn support_credit_requires_a_shaped_owner_reference() {
    for stand_in in ["", "none", "not_applicable_no_emitter", "issue 6724"] {
        let mut row = passing_row();
        row.disposition = "supported".to_string();
        row.terminal_state_owner = stand_in.to_string();
        row.exact_process_proof = "#7016".to_string();
        assert!(
            rules(vec![row], &agreeing_discovery()).contains(&"support-without-proof"),
            "`{stand_in}` must not stand in for a terminal-state owner"
        );
    }
}

/// A one-hop forwarder must not hide a concrete method: a helper declaring
/// `method: &str` is itself treated as a send site for its callers.
#[test]
fn a_call_through_a_local_forwarder_is_discovered() -> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r#"
impl Server {
    fn issue(&self, method: &str, params: Value) -> io::Result<()> {
        self.send_request(method, params).map(|_| ())
    }

    pub fn unregister(&self) {
        self.issue("client/unregisterCapability", json!(null));
    }
}
"#,
    )?;

    assert_eq!(
        emitted.get("client/unregisterCapability").map(Vec::as_slice),
        Some(["src/runtime/synthetic.rs#unregister".to_string()].as_slice()),
        "a concrete method passed to a declared forwarder must still be discovered"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// The catalog is the authority on maturity and ownership: a row may not grant
/// itself support the catalog does not record, even with well-formed cells.
#[test]
fn support_contradicting_the_catalog_fails() {
    let mut row = passing_row();
    row.disposition = "supported".to_string();
    row.terminal_state_owner = "#6724".to_string();
    row.exact_process_proof = "#7016".to_string();

    let found = rules(vec![row], &agreeing_discovery());
    assert!(
        found.contains(&"support-contradicts-catalog"),
        "features.toml records this row as not_proven with no state owner: {found:?}"
    );
}

/// A comment between the callee and its argument list is valid Rust and must
/// not hide a send site.
#[test]
fn a_comment_before_the_argument_list_is_still_discovered() -> Result<(), Box<dyn std::error::Error>>
{
    let (emitted, findings) = scan_synthetic(
        r#"
impl Server {
    pub fn emit_commented(&self) {
        self.send_request /* refresh */ ("workspace/codeLens/refresh", json!(null));
    }
}
"#,
    )?;

    assert_eq!(
        emitted.get("workspace/codeLens/refresh").map(Vec::as_slice),
        Some(["src/runtime/synthetic.rs#emit_commented".to_string()].as_slice()),
        "a block comment before `(` must not hide a send site"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

// ── Negative controls: instrument failure ───────────────────────────────

#[test]
fn an_empty_matrix_is_not_a_pass() {
    assert!(rules(Vec::new(), &agreeing_discovery()).contains(&"matrix-empty"));
}

#[test]
fn an_empty_discovery_is_not_a_pass() {
    let discovered = Discovered::default();
    let found = rules(vec![passing_row()], &discovered);
    assert!(found.contains(&"registry-empty"), "zero discovered methods is NOT_PROVEN: {found:?}");
    assert!(found.contains(&"catalog-empty"));
}

// ── Discovery parsers ───────────────────────────────────────────────────

#[test]
fn the_registry_parser_separates_direction_and_envelope() {
    let source = r#"
        pub(crate) const REGISTRY: &[MethodDescriptor] = &[
            c2s("textDocument/hover", EnvelopeKind::Request, LifecyclePhase::RequiresInitialized),
            s2c("workspace/applyEdit", EnvelopeKind::Request),
            s2c("window/logMessage", EnvelopeKind::Notification),
            ext(
                "$/perl-lsp/clientResponse",
                EnvelopeKind::Notification,
                MethodDirection::ClientToServer,
                LifecyclePhase::Anytime,
            ),
        ];
    "#;
    let (parsed, _findings) = parse_direction_registry(source, &BTreeMap::new());
    assert_eq!(parsed.get("workspace/applyEdit"), Some(&RegistryKind::ServerToClientRequest));
    assert_eq!(parsed.get("window/logMessage"), Some(&RegistryKind::ServerToClientNotification));
    assert_eq!(parsed.get("textDocument/hover"), Some(&RegistryKind::ClientToServer));
    assert_eq!(parsed.get("$/perl-lsp/clientResponse"), Some(&RegistryKind::ClientToServer));
}

/// The live registry must classify a known request, notification, and
/// opposite-direction method. This fails if the registry shape changes under us.
#[test]
fn the_live_registry_parses() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(
        repo_root().join("crates/perl-lsp-rs/src/protocol/method_direction.rs"),
    )?;
    let (parsed, _findings) = parse_direction_registry(&source, &BTreeMap::new());
    assert_eq!(parsed.get("workspace/applyEdit"), Some(&RegistryKind::ServerToClientRequest));
    assert_eq!(parsed.get("window/logMessage"), Some(&RegistryKind::ServerToClientNotification));
    assert_eq!(
        parsed.get("workspace/textDocumentContent"),
        Some(&RegistryKind::ClientToServer),
        "the client-to-server request must stay distinct from its /refresh counterpart"
    );
    Ok(())
}

/// Emission discovery must resolve a constant-named send and must not count a
/// send that only appears inside a `#[cfg(test)]` module.
#[test]
fn emission_discovery_resolves_constants_and_ignores_test_modules()
-> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    let constants_source =
        std::fs::read_to_string(root.join("crates/perl-lsp-rs-core/src/protocol/methods.rs"))?;
    let mut constants = BTreeMap::new();
    for line in constants_source.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("pub const ") else { continue };
        let Some((name, value)) = rest.split_once(": &str = \"") else { continue };
        let Some(value) = value.strip_suffix("\";") else { continue };
        constants.insert(name.trim().to_string(), value.to_string());
    }

    let (emitted, _ambiguous, findings) =
        scan_emission(&root, "crates/perl-lsp-rs/src/runtime", &constants)?;

    assert!(findings.is_empty(), "unresolved emission sites: {findings:?}");
    assert!(
        emitted.contains_key("workspace/applyEdit"),
        "the constant-named applyEdit send must be discovered, not only string literals"
    );
    assert!(
        !emitted.contains_key("client/unregisterCapability"),
        "unregisterCapability appears only in test modules and must not count as production \
         emission"
    );
    Ok(())
}

/// Write a synthetic runtime tree and scan it.
fn scan_synthetic(
    source: &str,
) -> Result<(BTreeMap<String, Vec<String>>, Vec<String>), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let runtime = dir.path().join("src").join("runtime");
    std::fs::create_dir_all(&runtime)?;
    std::fs::write(runtime.join("synthetic.rs"), source)?;

    let mut constants = BTreeMap::new();
    constants.insert("WORKSPACE_APPLY_EDIT".to_string(), "workspace/applyEdit".to_string());

    let (emitted, _ambiguous, findings) = scan_emission(dir.path(), "src/runtime", &constants)?;
    Ok((emitted, findings.into_iter().map(|finding| finding.rule.to_string()).collect()))
}

/// The scanner parses to the matching `)`, not to a line budget. A normally
/// formatted call whose method argument sits on the fourth line must still be
/// discovered — the bounded-window version silently dropped it, because the
/// visible arguments were all plain identifiers and read as forwarding.
#[test]
fn a_call_wrapped_beyond_three_lines_is_still_discovered() -> Result<(), Box<dyn std::error::Error>>
{
    let (emitted, findings) = scan_synthetic(
        r"
impl Server {
    pub fn emit_wrapped(&self) {
        self.send_request_internal(
            id,
            params,
            WORKSPACE_APPLY_EDIT,
        );
    }
}
",
    )?;

    assert_eq!(
        emitted.get("workspace/applyEdit").map(Vec::as_slice),
        Some(["src/runtime/synthetic.rs#emit_wrapped".to_string()].as_slice()),
        "a call whose method sits past the third line must still be attributed"
    );
    assert!(findings.is_empty(), "a resolvable wrapped call is not a finding: {findings:?}");
    Ok(())
}

/// Rust allows whitespace between a callee and its argument list. The trigger
/// match must tolerate it, or valid source disappears from the denominator
/// without a finding.
#[test]
fn whitespace_before_the_argument_list_is_still_discovered()
-> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r#"
impl Server {
    pub fn emit_spaced(&self) {
        self.send_request ("workspace/codeLens/refresh", json!(null));
    }
}
"#,
    )?;

    assert_eq!(
        emitted.get("workspace/codeLens/refresh").map(Vec::as_slice),
        Some(["src/runtime/synthetic.rs#emit_spaced".to_string()].as_slice()),
        "a space before `(` must not hide a send site"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// The same tolerance in the registry parser: `s2c ("m", ..)` is valid Rust and
/// must not drop a classified method.
#[test]
fn the_registry_parser_tolerates_whitespace_before_the_argument_list() {
    let (parsed, _findings) = parse_direction_registry(
        r#"
        pub(crate) const REGISTRY: &[MethodDescriptor] = &[
            s2c ("workspace/applyEdit", EnvelopeKind::Request),
        ];
    "#,
        &BTreeMap::new(),
    );
    assert_eq!(parsed.get("workspace/applyEdit"), Some(&RegistryKind::ServerToClientRequest));
}

/// A send whose method cannot be resolved fails closed unless the enclosing
/// function declares the method as its own caller-supplied parameter.
#[test]
fn an_unresolvable_send_outside_a_declared_forwarder_fails_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r"
impl Server {
    pub fn emit_dynamic(&self) {
        let chosen = pick();
        self.send_request(chosen, params);
    }
}
",
    )?;

    assert!(emitted.is_empty());
    assert!(
        findings.contains(&"emission-unresolved".to_string()),
        "an unattributable send must be a finding, not a silent skip: {findings:?}"
    );
    Ok(())
}

/// A brace-less `#[cfg(test)] mod name;` must drop only that declaration.
/// Jumping to the next `{` anywhere later in the file deleted unrelated
/// production code, so a real emitter could read as "not emitted".
#[test]
fn a_braceless_test_module_declaration_keeps_later_production_code()
-> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r#"
#[cfg(test)]
mod tests;

impl Server {
    pub fn emit_after_the_declaration(&self) {
        self.send_request("workspace/codeLens/refresh", json!(null));
    }
}
"#,
    )?;

    assert_eq!(
        emitted.get("workspace/codeLens/refresh").map(Vec::as_slice),
        Some(["src/runtime/synthetic.rs#emit_after_the_declaration".to_string()].as_slice()),
        "production code following a brace-less test-module declaration must survive stripping"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// The block form still strips, so a test-only send stays out of production
/// emission. This is the control that the fix above did not over-correct.
#[test]
fn a_block_test_module_is_still_stripped() -> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r#"
#[cfg(test)]
mod tests {
    fn only_a_test() {
        server.send_request("workspace/codeLens/refresh", json!(null));
    }
}
"#,
    )?;

    assert!(emitted.is_empty(), "a test-only send must not count as production: {emitted:?}");
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// The forwarding exemption is closed: it applies only to a function whose own
/// signature takes `method: &str`, not to any call passing bare identifiers.
#[test]
fn a_declared_forwarder_is_exempt() -> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r"
impl Server {
    fn send_request_internal(&self, method: &str, params: Value) -> io::Result<()> {
        self.send_request(method, params).map(|_| ())
    }
}
",
    )?;

    assert!(emitted.is_empty());
    assert!(findings.is_empty(), "a declared forwarder is not a finding: {findings:?}");
    Ok(())
}

#[test]
fn the_feature_catalog_parser_selects_only_server_rows() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(repo_root().join("features.toml"))?;
    let rows = parse_feature_catalog(&source)?;
    assert!(rows.contains_key("lsp.code_lens_refresh"));
    assert_eq!(rows.get("lsp.code_lens_refresh").map(|row| row.spec.as_str()), Some("LSP 3.16"));
    assert!(
        !rows.contains_key("lsp.hover"),
        "client-to-server rows must not enter the server-request catalog view"
    );
    Ok(())
}

/// The catalog is parsed as TOML, not scanned for a substring. A row whose
/// prose quotes the direction key must not be classified as a server row, and
/// a real row whose formatting differs must still be found.
#[test]
fn the_catalog_parser_reads_the_direction_key_not_prose() -> Result<(), Box<dyn std::error::Error>>
{
    let source = r#"
[[feature]]
id = "lsp.prose_only"
spec = "LSP 3.0"
direction = "client_to_server"
description = "uses direction = \"server_to_client\" semantics internally"

[[feature]]
id = "lsp.oddly_spaced"
spec   =    "LSP 3.17"
direction="server_to_client"
"#;
    let rows = parse_feature_catalog(source)?;
    assert!(
        !rows.contains_key("lsp.prose_only"),
        "a description quoting the direction key must not create a server row"
    );
    assert_eq!(
        rows.get("lsp.oddly_spaced").map(|row| row.spec.as_str()),
        Some("LSP 3.17"),
        "a real server row must survive different spacing and quoting"
    );
    Ok(())
}

/// The 3.18 boundary anchors on the spec's leading version token, so prose
/// mentioning another version cannot decide a baseline.
#[test]
fn the_318_boundary_anchors_on_the_leading_version_token() {
    use super::check::{declared_version, declares_selected_318};

    assert_eq!(declared_version("LSP 3.16 (superseded by 3.18)"), Some("3.16"));
    assert!(
        !declares_selected_318("LSP 3.16 (superseded by 3.18)"),
        "a trailing mention of 3.18 must not make a 3.16 surface 3.18-selected"
    );
    assert!(declares_selected_318("LSP 3.18"));
    assert!(declares_selected_318("@proposed"));
    assert!(!declares_selected_318("LSP 3.17"));
}

// ── Determinism ─────────────────────────────────────────────────────────

#[test]
fn the_rendered_view_and_fingerprint_are_stable() -> Result<(), Box<dyn std::error::Error>> {
    let matrix = load(&repo_root(), Path::new("policy/server-request-ownership.v1.toml"))?;
    let first = render(&matrix, None);
    let second = render(&matrix, None);
    assert_eq!(first, second);
    assert_eq!(fingerprint(&first), fingerprint(&second));
    Ok(())
}

#[test]
fn the_fingerprint_is_sensitive_to_a_load_bearing_field() {
    let base = matrix_of(vec![passing_row()]);
    let mut mutated_row = passing_row();
    mutated_row.disposition = "not_proven".to_string();
    let mutated = matrix_of(vec![mutated_row]);

    assert_ne!(
        fingerprint(&render(&base, None)),
        fingerprint(&render(&mutated, None)),
        "a disposition change must move the fingerprint"
    );
}
