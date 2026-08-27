//! Callable-local semantic summary assembly (#12674, I02).
//!
//! This assembler joins EXISTING canonical compiler facts — HIR declaration
//! items, the canonical per-body PIR lowering, and the HIR scope graph — by
//! identity into validated [`CallableSemanticSummary`] packets, one per
//! admitted callable. It answers for direct intraprocedural behavior only:
//! every outbound call is recorded as an unresolved transitive dependency
//! naming exactly the facets it blocks.
//!
//! # Honesty laws (issue falsifiers)
//!
//! - No rescanning of AST/source and no reconstruction from text, name, or
//!   path: the assembler consumes HIR/PIR objects only.
//! - No new fact vocabulary: packets reference canonical identities
//!   ([`CallableFactRef`], [`BoundaryLink`]).
//! - No composing callee facts, no call resolution, no call graph/SCC, no
//!   project traversal, no provider/query behavior.
//! - An unresolved outbound call is never treated as pure, empty, or
//!   non-throwing: it blocks at minimum the `Result` and `Effect` facets.
//! - Source (lowered PIR) order is preserved for effects, exits, and
//!   outbound calls; canonical sorted order applies to identity sets only.
//! - Missing, unmodeled, or unsupported evidence is declared in the facet
//!   ledger — never silently an exact empty set. Completeness is
//!   facet-specific.
//! - Every admitted callable yields exactly one summary or one explicit
//!   blocker; a body that lowers to zero PIR nodes gets a blocker, never a
//!   zero-work summary (the work law).
//!
//! # Substrate seams (documented limitations, not silent gaps)
//!
//! - The per-body PIR lowering ([`lower_single_body`]) models lexical
//!   reads/writes/modifies, stash access, assignments, branches, loops, and
//!   returns — but NOT calls or dynamic boundaries (they are recorded as
//!   unsupported constructs in that lowering). Outbound calls and dynamic
//!   boundaries therefore come from the canonical flat HIR items
//!   ([`HirKind::CallExpr`], [`HirKind::MethodCallExpr`],
//!   [`HirKind::IndirectCallExpr`], [`HirKind::DynamicBoundary`]), attributed
//!   to the innermost callable through the HIR scope graph, and referenced
//!   by [`CallableFactRef::HirItem`] identity.
//! - HIR body expressions the per-body PIR lowering does not model
//!   ([`HirExpr::Opaque`], subscripts, heredocs, readlines, globs) are
//!   counted as declared `missing` evidence in the `Place`/`Effect` facets,
//!   so a body containing unmodeled expressions can never report those
//!   facets `Complete`.
//! - Phase blocks (`BEGIN`/`CHECK`/`UNITCHECK`/...) own neither a
//!   `SubDecl`/`MethodDecl` item nor an [`HirBody`] in this substrate, so
//!   they are not admitted callables; every admitted callable is a runtime
//!   callable and declares the `CompileEffect` facet inapplicable.
//! - `BareReturn` is recorded only when the PIR `Return` node's source range
//!   joins exactly to a HIR body `Return` expression whose `value` is
//!   absent; when that join fails the exit stays `ExplicitReturn` and the
//!   detection gap is declared in the `Result` facet.

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    BodyOwnerKind, DynamicBoundaryKind as HirDynamicBoundaryKind, HirBody, HirExpr, HirExprId,
    HirFile, HirItem, HirKind, HirScopeId, ScopeKind, lower_ast,
};
use perl_parser_core::pir::{PirNode, PirOperation, lower_single_body};
use perl_semantic_facts::interprocedural::{
    BindingPlaceRef, BodyIdentity, CallResolution, CallableFactRef, CallableSemanticSummary,
    CallableSemanticSummaryRef, ClaimCeiling, CompositionPolicy, EffectKind, EffectRef,
    FacetCompleteness, OutboundCallDependency, OutboundCallee, PlaceRole, PrivacyClass,
    RefusalCeiling, ResultExitKind, ResultExitRef, ResultFacets, SummaryCurrentness,
    SummaryFacetKind, SummaryFacetStatus, SummaryWorkLedger, WorkBudget,
};
use perl_semantic_facts::semantic_identity::SemanticIdentityFingerprint;
use perl_semantic_facts::{
    AnchorId, BoundaryDisposition, BoundaryKind, BoundaryLink, EntityId, FileId,
    SemanticReasonCode, SourceAnchor, SourceGeneration,
};

/// Assembly inputs shared by every callable in one file.
#[derive(Debug, Clone)]
pub struct SummaryAssemblyContext {
    /// Document identity every anchor in the produced packets names.
    pub document: FileId,
    /// Source generation the summaries are assembled for. Packets are
    /// `Fresh` for exactly this generation when it is known.
    pub source_generation: SourceGeneration,
    /// Caller-supplied content identity of the file's body set (for example
    /// the editor's document digest), mixed into each per-callable body
    /// identity. [`BodyIdentity::Unknown`] when the caller has none — the
    /// per-callable digest is still computed from the lowering itself.
    pub body: BodyIdentity,
    /// Composition policy the packets offer (typically
    /// [`CompositionPolicy::DirectOnly`] for I02).
    pub composition_policy: CompositionPolicy,
    /// Work budget the packets offer to composition.
    pub work_budget: WorkBudget,
    /// Privacy classification of the produced packets.
    pub privacy: PrivacyClass,
}

/// Why one admitted callable has no summary. Every admitted callable gets
/// exactly one summary or one explicit blocker — never a silent skip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssemblyBlocker {
    /// Callable name as recorded (`None` for anonymous subs).
    pub callable_name: Option<String>,
    /// Index of the callable's body in the file's body arena, or
    /// `usize::MAX` when the declaration has no lowerable body at all.
    pub body_idx: usize,
    /// Human-readable reason the summary could not be assembled.
    pub reason: String,
}

/// Result of assembling summaries for one file.
#[derive(Debug, Clone, Default)]
pub struct CallableSummaryAssembly {
    /// One validated packet per summarized callable, in declaration order.
    pub summaries: Vec<CallableSemanticSummary>,
    /// One explicit blocker per admitted callable that could not be
    /// summarized.
    pub blockers: Vec<AssemblyBlocker>,
    /// Files processed (always 1 for a successful assembly).
    pub files_processed: u32,
}

/// Terminal assembly error. A parse failure is an explicit error, never an
/// empty success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssemblyError {
    /// The source failed to parse; no summaries exist.
    ParseFailed(String),
}

impl std::fmt::Display for AssemblyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseFailed(message) => write!(f, "parse failed: {message}"),
        }
    }
}

impl std::error::Error for AssemblyError {}

/// One callable declaration joined from the flat HIR items.
struct CallableDecl<'a> {
    name: Option<String>,
    is_method: bool,
    has_signature: bool,
    has_prototype: bool,
    item: &'a HirItem,
}

/// Assemble callable-local semantic summaries for every admitted callable in
/// `file` (HIR `SubDecl`/`MethodDecl` declarations, including anonymous
/// subs).
///
/// Deterministic: two assemblies of the same `file` and `ctx` produce
/// byte-identical canonical packets.
#[must_use]
pub fn assemble_callable_summaries(
    file: &HirFile,
    ctx: &SummaryAssemblyContext,
) -> CallableSummaryAssembly {
    let decls = collect_callable_decls(file);
    let mut assembly = CallableSummaryAssembly::default();
    // Per-(kind, name) occurrence cursors pair each declaration with its
    // body. Both sequences are depth-first source order, so the k-th
    // declaration with a given (kind, name) owns the k-th matching body.
    let mut body_cursor = std::collections::BTreeMap::<(bool, Option<String>), usize>::new();
    let callable_bodies = collect_callable_bodies(file);

    for decl in &decls {
        let key = (decl.is_method, decl.name.clone());
        let occurrence = body_cursor.entry(key.clone()).or_insert(0);
        let body = callable_bodies.get(&key).and_then(|bodies| bodies.get(*occurrence));
        *occurrence += 1;
        match body {
            Some(&(body_idx, body)) => {
                assemble_one(file, ctx, decl, body_idx, body, &mut assembly);
            }
            None => assembly.blockers.push(AssemblyBlocker {
                callable_name: decl.name.clone(),
                body_idx: usize::MAX,
                reason: "declaration has no lowerable body in the HIR body arena".to_string(),
            }),
        }
    }
    assembly.files_processed = 1;
    assembly
}

/// Parse `source`, lower it to HIR, and assemble callable summaries.
///
/// A parse failure is an explicit [`AssemblyError::ParseFailed`], never an
/// empty success.
pub fn assemble_from_source(
    source: &str,
    ctx: &SummaryAssemblyContext,
) -> Result<CallableSummaryAssembly, AssemblyError> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|err| AssemblyError::ParseFailed(err.to_string()))?;
    let file = lower_ast(&ast);
    Ok(assemble_callable_summaries(&file, ctx))
}

/// Collect the callable declaration items in stable source order.
fn collect_callable_decls<'a>(file: &'a HirFile) -> Vec<CallableDecl<'a>> {
    file.items
        .iter()
        .filter_map(|item| match &item.kind {
            HirKind::SubDecl(decl) => Some(CallableDecl {
                name: decl.name.clone(),
                is_method: false,
                has_signature: decl.has_signature,
                has_prototype: decl.has_prototype,
                item,
            }),
            HirKind::MethodDecl(decl) => Some(CallableDecl {
                name: Some(decl.name.clone()),
                is_method: true,
                has_signature: decl.has_signature,
                has_prototype: false,
                item,
            }),
            _ => None,
        })
        .collect()
}

/// Collect callable bodies grouped by (is_method, name), each group in
/// stable source order, paired with the body's arena index.
fn collect_callable_bodies(
    file: &HirFile,
) -> std::collections::BTreeMap<(bool, Option<String>), Vec<(usize, &HirBody)>> {
    let mut groups: std::collections::BTreeMap<(bool, Option<String>), Vec<(usize, &HirBody)>> =
        std::collections::BTreeMap::new();
    for (body_idx, body) in file.bodies.iter().enumerate() {
        let key = match &body.owner {
            BodyOwnerKind::Subroutine { name } => (false, name.clone()),
            BodyOwnerKind::Method { name } => (true, Some(name.clone())),
            BodyOwnerKind::ProgramRoot => continue,
        };
        groups.entry(key).or_default().push((body_idx, body));
    }
    groups
}

/// The innermost callable (`Subroutine`/`Method`) scope enclosing `start`,
/// walked through the canonical scope graph. Attribution by scope identity
/// keeps a nested callable's items out of its parent's packet.
fn owning_callable_scope(file: &HirFile, start: HirScopeId) -> Option<HirScopeId> {
    let mut current = Some(start);
    while let Some(id) = current {
        let frame = file.scope_graph.scopes.iter().find(|scope| scope.id == id)?;
        if matches!(frame.kind, ScopeKind::Subroutine | ScopeKind::Method) {
            return Some(id);
        }
        current = frame.parent;
    }
    None
}

/// Assemble one callable into a summary or record its blocker.
fn assemble_one(
    file: &HirFile,
    ctx: &SummaryAssemblyContext,
    decl: &CallableDecl<'_>,
    body_idx: usize,
    body: &HirBody,
    assembly: &mut CallableSummaryAssembly,
) {
    let name = decl.name.clone();
    let mut block = |reason: &str| {
        assembly.blockers.push(AssemblyBlocker {
            callable_name: name.clone(),
            body_idx,
            reason: reason.to_string(),
        });
    };

    // The declaration's scope is the callable's own pad scope; without it,
    // outbound calls cannot be attributed honestly.
    let Some(decl_scope) = decl.item.scope_context else {
        block("declaration has no scope identity; outbound calls cannot be attributed");
        return;
    };
    // Declaration/body join check: the declaration range must enclose the
    // body root block range. A failed join is a blocker, never a guessed
    // pairing. Ranges are normalized because parser recovery can emit
    // degenerate spans.
    let decl_range = normalize_range(decl.item.range);
    let body_range = body.source_map.block_range(body.root_block).map(normalize_range);
    let joins = match body_range {
        Some(body_range) => {
            decl_range.start <= body_range.start && body_range.end <= decl_range.end
        }
        None => false,
    };
    if !joins {
        block("declaration/body join failed: the declaration range does not enclose the body");
        return;
    }

    let nodes = lower_single_body(body, perl_parser_core::hir::HirBodyId(body_idx as u32), file);
    let outbound = collect_outbound(file, ctx, decl_scope);
    // The work law: zero useful visited operations can never satisfy a
    // summary. Visited operations are the per-body PIR nodes plus the
    // attributed flat call/boundary items this callable owns.
    if nodes.is_empty() && outbound.items_visited == 0 {
        block("body lowered to zero PIR nodes: zero useful work can never satisfy a summary");
        return;
    }

    let packet = build_packet(ctx, decl, decl_range, body_idx, body, &nodes, outbound);
    assembly.summaries.push(packet);
}

/// Saturating `usize` → `u32` for source byte offsets (mirrors the
/// `usize_to_u32` convention used across the workspace).
fn offset_u32(offset: usize) -> u32 {
    offset.min(u32::MAX as usize) as u32
}

/// Normalize a possibly degenerate (inverted) range.
fn normalize_range(range: perl_parser_core::SourceLocation) -> perl_parser_core::SourceLocation {
    if range.end < range.start {
        perl_parser_core::SourceLocation { start: range.end, end: range.start }
    } else {
        range
    }
}

/// Convert a substrate source range into a contract anchor for `document`.
fn to_anchor(range: perl_parser_core::SourceLocation, document: FileId) -> SourceAnchor {
    let range = normalize_range(range);
    SourceAnchor::new(
        Some(AnchorId(range.start as u64)),
        document,
        offset_u32(range.start),
        offset_u32(range.end),
    )
}

/// Convert a PIR node anchor into a contract anchor, when source-backed.
fn pir_anchor(node: &PirNode, document: FileId) -> Option<SourceAnchor> {
    node.source_anchor.range.map(|range| to_anchor(range, document))
}

/// Map a canonical HIR dynamic-boundary kind onto the envelope boundary
/// vocabulary (passthrough — no new boundary kinds are invented).
fn map_boundary_link(kind: HirDynamicBoundaryKind) -> BoundaryLink {
    let (kind, reason) = match kind {
        HirDynamicBoundaryKind::EvalExpression
        | HirDynamicBoundaryKind::DoExpression
        | HirDynamicBoundaryKind::EmbeddedRegexCode => {
            (BoundaryKind::Unsupported, SemanticReasonCode::UnsupportedEffect)
        }
        HirDynamicBoundaryKind::SymbolicReferenceDeref => {
            (BoundaryKind::SymbolicReference, SemanticReasonCode::DynamicValue)
        }
        // CoderefCall, DynamicStashMutation, Autoload, and any future kind:
        // the closest existing boundary category is a dynamic value.
        _ => (BoundaryKind::DynamicValue, SemanticReasonCode::DynamicValue),
    };
    BoundaryLink::new(None, kind, BoundaryDisposition::Degrade, reason)
}

/// Count HIR body expressions the per-body PIR lowering does not model.
/// These are declared `missing` evidence in the Place/Effect facets so an
/// unmodeled body can never report those facets Complete (law 7).
fn count_unmodeled(body: &HirBody) -> u32 {
    let mut count = 0u32;
    for expr in body.exprs.iter() {
        match expr {
            HirExpr::Opaque { .. }
            | HirExpr::Subscript(_)
            | HirExpr::Heredoc { .. }
            | HirExpr::Readline { .. }
            | HirExpr::Glob { .. } => count = count.saturating_add(1),
            _ => {}
        }
    }
    count
}

/// The `(range, has_value)` shape of every HIR `Return` expression in the
/// body, used to distinguish bare `return;` from `return EXPR;` without
/// guessing.
fn hir_return_shapes(body: &HirBody) -> Vec<(perl_parser_core::SourceLocation, bool)> {
    let mut shapes = Vec::new();
    for (index, expr) in body.exprs.iter().enumerate() {
        if let HirExpr::Return { value } = expr
            && let Some(range) = body.source_map.expr_range(HirExprId(index as u32))
        {
            shapes.push((range, value.is_some()));
        }
    }
    shapes
}

/// Deterministic entity identity for one callable, minted with the
/// canonical semantic-identity fingerprint over (document, body index,
/// recorded name, declaration anchor) — never a new hash scheme.
fn callable_entity_id(
    ctx: &SummaryAssemblyContext,
    body_idx: usize,
    name: Option<&str>,
    range: perl_parser_core::SourceLocation,
) -> EntityId {
    let fingerprint = SemanticIdentityFingerprint::new("callable-semantic-summary-v1")
        .field("document", &ctx.document.0.to_string())
        .field("body", &body_idx.to_string())
        .field("name", name.unwrap_or("<anonymous>"))
        .field("anchor-start", &range.start.to_string())
        .field("anchor-end", &range.end.to_string())
        .finish();
    let high = fingerprint.get(..16).unwrap_or(&fingerprint);
    EntityId(u64::from_str_radix(high, 16).unwrap_or(0))
}

/// Content identity of one callable body: a canonical fingerprint over the
/// caller-supplied file body identity, the body index, and the lowered
/// operation sequence (operation families and anchors in lowering order).
fn body_identity(ctx: &SummaryAssemblyContext, body_idx: usize, nodes: &[PirNode]) -> BodyIdentity {
    let mut fingerprint = SemanticIdentityFingerprint::new("callable-body-v1")
        .field("document", &ctx.document.0.to_string())
        .field("body", &body_idx.to_string());
    if let BodyIdentity::Exact(file_identity) = &ctx.body {
        fingerprint = fingerprint.field("file-body", file_identity);
    }
    for node in nodes {
        fingerprint = fingerprint.field("op", node.operation.name()).field(
            "op-anchor",
            &node
                .source_anchor
                .range
                .map(|range| format!("{}:{}", range.start, range.end))
                .unwrap_or_else(|| "none".to_string()),
        );
    }
    BodyIdentity::Exact(fingerprint.finish())
}

/// Record one method-call-shaped outbound dependency. A bareword/identifier
/// receiver (`Foo->bar`) is the static class-call shape; any other receiver
/// makes dispatch receiver-dependent, a dynamic boundary that also blocks
/// Control.
fn push_method_call(
    outbound_calls: &mut Vec<OutboundCallDependency>,
    reference: CallableFactRef,
    anchor: Option<SourceAnchor>,
    method: &str,
    object_kind: &str,
    dynamic_link: &BoundaryLink,
) {
    if object_kind == "Identifier" {
        outbound_calls.push(OutboundCallDependency::new(
            reference,
            anchor,
            OutboundCallee::Named(method.to_string()),
            vec![SummaryFacetKind::Result, SummaryFacetKind::Effect],
            CallResolution::UnresolvedTransitive,
        ));
    } else {
        outbound_calls.push(OutboundCallDependency::new(
            reference,
            anchor,
            OutboundCallee::Dynamic(dynamic_link.clone()),
            vec![SummaryFacetKind::Result, SummaryFacetKind::Effect, SummaryFacetKind::Control],
            CallResolution::UnresolvedTransitive,
        ));
    }
}

/// Outbound calls and dynamic boundaries joined from the canonical flat HIR
/// items for one callable.
struct OutboundJoin {
    /// Outbound call dependencies in item (source) order.
    calls: Vec<OutboundCallDependency>,
    /// Boundary links observed inside the callable (including dynamic-callee
    /// links); canonicalized by the envelope constructor.
    boundaries: Vec<BoundaryLink>,
    /// Flat items visited for this callable (call + boundary items).
    items_visited: u32,
}

/// Collect the outbound calls and dynamic boundaries one callable owns.
///
/// The per-body PIR lowering does not model calls or dynamic boundaries;
/// the flat HIR items do. Items are attributed to the innermost callable by
/// scope identity and preserved in item (source) order.
fn collect_outbound(
    file: &HirFile,
    ctx: &SummaryAssemblyContext,
    decl_scope: HirScopeId,
) -> OutboundJoin {
    let mut calls: Vec<OutboundCallDependency> = Vec::new();
    let mut boundaries: Vec<BoundaryLink> = Vec::new();
    let mut items_visited = 0u32;
    let dynamic_link = map_boundary_link(HirDynamicBoundaryKind::CoderefCall);
    for item in &file.items {
        let attributed = item.scope_context.and_then(|scope| owning_callable_scope(file, scope))
            == Some(decl_scope);
        if !attributed {
            continue;
        }
        let anchor = Some(to_anchor(item.range, ctx.document));
        let reference = CallableFactRef::HirItem(u64::from(item.id.index()));
        match &item.kind {
            HirKind::CallExpr(call) => {
                items_visited = items_visited.saturating_add(1);
                match call.form {
                    perl_parser_core::hir::CallForm::NamedFunction => {
                        calls.push(OutboundCallDependency::new(
                            reference,
                            anchor,
                            OutboundCallee::Named(call.name.clone()),
                            vec![SummaryFacetKind::Result, SummaryFacetKind::Effect],
                            CallResolution::UnresolvedTransitive,
                        ));
                    }
                    // Coderef/dynamic callee: the call can do anything,
                    // including transferring control — Control is blocked
                    // alongside Result/Effect.
                    _ => {
                        calls.push(OutboundCallDependency::new(
                            reference,
                            anchor,
                            OutboundCallee::Dynamic(dynamic_link.clone()),
                            vec![
                                SummaryFacetKind::Result,
                                SummaryFacetKind::Effect,
                                SummaryFacetKind::Control,
                            ],
                            CallResolution::UnresolvedTransitive,
                        ));
                    }
                }
            }
            HirKind::MethodCallExpr(call) => {
                items_visited = items_visited.saturating_add(1);
                push_method_call(
                    &mut calls,
                    reference,
                    anchor,
                    &call.method,
                    call.object_kind,
                    &dynamic_link,
                );
            }
            HirKind::IndirectCallExpr(call) => {
                items_visited = items_visited.saturating_add(1);
                push_method_call(
                    &mut calls,
                    reference,
                    anchor,
                    &call.method,
                    call.object_kind,
                    &dynamic_link,
                );
            }
            HirKind::DynamicBoundary(boundary) => {
                items_visited = items_visited.saturating_add(1);
                boundaries.push(map_boundary_link(boundary.kind));
            }
            _ => {}
        }
    }
    // Every dynamic-callee dependency also carries its boundary into the
    // referenced boundary set (deduplicated by the envelope constructor).
    for dependency in &calls {
        if let OutboundCallee::Dynamic(link) = &dependency.callee {
            boundaries.push(link.clone());
        }
    }
    OutboundJoin { calls, boundaries, items_visited }
}

/// Build the packet for one callable whose body lowered to `nodes`.
fn build_packet(
    ctx: &SummaryAssemblyContext,
    decl: &CallableDecl<'_>,
    decl_range: perl_parser_core::SourceLocation,
    body_idx: usize,
    body: &HirBody,
    nodes: &[PirNode],
    outbound: OutboundJoin,
) -> CallableSemanticSummary {
    let body_idx_u32 = body_idx as u32;
    let op_ref = |node: &PirNode| CallableFactRef::PirOp {
        body: body_idx_u32,
        op: u64::from(node.id.index()),
    };

    // ── Source-ordered joins over the per-body PIR lowering ──────────────
    let mut bindings: Vec<BindingPlaceRef> = Vec::new();
    let mut effects: Vec<EffectRef> = Vec::new();
    let mut result_exits: Vec<ResultExitRef> = Vec::new();
    let mut branch_loop_ops = 0u32;
    let mut modeled_conditions = 0u32;
    let mut bare_detection_gaps = 0u32;
    let return_shapes = hir_return_shapes(body);

    for node in nodes {
        let anchor = pir_anchor(node, ctx.document);
        match &node.operation {
            PirOperation::LexicalRead { name } => bindings.push(BindingPlaceRef::new(
                format!("{}{}", name.sigil, name.name),
                PlaceRole::Read,
                op_ref(node),
                anchor,
            )),
            PirOperation::LexicalWrite { name } => bindings.push(BindingPlaceRef::new(
                format!("{}{}", name.sigil, name.name),
                PlaceRole::Write,
                op_ref(node),
                anchor,
            )),
            PirOperation::Modify { name, .. } => {
                bindings.push(BindingPlaceRef::new(
                    format!("{}{}", name.sigil, name.name),
                    PlaceRole::Modify,
                    op_ref(node),
                    anchor,
                ));
                effects.push(EffectRef::new(EffectKind::Modify, op_ref(node), anchor));
            }
            PirOperation::Assign => {
                effects.push(EffectRef::new(EffectKind::Assign, op_ref(node), anchor));
            }
            PirOperation::StashRead { .. } => {
                effects.push(EffectRef::new(EffectKind::StashRead, op_ref(node), anchor));
            }
            PirOperation::StashWrite { .. } => {
                effects.push(EffectRef::new(EffectKind::StashWrite, op_ref(node), anchor));
            }
            PirOperation::StashModify { .. } => {
                effects.push(EffectRef::new(EffectKind::StashModify, op_ref(node), anchor));
            }
            PirOperation::Return => {
                // Bare-return detection joins the PIR node's range to the HIR
                // body Return expression. When the join fails the exit stays
                // ExplicitReturn and the gap is declared — never guessed.
                let kind = node
                    .source_anchor
                    .range
                    .and_then(|range| {
                        return_shapes.iter().find(|(shape_range, _)| *shape_range == range)
                    })
                    .map(|(_, has_value)| {
                        if *has_value {
                            ResultExitKind::ExplicitReturn
                        } else {
                            ResultExitKind::BareReturn
                        }
                    })
                    .unwrap_or_else(|| {
                        bare_detection_gaps = bare_detection_gaps.saturating_add(1);
                        ResultExitKind::ExplicitReturn
                    });
                result_exits.push(ResultExitRef::new(kind, Some(op_ref(node)), anchor));
            }
            PirOperation::Branch { condition } | PirOperation::Loop { condition } => {
                branch_loop_ops = branch_loop_ops.saturating_add(1);
                if condition.is_some() {
                    modeled_conditions = modeled_conditions.saturating_add(1);
                }
            }
            _ => {}
        }
    }
    // Every callable body has exactly one implicit fallthrough exit, recorded
    // last in source order.
    result_exits.push(ResultExitRef::new(ResultExitKind::ImplicitFallthrough, None, None));

    // ── Outbound calls and boundaries (joined by scope identity) ─────────
    let OutboundJoin { calls: outbound_calls, boundaries, items_visited } = outbound;

    // ── Facet ledger (completeness is facet-specific and declared) ───────
    let unmodeled = count_unmodeled(body);
    let n_returns = result_exits.len().saturating_sub(1) as u32;
    let n_bindings = bindings.len() as u32;
    let n_effects = effects.len() as u32;
    let n_boundaries = boundaries.len() as u32;
    let n_outbound = outbound_calls.len() as u32;
    let blocking = |facet: SummaryFacetKind| {
        outbound_calls
            .iter()
            .filter(|dependency| dependency.blocked_facets.contains(&facet))
            .count() as u32
    };
    let has_boundary = !boundaries.is_empty();
    let limited = SummaryFacetStatus::Limited;
    let complete = SummaryFacetStatus::Complete;

    let result_status =
        if blocking(SummaryFacetKind::Result) == 0 && !has_boundary && bare_detection_gaps == 0 {
            complete
        } else {
            limited
        };
    let parameter_status = if decl.has_signature || decl.has_prototype {
        // A signature/prototype exists, but PIR v0 does not lower parameter
        // detail — declared, not silently zero.
        limited
    } else {
        SummaryFacetStatus::NotProven
    };
    let place_status = if has_boundary || unmodeled > 0 { limited } else { complete };
    let effect_status = if has_boundary || unmodeled > 0 || blocking(SummaryFacetKind::Effect) > 0 {
        limited
    } else {
        complete
    };

    let facets = vec![
        FacetCompleteness::new(
            SummaryFacetKind::Result,
            result_status,
            n_returns.saturating_add(1),
            result_exits.len() as u32,
            n_returns,
            bare_detection_gaps,
            0,
            blocking(SummaryFacetKind::Result),
        ),
        FacetCompleteness::new(
            SummaryFacetKind::ParameterBinding,
            parameter_status,
            n_bindings,
            n_bindings,
            0,
            // Parameter detail beyond the declaration flags is not lowered
            // by this substrate; declared in both branches.
            1,
            0,
            0,
        ),
        FacetCompleteness::new(
            SummaryFacetKind::Place,
            place_status,
            n_bindings,
            n_bindings,
            0,
            0,
            unmodeled,
            0,
        ),
        FacetCompleteness::new(
            SummaryFacetKind::Effect,
            effect_status,
            n_effects,
            n_effects,
            n_effects,
            0,
            unmodeled,
            blocking(SummaryFacetKind::Effect),
        ),
        // No canonical alias/escape fact family exists — declared NotProven,
        // never Complete-with-zero.
        FacetCompleteness::new(
            SummaryFacetKind::AliasEscape,
            SummaryFacetStatus::NotProven,
            0,
            0,
            0,
            1,
            0,
            0,
        ),
        // The substrate records no per-callable diagnostics.
        FacetCompleteness::new(
            SummaryFacetKind::Diagnostic,
            SummaryFacetStatus::NotProven,
            0,
            0,
            0,
            1,
            0,
            0,
        ),
        // Exception behavior is not modeled by this substrate.
        FacetCompleteness::new(
            SummaryFacetKind::Exception,
            SummaryFacetStatus::NotProven,
            0,
            0,
            0,
            1,
            0,
            blocking(SummaryFacetKind::Exception),
        ),
        // Branch/Loop conditions are populated by later control-flow
        // lowering; no CFG exists, so Control is always Limited with the gap
        // declared.
        FacetCompleteness::new(
            SummaryFacetKind::Control,
            limited,
            branch_loop_ops,
            modeled_conditions,
            0,
            u32::from(branch_loop_ops == 0),
            branch_loop_ops.saturating_sub(modeled_conditions),
            blocking(SummaryFacetKind::Control),
        ),
        // Every admitted callable is a runtime callable: phase blocks own no
        // HirBody in this substrate, so CompileEffect is declared
        // inapplicable rather than silently empty.
        FacetCompleteness::new(
            SummaryFacetKind::CompileEffect,
            SummaryFacetStatus::NotProven,
            0,
            0,
            0,
            1,
            0,
            0,
        ),
        // Every observed boundary item is represented in the packet.
        FacetCompleteness::new(
            SummaryFacetKind::Boundary,
            complete,
            n_boundaries,
            n_boundaries,
            n_boundaries,
            0,
            0,
            0,
        ),
        // Every observed call item is recorded as a dependency — the
        // dependencies are this facet's content, counted in selected.
        FacetCompleteness::new(
            SummaryFacetKind::OutboundCall,
            complete,
            n_outbound,
            n_outbound,
            n_outbound,
            0,
            0,
            0,
        ),
    ];

    // ── Envelope and packet ──────────────────────────────────────────────
    // Visited operations: per-body PIR nodes plus the attributed flat
    // call/boundary items (the work law counts both).
    let visited_ops = (nodes.len() as u32).saturating_add(items_visited);
    let all_complete = facets.iter().all(|entry| entry.status == SummaryFacetStatus::Complete);
    let currentness = if ctx.source_generation.is_known() {
        SummaryCurrentness::Fresh(ctx.source_generation.clone())
    } else {
        SummaryCurrentness::Unknown
    };
    let entity = callable_entity_id(ctx, body_idx, decl.name.as_deref(), decl_range);
    let summary_ref = CallableSemanticSummaryRef::new(
        entity,
        ctx.source_generation.clone(),
        vec![],
        boundaries,
        ctx.composition_policy,
        ResultFacets::new(true, true, false, true),
        currentness,
        ctx.work_budget,
        RefusalCeiling::Refuse,
        if all_complete { ClaimCeiling::Exact } else { ClaimCeiling::Provisional },
        ctx.privacy,
    );

    let mut packet = CallableSemanticSummary::new(
        entity,
        decl.name.clone(),
        body_identity(ctx, body_idx, nodes),
        ctx.source_generation.clone(),
        to_anchor(decl_range, ctx.document),
        summary_ref,
        facets,
        result_exits,
        bindings,
        effects,
        outbound_calls,
        SummaryWorkLedger::new(1, 1, visited_ops, visited_ops, visited_ops, 0),
    );
    // Canonical byte size of the packet (serialized with a zero ledger
    // field, then recorded — the field documents its own zero-filled
    // measurement).
    packet.work.bytes_retained =
        packet.canonical_bytes().map(|bytes| bytes.len() as u64).unwrap_or(0);
    packet
}
