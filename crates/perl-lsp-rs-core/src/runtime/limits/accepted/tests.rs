//! Discriminating proof for the accepted runtime-limit view and denominator.

use std::time::Duration;

use perl_test_must::{must, must_err, must_some_with, must_with};

use super::super::{LspLimits, MemoryBudget};
use super::{
    AcceptedLimitsAssembly, ConfigurationGenerationIdentity, DenominatorRow, DomainOwner,
    EnvelopeOwner, HardEnvelopeIdentity, LimitConsumerState, LimitLookup, LimitScope,
    RUNTIME_LIMITS_DENOMINATOR, RelationalValidation, ResultCap, RuntimeLimitsChange,
    RuntimeLimitsFamily, ScopeIdentity, denominator_violations,
};

fn assemble(limits: &LspLimits) -> super::AcceptedRuntimeLimitsView {
    must_with(
        AcceptedLimitsAssembly::not_proven_prerequisites(ScopeIdentity::Global).assemble(limits),
        "default snapshot assembles",
    )
}

fn live_row() -> DenominatorRow {
    *must_some_with(
        RUNTIME_LIMITS_DENOMINATOR.iter().find(|row| row.id == "limits.references_cap.handler"),
        "references live row exists",
    )
}

#[test]
fn current_denominator_is_closed_and_stable_across_two_checks() {
    let first = denominator_violations(RUNTIME_LIMITS_DENOMINATOR);
    let second = denominator_violations(RUNTIME_LIMITS_DENOMINATOR);
    assert!(first.is_empty(), "denominator violations: {first:?}");
    assert_eq!(first, second, "denominator check must be deterministic");
}

#[test]
fn dropping_a_public_limit_field_fails_the_denominator() {
    let mut rows: Vec<_> = RUNTIME_LIMITS_DENOMINATOR.to_vec();
    rows.retain(|row| row.rust_field != Some("workspace_symbol_cap"));
    let violations = denominator_violations(&rows);
    assert!(
        violations.iter().any(|entry| entry.contains("workspace_symbol_cap")
            && entry.contains("absent from the denominator")),
        "missing public field must fail: {violations:?}"
    );
}

#[test]
fn dropping_an_accessor_row_fails_the_denominator() {
    let mut rows: Vec<_> = RUNTIME_LIMITS_DENOMINATOR.to_vec();
    rows.retain(|row| row.accessor != Some("diagnostics_per_file_cap"));
    let violations = denominator_violations(&rows);
    assert!(
        violations.iter().any(|entry| entry.contains("diagnostics_per_file_cap")
            && entry.contains("has no denominator row")),
        "missing accessor must fail: {violations:?}"
    );
}

#[test]
fn parsed_no_consumer_marked_live_without_a_site_fails() {
    let mut rows: Vec<_> = RUNTIME_LIMITS_DENOMINATOR.to_vec();
    let position = must_some_with(
        rows.iter().position(|row| row.id == "limits.diagnostics_per_file_cap.unconsumed"),
        "diagnostics unconsumed row",
    );
    rows[position].state = LimitConsumerState::Live;
    let violations = denominator_violations(&rows);
    assert!(
        violations.iter().any(|entry| entry.contains("live without a production site")),
        "parsed field with zero readers must not be behavior-backed: {violations:?}"
    );
}

#[test]
fn absorbing_an_ai_limit_as_live_fails() {
    let mut rows: Vec<_> = RUNTIME_LIMITS_DENOMINATOR.to_vec();
    let position = must_some_with(
        rows.iter().position(|row| row.id == "adjacent.ai.timeout_ms"),
        "ai timeout row",
    );
    rows[position].state = LimitConsumerState::Live;
    rows[position].production_site = Some("crates/perl-lsp-rs-core/src/runtime/limits/mod.rs");
    rows[position].production_marker = Some("pub struct LspLimits");
    let violations = denominator_violations(&rows);
    assert!(
        violations.iter().any(|entry| entry.contains("non-runtime owner as live")
            || entry.contains("absorbed as live")),
        "AI budgets must not be absorbed: {violations:?}"
    );
}

#[test]
fn registering_a_stale_schema_key_fails() {
    let mut rows: Vec<_> = RUNTIME_LIMITS_DENOMINATOR.to_vec();
    let mut phantom = live_row();
    phantom.id = "limits.historical_indexed_file_cap";
    phantom.test_id = "stale_historical_field";
    phantom.schema_key = Some("indexedFileCap");
    phantom.rust_field = None;
    phantom.accessor = None;
    rows.push(phantom);
    let violations = denominator_violations(&rows);
    assert!(
        violations.iter().any(|entry| entry.contains("indexedFileCap")),
        "stale historical field must fail: {violations:?}"
    );
}

#[test]
fn identical_accepted_input_classifies_as_unchanged() {
    let view = assemble(&LspLimits::default());
    assert_eq!(view.classify_change(&view), RuntimeLimitsChange::Unchanged);
    let again = assemble(&LspLimits::default());
    assert_eq!(view.classify_change(&again), RuntimeLimitsChange::Unchanged);
    assert_eq!(view.fingerprint(), again.fingerprint());
}

#[test]
fn read_and_deadline_changes_compose_deterministically() {
    let base = LspLimits::default();
    let left = assemble(&base);
    let mut changed = base.clone();
    changed.workspace_symbol_cap = 10;
    changed.completion_deadline = Duration::from_millis(1);
    let right = assemble(&changed);
    assert_eq!(
        left.classify_change(&right),
        RuntimeLimitsChange::Composite(vec![
            RuntimeLimitsFamily::ReadResultCaps,
            RuntimeLimitsFamily::ProviderDeadlines,
        ])
    );
}

#[test]
fn presentation_change_is_not_a_read_query_change() {
    let base = LspLimits::default();
    let left = assemble(&base);
    let mut changed = base.clone();
    changed.code_lens_cap = 3;
    let right = assemble(&changed);
    assert_eq!(
        left.classify_change(&right),
        RuntimeLimitsChange::Family(RuntimeLimitsFamily::PresentationResultCaps)
    );
}

#[test]
fn global_and_root_scopes_do_not_collapse() {
    let limits = LspLimits::default();
    let global = assemble(&limits);
    let root = must_with(
        AcceptedLimitsAssembly::not_proven_prerequisites(must_with(
            ScopeIdentity::root("file:///workspace/root"),
            "root identity",
        ))
        .assemble(&limits),
        "root-scoped view assembles",
    );
    assert_ne!(global, root);
    assert_ne!(global.fingerprint(), root.fingerprint());
    assert!(global.scope().is_global());
    assert!(!root.scope().is_global());
}

#[test]
fn root_identity_does_not_retain_private_paths() {
    let root = must_with(ScopeIdentity::root("/home/secret-user/private-project"), "root identity");
    let rendered = format!("{root:?}");
    assert!(!rendered.contains("secret-user"));
    assert!(!rendered.contains("private-project"));
    assert!(!rendered.contains("/home/"));
}

#[test]
fn inverted_memory_thresholds_are_invalid_and_not_repaired() {
    let limits = LspLimits {
        memory_budget: MemoryBudget {
            warning_threshold_bytes: 8,
            critical_threshold_bytes: 4,
            ..MemoryBudget::default()
        },
        ..LspLimits::default()
    };
    let view = assemble(&limits);
    assert_eq!(view.relational(), RelationalValidation::Invalid { group: "memory_thresholds" });
    assert!(
        matches!(
            view.memory_cache().warning_threshold,
            LimitLookup::ParsedNoConsumer { snapshot } if snapshot.get() == 8
        ),
        "warning snapshot must be preserved: {:?}",
        view.memory_cache().warning_threshold
    );
    assert!(
        matches!(
            view.memory_cache().critical_threshold,
            LimitLookup::ParsedNoConsumer { snapshot } if snapshot.get() == 4
        ),
        "critical snapshot must be preserved: {:?}",
        view.memory_cache().critical_threshold
    );
}

#[test]
fn zero_cap_is_accepted_zero_not_missing_instrumentation() {
    let limits = LspLimits { workspace_symbol_cap: 0, ..LspLimits::default() };
    let view = assemble(&limits);
    assert!(
        matches!(
            view.result_caps().workspace_symbols,
            LimitLookup::Accepted(cap) if cap == ResultCap::from_accepted(0)
        ),
        "zero must remain an accepted cap: {:?}",
        view.result_caps().workspace_symbols
    );
    assert!(view.result_caps().workspace_symbols.is_behavior_backed());
}

#[test]
fn missing_diagnostics_consumer_is_not_behavior_backed() {
    let view = assemble(&LspLimits::default());
    assert!(!view.result_caps().diagnostics_per_file.is_behavior_backed());
    assert_eq!(view.result_caps().diagnostics_per_file.accepted(), None);
    assert!(
        matches!(
            view.result_caps().diagnostics_per_file,
            LimitLookup::ParsedNoConsumer { snapshot } if snapshot == ResultCap::from_accepted(200)
        ),
        "diagnostics must stay parsed-no-consumer: {:?}",
        view.result_caps().diagnostics_per_file
    );
}

#[test]
fn consumer_api_does_not_expose_json_or_snapshot_handles() {
    let view = assemble(&LspLimits::default());
    let _cap: ResultCap = must(view.result_caps().workspace_symbols.accepted().ok_or("cap"));
    let type_name = std::any::type_name_of_val(&view);
    assert!(!type_name.contains("serde_json"));
    assert!(!type_name.contains("LspLimits"));
    assert!(!type_name.contains("Value"));
}

#[test]
fn assembly_copies_magnitudes_and_does_not_borrow_the_snapshot() {
    let mut limits = LspLimits { references_cap: 17, ..LspLimits::default() };
    let view = assemble(&limits);
    limits.references_cap = 99;
    assert_eq!(limits.references_cap, 99);
    assert!(
        matches!(view.result_caps().references, LimitLookup::Accepted(cap) if cap.get() == 17),
        "accepted references cap: {:?}",
        view.result_caps().references
    );
}

#[test]
fn empty_generation_material_fails_closed() {
    assert_eq!(
        must_err(ConfigurationGenerationIdentity::bound("")),
        super::identity::IdentityError::EmptyMaterial
    );
    assert_eq!(
        must_err(HardEnvelopeIdentity::bound(1, "")),
        super::identity::IdentityError::EmptyMaterial
    );
    assert_eq!(must_err(ScopeIdentity::root("")), super::identity::IdentityError::EmptyMaterial);
}

#[test]
fn bound_identities_are_distinct_from_not_proven() {
    let bound =
        must_with(ConfigurationGenerationIdentity::bound("generation-1"), "bound generation");
    let unbound = ConfigurationGenerationIdentity::not_proven("unbound");
    assert_ne!(bound, unbound);
    assert!(bound.fingerprint().is_some());
    assert!(unbound.fingerprint().is_none());
}

#[test]
fn live_rows_stay_global_except_named_operation_scopes() {
    for row in RUNTIME_LIMITS_DENOMINATOR.iter().filter(|row| row.state == LimitConsumerState::Live)
    {
        assert!(
            matches!(row.scope, LimitScope::Global | LimitScope::Operation),
            "{} unexpectedly {:?}",
            row.id,
            row.scope
        );
        assert_eq!(row.envelope_owner, EnvelopeOwner::Issue7479);
        assert_eq!(row.domain_owner, DomainOwner::RuntimeLimits);
    }
}

#[test]
fn memory_warning_and_critical_share_a_relational_group() {
    let warning = RUNTIME_LIMITS_DENOMINATOR
        .iter()
        .find(|row| row.rust_field == Some("warning_threshold_bytes"));
    let critical = RUNTIME_LIMITS_DENOMINATOR
        .iter()
        .find(|row| row.rust_field == Some("critical_threshold_bytes"));
    assert_eq!(warning.map(|row| row.relational_group), Some(Some("memory_thresholds")));
    assert_eq!(critical.map(|row| row.relational_group), Some(Some("memory_thresholds")));
}

#[test]
fn adjacent_rows_are_never_runtime_family_consumers() {
    for row in RUNTIME_LIMITS_DENOMINATOR {
        if row.domain_owner == DomainOwner::RuntimeLimits {
            continue;
        }
        assert_ne!(row.state, LimitConsumerState::Live, "{}", row.id);
        assert!(
            row.view_field.starts_with("adjacent.")
                || row.domain_owner == DomainOwner::WorkspaceIndexedSource,
            "{}",
            row.id
        );
    }
}

#[test]
fn transferred_lsp_limits_fields_are_not_accepted_lookups() {
    let view = assemble(&LspLimits::default());
    assert!(!view.source_admission().max_symbols_per_file.is_behavior_backed());
    assert!(!view.source_admission().parse_storm_threshold.is_behavior_backed());
    assert!(!view.index_io_deadlines().regex_scan.is_behavior_backed());
    assert!(!view.index_io_deadlines().file_index.is_behavior_backed());
    assert!(!view.index_io_deadlines().filesystem.is_behavior_backed());
    assert!(!view.degradation_policy().return_partial_on_timeout.is_behavior_backed());
    assert!(!view.degradation_policy().include_open_docs_when_degraded.is_behavior_backed());
    assert!(
        matches!(view.index_io_deadlines().file_index, LimitLookup::FixedInternal { .. }),
        "no-reader internals are not FixedProduct: {:?}",
        view.index_io_deadlines().file_index
    );
    assert_eq!(view.index_io_deadlines().file_index.accepted(), None);
}

#[test]
fn unconsumed_snapshot_changes_do_not_classify_as_accepted_family_changes() {
    let base = LspLimits::default();
    let left = assemble(&base);
    let mut transferred = base.clone();
    transferred.max_symbols_per_file = base.max_symbols_per_file.saturating_add(1);
    let transferred_view = assemble(&transferred);
    assert_eq!(left.classify_change(&transferred_view), RuntimeLimitsChange::Unchanged);
    assert_ne!(left.fingerprint(), transferred_view.fingerprint());

    let mut parsed = base.clone();
    parsed.diagnostics_per_file_cap = 1;
    let parsed_view = assemble(&parsed);
    assert_eq!(left.classify_change(&parsed_view), RuntimeLimitsChange::Unchanged);
    assert_ne!(left.fingerprint(), parsed_view.fingerprint());

    let mut internal = base.clone();
    internal.file_index_deadline = base.file_index_deadline + Duration::from_nanos(1);
    let internal_view = assemble(&internal);
    assert_eq!(left.classify_change(&internal_view), RuntimeLimitsChange::Unchanged);
    assert_ne!(left.fingerprint(), internal_view.fingerprint());
}

#[test]
fn accepted_file_size_change_still_classifies_source_admission() {
    let base = LspLimits::default();
    let left = assemble(&base);
    let mut changed = base.clone();
    changed.max_file_size_bytes = base.max_file_size_bytes.saturating_add(1);
    let right = assemble(&changed);
    assert_eq!(
        left.classify_change(&right),
        RuntimeLimitsChange::Family(RuntimeLimitsFamily::SourceAdmission)
    );
}

#[test]
fn submillisecond_deadline_change_has_a_distinct_fingerprint() {
    let base = LspLimits::default();
    let left = assemble(&base);
    let mut changed = base.clone();
    changed.completion_deadline = base.completion_deadline + Duration::from_nanos(1);
    let right = assemble(&changed);
    assert_ne!(left.fingerprint(), right.fingerprint());
    assert_eq!(
        left.classify_change(&right),
        RuntimeLimitsChange::Family(RuntimeLimitsFamily::ProviderDeadlines)
    );
}

#[test]
fn definition_only_file_content_validator_is_not_a_live_consumer() {
    let row = must_some_with(
        RUNTIME_LIMITS_DENOMINATOR
            .iter()
            .find(|row| row.id == "limits.file_size_bytes.file_content"),
        "file_content denominator row exists",
    );
    assert_eq!(row.state, LimitConsumerState::ParsedNoConsumer);
    assert!(row.production_site.is_none());
    assert!(row.first_effect.is_none());
}

#[test]
fn schema_generation_is_versioned() {
    let view = assemble(&LspLimits::default());
    assert_eq!(view.schema_generation(), super::ACCEPTED_RUNTIME_LIMITS_SCHEMA_GENERATION);
    assert_eq!(view.schema_generation(), 2);
}

#[test]
fn default_memory_relationship_is_valid() {
    let view = assemble(&LspLimits::default());
    assert_eq!(view.relational(), RelationalValidation::Valid);
}

#[test]
fn not_proven_status_cannot_claim_zero_or_current() {
    let mut rows: Vec<_> = RUNTIME_LIMITS_DENOMINATOR.to_vec();
    let position = must_some_with(
        rows.iter().position(|row| row.id == "adjacent.testing.timeouts"),
        "testing adjacent row",
    );
    rows[position].status_projection = "rendered-current";
    let violations = denominator_violations(&rows);
    assert!(
        violations.iter().any(|entry| entry.contains("renders missing instrumentation")),
        "not_proven must not be projected as current: {violations:?}"
    );
}
