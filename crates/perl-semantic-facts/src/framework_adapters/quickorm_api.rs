//! Version-bound DBIx::QuickORM API return and preserving-method contract (#13374).
//!
//! This module is reference data, not inference. It pins, for one exact
//! upstream revision, which QuickORM methods return a handle that *preserves*
//! the receiver's source/row type parameters, which *transform* them, which are
//! row terminals, and which are metadata, counts, or side effects.
//!
//! # Why this exists
//!
//! Type propagation for QuickORM cannot be written from method spelling. At the
//! pinned revision the same method name means different things on different
//! receivers (`first` is a row terminal on a handle but an iterator cursor on
//! [`DBIx::QuickORM::Iterator`]), and a large family of handle methods are
//! dual-purpose: called with no arguments they return stored state, called with
//! arguments they return a refined clone. Encoding that as scattered
//! per-call-site conditionals in the analyzer would duplicate the same upstream
//! reading many times over. Consumers read these rows instead.
//!
//! # Authority boundary
//!
//! Every row is derived from reviewed upstream source at
//! [`QUICKORM_UPSTREAM_COMMIT`], never from this repository's own inference
//! output. A row records where it was read so a later reviewer can re-derive it.
//! This module deliberately contains no parsing, no provider behavior, and no
//! type inference; it changes no runtime behavior on its own.
//!
//! ## Composed roles are part of the surface
//!
//! `Handle.pm:28-29` composes `DBIx::QuickORM::Role::Handle` and
//! `DBIx::QuickORM::Role::Source`. A method can therefore be reachable on a
//! handle without appearing anywhere in `Handle.pm` — `any` and `cachable` both
//! are. Reading only the package body and concluding a method is absent is
//! wrong, so [`QUICKORM_EVIDENCE_FILES`] names the roles and rows cite them
//! directly. Before recording anything as
//! [`QuickOrmReturnClass::UnsupportedDynamicVariant`], check the composed roles
//! as well as the package.
//!
//! # Scope of this revision
//!
//! Covered receivers: the generated table-package helper, `ORM`, `Connection`,
//! `Handle` (including its composed roles), `Handle` acting as a derived-table
//! source, `Row`, and `Iterator`.
//!
//! Deliberately out of scope for this revision, and therefore absent rather
//! than guessed: connection transaction and lifecycle control (`txn`,
//! `auto_retry`, `connected`, `disconnect`, `reconnect`), the `state_*` row
//! manager interface, schema/table/column builder internals, dialect and plugin
//! surfaces, and every method reached only through runtime schema fill.

use serde::{Deserialize, Serialize};

/// Upstream distribution this contract is bound to.
pub const QUICKORM_FRAMEWORK_NAME: &str = "DBIx::QuickORM";

/// Stable identity of this registry's schema and revision.
pub const QUICKORM_API_RETURN_REGISTRY_VERSION: &str = "quickorm.api-return.1.v1";

/// Upstream version string observed in `lib/DBIx/QuickORM.pm` at
/// [`QUICKORM_UPSTREAM_COMMIT`].
pub const QUICKORM_UPSTREAM_VERSION: &str = "0.000029";

/// Immutable upstream commit every row in this registry was reviewed against.
pub const QUICKORM_UPSTREAM_COMMIT: &str = "99d7d6155933efd54ce7d66d8bfeb9e1ebda81a5";

/// Upstream repository the reviewed commit belongs to.
pub const QUICKORM_UPSTREAM_REPOSITORY: &str = "https://github.com/exodist/DBIx-QuickORM";

/// Upstream files rows in this registry may cite.
pub const QUICKORM_EVIDENCE_FILES: &[&str] = &[
    "lib/DBIx/QuickORM.pm",
    "lib/DBIx/QuickORM/Connection.pm",
    "lib/DBIx/QuickORM/Handle.pm",
    "lib/DBIx/QuickORM/Iterator.pm",
    "lib/DBIx/QuickORM/ORM.pm",
    "lib/DBIx/QuickORM/Role/Handle.pm",
    "lib/DBIx/QuickORM/Role/Row.pm",
    "lib/DBIx/QuickORM/Role/Source.pm",
    "lib/DBIx/QuickORM/Row.pm",
];

/// The object a case's method is invoked on.
///
/// Receiver identity is load-bearing: it is not derivable from the method name.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum QuickOrmReceiver {
    /// A caller package that imported QuickORM as a table class and therefore
    /// had helpers installed into it.
    GeneratedTablePackage,
    /// A `DBIx::QuickORM::ORM` instance.
    Orm,
    /// A `DBIx::QuickORM::Connection` instance.
    Connection,
    /// A `DBIx::QuickORM::Handle` instance used as a query handle.
    Handle,
    /// A `DBIx::QuickORM::Handle` consumed as a derived-table source, where the
    /// `Role::Source` interface answers for the subquery rather than for a
    /// concrete table.
    HandleAsDerivedSource,
    /// A `DBIx::QuickORM::Row` instance.
    Row,
    /// A `DBIx::QuickORM::Iterator` instance.
    Iterator,
}

impl QuickOrmReceiver {
    /// Stable lowercase token for rendering and comparison.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GeneratedTablePackage => "generated_table_package",
            Self::Orm => "orm",
            Self::Connection => "connection",
            Self::Handle => "handle",
            Self::HandleAsDerivedSource => "handle_as_derived_source",
            Self::Row => "row",
            Self::Iterator => "iterator",
        }
    }
}

/// The argument shape that selects this case.
///
/// For dual-purpose accessors the argument cohort, not the method name, decides
/// the return class.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum QuickOrmArgumentCohort {
    /// The method accepts no arguments.
    None,
    /// The zero-argument form of a dual-purpose accessor, which reads state.
    ZeroArgGetter,
    /// The argument-bearing form of a dual-purpose accessor, which clones.
    ValueSetter,
    /// One or more required arguments that do not change the return class.
    Required,
    /// Optional arguments that do not change the return class.
    Optional,
    /// A trailing coderef is required.
    TrailingCoderef,
    /// A trailing data hashref is required.
    TrailingDataHashref,
    /// No argument names a new source or row, so the result is a plain copy of
    /// the receiver.
    NoSourceArgument,
    /// An argument that names or is a source or row replaces the receiver's
    /// source or row on the result. This covers a `Role::Source` or `Role::Row`
    /// object and a bare table-name string, which upstream defers and resolves
    /// through the connection before setting it as the source
    /// (Handle.pm:930-948).
    SourceOrRowRebinding,
}

impl QuickOrmArgumentCohort {
    /// Stable lowercase token for rendering and comparison.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ZeroArgGetter => "zero_arg_getter",
            Self::ValueSetter => "value_setter",
            Self::Required => "required",
            Self::Optional => "optional",
            Self::TrailingCoderef => "trailing_coderef",
            Self::TrailingDataHashref => "trailing_data_hashref",
            Self::NoSourceArgument => "no_source_argument",
            Self::SourceOrRowRebinding => "source_or_row_rebinding",
        }
    }
}

/// What a case returns, in the vocabulary this contract publishes.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum QuickOrmReturnClass {
    /// A handle whose source and row type parameters equal the receiver's.
    PreserveHandleSourceRow,
    /// A handle whose source or row type parameters differ from the receiver's,
    /// either because a join rewrote the source or because the source was
    /// selected from an argument.
    TransformHandleSourceRow,
    /// At most one row: a row object, `undef`, a plain hash under `data_only`,
    /// or an async row placeholder.
    SingleOptionalRow,
    /// Zero or more rows, as a list or an arrayref.
    MultipleRows,
    /// An iterator whose items carry the row identity.
    IteratorOfRows,
    /// An open hash or hash sequence of field values rather than a blessed row.
    OpenHashOrHashSequence,
    /// A handle that erases row identity to plain data for later terminals.
    DataOnlyTransition,
    /// A boolean or a count.
    BooleanOrCount,
    /// Metadata, a builder, or a scalar that is not a row and not a count.
    MetadataOrScalar,
    /// A write or another side effect whose result is not a queryable handle.
    MutationOrSideEffectResult,
    /// Not resolvable to an exact return at the pinned revision.
    UnsupportedDynamicVariant,
}

impl QuickOrmReturnClass {
    /// Stable lowercase token for rendering and comparison.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PreserveHandleSourceRow => "preserve_handle_source_row",
            Self::TransformHandleSourceRow => "transform_handle_source_row",
            Self::SingleOptionalRow => "single_optional_row",
            Self::MultipleRows => "multiple_rows",
            Self::IteratorOfRows => "iterator_of_rows",
            Self::OpenHashOrHashSequence => "open_hash_or_hash_sequence",
            Self::DataOnlyTransition => "data_only_transition",
            Self::BooleanOrCount => "boolean_or_count",
            Self::MetadataOrScalar => "metadata_or_scalar",
            Self::MutationOrSideEffectResult => "mutation_or_side_effect_result",
            Self::UnsupportedDynamicVariant => "unsupported_dynamic_variant",
        }
    }

    /// True when this class *can* carry row identity that type propagation
    /// keeps parameterized by a source.
    ///
    /// This is a property of the class alone, so it is a necessary and not a
    /// sufficient condition. A terminal reached through a `data_only` handle
    /// yields plain hashes rather than blessed rows even though its class is
    /// row-carrying — see the `handle.data_only` row, whose
    /// [`QuickOrmTypeParamEffect::ErasedToPlainData`] records that erasure.
    /// A consumer must therefore combine this with the handle's `data_only`
    /// state; it must not be read as an unconditional promise of a row.
    pub const fn may_carry_row_identity(self) -> bool {
        matches!(
            self,
            Self::PreserveHandleSourceRow
                | Self::TransformHandleSourceRow
                | Self::SingleOptionalRow
                | Self::MultipleRows
                | Self::IteratorOfRows
        )
    }
}

/// How many values a case yields.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum QuickOrmMultiplicity {
    /// Exactly one value.
    One,
    /// One value or `undef`.
    ZeroOrOne,
    /// A flat list of zero or more values.
    ListOfZeroOrMore,
    /// An arrayref holding zero or more values.
    ArrayRefOfZeroOrMore,
    /// An iterator yielding zero or more values.
    IteratorOfZeroOrMore,
    /// A hash or hashref of field values that is always present.
    Hash,
    /// A hashref of field values that is `undef` when the backing state slot
    /// is absent.
    OptionalHash,
    /// A flat key/value list in list context, not a hash container.
    ///
    /// Scalar context is deliberately *not* specified here: it follows the
    /// producing construct, not the shape. A parenthesized list yields its last
    /// element, while a `map` expression yields the number of elements it
    /// produced. Each row records which of those applies, so a consumer must
    /// read the row rather than assume one behavior for the class.
    KeyValueSequence,
    /// No meaningful return value.
    Nothing,
}

impl QuickOrmMultiplicity {
    /// Stable lowercase token for rendering and comparison.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::One => "one",
            Self::ZeroOrOne => "zero_or_one",
            Self::ListOfZeroOrMore => "list_of_zero_or_more",
            Self::ArrayRefOfZeroOrMore => "arrayref_of_zero_or_more",
            Self::IteratorOfZeroOrMore => "iterator_of_zero_or_more",
            Self::Hash => "hash",
            Self::OptionalHash => "optional_hash",
            Self::KeyValueSequence => "key_value_sequence",
            Self::Nothing => "nothing",
        }
    }
}

/// What happens to the receiver's source/row type parameters.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum QuickOrmTypeParamEffect {
    /// Source and row identity are carried through unchanged.
    PreservedFromReceiver,
    /// The source becomes a join and the row becomes a join row.
    TransformedToJoinRow,
    /// The source is taken from an argument rather than the receiver.
    DerivedFromArgumentSource,
    /// Row identity is erased to plain data.
    ErasedToPlainData,
    /// The effect depends on the argument's value rather than on the call, so
    /// it cannot be resolved from the call site alone.
    DeterminedByArgumentValue,
    /// The case carries no source/row type parameter.
    NotApplicable,
}

impl QuickOrmTypeParamEffect {
    /// Stable lowercase token for rendering and comparison.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PreservedFromReceiver => "preserved_from_receiver",
            Self::TransformedToJoinRow => "transformed_to_join_row",
            Self::DerivedFromArgumentSource => "derived_from_argument_source",
            Self::ErasedToPlainData => "erased_to_plain_data",
            Self::DeterminedByArgumentValue => "determined_by_argument_value",
            Self::NotApplicable => "not_applicable",
        }
    }
}

/// Which execution modes the case admits.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum QuickOrmModeSupport {
    /// Upstream croaks unless the handle is synchronous.
    SyncOnly,
    /// Synchronous, async, aside, and forked handles are all admitted.
    SyncAsyncAsideForked,
    /// Synchronous, async, and aside handles are admitted; upstream croaks on a
    /// forked handle.
    SyncAsyncAsideOnly,
    /// Admitted on async handles, but the async form returns a row placeholder
    /// rather than the synchronous result shape.
    SyncWithAsyncRowResult,
    /// Execution mode does not affect this case.
    NotApplicable,
}

impl QuickOrmModeSupport {
    /// Stable lowercase token for rendering and comparison.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SyncOnly => "sync_only",
            Self::SyncAsyncAsideForked => "sync_async_aside_forked",
            Self::SyncAsyncAsideOnly => "sync_async_aside_only",
            Self::SyncWithAsyncRowResult => "sync_with_async_row_result",
            Self::NotApplicable => "not_applicable",
        }
    }
}

/// Whether upstream refuses the call in void context.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum QuickOrmVoidContext {
    /// Upstream croaks when the result is discarded.
    Croaks,
    /// Void context is permitted.
    Permitted,
}

impl QuickOrmVoidContext {
    /// Stable lowercase token for rendering and comparison.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Croaks => "croaks",
            Self::Permitted => "permitted",
        }
    }
}

/// How exactly the case resolves at the pinned revision.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum QuickOrmBoundary {
    /// The return is statically determined by receiver and argument cohort.
    Exact,
    /// The return depends on runtime source, row, or data-only state that this
    /// contract does not resolve statically.
    RuntimeResolved,
    /// The method cannot be resolved at the pinned revision.
    UnsupportedAtPinnedVersion,
}

impl QuickOrmBoundary {
    /// Stable lowercase token for rendering and comparison.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::RuntimeResolved => "runtime_resolved",
            Self::UnsupportedAtPinnedVersion => "unsupported_at_pinned_version",
        }
    }
}

/// Where a row was read in reviewed upstream source.
///
/// `Serialize` only, by construction: the fields are `&'static str` borrowed
/// from the compiled-in table, which cannot be deserialized into. The registry
/// is compiled-in reference data, never persisted and read back, so there is no
/// round trip to support — a consumer reads [`QUICKORM_API_CASES`] directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct QuickOrmEvidence {
    /// Path relative to the upstream distribution root.
    pub file: &'static str,
    /// One-based line of the definition that establishes the return.
    pub line: u32,
}

/// One reviewed `receiver × method × argument cohort` return contract.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct QuickOrmApiCase {
    /// Stable identity of this case, unique within the registry.
    pub api_case_id: &'static str,
    /// Upstream package that defines the method.
    pub package: &'static str,
    /// Method name as written at the call site.
    pub method: &'static str,
    /// Object the method is invoked on.
    pub receiver: QuickOrmReceiver,
    /// Preconditions upstream enforces on the receiver.
    pub receiver_constraints: &'static [&'static str],
    /// Argument shape that selects this case.
    pub arguments: QuickOrmArgumentCohort,
    /// What the case returns.
    pub return_class: QuickOrmReturnClass,
    /// How many values the case yields.
    pub multiplicity: QuickOrmMultiplicity,
    /// Effect on source/row type parameters.
    pub type_params: QuickOrmTypeParamEffect,
    /// Admitted execution modes.
    pub mode: QuickOrmModeSupport,
    /// Void-context behavior.
    pub void_context: QuickOrmVoidContext,
    /// Static resolution boundary.
    pub boundary: QuickOrmBoundary,
    /// Reviewed upstream evidence for this row.
    pub evidence: QuickOrmEvidence,
    /// Short reviewer-facing note.
    pub notes: &'static str,
}

const HANDLE: &str = "lib/DBIx/QuickORM/Handle.pm";
const ROW: &str = "lib/DBIx/QuickORM/Row.pm";
const ITER: &str = "lib/DBIx/QuickORM/Iterator.pm";
const CONN: &str = "lib/DBIx/QuickORM/Connection.pm";
const ORM: &str = "lib/DBIx/QuickORM/ORM.pm";
const DSL: &str = "lib/DBIx/QuickORM.pm";
/// `Handle.pm:28` composes this role, so its methods are reachable on a handle
/// even though they are not defined in `Handle.pm`.
const ROLE_HANDLE: &str = "lib/DBIx/QuickORM/Role/Handle.pm";
/// `Handle.pm:29` composes this role; a handle answers its interface when it is
/// used as a derived-table source.
const ROLE_SOURCE: &str = "lib/DBIx/QuickORM/Role/Source.pm";
/// `Row.pm:24` composes this role, which supplies most of the row surface —
/// including every link-following method — outside `Row.pm` itself.
const ROLE_ROW: &str = "lib/DBIx/QuickORM/Role/Row.pm";

const PKG_HANDLE: &str = "DBIx::QuickORM::Handle";
const PKG_ROW: &str = "DBIx::QuickORM::Row";
const PKG_ITER: &str = "DBIx::QuickORM::Iterator";
const PKG_CONN: &str = "DBIx::QuickORM::Connection";
const PKG_ORM: &str = "DBIx::QuickORM::ORM";
const PKG_DSL: &str = "DBIx::QuickORM";

const NO_CONSTRAINTS: &[&str] = &[];

use QuickOrmArgumentCohort as A;
use QuickOrmBoundary as B;
use QuickOrmModeSupport as M;
use QuickOrmMultiplicity as N;
use QuickOrmReceiver as R;
use QuickOrmReturnClass as C;
use QuickOrmTypeParamEffect as T;
use QuickOrmVoidContext as V;

/// The reviewed QuickORM API return contract, ordered by [`QuickOrmApiCase::api_case_id`].
/// The reviewed QuickORM API return contract, ordered by [`QuickOrmApiCase::api_case_id`].
pub const QUICKORM_API_CASES: &[QuickOrmApiCase] = &[
    // ---- Connection
    QuickOrmApiCase {
        api_case_id: "conn.all",
        package: PKG_CONN,
        method: "all",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MultipleRows,
        multiplicity: N::ListOfZeroOrMore,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncOnly,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: CONN, line: 996 },
        notes: "Delegates to handle(@_)->all; inherits the handle terminal's sync-only rule.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.any",
        package: PKG_CONN,
        method: "any",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::SingleOptionalRow,
        multiplicity: N::ZeroOrOne,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_HANDLE, line: 107 },
        notes: "Delegates to the handle's `any`, which `Role::Handle` (composed at Handle.pm:28) defines as `shift->first(@_)`. It is an alias for first, not an unsupported method.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.aside",
        package: PKG_CONN,
        method: "aside",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: CONN, line: 993 },
        notes: "Builds a handle from the argument source, then marks it aside. Implemented as a tail call `$self->handle(@_)->aside` (Connection.pm:992-994), so Perl propagates the caller's context and the handle refiner's `defined wantarray` guard croaks in void context.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.async",
        package: PKG_CONN,
        method: "async",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: CONN, line: 992 },
        notes: "Builds a handle from the argument source, then marks it async. Implemented as a tail call `$self->handle(@_)->async` (Connection.pm:992-994), so Perl propagates the caller's context and the handle refiner's `defined wantarray` guard croaks in void context.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.by_id",
        package: PKG_CONN,
        method: "by_id",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::SingleOptionalRow,
        multiplicity: N::ZeroOrOne,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: CONN, line: 1004 },
        notes: "Trailing argument is the id; delegates to the handle terminal.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.by_ids",
        package: PKG_CONN,
        method: "by_ids",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MultipleRows,
        multiplicity: N::ArrayRefOfZeroOrMore,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: CONN, line: 1043 },
        notes: "Returns an arrayref, not a flat list.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.count",
        package: PKG_CONN,
        method: "count",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::BooleanOrCount,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncOnly,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: CONN, line: 1001 },
        notes: "Delegates to the sync-only handle count, which may be undef.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.db",
        package: PKG_CONN,
        method: "db",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: CONN, line: 433 },
        notes: "Returns the DB object backing the connection.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.delete",
        package: PKG_CONN,
        method: "delete",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MutationOrSideEffectResult,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: CONN, line: 1002 },
        notes: "Delegates to the handle write; undef when synchronous.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.find_or_insert",
        package: PKG_CONN,
        method: "find_or_insert",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::TrailingDataHashref,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: CONN, line: 1011 },
        notes: "one($arg) // insert($arg): the branch taken is runtime state, so the row comes from either a select or a write.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.first",
        package: PKG_CONN,
        method: "first",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::SingleOptionalRow,
        multiplicity: N::ZeroOrOne,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: CONN, line: 999 },
        notes: "Row terminal on a connection; distinct from Iterator::first.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.forked",
        package: PKG_CONN,
        method: "forked",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: CONN, line: 994 },
        notes: "Builds a handle from the argument source, then marks it forked. Implemented as a tail call `$self->handle(@_)->forked` (Connection.pm:992-994), so Perl propagates the caller's context and the handle refiner's `defined wantarray` guard croaks in void context.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.handle",
        package: PKG_CONN,
        method: "handle",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: CONN, line: 928 },
        notes: "Croaks on undef. A handle passed here is consumed as a derived table, not \
         refined in place.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.insert",
        package: PKG_CONN,
        method: "insert",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::TrailingDataHashref,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: CONN, line: 1006 },
        notes: "Delegates to the handle write, which returns the inserted row.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.iterate",
        package: PKG_CONN,
        method: "iterate",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::TrailingCoderef,
        return_class: C::MutationOrSideEffectResult,
        multiplicity: N::Nothing,
        type_params: T::NotApplicable,
        mode: M::SyncOnly,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: CONN, line: 1005 },
        notes: "Callback-driven; returns nothing.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.iterator",
        package: PKG_CONN,
        method: "iterator",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::IteratorOfRows,
        multiplicity: N::IteratorOfZeroOrMore,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: CONN, line: 997 },
        notes: "Item identity follows the selected source.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.one",
        package: PKG_CONN,
        method: "one",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::SingleOptionalRow,
        multiplicity: N::ZeroOrOne,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: CONN, line: 1000 },
        notes: "Delegates to the handle terminal, which may return undef.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.source",
        package: PKG_CONN,
        method: "source",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: CONN, line: 861 },
        notes: "Resolves a source; a blessed argument must do Role::Source, and no_fatal \
         turns a miss into undef instead of a croak.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.update",
        package: PKG_CONN,
        method: "update",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::TrailingDataHashref,
        return_class: C::MutationOrSideEffectResult,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: CONN, line: 1008 },
        notes: "Delegates to the handle write; undef when synchronous.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.update_or_insert",
        package: PKG_CONN,
        method: "update_or_insert",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::TrailingDataHashref,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: CONN, line: 1010 },
        notes: "Alias onto the handle upsert path; upstream delegation proves equivalence. Returns the row.",
    },
    QuickOrmApiCase {
        api_case_id: "conn.vivify",
        package: PKG_CONN,
        method: "vivify",
        receiver: R::Connection,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::TrailingDataHashref,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: CONN, line: 1007 },
        notes: "Delegates to handle vivify, returning an in-memory row bound to the selected source.",
    },
    // ---- Generated DSL
    QuickOrmApiCase {
        api_case_id: "dsl.qorm_table",
        package: PKG_DSL,
        method: "qorm_table",
        receiver: R::GeneratedTablePackage,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: DSL, line: 951 },
        notes: "Installed into a table package as a closure returning a clone of the table \
         definition. It is schema metadata, never a row.",
    },
    // ---- Handle
    QuickOrmApiCase {
        api_case_id: "handle.all",
        package: PKG_HANDLE,
        method: "all",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::MultipleRows,
        multiplicity: N::ListOfZeroOrMore,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncOnly,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 3264 },
        notes: "Croaks unless sync. Yields a flat list of rows, or of plain data under \
         data_only; item identity follows the handle's source.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.all_fields",
        package: PKG_HANDLE,
        method: "all_fields",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1127 },
        notes: "Clone selecting every field and clearing omit.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.and",
        package: PKG_HANDLE,
        method: "and",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1153 },
        notes: "Installed as a named closure in a glob block (Handle.pm:1151-1163). Clones with the existing where combined through `qorm_and`; source and row are untouched.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.any",
        package: PKG_HANDLE,
        method: "any",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::SingleOptionalRow,
        multiplicity: N::ZeroOrOne,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_HANDLE, line: 107 },
        notes: "Supplied by `Role::Handle`, composed at Handle.pm:28, as a plain alias for first. Not defined in Handle.pm itself.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.aside",
        package: PKG_HANDLE,
        method: "aside",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1085 },
        notes: "Mode clone; returns self when already aside.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.async",
        package: PKG_HANDLE,
        method: "async",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1078 },
        notes: "Mode clone; returns self when already async.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.auto_refresh",
        package: PKG_HANDLE,
        method: "auto_refresh",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1057 },
        notes: "Config clone; returns self when already set.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.by_id.copy",
        package: PKG_HANDLE,
        method: "by_id",
        receiver: R::Handle,
        receiver_constraints: &["no where clause", "no associated row", "source has a primary key"],
        arguments: A::NoSourceArgument,
        return_class: C::SingleOptionalRow,
        multiplicity: N::ZeroOrOne,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1899 },
        notes: "Called with only the trailing id, `shift->handle()` receives an empty list and copies the receiver, so the row keeps the receiver's source. May answer from the row cache, returns raw_fields under data_only, and otherwise falls through to one(), which may be undef. There is no synchronous-handle gate.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.by_id.rebind",
        package: PKG_HANDLE,
        method: "by_id",
        receiver: R::Handle,
        receiver_constraints: &[
            "trailing id argument is always required",
            "rebound source has a primary key",
        ],
        arguments: A::SourceOrRowRebinding,
        return_class: C::SingleOptionalRow,
        multiplicity: N::ZeroOrOne,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1900 },
        notes: "`my $id = pop; my $self = shift->handle(@_)` forwards every leading argument into `Handle::handle`, which can replace SOURCE. The source, primary key and cache lookup then all read the rebound handle (Handle.pm:1905-1906,1933), so the row comes from the rebound source, not the receiver's. The where/row croaks are likewise checked against the rebound handle (Handle.pm:1902-1903).",
    },
    QuickOrmApiCase {
        api_case_id: "handle.by_ids",
        package: PKG_HANDLE,
        method: "by_ids",
        receiver: R::Handle,
        receiver_constraints: &["no where clause", "no associated row"],
        arguments: A::Required,
        return_class: C::MultipleRows,
        multiplicity: N::ArrayRefOfZeroOrMore,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1938 },
        notes: "Returns an arrayref of by_id results, not a flat list.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.cas",
        package: PKG_HANDLE,
        method: "cas",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MutationOrSideEffectResult,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideOnly,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 2729 },
        notes: "Returns a CAS::Result (Handle.pm:2788), not a row. Croaks on a forked handle (Handle.pm:2741); an async or aside result resolves lazily.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.clone.copy",
        package: PKG_HANDLE,
        method: "clone",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::NoSourceArgument,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 763 },
        notes: "Upstream clone, new, and handle are one code path: clone is `$self->handle(@_)` (Handle.pm:763) and new is `$proto->handle(@_)` (Handle.pm:761). With no source or row argument the result is a preserving copy.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.clone.rebind",
        package: PKG_HANDLE,
        method: "clone",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::SourceOrRowRebinding,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 763 },
        notes: "Upstream clone, new, and handle are one code path: clone is `$self->handle(@_)` (Handle.pm:763) and new is `$proto->handle(@_)` (Handle.pm:761). An argument doing Role::Source or Role::Row overwrites SOURCE or ROW on the clone (Handle.pm:797-843), so the receiver's row type does not survive.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.connection.get",
        package: PKG_HANDLE,
        method: "connection",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ZeroArgGetter,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1294 },
        notes: "Zero-argument form returns the stored connection.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.connection.set",
        package: PKG_HANDLE,
        method: "connection",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ValueSetter,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1294 },
        notes: "Argument form clones with a new connection.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.count",
        package: PKG_HANDLE,
        method: "count",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::BooleanOrCount,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncOnly,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 3313 },
        notes: "Croaks on an async handle. Returns a count, never a row, and undef when the select yields no row at all (Handle.pm:3324) — reachable when an inherited offset suppresses the aggregate row.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.cross_join",
        package: PKG_HANDLE,
        method: "cross_join",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::TransformedToJoinRow,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 391 },
        notes: "Clone whose source is the resulting join; the original row type does not \
         survive.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.data_only.enable",
        package: PKG_HANDLE,
        method: "data_only",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::DataOnlyTransition,
        multiplicity: N::One,
        type_params: T::ErasedToPlainData,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1099 },
        notes: "The no-argument form clones with data-only set, so later terminals yield plain hashes instead of blessed rows.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.data_only.set",
        package: PKG_HANDLE,
        method: "data_only",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ValueSetter,
        return_class: C::DataOnlyTransition,
        multiplicity: N::One,
        type_params: T::DeterminedByArgumentValue,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1102 },
        notes: "A truthy value erases row identity; `data_only(0)` clones with the mode cleared and restores blessed-row terminals (Handle.pm:1102-1105). The effect follows the argument's truthiness, so a consumer must read the value, not just the call.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.delete",
        package: PKG_HANDLE,
        method: "delete",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::MutationOrSideEffectResult,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 2430 },
        notes: "Returns the statement handle on a non-sync handle and undef when synchronous (Handle.pm:2525-2528); never a row. A forked bulk delete requires a bound row.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.dialect",
        package: PKG_HANDLE,
        method: "dialect",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 202 },
        notes: "Dialect metadata for the handle's connection.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.distinct",
        package: PKG_HANDLE,
        method: "distinct",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1113 },
        notes: "Config clone; returns self when already distinct. A falsy argument clears the flag without changing row identity.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.fields.get",
        package: PKG_HANDLE,
        method: "fields",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ZeroArgGetter,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1315 },
        notes: "Zero-argument form returns the stored field selection.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.fields.set",
        package: PKG_HANDLE,
        method: "fields",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ValueSetter,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1315 },
        notes: "A single arrayref replaces the selection; other arguments append to it.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.first",
        package: PKG_HANDLE,
        method: "first",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::SingleOptionalRow,
        multiplicity: N::ZeroOrOne,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 3238 },
        notes: "Row terminal: undef when nothing matches, a plain hash under data_only, or \
         an async row placeholder. Distinct from Iterator::first.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.forked",
        package: PKG_HANDLE,
        method: "forked",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1092 },
        notes: "Mode clone; returns self when already forked.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.full_join",
        package: PKG_HANDLE,
        method: "full_join",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::TransformedToJoinRow,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 390 },
        notes: "Clone whose source is the resulting join.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.handle.copy",
        package: PKG_HANDLE,
        method: "handle",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::NoSourceArgument,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 765 },
        notes: "With no source or row argument this is the same preserving copy as clone.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.handle.rebind",
        package: PKG_HANDLE,
        method: "handle",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::SourceOrRowRebinding,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 765 },
        notes: "An argument doing Role::Source or Role::Row replaces the source or row (Handle.pm:797-843).",
    },
    QuickOrmApiCase {
        api_case_id: "handle.inner_join",
        package: PKG_HANDLE,
        method: "inner_join",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::TransformedToJoinRow,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 389 },
        notes: "Clone whose source is the resulting join.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.insert",
        package: PKG_HANDLE,
        method: "insert",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 2077 },
        notes: "Returns the inserted row: `_insert` ends in `state_insert_row` (Handle.pm:2312-2323), or a `Row::Async` placeholder on an async statement (Handle.pm:2305). Routes through the refreshing path when auto_refresh or a literal write is in play.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.insert_and_refresh",
        package: PKG_HANDLE,
        method: "insert_and_refresh",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 2083 },
        notes: "Always takes the refreshing path, which branches on `is_sync` (Handle.pm:2141-2147) rather than refusing a non-sync handle.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.internal_transactions",
        package: PKG_HANDLE,
        method: "internal_transactions",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1136 },
        notes: "Config clone.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.internal_txns",
        package: PKG_HANDLE,
        method: "internal_txns",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1133 },
        notes: "Alias; upstream delegates to internal_transactions, which proves equivalence.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.is_aside",
        package: PKG_HANDLE,
        method: "is_aside",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1554 },
        notes: "Mode predicate.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.is_async",
        package: PKG_HANDLE,
        method: "is_async",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1553 },
        notes: "Mode predicate.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.is_forked",
        package: PKG_HANDLE,
        method: "is_forked",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1555 },
        notes: "Mode predicate.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.is_sync",
        package: PKG_HANDLE,
        method: "is_sync",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1552 },
        notes: "True only when none of forked, async, or aside is set.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.iterate",
        package: PKG_HANDLE,
        method: "iterate",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::TrailingCoderef,
        return_class: C::MutationOrSideEffectResult,
        multiplicity: N::Nothing,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncOnly,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 3376 },
        notes: "Croaks unless the final argument is a coderef and the handle is sync; \
         returns nothing.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.iterator",
        package: PKG_HANDLE,
        method: "iterator",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::IteratorOfRows,
        multiplicity: N::IteratorOfZeroOrMore,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 3289 },
        notes: "Returns an Iterator whose items are rows, or plain data under data_only. \
         Item identity is not erased.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.join",
        package: PKG_HANDLE,
        method: "join",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::TransformedToJoinRow,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 385 },
        notes: "Installed as a glob alias onto the shared join implementation.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.left_join",
        package: PKG_HANDLE,
        method: "left_join",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::TransformedToJoinRow,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 387 },
        notes: "Clone whose source is the resulting join.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.limit.get",
        package: PKG_HANDLE,
        method: "limit",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ZeroArgGetter,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1341 },
        notes: "Zero-argument form returns the stored limit.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.limit.set",
        package: PKG_HANDLE,
        method: "limit",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ValueSetter,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1341 },
        notes: "Argument form clones with a new limit.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.new.copy",
        package: PKG_HANDLE,
        method: "new",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::NoSourceArgument,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 761 },
        notes: "Upstream documents new, handle and clone as interchangeable aliases usable on an existing instance or on the class (Handle.pm:486-490); new is `$proto->handle(@_)`. Called on an existing handle with no source or row argument it is a preserving copy, exactly like clone.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.new.rebind",
        package: PKG_HANDLE,
        method: "new",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::SourceOrRowRebinding,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 761 },
        notes: "Upstream documents new, handle and clone as interchangeable aliases usable on an existing instance or on the class (Handle.pm:486-490); new is `$proto->handle(@_)`. Called on the class, or with a source or row argument, the result's source comes from the arguments.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.no_auto_refresh",
        package: PKG_HANDLE,
        method: "no_auto_refresh",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1064 },
        notes: "Config clone; returns self when already unset.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.no_internal_transactions",
        package: PKG_HANDLE,
        method: "no_internal_transactions",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1143 },
        notes: "Config clone with inverted argument sense.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.no_internal_txns",
        package: PKG_HANDLE,
        method: "no_internal_txns",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1134 },
        notes: "Alias; upstream delegates to no_internal_transactions.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.offset.get",
        package: PKG_HANDLE,
        method: "offset",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ZeroArgGetter,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1348 },
        notes: "Zero-argument form returns the stored offset.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.offset.set",
        package: PKG_HANDLE,
        method: "offset",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ValueSetter,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1348 },
        notes: "Argument form clones with a new offset.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.omit.get",
        package: PKG_HANDLE,
        method: "omit",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ZeroArgGetter,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1328 },
        notes: "Zero-argument form returns the stored omit set.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.omit.set",
        package: PKG_HANDLE,
        method: "omit",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ValueSetter,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1328 },
        notes: "A single arrayref replaces the omit set; other arguments append to it.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.one",
        package: PKG_HANDLE,
        method: "one",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::SingleOptionalRow,
        multiplicity: N::ZeroOrOne,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 3212 },
        notes: "Returns undef when nothing matches, a plain hash under data_only, or an \
         async row placeholder. It is not unconditionally a row.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.or",
        package: PKG_HANDLE,
        method: "or",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1159 },
        notes: "Installed as a named closure in a glob block (Handle.pm:1151-1163). Clones with the existing where combined through `qorm_or`; source and row are untouched.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.order_by.get",
        package: PKG_HANDLE,
        method: "order_by",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ZeroArgGetter,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1369 },
        notes: "Zero-argument form returns the stored ordering.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.order_by.set",
        package: PKG_HANDLE,
        method: "order_by",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ValueSetter,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1369 },
        notes: "Several arguments are collected into an arrayref.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.right_join",
        package: PKG_HANDLE,
        method: "right_join",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::TransformedToJoinRow,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 388 },
        notes: "Clone whose source is the resulting join.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.row.get",
        package: PKG_HANDLE,
        method: "row",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ZeroArgGetter,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1308 },
        notes: "Zero-argument form returns the bound row, if any.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.row.set",
        package: PKG_HANDLE,
        method: "row",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ValueSetter,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1308 },
        notes: "Binding a row clears the where clause, and it also replaces the source: the consistency croak is guarded by `if ($set{+SOURCE})`, which only tracks a source passed in the same call, so `row($other)` takes the else branch and overwrites the inherited source with the row's own (Handle.pm:833-840). The receiver's row type does not survive.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.source.get",
        package: PKG_HANDLE,
        method: "source",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ZeroArgGetter,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1301 },
        notes: "Zero-argument form returns the source object, not a row.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.source.set",
        package: PKG_HANDLE,
        method: "source",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ValueSetter,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1301 },
        notes: "Replacing the source replaces the handle's row identity.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.sql_builder.get",
        package: PKG_HANDLE,
        method: "sql_builder",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ZeroArgGetter,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1284 },
        notes: "Zero-argument form returns the builder, resolving and caching it if unset.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.sql_builder.set",
        package: PKG_HANDLE,
        method: "sql_builder",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ValueSetter,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1284 },
        notes: "Argument form clones with a new builder.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.subquery_alias.get",
        package: PKG_HANDLE,
        method: "subquery_alias",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ZeroArgGetter,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1440 },
        notes: "Zero-argument form returns the stored alias.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.subquery_alias.set",
        package: PKG_HANDLE,
        method: "subquery_alias",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ValueSetter,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1440 },
        notes: "Argument form clones with a new alias.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.sync",
        package: PKG_HANDLE,
        method: "sync",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1071 },
        notes: "Clears forked, async, and aside together.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.target.get",
        package: PKG_HANDLE,
        method: "target",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ZeroArgGetter,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1362 },
        notes: "Zero-argument form returns the stored write target.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.target.set",
        package: PKG_HANDLE,
        method: "target",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ValueSetter,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1362 },
        notes: "Argument form clones with a new write target.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.update",
        package: PKG_HANDLE,
        method: "update",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::MutationOrSideEffectResult,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 2530 },
        notes: "Returns the statement handle on a non-sync handle and undef when synchronous (Handle.pm:2718-2728); never a row. A bulk update without a bound row croaks unless the handle is synchronous.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.upsert",
        package: PKG_HANDLE,
        method: "upsert",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 2066 },
        notes: "Same row-returning path as insert, with the upsert flag set (Handle.pm:2066-2070).",
    },
    QuickOrmApiCase {
        api_case_id: "handle.upsert_and_refresh",
        package: PKG_HANDLE,
        method: "upsert_and_refresh",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncWithAsyncRowResult,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 2072 },
        notes: "Always takes the refreshing path; returns the row.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.using_internal_transactions",
        package: PKG_HANDLE,
        method: "using_internal_transactions",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1556 },
        notes: "Config predicate.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.vivify.copy",
        package: PKG_HANDLE,
        method: "vivify",
        receiver: R::Handle,
        receiver_constraints: &["trailing data hashref is always required"],
        arguments: A::NoSourceArgument,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1946 },
        notes: "Croaks with fewer than two arguments and without a trailing data hashref (Handle.pm:1946-1948). Called with only that hashref, `shift->handle()` receives an empty list and copies the receiver, so the vivified row keeps the receiver's source. There is no synchronous-handle gate.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.vivify.rebind",
        package: PKG_HANDLE,
        method: "vivify",
        receiver: R::Handle,
        receiver_constraints: &["trailing data hashref is always required"],
        arguments: A::SourceOrRowRebinding,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1950 },
        notes: "`my $data = pop; ... my $self = shift->handle(@_)` forwards every leading argument into `Handle::handle`, which can replace SOURCE. `state_vivify_row` is then passed `source => $self->{+SOURCE}` (Handle.pm:1954-1955), so the in-memory row is bound to the rebound source, not the receiver's.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.where.get",
        package: PKG_HANDLE,
        method: "where",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ZeroArgGetter,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1355 },
        notes: "Zero-argument form returns the stored where clause.",
    },
    QuickOrmApiCase {
        api_case_id: "handle.where.set",
        package: PKG_HANDLE,
        method: "where",
        receiver: R::Handle,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::ValueSetter,
        return_class: C::PreserveHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Croaks,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1355 },
        notes: "Setting a where clause clears any bound row.",
    },
    // ---- Handle as a derived-table source
    QuickOrmApiCase {
        api_case_id: "handle_source.cachable",
        package: PKG_HANDLE,
        method: "cachable",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ROLE_SOURCE, line: 97 },
        notes: "Supplied by `Role::Source` (composed at Handle.pm:29) and not overridden by Handle. It derives from primary_key, which a handle answers as undef, so a derived table is never cachable.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.field_affinity",
        package: PKG_HANDLE,
        method: "field_affinity",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1502 },
        notes: "Delegates to the inner source, defaulting to string when the field is absent.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.field_db_name",
        package: PKG_HANDLE,
        method: "field_db_name",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1482 },
        notes: "Identity on a derived table.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.field_is_generated",
        package: PKG_HANDLE,
        method: "field_is_generated",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1475 },
        notes: "Always false for a derived table.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.field_orm_name",
        package: PKG_HANDLE,
        method: "field_orm_name",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1483 },
        notes: "Identity on a derived table.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.field_type",
        package: PKG_HANDLE,
        method: "field_type",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1494 },
        notes: "Undef unless the inner source has the field.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.fields_list_all",
        package: PKG_HANDLE,
        method: "fields_list_all",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1477 },
        notes: "A derived table reports the wildcard rather than an enumerable field list.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.fields_to_fetch",
        package: PKG_HANDLE,
        method: "fields_to_fetch",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1476 },
        notes: "A derived table reports the wildcard.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.fields_to_omit",
        package: PKG_HANDLE,
        method: "fields_to_omit",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1478 },
        notes: "Always undef for a derived table.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.has_field",
        package: PKG_HANDLE,
        method: "has_field",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1485 },
        notes: "A derived table's output columns are not enumerable, so any name is accepted.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.is_writable",
        package: PKG_HANDLE,
        method: "is_writable",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1470 },
        notes: "A derived table is read-only.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.primary_key",
        package: PKG_HANDLE,
        method: "primary_key",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1472 },
        notes: "Answers undef unconditionally (Handle.pm:1472); a concrete table source answers differently.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.row_class",
        package: PKG_HANDLE,
        method: "row_class",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1473 },
        notes: "Answers undef. Handle defines this unconditionally (Handle.pm:1473) as its Role::Source answer; it is not gated on the handle actually being nested. Reading it as the generic row-class answer would still be wrong: a concrete table source answers differently.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.source_db_moniker",
        package: PKG_HANDLE,
        method: "source_db_moniker",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: &["handle has a source", "subquery alias is an identifier"],
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1449 },
        notes: "Renders the inner query as a literal subquery reference with binds. Croaks when the handle has no source, or when the subquery alias is not an identifier.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.source_has_aliases",
        package: PKG_HANDLE,
        method: "source_has_aliases",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1474 },
        notes: "Always false for a derived table.",
    },
    QuickOrmApiCase {
        api_case_id: "handle_source.source_orm_name",
        package: PKG_HANDLE,
        method: "source_orm_name",
        receiver: R::HandleAsDerivedSource,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: HANDLE, line: 1467 },
        notes: "Falls back to the default subquery alias.",
    },
    // ---- Iterator
    QuickOrmApiCase {
        api_case_id: "iter.first",
        package: PKG_ITER,
        method: "first",
        receiver: R::Iterator,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::SingleOptionalRow,
        multiplicity: N::ZeroOrOne,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ITER, line: 129 },
        notes: "Resets to the start and returns the first item. Same spelling as the handle \
         and connection terminals, different receiver and different semantics.",
    },
    QuickOrmApiCase {
        api_case_id: "iter.last",
        package: PKG_ITER,
        method: "last",
        receiver: R::Iterator,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::SingleOptionalRow,
        multiplicity: N::ZeroOrOne,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ITER, line: 143 },
        notes: "Exhausts the generator and returns the last item, or undef.",
    },
    QuickOrmApiCase {
        api_case_id: "iter.list",
        package: PKG_ITER,
        method: "list",
        receiver: R::Iterator,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MultipleRows,
        multiplicity: N::ListOfZeroOrMore,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ITER, line: 164 },
        notes: "Exhausts the generator and returns every item as a flat list.",
    },
    QuickOrmApiCase {
        api_case_id: "iter.next",
        package: PKG_ITER,
        method: "next",
        receiver: R::Iterator,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::SingleOptionalRow,
        multiplicity: N::ZeroOrOne,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ITER, line: 106 },
        notes: "Yields the next item, or empty once exhausted. Item identity is whatever the \
         producing handle put in.",
    },
    QuickOrmApiCase {
        api_case_id: "iter.ready",
        package: PKG_ITER,
        method: "ready",
        receiver: R::Iterator,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ITER, line: 184 },
        notes: "True unless a readiness coderef was supplied and reports otherwise.",
    },
    // ---- ORM
    QuickOrmApiCase {
        api_case_id: "orm.connect",
        package: PKG_ORM,
        method: "connect",
        receiver: R::Orm,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ORM, line: 143 },
        notes: "Establishes and returns a connection.",
    },
    QuickOrmApiCase {
        api_case_id: "orm.connection",
        package: PKG_ORM,
        method: "connection",
        receiver: R::Orm,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ORM, line: 193 },
        notes: "Returns the cached connection, connecting on first use.",
    },
    QuickOrmApiCase {
        api_case_id: "orm.db",
        package: PKG_ORM,
        method: "db",
        receiver: R::Orm,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ORM, line: 122 },
        notes: "Returns the DB definition. With an argument it sets it write-once, croaking if the DB is already set or was never set (ORM.pm:122-131).",
    },
    QuickOrmApiCase {
        api_case_id: "orm.disconnect",
        package: PKG_ORM,
        method: "disconnect",
        receiver: R::Orm,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MutationOrSideEffectResult,
        multiplicity: N::Nothing,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ORM, line: 180 },
        notes: "Lifecycle side effect.",
    },
    QuickOrmApiCase {
        api_case_id: "orm.handle",
        package: PKG_ORM,
        method: "handle",
        receiver: R::Orm,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::SyncAsyncAsideForked,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ORM, line: 198 },
        notes: "Delegates to the connection; the result's source comes from the arguments.",
    },
    QuickOrmApiCase {
        api_case_id: "orm.reconnect",
        package: PKG_ORM,
        method: "reconnect",
        receiver: R::Orm,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ORM, line: 187 },
        notes: "Replaces and returns the connection.",
    },
    // ---- Row
    QuickOrmApiCase {
        api_case_id: "row.cas",
        package: PKG_ROW,
        method: "cas",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MutationOrSideEffectResult,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 301 },
        notes: "Delegates to `_stored_handle->cas`. Handle::cas croaks on a forked handle, but a row builds its own handle from the connection.",
    },
    QuickOrmApiCase {
        api_case_id: "row.check_pk",
        package: PKG_ROW,
        method: "check_pk",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 183 },
        notes: "Returns the receiving row when its source has a primary key, otherwise croaks (Role/Row.pm:183-187).",
    },
    QuickOrmApiCase {
        api_case_id: "row.check_sync",
        package: PKG_ROW,
        method: "check_sync",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 186 },
        notes: "Returns `_check_stale`, which returns the receiving row (Row.pm:513-516) or croaks. It is not a boolean.",
    },
    QuickOrmApiCase {
        api_case_id: "row.clone",
        package: PKG_ROW,
        method: "clone",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 147 },
        notes: "Produces another row of the same source identity.",
    },
    QuickOrmApiCase {
        api_case_id: "row.conflate_args",
        package: PKG_ROW,
        method: "conflate_args",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::OpenHashOrHashSequence,
        multiplicity: N::KeyValueSequence,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 156 },
        notes: "Returns a flat key/value list — field, value, source, dialect, affinity (Role/Row.pm:160) — not a hash container. It is a parenthesized list, so in scalar context the comma operator yields its last element.",
    },
    QuickOrmApiCase {
        api_case_id: "row.connection",
        package: PKG_ROW,
        method: "connection",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ROW, line: 111 },
        notes: "Connection behind the row's data object.",
    },
    QuickOrmApiCase {
        api_case_id: "row.delete",
        package: PKG_ROW,
        method: "delete",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MutationOrSideEffectResult,
        multiplicity: N::Nothing,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 296 },
        notes: "Delegates to `_stored_handle->delete`. That handle comes from `connection->handle($self)` and is synchronous, so Handle::delete always takes its `return undef` branch (Handle.pm:2525-2528) — there is no statement handle to observe.",
    },
    QuickOrmApiCase {
        api_case_id: "row.desynced_data",
        package: PKG_ROW,
        method: "desynced_data",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::OpenHashOrHashSequence,
        multiplicity: N::OptionalHash,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 118 },
        notes: "Reads the DESYNC slot directly (Row.pm:118), so it is undef unless the row is desynced.",
    },
    QuickOrmApiCase {
        api_case_id: "row.dialect",
        package: PKG_ROW,
        method: "dialect",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 97 },
        notes: "Delegates to the connection's dialect.",
    },
    QuickOrmApiCase {
        api_case_id: "row.discard",
        package: PKG_ROW,
        method: "discard",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ROW, line: 287 },
        notes: "Clears pending and desync state and returns the same row.",
    },
    QuickOrmApiCase {
        api_case_id: "row.display",
        package: PKG_ROW,
        method: "display",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 138 },
        notes: "Human-readable source name plus primary-key values; a string, never a row.",
    },
    QuickOrmApiCase {
        api_case_id: "row.field",
        package: PKG_ROW,
        method: "field",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 436 },
        notes: "Inflated single field value. This is the ordinary column accessor; named \
         per-column accessors are an autorow feature and are not modeled here.",
    },
    QuickOrmApiCase {
        api_case_id: "row.field_affinity",
        package: PKG_ROW,
        method: "field_affinity",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 100 },
        notes: "Delegates to the source, passing the row's dialect.",
    },
    QuickOrmApiCase {
        api_case_id: "row.field_is_desynced",
        package: PKG_ROW,
        method: "field_is_desynced",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 453 },
        notes: "Per-field desync predicate.",
    },
    QuickOrmApiCase {
        api_case_id: "row.fields",
        package: PKG_ROW,
        method: "fields",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::OpenHashOrHashSequence,
        multiplicity: N::Hash,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 439 },
        notes: "Inflated field map merged from pending over stored.",
    },
    QuickOrmApiCase {
        api_case_id: "row.follow",
        package: PKG_ROW,
        method: "follow",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 333 },
        notes: "Resolves the link and returns a handle on the link's *other* table (Role/Row.pm:344), so the receiver's row type does not survive. The handle comes from the connection and starts synchronous; a caller may refine it afterwards.",
    },
    QuickOrmApiCase {
        api_case_id: "row.force_sync",
        package: PKG_ROW,
        method: "force_sync",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 269 },
        notes: "Clears the desync flag and returns the same row (Row.pm:269-273).",
    },
    QuickOrmApiCase {
        api_case_id: "row.handle.copy",
        package: PKG_ROW,
        method: "handle",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::NoSourceArgument,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 121 },
        notes: "Role/Row.pm:121-124 builds a handle scoped to the row's own source and row, then passes trailing arguments through Handle::handle. With no source or row argument the result stays on the receiver's own source, and starts synchronous.",
    },
    QuickOrmApiCase {
        api_case_id: "row.handle.rebind",
        package: PKG_ROW,
        method: "handle",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::SourceOrRowRebinding,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 121 },
        notes: "Role/Row.pm:121-124 builds a handle scoped to the row's own source and row, then passes trailing arguments through Handle::handle. A source or row argument reaches Handle::handle and replaces that source, so the result can be a handle on another table.",
    },
    QuickOrmApiCase {
        api_case_id: "row.has_field",
        package: PKG_ROW,
        method: "has_field",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 99 },
        notes: "Delegates to the source; croaks without a field name.",
    },
    QuickOrmApiCase {
        api_case_id: "row.has_pending",
        package: PKG_ROW,
        method: "has_pending",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ROW, line: 126 },
        notes: "State predicate.",
    },
    QuickOrmApiCase {
        api_case_id: "row.in_storage",
        package: PKG_ROW,
        method: "in_storage",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ROW, line: 123 },
        notes: "State predicate.",
    },
    QuickOrmApiCase {
        api_case_id: "row.insert",
        package: PKG_ROW,
        method: "insert",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 239 },
        notes: "Writes through the connection and returns the same row (Role/Row.pm:248). Croaks when already stored or when nothing is pending.",
    },
    QuickOrmApiCase {
        api_case_id: "row.insert_or_save",
        package: PKG_ROW,
        method: "insert_or_save",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 230 },
        notes: "Dispatches to save when stored and insert when pending; both return the row. Croaks when there is nothing to write.",
    },
    QuickOrmApiCase {
        api_case_id: "row.insert_related",
        package: PKG_ROW,
        method: "insert_related",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::DerivedFromArgumentSource,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 358 },
        notes: "Inserts into the link's other table with the local values copied in, so the result belongs to that table, not the receiver's source.",
    },
    QuickOrmApiCase {
        api_case_id: "row.is_desynced",
        package: PKG_ROW,
        method: "is_desynced",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ROW, line: 125 },
        notes: "State predicate.",
    },
    QuickOrmApiCase {
        api_case_id: "row.is_invalid",
        package: PKG_ROW,
        method: "is_invalid",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ROW, line: 120 },
        notes: "State predicate.",
    },
    QuickOrmApiCase {
        api_case_id: "row.is_stored",
        package: PKG_ROW,
        method: "is_stored",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ROW, line: 124 },
        notes: "Alias; upstream delegates to in_storage.",
    },
    QuickOrmApiCase {
        api_case_id: "row.is_valid",
        package: PKG_ROW,
        method: "is_valid",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ROW, line: 121 },
        notes: "State predicate.",
    },
    QuickOrmApiCase {
        api_case_id: "row.obtain",
        package: PKG_ROW,
        method: "obtain",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::SingleOptionalRow,
        multiplicity: N::ZeroOrOne,
        type_params: T::DerivedFromArgumentSource,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 348 },
        notes: "follow($link)->one, so it may be undef. Croaks unless the link is unique. `follow` builds a fresh handle from the connection, which is synchronous, so this never yields an async placeholder.",
    },
    QuickOrmApiCase {
        api_case_id: "row.pending_data",
        package: PKG_ROW,
        method: "pending_data",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::OpenHashOrHashSequence,
        multiplicity: N::OptionalHash,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 117 },
        notes: "Reads the PENDING slot directly (Row.pm:117), so it is undef when nothing is pending; `has_pending` guards the absent slot explicitly.",
    },
    QuickOrmApiCase {
        api_case_id: "row.pending_field",
        package: PKG_ROW,
        method: "pending_field",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 443 },
        notes: "Inflated pending value for one field.",
    },
    QuickOrmApiCase {
        api_case_id: "row.pending_fields",
        package: PKG_ROW,
        method: "pending_fields",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::OpenHashOrHashSequence,
        multiplicity: N::Hash,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 449 },
        notes: "Inflated pending field map.",
    },
    QuickOrmApiCase {
        api_case_id: "row.primary_key_field_list",
        package: PKG_ROW,
        method: "primary_key_field_list",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ListOfZeroOrMore,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 103 },
        notes: "Flat list of primary-key field names, empty when the source has no primary key.",
    },
    QuickOrmApiCase {
        api_case_id: "row.primary_key_hash",
        package: PKG_ROW,
        method: "primary_key_hash",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::OpenHashOrHashSequence,
        multiplicity: N::KeyValueSequence,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 105 },
        notes: "A `map` producing a flat key/value list (Role/Row.pm:105), not a hashref; `primary_key_hashref` is the reference form. In scalar context `map` yields the number of elements it produced, not a value — unlike the parenthesized list in conflate_args. Croaks through check_pk without a primary key.",
    },
    QuickOrmApiCase {
        api_case_id: "row.primary_key_hashref",
        package: PKG_ROW,
        method: "primary_key_hashref",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::OpenHashOrHashSequence,
        multiplicity: N::Hash,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 106 },
        notes: "The same pairs wrapped as a hashref (Role/Row.pm:106).",
    },
    QuickOrmApiCase {
        api_case_id: "row.primary_key_value_list",
        package: PKG_ROW,
        method: "primary_key_value_list",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ListOfZeroOrMore,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 104 },
        notes: "Raw stored primary-key values in field order; croaks through check_pk without a primary key.",
    },
    QuickOrmApiCase {
        api_case_id: "row.raw_field",
        package: PKG_ROW,
        method: "raw_field",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 437 },
        notes: "Uninflated single field value.",
    },
    QuickOrmApiCase {
        api_case_id: "row.raw_fields",
        package: PKG_ROW,
        method: "raw_fields",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::OpenHashOrHashSequence,
        multiplicity: N::Hash,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 440 },
        notes: "Uninflated field map; this is what by_id returns under data_only.",
    },
    QuickOrmApiCase {
        api_case_id: "row.raw_pending_field",
        package: PKG_ROW,
        method: "raw_pending_field",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 446 },
        notes: "Uninflated pending value for one field.",
    },
    QuickOrmApiCase {
        api_case_id: "row.raw_pending_fields",
        package: PKG_ROW,
        method: "raw_pending_fields",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::OpenHashOrHashSequence,
        multiplicity: N::Hash,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 451 },
        notes: "Uninflated pending field map.",
    },
    QuickOrmApiCase {
        api_case_id: "row.raw_stored_field",
        package: PKG_ROW,
        method: "raw_stored_field",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 445 },
        notes: "Uninflated stored value for one field.",
    },
    QuickOrmApiCase {
        api_case_id: "row.raw_stored_fields",
        package: PKG_ROW,
        method: "raw_stored_fields",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::OpenHashOrHashSequence,
        multiplicity: N::Hash,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 450 },
        notes: "Uninflated stored field map.",
    },
    QuickOrmApiCase {
        api_case_id: "row.refresh",
        package: PKG_ROW,
        method: "refresh",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 276 },
        notes: "Returns the refreshed row, or croaks when the row no longer exists (Row.pm:276-285). The handle comes from `_stored_handle`, so a row exposes no mode selector.",
    },
    QuickOrmApiCase {
        api_case_id: "row.row_data",
        package: PKG_ROW,
        method: "row_data",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 114 },
        notes: "Active row-data record.",
    },
    QuickOrmApiCase {
        api_case_id: "row.row_data_obj",
        package: PKG_ROW,
        method: "row_data_obj",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ROW, line: 113 },
        notes: "Row-data object itself.",
    },
    QuickOrmApiCase {
        api_case_id: "row.save",
        package: PKG_ROW,
        method: "save",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 250 },
        notes: "Returns the same row on every path, including the early return when nothing is pending (Role/Row.pm:256-262).",
    },
    QuickOrmApiCase {
        api_case_id: "row.siblings",
        package: PKG_ROW,
        method: "siblings",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::TransformHandleSourceRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROLE_ROW, line: 373 },
        notes: "Returns a handle on the receiver's own source filtered to rows sharing the link's local values; includes the original row. The handle starts synchronous.",
    },
    QuickOrmApiCase {
        api_case_id: "row.source",
        package: PKG_ROW,
        method: "source",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::MetadataOrScalar,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ROW, line: 110 },
        notes: "Source behind the row. Same spelling as the handle accessor, different \
         receiver and no clone behavior.",
    },
    QuickOrmApiCase {
        api_case_id: "row.stored_data",
        package: PKG_ROW,
        method: "stored_data",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::OpenHashOrHashSequence,
        multiplicity: N::OptionalHash,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 116 },
        notes: "Reads the STORED slot directly (Row.pm:116), so it is undef when the row has no stored state.",
    },
    QuickOrmApiCase {
        api_case_id: "row.stored_field",
        package: PKG_ROW,
        method: "stored_field",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Required,
        return_class: C::MetadataOrScalar,
        multiplicity: N::ZeroOrOne,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 442 },
        notes: "Inflated stored value for one field.",
    },
    QuickOrmApiCase {
        api_case_id: "row.stored_fields",
        package: PKG_ROW,
        method: "stored_fields",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::OpenHashOrHashSequence,
        multiplicity: N::Hash,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 448 },
        notes: "Inflated stored field map.",
    },
    QuickOrmApiCase {
        api_case_id: "row.track_desync",
        package: PKG_ROW,
        method: "track_desync",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::None,
        return_class: C::BooleanOrCount,
        multiplicity: N::One,
        type_params: T::NotApplicable,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::Exact,
        evidence: QuickOrmEvidence { file: ROW, line: 108 },
        notes: "Constant true on the base row class.",
    },
    QuickOrmApiCase {
        api_case_id: "row.update",
        package: PKG_ROW,
        method: "update",
        receiver: R::Row,
        receiver_constraints: NO_CONSTRAINTS,
        arguments: A::Optional,
        return_class: C::SingleOptionalRow,
        multiplicity: N::One,
        type_params: T::PreservedFromReceiver,
        mode: M::NotApplicable,
        void_context: V::Permitted,
        boundary: B::RuntimeResolved,
        evidence: QuickOrmEvidence { file: ROW, line: 307 },
        notes: "Stages the changes, saves, and returns the same row (Row.pm:365).",
    },
];

/// All reviewed cases, in registry order.
pub fn quickorm_api_cases() -> &'static [QuickOrmApiCase] {
    QUICKORM_API_CASES
}

/// Look one case up by its stable identity.
pub fn quickorm_api_case(api_case_id: &str) -> Option<&'static QuickOrmApiCase> {
    QUICKORM_API_CASES.iter().find(|case| case.api_case_id == api_case_id)
}

/// Every case defined for one `package::method` spelling.
///
/// This returns more than one case whenever the argument cohort changes the
/// return, so callers must select on [`QuickOrmApiCase::arguments`] rather than
/// taking the first match.
pub fn quickorm_api_cases_for_method<'a>(
    package: &'a str,
    method: &'a str,
) -> impl Iterator<Item = &'static QuickOrmApiCase> + 'a {
    QUICKORM_API_CASES.iter().filter(move |case| case.package == package && case.method == method)
}

/// Every case defined on one receiver.
pub fn quickorm_api_cases_for_receiver(
    receiver: QuickOrmReceiver,
) -> impl Iterator<Item = &'static QuickOrmApiCase> {
    QUICKORM_API_CASES.iter().filter(move |case| case.receiver == receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_test_must::must_some_with;
    use std::collections::BTreeSet;

    fn case_by_id(id: &str) -> &'static QuickOrmApiCase {
        must_some_with(
            quickorm_api_case(id),
            format!("registry is missing the required case `{id}`"),
        )
    }

    // ------------------------------------------------------------- structure

    #[test]
    fn registry_is_not_empty_and_pins_one_upstream_revision() {
        assert!(!QUICKORM_API_CASES.is_empty());
        assert_eq!(QUICKORM_UPSTREAM_VERSION, "0.000029");
        assert_eq!(QUICKORM_UPSTREAM_COMMIT.len(), 40);
        assert!(
            QUICKORM_UPSTREAM_COMMIT.chars().all(|c| c.is_ascii_hexdigit()),
            "the pinned commit must be an exact SHA-1, not a branch or tag"
        );
    }

    #[test]
    fn api_case_ids_are_unique() {
        let mut seen = BTreeSet::new();
        for case in QUICKORM_API_CASES {
            assert!(seen.insert(case.api_case_id), "duplicate api_case_id `{}`", case.api_case_id);
        }
    }

    #[test]
    fn registry_order_is_deterministic() {
        let ids: Vec<&str> = QUICKORM_API_CASES.iter().map(|c| c.api_case_id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(
            ids, sorted,
            "rows must stay sorted by api_case_id so generated views are stable"
        );
    }

    #[test]
    fn every_row_cites_reviewable_upstream_evidence() {
        for case in QUICKORM_API_CASES {
            assert!(
                QUICKORM_EVIDENCE_FILES.contains(&case.evidence.file),
                "`{}` cites unknown upstream file `{}`",
                case.api_case_id,
                case.evidence.file
            );
            assert!(case.evidence.line > 0, "`{}` must cite a real line", case.api_case_id);
            assert!(!case.notes.is_empty(), "`{}` must carry a reviewer note", case.api_case_id);
        }
    }

    #[test]
    fn api_case_id_prefix_agrees_with_receiver() {
        for case in QUICKORM_API_CASES {
            let expected = match case.receiver {
                QuickOrmReceiver::Connection => "conn.",
                QuickOrmReceiver::GeneratedTablePackage => "dsl.",
                QuickOrmReceiver::Handle => "handle.",
                QuickOrmReceiver::HandleAsDerivedSource => "handle_source.",
                QuickOrmReceiver::Iterator => "iter.",
                QuickOrmReceiver::Orm => "orm.",
                QuickOrmReceiver::Row => "row.",
            };
            assert!(
                case.api_case_id.starts_with(expected),
                "`{}` does not carry the `{expected}` prefix for its receiver",
                case.api_case_id
            );
        }
    }

    #[test]
    fn classes_remain_distinct_and_populated() {
        let classes: BTreeSet<&str> =
            QUICKORM_API_CASES.iter().map(|c| c.return_class.as_str()).collect();
        for required in [
            "preserve_handle_source_row",
            "transform_handle_source_row",
            "single_optional_row",
            "multiple_rows",
            "iterator_of_rows",
            "open_hash_or_hash_sequence",
            "data_only_transition",
            "boolean_or_count",
            "metadata_or_scalar",
            "mutation_or_side_effect_result",
        ] {
            assert!(
                classes.contains(required),
                "class `{required}` has no row, so it cannot be discriminated"
            );
        }

        // `unsupported_dynamic_variant` is deliberately unpopulated. Every
        // method in scope resolves at the pinned revision, including
        // `Connection::any`, which `Role::Handle` supplies. A row may only
        // claim this class with evidence that the method is genuinely absent
        // from the composed roles as well as the package.
        assert!(
            !classes.contains("unsupported_dynamic_variant"),
            "a row claims to be unsupported; confirm against the composed roles \
             (Handle.pm:28-29) before recording that, not just the package body"
        );
    }

    // --------------------------------------------------- required mutation controls

    /// Mutation control 1: `qorm_table` must never be a row result.
    #[test]
    fn qorm_table_is_metadata_not_a_row() {
        let case = case_by_id("dsl.qorm_table");
        assert_eq!(case.return_class, QuickOrmReturnClass::MetadataOrScalar);
        assert!(!case.return_class.may_carry_row_identity());
        assert_eq!(case.receiver, QuickOrmReceiver::GeneratedTablePackage);
    }

    /// Mutation control 2: a source-transforming join must not be marked preserving.
    #[test]
    fn joins_transform_the_source_and_row() {
        for id in [
            "handle.join",
            "handle.left_join",
            "handle.right_join",
            "handle.inner_join",
            "handle.full_join",
            "handle.cross_join",
        ] {
            let case = case_by_id(id);
            assert_eq!(
                case.return_class,
                QuickOrmReturnClass::TransformHandleSourceRow,
                "`{id}` must not claim to preserve the receiver's row type"
            );
            assert_eq!(
                case.type_params,
                QuickOrmTypeParamEffect::TransformedToJoinRow,
                "`{id}` must record that the row becomes a join row"
            );
        }
    }

    /// Mutation control 3: `one` must not unconditionally return a row.
    #[test]
    fn one_is_optional_not_unconditional() {
        for id in ["handle.one", "handle.first", "conn.one", "conn.first"] {
            let case = case_by_id(id);
            assert_eq!(case.return_class, QuickOrmReturnClass::SingleOptionalRow);
            assert_eq!(
                case.multiplicity,
                QuickOrmMultiplicity::ZeroOrOne,
                "`{id}` can return undef and must not be modeled as exactly one row"
            );
        }
    }

    /// Mutation control 4: `all` and `iterator` must keep item identity.
    #[test]
    fn all_and_iterator_keep_item_identity() {
        let all = case_by_id("handle.all");
        assert_eq!(all.return_class, QuickOrmReturnClass::MultipleRows);
        assert_eq!(all.multiplicity, QuickOrmMultiplicity::ListOfZeroOrMore);
        assert_eq!(all.type_params, QuickOrmTypeParamEffect::PreservedFromReceiver);

        let iterator = case_by_id("handle.iterator");
        assert_eq!(iterator.return_class, QuickOrmReturnClass::IteratorOfRows);
        assert_eq!(iterator.multiplicity, QuickOrmMultiplicity::IteratorOfZeroOrMore);
        assert_eq!(iterator.type_params, QuickOrmTypeParamEffect::PreservedFromReceiver);

        // by_ids returns an arrayref, which is not the same shape as all().
        let by_ids = case_by_id("handle.by_ids");
        assert_eq!(by_ids.multiplicity, QuickOrmMultiplicity::ArrayRefOfZeroOrMore);
    }

    /// Mutation control 5: `data_only` must not be a blessed row.
    #[test]
    fn data_only_erases_row_identity() {
        // The bare call erases row identity outright.
        let enable = case_by_id("handle.data_only.enable");
        assert_eq!(enable.return_class, QuickOrmReturnClass::DataOnlyTransition);
        assert_eq!(enable.type_params, QuickOrmTypeParamEffect::ErasedToPlainData);
        assert_eq!(enable.boundary, QuickOrmBoundary::Exact);

        // `data_only(0)` restores blessed rows, so the value-bearing form must
        // not assert erasure; its effect follows the argument.
        let set = case_by_id("handle.data_only.set");
        assert_eq!(set.return_class, QuickOrmReturnClass::DataOnlyTransition);
        assert_eq!(set.type_params, QuickOrmTypeParamEffect::DeterminedByArgumentValue);
        assert_eq!(set.boundary, QuickOrmBoundary::RuntimeResolved);
        // `data_only(0)` clears the mode and restores blessed rows, so the
        // value-bearing form cannot be statically resolved from the call alone.
        assert_eq!(case_by_id("handle.data_only.set").boundary, QuickOrmBoundary::RuntimeResolved);
        assert_eq!(case_by_id("handle.data_only.enable").boundary, QuickOrmBoundary::Exact);
    }

    /// Mutation control 6: sync-only terminals must not claim async support.
    #[test]
    fn sync_only_terminals_are_marked_sync_only() {
        for id in ["handle.all", "handle.count", "handle.iterate"] {
            let case = case_by_id(id);
            assert_eq!(
                case.mode,
                QuickOrmModeSupport::SyncOnly,
                "`{id}` croaks on a non-sync handle upstream"
            );
        }
        // one/first do run on async handles, but with a different result shape.
        for id in ["handle.one", "handle.first"] {
            assert_eq!(case_by_id(id).mode, QuickOrmModeSupport::SyncWithAsyncRowResult);
        }
    }

    /// Mutation control 7: a runtime-resolved selector must not be upgraded to exact.
    #[test]
    fn runtime_resolved_selectors_stay_qualified() {
        for id in ["conn.handle", "conn.source", "orm.handle", "handle.source.set"] {
            let case = case_by_id(id);
            assert_eq!(
                case.boundary,
                QuickOrmBoundary::RuntimeResolved,
                "`{id}` resolves its source at runtime and must stay qualified"
            );
        }
    }

    /// Mutation control 8: method spelling alone must not decide semantics.
    #[test]
    fn receiver_identity_is_load_bearing() {
        // `first` means three different things on three receivers.
        let handle_first = case_by_id("handle.first");
        let iter_first = case_by_id("iter.first");
        assert_eq!(handle_first.method, iter_first.method);
        assert_ne!(handle_first.receiver, iter_first.receiver);
        assert_ne!(handle_first.mode, iter_first.mode);

        // `source` is a dual-purpose accessor on a handle and a plain reader on a row.
        let row_source = case_by_id("row.source");
        assert_eq!(row_source.return_class, QuickOrmReturnClass::MetadataOrScalar);
        assert_eq!(
            case_by_id("handle.source.set").return_class,
            QuickOrmReturnClass::TransformHandleSourceRow
        );

        // row_class answers undef as the derived-table Role::Source answer, so
        // the receiver, not the method name, carries that meaning.
        let row_class = case_by_id("handle_source.row_class");
        assert_eq!(row_class.receiver, QuickOrmReceiver::HandleAsDerivedSource);

        // `any` is reachable on a handle only through the composed
        // `Role::Handle`, where it is a plain alias for `first`. Its rows must
        // therefore match the corresponding `first` rows rather than diverge.
        for (any_id, first_id) in [("handle.any", "handle.first"), ("conn.any", "conn.first")] {
            let any = case_by_id(any_id);
            let first = case_by_id(first_id);
            assert_eq!(any.return_class, first.return_class, "{any_id} aliases {first_id}");
            assert_eq!(any.multiplicity, first.multiplicity, "{any_id} aliases {first_id}");
            assert_eq!(any.type_params, first.type_params, "{any_id} aliases {first_id}");
            assert_eq!(
                any.evidence.file, ROLE_HANDLE,
                "{any_id} is supplied by the composed role, so it must cite the role"
            );
        }
    }

    // ------------------------------------------------------- invariant coverage

    #[test]
    fn dual_purpose_accessors_have_both_forms() {
        for method in [
            "connection",
            "fields",
            "limit",
            "offset",
            "omit",
            "order_by",
            "row",
            "source",
            "sql_builder",
            "subquery_alias",
            "target",
            "where",
        ] {
            let cohorts: BTreeSet<QuickOrmArgumentCohort> =
                quickorm_api_cases_for_method(PKG_HANDLE, method)
                    .filter(|c| c.receiver == QuickOrmReceiver::Handle)
                    .map(|c| c.arguments)
                    .collect();
            assert!(
                cohorts.contains(&QuickOrmArgumentCohort::ZeroArgGetter)
                    && cohorts.contains(&QuickOrmArgumentCohort::ValueSetter),
                "`{method}` is dual-purpose upstream and needs both argument cohorts"
            );
        }
    }

    #[test]
    fn void_context_croak_is_recorded_for_refining_methods() {
        for id in [
            "handle.auto_refresh",
            "handle.data_only.enable",
            "handle.data_only.set",
            "handle.where.set",
            "handle.left_join",
            "handle.and",
            "handle.or",
        ] {
            assert_eq!(
                case_by_id(id).void_context,
                QuickOrmVoidContext::Croaks,
                "`{id}` croaks in void context upstream"
            );
        }
    }

    #[test]
    fn mutations_never_carry_row_identity_as_a_handle() {
        // `may_carry_row_identity` partitions the vocabulary; a row-bearing class
        // must never be answered as plain data, metadata, a count, or a write.
        for case in QUICKORM_API_CASES {
            let expected = matches!(
                case.return_class,
                QuickOrmReturnClass::PreserveHandleSourceRow
                    | QuickOrmReturnClass::TransformHandleSourceRow
                    | QuickOrmReturnClass::SingleOptionalRow
                    | QuickOrmReturnClass::MultipleRows
                    | QuickOrmReturnClass::IteratorOfRows
            );
            assert_eq!(
                case.return_class.may_carry_row_identity(),
                expected,
                "`{}` is misclassified by may_carry_row_identity",
                case.api_case_id
            );
        }

        // Every write row must also erase the handle's type parameters or keep
        // them explicit; none may claim a join transform it never performs.
        for case in QUICKORM_API_CASES
            .iter()
            .filter(|c| c.return_class == QuickOrmReturnClass::MutationOrSideEffectResult)
        {
            assert!(
                !case.return_class.may_carry_row_identity(),
                "`{}` cannot be both a write and a queryable handle",
                case.api_case_id
            );
            assert_ne!(
                case.type_params,
                QuickOrmTypeParamEffect::TransformedToJoinRow,
                "`{}` is a write and cannot produce a join row",
                case.api_case_id
            );
        }
    }

    /// A method supplied by a composed role must cite that role, not the
    /// package body. Reading only `Handle.pm` is what produced the withdrawn
    /// "Connection::any is unsupported" row.
    #[test]
    fn role_supplied_methods_cite_their_role() {
        for (id, file) in [
            ("handle.any", ROLE_HANDLE),
            ("conn.any", ROLE_HANDLE),
            ("handle_source.cachable", ROLE_SOURCE),
            ("row.follow", ROLE_ROW),
            ("row.obtain", ROLE_ROW),
            ("row.siblings", ROLE_ROW),
            ("row.save", ROLE_ROW),
            ("row.insert", ROLE_ROW),
            ("row.check_pk", ROLE_ROW),
            ("row.primary_key_hashref", ROLE_ROW),
        ] {
            assert_eq!(
                case_by_id(id).evidence.file,
                file,
                "`{id}` comes from a composed role and must cite it"
            );
        }
    }

    /// A link method that crosses to another table must not claim the
    /// receiver's row type, and one that stays on the source must.
    #[test]
    fn row_link_methods_separate_other_table_from_own_source() {
        for id in ["row.follow", "row.obtain", "row.insert_related"] {
            assert_eq!(
                case_by_id(id).type_params,
                QuickOrmTypeParamEffect::DerivedFromArgumentSource,
                "`{id}` resolves a link to another table"
            );
        }
        for id in ["row.siblings", "row.handle.copy"] {
            assert_eq!(
                case_by_id(id).type_params,
                QuickOrmTypeParamEffect::PreservedFromReceiver,
                "`{id}` stays on the receiver's own source"
            );
        }
    }

    /// `and`/`or` combine the where clause; they never touch source or row.
    #[test]
    fn where_combinators_preserve_the_handle() {
        for id in ["handle.and", "handle.or"] {
            let case = case_by_id(id);
            assert_eq!(case.return_class, QuickOrmReturnClass::PreserveHandleSourceRow);
            assert_eq!(case.void_context, QuickOrmVoidContext::Croaks);
        }
    }

    /// Binding a row also rebinds the source, because the consistency croak
    /// only fires for a source passed in the same call.
    #[test]
    fn binding_a_row_rebinds_the_source() {
        let case = case_by_id("handle.row.set");
        assert_eq!(case.return_class, QuickOrmReturnClass::TransformHandleSourceRow);
        assert_eq!(case.type_params, QuickOrmTypeParamEffect::DerivedFromArgumentSource);
        // The reading form still just returns the bound row, if any.
        assert_eq!(
            case_by_id("handle.row.get").return_class,
            QuickOrmReturnClass::MetadataOrScalar
        );
    }

    /// `new`, `clone` and `handle` are documented interchangeable aliases, so
    /// each must carry both the copying and the rebinding form.
    #[test]
    fn constructor_aliases_share_both_forms() {
        for method in ["new", "clone", "handle"] {
            let cohorts: BTreeSet<QuickOrmArgumentCohort> =
                quickorm_api_cases_for_method(PKG_HANDLE, method)
                    .filter(|c| c.receiver == QuickOrmReceiver::Handle)
                    .map(|c| c.arguments)
                    .collect();
            assert!(
                cohorts.contains(&QuickOrmArgumentCohort::NoSourceArgument)
                    && cohorts.contains(&QuickOrmArgumentCohort::SourceOrRowRebinding),
                "`{method}` is an alias of the same body and needs both forms"
            );
        }
    }

    /// `count` yields undef when the select returns no aggregate row.
    #[test]
    fn count_is_optional() {
        for id in ["handle.count", "conn.count"] {
            let case = case_by_id(id);
            assert_eq!(case.return_class, QuickOrmReturnClass::BooleanOrCount);
            assert_eq!(
                case.multiplicity,
                QuickOrmMultiplicity::ZeroOrOne,
                "`{id}` returns undef when no row arrives"
            );
        }
    }

    /// A proxy that tail-calls a void-croaking refiner inherits the croak,
    /// because Perl propagates the caller's context through the final
    /// expression of the sub.
    #[test]
    fn tail_call_proxies_inherit_the_void_context_croak() {
        for (proxy, target) in [
            ("conn.async", "handle.async"),
            ("conn.aside", "handle.aside"),
            ("conn.forked", "handle.forked"),
        ] {
            assert_eq!(
                case_by_id(target).void_context,
                QuickOrmVoidContext::Croaks,
                "{target} is the refiner that croaks"
            );
            assert_eq!(
                case_by_id(proxy).void_context,
                QuickOrmVoidContext::Croaks,
                "`{proxy}` tail-calls `{target}`, so the croak reaches the caller"
            );
        }
    }

    /// Every method that forwards trailing arguments into `Handle::handle` can
    /// be rebound by them, so each must carry both cohorts rather than claiming
    /// one unconditional effect.
    #[test]
    fn handle_forwarding_methods_carry_both_cohorts() {
        // Derived, not guessed. At the pinned commit the complete set of methods
        // that forward a caller-supplied argument list into `Handle::handle` is
        // enumerated by, from the upstream tree:
        //
        //     grep -rn -- '->handle(@_)' lib/DBIx/QuickORM/
        //
        // On a Handle receiver that yields exactly Handle.pm:761 (`new`), 763
        // (`clone`), 1900 (`by_id`) and 1950 (`vivify`), plus Role/Row.pm:123
        // (`handle`) on a Row; `Handle::handle` itself is the forwardee. The
        // Connection.pm forwarders are excluded deliberately: a Connection has
        // no bound source to preserve, so those rows are unconditionally
        // `DerivedFromArgumentSource` rather than split.
        //
        // `by_ids` is *not* a forwarder: it does `my $self = shift` and maps
        // `by_id($_)` over the remaining arguments (Handle.pm:1938-1943), so
        // each inner call forwards an empty list and cannot rebind.
        for (pkg, method) in [
            (PKG_HANDLE, "by_id"),
            (PKG_HANDLE, "clone"),
            (PKG_HANDLE, "handle"),
            (PKG_HANDLE, "new"),
            (PKG_HANDLE, "vivify"),
            (PKG_ROW, "handle"),
        ] {
            let cohorts: BTreeSet<QuickOrmArgumentCohort> =
                quickorm_api_cases_for_method(pkg, method).map(|c| c.arguments).collect();
            assert!(
                cohorts.contains(&QuickOrmArgumentCohort::NoSourceArgument)
                    && cohorts.contains(&QuickOrmArgumentCohort::SourceOrRowRebinding),
                "`{pkg}::{method}` forwards into Handle::handle and needs both forms"
            );
        }

        // The rebinding half of every split must actually say the type comes
        // from the argument; carrying the cohort while still claiming to
        // preserve the receiver is the exact defect this seam exists to catch.
        for id in [
            "handle.by_id.rebind",
            "handle.clone.rebind",
            "handle.handle.rebind",
            "handle.new.rebind",
            "handle.vivify.rebind",
            "row.handle.rebind",
        ] {
            assert_eq!(
                case_by_id(id).type_params,
                QuickOrmTypeParamEffect::DerivedFromArgumentSource,
                "`{id}` rebinds the source, so it cannot preserve the receiver's type"
            );
        }

        // ...and the preserving half must not claim to derive from an argument.
        for id in [
            "handle.by_id.copy",
            "handle.clone.copy",
            "handle.handle.copy",
            "handle.new.copy",
            "handle.vivify.copy",
            "row.handle.copy",
        ] {
            assert_eq!(
                case_by_id(id).type_params,
                QuickOrmTypeParamEffect::PreservedFromReceiver,
                "`{id}` forwards an empty list, so it copies the receiver's type"
            );
        }
    }

    /// A `Row` never carries a handle execution mode. It builds its own handle
    /// through the connection, and `Connection::handle` yields a synchronous
    /// handle — proved by `Connection::async` having to call `->async` on that
    /// handle explicitly (Connection.pm:992). So no row method can admit an
    /// async form or return an async placeholder.
    #[test]
    fn row_methods_expose_no_execution_mode() {
        for case in quickorm_api_cases_for_receiver(QuickOrmReceiver::Row) {
            assert_eq!(
                case.mode,
                QuickOrmModeSupport::NotApplicable,
                "`{}` has a Row receiver, which exposes no mode selector",
                case.api_case_id
            );
        }
    }

    #[test]
    fn lookup_helpers_agree_with_the_table() {
        assert_eq!(quickorm_api_cases().len(), QUICKORM_API_CASES.len());
        assert!(quickorm_api_case("handle.one").is_some());
        assert!(quickorm_api_case("handle.does_not_exist").is_none());

        let handle_rows = quickorm_api_cases_for_receiver(QuickOrmReceiver::Handle).count();
        assert!(handle_rows > 0);

        let where_rows = quickorm_api_cases_for_method(PKG_HANDLE, "where").count();
        assert_eq!(where_rows, 2, "where has a getter and a setter form");
    }

    // ------------------------------------------------- write-result contracts

    /// A write that yields a row must keep that row's type identity. Upstream
    /// `_insert` returns `state_insert_row` (Handle.pm:2312-2323) or a
    /// `Row::Async` placeholder (Handle.pm:2305), and `vivify` returns
    /// `state_vivify_row` (Handle.pm:1954).
    #[test]
    fn row_producing_writes_keep_row_identity() {
        for id in [
            "handle.insert",
            "handle.insert_and_refresh",
            "handle.upsert",
            "handle.upsert_and_refresh",
            "handle.vivify.copy",
            "handle.vivify.rebind",
            "conn.insert",
            "conn.vivify",
            "conn.update_or_insert",
            "conn.find_or_insert",
        ] {
            let case = case_by_id(id);
            assert!(
                case.return_class.may_carry_row_identity(),
                "`{id}` returns a row upstream and must not be modeled as a bare write"
            );
            assert_ne!(
                case.type_params,
                QuickOrmTypeParamEffect::NotApplicable,
                "`{id}` yields a row, so its source parameter must be recorded"
            );
        }
    }

    /// A write whose result is not a row must not claim row identity. Upstream
    /// `update`/`delete` return the statement handle on a non-sync handle and
    /// undef when synchronous; `cas` returns a CAS::Result.
    #[test]
    fn non_row_writes_do_not_claim_row_identity() {
        for id in [
            "handle.update",
            "handle.delete",
            "handle.cas",
            "conn.update",
            "conn.delete",
            "row.delete",
            "row.cas",
        ] {
            let case = case_by_id(id);
            assert!(
                !case.return_class.may_carry_row_identity(),
                "`{id}` does not return a row upstream"
            );
        }
    }

    /// `SyncOnly` means upstream croaks unless the handle is synchronous. Only
    /// three cases actually do; every write admits at least one non-sync mode,
    /// so a blanket `SyncOnly` on writes would be false.
    #[test]
    fn sync_only_is_confined_to_the_cases_that_croak() {
        let sync_only: BTreeSet<&str> = QUICKORM_API_CASES
            .iter()
            .filter(|c| c.mode == QuickOrmModeSupport::SyncOnly)
            .map(|c| c.api_case_id)
            .collect();
        let expected: BTreeSet<&str> = [
            "conn.all",
            "conn.count",
            "conn.iterate",
            "handle.all",
            "handle.count",
            "handle.iterate",
        ]
        .into_iter()
        .collect();
        assert_eq!(
            sync_only, expected,
            "SyncOnly must name exactly the cases that croak on a non-sync handle"
        );
    }

    /// `cas` croaks on a forked handle (Handle.pm:2741) but runs async/aside.
    #[test]
    fn cas_refuses_forked_handles_only() {
        assert_eq!(
            case_by_id("handle.cas").mode,
            QuickOrmModeSupport::SyncAsyncAsideOnly,
            "handle.cas admits async and aside but croaks on a forked handle"
        );
        // A row builds its own handle from the connection, so it exposes no
        // mode selector of its own.
        assert_eq!(case_by_id("row.cas").mode, QuickOrmModeSupport::NotApplicable);
    }

    /// Methods that hand back the receiving row must say so, or a consumer
    /// loses the row type across a chained call.
    #[test]
    fn receiver_returning_row_methods_preserve_type_params() {
        for id in ["row.check_sync", "row.force_sync", "row.discard", "row.update", "row.refresh"] {
            let case = case_by_id(id);
            assert!(
                case.return_class.may_carry_row_identity(),
                "`{id}` returns the receiving row upstream"
            );
            assert_eq!(
                case.type_params,
                QuickOrmTypeParamEffect::PreservedFromReceiver,
                "`{id}` must preserve the receiver's row type"
            );
            assert_eq!(case.multiplicity, QuickOrmMultiplicity::One);
        }
    }

    /// The three direct state slots are read without a guard, so they are undef
    /// when the slot is absent; the derived field maps are always built.
    #[test]
    fn optional_state_slots_are_distinguished_from_built_maps() {
        for id in ["row.stored_data", "row.pending_data", "row.desynced_data"] {
            assert_eq!(
                case_by_id(id).multiplicity,
                QuickOrmMultiplicity::OptionalHash,
                "`{id}` reads its slot directly and can be undef"
            );
        }
        for id in ["row.fields", "row.raw_fields", "row.stored_fields", "row.pending_fields"] {
            assert_eq!(
                case_by_id(id).multiplicity,
                QuickOrmMultiplicity::Hash,
                "`{id}` is built by `_fields`, which returns `\\%out` (Row.pm:591)"
            );
        }

        // A flat key/value list is not a hash container. Upstream returns a
        // bare list from `conflate_args` and `primary_key_hash`, while
        // `primary_key_hashref` wraps the same pairs. Their scalar-context
        // behavior differs by construct (last element vs. element count), so
        // each row records its own; the class does not assert one.
        for id in ["row.conflate_args", "row.primary_key_hash"] {
            assert_eq!(
                case_by_id(id).multiplicity,
                QuickOrmMultiplicity::KeyValueSequence,
                "`{id}` returns a flat list upstream, not a hash"
            );
        }
        assert_eq!(
            case_by_id("row.primary_key_hashref").multiplicity,
            QuickOrmMultiplicity::Hash,
            "the hashref form is the one that really is a container"
        );
    }

    // ------------------------------------------------- cross-cutting coherence

    /// Whole-table coherence. Per-row correctness rests on review against the
    /// cited upstream line, but these invariants hold for every row and catch
    /// whole classes of miscoding without re-reading upstream.
    #[test]
    fn every_row_is_internally_coherent() {
        for case in QUICKORM_API_CASES {
            let id = case.api_case_id;

            if case.return_class.may_carry_row_identity() {
                assert_ne!(
                    case.type_params,
                    QuickOrmTypeParamEffect::NotApplicable,
                    "`{id}` carries row identity, so its type parameter must be recorded"
                );
                assert_ne!(
                    case.multiplicity,
                    QuickOrmMultiplicity::Nothing,
                    "`{id}` carries row identity but yields nothing"
                );
            }

            // Hash multiplicities belong to the hash-returning class and nowhere else.
            let hashish = matches!(
                case.multiplicity,
                QuickOrmMultiplicity::Hash
                    | QuickOrmMultiplicity::OptionalHash
                    | QuickOrmMultiplicity::KeyValueSequence
            );
            assert_eq!(
                hashish,
                case.return_class == QuickOrmReturnClass::OpenHashOrHashSequence,
                "`{id}` disagrees about being a hash result"
            );

            // Only handle-shaped results may claim a join transform.
            if case.type_params == QuickOrmTypeParamEffect::TransformedToJoinRow {
                assert_eq!(
                    case.return_class,
                    QuickOrmReturnClass::TransformHandleSourceRow,
                    "`{id}` claims a join transform without being a handle transform"
                );
            }

            // An unsupported case must not also assert a resolved shape.
            if case.return_class == QuickOrmReturnClass::UnsupportedDynamicVariant {
                assert_eq!(case.boundary, QuickOrmBoundary::UnsupportedAtPinnedVersion);
                assert_eq!(case.type_params, QuickOrmTypeParamEffect::NotApplicable);
            }

            // A preserving handle result must genuinely preserve.
            if case.return_class == QuickOrmReturnClass::PreserveHandleSourceRow {
                assert_eq!(
                    case.type_params,
                    QuickOrmTypeParamEffect::PreservedFromReceiver,
                    "`{id}` claims to preserve but records another effect"
                );
                assert_eq!(case.multiplicity, QuickOrmMultiplicity::One);
            }

            // A zero-argument getter cannot return a refined clone.
            if case.arguments == QuickOrmArgumentCohort::ZeroArgGetter {
                assert!(
                    !matches!(
                        case.return_class,
                        QuickOrmReturnClass::PreserveHandleSourceRow
                            | QuickOrmReturnClass::TransformHandleSourceRow
                    ),
                    "`{id}` is the reading form and must not return a handle"
                );
            }
        }
    }
}
