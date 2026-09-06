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
//!   ([`HirExpr::Opaque`], subscripts, heredocs, readlines, globs, and the
//!   regex families — regex literals, matches, substitutions and
//!   transliterations) are counted as declared `missing` evidence in the
//!   `Place`/`Effect` facets, so a body containing unmodeled expressions can
//!   never report those facets `Complete`.
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
    BindingPlaceRef, BodyIdentity, BoundarySiteRef, CallResolution, CallableFactRef,
    CallableSemanticSummary, CallableSemanticSummaryRef, ClaimCeiling, CompositionPolicy,
    EffectKind, EffectRef, FacetCompleteness, OutboundCallDependency, OutboundCallee, PlaceRole,
    PrivacyClass, RefusalCeiling, ResultExitKind, ResultExitRef, ResultFacets, SummaryCurrentness,
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
    let callable_bodies = collect_callable_bodies(file);

    for decl in &decls {
        // Range-containment pairing (order-independent by construction): a
        // declaration owns the body whose owner matches AND whose root-block
        // range is enclosed in the declaration's range. A declaration whose
        // body was never lowered (e.g. a signature-default anonymous sub)
        // finds zero candidates — it blocks honestly without shifting any
        // other declaration's pairing. When nested declarations enclose
        // several matching bodies, the declaration's direct body is the
        // MAXIMAL one (it encloses every nested candidate); only a tie
        // between distinct maximal candidates is genuinely ambiguous.
        let decl_range = normalize_range(decl.item.range);
        let candidates: Vec<(usize, &HirBody, usize, usize)> = callable_bodies
            .iter()
            .filter_map(|(body_idx, body)| {
                if !owner_matches(body, decl.is_method, &decl.name) {
                    return None;
                }
                let root = body.source_map.block_range(body.root_block).map(normalize_range)?;
                (decl_range.start <= root.start && root.end <= decl_range.end)
                    .then_some((*body_idx, *body, root.start, root.end))
            })
            .collect();
        let pairing = select_direct_body(candidates);
        match pairing {
            Ok((body_idx, body)) => {
                assemble_one(file, ctx, decl, decl_range, body_idx, body, &mut assembly);
            }
            Err(0) => assembly.blockers.push(AssemblyBlocker {
                callable_name: decl.name.clone(),
                body_idx: usize::MAX,
                reason: "declaration has no lowerable body enclosed in its range".to_string(),
            }),
            Err(n) => assembly.blockers.push(AssemblyBlocker {
                callable_name: decl.name.clone(),
                body_idx: usize::MAX,
                reason: format!(
                    "ambiguous declaration/body pairing: {n} maximal candidate bodies tie inside \
                     the declaration range (fail closed, never guess)"
                ),
            }),
        }
    }
    assembly.files_processed = 1;
    assembly
}

/// Select a declaration's direct body from its enclosed matching candidates
/// (`body_idx`, `body`, root `start`, root `end`): the unique maximal-range
/// candidate. The direct body encloses every nested candidate, so it always
/// has the strictly largest span; `Err(0)` means no candidate, `Err(n)`
/// means an n-way maximal tie (genuinely ambiguous, fail closed).
fn select_direct_body(
    candidates: Vec<(usize, &HirBody, usize, usize)>,
) -> Result<(usize, &HirBody), usize> {
    let Some(max_span) =
        candidates.iter().map(|(_, _, start, end)| end.saturating_sub(*start)).max()
    else {
        return Err(0);
    };
    let mut maximal =
        candidates.into_iter().filter(|(_, _, start, end)| end.saturating_sub(*start) == max_span);
    let Some((body_idx, body, _, _)) = maximal.next() else {
        return Err(0);
    };
    let tie_count = maximal.count();
    if tie_count == 0 { Ok((body_idx, body)) } else { Err(tie_count + 1) }
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

/// Collect callable bodies (subroutine and method owners) with their arena
/// indices, in body-arena order.
fn collect_callable_bodies(file: &HirFile) -> Vec<(usize, &HirBody)> {
    file.bodies
        .iter()
        .enumerate()
        .filter(|(_, body)| !matches!(body.owner, BodyOwnerKind::ProgramRoot))
        .collect()
}

/// Whether a body's owner matches a declaration's (kind, name) identity.
fn owner_matches(body: &HirBody, is_method: bool, name: &Option<String>) -> bool {
    match (&body.owner, is_method) {
        (BodyOwnerKind::Subroutine { name: owner }, false) => owner == name,
        (BodyOwnerKind::Method { name: owner }, true) => name.as_ref() == Some(owner),
        _ => false,
    }
}

/// The innermost callable (`Subroutine`/`Method`) scope enclosing `start`,
/// walked through the canonical scope graph. Attribution by scope identity
/// keeps a nested callable's items out of its parent's packet.
///
/// Depth cap: a cyclic scope graph produced by recovery parsing must not
/// loop forever — past the cap the chain is untrustworthy, so no owner is
/// reported (fail closed).
fn owning_callable_scope(file: &HirFile, start: HirScopeId) -> Option<HirScopeId> {
    const MAX_SCOPE_WALK_DEPTH: usize = 1024;
    let mut current = Some(start);
    let mut remaining = MAX_SCOPE_WALK_DEPTH;
    while let Some(id) = current {
        if remaining == 0 {
            return None;
        }
        remaining -= 1;
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
    decl_range: perl_parser_core::SourceLocation,
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

    let nodes = lower_single_body(body, perl_parser_core::hir::HirBodyId(body_idx as u32), file);
    let outbound = collect_outbound(file, ctx, decl_scope);
    let unmodeled = count_unmodeled(body);
    // The work law: zero useful visited operations can never satisfy a
    // summary. Visited operations are the per-body PIR nodes, the attributed
    // flat items this callable owns, and the unmodeled body expressions the
    // assembler walked and declared missing. A genuinely empty body (no
    // nodes, no items, no expressions) is the only zero-work shape.
    let visited_ops =
        (nodes.len() as u32).saturating_add(outbound.items_visited).saturating_add(unmodeled);
    if visited_ops == 0 {
        block("body lowered to zero PIR nodes: zero useful work can never satisfy a summary");
        return;
    }

    let mut packet =
        build_packet(ctx, decl, decl_range, body_idx, body, &nodes, outbound, unmodeled);
    // Fail closed: an assembled packet that fails its own contract
    // validation is a blocker naming the violations, never an invalid
    // summary reported as success.
    if let Err(violations) = packet.validate() {
        block(&format!(
            "assembled packet failed contract validation (fail closed): {}",
            violations.join("; ")
        ));
        return;
    }
    // Measure the canonical byte size AFTER validation, where the blocker
    // channel exists: a serialization failure is an instrument error and
    // becomes a blocker — never a fabricated 0-byte accounting presented
    // as success. (Measured over the zero-filled ledger field; the field
    // documents its own zero-filled measurement.)
    match packet.canonical_bytes() {
        Ok(bytes) => packet.work.bytes_retained = bytes.len() as u64,
        Err(err) => {
            block(&format!("packet serialization failed (instrument error, fail closed): {err}"));
            return;
        }
    }
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
            | HirExpr::Glob { .. }
            // Regex families (#7136). Canonical body HIR models these as typed
            // forms, but per-body PIR lowering still records all four as
            // unsupported (canonical PIR-A regex operations are #7137), so they
            // remain unmodeled for this law.
            //
            // This is deliberately wider than restoring the previous behavior,
            // and the difference is worth stating: before the families were
            // typed, only the unbound form was counted (`qr//` and bare
            // `/.../` lowered to `Opaque`), while a bound `$x =~ …` lowered to
            // `HirExpr::Call` and was not counted. A callable whose only
            // unmodeled content is a bound match, substitution or
            // transliteration therefore reported `Complete` before and reports
            // `Limited` now.
            //
            // That downgrade is the honest reading of this law rather than an
            // accident of the refactor: `s///` and `tr///` write a place PIR
            // does not record, and a match writes capture/match state, so a
            // body containing one has evidence this assembler cannot see. The
            // contrast with `HirExpr::Call` — which is also PIR-unsupported yet
            // still leaves `Place` complete — is consistent for the same
            // reason: a call's places are its arguments, which *are* modeled,
            // and its `Effect` facet is already blocked separately by the
            // unresolved outbound-call dependency.
            | HirExpr::Regex(_)
            | HirExpr::Match(_)
            | HirExpr::Substitution(_)
            | HirExpr::Transliteration(_) => count = count.saturating_add(1),
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
    // Derive the u64 by a deterministic byte-fold over the full fingerprint
    // string (FNV-style wrapping multiply-add over every byte): no parsing,
    // no failure path, and no fabricated sentinel identity — the derivation
    // is total by construction.
    let mut id = 0xcbf2_9ce4_8422_2325u64;
    for byte in fingerprint.as_bytes() {
        id = id.wrapping_mul(0x0000_0100_0000_01b3).wrapping_add(u64::from(*byte));
    }
    EntityId(id)
}

/// Content identity of one callable body: a canonical fingerprint over the
/// caller-supplied file body identity, the body index, the lowered operation
/// sequence (operation families and anchors in lowering order), and the
/// outbound joins — each dependency's callee shape and anchor and each
/// boundary site's kind and anchor, in source order. A changed call target
/// or boundary site is a different body.
fn body_identity(
    ctx: &SummaryAssemblyContext,
    body_idx: usize,
    nodes: &[PirNode],
    outbound_calls: &[OutboundCallDependency],
    boundary_sites: &[BoundarySiteRef],
) -> BodyIdentity {
    let mut fingerprint = SemanticIdentityFingerprint::new("callable-body-v1")
        .field("document", &ctx.document.0.to_string())
        .field("body", &body_idx.to_string());
    if let BodyIdentity::Exact(file_identity) = &ctx.body {
        fingerprint = fingerprint.field("file-body", file_identity);
    }
    let anchor_text = |anchor: &Option<SourceAnchor>| {
        anchor
            .map(|anchor| format!("{}:{}", anchor.start_byte, anchor.end_byte))
            .unwrap_or_else(|| "none".to_string())
    };
    for node in nodes {
        // Full operation payload (Debug covers names/operators/kinds — never
        // source text): two equal-length but different operations are
        // different bodies.
        //
        // Scope limit: this holds for operations that *become* PIR nodes. A
        // construct the per-body lowering records as unsupported emits no node
        // and so contributes nothing here — regex-family operations are the
        // clearest case, and `/foo/i`, `/foo/g` and `/bar/i` in an otherwise
        // identical callable currently share one identity. That predates the
        // typed regex variants (a bound match was previously an unsupported
        // `Call`, an unbound one an `Opaque`) and is tracked by #14645. Do not
        // read the sentence above as covering every edit to a body.
        fingerprint = fingerprint.field("op", &format!("{:?}", node.operation)).field(
            "op-anchor",
            &node
                .source_anchor
                .range
                .map(|range| format!("{}:{}", range.start, range.end))
                .unwrap_or_else(|| "none".to_string()),
        );
    }
    for dependency in outbound_calls {
        let callee = match &dependency.callee {
            OutboundCallee::Named(name) => format!("named:{name}"),
            OutboundCallee::Dynamic(_) => "dynamic".to_string(),
            // Unknown and any future callee shape: conservative identity.
            _ => "unknown".to_string(),
        };
        fingerprint = fingerprint
            .field("call", &callee)
            .field("call-anchor", &anchor_text(&dependency.anchor));
    }
    for site in boundary_sites {
        fingerprint = fingerprint
            .field("boundary", &format!("{:?}", site.kind))
            .field("boundary-anchor", &anchor_text(&site.anchor));
    }
    BodyIdentity::Exact(fingerprint.finish())
}

/// Record one method-call-shaped outbound dependency. The HIR
/// `MethodCallExpr`/`IndirectCallExpr` payload carries NO receiver name —
/// only the method name, argument count, and the receiver's AST kind — so
/// the class identity (`Foo` in `Foo->run()`) is unavailable at this seam.
/// Emitting `Named(method)` would be false precision (two classes' `run`
/// would share one callee identity), so every Identifier-receiver call is
/// [`OutboundCallee::Unknown`]; the method name is dropped because the
/// contract's `Unknown` variant carries no payload. Honest imprecision
/// beats wrong precision; I03/I04 must resolve method targets from the
/// call-site anchor. A non-Identifier receiver additionally blocks Control
/// (receiver-dependent dispatch).
fn push_method_call(
    outbound_calls: &mut Vec<OutboundCallDependency>,
    boundary_sites: &mut Vec<BoundarySiteRef>,
    reference: CallableFactRef,
    anchor: Option<SourceAnchor>,
    object_kind: &str,
    dynamic_link: &BoundaryLink,
) {
    if object_kind == "Identifier" {
        outbound_calls.push(OutboundCallDependency::new(
            reference,
            anchor,
            OutboundCallee::Unknown,
            vec![SummaryFacetKind::Result, SummaryFacetKind::Effect, SummaryFacetKind::Exception],
            CallResolution::UnresolvedTransitive,
        ));
    } else {
        // The dynamic-dispatch call site is itself one boundary site.
        boundary_sites.push(BoundarySiteRef::new(dynamic_link.kind, reference.clone(), anchor));
        outbound_calls.push(OutboundCallDependency::new(
            reference,
            anchor,
            OutboundCallee::Dynamic(dynamic_link.clone()),
            vec![
                SummaryFacetKind::Result,
                SummaryFacetKind::Effect,
                SummaryFacetKind::Exception,
                SummaryFacetKind::Control,
            ],
            CallResolution::UnresolvedTransitive,
        ));
    }
}

/// Outbound joins from the canonical flat HIR items for one callable.
struct OutboundJoin {
    /// Outbound call dependencies in item (source) order.
    calls: Vec<OutboundCallDependency>,
    /// Boundary links observed inside the callable (including dynamic-callee
    /// links); canonicalized by the envelope constructor.
    boundaries: Vec<BoundaryLink>,
    /// Every observed boundary site in source order — the per-site
    /// provenance record behind `boundaries`.
    boundary_sites: Vec<BoundarySiteRef>,
    /// Loop-control transfers (`next`/`last`/`redo`, label `goto`) the
    /// per-body PIR lowering does not model — declared Control evidence.
    control_transfers: u32,
    /// Flat items visited for this callable (call, boundary, and
    /// control-transfer items).
    items_visited: u32,
}

/// Collect the outbound calls, dynamic boundaries, and control transfers one
/// callable owns.
///
/// The per-body PIR lowering does not model calls, dynamic boundaries, or
/// loop-control transfers; the flat HIR items carry them. Items are
/// attributed to the innermost callable by scope identity and preserved in
/// item (source) order.
fn collect_outbound(
    file: &HirFile,
    ctx: &SummaryAssemblyContext,
    decl_scope: HirScopeId,
) -> OutboundJoin {
    let mut calls: Vec<OutboundCallDependency> = Vec::new();
    let mut boundaries: Vec<BoundaryLink> = Vec::new();
    let mut boundary_sites: Vec<BoundarySiteRef> = Vec::new();
    let mut control_transfers = 0u32;
    let mut items_visited = 0u32;
    let dynamic_link = map_boundary_link(HirDynamicBoundaryKind::CoderefCall);
    // The named-call blocked set: an unresolved call can return anything,
    // do anything, and throw anything — it is never "non-throwing".
    const CALL_BLOCKS: [SummaryFacetKind; 3] =
        [SummaryFacetKind::Result, SummaryFacetKind::Effect, SummaryFacetKind::Exception];
    // The dynamic-call blocked set: a dynamic callee or frame-replacing
    // transfer can additionally invalidate control.
    const DYNAMIC_CALL_BLOCKS: [SummaryFacetKind; 4] = [
        SummaryFacetKind::Result,
        SummaryFacetKind::Effect,
        SummaryFacetKind::Exception,
        SummaryFacetKind::Control,
    ];
    for (item_idx, item) in file.items.iter().enumerate() {
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
                            CALL_BLOCKS.to_vec(),
                            CallResolution::UnresolvedTransitive,
                        ));
                    }
                    // Coderef/dynamic callee: the call can do anything,
                    // including transferring control. The call site is one
                    // boundary site — its HIR-emitted CoderefCall boundary
                    // item is folded into this site (never double-counted).
                    _ => {
                        boundary_sites.push(BoundarySiteRef::new(
                            dynamic_link.kind,
                            reference.clone(),
                            anchor,
                        ));
                        calls.push(OutboundCallDependency::new(
                            reference,
                            anchor,
                            OutboundCallee::Dynamic(dynamic_link.clone()),
                            DYNAMIC_CALL_BLOCKS.to_vec(),
                            CallResolution::UnresolvedTransitive,
                        ));
                    }
                }
            }
            HirKind::MethodCallExpr(call) => {
                items_visited = items_visited.saturating_add(1);
                push_method_call(
                    &mut calls,
                    &mut boundary_sites,
                    reference,
                    anchor,
                    call.object_kind,
                    &dynamic_link,
                );
            }
            HirKind::IndirectCallExpr(call) => {
                items_visited = items_visited.saturating_add(1);
                push_method_call(
                    &mut calls,
                    &mut boundary_sites,
                    reference,
                    anchor,
                    call.object_kind,
                    &dynamic_link,
                );
            }
            HirKind::DynamicBoundary(boundary) => {
                items_visited = items_visited.saturating_add(1);
                // A CoderefCall boundary item immediately followed by its
                // coderef CallExpr (the HIR emission contract) is folded
                // into that call's single site — skipped here, never
                // double-counted, never silently dropped when unmatched.
                let folded_into_call = boundary.kind == HirDynamicBoundaryKind::CoderefCall
                    && file.items.get(item_idx + 1).is_some_and(|next| {
                        let next_attributed =
                            next.scope_context.and_then(|scope| owning_callable_scope(file, scope))
                                == Some(decl_scope);
                        next_attributed
                            && matches!(&next.kind, HirKind::CallExpr(call)
                                if call.form == perl_parser_core::hir::CallForm::Coderef)
                            && normalize_range(next.range) == normalize_range(item.range)
                    });
                if !folded_into_call {
                    boundary_sites.push(BoundarySiteRef::new(
                        map_boundary_link(boundary.kind).kind,
                        reference,
                        anchor,
                    ));
                }
                boundaries.push(map_boundary_link(boundary.kind));
            }
            HirKind::ControlTransfer(transfer) => {
                items_visited = items_visited.saturating_add(1);
                match transfer.kind {
                    // `goto &sub` / `goto $expr` replaces the entire frame:
                    // an outbound call whose target the ControlTransfer item
                    // does not name (`label` records only bare-label gotos)
                    // — Unknown, never dropped.
                    perl_parser_core::hir::ControlTransferKind::Goto
                        if transfer.label.is_none() =>
                    {
                        calls.push(OutboundCallDependency::new(
                            reference,
                            anchor,
                            OutboundCallee::Unknown,
                            DYNAMIC_CALL_BLOCKS.to_vec(),
                            CallResolution::UnresolvedTransitive,
                        ));
                    }
                    // Label `goto`, `next`, `last`, `redo`: not outbound
                    // calls, but control evidence the per-body PIR lowering
                    // does not model — declared against the Control facet.
                    _ => control_transfers = control_transfers.saturating_add(1),
                }
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
    OutboundJoin { calls, boundaries, boundary_sites, control_transfers, items_visited }
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
    unmodeled: u32,
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
    let OutboundJoin {
        calls: outbound_calls,
        boundaries,
        boundary_sites,
        control_transfers,
        items_visited,
    } = outbound;

    // ── Facet ledger (completeness is facet-specific and declared) ───────
    let n_returns = result_exits.len().saturating_sub(1) as u32;
    let n_bindings = bindings.len() as u32;
    let n_effects = effects.len() as u32;
    let n_sites = boundary_sites.len() as u32;
    let n_outbound = outbound_calls.len() as u32;
    let blocking = |facet: SummaryFacetKind| {
        outbound_calls
            .iter()
            .filter(|dependency| dependency.blocked_facets.contains(&facet))
            .count() as u32
    };
    let has_boundary = !boundary_sites.is_empty();
    let limited = SummaryFacetStatus::Limited;
    let complete = SummaryFacetStatus::Complete;

    let result_status = if blocking(SummaryFacetKind::Result) == 0
        && !has_boundary
        && bare_detection_gaps == 0
        && unmodeled == 0
    {
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
        // lowering; no CFG exists, and loop-control transfers
        // (next/last/redo/label-goto) are unmodeled by the per-body PIR
        // lowering — Control is always Limited with both gaps declared.
        FacetCompleteness::new(
            SummaryFacetKind::Control,
            limited,
            branch_loop_ops.saturating_add(control_transfers),
            modeled_conditions,
            0,
            u32::from(branch_loop_ops == 0),
            branch_loop_ops.saturating_sub(modeled_conditions).saturating_add(control_transfers),
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
        // Every observed boundary site is represented in the packet; the
        // ledger counts the site record, never the deduped link set.
        FacetCompleteness::new(
            SummaryFacetKind::Boundary,
            complete,
            n_sites,
            n_sites,
            n_sites,
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
    // Work accounting: planned operations are the evidence units offered to
    // the walk (body statement/expression arena nodes plus attributed flat
    // items); visited operations are the PIR nodes emitted, the flat items
    // joined, and the unmodeled expressions walked and declared missing.
    // The lowering may model one expression as several operations, so
    // visited can honestly exceed planned — the counts are independent
    // accountings, never a fabricated equality.
    let planned_ops = (body.exprs.len() as u32)
        .saturating_add(body.stmts.len() as u32)
        .saturating_add(items_visited);
    let visited_ops = (nodes.len() as u32).saturating_add(items_visited).saturating_add(unmodeled);
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

    // The packet leaves the constructor with a zero byte ledger; the caller
    // (`assemble_one`) measures the canonical byte size after validation,
    // where a serialization failure can become an explicit blocker.
    CallableSemanticSummary::new(
        entity,
        decl.name.clone(),
        body_identity(ctx, body_idx, nodes, &outbound_calls, &boundary_sites),
        ctx.source_generation.clone(),
        to_anchor(decl_range, ctx.document),
        summary_ref,
        facets,
        result_exits,
        bindings,
        effects,
        outbound_calls,
        boundary_sites,
        SummaryWorkLedger::new(1, 1, planned_ops, visited_ops, visited_ops, 0),
    )
}
