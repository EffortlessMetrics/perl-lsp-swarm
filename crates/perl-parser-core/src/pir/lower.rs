//! HIR-to-PIR v0 lowering.
//!
//! Lowering is intentionally conservative. It lowers the data-access, call, and
//! dynamic-boundary operation families that the current HIR substrate can prove
//! from source, anchors every source-derived node, preserves dynamic-boundary
//! links, and records everything it could not lower in the receipt. It never
//! evaluates Perl and never changes provider behavior.

use std::collections::HashMap;

use crate::hir::{
    AccessMode, AssignMode, CallForm, DynamicBoundaryKind, HirBody, HirBodyId, HirExpr, HirExprId,
    HirFile, HirItem, HirKind, HirScopeId, HirStmt, Sigil, UnaryMode, VariableKind,
    HIR_BODY_MODEL_VERSION,
};

use super::model::{
    LexicalName, PIR_RECEIPT_VERSION, PirAnchorCoverage, PirCallee, PirContext,
    PirDynamicBoundaryKind, PirEdge, PirEdgeKind, PirGraph, PirId, PirLoweringMode, PirMethod,
    PirNode, PirOperation, PirReceipt, PirReceiver, PirSourceAnchor, SymbolName,
};

/// Lower a [`HirFile`] into a PIR v0 graph with no caller-supplied identity.
#[must_use]
pub fn lower_hir(file: &HirFile) -> PirGraph {
    lower_hir_with_identity(file, None)
}

/// Lower a [`HirFile`] into a PIR v0 graph, tagging the receipt with an
/// optional caller-supplied source or fixture identity.
#[must_use]
pub fn lower_hir_with_identity(file: &HirFile, source_identity: Option<String>) -> PirGraph {
    let mut lowerer = Lowerer::new(source_identity);
    for item in &file.items {
        lowerer.lower_item(item);
    }
    lowerer.finish()
}

struct Lowerer {
    nodes: Vec<PirNode>,
    edges: Vec<PirEdge>,
    next_id: u32,
    last_in_scope: HashMap<Option<HirScopeId>, PirId>,
    /// Most recent dynamic-callee boundary HIR emitted, awaiting the coderef
    /// call it belongs to. HIR lowers a coderef invocation as a
    /// `DynamicBoundary(CoderefCall)` item immediately followed by the
    /// `CallExpr { form: Coderef }` item, so PIR links the two rather than
    /// synthesizing a second boundary.
    pending_dynamic_callee: Option<PirId>,
    unsupported: HashMap<&'static str, usize>,
    source_identity: Option<String>,
}

impl Lowerer {
    fn new(source_identity: Option<String>) -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            next_id: 0,
            last_in_scope: HashMap::new(),
            pending_dynamic_callee: None,
            unsupported: HashMap::new(),
            source_identity,
        }
    }

    fn lower_item(&mut self, item: &HirItem) {
        // HIR emits a `DynamicBoundary(CoderefCall)` immediately before its
        // `CallExpr { form: Coderef }`, so the pending boundary is consumed by
        // the very next item. Clear it before any other item so a boundary can
        // never mis-link to a later, unrelated coderef call even if HIR's
        // emission order changes.
        let consumes_pending_callee =
            matches!(&item.kind, HirKind::CallExpr(call) if matches!(call.form, CallForm::Coderef));
        if !consumes_pending_callee {
            self.pending_dynamic_callee = None;
        }

        match &item.kind {
            HirKind::VariableDecl(decl) => self.lower_variable_decl(item, decl),
            HirKind::CallExpr(call) => self.lower_call(item, call),
            HirKind::MethodCallExpr(call) => {
                self.lower_method_call(item, &call.method, call.object_kind, call.arg_count)
            }
            HirKind::IndirectCallExpr(call) => {
                self.lower_method_call(item, &call.method, call.object_kind, call.arg_count)
            }
            HirKind::DynamicBoundary(boundary) => {
                self.lower_dynamic_boundary(
                    item,
                    map_boundary_kind(boundary.kind),
                    boundary.reason.clone(),
                );
            }
            // Construct families PIR v0 does not yet lower. They remain visible
            // in the receipt instead of being silently dropped.
            other => {
                *self.unsupported.entry(hir_kind_name(other)).or_insert(0) += 1;
            }
        }
    }

    fn lower_variable_decl(&mut self, item: &HirItem, decl: &crate::hir::VariableDecl) {
        for variable in &decl.variables {
            let anchor = PirSourceAnchor::explicit(variable.range, item.id);
            let operation = if is_stash_declarator(&decl.declarator) {
                PirOperation::StashWrite {
                    symbol: SymbolName {
                        sigil: variable.sigil.clone(),
                        name: variable.name.clone(),
                        package: item.package_context.clone(),
                    },
                }
            } else {
                PirOperation::LexicalWrite {
                    name: LexicalName {
                        sigil: variable.sigil.clone(),
                        name: variable.name.clone(),
                    },
                }
            };
            // The declaration names a write target, which is a known lvalue.
            self.push_node(item, anchor, operation, PirContext::Lvalue, None);
        }

        if decl.has_initializer {
            // A declaration-with-initializer statement evaluates the assignment
            // in void context; the bound lvalues above carry the known context.
            let anchor = PirSourceAnchor::explicit(item.range, item.id);
            self.push_node(item, anchor, PirOperation::Assign, PirContext::Void, None);
        }
    }

    fn lower_call(&mut self, item: &HirItem, call: &crate::hir::CallExpr) {
        match call.form {
            CallForm::NamedFunction => {
                let anchor = PirSourceAnchor::explicit(item.range, item.id);
                let operation = PirOperation::Call {
                    callee: named_callee(&call.name),
                    arg_count: call.arg_count,
                };
                self.push_node(item, anchor, operation, PirContext::Unknown, None);
            }
            CallForm::Coderef => {
                // HIR already emitted a `DynamicBoundary(CoderefCall)` item
                // just before this call; link to it instead of duplicating it.
                let boundary_id = self.pending_dynamic_callee.take();
                let anchor = PirSourceAnchor::explicit(item.range, item.id);
                let operation =
                    PirOperation::Call { callee: PirCallee::Dynamic, arg_count: call.arg_count };
                self.push_node(item, anchor, operation, PirContext::Unknown, boundary_id);
            }
        }
    }

    fn lower_method_call(
        &mut self,
        item: &HirItem,
        method: &str,
        object_kind: &'static str,
        arg_count: usize,
    ) {
        let anchor = PirSourceAnchor::explicit(item.range, item.id);
        let operation = PirOperation::MethodCall {
            receiver: PirReceiver::Expression { kind: object_kind },
            method: PirMethod::Named(method.to_string()),
            arg_count,
        };
        self.push_node(item, anchor, operation, PirContext::Unknown, None);
    }

    fn lower_dynamic_boundary(
        &mut self,
        item: &HirItem,
        kind: PirDynamicBoundaryKind,
        reason: String,
    ) -> PirId {
        let anchor = PirSourceAnchor::dynamic_boundary(item.range, item.id);
        let id = self.push_node(
            item,
            anchor,
            PirOperation::DynamicBoundary { kind, reason },
            PirContext::Unknown,
            None,
        );
        // Control may not return through a dynamic boundary; record the exit
        // edge instead of dropping it.
        self.edges.push(PirEdge { from: id, to: None, kind: PirEdgeKind::DynamicExit });
        if kind == PirDynamicBoundaryKind::DynamicCallee {
            // Hold this boundary for the coderef call HIR emits next.
            self.pending_dynamic_callee = Some(id);
        }
        id
    }

    fn push_node(
        &mut self,
        item: &HirItem,
        source_anchor: PirSourceAnchor,
        operation: PirOperation,
        context: PirContext,
        dynamic_boundary: Option<PirId>,
    ) -> PirId {
        let id = PirId::from_index(self.next_id);
        self.next_id += 1;

        // Conservative intra-region control flow: link consecutive nodes that
        // share a scope with a fallthrough edge.
        let scope = item.scope_context;
        if let Some(previous) = self.last_in_scope.get(&scope).copied() {
            self.edges.push(PirEdge {
                from: previous,
                to: Some(id),
                kind: PirEdgeKind::Fallthrough,
            });
        }
        self.last_in_scope.insert(scope, id);

        self.nodes.push(PirNode {
            id,
            source_anchor,
            operation,
            context,
            dynamic_boundary,
            scope,
            package_context: item.package_context.clone(),
        });
        id
    }

    fn finish(self) -> PirGraph {
        // A DynamicCallee boundary is always consumed by the coderef call HIR
        // emits next, so nothing should be pending here. If a future HIR change
        // emits a boundary without its call, this catches the invariant break in
        // debug builds rather than silently leaving a boundary unlinked.
        debug_assert!(
            self.pending_dynamic_callee.is_none(),
            "pending_dynamic_callee was not consumed: HIR emitted a DynamicCallee \
             boundary without a following coderef CallExpr",
        );
        let receipt =
            build_receipt(&self.nodes, self.edges.len(), self.unsupported, self.source_identity);
        PirGraph { nodes: self.nodes, edges: self.edges, receipt }
    }
}

fn build_receipt(
    nodes: &[PirNode],
    edge_count: usize,
    unsupported: HashMap<&'static str, usize>,
    source_identity: Option<String>,
) -> PirReceipt {
    let mut operation_counts = std::collections::BTreeMap::new();
    let mut context_counts = std::collections::BTreeMap::new();
    let mut dynamic_boundary_counts = std::collections::BTreeMap::new();
    let mut coverage = PirAnchorCoverage::default();

    for node in nodes {
        *operation_counts.entry(node.operation.name()).or_insert(0) += 1;
        *context_counts.entry(node.context.name()).or_insert(0) += 1;
        if node.source_anchor.is_anchored() {
            coverage.anchored += 1;
        } else {
            coverage.unanchored += 1;
        }
        if let PirOperation::DynamicBoundary { kind, .. } = &node.operation {
            *dynamic_boundary_counts.entry(kind.name()).or_insert(0) += 1;
        }
    }

    let unsupported_construct_counts = unsupported.into_iter().collect();

    PirReceipt {
        schema_version: PIR_RECEIPT_VERSION,
        source_identity,
        lowering_mode: PirLoweringMode::HirV0,
        node_count: nodes.len(),
        edge_count,
        operation_counts,
        context_counts,
        source_anchor_coverage: coverage,
        dynamic_boundary_counts,
        unsupported_construct_counts,
        // PIR v0 lowering consumes only HIR; no ambient inputs participate.
        ambient_inputs: Vec::new(),
        // PIR v0 never changes provider behavior.
        provider_behavior_changed: false,
    }
}

fn is_stash_declarator(declarator: &str) -> bool {
    // `our` binds a package/stash symbol; `local` dynamically scopes a
    // package/global slot. Both are stash writes. `my`/`state` are lexical.
    matches!(declarator, "our" | "local")
}

fn named_callee(name: &str) -> PirCallee {
    match name.rsplit_once("::") {
        Some((package, bare)) if !package.is_empty() && !bare.is_empty() => {
            PirCallee::Named { name: bare.to_string(), package: Some(package.to_string()) }
        }
        _ => PirCallee::Named { name: name.to_string(), package: None },
    }
}

fn map_boundary_kind(kind: DynamicBoundaryKind) -> PirDynamicBoundaryKind {
    match kind {
        DynamicBoundaryKind::CoderefCall => PirDynamicBoundaryKind::DynamicCallee,
        DynamicBoundaryKind::EvalExpression => PirDynamicBoundaryKind::EvalExpression,
        DynamicBoundaryKind::DoExpression => PirDynamicBoundaryKind::DoExpression,
        DynamicBoundaryKind::DynamicStashMutation => PirDynamicBoundaryKind::RuntimeStashMutation,
        DynamicBoundaryKind::Autoload => PirDynamicBoundaryKind::Autoload,
        DynamicBoundaryKind::SymbolicReferenceDeref => PirDynamicBoundaryKind::SymbolicReference,
    }
}

fn hir_kind_name(kind: &HirKind) -> &'static str {
    match kind {
        HirKind::PackageDecl(_) => "PackageDecl",
        HirKind::SubDecl(_) => "SubDecl",
        HirKind::MethodDecl(_) => "MethodDecl",
        HirKind::UseDecl(_) => "UseDecl",
        HirKind::RequireDecl(_) => "RequireDecl",
        HirKind::VariableDecl(_) => "VariableDecl",
        HirKind::CallExpr(_) => "CallExpr",
        HirKind::MethodCallExpr(_) => "MethodCallExpr",
        HirKind::IndirectCallExpr(_) => "IndirectCallExpr",
        HirKind::BarewordExpr(_) => "BarewordExpr",
        HirKind::LiteralExpr(_) => "LiteralExpr",
        HirKind::BlockShell(_) => "BlockShell",
        // Control-flow variants added by #1902. PIR v0 does not yet lower
        // branch/loop/return — they fall through to unsupported_construct_counts
        // where the gap is visible in every receipt, consistent with the PR's
        // stated scope ("Branch/Loop/Return reserved but not yet populated").
        HirKind::BranchShell(_) => "BranchShell",
        HirKind::LoopShell(_) => "LoopShell",
        HirKind::ControlTransfer(_) => "ControlTransfer",
        HirKind::StatementModifierShell(_) => "StatementModifierShell",
        HirKind::DynamicBoundary(_) => "DynamicBoundary",
    }
}

// ── PIR-A: lower from canonical HirFile::bodies ───────────────────────────────
//
// This is the new canonical lowering path introduced in PR 2 (#2578). It lowers
// Read/Write/Modify operations directly from the body arenas attached to
// `HirFile::bodies` by `lower_ast()` (PR 1, #2575/#2602).
//
// The old `lower_hir` (flat-items path above) lowers from `HirFile::items` and
// is now dormant relative to body-based facts. It is retained for backward
// compatibility until its callers are migrated; once fully superseded it should
// be retired (#2578 follow-up).

/// Lower a [`HirFile`]'s canonical body arenas into a PIR-A graph.
///
/// Requires that `lower_ast()` has run — i.e. `file.body_model_version ==
/// HIR_BODY_MODEL_VERSION`. If the version check fails the returned graph is
/// empty and the receipt records the mismatch in `ambient_inputs`.
#[must_use]
pub fn lower_hir_bodies(file: &HirFile) -> PirGraph {
    lower_hir_bodies_with_identity(file, None)
}

/// Lower a [`HirFile`]'s canonical body arenas into a PIR-A graph, tagging the
/// receipt with an optional caller-supplied source or fixture identity.
#[must_use]
pub fn lower_hir_bodies_with_identity(
    file: &HirFile,
    source_identity: Option<String>,
) -> PirGraph {
    // Verifier rule: schema-version mismatch → empty graph.
    if file.body_model_version != HIR_BODY_MODEL_VERSION {
        let receipt = PirReceipt {
            schema_version: PIR_RECEIPT_VERSION,
            source_identity,
            lowering_mode: PirLoweringMode::HirV0,
            node_count: 0,
            edge_count: 0,
            operation_counts: Default::default(),
            context_counts: Default::default(),
            source_anchor_coverage: Default::default(),
            dynamic_boundary_counts: Default::default(),
            unsupported_construct_counts: Default::default(),
            ambient_inputs: vec![format!(
                "body_model_version mismatch: expected {HIR_BODY_MODEL_VERSION}, got {}",
                file.body_model_version
            )],
            provider_behavior_changed: false,
        };
        return PirGraph { nodes: vec![], edges: vec![], receipt };
    }

    let mut lowerer = BodyLowerer::new(source_identity);
    for (body_idx, body) in file.bodies.iter().enumerate() {
        lowerer.lower_body(body, HirBodyId(body_idx as u32), file);
    }
    lowerer.finish()
}

/// Body-arena lowerer for PIR-A.
///
/// Walks `HirBody` arenas and emits `Read`/`Write`/`Modify` PIR operations.
/// Verifier rules are applied inline — any node that would emit a wrong fact
/// instead produces nothing (fail-closed).
struct BodyLowerer {
    nodes: Vec<PirNode>,
    edges: Vec<PirEdge>,
    next_id: u32,
    last_in_scope: HashMap<Option<HirScopeId>, PirId>,
    unsupported: HashMap<&'static str, usize>,
    source_identity: Option<String>,
}

impl BodyLowerer {
    fn new(source_identity: Option<String>) -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            next_id: 0,
            last_in_scope: HashMap::new(),
            unsupported: HashMap::new(),
            source_identity,
        }
    }

    fn lower_body(&mut self, body: &HirBody, _body_id: HirBodyId, file: &HirFile) {
        // Walk the root block's statements.
        if let Some(root_block) = body.block(body.root_block) {
            for stmt_id in &root_block.stmts {
                self.lower_stmt(body, *stmt_id, file);
            }
        }
    }

    fn lower_stmt(&mut self, body: &HirBody, stmt_id: crate::hir::HirStmtId, file: &HirFile) {
        let stmt = match body.stmt(stmt_id) {
            Some(s) => s,
            None => return,
        };
        match stmt {
            HirStmt::Let { name, sigil, storage: _, init } => {
                // Declaration site: the variable is always a Write.
                // The storage class (my/state/our) is already captured in VariableKind
                // through the HIR body; here we emit from the declaration sigil/name.
                let range = body.source_map.stmt_ranges.get(stmt_id.0 as usize).copied();
                if let Some(range) = range {
                    let anchor = self.make_body_anchor(range);
                    let op = PirOperation::LexicalWrite {
                        name: LexicalName { sigil: sigil_str(sigil), name: name.clone() },
                    };
                    self.push_body_node(anchor, op, PirContext::Lvalue, None, file);
                }
                // Lower the initializer expression.
                if let Some(init_id) = init {
                    self.lower_expr(body, *init_id, file);
                }
            }
            HirStmt::Expr(expr_id) => {
                self.lower_expr(body, *expr_id, file);
            }
        }
    }

    fn lower_expr(&mut self, body: &HirBody, expr_id: HirExprId, file: &HirFile) {
        let expr = match body.expr(expr_id) {
            Some(e) => e,
            None => return,
        };
        let range = body.source_map.expr_ranges.get(expr_id.0 as usize).copied();
        let range = match range {
            Some(r) => r,
            None => return, // no source range → fail-closed, emit nothing
        };

        match expr {
            HirExpr::Variable(v) => {
                self.lower_variable_expr(v, range, file);
            }

            HirExpr::Assign { lhs, rhs, mode } => {
                match mode {
                    AssignMode::Simple => {
                        // Lower LHS as a Write place, then RHS as a Read.
                        self.lower_expr(body, *lhs, file);
                        self.lower_expr(body, *rhs, file);
                        // Emit an Assign node spanning the whole expression.
                        let anchor = self.make_body_anchor(range);
                        self.push_body_node(anchor, PirOperation::Assign, PirContext::Void, None, file);
                    }
                    AssignMode::ReadModifyWrite => {
                        // Compound assign: emit a single Modify node — place evaluated once.
                        // The LHS expr must be a Variable; if it isn't, fall through to unsupported.
                        if let Some(HirExpr::Variable(v)) = body.expr(*lhs) {
                            let op_text = compound_op_for_rmw_assign(body, *lhs);
                            self.lower_variable_modify(v, op_text, range, file);
                            // Lower RHS as a Read operand.
                            self.lower_expr(body, *rhs, file);
                        } else {
                            // Non-variable LHS for compound assign → unsupported (fail-closed).
                            *self.unsupported.entry("CompoundAssignNonVarLhs").or_insert(0) += 1;
                        }
                    }
                }
            }

            HirExpr::Unary { operand, mode, op } => {
                match mode {
                    UnaryMode::ReadModifyWrite => {
                        // `++`/`--` on a variable → Modify.
                        if let Some(HirExpr::Variable(v)) = body.expr(*operand) {
                            self.lower_variable_modify(v, op.clone(), range, file);
                        } else {
                            *self.unsupported.entry("UnaryRmwNonVar").or_insert(0) += 1;
                        }
                    }
                    UnaryMode::Read => {
                        self.lower_expr(body, *operand, file);
                    }
                }
            }

            HirExpr::Binary { lhs, rhs, op: _ } => {
                // Lower both operands as reads; the binary op itself is not modeled
                // in PIR-A (no CFG, no value tracking).
                self.lower_expr(body, *lhs, file);
                self.lower_expr(body, *rhs, file);
            }

            HirExpr::Opaque { ast_kind } => {
                // Fail-closed: opaque nodes never emit exact facts.
                *self.unsupported.entry(ast_kind_to_static(ast_kind)).or_insert(0) += 1;
            }

            HirExpr::Call { args: _, ast_kind: _ } => {
                // PIR-A does not lower calls from body arenas yet.
                *self.unsupported.entry("Call").or_insert(0) += 1;
            }
        }
    }

    /// Emit a Read or Write PIR node for a `HirVariable`, respecting its `VariableKind`.
    fn lower_variable_expr(
        &mut self,
        v: &crate::hir::HirVariable,
        range: crate::SourceLocation,
        file: &HirFile,
    ) {
        let anchor = self.make_body_anchor(range);
        let sigil = sigil_str(&v.sigil);
        let op = match (v.access, &v.kind) {
            // Verifier rule: Unresolved/Dynamic place → no exact Read/Write.
            // VariableKind has Lexical and Package; Package covers both resolved
            // package aliases and unresolved globals. We emit ops for both —
            // a StashRead/StashWrite for Package, LexicalRead/LexicalWrite for Lexical.
            (AccessMode::Read, VariableKind::Lexical) => PirOperation::LexicalRead {
                name: LexicalName { sigil, name: v.name.clone() },
            },
            (AccessMode::Write, VariableKind::Lexical) => PirOperation::LexicalWrite {
                name: LexicalName { sigil, name: v.name.clone() },
            },
            (AccessMode::ReadModifyWrite, VariableKind::Lexical) => {
                // ReadModifyWrite access on a Variable node means it came from
                // lower_expr_as_place for compound assign LHS — this is the Modify path.
                // It should have been caught in the Assign arm above; here it's a fallback.
                PirOperation::Modify {
                    name: LexicalName { sigil, name: v.name.clone() },
                    op: "+=".to_string(), // conservative default
                }
            }
            (AccessMode::Read, VariableKind::Package) => PirOperation::StashRead {
                symbol: SymbolName {
                    sigil,
                    name: v.name.clone(),
                    package: package_from_name(&v.name),
                },
            },
            (AccessMode::Write, VariableKind::Package) => PirOperation::StashWrite {
                symbol: SymbolName {
                    sigil,
                    name: v.name.clone(),
                    package: package_from_name(&v.name),
                },
            },
            (AccessMode::ReadModifyWrite, VariableKind::Package) => {
                // Same fallback as lexical RMW above.
                PirOperation::StashModify {
                    symbol: SymbolName {
                        sigil,
                        name: v.name.clone(),
                        package: package_from_name(&v.name),
                    },
                    op: "+=".to_string(),
                }
            }
        };
        self.push_body_node(anchor, op, PirContext::Unknown, None, file);
    }

    /// Emit a Modify (or StashModify) PIR node for a compound-assign or `++`/`--`.
    fn lower_variable_modify(
        &mut self,
        v: &crate::hir::HirVariable,
        op_text: String,
        range: crate::SourceLocation,
        file: &HirFile,
    ) {
        let anchor = self.make_body_anchor(range);
        let sigil = sigil_str(&v.sigil);
        let op = match &v.kind {
            VariableKind::Lexical => PirOperation::Modify {
                name: LexicalName { sigil, name: v.name.clone() },
                op: op_text,
            },
            VariableKind::Package => PirOperation::StashModify {
                symbol: SymbolName {
                    sigil,
                    name: v.name.clone(),
                    package: package_from_name(&v.name),
                },
                op: op_text,
            },
        };
        self.push_body_node(anchor, op, PirContext::Unknown, None, file);
    }

    fn make_body_anchor(&self, range: crate::SourceLocation) -> PirSourceAnchor {
        // Body nodes don't have a HirId — they come from the body arena, not the
        // flat items list. Use a synthetic HirId(0) as a placeholder; the range
        // and anchor_id carry the meaningful identity.
        use crate::hir::HirId;
        PirSourceAnchor {
            kind: super::model::PirAnchorKind::ExplicitSource,
            range: Some(range),
            anchor_id: Some(perl_semantic_facts::AnchorId(range.start as u64)),
            hir_item: Some(HirId::from_index(0)),
        }
    }

    fn push_body_node(
        &mut self,
        source_anchor: PirSourceAnchor,
        operation: PirOperation,
        context: PirContext,
        dynamic_boundary: Option<PirId>,
        file: &HirFile,
    ) -> PirId {
        let id = PirId::from_index(self.next_id);
        self.next_id += 1;

        // Conservative intra-body fallthrough edges (scope = None for body nodes,
        // since body arenas don't carry HirScopeId per-node in this slice).
        let scope: Option<HirScopeId> = None;
        if let Some(previous) = self.last_in_scope.get(&scope).copied() {
            self.edges.push(PirEdge {
                from: previous,
                to: Some(id),
                kind: PirEdgeKind::Fallthrough,
            });
        }
        self.last_in_scope.insert(scope, id);

        let _ = file; // reserved for future scope/package lookup from ScopeGraph
        self.nodes.push(PirNode {
            id,
            source_anchor,
            operation,
            context,
            dynamic_boundary,
            scope,
            package_context: None, // deferred: body arenas don't carry package_context yet
        });
        id
    }

    fn finish(self) -> PirGraph {
        let receipt =
            build_receipt(&self.nodes, self.edges.len(), self.unsupported, self.source_identity);
        PirGraph { nodes: self.nodes, edges: self.edges, receipt }
    }
}

// ── Helpers for PIR-A body lowering ──────────────────────────────────────────

fn sigil_str(sigil: &Sigil) -> String {
    match sigil {
        Sigil::Scalar => "$".to_string(),
        Sigil::Array => "@".to_string(),
        Sigil::Hash => "%".to_string(),
        Sigil::Code => "&".to_string(),
        Sigil::Glob => "*".to_string(),
    }
}

/// Extract package prefix from a qualified name (`Foo::x` → `Some("Foo")`).
fn package_from_name(name: &str) -> Option<String> {
    name.rsplit_once("::").map(|(pkg, _)| pkg.to_string())
}

/// Map a known-bad `ast_kind` string to a static str for the unsupported counter.
fn ast_kind_to_static(kind: &str) -> &'static str {
    // We can't return the dynamic string as a static ref; map to a small set
    // of known opaque kinds. Everything else maps to "OpaqueExpr".
    match kind {
        "ExpressionStatement" => "OpaqueExpressionStatement",
        "FunctionCall" | "Call" => "OpaqueCall",
        "MethodCall" => "OpaqueMethodCall",
        _ => "OpaqueExpr",
    }
}

/// Return the compound operator text for the LHS variable node of an
/// `AssignMode::ReadModifyWrite` assignment.
///
/// HIR stores the access mode on the variable node but not the original
/// operator string at the Assign level (the body lowerer peels the AST `op`
/// into `AssignMode` only). For PIR-A receipts we emit a reasonable default;
/// the exact text is available in the HIR body source map if needed later.
fn compound_op_for_rmw_assign(_body: &HirBody, _lhs_id: HirExprId) -> String {
    // The operator string was recorded in the AST but is not yet threaded
    // through the HirExpr::Assign variant. Use a conservative placeholder.
    // A future HIR body revision can add `op: Option<String>` to Assign to
    // forward the exact text; until then receipts show "compound" for the op.
    "compound".to_string()
}

#[cfg(test)]
mod tests {
    use super::super::model::PirAnchorKind;
    use super::*;
    use crate::Parser;
    use crate::hir::lower_ast;
    use perl_tdd_support::must_some;

    fn lower(source: &str) -> PirGraph {
        let mut parser = Parser::new(source);
        let output = parser.parse_with_recovery();
        let hir = lower_ast(&output.ast);
        lower_hir(&hir)
    }

    #[test]
    fn empty_source_yields_empty_graph() {
        let graph = lower("");
        assert!(graph.is_empty());
        assert_eq!(graph.nodes.len(), 0);
        assert_eq!(graph.edges.len(), 0);
        assert_eq!(graph.receipt.node_count, 0);
        assert_eq!(graph.receipt.edge_count, 0);
    }

    #[test]
    fn lexical_declaration_creates_write_and_assign() {
        let graph = lower("my $x = 1;");
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.nodes[0].operation.name(), "LexicalWrite");
        assert_eq!(graph.nodes[0].context, PirContext::Lvalue);
        assert_eq!(graph.nodes[1].operation.name(), "Assign");
        assert_eq!(graph.nodes[1].context, PirContext::Void);
    }

    #[test]
    fn our_declaration_is_stash_write() {
        let graph = lower("package Acme; our @items = (1, 2);");
        let stash = must_some(
            graph.nodes.iter().find(|n| matches!(n.operation, PirOperation::StashWrite { .. })),
        );
        if let PirOperation::StashWrite { symbol } = &stash.operation {
            assert_eq!(symbol.sigil, "@");
            assert_eq!(symbol.name, "items");
            assert_eq!(symbol.package.as_deref(), Some("Acme"));
        } else {
            panic!("expected StashWrite");
        }
    }

    #[test]
    fn local_declaration_is_stash_write() {
        let graph = lower("local $x;");
        let stash = must_some(
            graph.nodes.iter().find(|n| matches!(n.operation, PirOperation::StashWrite { .. })),
        );
        if let PirOperation::StashWrite { symbol } = &stash.operation {
            assert_eq!(symbol.sigil, "$");
            assert_eq!(symbol.name, "x");
        } else {
            panic!("expected StashWrite");
        }
    }

    #[test]
    fn named_call_with_package_qualifier() {
        let graph = lower("Bar::baz();");
        let call = must_some(
            graph.nodes.iter().find(|n| matches!(n.operation, PirOperation::Call { .. })),
        );
        if let PirOperation::Call { callee, arg_count } = &call.operation {
            assert_eq!(
                *callee,
                PirCallee::Named { name: "baz".to_string(), package: Some("Bar".to_string()) }
            );
            assert_eq!(*arg_count, 0);
        } else {
            panic!("expected Call");
        }
    }

    #[test]
    fn deep_package_qualified_call() {
        let graph = lower("A::B::foo();");
        let call = must_some(
            graph.nodes.iter().find(|n| matches!(n.operation, PirOperation::Call { .. })),
        );
        if let PirOperation::Call { callee, .. } = &call.operation {
            assert_eq!(
                *callee,
                PirCallee::Named { name: "foo".to_string(), package: Some("A::B".to_string()) }
            );
        } else {
            panic!("expected Call");
        }
    }

    #[test]
    fn unqualified_call_has_no_package() {
        let graph = lower("foo(1, 2, 3);");
        let call = must_some(
            graph.nodes.iter().find(|n| matches!(n.operation, PirOperation::Call { .. })),
        );
        if let PirOperation::Call { callee, arg_count } = &call.operation {
            assert_eq!(*callee, PirCallee::Named { name: "foo".to_string(), package: None });
            assert_eq!(*arg_count, 3);
        } else {
            panic!("expected Call");
        }
    }

    #[test]
    fn method_call_preserves_method_and_args() {
        let graph = lower("$obj->frobnicate(1, 2);");
        let method = must_some(
            graph.nodes.iter().find(|n| matches!(n.operation, PirOperation::MethodCall { .. })),
        );
        if let PirOperation::MethodCall { method, arg_count, .. } = &method.operation {
            assert_eq!(*method, PirMethod::Named("frobnicate".to_string()));
            assert_eq!(*arg_count, 2);
        } else {
            panic!("expected MethodCall");
        }
    }

    #[test]
    fn coderef_call_links_to_dynamic_boundary() {
        let graph = lower("my $cb; $cb->(1);");
        let call = must_some(graph.nodes.iter().find(|n| {
            matches!(n.operation, PirOperation::Call { callee: PirCallee::Dynamic, .. })
        }));
        let boundary_id = must_some(call.dynamic_boundary);
        let boundary = must_some(graph.node(boundary_id));
        if let PirOperation::DynamicBoundary { kind, .. } = &boundary.operation {
            assert_eq!(*kind, PirDynamicBoundaryKind::DynamicCallee);
        } else {
            panic!("expected DynamicBoundary");
        }
    }

    #[test]
    fn multiple_coderef_calls_have_separate_boundaries() {
        let graph = lower("my ($a, $b); $a->(); $b->();");
        let calls: Vec<_> = graph
            .nodes
            .iter()
            .filter(|n| {
                matches!(n.operation, PirOperation::Call { callee: PirCallee::Dynamic, .. })
            })
            .collect();
        assert_eq!(calls.len(), 2);
        let b0 = must_some(calls[0].dynamic_boundary);
        let b1 = must_some(calls[1].dynamic_boundary);
        assert_ne!(b0, b1);
    }

    #[test]
    fn eval_creates_dynamic_boundary() {
        let graph = lower(r#"eval "$code";"#);
        assert_eq!(graph.receipt.dynamic_boundary_counts.get("EvalExpression"), Some(&1));
    }

    #[test]
    fn symbolic_reference_creates_boundary() {
        let graph = lower("no strict 'refs'; my $v = ${$name};");
        assert_eq!(graph.receipt.dynamic_boundary_counts.get("SymbolicReference"), Some(&1));
    }

    #[test]
    fn typeglob_creates_dynamic_boundary() {
        let graph = lower("*alias = $thing;");
        assert_eq!(graph.receipt.dynamic_boundary_counts.get("RuntimeStashMutation"), Some(&1));
    }

    #[test]
    fn autoload_creates_dynamic_boundary() {
        let graph = lower("sub AUTOLOAD { }");
        assert_eq!(graph.receipt.dynamic_boundary_counts.get("Autoload"), Some(&1));
    }

    #[test]
    fn dynamic_boundary_has_correct_anchor_kind() {
        let graph = lower(r#"eval "$code";"#);
        let boundary = must_some(
            graph
                .nodes
                .iter()
                .find(|n| matches!(n.operation, PirOperation::DynamicBoundary { .. })),
        );
        assert_eq!(boundary.source_anchor.kind, PirAnchorKind::DynamicBoundary);
    }

    #[test]
    fn all_nodes_have_source_anchors() {
        let graph = lower("package Foo; my $x = bar(); $obj->m(); our $y;");
        for node in &graph.nodes {
            assert!(node.source_anchor.is_anchored());
        }
        assert_eq!(graph.receipt.source_anchor_coverage.unanchored, 0);
    }

    #[test]
    fn branch_shell_counted_in_receipt() {
        let graph = lower("if (1) { 1 }");
        assert_eq!(graph.receipt.unsupported_construct_counts.get("BranchShell"), Some(&1));
    }

    #[test]
    fn loop_shell_counted_in_receipt() {
        let graph = lower("while (1) { last; }");
        assert_eq!(graph.receipt.unsupported_construct_counts.get("LoopShell"), Some(&1));
    }

    #[test]
    fn control_transfer_counted_in_receipt() {
        let graph = lower("sub f { return 1; }");
        assert_eq!(graph.receipt.unsupported_construct_counts.get("ControlTransfer"), Some(&1));
    }

    #[test]
    fn statement_modifier_counted_in_receipt() {
        let graph = lower("$x = 1 if $y;");
        assert_eq!(
            graph.receipt.unsupported_construct_counts.get("StatementModifierShell"),
            Some(&1)
        );
    }

    #[test]
    fn all_four_control_flow_constructs() {
        let graph = lower(
            r#"
if (1) { 1 }
while (1) { last; }
sub f { return 1; }
$x = 1 if $y;
"#,
        );
        assert_eq!(graph.receipt.unsupported_construct_counts.get("BranchShell"), Some(&1));
        assert_eq!(graph.receipt.unsupported_construct_counts.get("LoopShell"), Some(&1));
        assert_eq!(graph.receipt.unsupported_construct_counts.get("ControlTransfer"), Some(&2));
        assert_eq!(
            graph.receipt.unsupported_construct_counts.get("StatementModifierShell"),
            Some(&1)
        );
    }

    #[test]
    fn receipt_operation_counts_match_nodes() {
        let graph = lower("my $x = 1; foo(); $obj->m(); eval '1';");
        let op_total: usize = graph.receipt.operation_counts.values().sum();
        assert_eq!(op_total, graph.nodes.len());
        assert_eq!(graph.receipt.node_count, graph.nodes.len());
    }

    #[test]
    fn receipt_context_counts_match_nodes() {
        let graph = lower("my $x = 1; foo(); $obj->m(); eval '1';");
        let ctx_total: usize = graph.receipt.context_counts.values().sum();
        assert_eq!(ctx_total, graph.nodes.len());
    }

    #[test]
    fn fallthrough_edges_between_statements() {
        let graph = lower("foo(); bar(); baz();");
        let count = graph.edges.iter().filter(|e| e.kind == PirEdgeKind::Fallthrough).count();
        assert_eq!(count, 2);
    }

    #[test]
    fn lowering_is_deterministic() {
        let mut parser = Parser::new("package Foo; my $x = bar(); $obj->m(); eval '1'; our @z;");
        let output = parser.parse_with_recovery();
        let hir = lower_ast(&output.ast);
        let first = lower_hir(&hir);
        let second = lower_hir(&hir);
        assert_eq!(first, second);
    }

    #[test]
    fn source_identity_threaded_to_receipt() {
        let mut parser = Parser::new("my $x = 1;");
        let output = parser.parse_with_recovery();
        let hir = lower_ast(&output.ast);
        let graph = lower_hir_with_identity(&hir, Some("fixture://demo.pl".to_string()));
        assert_eq!(graph.receipt.source_identity.as_deref(), Some("fixture://demo.pl"));
    }

    #[test]
    fn node_id_lookup_round_trips() {
        let graph = lower("my $x = 1;");
        for node in &graph.nodes {
            let found = must_some(graph.node(node.id));
            assert_eq!(found.id, node.id);
        }
    }

    #[test]
    fn multi_variable_declaration_creates_multiple_writes() {
        let graph = lower("my ($a, $b) = (1, 2);");
        let writes: Vec<_> = graph
            .nodes
            .iter()
            .filter(|n| matches!(n.operation, PirOperation::LexicalWrite { .. }))
            .collect();
        assert_eq!(writes.len(), 2);
    }

    #[test]
    fn leading_colons_not_empty_package() {
        let graph = lower("::foo();");
        let call = must_some(
            graph.nodes.iter().find(|n| matches!(n.operation, PirOperation::Call { .. })),
        );
        if let PirOperation::Call { callee, .. } = &call.operation {
            assert_eq!(*callee, PirCallee::Named { name: "foo".to_string(), package: None });
        } else {
            panic!("expected Call");
        }
    }

    #[test]
    fn receipt_edge_count_matches() {
        let graph = lower("my $x = 1; foo(); bar();");
        assert_eq!(graph.receipt.edge_count, graph.edges.len());
    }

    #[test]
    fn lowering_mode_is_hir_v0() {
        let graph = lower("my $x = 1;");
        assert_eq!(graph.receipt.lowering_mode, PirLoweringMode::HirV0);
    }

    #[test]
    fn unsupported_constructs_visible() {
        let graph = lower("package Foo; use strict; sub f {}");
        let unsupported = &graph.receipt.unsupported_construct_counts;
        assert_eq!(unsupported.get("PackageDecl"), Some(&1));
        assert_eq!(unsupported.get("UseDecl"), Some(&1));
        assert_eq!(unsupported.get("SubDecl"), Some(&1));
    }

    #[test]
    fn multiple_dynamic_boundary_types() {
        let graph = lower(
            r#"
eval "$code";
no strict 'refs'; ${$name};
*alias = $thing;
sub AUTOLOAD {}
"#,
        );
        assert_eq!(graph.receipt.dynamic_boundary_counts.get("EvalExpression"), Some(&1));
        assert_eq!(graph.receipt.dynamic_boundary_counts.get("SymbolicReference"), Some(&1));
        assert_eq!(graph.receipt.dynamic_boundary_counts.get("RuntimeStashMutation"), Some(&1));
        assert_eq!(graph.receipt.dynamic_boundary_counts.get("Autoload"), Some(&1));
    }

    #[test]
    fn provider_behavior_not_changed() {
        let graph = lower("my $x = 1;");
        assert!(!graph.receipt.provider_behavior_changed);
    }

    #[test]
    fn ambient_inputs_empty() {
        let graph = lower("my $x = 1;");
        assert!(graph.receipt.ambient_inputs.is_empty());
    }
}
