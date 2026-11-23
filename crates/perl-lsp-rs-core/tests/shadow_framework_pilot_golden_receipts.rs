//! Golden-receipt differential proof for #9086 pilot migration.
//!
//! Fixtures under `tests/fixtures/shadow_framework_pilot/` were captured from the
//! pre-migration shadow entry points and must stay byte-identical after routing
//! through `run_shadow_compare`.

use perl_lsp_rs_core::providers::diagnostics::diagnostics_shadow::{
    DiagnosticAction, DiagnosticClassification, diagnostics_undefined_symbol_cutover,
    diagnostics_undefined_symbol_shadow,
};
use perl_lsp_rs_core::providers::navigation::safe_delete_shadow::{
    SafeDeleteCutoverResult, safe_delete_cutover, safe_delete_shadow,
};
use perl_semantic_facts::{
    AnchorId, Confidence, DefinitionCandidate, DefinitionRank, DefinitionRankReason, EntityId,
    EntityKind, FileId, PlanBlocker, PlanBlockerReason, Provenance, SafeDeletePlan, ScopeId,
};
use perl_workspace::semantic::queries::{DynamicCallableEvidence, QueryContext, SemanticQueries};
use perl_workspace::semantic_shadow_compare::SemanticShadowCompareReceipt;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/shadow_framework_pilot")
}

fn assert_receipt_matches_golden(
    receipt: &SemanticShadowCompareReceipt,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = fixture_dir().join(format!("{name}.json"));
    let actual_bytes = serde_json::to_vec_pretty(receipt)?;
    if std::env::var("UPDATE_SHADOW_PILOT_GOLDENS").as_deref() == Ok("1") {
        std::fs::create_dir_all(fixture_dir())?;
        std::fs::write(&path, &actual_bytes)?;
        return Ok(());
    }
    let expected_bytes = std::fs::read(&path)?;
    assert_eq!(
        actual_bytes, expected_bytes,
        "receipt {name} drifted from pre-migration golden fixture bytes"
    );
    Ok(())
}

struct PilotStub {
    safe_delete_plan: SafeDeletePlan,
    definitions: Vec<DefinitionCandidate>,
}

impl SemanticQueries for PilotStub {
    fn symbol_at(
        &self,
        _file_id: FileId,
        _byte_offset: u32,
    ) -> Option<(perl_semantic_facts::EntityFact, perl_semantic_facts::OccurrenceFact)> {
        None
    }

    fn definitions(&self, _symbol: &str, _context: &QueryContext) -> Vec<DefinitionCandidate> {
        self.definitions.clone()
    }

    fn references(&self, _entity_id: EntityId) -> Vec<perl_semantic_facts::OccurrenceFact> {
        Vec::new()
    }

    fn visible_symbols_at(
        &self,
        _file_id: FileId,
        _byte_offset: u32,
        _scope_id: Option<ScopeId>,
    ) -> Vec<perl_semantic_facts::VisibleSymbol> {
        Vec::new()
    }

    fn method_candidates(
        &self,
        _receiver_package: &str,
        _method_name: &str,
    ) -> Vec<DefinitionCandidate> {
        Vec::new()
    }

    fn rename_plan(&self, entity_id: EntityId, new_name: &str) -> perl_semantic_facts::RenamePlan {
        perl_semantic_facts::RenamePlan::new(
            entity_id,
            String::new(),
            new_name.to_string(),
            vec![],
            vec![],
            vec![],
        )
    }

    fn safe_delete_plan(&self, _entity_id: EntityId) -> SafeDeletePlan {
        self.safe_delete_plan.clone()
    }

    fn dynamic_boundary_at(
        &self,
        _file_id: FileId,
        _byte_offset: u32,
        _symbol: Option<&str>,
    ) -> Option<perl_semantic_facts::OccurrenceFact> {
        None
    }

    fn dynamic_callable_may_be_visible_at(
        &self,
        _file_id: FileId,
        _byte_offset: u32,
        _symbol: &str,
    ) -> Option<DynamicCallableEvidence> {
        None
    }
}

fn empty_safe_delete_plan() -> SafeDeletePlan {
    SafeDeletePlan::new(EntityId(1), "my_sub".to_string(), vec![], vec![])
}

fn blocked_safe_delete_plan() -> SafeDeletePlan {
    SafeDeletePlan::new(
        EntityId(1),
        "my_sub".to_string(),
        vec![PlanBlocker::new(
            PlanBlockerReason::ReferencesExist,
            None,
            "references exist".to_string(),
        )],
        vec![],
    )
}

fn undefined_symbol_candidate() -> DefinitionCandidate {
    DefinitionCandidate::new(
        EntityId(2),
        AnchorId(10),
        "defined_sub".to_string(),
        "defined_sub".to_string(),
        None,
        EntityKind::Subroutine,
        Provenance::ExactAst,
        Confidence::High,
        DefinitionRank::ExactQualified,
        DefinitionRankReason::ExactQualifiedName,
    )
}

#[test]
fn safe_delete_shadow_allowed_golden_receipt() -> Result<(), Box<dyn std::error::Error>> {
    let stub = PilotStub { safe_delete_plan: empty_safe_delete_plan(), definitions: vec![] };
    let result = safe_delete_shadow(true, &stub, EntityId(1), "my_sub");
    assert!(result.legacy_allowed);
    assert_receipt_matches_golden(&result.receipt, "safe_delete_shadow_allowed")?;
    Ok(())
}

#[test]
fn safe_delete_shadow_legacy_unavailable_golden_receipt() -> Result<(), Box<dyn std::error::Error>>
{
    let stub = PilotStub { safe_delete_plan: blocked_safe_delete_plan(), definitions: vec![] };
    let result = safe_delete_shadow(false, &stub, EntityId(1), "my_sub");
    assert!(!result.legacy_allowed);
    assert_receipt_matches_golden(&result.receipt, "safe_delete_shadow_legacy_unavailable")?;
    Ok(())
}

#[test]
fn safe_delete_cutover_blocked_golden_receipt() -> Result<(), Box<dyn std::error::Error>> {
    let stub = PilotStub { safe_delete_plan: blocked_safe_delete_plan(), definitions: vec![] };
    let outcome = safe_delete_cutover(true, &stub, EntityId(1), "my_sub");
    assert!(matches!(outcome.result, SafeDeleteCutoverResult::Blocked { .. }));
    assert_receipt_matches_golden(&outcome.receipt, "safe_delete_cutover_blocked")?;
    Ok(())
}

#[test]
fn diagnostics_shadow_exact_warn_golden_receipt() -> Result<(), Box<dyn std::error::Error>> {
    let stub = PilotStub { safe_delete_plan: empty_safe_delete_plan(), definitions: vec![] };
    let result =
        diagnostics_undefined_symbol_shadow(false, &stub, "missing_sub", FileId(1), None, 12);
    assert!(!result.legacy_should_warn);
    assert_receipt_matches_golden(&result.receipt, "diagnostics_shadow_exact_warn")?;
    Ok(())
}

#[test]
fn diagnostics_cutover_dynamic_scope_golden_receipt() -> Result<(), Box<dyn std::error::Error>> {
    let stub = PilotStub {
        safe_delete_plan: empty_safe_delete_plan(),
        definitions: vec![undefined_symbol_candidate()],
    };
    let outcome =
        diagnostics_undefined_symbol_cutover(true, &stub, "defined_sub", FileId(1), None, 5, true);
    assert_eq!(outcome.action, DiagnosticAction::Suppress);
    assert_eq!(outcome.classification, DiagnosticClassification::DynamicOrUnavailable);
    assert_receipt_matches_golden(&outcome.receipt, "diagnostics_cutover_dynamic_scope")?;
    Ok(())
}

#[test]
fn diagnostics_cutover_defined_symbol_golden_receipt() -> Result<(), Box<dyn std::error::Error>> {
    let stub = PilotStub {
        safe_delete_plan: empty_safe_delete_plan(),
        definitions: vec![undefined_symbol_candidate()],
    };
    let outcome =
        diagnostics_undefined_symbol_cutover(true, &stub, "defined_sub", FileId(1), None, 5, false);
    assert_eq!(outcome.action, DiagnosticAction::Suppress);
    assert_receipt_matches_golden(&outcome.receipt, "diagnostics_cutover_defined_symbol")?;
    Ok(())
}
