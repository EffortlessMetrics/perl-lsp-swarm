//! Side-effect-free adapters from current HIR call shells into [`CallSemantics`].
//!
//! Flat HIR retains name/count/`object_kind` shells, not operand identities.
//! Body HIR retains argument IDs (and folds the receiver into `args[0]` for
//! method/indirect forms) but drops callee names. Adapters record those losses
//! as typed unknown/unavailable state. They never scan source text.

use perl_ast::{
    ArgumentSource, CODEREF_CALL_SENTINEL, CallBoundary, CallLimitation, CallOperand,
    CallSemantics, CallSyntax, CalleeIdentity, CurrentRepresentation, DispatchKind,
    EvaluationContract, OperandContext, OperandList, OperandPresence, OperandRole,
    PrototypeDisposition,
};

use super::{
    CallExpr, CallForm, DynamicBoundaryKind, HirBody, HirExpr, HirExprId, HirItem, HirKind,
    IndirectCallExpr, MethodCallExpr, RecoveryConfidence,
};
use crate::SourceLocation;

/// Adapt a flat-HIR item if it is a call shell or a coderef dynamic boundary.
#[must_use]
pub fn adapt_flat_hir_call(item: &HirItem) -> Option<CallSemantics> {
    match &item.kind {
        HirKind::CallExpr(call) => Some(adapt_call_expr(item, call)),
        HirKind::MethodCallExpr(call) => Some(adapt_method_expr(item, call)),
        HirKind::IndirectCallExpr(call) => Some(adapt_indirect_expr(item, call)),
        HirKind::DynamicBoundary(boundary) if boundary.kind == DynamicBoundaryKind::CoderefCall => {
            Some(adapt_coderef_boundary(item))
        }
        _ => None,
    }
}

/// Adapt a body-HIR expression if it is [`HirExpr::Call`].
#[must_use]
pub fn adapt_body_hir_call(body: &HirBody, expr_id: HirExprId) -> Option<CallSemantics> {
    let expr = body.expr(expr_id)?;
    let HirExpr::Call { args, ast_kind, callee_span } = expr else {
        return None;
    };
    let call_range = body.source_map.expr_range(expr_id)?;
    Some(adapt_body_call(call_range, ast_kind, args, *callee_span, body))
}

fn adapt_call_expr(item: &HirItem, call: &CallExpr) -> CallSemantics {
    let recovered = item.recovery_confidence != RecoveryConfidence::Parsed;
    let (syntax, dispatch, callee) = match call.form {
        CallForm::Coderef => (
            CallSyntax::Coderef,
            if recovered { DispatchKind::Boundary } else { DispatchKind::Coderef },
            if call.name == CODEREF_CALL_SENTINEL {
                CalleeIdentity::SentinelCoderef
            } else {
                CalleeIdentity::Named { name: call.name.clone() }
            },
        ),
        CallForm::NamedFunction => {
            let core_qualified = call.name.starts_with("CORE::");
            (
                if core_qualified { CallSyntax::CoreQualified } else { CallSyntax::Named },
                if recovered { DispatchKind::Boundary } else { DispatchKind::Function },
                CalleeIdentity::Named { name: call.name.clone() },
            )
        }
    };
    CallSemantics::compose(
        syntax,
        dispatch,
        callee,
        OperandPresence::Unknown,
        item.range,
        item.anchor.name_range,
        OperandPresence::Unknown,
        OperandList::Unavailable { known_count: Some(call.arg_count) },
        OperandList::Unavailable { known_count: None },
        OperandPresence::Unknown,
        ArgumentSource::Unknown,
        PrototypeDisposition::Unknown,
        EvaluationContract::Unknown,
        if recovered { CallBoundary::Recovered } else { CallBoundary::Partial },
        vec![
            CallLimitation::OperandsUnavailableInRepresentation,
            CallLimitation::CalleeSpanUnavailable,
            CallLimitation::NamedParensFormUnavailable,
            CallLimitation::EvaluationOrderUnknown,
        ],
        CurrentRepresentation::HirCallExpr,
    )
}

fn adapt_method_expr(item: &HirItem, call: &MethodCallExpr) -> CallSemantics {
    let recovered = item.recovery_confidence != RecoveryConfidence::Parsed;
    let _ = call.object_kind;
    CallSemantics::compose(
        CallSyntax::MethodArrow,
        if recovered { DispatchKind::Boundary } else { DispatchKind::Method },
        CalleeIdentity::Method { name: call.method.clone() },
        OperandPresence::Unknown,
        item.range,
        item.anchor.name_range,
        OperandPresence::Unknown,
        OperandList::Unavailable { known_count: Some(call.arg_count) },
        OperandList::Unavailable { known_count: None },
        OperandPresence::Unknown,
        ArgumentSource::Unknown,
        PrototypeDisposition::Unknown,
        EvaluationContract::Unknown,
        if recovered { CallBoundary::Recovered } else { CallBoundary::Partial },
        vec![
            CallLimitation::OperandsUnavailableInRepresentation,
            CallLimitation::ObjectKindIsClassificationNotOperand,
            CallLimitation::EvaluationOrderUnknown,
        ],
        CurrentRepresentation::HirMethodCallExpr,
    )
}

fn adapt_indirect_expr(item: &HirItem, call: &IndirectCallExpr) -> CallSemantics {
    let recovered = item.recovery_confidence != RecoveryConfidence::Parsed;
    let print_like = matches!(call.method.as_str(), "print" | "printf" | "say");
    let _ = call.object_kind;
    CallSemantics::compose(
        if print_like { CallSyntax::PrintFilehandleLike } else { CallSyntax::Indirect },
        if recovered {
            DispatchKind::Boundary
        } else if print_like {
            DispatchKind::Builtin
        } else {
            DispatchKind::IndirectObject
        },
        CalleeIdentity::Method { name: call.method.clone() },
        OperandPresence::Unknown,
        item.range,
        item.anchor.name_range,
        OperandPresence::Unknown,
        OperandList::Unavailable { known_count: Some(call.arg_count) },
        OperandList::Unavailable { known_count: None },
        OperandPresence::Unknown,
        ArgumentSource::Unknown,
        PrototypeDisposition::Unknown,
        EvaluationContract::Unknown,
        if recovered { CallBoundary::Recovered } else { CallBoundary::Partial },
        vec![
            CallLimitation::OperandsUnavailableInRepresentation,
            CallLimitation::ObjectKindIsClassificationNotOperand,
            CallLimitation::EvaluationOrderUnknown,
        ],
        CurrentRepresentation::HirIndirectCallExpr,
    )
}

fn adapt_coderef_boundary(item: &HirItem) -> CallSemantics {
    CallSemantics::compose(
        CallSyntax::Coderef,
        DispatchKind::Boundary,
        CalleeIdentity::SentinelCoderef,
        OperandPresence::Unknown,
        item.range,
        item.anchor.name_range,
        OperandPresence::Unknown,
        OperandList::Unavailable { known_count: None },
        OperandList::Unavailable { known_count: None },
        OperandPresence::Unknown,
        ArgumentSource::Unknown,
        PrototypeDisposition::NotApplicable,
        EvaluationContract::Unknown,
        CallBoundary::Dynamic,
        vec![
            CallLimitation::OperandsUnavailableInRepresentation,
            CallLimitation::CoderefSentinelNotCanonical,
            CallLimitation::EvaluationOrderUnknown,
        ],
        CurrentRepresentation::HirDynamicBoundary,
    )
}

fn adapt_body_call(
    call_range: SourceLocation,
    ast_kind: &str,
    args: &[HirExprId],
    callee_span: Option<SourceLocation>,
    body: &HirBody,
) -> CallSemantics {
    let recovered = ast_kind == "Error";
    let (syntax, dispatch, callee, receiver_is_first) = match ast_kind {
        "MethodCall" => {
            (CallSyntax::MethodArrow, DispatchKind::Method, CalleeIdentity::Unknown, true)
        }
        "IndirectCall" => {
            (CallSyntax::Indirect, DispatchKind::IndirectObject, CalleeIdentity::Unknown, true)
        }
        "AmperCall" => {
            (CallSyntax::Ampersand, DispatchKind::Function, CalleeIdentity::Unknown, false)
        }
        _ => (CallSyntax::Named, DispatchKind::Function, CalleeIdentity::Unknown, false),
    };

    let mut source_order = 0_u32;
    let mut argument_operands = Vec::new();
    let mut arg_iter = args.iter();
    let (receiver, filehandle);

    if receiver_is_first {
        match arg_iter.next() {
            Some(id) => {
                receiver = OperandPresence::Present(operand_from_hir_id(
                    body,
                    *id,
                    OperandRole::Receiver,
                    source_order,
                ));
                source_order = source_order.saturating_add(1);
            }
            None => receiver = OperandPresence::Unknown,
        }
        filehandle = OperandPresence::Absent;
    } else {
        receiver = OperandPresence::Absent;
        filehandle = OperandPresence::Absent;
    }

    for id in arg_iter {
        argument_operands.push(operand_from_hir_id(body, *id, OperandRole::Argument, source_order));
        source_order = source_order.saturating_add(1);
    }

    let argument_source = if argument_operands.is_empty() {
        ArgumentSource::Unknown
    } else {
        ArgumentSource::ExplicitList
    };

    let mut limitations =
        vec![CallLimitation::CalleeNameUnavailable, CallLimitation::EvaluationOrderUnknown];
    if callee_span.is_none() {
        limitations.push(CallLimitation::CalleeSpanUnavailable);
    }
    if ast_kind == "AmperCall" {
        limitations.push(CallLimitation::AmpersandParensFormUnavailable);
    }
    if recovered {
        limitations.push(CallLimitation::RecoveredSource);
    }

    CallSemantics::compose(
        syntax,
        if recovered { DispatchKind::Boundary } else { dispatch },
        callee,
        OperandPresence::Unknown,
        call_range,
        callee_span,
        receiver,
        OperandList::Explicit(argument_operands),
        OperandList::Unavailable { known_count: None },
        filehandle,
        argument_source,
        PrototypeDisposition::Unknown,
        EvaluationContract::Unknown,
        if recovered { CallBoundary::Recovered } else { CallBoundary::Partial },
        limitations,
        CurrentRepresentation::HirBodyCall,
    )
}

fn operand_from_hir_id(
    body: &HirBody,
    id: HirExprId,
    role: OperandRole,
    source_order: u32,
) -> CallOperand {
    CallOperand::new(
        body.source_map.expr_range(id),
        role,
        source_order,
        EvaluationContract::Unknown,
        OperandContext::Unknown,
    )
}
