//! Versioned transport-neutral call shape (#8851, parent #5242).
//!
//! This module is the **source/operation schema** for a call. It is not resolved
//! dispatch, prototype application, MRO, or a runtime result. Adapters consume
//! current AST fields only; they never scan source text.
//!
//! Syntax form and dispatch interpretation stay orthogonal. An adapter may
//! return typed partial or recovered state. Absence of an operand is
//! [`OperandPresence::Unknown`] or [`OperandList::Unavailable`], never a silent
//! empty [`Vec`].
//!
//! # What this does not own
//!
//! HIR/PIR migration, provider behavior, call resolution, and the #8848
//! inventory's stable source-family IDs. Current representations receive a
//! disposition in [`CURRENT_REPRESENTATION_DISPOSITIONS`]; PIR and semantic
//! facts are recorded as not adapted in this slice.

use crate::{Node, NodeKind};
use perl_position_tracking::SourceLocation;

/// Schema family token. Changing serialized meaning requires a version bump.
pub const CALL_SEMANTICS_SCHEMA_ID: &str = "call-semantics";

/// Vocabulary version encoded on the wire.
pub const CALL_SEMANTICS_SCHEMA_VERSION: u32 = 1;

/// Wire form `call-semantics.v{version}`.
pub const CALL_SEMANTICS_SCHEMA_WIRE: &str = "call-semantics.v1";

/// Parser sentinel stored in [`NodeKind::FunctionCall::name`] for `$cb->()`.
pub const CODEREF_CALL_SENTINEL: &str = "->()";

/// Source-observable call syntax. Orthogonal to [`DispatchKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CallSyntax {
    /// Named call whose parentheses/list-operator form is not separately proven.
    Named,
    /// Bare call without parentheses. Current AST adapters never emit this:
    /// [`NodeKind::FunctionCall`] does not retain delimiter identity.
    Bare,
    /// Explicit `&` sigil call.
    Ampersand,
    /// Coderef invocation (`$cb->()` / `&$cb` callee operand).
    Coderef,
    /// Arrow method call (`$obj->method`).
    MethodArrow,
    /// Indirect-object syntax (`method Class @args`) that is not print-like.
    Indirect,
    /// Builtin or list-operator spelling observed in the callee name field.
    BuiltinListOp,
    /// `CORE::`-qualified callee name field.
    CoreQualified,
    /// Block-taking operator (`map`/`grep`/`sort` with a block operand).
    BlockBearing,
    /// Print/say/printf filehandle-like indirect form.
    PrintFilehandleLike,
    /// Recovered or otherwise unclassifiable call-like node.
    RecoveredUnknown,
}

/// Dispatch interpretation. Orthogonal to [`CallSyntax`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DispatchKind {
    /// Ordinary named function dispatch.
    Function,
    /// Coderef invocation.
    Coderef,
    /// Method dispatch with a known method spelling.
    Method,
    /// Indirect-object dispatch.
    IndirectObject,
    /// Builtin/list-operator dispatch from a closed name-field table.
    Builtin,
    /// Dynamic callee or method selector.
    Dynamic,
    /// Recovered, partial, or representation-boundary call.
    Boundary,
}

/// How argument operands were retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ArgumentSource {
    /// Implicit caller `@_` forwarding (`&foo` without parentheses).
    ///
    /// Current AST cannot populate this from `args.is_empty()` alone.
    ImplicitCallerArgs,
    /// Explicit empty argument list (`&foo()` / `foo()`).
    ///
    /// Current AST cannot populate this from `args.is_empty()` alone.
    ExplicitEmpty,
    /// Explicit non-empty argument list retained as fields.
    ExplicitList,
    /// Empty-versus-implicit distinction is unavailable.
    Unknown,
}

/// Role of one operand. One operand cannot hold two incompatible roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OperandRole {
    /// Callee expression (coderef, dynamic selector).
    Callee,
    /// Method invocant.
    Receiver,
    /// Ordinary argument.
    Argument,
    /// Block operand of a block-taking operator.
    Block,
    /// Filehandle operand of a print-like form.
    Filehandle,
}

/// Presence of a single optional operand.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OperandPresence {
    /// Operand is present with a source-backed role.
    Present(CallOperand),
    /// Operand is proven absent for this form.
    Absent,
    /// Representation does not retain this operand.
    Unknown,
}

impl OperandPresence {
    /// Present operand, or `None` when absent or unknown.
    #[must_use]
    pub fn present(&self) -> Option<&CallOperand> {
        match self {
            Self::Present(operand) => Some(operand),
            Self::Absent | Self::Unknown => None,
        }
    }
}

/// Ordered operand list that distinguishes empty from unavailable.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OperandList {
    /// Representation retains ordered operands. An empty vec is explicit empty.
    Explicit(Vec<CallOperand>),
    /// Representation does not retain operands. `known_count` is a count-only
    /// shell fact and must not be treated as identities.
    Unavailable {
        /// Count retained by a shell that dropped operand identity.
        known_count: Option<usize>,
    },
}

impl OperandList {
    /// Explicit operands, or `None` when the list is unavailable.
    #[must_use]
    pub fn explicit(&self) -> Option<&[CallOperand]> {
        match self {
            Self::Explicit(items) => Some(items.as_slice()),
            Self::Unavailable { .. } => None,
        }
    }

    /// True when a consumer would be wrong to treat this as an empty argument list.
    #[must_use]
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
}

/// One source-ordered call operand. Does not own an AST subtree.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CallOperand {
    /// Source range of the operand node, when the representation retains one.
    pub source_range: Option<SourceLocation>,
    /// Role of this operand.
    pub role: OperandRole,
    /// Zero-based source order among retained operands of this call.
    pub source_order: u32,
    /// Evaluation proposition. Never inferred from traversal order.
    pub evaluation: EvaluationContract,
    /// Context requested of this operand, when known.
    pub context: OperandContext,
}

impl CallOperand {
    /// Construct one operand. External crates cannot use a struct literal
    /// because this type is non-exhaustive.
    #[must_use]
    pub fn new(
        source_range: Option<SourceLocation>,
        role: OperandRole,
        source_order: u32,
        evaluation: EvaluationContract,
        context: OperandContext,
    ) -> Self {
        Self { source_range, role, source_order, evaluation, context }
    }
}

/// Evaluation-order / evaluate-once proposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EvaluationContract {
    /// Source order is proven by an admitted language rule.
    ProvenSourceOrder,
    /// Proven by a named operation rule.
    ProvenByOperationRule {
        /// Stable rule identifier.
        rule_id: &'static str,
    },
    /// Operand may execute conditionally or repeatedly.
    ConditionalOrRepeated {
        /// Stable rule identifier.
        rule_id: &'static str,
    },
    /// Evaluation facts are not supplied by this representation.
    Unknown,
    /// Evaluation is not applicable (absent operand, recovered hole).
    NotApplicable,
}

/// Operand/result context vocabulary aligned with current PIR
/// (`Scalar`/`List`/`Void`/`Unknown`). Current PIR has no `Lvalue` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OperandContext {
    /// Scalar context.
    Scalar,
    /// List context.
    List,
    /// Void context.
    Void,
    /// Context is not statically proven.
    Unknown,
}

/// Prototype / list-operator contract disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PrototypeDisposition {
    /// No prototype fact is available.
    Unknown,
    /// Prototype application is not a property of this syntax (ampersand bypass
    /// is a source flag; application still requires declaration evidence).
    NotApplicable,
    /// A prototype may apply, but resolution is out of scope for this schema.
    RequiresResolution,
}

/// Completeness of the adapted view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CallBoundary {
    /// Every retained field is source-backed and complete for this form.
    Exact,
    /// Some required facts are typed unknown/unavailable.
    Partial,
    /// Callee or method selector is dynamic.
    Dynamic,
    /// Parser recovery or unknown source form.
    Recovered,
}

/// Callee identity as retained by the current representation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CalleeIdentity {
    /// Name field of a named/ampersand/CORE/builtin call.
    Named {
        /// Callee spelling from the AST/HIR name field, not from source rescan.
        name: String,
    },
    /// Parser sentinel `->()` was the name field; it is not canonical identity.
    SentinelCoderef,
    /// Method spelling from the method field.
    Method {
        /// Method spelling as stored by the current node.
        name: String,
    },
    /// Callee is not retained.
    Unknown,
}

/// Current representation an adapter consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CurrentRepresentation {
    /// [`NodeKind::FunctionCall`].
    AstFunctionCall,
    /// [`NodeKind::AmperCall`].
    AstAmperCall,
    /// [`NodeKind::MethodCall`].
    AstMethodCall,
    /// [`NodeKind::IndirectCall`].
    AstIndirectCall,
    /// Recovered AST wrapper whose partial child is call-like.
    AstRecovered,
    /// Flat HIR [`CallExpr`] shell.
    HirCallExpr,
    /// Flat HIR [`MethodCallExpr`] shell.
    HirMethodCallExpr,
    /// Flat HIR [`IndirectCallExpr`] shell.
    HirIndirectCallExpr,
    /// Flat HIR dynamic-boundary item for a coderef call.
    HirDynamicBoundary,
    /// Body-HIR [`HirExpr::Call`].
    HirBodyCall,
}

impl CurrentRepresentation {
    /// Mapping-table row id for this representation. Exhaustive over variants.
    #[must_use]
    pub const fn mapping_id(self) -> &'static str {
        match self {
            Self::AstFunctionCall => "ast.FunctionCall",
            Self::AstAmperCall => "ast.AmperCall",
            Self::AstMethodCall => "ast.MethodCall",
            Self::AstIndirectCall => "ast.IndirectCall",
            Self::AstRecovered => "ast.Error.partial-call",
            Self::HirCallExpr => "hir.CallExpr",
            Self::HirMethodCallExpr => "hir.MethodCallExpr",
            Self::HirIndirectCallExpr => "hir.IndirectCallExpr",
            Self::HirDynamicBoundary => "hir.DynamicBoundary.CoderefCall",
            Self::HirBodyCall => "hir.HirExpr.Call",
        }
    }
}

/// Disposition of one current representation relative to this schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RepresentationDisposition {
    /// Adapter represents the fields exactly.
    ExactlyRepresented,
    /// Adapter represents the fields with explicit loss recorded as limitations.
    RepresentedWithExplicitLoss,
    /// Not adapted in this PR; recorded so the row is not silently dropped.
    UnknownUntilProducerMigration,
    /// Not a call representation.
    NotApplicable,
}

/// One current-representation mapping row (#8851 acceptance / #5238 disposition).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepresentationMapping {
    /// Stable row id for this representation.
    pub id: &'static str,
    /// Layer owning the representation.
    pub layer: &'static str,
    /// Disposition of this row in the #8851 schema.
    pub disposition: RepresentationDisposition,
    /// Owner of remaining work, when not exact.
    pub owner_issue: &'static str,
}

/// Checked mapping of every current call vocabulary this PR knows about.
///
/// PIR, semantic facts, and providers are not migrated here. They are listed
/// so no current field disappears without a disposition.
pub const CURRENT_REPRESENTATION_DISPOSITIONS: &[RepresentationMapping] = &[
    RepresentationMapping {
        id: "ast.FunctionCall",
        layer: "ast",
        disposition: RepresentationDisposition::RepresentedWithExplicitLoss,
        owner_issue: "8851",
    },
    RepresentationMapping {
        id: "ast.AmperCall",
        layer: "ast",
        disposition: RepresentationDisposition::RepresentedWithExplicitLoss,
        owner_issue: "8851",
    },
    RepresentationMapping {
        id: "ast.MethodCall",
        layer: "ast",
        disposition: RepresentationDisposition::RepresentedWithExplicitLoss,
        owner_issue: "8851",
    },
    RepresentationMapping {
        id: "ast.IndirectCall",
        layer: "ast",
        disposition: RepresentationDisposition::RepresentedWithExplicitLoss,
        owner_issue: "8851",
    },
    RepresentationMapping {
        id: "ast.Error.partial-call",
        layer: "ast",
        disposition: RepresentationDisposition::RepresentedWithExplicitLoss,
        owner_issue: "8851",
    },
    RepresentationMapping {
        id: "hir.CallExpr",
        layer: "flat-hir",
        disposition: RepresentationDisposition::RepresentedWithExplicitLoss,
        owner_issue: "8851",
    },
    RepresentationMapping {
        id: "hir.MethodCallExpr",
        layer: "flat-hir",
        disposition: RepresentationDisposition::RepresentedWithExplicitLoss,
        owner_issue: "8851",
    },
    RepresentationMapping {
        id: "hir.IndirectCallExpr",
        layer: "flat-hir",
        disposition: RepresentationDisposition::RepresentedWithExplicitLoss,
        owner_issue: "8851",
    },
    RepresentationMapping {
        id: "hir.DynamicBoundary.CoderefCall",
        layer: "flat-hir",
        disposition: RepresentationDisposition::RepresentedWithExplicitLoss,
        owner_issue: "8851",
    },
    RepresentationMapping {
        id: "hir.HirExpr.Call",
        layer: "body-hir",
        disposition: RepresentationDisposition::RepresentedWithExplicitLoss,
        owner_issue: "8851",
    },
    RepresentationMapping {
        id: "pir.PirCallee",
        layer: "pir",
        disposition: RepresentationDisposition::UnknownUntilProducerMigration,
        owner_issue: "8854",
    },
    RepresentationMapping {
        id: "pir.PirReceiver",
        layer: "pir",
        disposition: RepresentationDisposition::UnknownUntilProducerMigration,
        owner_issue: "8854",
    },
    RepresentationMapping {
        id: "pir.PirMethod",
        layer: "pir",
        disposition: RepresentationDisposition::UnknownUntilProducerMigration,
        owner_issue: "8854",
    },
    RepresentationMapping {
        id: "pir.PirContext",
        layer: "pir",
        disposition: RepresentationDisposition::UnknownUntilProducerMigration,
        owner_issue: "8854",
    },
    RepresentationMapping {
        id: "semantic.call-facts",
        layer: "semantic",
        disposition: RepresentationDisposition::UnknownUntilProducerMigration,
        owner_issue: "8855",
    },
];

/// Typed limitation retained instead of guessing a missing fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CallLimitation {
    /// `&foo` versus `&foo()` cannot be recovered from an empty `args` vec.
    AmpersandParensFormUnavailable,
    /// `foo` versus `foo()` cannot be recovered from argument count.
    NamedParensFormUnavailable,
    /// Callee name is not a distinct AST child, so it has no independent span.
    CalleeSpanUnavailable,
    /// Body HIR call node does not retain the callee name field.
    CalleeNameUnavailable,
    /// Flat HIR retains a count, not operand identities.
    OperandsUnavailableInRepresentation,
    /// Flat HIR `object_kind` is a classification string, not a receiver operand.
    ObjectKindIsClassificationNotOperand,
    /// FunctionCall `->()` sentinel is not canonical callee identity.
    CoderefSentinelNotCanonical,
    /// Prototype application needs declaration/import evidence.
    PrototypeRequiresResolution,
    /// Method selector is text-only and looks dynamic (`$name` / `${...}`).
    DynamicMethodNameTextOnly,
    /// Parser recovery wrapper; inner fields may be incomplete.
    RecoveredSource,
    /// Traversal/lowering order is not language evaluation proof.
    EvaluationOrderUnknown,
    /// Body HIR `FunctionCall` dropped the name field, so named vs `->()` is
    /// unavailable.
    BodyHirFunctionCallFormUnavailable,
    /// Body HIR `IndirectCall` dropped the method spelling, so invocant vs
    /// filehandle is unavailable.
    BodyHirIndirectInvocantRoleUnavailable,
    /// Operand is a recovery hole, not a source-backed expression identity.
    RecoveredHoleNotAnOperandIdentity,
}

/// Versioned normalized call view.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CallSemantics {
    /// Schema family token.
    pub schema_id: &'static str,
    /// Schema vocabulary version.
    pub schema_version: u32,
    /// Source syntax form.
    pub syntax: CallSyntax,
    /// Dispatch interpretation.
    pub dispatch: DispatchKind,
    /// Callee identity as retained.
    pub callee: CalleeIdentity,
    /// Callee *expression* operand, when the representation retains one.
    ///
    /// Named/ampersand/method spelling lives in [`Self::callee`]; a coderef
    /// callee expression is this operand. Absence is not an empty argument.
    pub callee_operand: OperandPresence,
    /// Exact call-node source range.
    pub call_range: SourceLocation,
    /// Independent callee span, when the representation retains one.
    pub callee_anchor: Option<SourceLocation>,
    /// Receiver/invocant operand.
    pub receiver: OperandPresence,
    /// Ordered argument operands.
    pub arguments: OperandList,
    /// Ordered block operands.
    pub blocks: OperandList,
    /// Filehandle operand.
    pub filehandle: OperandPresence,
    /// Argument-list source disposition.
    pub argument_source: ArgumentSource,
    /// Prototype contract disposition.
    pub prototype: PrototypeDisposition,
    /// Evaluation-order contract.
    pub evaluation_order: EvaluationContract,
    /// Completeness boundary.
    pub boundary: CallBoundary,
    /// Explicit limitations.
    pub limitations: Vec<CallLimitation>,
    /// Representation that produced this view.
    pub representation: CurrentRepresentation,
}

impl CallSemantics {
    /// Compose a call view. External crates cannot use a struct literal
    /// because this type is non-exhaustive.
    #[expect(
        clippy::too_many_arguments,
        reason = "closed constructor is the foreign-crate substitute for a non-exhaustive struct literal"
    )]
    #[must_use]
    pub fn compose(
        syntax: CallSyntax,
        dispatch: DispatchKind,
        callee: CalleeIdentity,
        callee_operand: OperandPresence,
        call_range: SourceLocation,
        callee_anchor: Option<SourceLocation>,
        receiver: OperandPresence,
        arguments: OperandList,
        blocks: OperandList,
        filehandle: OperandPresence,
        argument_source: ArgumentSource,
        prototype: PrototypeDisposition,
        evaluation_order: EvaluationContract,
        boundary: CallBoundary,
        limitations: Vec<CallLimitation>,
        representation: CurrentRepresentation,
    ) -> Self {
        Self {
            schema_id: CALL_SEMANTICS_SCHEMA_ID,
            schema_version: CALL_SEMANTICS_SCHEMA_VERSION,
            syntax,
            dispatch,
            callee,
            callee_operand,
            call_range,
            callee_anchor,
            receiver,
            arguments,
            blocks,
            filehandle,
            argument_source,
            prototype,
            evaluation_order,
            boundary,
            limitations,
            representation,
        }
    }
}

/// Combinations that must not be treated as a proven call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CallContradiction {
    /// Schema identity does not match this vocabulary.
    UnknownSchema,
    /// Recovered/unknown syntax claimed exact completeness.
    RecoveredPromotedExact,
    /// Implicit `@_` combined with an explicit argument list.
    ImplicitArgsWithExplicitList,
    /// Receiver present on a form that cannot carry one.
    ReceiverOnNonReceiverForm,
    /// Filehandle present on a form that is not print-like.
    FilehandleOnUnadmittedForm,
    /// Empty explicit argument vec claimed as implicit caller `@_`.
    ///
    /// `ExplicitEmpty` with `Explicit([])` is the valid representation of a
    /// proven `foo()` / `&foo()`. Current AST adapters must not emit that
    /// pair; they use [`ArgumentSource::Unknown`] instead.
    EmptyArgsGuessed,
    /// Unavailable operand list treated as an explicit empty list is a
    /// construction error when `arguments` is unavailable and
    /// `argument_source` is `ExplicitEmpty`.
    UnavailableArgsMarkedExplicitEmpty,
}

impl CallSemantics {
    /// Wire schema identity. Changing serialized meaning must bump the version.
    #[must_use]
    pub fn schema_wire(&self) -> &'static str {
        CALL_SEMANTICS_SCHEMA_WIRE
    }

    /// Checked contradictions. Recovered/partial forms may be valid.
    #[must_use]
    pub fn contradictions(&self) -> Vec<CallContradiction> {
        let mut out = Vec::new();
        if self.schema_id != CALL_SEMANTICS_SCHEMA_ID
            || self.schema_version != CALL_SEMANTICS_SCHEMA_VERSION
        {
            out.push(CallContradiction::UnknownSchema);
        }
        if self.syntax == CallSyntax::RecoveredUnknown && self.boundary == CallBoundary::Exact {
            out.push(CallContradiction::RecoveredPromotedExact);
        }
        if self.argument_source == ArgumentSource::ImplicitCallerArgs
            && matches!(&self.arguments, OperandList::Explicit(items) if !items.is_empty())
        {
            out.push(CallContradiction::ImplicitArgsWithExplicitList);
        }
        if matches!(self.receiver, OperandPresence::Present(_))
            && !matches!(
                self.syntax,
                CallSyntax::MethodArrow | CallSyntax::Indirect | CallSyntax::PrintFilehandleLike
            )
        {
            out.push(CallContradiction::ReceiverOnNonReceiverForm);
        }
        if matches!(self.filehandle, OperandPresence::Present(_))
            && self.syntax != CallSyntax::PrintFilehandleLike
        {
            out.push(CallContradiction::FilehandleOnUnadmittedForm);
        }
        if matches!(&self.arguments, OperandList::Explicit(items) if items.is_empty())
            && self.argument_source == ArgumentSource::ImplicitCallerArgs
        {
            out.push(CallContradiction::EmptyArgsGuessed);
        }
        if matches!(self.arguments, OperandList::Unavailable { .. })
            && self.argument_source == ArgumentSource::ExplicitEmpty
        {
            out.push(CallContradiction::UnavailableArgsMarkedExplicitEmpty);
        }
        out
    }
}

/// Adapt a current AST node into [`CallSemantics`].
///
/// Returns `None` when the node is not a call representation. Never scans
/// source text. Side-effect free.
#[must_use]
pub fn adapt_ast_call(node: &Node) -> Option<CallSemantics> {
    match &node.kind {
        NodeKind::FunctionCall { name, args } => {
            Some(with_recovery_from_args(adapt_function_call(node.location, name, args), args))
        }
        NodeKind::AmperCall { name, args } => {
            Some(with_recovery_from_args(adapt_amper_call(node.location, name, args), args))
        }
        NodeKind::MethodCall { object, method, args } => {
            Some(with_recovery_from_receiver_and_args(
                adapt_method_call(node.location, object, method, args),
                object,
                args,
            ))
        }
        NodeKind::IndirectCall { method, object, args } => {
            Some(with_recovery_from_receiver_and_args(
                adapt_indirect_call(node.location, method, object, args),
                object,
                args,
            ))
        }
        NodeKind::Error { partial, .. } => {
            let inner = partial.as_deref()?;
            let mut semantics = adapt_ast_call(inner)?;
            semantics.representation = CurrentRepresentation::AstRecovered;
            semantics.boundary = CallBoundary::Recovered;
            semantics.dispatch = DispatchKind::Boundary;
            semantics.syntax = CallSyntax::RecoveredUnknown;
            push_unique(&mut semantics.limitations, CallLimitation::RecoveredSource);
            Some(semantics)
        }
        _ => None,
    }
}

fn adapt_function_call(call_range: SourceLocation, name: &str, args: &[Node]) -> CallSemantics {
    if name == CODEREF_CALL_SENTINEL {
        return adapt_coderef_call(call_range, args);
    }

    let core_qualified = name.starts_with("CORE::");
    let bare_name = name.strip_prefix("CORE::").unwrap_or(name);
    let first_is_block = args.first().is_some_and(|arg| matches!(arg.kind, NodeKind::Block { .. }));
    let block_taking = is_block_taking_name(bare_name) && first_is_block;
    let list_op = is_list_operator_name(bare_name);

    let (syntax, dispatch) = if core_qualified {
        (
            CallSyntax::CoreQualified,
            if list_op { DispatchKind::Builtin } else { DispatchKind::Function },
        )
    } else if block_taking {
        (CallSyntax::BlockBearing, DispatchKind::Builtin)
    } else if list_op {
        (CallSyntax::BuiltinListOp, DispatchKind::Builtin)
    } else {
        (CallSyntax::Named, DispatchKind::Function)
    };

    let limitations = vec![
        CallLimitation::NamedParensFormUnavailable,
        CallLimitation::CalleeSpanUnavailable,
        CallLimitation::PrototypeRequiresResolution,
        CallLimitation::EvaluationOrderUnknown,
    ];
    let (blocks, arguments) = if block_taking {
        match args.split_first() {
            Some((block_node, rest_nodes)) => {
                let block = operand_from_node(block_node, OperandRole::Block, 0);
                let rest: Vec<CallOperand> = rest_nodes
                    .iter()
                    .enumerate()
                    .map(|(i, arg)| operand_from_node(arg, OperandRole::Argument, i as u32 + 1))
                    .collect();
                (OperandList::Explicit(vec![block]), OperandList::Explicit(rest))
            }
            None => (
                OperandList::Unavailable { known_count: None },
                OperandList::Unavailable { known_count: None },
            ),
        }
    } else {
        (
            OperandList::Explicit(Vec::new()),
            OperandList::Explicit(
                args.iter()
                    .enumerate()
                    .map(|(i, arg)| operand_from_node(arg, OperandRole::Argument, i as u32))
                    .collect(),
            ),
        )
    };

    let argument_source = if args.is_empty() || (block_taking && args.len() == 1) {
        ArgumentSource::Unknown
    } else {
        ArgumentSource::ExplicitList
    };

    CallSemantics {
        schema_id: CALL_SEMANTICS_SCHEMA_ID,
        schema_version: CALL_SEMANTICS_SCHEMA_VERSION,
        syntax,
        dispatch,
        callee: CalleeIdentity::Named { name: name.to_string() },
        callee_operand: OperandPresence::Absent,
        call_range,
        callee_anchor: None,
        receiver: OperandPresence::Absent,
        arguments,
        blocks,
        filehandle: OperandPresence::Absent,
        argument_source,
        prototype: PrototypeDisposition::RequiresResolution,
        evaluation_order: EvaluationContract::Unknown,
        boundary: CallBoundary::Partial,
        limitations,
        representation: CurrentRepresentation::AstFunctionCall,
    }
}

fn adapt_coderef_call(call_range: SourceLocation, args: &[Node]) -> CallSemantics {
    let (callee_operand, argument_nodes) = match args.split_first() {
        Some((callee, rest)) => {
            (OperandPresence::Present(operand_from_node(callee, OperandRole::Callee, 0)), rest)
        }
        None => (OperandPresence::Unknown, args),
    };
    let arguments = argument_nodes
        .iter()
        .enumerate()
        .map(|(i, arg)| operand_from_node(arg, OperandRole::Argument, i as u32 + 1))
        .collect();
    let argument_source = if argument_nodes.is_empty() {
        ArgumentSource::Unknown
    } else {
        ArgumentSource::ExplicitList
    };

    CallSemantics {
        schema_id: CALL_SEMANTICS_SCHEMA_ID,
        schema_version: CALL_SEMANTICS_SCHEMA_VERSION,
        syntax: CallSyntax::Coderef,
        dispatch: DispatchKind::Coderef,
        callee: CalleeIdentity::SentinelCoderef,
        callee_operand,
        call_range,
        callee_anchor: None,
        receiver: OperandPresence::Absent,
        arguments: OperandList::Explicit(arguments),
        blocks: OperandList::Explicit(Vec::new()),
        filehandle: OperandPresence::Absent,
        argument_source,
        prototype: PrototypeDisposition::NotApplicable,
        evaluation_order: EvaluationContract::Unknown,
        boundary: CallBoundary::Partial,
        limitations: vec![
            CallLimitation::CoderefSentinelNotCanonical,
            CallLimitation::CalleeSpanUnavailable,
            CallLimitation::NamedParensFormUnavailable,
            CallLimitation::EvaluationOrderUnknown,
        ],
        representation: CurrentRepresentation::AstFunctionCall,
    }
}

fn adapt_amper_call(call_range: SourceLocation, name: &str, args: &[Node]) -> CallSemantics {
    let arguments: Vec<CallOperand> = args
        .iter()
        .enumerate()
        .map(|(i, arg)| operand_from_node(arg, OperandRole::Argument, i as u32))
        .collect();
    let argument_source =
        if args.is_empty() { ArgumentSource::Unknown } else { ArgumentSource::ExplicitList };

    CallSemantics {
        schema_id: CALL_SEMANTICS_SCHEMA_ID,
        schema_version: CALL_SEMANTICS_SCHEMA_VERSION,
        syntax: CallSyntax::Ampersand,
        dispatch: DispatchKind::Function,
        callee: CalleeIdentity::Named { name: name.to_string() },
        callee_operand: OperandPresence::Absent,
        call_range,
        callee_anchor: None,
        receiver: OperandPresence::Absent,
        arguments: OperandList::Explicit(arguments),
        blocks: OperandList::Explicit(Vec::new()),
        filehandle: OperandPresence::Absent,
        argument_source,
        prototype: PrototypeDisposition::NotApplicable,
        evaluation_order: EvaluationContract::Unknown,
        boundary: CallBoundary::Partial,
        limitations: vec![
            CallLimitation::AmpersandParensFormUnavailable,
            CallLimitation::CalleeSpanUnavailable,
            CallLimitation::EvaluationOrderUnknown,
        ],
        representation: CurrentRepresentation::AstAmperCall,
    }
}

fn adapt_method_call(
    call_range: SourceLocation,
    object: &Node,
    method: &str,
    args: &[Node],
) -> CallSemantics {
    let receiver = OperandPresence::Present(operand_from_node(object, OperandRole::Receiver, 0));
    let arguments: Vec<CallOperand> = args
        .iter()
        .enumerate()
        .map(|(i, arg)| operand_from_node(arg, OperandRole::Argument, i as u32 + 1))
        .collect();
    let dynamic = method_spelling_is_dynamic(method);
    let mut limitations =
        vec![CallLimitation::CalleeSpanUnavailable, CallLimitation::EvaluationOrderUnknown];
    if dynamic {
        limitations.push(CallLimitation::DynamicMethodNameTextOnly);
    }
    let argument_source =
        if args.is_empty() { ArgumentSource::Unknown } else { ArgumentSource::ExplicitList };

    CallSemantics {
        schema_id: CALL_SEMANTICS_SCHEMA_ID,
        schema_version: CALL_SEMANTICS_SCHEMA_VERSION,
        syntax: CallSyntax::MethodArrow,
        dispatch: if dynamic { DispatchKind::Dynamic } else { DispatchKind::Method },
        callee: CalleeIdentity::Method { name: method.to_string() },
        callee_operand: OperandPresence::Absent,
        call_range,
        callee_anchor: None,
        receiver,
        arguments: OperandList::Explicit(arguments),
        blocks: OperandList::Explicit(Vec::new()),
        filehandle: OperandPresence::Absent,
        argument_source,
        prototype: PrototypeDisposition::Unknown,
        evaluation_order: EvaluationContract::Unknown,
        boundary: if dynamic { CallBoundary::Dynamic } else { CallBoundary::Partial },
        limitations,
        representation: CurrentRepresentation::AstMethodCall,
    }
}

fn adapt_indirect_call(
    call_range: SourceLocation,
    method: &str,
    object: &Node,
    args: &[Node],
) -> CallSemantics {
    let print_like = is_print_family_name(method);
    let object_role = if print_like { OperandRole::Filehandle } else { OperandRole::Receiver };
    let object_operand = operand_from_node(object, object_role, 0);
    let arguments: Vec<CallOperand> = args
        .iter()
        .enumerate()
        .map(|(i, arg)| operand_from_node(arg, OperandRole::Argument, i as u32 + 1))
        .collect();
    let argument_source =
        if args.is_empty() { ArgumentSource::Unknown } else { ArgumentSource::ExplicitList };

    let (syntax, dispatch, receiver, filehandle) = if print_like {
        (
            CallSyntax::PrintFilehandleLike,
            DispatchKind::Builtin,
            OperandPresence::Absent,
            OperandPresence::Present(object_operand),
        )
    } else {
        (
            CallSyntax::Indirect,
            DispatchKind::IndirectObject,
            OperandPresence::Present(object_operand),
            OperandPresence::Absent,
        )
    };

    CallSemantics {
        schema_id: CALL_SEMANTICS_SCHEMA_ID,
        schema_version: CALL_SEMANTICS_SCHEMA_VERSION,
        syntax,
        dispatch,
        callee: CalleeIdentity::Method { name: method.to_string() },
        callee_operand: OperandPresence::Absent,
        call_range,
        callee_anchor: None,
        receiver,
        arguments: OperandList::Explicit(arguments),
        blocks: OperandList::Explicit(Vec::new()),
        filehandle,
        argument_source,
        prototype: PrototypeDisposition::Unknown,
        evaluation_order: EvaluationContract::Unknown,
        boundary: CallBoundary::Partial,
        limitations: vec![
            CallLimitation::CalleeSpanUnavailable,
            CallLimitation::EvaluationOrderUnknown,
        ],
        representation: CurrentRepresentation::AstIndirectCall,
    }
}

fn operand_from_node(node: &Node, role: OperandRole, source_order: u32) -> CallOperand {
    let hole = node_is_recovery_hole(node);
    CallOperand {
        source_range: if hole { None } else { Some(node.location) },
        role,
        source_order,
        evaluation: if hole {
            EvaluationContract::NotApplicable
        } else {
            EvaluationContract::Unknown
        },
        context: OperandContext::Unknown,
    }
}

fn node_is_recovery_hole(node: &Node) -> bool {
    matches!(
        node.kind,
        NodeKind::Error { .. }
            | NodeKind::MissingExpression
            | NodeKind::MissingStatement
            | NodeKind::MissingIdentifier
            | NodeKind::MissingBlock
            | NodeKind::UnknownRest
    )
}

fn is_block_taking_name(name: &str) -> bool {
    matches!(name, "map" | "grep" | "sort")
}

fn is_print_family_name(name: &str) -> bool {
    matches!(name, "print" | "printf" | "say")
}

fn is_list_operator_name(name: &str) -> bool {
    matches!(
        name,
        "print"
            | "printf"
            | "say"
            | "map"
            | "grep"
            | "sort"
            | "push"
            | "pop"
            | "shift"
            | "unshift"
            | "join"
            | "split"
            | "open"
            | "close"
            | "system"
            | "exec"
            | "die"
            | "warn"
            | "keys"
            | "values"
            | "each"
    )
}

fn method_spelling_is_dynamic(method: &str) -> bool {
    method.starts_with('$') || method.starts_with("${")
}

fn push_unique(items: &mut Vec<CallLimitation>, item: CallLimitation) {
    if !items.contains(&item) {
        items.push(item);
    }
}

fn with_recovery_from_args(semantics: CallSemantics, args: &[Node]) -> CallSemantics {
    if args.iter().any(node_contains_recovery) {
        mark_recovered_with_holes(semantics, args.iter().any(node_is_recovery_hole))
    } else {
        semantics
    }
}

fn with_recovery_from_receiver_and_args(
    semantics: CallSemantics,
    receiver: &Node,
    args: &[Node],
) -> CallSemantics {
    if node_contains_recovery(receiver) || args.iter().any(node_contains_recovery) {
        mark_recovered_with_holes(
            semantics,
            node_is_recovery_hole(receiver) || args.iter().any(node_is_recovery_hole),
        )
    } else {
        semantics
    }
}

fn mark_recovered(mut semantics: CallSemantics) -> CallSemantics {
    semantics.boundary = CallBoundary::Recovered;
    semantics.dispatch = DispatchKind::Boundary;
    push_unique(&mut semantics.limitations, CallLimitation::RecoveredSource);
    semantics
}

fn mark_recovered_with_holes(mut semantics: CallSemantics, hole: bool) -> CallSemantics {
    semantics = mark_recovered(semantics);
    if hole {
        push_unique(&mut semantics.limitations, CallLimitation::RecoveredHoleNotAnOperandIdentity);
    }
    semantics
}

fn node_contains_recovery(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Error { .. }
        | NodeKind::MissingExpression
        | NodeKind::MissingStatement
        | NodeKind::MissingIdentifier
        | NodeKind::MissingBlock
        | NodeKind::UnknownRest => true,
        _ => node.children().into_iter().any(node_contains_recovery),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_wire_identity_is_versioned() {
        assert_eq!(CALL_SEMANTICS_SCHEMA_WIRE, "call-semantics.v1");
        assert!(CALL_SEMANTICS_SCHEMA_WIRE.contains(&CALL_SEMANTICS_SCHEMA_VERSION.to_string()));
        assert_eq!(
            CALL_SEMANTICS_SCHEMA_WIRE,
            format!("{CALL_SEMANTICS_SCHEMA_ID}.v{CALL_SEMANTICS_SCHEMA_VERSION}")
        );
    }

    #[test]
    fn mapping_table_covers_ast_call_variants() {
        let ids: Vec<_> = CURRENT_REPRESENTATION_DISPOSITIONS.iter().map(|row| row.id).collect();
        for required in [
            "ast.FunctionCall",
            "ast.AmperCall",
            "ast.MethodCall",
            "ast.IndirectCall",
            "hir.CallExpr",
            "hir.MethodCallExpr",
            "hir.IndirectCallExpr",
            "hir.HirExpr.Call",
            "pir.PirCallee",
            "pir.PirReceiver",
            "pir.PirMethod",
            "pir.PirContext",
        ] {
            assert!(ids.contains(&required), "missing mapping row {required}");
        }
    }

    #[test]
    fn every_current_representation_variant_has_a_mapping_row() {
        let ids: Vec<_> = CURRENT_REPRESENTATION_DISPOSITIONS.iter().map(|row| row.id).collect();
        for representation in [
            CurrentRepresentation::AstFunctionCall,
            CurrentRepresentation::AstAmperCall,
            CurrentRepresentation::AstMethodCall,
            CurrentRepresentation::AstIndirectCall,
            CurrentRepresentation::AstRecovered,
            CurrentRepresentation::HirCallExpr,
            CurrentRepresentation::HirMethodCallExpr,
            CurrentRepresentation::HirIndirectCallExpr,
            CurrentRepresentation::HirDynamicBoundary,
            CurrentRepresentation::HirBodyCall,
        ] {
            let id = representation.mapping_id();
            assert!(ids.contains(&id), "missing mapping row {id} for {representation:?}");
        }
    }
}
