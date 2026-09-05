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
        emission_scan_root: "crates/perl-lsp-rs/src".to_string(),
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
    catalog_rows.insert("lsp.code_lens_refresh".to_string(), catalog("LSP 3.16"));
    Discovered { registry, emitted, catalog_rows, ambiguous_symbols: BTreeSet::new() }
}

fn catalog(spec: &str) -> CatalogRow {
    CatalogRow {
        spec: spec.to_string(),
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

/// The committed scan root must cover the whole sender crate.
///
/// `an_emitter_outside_the_runtime_subtree_is_discovered` proves the *reader*
/// sees more under a crate-wide root than under a runtime-only one, but it does
/// so on a synthetic tree. Nothing asserted what the committed matrix actually
/// selects, so narrowing `[meta] emission_scan_root` back to
/// `crates/perl-lsp-rs/src/runtime` passed every test and left the live gate
/// green — silently restoring the blindness the widening removed, because a
/// file that is never read cannot produce a finding.
///
/// The boundary is derivable rather than chosen: `OutboundSink`, which declares
/// `send_request`, is `pub(crate)` in `runtime/outbound.rs`, so every call site
/// that can reach a sender lies inside this crate and none lies outside it.
/// Widening past the crate would sweep in the unrelated `send_request` helpers
/// in `perl-dap` and `perl-parser`.
#[test]
fn the_live_scan_root_covers_the_whole_sender_crate() -> Result<(), Box<dyn std::error::Error>> {
    let matrix = load(&repo_root(), Path::new("policy/server-request-ownership.v1.toml"))?;
    assert_eq!(
        matrix.meta.emission_scan_root, "crates/perl-lsp-rs/src",
        "the scan root must be the crate that declares the sender; a narrower root cannot \
         report what it never reads"
    );
    Ok(())
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

/// A renamed symbol is caught by the source-derived comparison, not by a
/// substring test for the name: the citation is not in the discovered set, and
/// the symbol that really emits is left uncited.
#[test]
fn a_stale_emitter_symbol_fails() {
    let mut row = passing_row();
    row.emitters =
        vec!["crates/perl-lsp-rs/src/runtime/client_requests.rs#renamed_away_symbol".to_string()];
    let findings = rules(vec![row], &agreeing_discovery());
    assert!(findings.contains(&"emitter-not-discovered"), "{findings:?}");
    assert!(findings.contains(&"emitter-uncited"), "{findings:?}");
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
    discovered.catalog_rows.insert("lsp.folding_range_refresh".to_string(), catalog("LSP 3.18"));
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
    discovered.catalog_rows.insert("lsp.folding_range_refresh".to_string(), catalog("LSP 3.18"));
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
    discovered.catalog_rows.insert("lsp.inlay_hint_refresh".to_string(), catalog("LSP 3.17"));
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
/// version, so the cited row must name this method.
#[test]
fn citing_a_catalog_row_belonging_to_another_method_fails() {
    let mut discovered = agreeing_discovery();
    // `lsp.show_message_request` is a real server-to-client row, but it is not
    // this method's row.
    discovered.catalog_rows.insert("lsp.show_message_request".to_string(), catalog("LSP 3.16"));
    let mut row = passing_row();
    row.feature_catalog_row = "lsp.show_message_request".to_string();

    assert!(
        rules(vec![row], &discovered).contains(&"catalog-identity-mismatch"),
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

/// Write a synthetic runtime tree of named files and scan it.
fn scan_synthetic_files(
    files: &[(&str, &str)],
) -> Result<(BTreeMap<String, Vec<String>>, Vec<String>), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let runtime = dir.path().join("src").join("runtime");
    std::fs::create_dir_all(&runtime)?;
    for (name, source) in files {
        std::fs::write(runtime.join(name), source)?;
    }

    let mut constants = BTreeMap::new();
    constants.insert("WORKSPACE_APPLY_EDIT".to_string(), "workspace/applyEdit".to_string());

    let (emitted, _ambiguous, findings) = scan_emission(dir.path(), "src/runtime", &constants)?;
    Ok((emitted, findings.into_iter().map(|finding| finding.rule.to_string()).collect()))
}

/// Write one synthetic runtime file and scan it.
fn scan_synthetic(
    source: &str,
) -> Result<(BTreeMap<String, Vec<String>>, Vec<String>), Box<dyn std::error::Error>> {
    scan_synthetic_files(&[("synthetic.rs", source)])
}

/// `senders` holds bare names, because a call site names a function rather than
/// a definition. An unrelated function sharing a forwarder's name therefore made
/// its own callers' arguments read as emitted methods, silently. Attributing the
/// call needs types a syntactic reader lacks, so the collision must be reported.
#[test]
fn a_forwarder_name_shared_with_an_unrelated_function_is_reported()
-> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic_files(&[
        (
            "real.rs",
            r"
impl Server {
    fn relay(&self, method: &str, params: Value) -> io::Result<()> {
        self.send_request(id, method, params)
    }
}
",
        ),
        (
            "other.rs",
            r#"
impl Unrelated {
    fn relay(&self, label: &str) -> String {
        label.to_string()
    }
    fn caller(&self) -> String {
        self.relay("phantom/method")
    }
}
"#,
        ),
    ])?;

    assert_eq!(
        findings,
        vec!["forwarder-ambiguous".to_string()],
        "a name that both forwards and does not must not resolve silently"
    );
    assert!(
        !emitted.contains_key("phantom/method"),
        "the unrelated call is not an emission: {emitted:?}"
    );
    Ok(())
}

/// `#[cfg(test)] mod tests;` leaves its code in another file, and that file,
/// parsed alone, shows no sign of having been gated. Skipping by filename could
/// not recover it: `tests.rs` does not end in `_tests.rs`, so a send there was
/// read as production. The declaration is resolved instead.
#[test]
fn a_send_in_an_externally_declared_test_module_is_not_production()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let runtime = dir.path().join("src").join("runtime");
    std::fs::create_dir_all(runtime.join("owner"))?;
    std::fs::write(
        runtime.join("owner.rs"),
        r"
#[cfg(test)]
mod tests;
mod helpers;
",
    )?;
    std::fs::write(
        runtime.join("owner").join("tests.rs"),
        r#"
impl Server {
    fn exercise(&self) -> io::Result<()> {
        self.send_request(id, "test-only/never-sent", params)
    }
}
"#,
    )?;
    // The ungated sibling is the control: resolution must not swallow it.
    std::fs::write(
        runtime.join("owner").join("helpers.rs"),
        r#"
impl Server {
    fn emit(&self) -> io::Result<()> {
        self.send_request(id, "window/showDocument", params)
    }
}
"#,
    )?;

    let constants = BTreeMap::new();
    let (emitted, _ambiguous, findings) = scan_emission(dir.path(), "src", &constants)?;

    assert!(
        !emitted.contains_key("test-only/never-sent"),
        "a send inside an externally declared test module is not production: {emitted:?}"
    );
    assert_eq!(
        emitted.get("window/showDocument").map(Vec::len),
        Some(1),
        "an ungated sibling module is still production: {emitted:?}"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// A forwarder that implements a trait method is the ordinary shape here --
/// `OutboundSink` is exactly it. The declaration carries no body, so counting it
/// as a second, non-forwarding definition made the ambiguity check fire on a
/// perfectly attributable call and suppress the emission it should have found.
#[test]
fn a_bare_trait_declaration_is_not_a_competing_definition() -> Result<(), Box<dyn std::error::Error>>
{
    let (emitted, findings) = scan_synthetic_files(&[
        (
            "sink.rs",
            r"
pub(crate) trait Sink {
    fn relay(&self, method: &str, params: Value) -> io::Result<()>;
}
",
        ),
        (
            "impl.rs",
            r#"
impl Sink for Server {
    fn relay(&self, method: &str, params: Value) -> io::Result<()> {
        self.send_request(id, method, params)
    }
}
impl Server {
    fn caller(&self) -> io::Result<()> {
        self.relay("window/showDocument", params)
    }
}
"#,
        ),
    ])?;

    assert!(findings.is_empty(), "a bare declaration is not a rival definition: {findings:?}");
    assert_eq!(
        emitted.get("window/showDocument").map(Vec::len),
        Some(1),
        "the call through the trait forwarder is still an emission: {emitted:?}"
    );
    Ok(())
}

/// A trait method *with* a default body is a real definition, so it still
/// counts -- otherwise the exclusion above would reopen the collision it closes.
#[test]
fn a_defaulted_trait_method_still_counts_as_a_definition() -> Result<(), Box<dyn std::error::Error>>
{
    let (emitted, findings) = scan_synthetic_files(&[
        (
            "real.rs",
            r"
impl Server {
    fn relay(&self, method: &str, params: Value) -> io::Result<()> {
        self.send_request(id, method, params)
    }
}
",
        ),
        (
            "defaulted.rs",
            r#"
pub(crate) trait Unrelated {
    fn relay(&self, label: &str) -> String {
        label.to_string()
    }
}
impl Server {
    fn caller(&self) -> String {
        self.relay("phantom/method")
    }
}
"#,
        ),
    ])?;

    assert_eq!(findings, vec!["forwarder-ambiguous".to_string()], "{findings:?}");
    assert!(!emitted.contains_key("phantom/method"), "{emitted:?}");
    Ok(())
}

/// The mirror of the above: when every definition of a shared name really does
/// forward, resolving by name is sound and must stay silent, or a trait with
/// several implementations would be unusable.
#[test]
fn a_forwarder_name_shared_only_with_other_forwarders_is_not_reported()
-> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic_files(&[
        (
            "one.rs",
            r"
impl ServerOne {
    fn relay(&self, method: &str, params: Value) -> io::Result<()> {
        self.send_request(id, method, params)
    }
}
",
        ),
        (
            "two.rs",
            r#"
impl ServerTwo {
    fn relay(&self, method: &str, params: Value) -> io::Result<()> {
        self.send_request(id, method, params)
    }
    fn caller(&self) -> io::Result<()> {
        self.relay("window/showDocument", params)
    }
}
"#,
        ),
    ])?;

    assert!(findings.is_empty(), "{findings:?}");
    assert!(
        emitted.contains_key("window/showDocument"),
        "a call through a genuine forwarder is still an emission: {emitted:?}"
    );
    Ok(())
}

/// A send outside `src/runtime` but inside the sender's own crate must still be
/// discovered. `OutboundSink` is `pub(crate)`, so the reachable surface is the
/// whole crate; scanning only the runtime subtree let a production emitter
/// elsewhere in it pass unseen, and an unread file reads as absence.
#[test]
fn an_emitter_outside_the_runtime_subtree_is_discovered() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let protocol = dir.path().join("src").join("protocol");
    std::fs::create_dir_all(&protocol)?;
    std::fs::write(
        protocol.join("elsewhere.rs"),
        r#"
impl Server {
    pub fn emit_from_protocol(&self) -> io::Result<()> {
        self.send_request(id, "window/showDocument", params)
    }
}
"#,
    )?;

    let constants = BTreeMap::new();

    // The crate-wide root sees it.
    let (emitted, _ambiguous, findings) = scan_emission(dir.path(), "src", &constants)?;
    assert_eq!(
        emitted.get("window/showDocument").map(Vec::len),
        Some(1),
        "a send anywhere in the sender's crate is an emission: {emitted:?}"
    );
    assert!(findings.is_empty(), "{findings:?}");

    // The old runtime-only root did not, which is the regression this pins.
    let (narrow, _ambiguous, _findings) = scan_emission(dir.path(), "src/runtime", &constants)?;
    assert!(
        !narrow.contains_key("window/showDocument"),
        "pins why the root was widened: the narrow root misses it in silence: {narrow:?}"
    );
    Ok(())
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

// ── Falsifiers for the open review findings (pre-fix) ───────────────────

/// Two catalog rows that share a spec version *and* an area were previously
/// interchangeable: `spec` plus `area` cannot tell `lsp.code_lens_refresh` from
/// `lsp.semantic_tokens_refresh`, so ownership could be assigned to the wrong
/// feature while the gate stayed green.
#[test]
fn a_catalog_row_of_the_same_spec_and_area_cannot_be_swapped_in() {
    let mut row = passing_row();
    row.feature_catalog_row = "lsp.semantic_tokens_refresh".to_string();
    let mut discovered = agreeing_discovery();
    discovered.catalog_rows.insert("lsp.semantic_tokens_refresh".to_string(), catalog("LSP 3.16"));

    assert!(
        rules(vec![row], &discovered).contains(&"catalog-identity-mismatch"),
        "a same-spec, same-area catalog row belonging to another method must be refused"
    );
}

/// Rust permits nested block comments. Stopping at the first `*/` left the
/// scanner unable to find the `(`, and the send silently left the denominator.
#[test]
fn a_nested_block_comment_before_the_argument_list_does_not_hide_a_send()
-> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r#"
impl Server {
    pub fn emit_commented(&self) {
        self.send_request /* outer /* inner */ still outer */ ("workspace/codeLens/refresh", p);
    }
}
"#,
    )?;

    assert_eq!(
        emitted.get("workspace/codeLens/refresh").map(Vec::as_slice),
        Some(["src/runtime/synthetic.rs#emit_commented".to_string()].as_slice()),
        "a nested block comment must not hide a send site"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// A send written in associated-function form is an ordinary emission. Matching
/// only `.send_request` let `Self::send_request(..)` leave discovery entirely.
#[test]
fn an_associated_function_send_is_still_discovered() -> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r#"
impl Server {
    pub fn emit_via_path(&self) {
        Self::send_request(self, "workspace/codeLens/refresh", p);
    }
}
"#,
    )?;

    assert_eq!(
        emitted.get("workspace/codeLens/refresh").map(Vec::as_slice),
        Some(["src/runtime/synthetic.rs#emit_via_path".to_string()].as_slice()),
        "an associated-function send must be discovered like a method call"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// A nested item does not end the body that declares it. The scanner bounded a
/// `fn` at the next `fn` in the file, so a send written after an inner helper
/// fell outside its own function: the forwarder never joined the closure and
/// its callers stopped being scanned, with no finding raised. `syn` gives the
/// real body; this pins that it stays that way.
#[test]
fn a_forwarder_whose_send_follows_a_nested_fn_still_propagates()
-> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r#"
impl Server {
    pub fn forward(&self, method: &str, params: Value) {
        fn shape(params: &Value) -> Value {
            params.clone()
        }
        self.send_request(method, shape(&params));
    }

    pub fn emit_after_nested(&self) {
        self.forward("workspace/codeLens/refresh", params);
    }
}
"#,
    )?;

    assert_eq!(
        emitted.get("workspace/codeLens/refresh").map(Vec::as_slice),
        Some(["src/runtime/synthetic.rs#emit_after_nested".to_string()].as_slice()),
        "an inner helper must not truncate the forwarder that declares it"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// The same confusion on the attribution side: picking the last `fn` declared
/// *before* a send credited an inner helper with its enclosing function's
/// emission, naming the wrong symbol in the row's `emitters` cell.
#[test]
fn a_send_after_a_nested_fn_is_attributed_to_the_enclosing_fn()
-> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r#"
impl Server {
    pub fn request_code_lens_refresh(&self) {
        fn shape(params: &Value) -> Value {
            params.clone()
        }
        self.send_request("workspace/codeLens/refresh", shape(&params));
    }
}
"#,
    )?;

    assert_eq!(
        emitted.get("workspace/codeLens/refresh").map(Vec::as_slice),
        Some(["src/runtime/synthetic.rs#request_code_lens_refresh".to_string()].as_slice()),
        "the enclosing function owns the send, not the helper declared above it"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// `method: &str` in a signature is not evidence that a helper sends anything.
/// Treating every such helper as a forwarder turned its literal-argument
/// callers into phantom requests — and its identifier-argument callers into
/// spurious `emission-unresolved` findings.
#[test]
fn a_method_named_helper_that_never_sends_is_not_a_forwarder()
-> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r#"
impl Server {
    fn is_lifecycle_method(&self, method: &str) -> bool {
        matches!(method, "initialize" | "shutdown")
    }

    pub fn route(&self, incoming: &str) -> bool {
        self.is_lifecycle_method("workspace/codeLens/refresh")
    }
}
"#,
    )?;

    assert!(
        emitted.is_empty(),
        "a helper that only inspects a method name emits nothing: {emitted:?}"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// The tightened forwarder rule must not lose the real one: a wrapper that
/// takes the method from its caller *and* reaches a sender still makes its own
/// callers send sites.
#[test]
fn a_forwarder_that_reaches_a_sender_still_propagates() -> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r#"
impl Server {
    fn dispatch(&self, method: &str, params: Value) -> io::Result<()> {
        self.send_request(method, params)
    }

    pub fn refresh(&self) {
        self.dispatch("workspace/codeLens/refresh", p);
    }
}
"#,
    )?;

    assert_eq!(
        emitted.get("workspace/codeLens/refresh").map(Vec::as_slice),
        Some(["src/runtime/synthetic.rs#refresh".to_string()].as_slice()),
        "a real forwarder must still expose its callers as send sites"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

// ── Falsifiers for the third review round (pre-fix) ─────────────────────

/// Only the exact `#[cfg(test)]` spelling was stripped, so a send behind a
/// compound test-only gate counted as production emission.
#[test]
fn a_compound_test_only_gate_is_still_stripped() -> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r#"
#[cfg(all(test, feature = "expose_lsp_test_api"))]
mod harness {
    impl Server {
        pub fn emit_in_test(&self) {
            self.send_request("workspace/codeLens/refresh", p);
        }
    }
}
"#,
    )?;

    assert!(emitted.is_empty(), "a test-only send is not production emission: {emitted:?}");
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// `any(test, feature = "x")` compiles in production, so its sends must count.
/// The fix must not over-strip into a silently smaller denominator.
#[test]
fn a_gate_that_compiles_in_production_is_not_stripped() -> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r#"
impl Server {
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub fn emit_maybe(&self) {
        self.send_request("workspace/codeLens/refresh", p);
    }
}
"#,
    )?;

    assert_eq!(
        emitted.get("workspace/codeLens/refresh").map(Vec::as_slice),
        Some(["src/runtime/synthetic.rs#emit_maybe".to_string()].as_slice()),
        "a gate that can compile in production must keep its send in the denominator"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// The unresolved-send exemption keyed on the enclosing signature alone, so a
/// forwarder that sent some *other* expression shipped a request with no row
/// and no finding.
#[test]
fn a_forwarder_sending_a_different_expression_fails_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r"
impl Server {
    fn dispatch(&self, method: &str, params: Value) -> io::Result<()> {
        let computed_method = rewrite(method);
        self.send_request(computed_method, params)
    }
}
",
    )?;

    assert!(emitted.is_empty(), "{emitted:?}");
    assert_eq!(
        findings,
        vec!["emission-unresolved".to_string()],
        "only the declared forwarding parameter is exempt; any other expression is a finding"
    );
    Ok(())
}

/// A helper may take `method: &str`, never hand it to a sender, and still send
/// a method of its own. Promoting it on the strength of the parameter alone let
/// its callers' unrelated arguments enter discovery as emitted methods, with no
/// finding to mark the guess.
#[test]
fn a_helper_that_sends_without_forwarding_its_parameter_does_not_promote_callers()
-> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r#"
impl Server {
    fn log_and_send(&self, method: &str, params: Value) -> io::Result<()> {
        record(method);
        self.send_request("real/method", params)
    }
    fn caller(&self) -> io::Result<()> {
        self.log_and_send("phantom/method", Value::Null)
    }
}
"#,
    )?;

    assert!(
        !emitted.contains_key("phantom/method"),
        "a caller argument is not an emitted method when the helper never forwards it: {emitted:?}"
    );
    assert!(
        emitted.contains_key("real/method"),
        "the helper's own send is still discovered: {emitted:?}"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// `other_method: &str` contains `method: &str`, so a helper that forwards
/// nothing of the kind was read as a forwarder.
#[test]
fn a_similarly_named_parameter_is_not_the_forwarding_parameter()
-> Result<(), Box<dyn std::error::Error>> {
    let (_emitted, findings) = scan_synthetic(
        r"
impl Server {
    fn relay(&self, other_method: &str, params: Value) -> io::Result<()> {
        self.send_request(other_method, params)
    }
}
",
    )?;

    assert_eq!(
        findings,
        vec!["emission-unresolved".to_string()],
        "`other_method: &str` is not `method: &str`; the send must fail closed"
    );
    Ok(())
}

/// A send quoted in a trailing comment, a block comment, or a string literal is
/// not evidence a request is still emitted — but it kept a stale row alive.
#[test]
fn sender_shaped_text_outside_code_is_not_an_emission() -> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r####"
impl Server {
    pub fn documented(&self) {
        let removed = "self.send_request(\"workspace/codeLens/refresh\", p)";
        let raw = r#"self.send_request("workspace/inlayHint/refresh", p)"#;
        let quote = '"';
        // self.send_request("workspace/diagnostic/refresh", p)
        let live = 1; // self.send_request("workspace/foldingRange/refresh", p)
        /* self.send_request("workspace/semanticTokens/refresh", p) */
    }
}
"####,
    )?;

    assert!(emitted.is_empty(), "no comment or string may register as emission: {emitted:?}");
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

// ── Falsifiers for the fourth review round (pre-fix) ────────────────────

/// A string containing `fn ` invented a declaration boundary, so a send was
/// attributed to an owner that does not exist.
#[test]
fn a_string_containing_fn_does_not_invent_an_emitter_owner()
-> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r#"
impl Server {
    pub fn real_owner(&self) {
        let doc = "fn fake_owner(&self)";
        self.send_request("workspace/codeLens/refresh", p);
    }
}
"#,
    )?;

    assert_eq!(
        emitted.get("workspace/codeLens/refresh").map(Vec::as_slice),
        Some(["src/runtime/synthetic.rs#real_owner".to_string()].as_slice()),
        "attribution must stay with the real enclosing function"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// A forwarding wrapper and its literal caller in different files: the
/// per-file forwarder set could not connect them, so the request escaped.
#[test]
fn a_cross_file_forwarder_still_exposes_its_callers() -> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic_files(&[
        (
            "outbound.rs",
            r"
impl Server {
    pub fn dispatch(&self, method: &str, params: Value) -> io::Result<()> {
        self.send_request(method, params)
    }
}
",
        ),
        (
            "caller.rs",
            r#"
impl Server {
    pub fn refresh(&self) {
        self.dispatch("workspace/codeLens/refresh", p);
    }
}
"#,
        ),
    ])?;

    assert_eq!(
        emitted.get("workspace/codeLens/refresh").map(Vec::as_slice),
        Some(["src/runtime/caller.rs#refresh".to_string()].as_slice()),
        "a wrapper in another file is still a send site for its callers"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}

/// A source file the reader cannot parse is an instrument failure, not an
/// empty file. Silence from the scanner must never read as absence.
#[test]
fn an_unparsable_source_file_is_a_finding() -> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic("impl Server { pub fn broken(&self) { (( }")?;

    assert!(emitted.is_empty(), "{emitted:?}");
    assert_eq!(
        findings,
        vec!["emission-source-unparsable".to_string()],
        "a file the reader cannot read is a finding, not a silent skip"
    );
    Ok(())
}

/// `advertised_not_proven` asserts the method is advertised. A catalog that
/// records it as unadvertised contradicts the row, exactly as it would for
/// `supported`.
#[test]
fn advertised_credit_contradicting_the_catalog_fails() {
    let mut discovered = agreeing_discovery();
    let mut unadvertised = catalog("LSP 3.16");
    unadvertised.advertised = false;
    discovered.catalog_rows.insert("lsp.code_lens_refresh".to_string(), unadvertised);

    assert!(
        rules(vec![passing_row()], &discovered).contains(&"disposition-contradicts-catalog"),
        "a row may not claim advertised credit the catalog withholds"
    );
}

/// The mirror is deliberately absent: `helper_only_unadvertised` claims less
/// than the catalog records, and this gate stops a row overstating its surface,
/// not understating it. This control pins that as a decision rather than an
/// oversight — if the rule is added later, this test is the thing that fails.
#[test]
fn helper_only_credit_is_not_checked_against_catalog_advertisement() {
    let mut row = passing_row();
    row.disposition = "helper_only_unadvertised".to_string();
    row.emission = "not_emitted".to_string();
    row.emitters = Vec::new();
    let mut discovered = agreeing_discovery();
    discovered.emitted.clear();

    assert!(
        !rules(vec![row], &discovered).contains(&"disposition-contradicts-catalog"),
        "understating a surface is not the failure this gate is for"
    );
}

/// A complete `#[cfg(test)]` attribute quoted in a string once steered the
/// text-based gate stripper and erased the production code after it. A parser
/// cannot be steered by a string's contents; this pins that.
#[test]
fn a_quoted_cfg_attribute_cannot_erase_production_code() -> Result<(), Box<dyn std::error::Error>> {
    let (emitted, findings) = scan_synthetic(
        r####"
impl Server {
    pub fn documented(&self) {
        let ordinary = "#[cfg(test)] mod hidden { fn x() {} }";
        let raw = r#"#[cfg(test)] mod also_hidden;"#;
        self.send_request("workspace/codeLens/refresh", p);
    }
}
"####,
    )?;

    assert_eq!(
        emitted.get("workspace/codeLens/refresh").map(Vec::as_slice),
        Some(["src/runtime/synthetic.rs#documented".to_string()].as_slice()),
        "a quoted attribute may not remove the production send that follows it"
    );
    assert!(findings.is_empty(), "{findings:?}");
    Ok(())
}
