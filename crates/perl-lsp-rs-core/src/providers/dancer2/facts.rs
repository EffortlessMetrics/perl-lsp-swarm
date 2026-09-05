//! Per-file canonical Dancer2 fact assembly (#8928).
//!
//! Runs the canonical extractors over one document and mints the canonical
//! fact family through the registry-activated producers for every exactly
//! activated package. Facts are recomputed from the current snapshot on
//! every request — the source generation is derived from the document
//! content digest, so an edit immediately changes the generation and a
//! stale exact answer cannot survive a re-query.

use super::activation::Dancer2FileActivations;
use perl_parser_core::Node;
use perl_semantic_analyzer::analysis::dancer2_hooks::extract_dancer2_hook_declarations;
use perl_semantic_analyzer::analysis::dancer2_routes::extract_dancer2_route_contexts;
use perl_semantic_facts::FileId;
use perl_semantic_facts::framework_adapters::dancer2_hooks::{
    Dancer2HookDeclaration, dancer2_hook_facts, dancer2_hook_handler_context_facts,
};
use perl_semantic_facts::framework_adapters::dancer2_routes::{
    Dancer2RouteFacts, dancer2_route_family_facts,
};
use perl_semantic_facts::handler::FrameworkHandler as RouteHandler;
use perl_semantic_facts::hook::HookFact;
use perl_semantic_facts::route::{RouteFact, RouteHandlerContextFact};

/// Canonical Dancer2 facts for one document (all exactly activated packages).
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct CanonicalDancer2FileFacts {
    /// Minted route facts (exact and degraded, generation owned).
    pub routes: Vec<RouteFact>,
    /// Minted prefix declaration facts.
    pub prefixes: Vec<perl_semantic_facts::route::RoutePrefixFact>,
    /// Minted route parameter facts.
    pub parameters: Vec<perl_semantic_facts::route::RouteParameterFact>,
    /// Minted inline handler-context facts.
    pub handler_contexts: Vec<RouteHandlerContextFact>,
    /// Minted hook facts.
    pub hooks: Vec<HookFact>,
    /// Source-extracted route declarations of exactly activated packages,
    /// retained for bounded diagnostics (excluded keywords mint no fact but
    /// remain source observations).
    pub extracted_routes:
        Vec<perl_semantic_facts::framework_adapters::dancer2_routes::Dancer2RouteDeclaration>,
}

impl CanonicalDancer2FileFacts {
    /// Whether any canonical fact was minted (activation was exact and the
    /// document carries admitted declarations).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
            && self.prefixes.is_empty()
            && self.parameters.is_empty()
            && self.handler_contexts.is_empty()
            && self.hooks.is_empty()
    }

    /// The route fact whose declaration span contains `offset`, excluding
    /// the handler operand span: positions inside the inline handler body
    /// belong to the generic paths, not to the route operands.
    #[must_use]
    pub fn route_at(&self, offset: usize) -> Option<&RouteFact> {
        self.routes.iter().find(|fact| {
            if !span_contains(&fact.envelope.anchor, offset) {
                return false;
            }
            let handler_span = match &fact.route.handler {
                RouteHandler::InlineSub { anchor } | RouteHandler::StaticCoderef { anchor, .. } => {
                    Some(*anchor)
                }
                RouteHandler::Bounded { anchor, .. } => *anchor,
                _ => None,
            };
            !handler_span.is_some_and(|anchor| span_contains(&anchor, offset))
        })
    }

    /// The hook fact whose declaration span contains `offset`, excluding
    /// the handler operand span (same containment rule as [`Self::route_at`]).
    #[must_use]
    pub fn hook_at(&self, offset: usize) -> Option<&HookFact> {
        self.hooks.iter().find(|fact| {
            if !span_contains(&fact.envelope.anchor, offset) {
                return false;
            }
            let handler_span = match &fact.hook.handler {
                perl_semantic_facts::handler::FrameworkHandler::InlineSub { anchor }
                | perl_semantic_facts::handler::FrameworkHandler::StaticCoderef {
                    anchor, ..
                } => Some(*anchor),
                perl_semantic_facts::handler::FrameworkHandler::Bounded { anchor, .. } => *anchor,
                _ => None,
            };
            !handler_span.is_some_and(|anchor| span_contains(&anchor, offset))
        })
    }

    /// The canonical handler-context fact whose interval contains `offset`.
    ///
    /// This is the one context query for the Dancer2 cell: route handlers and
    /// admitted hook handlers are both minted into `handler_contexts`, so no
    /// consumer needs its own span heuristic for either. The returned fact
    /// carries `handler_kind` (route vs hook) and `request_context` (whether
    /// the reviewed contract establishes request-scoped keyword availability
    /// inside the interval).
    ///
    /// A `Some(..)` answer means "an exact handler body owns this offset". It
    /// does **not** by itself mean request-scoped keywords are available —
    /// gate that on [`RouteHandlerContextFact::establishes_request_context`],
    /// or use [`Self::inside_request_context`].
    /// When intervals overlap the innermost one wins, so the answer does not
    /// depend on minting order. The extractors do not currently descend into
    /// a handler body, so overlap cannot arise today; selecting the narrowest
    /// containing interval keeps this query correct without depending on that
    /// invariant holding forever.
    #[must_use]
    pub fn request_context_at(&self, offset: usize) -> Option<&RouteHandlerContextFact> {
        self.handler_contexts
            .iter()
            .filter(|context| span_contains(&context.envelope.anchor, offset))
            .min_by_key(|context| {
                context.envelope.anchor.end_byte.saturating_sub(context.envelope.anchor.start_byte)
            })
    }

    /// Whether the reviewed contract establishes request context at `offset`.
    ///
    /// True inside an exact route handler and inside an admitted hook
    /// handler; false inside a hook position whose request context the
    /// reviewed contract does not establish, and false outside every handler.
    #[must_use]
    pub fn inside_request_context(&self, offset: usize) -> bool {
        self.request_context_at(offset)
            .is_some_and(RouteHandlerContextFact::establishes_request_context)
    }

    /// Whether `offset` lies inside one of the minted inline handler spans.
    ///
    /// Retained as the containment predicate; prefer
    /// [`Self::inside_request_context`] when deciding keyword availability,
    /// because an exact handler interval alone does not establish it.
    #[must_use]
    pub fn inside_handler_context(&self, offset: usize) -> bool {
        self.request_context_at(offset).is_some()
    }
}

/// Whether a source anchor's byte span contains `offset`.
fn span_contains(anchor: &perl_semantic_facts::SourceAnchor, offset: usize) -> bool {
    usize::try_from(anchor.start_byte).ok() <= Some(offset)
        && offset < usize::try_from(anchor.end_byte).unwrap_or(0)
}

/// Mint the canonical Dancer2 fact family for one document.
///
/// Uses only the canonical producers; when no package is exactly activated
/// the result is empty (zero facts of any kind). The generation embedded in
/// each activation's exact state (from [`super::activation::file_activations`])
/// is the generation the facts mint with.
#[must_use]
pub fn canonical_file_facts(
    ast: &Node,
    file_id: FileId,
    activations: &Dancer2FileActivations,
) -> CanonicalDancer2FileFacts {
    let mut facts = CanonicalDancer2FileFacts::default();
    let Some(detection) = &activations.detection else {
        return facts;
    };

    let route_contexts = extract_dancer2_route_contexts(ast, file_id);
    let hook_declarations: Vec<Dancer2HookDeclaration> =
        extract_dancer2_hook_declarations(ast, file_id);

    for activation in &activations.packages {
        if !activation.facts.is_exact() {
            continue;
        }
        let package = Some(activation.package.as_str());
        for declaration in &route_contexts.routes {
            if declaration.package.as_deref() == package {
                facts.extracted_routes.push(declaration.clone());
            }
        }
        let family: Dancer2RouteFacts = dancer2_route_family_facts(
            detection,
            &activation.facts,
            package,
            &route_contexts.routes,
            &route_contexts.prefixes,
        );
        facts.routes.extend(family.routes);
        facts.prefixes.extend(family.prefixes);
        facts.parameters.extend(family.parameters);
        facts.handler_contexts.extend(family.handler_contexts);
        facts.hooks.extend(dancer2_hook_facts(
            detection,
            &activation.facts,
            package,
            &hook_declarations,
        ));
        // Hook handler bodies join the same handler-context family as route
        // handlers, so one query answers both (#13604).
        facts.handler_contexts.extend(dancer2_hook_handler_context_facts(
            detection,
            &activation.facts,
            package,
            &hook_declarations,
        ));
    }
    facts
}
