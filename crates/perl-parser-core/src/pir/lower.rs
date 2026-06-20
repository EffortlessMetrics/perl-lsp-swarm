//! HIR-to-PIR v0 lowering.
//!
//! Lowering is intentionally conservative. It lowers the data-access, call, and
//! dynamic-boundary operation families that the current HIR substrate can prove
//! from source, anchors every source-derived node, preserves dynamic-boundary
//! links, and records everything it could not lower in the receipt. It never
//! evaluates Perl and never changes provider behavior.

use std::collections::HashMap;

use crate::hir::{CallForm, DynamicBoundaryKind, HirFile, HirItem, HirKind, HirScopeId};

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
        HirKind::DynamicBoundary(_) => "DynamicBoundary",
    }
}
