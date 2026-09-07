//! Canonical framework route fact family (#8918).
//!
//! [`RouteFact`] is the transport-neutral canonical record for statically
//! observed framework route declarations (Dancer2 `get`/`post`/`any`/... style
//! DSL routes). It follows the [`crate::CallableResultFact`] precedent: a
//! payload fact wrapping the canonical [`SemanticFactEnvelope`] with the
//! envelope kind forced to [`SemanticFactKind::Route`], so providers can
//! classify a route fact without framework internals.
//!
//! The family is framework-neutral. Framework-specific minting (verb tables,
//! method normalization, activation gating) lives in concrete adapters such as
//! `framework_adapters::dancer2_routes`; this module only defines what a route
//! declaration must preserve: identity by source order, route name separate
//! from pattern, normalized method set, pattern kind, static options, handler
//! anchor, and the full declaration/keyword ranges. The handler relation
//! itself is the shared [`crate::handler`] contract (#8924 promoted the
//! static-coderef arm from a typed boundary to a resolved declaration fact).
//!
//! #8921 extends the family with the route context needed to understand
//! effective paths and route-handler-only DSL usage:
//!
//! - [`RouteEffectivePattern`] on every route declaration: the prefix-composed
//!   effective pattern (plain string concatenation, mirroring the reviewed
//!   Dancer2 1.x `Dancer2::Core::Route` BUILDARGS composition) with the
//!   source-order prefix declarations it depends on, or a typed boundary;
//! - [`RoutePrefixFact`]: one fact per statically supported `prefix`
//!   declaration (sticky set/clear and the lexical block form), anchored at
//!   exact tokens;
//! - [`RouteParameterFact`]: one fact per route-local parameter/capture
//!   segment of a literal route pattern (named `:param`, typed
//!   `:param[Type]`, splat `*`, megasplat `**`), anchored inside the pattern
//!   operand, with an explicit boundary where capture interpretation is not
//!   statically proven;
//! - [`RouteHandlerContextFact`]: the exact inline-handler source interval of
//!   one exact route, marking where route-handler-only DSL keywords are
//!   semantically available.
//!
//! Exactness contract:
//!
//! - a route fact is [`SemanticFactStatus::Exact`] only when the envelope is
//!   exact **and** the payload carries no dynamic or bounded member (literal
//!   pattern, exact method set, exact handler relation, literal options,
//!   composed
//!   or local effective pattern);
//! - dynamic/computed/bounded members keep the fact usable but degraded —
//!   typed boundaries, never false exact fields;
//! - two declarations with the same methods and pattern remain distinct route
//!   entities through `declaration_index` (source declaration identity).

use crate::framework::AdapterId;
use crate::{
    BoundaryDisposition, BoundaryKind, Confidence, EntityId, FactId, Provenance,
    SemanticConfidence, SemanticFactEnvelope, SemanticFactKind, SemanticFactStatus,
    SemanticFreshness, SemanticProducer, SemanticProvenance, SemanticReasonCode, SourceAnchor,
    SourceGeneration,
};
use serde::{Deserialize, Serialize};

// Shared handler-relation aliases (#8924): the route family keeps its public
// `RouteHandler` spelling while the arms live in the shared `handler` module
// so the hook family carries the identical contract.
pub use crate::handler::FrameworkHandler as RouteHandler;
pub use crate::handler::FrameworkHandlerBoundary as RouteHandlerBoundary;
pub use crate::handler::SubroutineTarget as RouteSubroutineTarget;

/// Kind of a route pattern operand.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RoutePatternKind {
    /// Literal string route pattern (e.g. `'/users/:id'`).
    Literal,
    /// Regex route pattern with an exact source anchor (e.g. `qr{^/re/(\d+)$}`).
    Regex,
    /// Dynamic/computed pattern — a boundary, not an exact pattern.
    Dynamic,
}

/// One route pattern with its exact source anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutePattern {
    /// Pattern classification.
    pub kind: RoutePatternKind,
    /// Exact source text of the pattern operand. `None` only for
    /// [`RoutePatternKind::Dynamic`].
    pub value: Option<String>,
    /// Source range of the pattern operand (exact tokens).
    pub anchor: SourceAnchor,
}

/// One literal route name with its exact source anchor.
///
/// A route name is optional in every supported grammar and is never a
/// substitute for the route pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteName {
    /// Literal route name value (unquoted).
    pub value: String,
    /// Source range of the name operand (exact tokens).
    pub anchor: SourceAnchor,
}

/// Route-name slot of one route declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteNameSelection {
    /// Literal route name.
    Literal(RouteName),
    /// Computed name operand — an explicit boundary.
    Dynamic {
        /// Bounded explanation.
        reason: String,
        /// Source range of the dynamic name operand.
        anchor: SourceAnchor,
    },
    /// No name operand in the declaration.
    Absent,
}

/// Value of one static matching option.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteOptionValue {
    /// Static literal option value (unquoted).
    Literal(String),
    /// Computed option value — an explicit limitation, not a literal.
    Dynamic {
        /// Bounded explanation.
        reason: String,
    },
}

/// One static matching option entry with source ranges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteOption {
    /// Option key (unquoted literal).
    pub key: String,
    /// Source range of the key operand.
    pub key_anchor: SourceAnchor,
    /// Option value.
    pub value: RouteOptionValue,
    /// Source range of the value operand.
    pub value_anchor: SourceAnchor,
}

/// Matching options of one route declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteOptions {
    /// Static literal option map (typed entries with ranges).
    Map(Vec<RouteOption>),
    /// Computed/unsupported option expression — an explicit boundary.
    Dynamic {
        /// Bounded explanation.
        reason: String,
        /// Source range of the dynamic options operand, when anchored.
        anchor: Option<SourceAnchor>,
    },
}

/// Normalized HTTP method set of one route declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteMethodSet {
    /// Exact normalized method names (uppercase, framework-normalized).
    Exact(Vec<String>),
    /// Computed method list — a boundary, never `ANY` exactness.
    Dynamic {
        /// Bounded explanation.
        reason: String,
    },
}

/// Effective (prefix-composed) pattern of one route declaration (#8921).
///
/// Mirrors the reviewed Dancer2 1.x `Dancer2::Core::Route` `BUILDARGS`
/// composition: a string pattern under a prefix registers as the plain
/// concatenation `prefix . pattern` (no slash insertion), and a string
/// pattern without a prefix is normalized to a leading `/`. Regex patterns
/// are never slash-normalized.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteEffectivePattern {
    /// Exact literal composition: the contributing literal prefix value(s)
    /// concatenated with the literal route pattern.
    Composed {
        /// Composed effective pattern (plain concatenation).
        value: String,
        /// Source-order indices of the prefix declarations this projection
        /// depends on (sticky set plus any enclosing lexical prefixes).
        prefix_declarations: Vec<u32>,
    },
    /// No active prefix: the route pattern alone, with the reviewed
    /// leading-`/` normalization for string patterns (regex patterns are
    /// carried as-is).
    Local {
        /// Effective pattern without any prefix contribution.
        value: String,
    },
    /// Composition is not exact: computed prefix, dynamic pattern, or a regex
    /// pattern under a literal prefix (whose `\Q`-quoted anchored composition
    /// is not a literal string).
    Boundary {
        /// Bounded explanation.
        reason: String,
    },
}

/// Deterministic local (unprefixed) projection of one declared pattern
/// value: string patterns gain the reviewed leading-`/` normalization,
/// regex patterns are carried as-is. Mirrors the upstream `Dancer2::Core::
/// Route` BUILDARGS normalization the analyzer composes under no prefix;
/// [`RouteFact::new`] canonicalizes `Local` values to it.
fn local_effective_value(pattern_value: &str, kind: RoutePatternKind) -> String {
    if kind == RoutePatternKind::Literal && !pattern_value.starts_with('/') {
        format!("/{pattern_value}")
    } else {
        pattern_value.to_string()
    }
}

/// One literal prefix value with its exact source anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutePrefixLiteral {
    /// Literal prefix value (unquoted).
    pub value: String,
    /// Source range of the prefix operand (exact tokens).
    pub anchor: SourceAnchor,
}

/// Prefix-selection slot of one prefix declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutePrefixSelection {
    /// Literal prefix value (sticky set, or the lexical block form).
    Literal(RoutePrefixLiteral),
    /// The prefix is cleared. Covers `prefix undef;` and the reviewed
    /// equivalent spellings (`prefix '/';` and an empty-string prefix, which
    /// the app-level prefix coercion reduces to no prefix).
    Cleared,
    /// Computed prefix operand — an explicit boundary; dependent route
    /// projections cannot be composed.
    Dynamic {
        /// Bounded explanation.
        reason: String,
        /// Source range of the dynamic operand, when anchored.
        anchor: Option<SourceAnchor>,
    },
}

/// Scope shape of one prefix declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutePrefixScope {
    /// One-argument `prefix VALUE;`: sets (or clears) the prefix for every
    /// following route declaration of the application, until the next prefix
    /// declaration.
    Sticky,
    /// `prefix VALUE => sub { ... };`: the reviewed lexical form. The block
    /// runs at load time, the effective prefix is the enclosing prefix
    /// concatenated with the literal operand, and the enclosing prefix state
    /// is restored after the block.
    Lexical {
        /// Source range of the load-time block (exact tokens).
        block_anchor: SourceAnchor,
    },
}

/// Canonical payload of one prefix declaration (#8921).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutePrefixDeclaration {
    /// Source-order prefix declaration identity within the owning file.
    pub declaration_index: u32,
    /// Prefix keyword token (`prefix`).
    pub keyword: String,
    /// Source range of the prefix keyword token.
    pub keyword_anchor: SourceAnchor,
    /// Prefix value selection.
    pub selection: RoutePrefixSelection,
    /// Scope shape.
    pub scope: RoutePrefixScope,
}

impl RoutePrefixDeclaration {
    /// Whether any payload member is a dynamic boundary.
    #[must_use]
    pub fn has_boundary(&self) -> bool {
        matches!(self.selection, RoutePrefixSelection::Dynamic { .. })
    }
}

/// Canonical framework route prefix fact: envelope plus prefix payload and
/// framework identity (#8921).
///
/// Deserialization is checked: wire payloads are rebuilt through
/// [`RoutePrefixFact::new`], so every constructor-side invariant (envelope
/// kind) also holds for decoded facts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoutePrefixFact {
    /// Canonical semantic envelope (kind forced to `RoutePrefix`).
    pub envelope: SemanticFactEnvelope,
    /// Framework name (e.g. `Dancer2`).
    pub framework_name: String,
    /// Producing adapter identity.
    pub adapter_id: AdapterId,
    /// Observed framework version the fact was minted against.
    pub framework_version: String,
    /// Owning application identity from the activating import.
    pub application_name: String,
    /// Prefix payload.
    pub prefix: RoutePrefixDeclaration,
}

#[derive(Deserialize)]
struct RoutePrefixFactWire {
    envelope: SemanticFactEnvelope,
    framework_name: String,
    adapter_id: AdapterId,
    framework_version: String,
    application_name: String,
    prefix: RoutePrefixDeclaration,
}

impl<'de> Deserialize<'de> for RoutePrefixFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = RoutePrefixFactWire::deserialize(deserializer)?;
        Ok(RoutePrefixFact::new(
            wire.envelope,
            wire.framework_name,
            wire.adapter_id,
            wire.framework_version,
            wire.application_name,
            wire.prefix,
        ))
    }
}

impl RoutePrefixFact {
    /// Construct a route prefix fact; the envelope kind is forced to
    /// [`SemanticFactKind::RoutePrefix`].
    #[must_use]
    pub fn new(
        mut envelope: SemanticFactEnvelope,
        framework_name: impl Into<String>,
        adapter_id: AdapterId,
        framework_version: impl Into<String>,
        application_name: impl Into<String>,
        prefix: RoutePrefixDeclaration,
    ) -> Self {
        envelope.kind = SemanticFactKind::RoutePrefix;
        Self {
            envelope,
            framework_name: framework_name.into(),
            adapter_id,
            framework_version: framework_version.into(),
            application_name: application_name.into(),
            prefix,
        }
    }

    /// Classify the complete prefix fact for a provider decision.
    #[must_use]
    pub fn status(&self) -> SemanticFactStatus {
        let envelope_status = self.envelope.status();
        if !matches!(envelope_status, SemanticFactStatus::Exact) {
            return envelope_status;
        }
        if self.prefix.has_boundary() {
            SemanticFactStatus::Degraded
        } else {
            SemanticFactStatus::Exact
        }
    }
}

/// Classification of one route-local parameter/capture segment (#8921).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteParameterKind {
    /// Named `:param` segment.
    Named,
    /// Typed `:param[Type]` segment. The token/type syntax is source-proven;
    /// the type constraint itself is runtime-validated and never proven
    /// statically (carried as the segment limitation).
    Typed {
        /// Declared type name (literal source text inside the brackets).
        type_name: String,
    },
    /// `*` splat: captures one URL segment.
    Splat,
    /// `**` megasplat: captures the remaining URL segments.
    Megasplat,
    /// The pattern's capture shape cannot be interpreted statically: a regex
    /// route pattern (no canonical regex fact layer proves its capture shape
    /// without runtime execution) or a literal composition whose
    /// prefix/pattern token boundary is ambiguous.
    CaptureUnsupported,
}

/// One route-local parameter/capture segment with its exact source anchor
/// inside the route pattern operand (#8921).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteParameterSegment {
    /// Segment classification.
    pub kind: RouteParameterKind,
    /// Key name for `:param`/`:param[Type]` segments; `None` for splat,
    /// megasplat, and unsupported-capture boundaries.
    pub name: Option<String>,
    /// Exact source range of the segment inside the pattern operand.
    pub anchor: SourceAnchor,
    /// Retained limitation of this segment, when any (e.g. a declared type
    /// constraint that static analysis cannot validate).
    pub limitation: Option<String>,
}

/// Canonical route-local parameter/capture fact: envelope plus segment
/// payload, route reference, and framework identity (#8921).
///
/// The fact is route-scoped by construction: it names the owning route
/// declaration and application, and the observed keys are never a globally
/// closed schema outside the owning route.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteParameterFact {
    /// Canonical semantic envelope (kind forced to `RouteParameter`).
    pub envelope: SemanticFactEnvelope,
    /// Framework name (e.g. `Dancer2`).
    pub framework_name: String,
    /// Producing adapter identity.
    pub adapter_id: AdapterId,
    /// Observed framework version the fact was minted against.
    pub framework_version: String,
    /// Owning application identity from the activating import.
    pub application_name: String,
    /// Owning route declaration (source-order identity within the file).
    pub route_declaration_index: u32,
    /// Parameter/capture segment.
    pub parameter: RouteParameterSegment,
}

#[derive(Deserialize)]
struct RouteParameterFactWire {
    envelope: SemanticFactEnvelope,
    framework_name: String,
    adapter_id: AdapterId,
    framework_version: String,
    application_name: String,
    route_declaration_index: u32,
    parameter: RouteParameterSegment,
}

impl<'de> Deserialize<'de> for RouteParameterFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = RouteParameterFactWire::deserialize(deserializer)?;
        Ok(RouteParameterFact::new(
            wire.envelope,
            wire.framework_name,
            wire.adapter_id,
            wire.framework_version,
            wire.application_name,
            wire.route_declaration_index,
            wire.parameter,
        ))
    }
}

impl RouteParameterFact {
    /// Construct a route parameter fact; the envelope kind is forced to
    /// [`SemanticFactKind::RouteParameter`].
    #[must_use]
    pub fn new(
        mut envelope: SemanticFactEnvelope,
        framework_name: impl Into<String>,
        adapter_id: AdapterId,
        framework_version: impl Into<String>,
        application_name: impl Into<String>,
        route_declaration_index: u32,
        parameter: RouteParameterSegment,
    ) -> Self {
        envelope.kind = SemanticFactKind::RouteParameter;
        Self {
            envelope,
            framework_name: framework_name.into(),
            adapter_id,
            framework_version: framework_version.into(),
            application_name: application_name.into(),
            route_declaration_index,
            parameter,
        }
    }

    /// Classify the complete parameter fact for a provider decision.
    ///
    /// An unsupported-capture segment stays degraded even under an exact
    /// envelope: it records a boundary, never a false exact key.
    #[must_use]
    pub fn status(&self) -> SemanticFactStatus {
        let envelope_status = self.envelope.status();
        if !matches!(envelope_status, SemanticFactStatus::Exact) {
            return envelope_status;
        }
        if matches!(self.parameter.kind, RouteParameterKind::CaptureUnsupported) {
            SemanticFactStatus::Degraded
        } else {
            SemanticFactStatus::Exact
        }
    }
}

/// Which framework declaration owns a handler-context interval.
///
/// The interval semantics are identical for both kinds — an exact inline
/// handler body of an exactly activated application — so one fact family
/// answers both. The kind names the owning declaration so a consumer can
/// explain *why* the context exists without rediscovering the syntax.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum HandlerContextKind {
    /// The interval is the inline handler of a route declaration.
    ///
    /// Default for wire compatibility: payloads minted before the hook
    /// handler-context producer existed are route contexts.
    #[default]
    Route,
    /// The interval is the inline handler of a hook declaration.
    Hook,
}

/// Whether the reviewed contract establishes framework request context
/// inside a handler interval.
///
/// This is deliberately separate from the envelope's exactness. The envelope
/// answers "is this interval an exact source fact"; this answers "does the
/// reviewed framework contract prove that request-scoped DSL keywords are
/// meaningful inside it". A handler position can have a perfectly exact
/// interval whose request context the reviewed contract does not establish.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum RequestContextAdmission {
    /// The reviewed contract establishes request context in this interval.
    ///
    /// Default for wire compatibility: the pre-existing producer minted
    /// contexts only for route handlers, which the reviewed contract admits.
    #[default]
    Established,
    /// The interval is exact, but the reviewed contract does not establish
    /// request context for this handler position.
    ///
    /// Absence of proof is not proof of absence: a consumer must not offer
    /// request-scoped keywords here, and must equally not claim that using
    /// one here is wrong.
    NotEstablished,
}

/// Canonical handler-context fact: the exact inline-handler source interval
/// of one exact route (#8921) or hook (#13604) declaration.
///
/// The interval marks where request-scoped DSL keywords of the reviewed DSL
/// contract may be available: it is application scoped, current-generation,
/// and identical to the declaration's inline `sub { ... }` tokens. Nested
/// lexical scopes inside the handler stay inside the interval (the request
/// context is preserved through nested blocks/subs); adjacent subs and blocks
/// stay outside it. No handler-context fact exists when the declaration is
/// malformed or its handler relation is a typed boundary.
///
/// [`Self::request_context`] carries whether availability is actually
/// established; [`Self::handler_kind`] names the owning declaration kind.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteHandlerContextFact {
    /// Canonical semantic envelope (kind forced to `RouteHandlerContext`;
    /// anchored at the handler interval).
    pub envelope: SemanticFactEnvelope,
    /// Framework name (e.g. `Dancer2`).
    pub framework_name: String,
    /// Producing adapter identity.
    pub adapter_id: AdapterId,
    /// Observed framework version the fact was minted against.
    pub framework_version: String,
    /// Owning application identity from the activating import.
    pub application_name: String,
    /// Owning declaration's source-order identity within the file.
    ///
    /// The index is scoped by [`Self::handler_kind`]: it is a route
    /// declaration index for [`HandlerContextKind::Route`] and a hook
    /// declaration index for [`HandlerContextKind::Hook`]. The field keeps
    /// its original name because this payload travels on a versioned wire
    /// contract where a rename is not an additive change.
    pub route_declaration_index: u32,
    /// Versioned identity of the reviewed DSL contract whose request-scoped
    /// keyword vocabulary applies inside the interval.
    pub dsl_contract_version: String,
    /// Which declaration kind owns this interval.
    pub handler_kind: HandlerContextKind,
    /// Whether the reviewed contract establishes request context here.
    pub request_context: RequestContextAdmission,
}

#[derive(Deserialize)]
struct RouteHandlerContextFactWire {
    envelope: SemanticFactEnvelope,
    framework_name: String,
    adapter_id: AdapterId,
    framework_version: String,
    application_name: String,
    route_declaration_index: u32,
    dsl_contract_version: String,
    // Additive fields: a payload minted before the hook producer existed
    // decodes as an established route context, which is exactly what it was.
    #[serde(default)]
    handler_kind: HandlerContextKind,
    #[serde(default)]
    request_context: RequestContextAdmission,
}

impl<'de> Deserialize<'de> for RouteHandlerContextFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = RouteHandlerContextFactWire::deserialize(deserializer)?;
        Ok(RouteHandlerContextFact::new(
            wire.envelope,
            wire.framework_name,
            wire.adapter_id,
            wire.framework_version,
            wire.application_name,
            wire.route_declaration_index,
            wire.dsl_contract_version,
        )
        .with_handler_kind(wire.handler_kind)
        .with_request_context(wire.request_context))
    }
}

impl RouteHandlerContextFact {
    /// Construct a route handler-context fact; the envelope kind is forced to
    /// [`SemanticFactKind::RouteHandlerContext`].
    #[must_use]
    #[allow(clippy::too_many_arguments)] // mirrors the fact contract fields
    pub fn new(
        mut envelope: SemanticFactEnvelope,
        framework_name: impl Into<String>,
        adapter_id: AdapterId,
        framework_version: impl Into<String>,
        application_name: impl Into<String>,
        route_declaration_index: u32,
        dsl_contract_version: impl Into<String>,
    ) -> Self {
        envelope.kind = SemanticFactKind::RouteHandlerContext;
        Self {
            envelope,
            framework_name: framework_name.into(),
            adapter_id,
            framework_version: framework_version.into(),
            application_name: application_name.into(),
            route_declaration_index,
            dsl_contract_version: dsl_contract_version.into(),
            handler_kind: HandlerContextKind::Route,
            request_context: RequestContextAdmission::Established,
        }
    }

    /// Set the owning declaration kind.
    ///
    /// [`Self::new`] mints the route-shaped default, so only the hook
    /// producer needs this.
    #[must_use]
    pub fn with_handler_kind(mut self, handler_kind: HandlerContextKind) -> Self {
        self.handler_kind = handler_kind;
        self
    }

    /// Set whether the reviewed contract establishes request context here.
    #[must_use]
    pub fn with_request_context(mut self, request_context: RequestContextAdmission) -> Self {
        self.request_context = request_context;
        self
    }

    /// Whether the reviewed contract establishes request context inside this
    /// interval.
    ///
    /// This is the predicate a provider gates request-scoped keyword
    /// availability on; an exact interval alone is not enough.
    #[must_use]
    pub fn establishes_request_context(&self) -> bool {
        self.request_context == RequestContextAdmission::Established
    }

    /// Classify the handler-context fact for a provider decision.
    #[must_use]
    pub fn status(&self) -> SemanticFactStatus {
        self.envelope.status()
    }
}

/// Canonical payload of one route declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteDeclaration {
    /// Source-order declaration identity within the owning file. Distinct
    /// declarations of the same methods/pattern stay distinct routes.
    pub declaration_index: u32,
    /// Route keyword token (e.g. `get`, `any`).
    pub keyword: String,
    /// Source range of the route keyword token.
    pub keyword_anchor: SourceAnchor,
    /// Literal route name, when present.
    pub route_name: RouteNameSelection,
    /// Normalized HTTP method set.
    pub methods: RouteMethodSet,
    /// Local route pattern as declared (see `effective_pattern` for the
    /// prefix-composed projection).
    pub pattern: RoutePattern,
    /// Prefix-composed effective pattern (#8921).
    pub effective_pattern: RouteEffectivePattern,
    /// Static matching options.
    pub options: RouteOptions,
    /// Handler relation.
    pub handler: RouteHandler,
}

impl RouteDeclaration {
    /// Literal route name value, when the name slot is literal.
    #[must_use]
    pub fn route_name_literal_value(&self) -> Option<&str> {
        match &self.route_name {
            RouteNameSelection::Literal(name) => Some(name.value.as_str()),
            RouteNameSelection::Dynamic { .. } | RouteNameSelection::Absent => None,
        }
    }

    /// Whether any payload member is a dynamic or bounded boundary.
    #[must_use]
    pub fn has_boundary(&self) -> bool {
        matches!(self.methods, RouteMethodSet::Dynamic { .. })
            || self.pattern.kind == RoutePatternKind::Dynamic
            || matches!(self.route_name, RouteNameSelection::Dynamic { .. })
            || matches!(self.options, RouteOptions::Dynamic { .. })
            || matches!(self.handler, RouteHandler::Bounded { .. })
            || matches!(self.effective_pattern, RouteEffectivePattern::Boundary { .. })
            || matches!(&self.options, RouteOptions::Map(entries)
                if entries.iter().any(|entry| matches!(entry.value, RouteOptionValue::Dynamic { .. })))
    }
}

/// Canonical framework route fact: envelope plus route payload and framework
/// identity.
///
/// Deserialization is checked: wire payloads are rebuilt through
/// [`RouteFact::new`], so every constructor-side invariant (envelope kind,
/// canonicalized method sets and option maps, no valueless literal patterns)
/// also holds for decoded facts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteFact {
    /// Canonical semantic envelope (kind forced to `Route`).
    pub envelope: SemanticFactEnvelope,
    /// Framework name (e.g. `Dancer2`).
    pub framework_name: String,
    /// Producing adapter identity.
    pub adapter_id: AdapterId,
    /// Observed framework version the fact was minted against.
    pub framework_version: String,
    /// Owning application identity from the activating import.
    pub application_name: String,
    /// Route payload.
    pub route: RouteDeclaration,
}

#[derive(Deserialize)]
struct RouteFactWire {
    envelope: SemanticFactEnvelope,
    framework_name: String,
    adapter_id: AdapterId,
    framework_version: String,
    application_name: String,
    route: RouteDeclaration,
}

impl<'de> Deserialize<'de> for RouteFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = RouteFactWire::deserialize(deserializer)?;
        Ok(RouteFact::new(
            wire.envelope,
            wire.framework_name,
            wire.adapter_id,
            wire.framework_version,
            wire.application_name,
            wire.route,
        ))
    }
}

impl RouteFact {
    /// Construct a route fact and canonicalize the payload.
    ///
    /// The envelope kind is forced to [`SemanticFactKind::Route`]. Method sets
    /// are sorted and deduplicated; option maps are sorted by key with the last
    /// occurrence of a duplicated key winning (hash-construction semantics).
    /// The effective-pattern slot is kept coherent with the pattern slot: a
    /// dynamic pattern forces an effective boundary, a composed effective
    /// pattern requires a literal pattern with at least one contributing
    /// prefix declaration, and a local projection is canonicalized to the
    /// deterministic value of the declared pattern.
    #[allow(clippy::too_many_arguments)] // mirrors the fact contract fields
    #[must_use]
    pub fn new(
        mut envelope: SemanticFactEnvelope,
        framework_name: impl Into<String>,
        adapter_id: AdapterId,
        framework_version: impl Into<String>,
        application_name: impl Into<String>,
        route: RouteDeclaration,
    ) -> Self {
        envelope.kind = SemanticFactKind::Route;
        let mut route = route;
        // A literal/regex pattern without a value is incoherent as an exact
        // operand: coerce it to the dynamic boundary so no exact status can
        // rest on a missing value (covers unchecked wire payloads).
        if matches!(route.pattern.kind, RoutePatternKind::Literal | RoutePatternKind::Regex)
            && route.pattern.value.is_none()
        {
            route.pattern.kind = RoutePatternKind::Dynamic;
        }
        // Effective-pattern coherence (covers unchecked wire payloads): a
        // dynamic pattern can never carry an exact effective pattern, and a
        // composed projection requires a literal pattern plus at least one
        // contributing prefix declaration.
        if route.pattern.kind == RoutePatternKind::Dynamic
            && !matches!(route.effective_pattern, RouteEffectivePattern::Boundary { .. })
        {
            route.effective_pattern = RouteEffectivePattern::Boundary {
                reason: "dynamic route pattern has no exact effective pattern".to_string(),
            };
        }
        if let RouteEffectivePattern::Composed { prefix_declarations, .. } =
            &route.effective_pattern
            && (route.pattern.kind != RoutePatternKind::Literal || prefix_declarations.is_empty())
        {
            route.effective_pattern = RouteEffectivePattern::Boundary {
                reason: "composed effective pattern requires a literal pattern and at least one \
                         contributing prefix declaration"
                    .to_string(),
            };
        }
        // A local projection is fully determined by the declared pattern —
        // leading-`/` normalization for string patterns, regex patterns
        // carried as-is — so a wire payload asserting any other value is
        // canonicalized back to the deterministic projection: no exact
        // status can rest on a local value the pattern does not produce
        // (covers unchecked wire payloads).
        if let (Some(pattern_value), RouteEffectivePattern::Local { value }) =
            (&route.pattern.value, &mut route.effective_pattern)
        {
            let deterministic = local_effective_value(pattern_value, route.pattern.kind);
            if *value != deterministic {
                *value = deterministic;
            }
        }
        if let RouteMethodSet::Exact(methods) = &mut route.methods {
            methods.sort();
            methods.dedup();
        }
        if let RouteOptions::Map(entries) = &mut route.options {
            entries.sort_by(|left, right| left.key.cmp(&right.key));
            // Hash-construction semantics: the last occurrence of a
            // duplicated key wins.
            let mut deduplicated: Vec<crate::route::RouteOption> =
                Vec::with_capacity(entries.len());
            for entry in entries.drain(..) {
                if deduplicated.last().is_some_and(|last| last.key == entry.key) {
                    deduplicated.pop();
                }
                deduplicated.push(entry);
            }
            *entries = deduplicated;
        }
        Self {
            envelope,
            framework_name: framework_name.into(),
            adapter_id,
            framework_version: framework_version.into(),
            application_name: application_name.into(),
            route,
        }
    }

    /// Classify the complete route fact for a provider decision.
    ///
    /// An exact envelope with a bounded payload member stays degraded: routes
    /// with dynamic/computed members are usable but never exact.
    #[must_use]
    pub fn status(&self) -> SemanticFactStatus {
        let envelope_status = self.envelope.status();
        if !matches!(envelope_status, SemanticFactStatus::Exact) {
            return envelope_status;
        }
        if self.route.has_boundary() {
            SemanticFactStatus::Degraded
        } else {
            SemanticFactStatus::Exact
        }
    }
}

/// Deterministic route fact identity for one (file, declaration, generation).
///
/// Identity derives from the owning file, the source declaration order, and
/// the minting generation: re-mining the same generation reproduces the same
/// identities, two declarations of the same route stay distinct, and facts of
/// different roots/edits (different generations over root-local file ids)
/// never collide. Addition keeps the entity identity disjoint from the fact
/// identity.
#[must_use]
pub fn route_fact_identity(
    file_id: crate::FileId,
    declaration_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    family_fact_identity(0x524F_5554_4531_3100_u64, file_id, declaration_index, 0, generation)
}

/// Deterministic route prefix fact identity for one
/// (file, prefix declaration, generation) (#8921).
///
/// Same contract as [`route_fact_identity`], salted per fact kind so a prefix
/// declaration and a route declaration sharing a source index never collide.
#[must_use]
pub fn route_prefix_fact_identity(
    file_id: crate::FileId,
    declaration_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    family_fact_identity(0x5052_4546_4958_3131_u64, file_id, declaration_index, 0, generation)
}

/// Deterministic route parameter fact identity for one
/// (file, route declaration, parameter, generation) (#8921).
///
/// The route's entity identity is returned as the entity of the parameter
/// fact: every parameter fact of one route shares the owning route entity.
#[must_use]
pub fn route_parameter_fact_identity(
    file_id: crate::FileId,
    declaration_index: u32,
    parameter_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let (_, route_entity) = route_fact_identity(file_id, declaration_index, generation);
    let (fact_id, _) = family_fact_identity(
        0x5041_5241_4D45_5431_u64,
        file_id,
        declaration_index,
        parameter_index,
        generation,
    );
    (fact_id, route_entity)
}

/// Deterministic route handler-context fact identity for one
/// (file, route declaration, generation) (#8921).
///
/// Distinct from the route fact identity (kind salt) while sharing the owning
/// route entity.
#[must_use]
pub fn route_handler_context_identity(
    file_id: crate::FileId,
    declaration_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let (_, route_entity) = route_fact_identity(file_id, declaration_index, generation);
    let (fact_id, _) =
        family_fact_identity(0x4841_4E44_4C52_4331_u64, file_id, declaration_index, 0, generation);
    (fact_id, route_entity)
}

/// Shared (file, index, generation) identity mix for the route fact family.
///
/// The kind salt keeps the fact kinds disjoint; the parameter index is folded
/// with its own multiplier so parameters of one route stay distinct.
fn family_fact_identity(
    kind_salt: u64,
    file_id: crate::FileId,
    declaration_index: u32,
    parameter_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let generation_digest = match generation {
        // FNV-1a accumulation: order-sensitive and repetition-sensitive, so
        // distinct generation identities (including transposed or repeated
        // spellings like `"11"` vs `"22"`) never collide.
        SourceGeneration::Known(value) => {
            value.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x1000_0000_01b3)
            })
        }
        // An unknown generation is still a distinct minting context; it can
        // never produce an exact fact (the envelope degrades it separately).
        SourceGeneration::Unknown => 0x1a2b_3c4d_5e6f_7081_u64,
    };
    let file = file_id.0.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let index = u64::from(declaration_index).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    let parameter = u64::from(parameter_index).wrapping_mul(0x87C3_7B29_11C5_21D3_u64);
    let fact = file ^ index ^ parameter ^ generation_digest ^ kind_salt;
    (FactId(fact), EntityId(fact.wrapping_add(1)))
}

/// Build the canonical envelope for one minted route-family fact.
///
/// Shared by framework adapters so every route-family fact carries the same
/// producer/provenance/freshness contract: producer `FrameworkAdapter`,
/// AST-exact provenance, high confidence, fresh generation, and invalidation
/// dependencies over the owning source file plus the activating framework
/// module. The reason code and optional boundary link reflect the payload's
/// exactness. The envelope kind is forced to `kind`.
#[allow(clippy::too_many_arguments)] // mirrors the envelope contract fields
#[must_use]
pub fn route_family_envelope(
    kind: SemanticFactKind,
    fact_id: FactId,
    entity_id: EntityId,
    package: Option<&str>,
    declaration_anchor: SourceAnchor,
    generation: &SourceGeneration,
    dependencies: Vec<crate::InvalidationDependency>,
    boundary: Option<BoundaryKind>,
    boundary_reason: crate::SemanticReasonCode,
    exact: bool,
) -> SemanticFactEnvelope {
    SemanticFactEnvelope::new(
        fact_id,
        Some(entity_id),
        kind,
        declaration_anchor,
        generation.clone(),
        None,
        package.map(ToString::to_string),
        crate::LifecyclePhase::Runtime,
        SemanticProducer::FrameworkAdapter,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
        boundary.map(|kind| {
            // No separate boundary fact is minted: the typed boundary lives in
            // the route payload, so the link carries no foreign boundary id.
            crate::BoundaryLink::new(None, kind, BoundaryDisposition::Degrade, boundary_reason)
        }),
        dependencies,
        if exact {
            SemanticReasonCode::ExactSource
        } else {
            SemanticReasonCode::GeneratedFromSource
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnchorId, FileId, InvalidationDependency};
    use perl_test_must::must_some;

    fn anchor(start: u32, end: u32) -> SourceAnchor {
        SourceAnchor::new(Some(AnchorId(start as u64)), FileId(1), start, end)
    }

    fn literal_route(declaration_index: u32) -> RouteDeclaration {
        RouteDeclaration {
            declaration_index,
            keyword: "get".to_string(),
            keyword_anchor: anchor(0, 3),
            route_name: RouteNameSelection::Absent,
            methods: RouteMethodSet::Exact(vec!["GET".to_string(), "HEAD".to_string()]),
            pattern: RoutePattern {
                kind: RoutePatternKind::Literal,
                value: Some("/x".to_string()),
                anchor: anchor(4, 8),
            },
            effective_pattern: RouteEffectivePattern::Local { value: "/x".to_string() },
            options: RouteOptions::Map(Vec::new()),
            handler: RouteHandler::InlineSub { anchor: anchor(12, 21) },
        }
    }

    fn envelope_for(fact_id: FactId, entity_id: EntityId, exact: bool) -> SemanticFactEnvelope {
        route_family_envelope(
            SemanticFactKind::Route,
            fact_id,
            entity_id,
            Some("App"),
            anchor(0, 21),
            &SourceGeneration::known("gen-1"),
            vec![InvalidationDependency::new("source:1", SourceGeneration::known("gen-1"))],
            if exact { None } else { Some(BoundaryKind::DynamicValue) },
            SemanticReasonCode::DynamicValue,
            exact,
        )
    }

    #[test]
    fn exact_literal_route_is_exact_and_forces_kind() {
        let (fact_id, entity_id) =
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        let fact = RouteFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            literal_route(0),
        );
        assert_eq!(fact.envelope.kind, SemanticFactKind::Route);
        assert_eq!(fact.status(), SemanticFactStatus::Exact);
    }

    #[test]
    fn bounded_payload_members_degrade_the_fact() {
        for mutation in [
            RouteMutation::Pattern,
            RouteMutation::Methods,
            RouteMutation::Handler,
            RouteMutation::Options,
            RouteMutation::EffectivePattern,
        ] {
            let (fact_id, entity_id) =
                route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
            let mut route = literal_route(0);
            mutation.apply(&mut route);
            assert!(route.has_boundary(), "{mutation:?} must be a boundary");
            // Bounded facts carry a boundary link on the envelope.
            let fact = RouteFact::new(
                envelope_for(fact_id, entity_id, false),
                "Dancer2",
                AdapterId(1),
                "1.1.1",
                "App",
                route,
            );
            assert_eq!(fact.status(), SemanticFactStatus::Degraded, "{mutation:?}");
        }
    }

    #[derive(Debug)]
    enum RouteMutation {
        Pattern,
        Methods,
        Handler,
        Options,
        EffectivePattern,
    }

    impl RouteMutation {
        fn apply(&self, route: &mut RouteDeclaration) {
            match self {
                RouteMutation::Pattern => {
                    route.pattern = RoutePattern {
                        kind: RoutePatternKind::Dynamic,
                        value: None,
                        anchor: anchor(4, 8),
                    };
                }
                RouteMutation::Methods => {
                    route.methods =
                        RouteMethodSet::Dynamic { reason: "computed method list".to_string() };
                }
                RouteMutation::Handler => {
                    route.handler = RouteHandler::Bounded {
                        boundary: RouteHandlerBoundary::String,
                        anchor: Some(anchor(12, 21)),
                        reason: "string handler".to_string(),
                    };
                }
                RouteMutation::Options => {
                    route.options = RouteOptions::Map(vec![RouteOption {
                        key: "agent".to_string(),
                        key_anchor: anchor(9, 15),
                        value: RouteOptionValue::Dynamic { reason: "computed".to_string() },
                        value_anchor: anchor(16, 20),
                    }]);
                }
                RouteMutation::EffectivePattern => {
                    route.effective_pattern =
                        RouteEffectivePattern::Boundary { reason: "computed prefix".to_string() };
                }
            }
        }
    }

    #[test]
    fn method_set_and_option_map_are_canonicalized() {
        let (fact_id, entity_id) =
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        let mut route = literal_route(1);
        route.methods =
            RouteMethodSet::Exact(vec!["POST".to_string(), "GET".to_string(), "POST".to_string()]);
        route.options = RouteOptions::Map(vec![
            RouteOption {
                key: "b".to_string(),
                key_anchor: anchor(1, 2),
                value: RouteOptionValue::Literal("first".to_string()),
                value_anchor: anchor(3, 4),
            },
            RouteOption {
                key: "a".to_string(),
                key_anchor: anchor(5, 6),
                value: RouteOptionValue::Literal("x".to_string()),
                value_anchor: anchor(7, 8),
            },
            RouteOption {
                key: "b".to_string(),
                key_anchor: anchor(9, 10),
                value: RouteOptionValue::Literal("last".to_string()),
                value_anchor: anchor(11, 12),
            },
        ]);
        let fact = RouteFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            route,
        );
        let methods = match &fact.route.methods {
            RouteMethodSet::Exact(methods) => Some(methods.clone()),
            RouteMethodSet::Dynamic { .. } => None,
        };
        assert_eq!(
            must_some(methods),
            vec!["GET".to_string(), "POST".to_string()],
            "method sets are sorted and deduplicated"
        );
        let entries = match &fact.route.options {
            RouteOptions::Map(entries) => Some(entries),
            RouteOptions::Dynamic { .. } => None,
        };
        let entries = must_some(entries);
        assert_eq!(entries.len(), 2, "duplicate keys collapse (last wins)");
        assert_eq!(entries[0].key, "a");
        assert_eq!(entries[1].key, "b");
        assert_eq!(
            entries[1].value,
            RouteOptionValue::Literal("last".to_string()),
            "hash-construction keeps the last duplicate"
        );
    }

    #[test]
    fn identities_are_distinct_per_declaration_and_deterministic() {
        let (fact_a, entity_a) =
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        let (fact_b, entity_b) =
            route_fact_identity(FileId(1), 1, &SourceGeneration::known("gen-1"));
        assert_ne!(fact_a, fact_b, "same route shape stays distinct by order");
        assert_ne!(entity_a, entity_b);
        assert_eq!(
            fact_a,
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1")).0,
            "deterministic"
        );
        // Facts of a different root/edit (different generation) never collide,
        // even with root-local file ids.
        let other_root = route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-b"));
        assert_ne!(fact_a, other_root.0);
        let stale = route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-2"));
        assert_ne!(fact_a, stale.0);
    }

    #[test]
    fn generation_digest_is_order_and_repetition_sensitive() {
        // Regression: an order-insensitive fold made repeated-byte spellings
        // collide (both digests cancelled to the seed).
        assert_ne!(
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("11")).0,
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("22")).0,
            "repeated-byte generations must not collide"
        );
        assert_ne!(
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-ab")).0,
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-ba")).0,
            "transposed generations must not collide"
        );
    }

    #[test]
    fn deserialization_reapplies_constructor_invariants() -> Result<(), serde_json::Error> {
        let (fact_id, entity_id) =
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        let fact = RouteFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            literal_route(0),
        );

        // A forged wire payload: wrong envelope kind, unsorted/duplicate
        // methods, a valueless literal pattern, and a composed effective
        // pattern over a dynamic pattern.
        let mut value = serde_json::to_value(&fact)?;
        value["envelope"]["kind"] = serde_json::json!("Declaration");
        value["route"]["methods"] = serde_json::json!({ "Exact": ["POST", "GET", "POST"] });
        value["route"]["pattern"]["value"] = serde_json::json!(null);
        value["route"]["effective_pattern"] = serde_json::json!({
            "Composed": { "value": "/api/x", "prefix_declarations": [3] }
        });
        let decoded: RouteFact = serde_json::from_value(value)?;
        assert_eq!(decoded.envelope.kind, SemanticFactKind::Route, "kind is forced");
        let methods = must_some(match &decoded.route.methods {
            RouteMethodSet::Exact(methods) => Some(methods.clone()),
            _ => None,
        });
        assert_eq!(methods, vec!["GET".to_string(), "POST".to_string()]);
        assert_eq!(
            decoded.route.pattern.kind,
            RoutePatternKind::Dynamic,
            "a valueless literal pattern is coerced to the dynamic boundary"
        );
        assert!(
            matches!(decoded.route.effective_pattern, RouteEffectivePattern::Boundary { .. }),
            "a dynamic pattern can never carry a composed effective pattern"
        );
        assert_eq!(decoded.status(), SemanticFactStatus::Degraded);
        Ok(())
    }

    #[test]
    fn local_effective_pattern_is_canonicalized_from_the_pattern() {
        // A forged wire payload: literal pattern `users` with a `Local`
        // value the pattern does not deterministically produce. The
        // constructor canonicalizes the projection back to `/users` (the
        // reviewed leading-`/` normalization), so no exact status rests on a
        // value the declared pattern cannot generate.
        let (fact_id, entity_id) =
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        let mut route = literal_route(0);
        route.pattern.value = Some("users".to_string());
        route.effective_pattern = RouteEffectivePattern::Local { value: "/admin".to_string() };
        let fact = RouteFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            route,
        );
        assert!(
            matches!(&fact.route.effective_pattern,
                RouteEffectivePattern::Local { value } if value == "/users"),
            "the local projection is the deterministic value of the declared pattern"
        );

        // A regex pattern's local projection is carried as-is, still
        // canonicalized against the declared pattern value.
        let (fact_id, entity_id) =
            route_fact_identity(FileId(1), 1, &SourceGeneration::known("gen-1"));
        let mut route = literal_route(1);
        route.pattern.kind = RoutePatternKind::Regex;
        route.pattern.value = Some("^/re/(\\d+)$".to_string());
        route.effective_pattern = RouteEffectivePattern::Local { value: "/other".to_string() };
        let fact = RouteFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            route,
        );
        assert!(
            matches!(&fact.route.effective_pattern,
                RouteEffectivePattern::Local { value } if value == "^/re/(\\d+)$"),
            "regex local projections carry the declared pattern as-is"
        );
    }

    #[test]
    fn composed_effective_pattern_requires_literal_pattern_and_contributions() {
        let (fact_id, entity_id) =
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        // Literal pattern + empty contributions: not a composition.
        let mut route = literal_route(0);
        route.effective_pattern = RouteEffectivePattern::Composed {
            value: "/x".to_string(),
            prefix_declarations: Vec::new(),
        };
        let fact = RouteFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            route,
        );
        assert!(
            matches!(fact.route.effective_pattern, RouteEffectivePattern::Boundary { .. }),
            "an empty contribution set is not a composition"
        );
        assert_eq!(fact.status(), SemanticFactStatus::Degraded);

        // Literal pattern + one contribution stays composed and exact.
        let (fact_id, entity_id) =
            route_fact_identity(FileId(1), 1, &SourceGeneration::known("gen-1"));
        let mut route = literal_route(1);
        route.effective_pattern = RouteEffectivePattern::Composed {
            value: "/api/x".to_string(),
            prefix_declarations: vec![0],
        };
        let fact = RouteFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            route,
        );
        assert!(matches!(&fact.route.effective_pattern,
                RouteEffectivePattern::Composed { value, prefix_declarations }
                if value == "/api/x" && prefix_declarations == &[0]));
        assert_eq!(fact.status(), SemanticFactStatus::Exact);
    }

    #[test]
    fn prefix_facts_force_kind_and_classify_boundaries() -> Result<(), serde_json::Error> {
        let (fact_id, entity_id) =
            route_prefix_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        let prefix = RoutePrefixDeclaration {
            declaration_index: 0,
            keyword: "prefix".to_string(),
            keyword_anchor: anchor(0, 6),
            selection: RoutePrefixSelection::Literal(RoutePrefixLiteral {
                value: "/api".to_string(),
                anchor: anchor(7, 12),
            }),
            scope: RoutePrefixScope::Sticky,
        };
        let fact = RoutePrefixFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            prefix,
        );
        assert_eq!(fact.envelope.kind, SemanticFactKind::RoutePrefix);
        assert_eq!(fact.status(), SemanticFactStatus::Exact);

        // A computed prefix operand stays a boundary, never a guessed value.
        let mut dynamic = fact.prefix.clone();
        dynamic.selection = RoutePrefixSelection::Dynamic {
            reason: "computed prefix".to_string(),
            anchor: Some(anchor(7, 12)),
        };
        let fact = RoutePrefixFact::new(
            envelope_for(fact_id, entity_id, false),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            dynamic,
        );
        assert_eq!(fact.status(), SemanticFactStatus::Degraded);
        assert_eq!(
            serde_json::from_str::<RoutePrefixFact>(&serde_json::to_string(&fact)?)?,
            fact,
            "prefix facts round-trip through the transport"
        );
        Ok(())
    }

    #[test]
    fn parameter_facts_share_the_route_entity_and_classify_capture_boundaries()
    -> Result<(), serde_json::Error> {
        let (fact_id, entity_id) =
            route_parameter_fact_identity(FileId(1), 2, 1, &SourceGeneration::known("gen-1"));
        let (_, route_entity) =
            route_fact_identity(FileId(1), 2, &SourceGeneration::known("gen-1"));
        assert_eq!(entity_id, route_entity, "parameter facts share the owning route entity");
        let segment = RouteParameterSegment {
            kind: RouteParameterKind::Typed { type_name: "Int".to_string() },
            name: Some("id".to_string()),
            anchor: anchor(9, 17),
            limitation: Some(
                "declared type constraint is runtime-validated, never proven statically"
                    .to_string(),
            ),
        };
        let fact = RouteParameterFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            2,
            segment,
        );
        assert_eq!(fact.envelope.kind, SemanticFactKind::RouteParameter);
        assert_eq!(fact.route_declaration_index, 2);
        assert_eq!(fact.status(), SemanticFactStatus::Exact);

        // An unsupported capture interpretation stays degraded even under an
        // exact envelope.
        let mut boundary = fact.clone();
        boundary.parameter.kind = RouteParameterKind::CaptureUnsupported;
        boundary.parameter.name = None;
        assert_eq!(boundary.status(), SemanticFactStatus::Degraded);
        assert_eq!(
            serde_json::from_str::<RouteParameterFact>(&serde_json::to_string(&fact)?)?,
            fact,
            "parameter facts round-trip through the transport"
        );
        Ok(())
    }

    #[test]
    fn handler_context_facts_force_kind_and_share_the_route_entity() -> Result<(), serde_json::Error>
    {
        let (fact_id, entity_id) =
            route_handler_context_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        let (_, route_entity) =
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        assert_eq!(entity_id, route_entity);
        assert_ne!(
            fact_id,
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1")).0,
            "the handler-context fact stays a distinct fact"
        );
        let fact = RouteHandlerContextFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            0,
            "dancer2-dsl.1-1.v2",
        );
        assert_eq!(fact.envelope.kind, SemanticFactKind::RouteHandlerContext);
        assert_eq!(fact.status(), SemanticFactStatus::Exact);
        assert_eq!(
            serde_json::from_str::<RouteHandlerContextFact>(&serde_json::to_string(&fact)?)?,
            fact,
            "handler-context facts round-trip through the transport"
        );
        Ok(())
    }

    #[test]
    fn family_identities_are_kind_and_parameter_distinct() {
        let generation = SourceGeneration::known("gen-1");
        let route = route_fact_identity(FileId(1), 0, &generation);
        let prefix = route_prefix_fact_identity(FileId(1), 0, &generation);
        let parameter_zero = route_parameter_fact_identity(FileId(1), 0, 0, &generation);
        let parameter_one = route_parameter_fact_identity(FileId(1), 0, 1, &generation);
        let handler_context = route_handler_context_identity(FileId(1), 0, &generation);
        let mut ids = vec![route.0, prefix.0, parameter_zero.0, parameter_one.0, handler_context.0];
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 5, "all family fact ids over one declaration differ");
        // Generation sensitivity carries over from the shared fold.
        assert_ne!(
            parameter_zero.0,
            route_parameter_fact_identity(FileId(1), 0, 0, &SourceGeneration::known("gen-2")).0
        );
    }

    #[test]
    fn route_fact_roundtrips_through_json() -> Result<(), serde_json::Error> {
        let (fact_id, entity_id) =
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        let fact = RouteFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(0x0044_4E43),
            "1.1.1",
            "MyApp",
            literal_route(0),
        );
        let serialized = serde_json::to_string(&fact)?;
        let decoded: RouteFact = serde_json::from_str(&serialized)?;
        assert_eq!(decoded, fact);
        assert_eq!(decoded.status(), SemanticFactStatus::Exact);
        Ok(())
    }
}
