//! Closed-vocabulary and source-kind compatibility controls for #8561.
use perl_pragma::compile_environment::{BoundaryDisposition, BoundaryReason};
use perl_semantic_facts::{BoundaryKind, CompileEffectSourceKind, SemanticReasonCode};
use serde_json::Value;

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn require(value: bool, message: &str) -> TestResult {
    if value { Ok(()) } else { Err(message.into()) }
}
fn vocabulary() -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(include_str!("fixtures/compile_boundary_vocabulary.json"))?)
}

#[test]
fn all_eight_dispositions_roundtrip_without_collapsing() -> TestResult {
    let fixture = vocabulary()?;
    let rows =
        fixture.get("dispositions").and_then(Value::as_array).ok_or("dispositions missing")?;
    require(rows.len() == 8, "disposition denominator changed")?;
    let mut values = Vec::new();
    for row in rows {
        let value: BoundaryDisposition = serde_json::from_value(row.clone())?;
        require(serde_json::to_value(value)? == *row, "disposition token changed")?;
        require(!values.contains(&value), "distinct dispositions collapsed")?;
        values.push(value);
    }
    require(
        serde_json::from_str::<BoundaryDisposition>("\"future_disposition\"").is_err(),
        "unknown disposition accepted",
    )
}

#[test]
fn detailed_reason_bridge_preserves_missing_authority() -> TestResult {
    let fixture = vocabulary()?;
    let rows = fixture.get("reasons").and_then(Value::as_array).ok_or("reasons missing")?;
    require(rows.len() == 29, "reason denominator changed")?;
    for row in rows {
        let token = row.get("token").ok_or("token missing")?;
        let reason: BoundaryReason = serde_json::from_value(token.clone())?;
        require(serde_json::to_value(reason)? == *token, "reason token changed")?;
        let semantic: Option<SemanticReasonCode> = serde_json::from_value(
            row.get("semantic_reason").ok_or("semantic bridge missing")?.clone(),
        )?;
        let kind: Option<BoundaryKind> = serde_json::from_value(
            row.get("boundary_kind").ok_or("boundary bridge missing")?.clone(),
        )?;
        require(reason.semantic_reason() == semantic, "semantic reason authority changed")?;
        require(reason.boundary_kind() == kind, "boundary kind authority changed")?;
    }
    for unknown in ["future_reason", "DynamicRequire", "", "dynamic-require"] {
        require(
            serde_json::from_value::<BoundaryReason>(Value::String(unknown.into())).is_err(),
            "unknown/noncanonical reason accepted",
        )?;
    }
    Ok(())
}

#[test]
fn source_kind_extraction_preserves_hir_type_and_exact_wire_tokens() -> TestResult {
    use perl_parser_core::hir::CompileEffectSourceKind as HirKind;
    let expected = [
        (HirKind::PackageDecl, "package_decl"),
        (HirKind::SubDecl, "sub_decl"),
        (HirKind::MethodDecl, "method_decl"),
        (HirKind::VariableDecl, "variable_decl"),
        (HirKind::UseDirective, "use_directive"),
        (HirKind::NoDirective, "no_directive"),
        (HirKind::RequireDirective, "require_directive"),
        (HirKind::PhaseBlock, "phase_block"),
        (HirKind::SymbolicReferenceDeref, "symbolic_reference_deref"),
        (HirKind::Assignment, "assignment"),
        (HirKind::TypeglobAssignment, "typeglob_assignment"),
        (HirKind::ScopeGraph, "scope_graph"),
        (HirKind::StashGraph, "stash_graph"),
        (HirKind::CompileEnvironment, "compile_environment"),
    ];
    for (hir, token) in expected {
        let shared: CompileEffectSourceKind = hir;
        require(
            serde_json::to_value(shared)? == Value::String(token.into()),
            "source kind token changed",
        )?;
        let decoded: CompileEffectSourceKind = serde_json::from_value(Value::String(token.into()))?;
        let back: HirKind = decoded;
        require(back == hir, "HIR reexport type/variant changed")?;
    }
    require(
        serde_json::from_str::<CompileEffectSourceKind>("\"future_source_kind\"").is_err(),
        "future source silently relabeled",
    )
}
