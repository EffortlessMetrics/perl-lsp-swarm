//! Registry-activated Dancer2 hook fact minting (#8924).
//!
//! This module turns source-extracted Dancer2 hook declarations into the
//! canonical [`HookFact`] family. It is the facts side of the #8924 hook
//! contract and builds directly on the #8914 activation seam:
//!
//! - hook facts mint **only through the registry-activated adapter**: a
//!   detected framework and an exact activation are both required; without
//!   them this function returns no facts (never name-only hook synthesis);
//! - a declaration whose `hook` keyword the activating import excluded via
//!   `!hook` is not a hook of this activation — the keyword was never
//!   imported, so no fact is minted;
//! - every fact is generation-owned: the envelope carries the detection
//!   receipt's project generation and invalidation dependencies over the
//!   owning source file and the activating `Dancer2` module;
//! - facts are shadow receipts: the adapter disposition remains `Shadow`, so
//!   no provider surface can publish them yet (#6822 owns publication).
//!
//! Alias authority is the reviewed Dancer2 1.1.1 contract
//! (`Dancer2::Core::App::hook_aliases` plus the `Dancer2::Core::Hook` name
//! coerce). Normalization is explicit and version-pinned: a literal in the
//! reviewed alias table resolves to its canonical hook name; a literal that
//! already **is** a reviewed canonical name stays canonical; anything else
//! keeps its literal name behind a typed boundary — plugin/engine/runtime
//! aliases (`all_hook_aliases` merges plugin aliases at runtime in upstream
//! Dancer2) are never guessed from dotted spelling.

use crate::framework::AdapterDetectionResult;
use crate::framework_adapters::dancer2::{
    DANCER2_ADAPTER_ID, DANCER2_DSL_CONTRACT_VERSION, DANCER2_FRAMEWORK_NAME,
    Dancer2ActivationFacts, Dancer2KeywordState,
};
use crate::handler::{FrameworkHandler, FrameworkHandlerBoundary};
use crate::hook::{
    HookDeclaration, HookFact, HookName, HookNameNormalization, HookNameSelection, hook_envelope,
    hook_fact_identity, hook_handler_context_identity,
};
use crate::route::{HandlerContextKind, RequestContextAdmission, RouteHandlerContextFact};
use crate::{
    AnchorId, BoundaryKind, FileId, InvalidationDependency, SemanticReasonCode, SourceAnchor,
    SourceGeneration,
};

/// Versioned identity of the reviewed Dancer2 hook/alias contract.
///
/// The alias table and canonical name set follow Dancer2 1.1.1
/// (`Dancer2::Core::App::hook_aliases`, `Dancer2::Core::App::supported_hooks`,
/// and the `Dancer2::Core::Hook` name coerce); the workspace
/// `dancer2_skeleton` fixture carries `1.1.1`.
pub const DANCER2_HOOK_CONTRACT_VERSION: &str = "dancer2-hooks.1-1.v1";

/// Hook keyword of the reviewed Dancer2 DSL contract.
pub const DANCER2_HOOK_KEYWORD: &str = "hook";

/// Reviewed alias table: literal hook name → canonical hook name.
///
/// Verbatim from Dancer2 1.1.1 `Dancer2::Core::App::hook_aliases`, including
/// the Dancer 1 compatibility spellings. The `before_template` entry is the
/// composed two-stage normalization (the `Dancer2::Core::Hook` name coerce to
/// `before_template_render`, then the alias to `engine.template.before_render`).
pub const DANCER2_HOOK_ALIASES: &[(&str, &str)] = &[
    ("before", "core.app.before_request"),
    ("before_request", "core.app.before_request"),
    ("after", "core.app.after_request"),
    ("after_request", "core.app.after_request"),
    ("init_error", "core.error.init"),
    ("before_error", "core.error.before"),
    ("after_error", "core.error.after"),
    ("on_route_exception", "core.app.route_exception"),
    ("before_file_render", "core.app.before_file_render"),
    ("after_file_render", "core.app.after_file_render"),
    ("before_handler_file_render", "handler.file.before_render"),
    ("after_handler_file_render", "handler.file.after_render"),
    // Compatibility spellings from Dancer 1.
    ("before_error_render", "core.error.before"),
    ("after_error_render", "core.error.after"),
    ("before_error_init", "core.error.init"),
    // Engine hooks reachable through reviewed aliases.
    ("before_template_render", "engine.template.before_render"),
    ("after_template_render", "engine.template.after_render"),
    ("before_layout_render", "engine.template.before_layout_render"),
    ("after_layout_render", "engine.template.after_layout_render"),
    ("before_serializer", "engine.serializer.before"),
    ("after_serializer", "engine.serializer.after"),
    // `Dancer2::Core::Hook` name coerce followed by the alias table:
    // `before_template` → `before_template_render` →
    // `engine.template.before_render`.
    ("before_template", "engine.template.before_render"),
];

/// Reviewed canonical hook names.
///
/// The application hook positions of Dancer2 1.1.1
/// (`Dancer2::Core::App::supported_hooks`) plus every canonical alias target
/// of [`DANCER2_HOOK_ALIASES`] (engine/serializer/handler names are valid
/// statically-nameable hook positions upstream; their registration depends on
/// engine candidates but their identity is version-authoritative).
pub const DANCER2_CANONICAL_HOOK_NAMES: &[&str] = &[
    "core.app.before_request",
    "core.app.after_request",
    "core.app.route_exception",
    "core.app.before_file_render",
    "core.app.after_file_render",
    "core.error.before",
    "core.error.after",
    "core.error.init",
    "engine.template.before_render",
    "engine.template.after_render",
    "engine.template.before_layout_render",
    "engine.template.after_layout_render",
    "engine.serializer.before",
    "engine.serializer.after",
    "handler.file.before_render",
    "handler.file.after_render",
];

/// Reviewed canonical hook positions whose semantics establish request
/// context (#13604).
///
/// These are the application hook positions that Dancer2 1.1.1 dispatches
/// from `Dancer2::Core::App::_dispatch_route`/`response_internal` with the
/// current request already installed on the app, so the request-scoped DSL
/// keywords (`request`, `params`, `redirect`, `cookie`, `session`, ...) are
/// meaningful inside the handler body.
///
/// Every other reviewed canonical position is deliberately absent rather
/// than assumed:
///
/// - `core.error.*` can run for an error constructed outside any request;
/// - `engine.template.*` runs wherever `template` is called, including
///   outside a request;
/// - `engine.serializer.*` and `handler.file.*`/`core.app.*_file_render` are
///   engine/handler positions whose request scoping depends on the calling
///   site rather than on the hook position itself.
///
/// Absence here means *not established*, never *proven absent*: a consumer
/// must not offer request-scoped keywords in those positions, and must
/// equally not report using one there as wrong.
pub const DANCER2_REQUEST_CONTEXT_HOOKS: &[&str] =
    &["core.app.before_request", "core.app.after_request", "core.app.route_exception"];

/// Whether the reviewed contract establishes request context for a hook
/// whose name normalized to `canonical`.
#[must_use]
pub fn dancer2_hook_establishes_request_context(canonical: &str) -> bool {
    DANCER2_REQUEST_CONTEXT_HOOKS.contains(&canonical)
}

/// Name-coerce stage of the reviewed contract
/// (`Dancer2::Core::Hook` name coercion): `before_template` is renamed to
/// `before_template_render` before alias lookup.
#[must_use]
pub fn coerce_dancer2_hook_name(literal: &str) -> &str {
    if literal == "before_template" { "before_template_render" } else { literal }
}

/// Normalize one literal hook name against the reviewed contract.
///
/// Canonical membership wins first (a literal that already is a reviewed
/// canonical name stays canonical); then the reviewed alias table applies.
/// Any other literal — bareword or dotted — stays unresolved: ownership may
/// belong to a plugin, an engine candidate, or nothing (upstream
/// `add_hook` croaks on invalid names at runtime), and dotted spelling alone
/// never proves ownership.
#[must_use]
pub fn normalize_dancer2_hook_name(literal: &str) -> HookNameNormalization {
    let coerced = coerce_dancer2_hook_name(literal);
    if DANCER2_CANONICAL_HOOK_NAMES.contains(&coerced) {
        return HookNameNormalization::Canonical;
    }
    if let Some((_, canonical)) = DANCER2_HOOK_ALIASES.iter().find(|(alias, _)| *alias == coerced) {
        return HookNameNormalization::Alias { canonical: (*canonical).to_string() };
    }
    HookNameNormalization::Unresolved {
        reason: format!(
            "hook name `{literal}` is not in the reviewed Dancer2 alias/canonical contract \
             ({DANCER2_HOOK_CONTRACT_VERSION}); plugin/engine/runtime ownership is not \
             statically provable"
        ),
    }
}

/// One source-extracted Dancer2 hook declaration awaiting minting.
///
/// Produced by the AST extractor in
/// `perl_semantic_analyzer::analysis::dancer2_hooks`; this carrier adds the
/// package/file/declaration identity around the canonical payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2HookDeclaration {
    /// Package the declaration appears in (activation scope).
    pub package: Option<String>,
    /// File the declaration appears in.
    pub file_id: FileId,
    /// Full declaration range (keyword start to last operand end).
    pub declaration_start_byte: u32,
    pub declaration_end_byte: u32,
    /// Canonical hook payload (name/handler).
    pub hook: HookDeclaration,
}

/// Mint canonical Dancer2 hook facts for one activating package.
///
/// Returns an empty vector unless `detection` established the framework and
/// `activation` is exact (registry-activated adapter contract). Declarations
/// of other packages and declarations behind a `!hook` exclusion at the
/// activating import are skipped. All minted facts carry the detection
/// receipt's generation and invalidation dependencies over the owning source
/// file and the `Dancer2` module.
#[must_use]
pub fn dancer2_hook_facts(
    detection: &AdapterDetectionResult,
    activation: &Dancer2ActivationFacts,
    package: Option<&str>,
    declarations: &[Dancer2HookDeclaration],
) -> Vec<HookFact> {
    if !detection.is_detected() || !activation.is_exact() {
        return Vec::new();
    }
    let Dancer2ActivationFacts { state, keywords, .. } = activation;
    let crate::framework_adapters::dancer2::Dancer2ActivationState::Exact {
        application_name,
        framework_version,
        source_generation,
    } = state
    else {
        return Vec::new();
    };

    // The activating import's `hook` keyword gates the whole family: without
    // the imported keyword (`!hook`, or an import list without it) no
    // declaration of this activation is a hook, so the lookup and exclusion
    // check are hoisted out of the per-declaration loop.
    if !keywords.iter().any(|fact| {
        fact.keyword == DANCER2_HOOK_KEYWORD && fact.state == Dancer2KeywordState::Imported
    }) {
        return Vec::new();
    }

    let mut facts = Vec::new();
    for declaration in declarations {
        if declaration.package.as_deref() != package {
            continue;
        }
        if declaration.hook.keyword != DANCER2_HOOK_KEYWORD {
            continue;
        }
        facts.push(mint_hook_fact(
            declaration,
            application_name,
            framework_version,
            source_generation,
        ));
    }
    facts
}

/// Mint canonical handler-context facts for one activating package's hooks
/// (#13604).
///
/// Gating is identical to [`dancer2_hook_facts`] — detected framework, exact
/// activation, imported `hook` keyword, matching package — so a hook can
/// never mint a context the hook family itself would not mint.
///
/// A context exists only for an **exact inline handler body**. A bounded
/// handler relation (string, static coderef, computed) has no owned source
/// interval, so it can claim no DSL availability; that matches the route
/// producer's rule.
///
/// The hook *name* decides [`RequestContextAdmission`], not whether the fact
/// exists: a reviewed position in [`DANCER2_REQUEST_CONTEXT_HOOKS`] is
/// `Established`; every other inline hook handler — including an unresolved
/// or dynamic name, which may well be a plugin position that does run in a
/// request — is `NotEstablished`. Minting the interval either way keeps the
/// "we do not know" case distinguishable from "there is no handler here",
/// which is what a consumer needs to avoid both a missing completion and a
/// false diagnostic.
#[must_use]
pub fn dancer2_hook_handler_context_facts(
    detection: &AdapterDetectionResult,
    activation: &Dancer2ActivationFacts,
    package: Option<&str>,
    declarations: &[Dancer2HookDeclaration],
) -> Vec<RouteHandlerContextFact> {
    if !detection.is_detected() || !activation.is_exact() {
        return Vec::new();
    }
    let Dancer2ActivationFacts { state, keywords, .. } = activation;
    let crate::framework_adapters::dancer2::Dancer2ActivationState::Exact {
        application_name,
        framework_version,
        source_generation,
    } = state
    else {
        return Vec::new();
    };

    if !keywords.iter().any(|fact| {
        fact.keyword == DANCER2_HOOK_KEYWORD && fact.state == Dancer2KeywordState::Imported
    }) {
        return Vec::new();
    }

    let mut facts = Vec::new();
    for declaration in declarations {
        if declaration.package.as_deref() != package {
            continue;
        }
        if declaration.hook.keyword != DANCER2_HOOK_KEYWORD {
            continue;
        }
        // Only an inline body owns a source interval; every bounded relation
        // is skipped rather than anchored at the declaration.
        let FrameworkHandler::InlineSub { anchor } = &declaration.hook.handler else {
            continue;
        };
        facts.push(mint_hook_handler_context_fact(
            declaration,
            *anchor,
            application_name,
            framework_version,
            source_generation,
        ));
    }
    facts
}

fn mint_hook_handler_context_fact(
    declaration: &Dancer2HookDeclaration,
    handler_anchor: SourceAnchor,
    application_name: &str,
    framework_version: &str,
    generation: &SourceGeneration,
) -> RouteHandlerContextFact {
    let (fact_id, entity_id) = hook_handler_context_identity(
        declaration.file_id,
        declaration.hook.declaration_index,
        generation,
    );
    let admission = match &declaration.hook.name {
        HookNameSelection::Literal(name) => match name.canonical() {
            Some(canonical) if dancer2_hook_establishes_request_context(canonical) => {
                RequestContextAdmission::Established
            }
            _ => RequestContextAdmission::NotEstablished,
        },
        HookNameSelection::Dynamic { .. } => RequestContextAdmission::NotEstablished,
    };
    // The interval itself is an exact source fact in every minted case; the
    // admission field, not the envelope, carries what the reviewed contract
    // does or does not establish about availability inside it.
    let envelope = hook_envelope(
        fact_id,
        entity_id,
        declaration.package.as_deref(),
        handler_anchor,
        generation,
        vec![
            InvalidationDependency::new(
                format!("source:{}", declaration.file_id.0),
                generation.clone(),
            ),
            InvalidationDependency::new("module:Dancer2", generation.clone()),
        ],
        None,
        SemanticReasonCode::ExactSource,
        true,
    );
    // `RouteHandlerContextFact::new` forces the envelope kind to
    // `RouteHandlerContext`, so this stays one handler-context fact family
    // and needs no new `SemanticFactKind` discriminant (which would require
    // a framework-adapter schema version bump).
    RouteHandlerContextFact::new(
        envelope,
        DANCER2_FRAMEWORK_NAME,
        DANCER2_ADAPTER_ID,
        framework_version,
        application_name,
        declaration.hook.declaration_index,
        DANCER2_DSL_CONTRACT_VERSION,
    )
    .with_handler_kind(HandlerContextKind::Hook)
    .with_request_context(admission)
}

fn mint_hook_fact(
    declaration: &Dancer2HookDeclaration,
    application_name: &str,
    framework_version: &str,
    generation: &SourceGeneration,
) -> HookFact {
    let (fact_id, entity_id) =
        hook_fact_identity(declaration.file_id, declaration.hook.declaration_index, generation);
    let declaration_anchor = SourceAnchor::new(
        Some(AnchorId(u64::from(declaration.declaration_start_byte))),
        declaration.file_id,
        declaration.declaration_start_byte,
        declaration.declaration_end_byte,
    );
    let exact = !declaration.hook.has_boundary();
    let (boundary_kind, boundary_reason) = primary_boundary(&declaration.hook);
    let envelope = hook_envelope(
        fact_id,
        entity_id,
        declaration.package.as_deref(),
        declaration_anchor,
        generation,
        vec![
            InvalidationDependency::new(
                format!("source:{}", declaration.file_id.0),
                generation.clone(),
            ),
            InvalidationDependency::new("module:Dancer2", generation.clone()),
        ],
        boundary_kind,
        boundary_reason,
        exact,
    );
    HookFact::new(
        envelope,
        DANCER2_FRAMEWORK_NAME,
        DANCER2_ADAPTER_ID,
        framework_version,
        application_name,
        declaration.hook.clone(),
    )
}

/// Primary envelope boundary for a bounded payload, in a fixed review order.
fn primary_boundary(hook: &HookDeclaration) -> (Option<BoundaryKind>, SemanticReasonCode) {
    if let HookNameSelection::Dynamic { .. } = &hook.name {
        return (Some(BoundaryKind::DynamicValue), SemanticReasonCode::DynamicValue);
    }
    if let HookNameSelection::Literal(HookName {
        normalization: HookNameNormalization::Unresolved { .. },
        ..
    }) = &hook.name
    {
        return (Some(BoundaryKind::Compatibility), SemanticReasonCode::CompatibilityBoundary);
    }
    if let FrameworkHandler::Bounded { boundary, .. } = &hook.handler {
        return (
            Some(match boundary {
                FrameworkHandlerBoundary::String => BoundaryKind::Compatibility,
                FrameworkHandlerBoundary::StaticCoderef | FrameworkHandlerBoundary::Computed => {
                    BoundaryKind::DynamicValue
                }
            }),
            SemanticReasonCode::DynamicValue,
        );
    }
    (None, SemanticReasonCode::ExactSource)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{
        AdapterCancellation, AdapterDetectionInput, DetectionEvidenceClass,
        ModuleActivationIdentity, ModuleObservationReceipt, ModuleSelectorEvaluation,
        ModuleSelectorOutcome, ModuleVersionEvidence,
    };
    use crate::framework_adapters::dancer2::{
        Dancer2ActivationState, DslSelection, dancer2_activation_facts, dancer2_descriptor,
        detect_dancer2, parse_dancer2_import_args,
    };
    use crate::handler::SubroutineTarget;
    use crate::{BoundaryKind, SemanticFactStatus};
    use perl_test_must::{must_some, must_some_with};

    fn detected_input(generation: &str) -> AdapterDetectionInput {
        let activation = ModuleActivationIdentity::new(
            "Dancer2",
            Some(FileId(7)),
            SourceGeneration::known(generation),
        )
        .with_observed_version(ModuleVersionEvidence::new(
            "1.1.1",
            SourceGeneration::known(generation),
        ));
        let observation = ModuleObservationReceipt::new(
            "module-resolver.v1",
            "root:fixture",
            "project-environment.v1",
            SourceGeneration::known(generation),
            "sha256:fixture-input",
            vec![ModuleSelectorEvaluation::new(
                "Dancer2",
                ModuleSelectorOutcome::Matched {
                    activation,
                    evidence_class: DetectionEvidenceClass::ResolvedModule,
                },
            )],
        );
        AdapterDetectionInput::new(
            dancer2_descriptor(),
            observation,
            None,
            AdapterCancellation::active(),
        )
    }

    fn exact_activation(generation: &str) -> Dancer2ActivationFacts {
        let detection = detect_dancer2(&detected_input(generation));
        dancer2_activation_facts(&detection, Some("App"), &parse_dancer2_import_args(&[]))
    }

    fn before_hook_declaration(index: u32) -> Dancer2HookDeclaration {
        Dancer2HookDeclaration {
            package: Some("App".to_string()),
            file_id: FileId(1),
            declaration_start_byte: 0,
            declaration_end_byte: 32,
            hook: HookDeclaration {
                declaration_index: index,
                keyword: "hook".to_string(),
                keyword_anchor: SourceAnchor::new(Some(AnchorId(0)), FileId(1), 0, 4),
                name: HookNameSelection::Literal(HookName {
                    literal: "before".to_string(),
                    anchor: SourceAnchor::new(Some(AnchorId(5)), FileId(1), 5, 13),
                    normalization: normalize_dancer2_hook_name("before"),
                }),
                handler: FrameworkHandler::InlineSub {
                    anchor: SourceAnchor::new(Some(AnchorId(16)), FileId(1), 16, 32),
                },
            },
        }
    }

    /// Build a hook declaration with a chosen literal name and handler.
    fn hook_declaration_named(
        index: u32,
        literal: &str,
        handler: FrameworkHandler,
    ) -> Dancer2HookDeclaration {
        let mut declaration = before_hook_declaration(index);
        declaration.hook.name = HookNameSelection::Literal(HookName {
            literal: literal.to_string(),
            anchor: SourceAnchor::new(Some(AnchorId(5)), FileId(1), 5, 13),
            normalization: normalize_dancer2_hook_name(literal),
        });
        declaration.hook.handler = handler;
        declaration
    }

    fn inline_handler() -> FrameworkHandler {
        FrameworkHandler::InlineSub {
            anchor: SourceAnchor::new(Some(AnchorId(16)), FileId(1), 16, 32),
        }
    }

    fn hook_contexts(declarations: &[Dancer2HookDeclaration]) -> Vec<RouteHandlerContextFact> {
        let detection = detect_dancer2(&detected_input("g1"));
        let activation = exact_activation("g1");
        dancer2_hook_handler_context_facts(&detection, &activation, Some("App"), declarations)
    }

    #[test]
    fn inline_admitted_hook_mints_an_established_request_context() {
        let contexts = hook_contexts(&[before_hook_declaration(0)]);
        assert_eq!(contexts.len(), 1, "{contexts:?}");
        let context = &contexts[0];
        assert_eq!(context.handler_kind, HandlerContextKind::Hook);
        assert_eq!(context.request_context, RequestContextAdmission::Established);
        assert!(context.establishes_request_context());
        // The interval is the handler body, not the whole declaration.
        assert_eq!(context.envelope.anchor.start_byte, 16);
        assert_eq!(context.envelope.anchor.end_byte, 32);
        // Stays inside the one handler-context fact family: no new
        // `SemanticFactKind` discriminant, so no schema version bump.
        assert_eq!(context.envelope.kind, crate::SemanticFactKind::RouteHandlerContext);
        assert_eq!(context.status(), SemanticFactStatus::Exact);
    }

    #[test]
    fn every_admitted_spelling_reaches_the_same_established_admission() {
        // Alias, Dancer 1 compatibility spelling, and the canonical name
        // itself must all land on the same reviewed position.
        for literal in [
            "before",
            "before_request",
            "core.app.before_request",
            "after",
            "after_request",
            "on_route_exception",
        ] {
            let contexts = hook_contexts(&[hook_declaration_named(0, literal, inline_handler())]);
            assert_eq!(contexts.len(), 1, "`{literal}` must mint a context");
            assert_eq!(
                contexts[0].request_context,
                RequestContextAdmission::Established,
                "`{literal}` is a reviewed request-context position"
            );
        }
    }

    #[test]
    fn reviewed_but_unadmitted_hook_positions_are_not_established() {
        // These are reviewed canonical positions, so a fact exists (the
        // interval is exact) — but the contract does not establish request
        // context, so availability must not be claimed.
        for literal in [
            "before_template_render",
            "after_template_render",
            "before_serializer",
            "init_error",
            "before_error",
            "before_file_render",
        ] {
            let contexts = hook_contexts(&[hook_declaration_named(0, literal, inline_handler())]);
            assert_eq!(contexts.len(), 1, "`{literal}` still owns an exact interval");
            assert_eq!(
                contexts[0].request_context,
                RequestContextAdmission::NotEstablished,
                "`{literal}` must not claim request context"
            );
            assert!(!contexts[0].establishes_request_context());
        }
    }

    #[test]
    fn unresolved_and_dynamic_hook_names_mint_an_unestablished_context() {
        // A plugin/engine position may well run in a request; we cannot
        // prove it. The interval is retained so a consumer can tell "unknown"
        // apart from "no handler here", but nothing is claimed.
        let unresolved =
            hook_contexts(&[hook_declaration_named(0, "some_plugin_position", inline_handler())]);
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].request_context, RequestContextAdmission::NotEstablished);

        let mut dynamic = before_hook_declaration(0);
        dynamic.hook.name = HookNameSelection::Dynamic {
            reason: "computed hook name".to_string(),
            anchor: SourceAnchor::new(Some(AnchorId(5)), FileId(1), 5, 13),
        };
        let dynamic = hook_contexts(&[dynamic]);
        assert_eq!(dynamic.len(), 1);
        assert_eq!(dynamic[0].request_context, RequestContextAdmission::NotEstablished);
    }

    #[test]
    fn a_bounded_handler_relation_mints_no_context_at_all() {
        // No inline body means no owned source interval; anchoring at the
        // declaration would let operand text claim handler availability.
        for boundary in [
            FrameworkHandlerBoundary::String,
            FrameworkHandlerBoundary::StaticCoderef,
            FrameworkHandlerBoundary::Computed,
        ] {
            let handler = FrameworkHandler::Bounded {
                reason: "bounded".to_string(),
                boundary,
                anchor: Some(SourceAnchor::new(Some(AnchorId(16)), FileId(1), 16, 32)),
            };
            let contexts = hook_contexts(&[hook_declaration_named(0, "before", handler)]);
            assert!(contexts.is_empty(), "{boundary:?} must mint no handler context");
        }
    }

    #[test]
    fn context_minting_requires_the_same_activation_gate_as_hook_facts() {
        let declarations = [before_hook_declaration(0)];
        let detection = detect_dancer2(&detected_input("g1"));

        // Undetected framework.
        let undetected = AdapterDetectionResult::new(
            dancer2_descriptor(),
            SourceGeneration::known("g1"),
            crate::framework::DetectionOutcome::Absent {
                reason: crate::framework::DetectionAbsenceReason::RequiredModulesMissing,
            },
        );
        assert!(
            dancer2_hook_handler_context_facts(
                &undetected,
                &exact_activation("g1"),
                Some("App"),
                &declarations
            )
            .is_empty(),
            "no detection means no context"
        );

        // `!hook` at the activating import: the keyword was never imported,
        // so nothing in this file is a hook of this activation.
        let excluded = dancer2_activation_facts(
            &detection,
            Some("App"),
            &parse_dancer2_import_args(&["!hook".to_string()]),
        );
        assert!(
            dancer2_hook_handler_context_facts(&detection, &excluded, Some("App"), &declarations)
                .is_empty(),
            "an excluded `hook` keyword mints no context"
        );

        // Another package's declaration.
        assert!(
            dancer2_hook_handler_context_facts(
                &detection,
                &exact_activation("g1"),
                Some("Other"),
                &declarations
            )
            .is_empty(),
            "a declaration of another package mints no context"
        );
    }

    #[test]
    fn hook_context_identity_is_disjoint_from_its_hook_and_from_a_route_context() {
        let generation = SourceGeneration::known("g1");
        let (hook_fact_id, hook_entity) = hook_fact_identity(FileId(1), 0, &generation);
        let (context_fact_id, context_entity) =
            crate::hook::hook_handler_context_identity(FileId(1), 0, &generation);
        let (route_context_fact_id, _) =
            crate::route::route_handler_context_identity(FileId(1), 0, &generation);

        // The context is a second fact about the same hook entity...
        assert_eq!(context_entity, hook_entity);
        // ...but never collides with the hook fact or a route context of the
        // same file/order/generation.
        assert_ne!(context_fact_id, hook_fact_id);
        assert_ne!(context_fact_id, route_context_fact_id);
    }

    #[test]
    fn the_admitted_request_context_set_is_an_exact_ratchet() {
        // Widening request-context admission is a reviewed contract change,
        // not an implementation detail: this fails if a position is added or
        // removed without updating the reviewed table deliberately.
        assert_eq!(
            DANCER2_REQUEST_CONTEXT_HOOKS,
            ["core.app.before_request", "core.app.after_request", "core.app.route_exception"]
        );
        for canonical in DANCER2_CANONICAL_HOOK_NAMES {
            assert_eq!(
                dancer2_hook_establishes_request_context(canonical),
                DANCER2_REQUEST_CONTEXT_HOOKS.contains(canonical),
                "`{canonical}` admission must come from the reviewed table alone"
            );
        }
        // A name outside the reviewed canonical set never establishes it.
        assert!(!dancer2_hook_establishes_request_context("core.app.not_a_hook"));
    }

    #[test]
    fn a_handler_context_payload_round_trips_and_older_payloads_decode_as_route_contexts()
    -> Result<(), serde_json::Error> {
        let contexts = hook_contexts(&[before_hook_declaration(0)]);
        let encoded = serde_json::to_string(&contexts)?;
        assert_eq!(serde_json::from_str::<Vec<RouteHandlerContextFact>>(&encoded)?, contexts);

        // Backward compatibility: a payload minted before the hook producer
        // existed carries neither additive field and must decode as exactly
        // what it was — an established route context.
        let mut legacy = serde_json::to_value(&contexts[0])?;
        let object = must_some_with(legacy.as_object_mut(), "fact encodes as a JSON object");
        object.remove("handler_kind");
        object.remove("request_context");
        let decoded: RouteHandlerContextFact = serde_json::from_value(legacy)?;
        assert_eq!(decoded.handler_kind, HandlerContextKind::Route);
        assert_eq!(decoded.request_context, RequestContextAdmission::Established);
        Ok(())
    }

    #[test]
    fn reviewed_alias_table_matches_the_1_1_contract() {
        // Spot anchors of the verbatim upstream table.
        let table = DANCER2_HOOK_ALIASES.to_vec();
        for (alias, canonical) in [
            ("before", "core.app.before_request"),
            ("after", "core.app.after_request"),
            ("before_serializer", "engine.serializer.before"),
            ("before_template", "engine.template.before_render"),
            ("before_error_render", "core.error.before"),
        ] {
            assert!(table.contains(&(alias, canonical)), "`{alias}` must alias `{canonical}`");
        }
        // Every alias target is a reviewed canonical name.
        for (_, canonical) in DANCER2_HOOK_ALIASES {
            assert!(
                DANCER2_CANONICAL_HOOK_NAMES.contains(canonical),
                "alias target `{canonical}` must be canonical"
            );
        }
    }

    #[test]
    fn normalization_is_explicit_and_version_authoritative() {
        assert_eq!(
            normalize_dancer2_hook_name("core.app.before_request"),
            HookNameNormalization::Canonical
        );
        assert_eq!(
            normalize_dancer2_hook_name("engine.serializer.after"),
            HookNameNormalization::Canonical
        );
        assert_eq!(
            normalize_dancer2_hook_name("before"),
            HookNameNormalization::Alias { canonical: "core.app.before_request".to_string() }
        );
        // Two-stage coerce + alias.
        assert_eq!(
            normalize_dancer2_hook_name("before_template"),
            HookNameNormalization::Alias { canonical: "engine.template.before_render".to_string() }
        );
        // Dancer 1 compatibility spelling.
        assert_eq!(
            normalize_dancer2_hook_name("after_error_render"),
            HookNameNormalization::Alias { canonical: "core.error.after".to_string() }
        );
    }

    #[test]
    fn unreviewed_names_stay_unresolved_boundaries() {
        for literal in [
            "before_auth",                // possible plugin alias
            "engine.custom.before_phase", // dotted, ownership unproven
            "plugin.database.before_dbi_connect",
            "core.app.not_a_reviewed_position",
        ] {
            assert!(
                matches!(
                    normalize_dancer2_hook_name(literal),
                    HookNameNormalization::Unresolved { .. }
                ),
                "`{literal}` must not claim canonical identity from spelling"
            );
        }
    }

    #[test]
    fn exact_activation_mints_generation_owned_hook_facts() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let activation = exact_activation("gen-1");
        let facts = dancer2_hook_facts(
            &detection,
            &activation,
            Some("App"),
            &[before_hook_declaration(0), before_hook_declaration(1)],
        );
        assert_eq!(facts.len(), 2, "same-name hooks stay distinct by order");
        for (index, fact) in facts.iter().enumerate() {
            assert_eq!(fact.framework_name, DANCER2_FRAMEWORK_NAME);
            assert_eq!(fact.adapter_id, DANCER2_ADAPTER_ID);
            assert_eq!(fact.framework_version, "1.1.1");
            assert_eq!(fact.application_name, "App");
            assert_eq!(fact.envelope.package.as_deref(), Some("App"));
            assert_eq!(fact.envelope.source_generation, SourceGeneration::known("gen-1"));
            assert_eq!(fact.hook.declaration_index, index as u32);
            assert_eq!(fact.status(), SemanticFactStatus::Exact);
        }
        assert_ne!(facts[0].envelope.fact_id, facts[1].envelope.fact_id);
    }

    #[test]
    fn no_activation_or_detection_mints_no_facts() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let mut activation = exact_activation("gen-1");
        activation.state = Dancer2ActivationState::NotActivated { reason: "test".to_string() };
        assert!(
            dancer2_hook_facts(&detection, &activation, Some("App"), &[before_hook_declaration(0)])
                .is_empty(),
            "a Dancer2-looking file in an unactivated context mints zero facts"
        );

        let observation = ModuleObservationReceipt::new(
            "module-resolver.v1",
            "root:fixture",
            "project-environment.v1",
            SourceGeneration::known("gen-1"),
            "sha256:fixture-input",
            vec![ModuleSelectorEvaluation::new("Dancer2", ModuleSelectorOutcome::Absent)],
        );
        let input = AdapterDetectionInput::new(
            dancer2_descriptor(),
            observation,
            None,
            AdapterCancellation::active(),
        );
        let undetected = detect_dancer2(&input);
        let activation =
            dancer2_activation_facts(&undetected, Some("App"), &parse_dancer2_import_args(&[]));
        assert!(
            dancer2_hook_facts(
                &undetected,
                &activation,
                Some("App"),
                &[before_hook_declaration(0)]
            )
            .is_empty()
        );
    }

    #[test]
    fn excluded_hook_keyword_mints_nothing() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let args: Vec<String> = ["qw(!hook)"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_import_args(&args);
        let activation = dancer2_activation_facts(&detection, Some("App"), &evidence);
        assert!(activation.is_exact(), "exclusion alone keeps activation exact");
        assert!(
            dancer2_hook_facts(&detection, &activation, Some("App"), &[before_hook_declaration(0)])
                .is_empty(),
            "`!hook` at the import means no hook keyword was imported"
        );
    }

    #[test]
    fn other_package_declarations_do_not_mint() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let activation = exact_activation("gen-1");
        let mut other = before_hook_declaration(0);
        other.package = Some("Other".to_string());
        assert!(dancer2_hook_facts(&detection, &activation, Some("App"), &[other]).is_empty());
    }

    #[test]
    fn unresolved_name_and_bounded_handler_degrade_with_distinct_boundaries() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let activation = exact_activation("gen-1");

        let mut unresolved = before_hook_declaration(0);
        unresolved.hook.name = HookNameSelection::Literal(HookName {
            literal: "before_auth".to_string(),
            anchor: SourceAnchor::new(Some(AnchorId(5)), FileId(1), 5, 17),
            normalization: normalize_dancer2_hook_name("before_auth"),
        });
        let facts = dancer2_hook_facts(&detection, &activation, Some("App"), &[unresolved]);
        assert_eq!(facts.len(), 1, "the declaration is retained");
        assert_eq!(facts[0].status(), SemanticFactStatus::Degraded);
        let boundary = must_some(facts[0].envelope.boundary.as_ref());
        assert_eq!(boundary.kind, BoundaryKind::Compatibility);

        let mut computed_handler = before_hook_declaration(0);
        computed_handler.hook.handler = FrameworkHandler::Bounded {
            boundary: FrameworkHandlerBoundary::Computed,
            anchor: Some(SourceAnchor::new(Some(AnchorId(16)), FileId(1), 16, 24)),
            reason: "computed handler".to_string(),
        };
        let facts = dancer2_hook_facts(&detection, &activation, Some("App"), &[computed_handler]);
        assert_eq!(facts[0].status(), SemanticFactStatus::Degraded);
        let boundary = must_some(facts[0].envelope.boundary.as_ref());
        assert_eq!(boundary.kind, BoundaryKind::DynamicValue);
    }

    #[test]
    fn resolved_static_coderef_handler_keeps_fact_exact() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let activation = exact_activation("gen-1");
        let mut declaration = before_hook_declaration(0);
        declaration.hook.handler = FrameworkHandler::StaticCoderef {
            name: "on_request".to_string(),
            anchor: SourceAnchor::new(Some(AnchorId(16)), FileId(1), 16, 27),
            target: SubroutineTarget {
                name: "on_request".to_string(),
                package: "App".to_string(),
                name_anchor: SourceAnchor::new(Some(AnchorId(40)), FileId(1), 40, 50),
                declaration_anchor: SourceAnchor::new(Some(AnchorId(35)), FileId(1), 35, 80),
                body_anchor: Some(SourceAnchor::new(Some(AnchorId(51)), FileId(1), 51, 80)),
            },
        };
        let facts = dancer2_hook_facts(&detection, &activation, Some("App"), &[declaration]);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].status(), SemanticFactStatus::Exact);
        assert!(facts[0].envelope.boundary.is_none());
    }

    #[test]
    fn custom_dsl_mints_nothing() {
        let detection = detect_dancer2(&detected_input("gen-1"));
        let mut evidence = parse_dancer2_import_args(&[]);
        evidence.dsl = Some(DslSelection::CustomLiteral("My::DSL".to_string()));
        let activation = dancer2_activation_facts(&detection, Some("App"), &evidence);
        assert!(!activation.is_exact());
        assert!(
            dancer2_hook_facts(&detection, &activation, Some("App"), &[before_hook_declaration(0)])
                .is_empty()
        );
    }
}
