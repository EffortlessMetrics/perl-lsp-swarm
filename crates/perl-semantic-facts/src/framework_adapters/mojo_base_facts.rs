//! Registry-activated Mojo::Base object-fact minting (#9682).
//!
//! This module turns source-extracted `has` declarations plus the #9681
//! checked activation/profile into the canonical object facts of a
//! `Mojo::Base` class: generated accessor members, the distinct reader and
//! fluent-setter callable-result relations, and the literal parent
//! relationship. It is the facts side of the #9682 producer leaf and builds
//! directly on the #9681 activation seam:
//!
//! - object facts mint **only through the registry-activated adapter**: a
//!   detected framework and an exact activation are both required. A
//!   same-named `has` call in a package without an exact activation is a hard
//!   negative and mints nothing — this module never treats `has` itself as
//!   activation evidence;
//! - every generated member points at the **real `has` declaration anchor**
//!   and never receives a fabricated method body. Members carry
//!   `Provenance::FrameworkSynthesis` / `Confidence::Medium` and the
//!   `SemanticReasonCode::GeneratedFromSource` reason, matching the accepted
//!   source-backed-generated provenance rules (PLSP-SPEC-0017 / -0024): a
//!   framework-generated accessor is source-backed by its generator, but it is
//!   never relabelled as explicit source;
//! - the fluent setter uses the neutral
//!   [`CallableResultRelation::ReceiverSelf`] vocabulary (#10904) rather than
//!   a Mojo-specific return-self shape, and carries no fabricated exit
//!   contributor: a generated accessor has a generator anchor, not a real
//!   return statement, so its callable-result facts stay
//!   [`CallableResultLimitation::GeneratedNoSource`];
//! - the reader result and the setter return-self relation are **two distinct
//!   facts about one member entity**. Default-value uncertainty limits the
//!   reader without erasing the independently determinate `ReceiverSelf`
//!   relation;
//! - the literal parent proposition reuses the canonical [`PackageEdge`] /
//!   [`PackageEdgeKind::Inherits`] vocabulary. This leaf produces the
//!   proposition a canonical relationship representation consumes; it does not
//!   claim `PackageEdge` is the final ProjectModel authority (that is the
//!   typed-relationship issue's to own) and it builds no Mojo-only hierarchy;
//! - facts are generation-owned and shadow receipts: every envelope carries
//!   the activation's source generation plus invalidation dependencies over
//!   the owning file and the activating `Mojo::Base` module, and the adapter
//!   disposition remains `Shadow`, so no provider surface can publish them
//!   (canonical publication is owned separately).
//!
//! Accessor semantics follow the reviewed `Mojo::Base` profile. `attr` binds
//! `($self, $attrs, $value, %kv)`: a name (or an array reference of names),
//! one optional default, and optional trailing key/value options — the corpus
//! spells `has app => undef, weak => 1;`. Every generated accessor is
//! read-write and a write returns the invocant, so an option this profile does
//! not model limits the *read* result without disturbing the accessor identity
//! or the write contract. A `sub { ... }` default is a lazy builder rather
//! than a code-reference value.

use crate::envelope::{
    CallableResultCompleteness, CallableResultFact, CallableResultLimitation,
    CallableResultRelation, SemanticFactEnvelope,
};
use crate::framework::AdapterDetectionResult;
use crate::framework_adapters::mojo_base::{
    MOJO_BASE_FRAMEWORK_NAME, MojoBaseActivationFacts, MojoBaseActivationOutcome,
};
use crate::{
    AnchorId, BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, EntityId, FactId,
    FileId, GeneratedMember, GeneratedMemberKind, InvalidationDependency, LifecyclePhase,
    PackageEdge, PackageEdgeKind, Provenance, SemanticConfidence, SemanticFactKind,
    SemanticFreshness, SemanticProducer, SemanticProvenance, SemanticReasonCode, SourceAnchor,
    SourceGeneration, ValueShape,
};

/// Attribute name selection of one source-extracted `has` declaration.
///
/// The name decides whether a member can be generated at all: only a literal
/// spelling names a method. Computed and malformed spellings stay explicit
/// typed boundaries and never become a guessed accessor name.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MojoBaseAttributeName {
    /// A source-literal attribute name (quoted, bareword, or `qw` word).
    Literal(String),
    /// A computed name expression — an explicit dynamic boundary.
    Dynamic {
        /// Bounded dynamic explanation.
        reason: String,
    },
    /// A `has` form the reviewed profile cannot interpret as one name.
    Malformed {
        /// Bounded malformed explanation.
        reason: String,
    },
}

impl MojoBaseAttributeName {
    /// The literal spelling, when this selection names a method.
    #[must_use]
    pub fn literal(&self) -> Option<&str> {
        match self {
            Self::Literal(name) => Some(name.as_str()),
            Self::Dynamic { .. } | Self::Malformed { .. } => None,
        }
    }
}

/// Default-value evidence of one source-extracted `has` declaration.
///
/// `Mojo::Base` admits two default *values*: a constant, and a code reference
/// invoked lazily to build the value. Any other reference croaks at runtime,
/// so it is an explicit unsupported boundary rather than a guessed value.
///
/// This describes the default operand only. `Mojo::Base::attr` binds
/// `($self, $attrs, $value, %kv)`, so a declaration may also carry trailing
/// option pairs after the default; those are recorded separately on
/// [`MojoBaseAttributeDeclaration::unmodeled_options`] and do not make the
/// default unsupported.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MojoBaseAttributeDefault {
    /// No default operand: the attribute reads as undefined until written.
    Absent,
    /// A source-literal constant default (string or number).
    Constant,
    /// A `sub { ... }` lazy builder called to build the default value.
    LazyBuilder,
    /// A computed default expression — an explicit dynamic boundary.
    Dynamic {
        /// Bounded dynamic explanation.
        reason: String,
    },
    /// A default form the reviewed profile does not admit (`Mojo::Base`
    /// rejects a non-code reference default at runtime).
    Unsupported {
        /// Bounded unsupported explanation.
        reason: String,
    },
}

/// Whether an explicit source method of the same name exists in the owning
/// package.
///
/// The collision is preserved as conflict evidence rather than silently
/// deleting either side, and the runtime outcome is **determinate rather than
/// order-dependent**: `Mojo::Base::attr` installs the generated accessor with
/// an unconditional `monkey_patch($class, $attr, $sub)` — no check for an
/// existing method — and Perl installs `sub name { ... }` at compile time
/// while the top-level `has` runs afterwards. The generated accessor therefore
/// overwrites the explicit method whichever order they appear in.
///
/// So the two sides carry different kinds of authority, and neither subsumes
/// the other:
///
/// - the **explicit method** has stronger *source* evidence — a real body, a
///   real span, exact provenance;
/// - the **generated accessor** is the *live* method after class
///   initialization.
///
/// A consumer that reports "definition" should weigh the first; one that
/// answers "what does calling this do" should weigh the second. The shadowing
/// is itself worth surfacing — an explicit method silently replaced by an
/// accessor is usually a mistake in the source, not an intent.
///
/// Scope of that determinacy: a **run-phase** `has` in the same file as the
/// `sub`. It runs after the whole file is compiled, so the `sub` already
/// exists and `monkey_patch` overwrites it unconditionally — source order
/// between the two does not matter.
///
/// A compile-phase `has` (inside `BEGIN`, `UNITCHECK`, `CHECK` or `INIT`)
/// interleaves with subroutine compilation instead, so whichever comes later
/// in source wins and the accessor can itself be overwritten. That winner is
/// reported as undetermined rather than guessed; see
/// [`MojoBaseExecutionPhase`]. A method installed at runtime after the `has`
/// executes remains outside the reviewed profile.
/// When a declaration's `has` call executes, relative to compilation.
///
/// Perl runs `use` inside an implicit `BEGIN`, so the whole file is compiled —
/// and every `use` in it imported — before any ordinary statement runs. That
/// makes execution phase, not source position, the thing that decides whether
/// a declaration saw the imported `has` and whether it survives a same-named
/// explicit `sub`. Both questions have opposite answers for the two phases, so
/// the phase travels with the carrier rather than being re-guessed at minting.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MojoBaseExecutionPhase {
    /// An ordinary statement, which runs after the whole file is compiled.
    ///
    /// Every `use` in the file has already imported by then, so such a
    /// declaration reaches the imported `has` regardless of whether it is
    /// written above or below the import.
    Run,
    /// A statement inside an early phaser (`BEGIN`, `UNITCHECK`, `CHECK`,
    /// `INIT`), which runs during compilation.
    ///
    /// Source order against the activating import therefore does matter: a
    /// phaser above the import runs before `has` exists.
    Compile,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MojoBaseExplicitMethodState {
    /// No same-named explicit method was found in the owning package.
    None,
    /// A same-named explicit `sub` is declared in the owning package. The
    /// generated accessor overwrites it at class initialization; the explicit
    /// declaration keeps the stronger source evidence.
    Collides,
}

/// One source-extracted `Mojo::Base` `has` attribute declaration.
///
/// Extraction is pure source observation performed by the semantic analyzer:
/// it knows the reviewed `has` grammar, it does not decide activation. An
/// object fact exists only after this module minted it over an exact
/// activation.
///
/// `has [qw(host port)];` contributes one declaration per name, all sharing
/// the statement's `declaration_index` and separated by `name_index`.
///
/// Like the Dancer2 declaration carriers, this input struct stays
/// exhaustively constructible so the extracting analyzer crate can build it
/// directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MojoBaseAttributeDeclaration {
    /// Source-order index of the `has` statement within its file.
    pub declaration_index: u32,
    /// Position of this name within the statement's name operand (`0` for a
    /// single name, the list position for an array-reference name list).
    pub name_index: u32,
    /// File the declaration appears in.
    pub file_id: FileId,
    /// Caller package at the declaration (activation scope).
    pub package: Option<String>,
    /// Anchor of the whole `has` statement — the generator anchor every
    /// generated member points at.
    pub declaration_anchor: SourceAnchor,
    /// Anchor of the name operand itself.
    pub name_anchor: SourceAnchor,
    /// Attribute name selection.
    pub name: MojoBaseAttributeName,
    /// Default-value evidence.
    pub default: MojoBaseAttributeDefault,
    /// Same-named explicit source method state in the owning package.
    pub explicit_method: MojoBaseExplicitMethodState,
    /// When this declaration's `has` call executes, relative to compilation.
    pub execution_phase: MojoBaseExecutionPhase,
    /// Option keys supplied after the default that the reviewed profile does
    /// not model.
    ///
    /// `Mojo::Base::attr` binds `($self, $attrs, $value, %kv)`, so a
    /// declaration may carry trailing key/value options — the corpus spells
    /// `has app => undef, weak => 1;`. The reviewed profile models the name
    /// and the default; every option key is recorded here rather than
    /// silently ignored or mistaken for extra operands, and it limits the
    /// reader result without disturbing the accessor identity or the write
    /// contract, both of which `Mojo::Base` still generates normally.
    pub unmodeled_options: Vec<String>,
    /// Source generation this declaration was extracted from.
    ///
    /// Load-bearing: minting refuses a declaration whose generation differs
    /// from the activation's, so a carrier extracted from an older parse
    /// cannot be restamped with a current generation and reappear as a fresh
    /// fact after the attribute was renamed or removed.
    pub source_generation: SourceGeneration,
}

/// One minted generated-accessor member fact.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MojoBaseGeneratedMemberFact {
    /// Shared semantic identity, generation, proof, and invalidation data.
    pub envelope: SemanticFactEnvelope,
    /// Canonical generated-member payload anchored at the real `has`
    /// declaration.
    pub member: GeneratedMember,
    /// Same-named explicit source method state preserved as conflict
    /// evidence.
    pub explicit_method: MojoBaseExplicitMethodState,
}

/// One minted literal-parent relationship fact.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MojoBaseParentFact {
    /// Shared semantic identity, generation, proof, and invalidation data.
    pub envelope: SemanticFactEnvelope,
    /// Canonical inheritance edge payload.
    pub edge: PackageEdge,
}

/// Canonical `Mojo::Base` object facts minted for one activating package.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MojoBaseObjectFacts {
    /// Generated accessor members, in source order.
    pub members: Vec<MojoBaseGeneratedMemberFact>,
    /// Reader-result relation, one per minted member, in the same order.
    pub reader_results: Vec<CallableResultFact>,
    /// Fluent setter return-self relation, one per minted member, in the same
    /// order.
    pub setter_results: Vec<CallableResultFact>,
    /// Literal parent relationship established by the activating import.
    pub parents: Vec<MojoBaseParentFact>,
}

impl MojoBaseObjectFacts {
    /// Whether no object fact of any kind was minted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
            && self.reader_results.is_empty()
            && self.setter_results.is_empty()
            && self.parents.is_empty()
    }
}

/// Domain separator keeping generated-member identities disjoint from every
/// other fact family minted over the same (file, declaration order,
/// generation).
const MOJO_MEMBER_IDENTITY_DOMAIN: u64 = 0x4D4F_4A4F_4D45_4D00;

/// Domain separator for the reader-result fact of one generated member.
const MOJO_READER_IDENTITY_DOMAIN: u64 = 0x4D4F_4A4F_5245_4144;

/// Domain separator for the setter return-self fact of one generated member.
const MOJO_SETTER_IDENTITY_DOMAIN: u64 = 0x4D4F_4A4F_5345_5401;

/// Domain separator for the literal-parent relationship fact.
const MOJO_PARENT_IDENTITY_DOMAIN: u64 = 0x4D4F_4A4F_5041_5201;

/// Deterministic generated-member identity for one
/// (file, `has` statement, name position, generation).
///
/// Identity derives from the owning file, the source declaration order, the
/// name position inside the statement, and the minting generation under a
/// Mojo-specific domain separator, so member facts never collide with the
/// route or hook families minted over the same file/order/generation.
#[must_use]
pub fn mojo_base_member_identity(
    file_id: FileId,
    declaration_index: u32,
    name_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let generation_digest = match generation {
        // FNV-1a accumulation: order- and repetition-sensitive, so distinct
        // generation identities never collide.
        SourceGeneration::Known(value) => {
            value.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x1000_0000_01b3)
            })
        }
        // An unknown generation is still a distinct minting context; the
        // envelope degrades it separately.
        SourceGeneration::Unknown => 0x1a2b_3c4d_5e6f_7081_u64,
    };
    let file = file_id.0.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let index = u64::from(declaration_index).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    let name = u64::from(name_index).wrapping_mul(0x87C3_7B29_11C5_21D3_u64);
    let fact = file ^ index ^ name ^ generation_digest ^ MOJO_MEMBER_IDENTITY_DOMAIN;
    (FactId(fact), EntityId(fact.wrapping_add(1)))
}

/// Deterministic reader-result identity for one generated member.
///
/// Shares the member's entity — the reader relation is a fact *about* the
/// generated accessor — while using its own fact-identity domain so it can
/// never collide with the member fact or the setter relation.
#[must_use]
pub fn mojo_base_reader_identity(
    file_id: FileId,
    declaration_index: u32,
    name_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let (fact_id, entity_id) =
        mojo_base_member_identity(file_id, declaration_index, name_index, generation);
    (FactId(fact_id.0 ^ MOJO_READER_IDENTITY_DOMAIN), entity_id)
}

/// Deterministic setter return-self identity for one generated member.
///
/// Same entity as the reader relation and its own fact-identity domain: one
/// accessor entity carries two semantically distinct callable-result facts.
#[must_use]
pub fn mojo_base_setter_identity(
    file_id: FileId,
    declaration_index: u32,
    name_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let (fact_id, entity_id) =
        mojo_base_member_identity(file_id, declaration_index, name_index, generation);
    (FactId(fact_id.0 ^ MOJO_SETTER_IDENTITY_DOMAIN), entity_id)
}

/// Deterministic literal-parent relationship identity for one activation
/// site.
///
/// A parent relationship belongs to the activating import, not to a `has`
/// statement, so there is no declaration order to key it by. The import's
/// start byte is passed through [`mojo_base_member_identity`]'s
/// `declaration_index` slot instead — deliberately, because that slot is just
/// a distinguishing integer inside the mix. Reusing it is safe only because
/// the parent domain separator below is XORed in afterwards: without it, a
/// `has` statement whose declaration index happened to equal the import's
/// start byte would collide with this fact. The separator, not the slot
/// value, is what keeps the families disjoint — see
/// `parent_identity_never_collides_with_a_member_family_identity`.
#[must_use]
pub fn mojo_base_parent_identity(
    file_id: FileId,
    import_start_byte: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let (fact_id, _) = mojo_base_member_identity(file_id, import_start_byte, 0, generation);
    let fact = FactId(fact_id.0 ^ MOJO_PARENT_IDENTITY_DOMAIN);
    (fact, EntityId(fact.0.wrapping_add(1)))
}

/// Mint the canonical `Mojo::Base` object facts for one activating package.
///
/// Returns no facts unless `detection` established the framework and
/// `activation` is exact — the registry-activated adapter contract. A
/// declaration of another package is skipped, and a declaration whose name is
/// computed or malformed mints no member (the typed boundary stays on the
/// declaration; this leaf never guesses an accessor name).
///
/// `file_id` owns the activating import — the activation profile carries the
/// import's source interval but not its file, and the parent relationship is
/// anchored in that file.
///
/// `package` must be the package that made the activation. It is checked
/// against `activation.package` and the whole call fails closed on a
/// mismatch, so a caller cannot borrow one package's activation to establish
/// another package's accessors or inheritance.
///
/// A declaration must then belong to that activation on **four** counts,
/// because the carriers arrive as a plain slice a caller could mismatch
/// against the activation:
///
/// - the same owning package;
/// - the same file as the activating import, so a same-named package in
///   another file cannot contribute accessors to this activation;
/// - the same source generation, so a carrier extracted from an older parse
///   cannot be restamped with the current generation and resurrect an accessor
///   that has since been renamed or removed;
/// - a position **after** the activating import, because `Mojo::Base` installs
///   `has` at import time. A `has` call earlier in the package is a different
///   function — the reviewed profile has no accessor to generate from it.
///
/// Every minted fact carries the activation's source generation plus
/// invalidation dependencies over the owning source file and the `Mojo::Base`
/// module, so an accessor, import, or module edit invalidates the dependent
/// projections in the current generation.
#[must_use]
pub fn mojo_base_object_facts(
    detection: &AdapterDetectionResult,
    activation: &MojoBaseActivationFacts,
    file_id: FileId,
    package: Option<&str>,
    declarations: &[MojoBaseAttributeDeclaration],
) -> MojoBaseObjectFacts {
    let mut facts = MojoBaseObjectFacts::default();
    if !detection.is_detected() || !activation.is_exact() {
        return facts;
    }
    // The activation already knows which package imported `Mojo::Base`. Asking
    // it for a different package's members must fail closed rather than trust
    // the caller: an activation `App` made establishes neither `Other`'s
    // accessors nor an `Other inherits Mojo::Base` edge.
    if activation.package.as_deref() != package {
        return facts;
    }
    let generation = &activation.source_generation;
    let (_, import_end_byte) = activation.source_interval;

    for declaration in declarations {
        if declaration.package.as_deref() != package {
            continue;
        }
        // A same-named package in another file is a different class: its `has`
        // calls are not this activation's accessors.
        if declaration.file_id != file_id {
            continue;
        }
        // A carrier from an older parse cannot be relabelled current.
        if declaration.source_generation != *generation {
            continue;
        }
        // Source order against the import only decides anything for a
        // declaration that runs during compilation.
        //
        // Perl runs `use` inside an implicit `BEGIN`, so the import completes
        // while the file is still being compiled — before any ordinary
        // statement executes. An ordinary `has` written above the import
        // therefore still calls the imported `has` when it finally runs, and
        // rejecting it on byte order would drop a real accessor. (In the
        // reviewed paren-less spelling that arrangement does not compile at
        // all — `has 'x';` above the import is a syntax error, since `has` is
        // not predeclared there — so this admits the parenthesized spelling
        // and costs nothing on the paren-less one.)
        //
        // A declaration inside an early phaser is the case where order does
        // decide: a phaser above the activating import runs before `has`
        // exists.
        if declaration.execution_phase == MojoBaseExecutionPhase::Compile
            && declaration.declaration_anchor.start_byte < import_end_byte
        {
            continue;
        }
        // Only a literal spelling names a method. A computed or malformed
        // name keeps its typed boundary on the declaration and mints nothing:
        // a guessed accessor name would be a fabricated member.
        let Some(name) = declaration.name.literal() else {
            continue;
        };
        let Some(owning_package) = package else {
            // An activation without an owning package cannot own a member:
            // the generated-member payload requires a real package identity.
            continue;
        };
        facts.members.push(mint_member_fact(declaration, name, owning_package, generation));
        facts.reader_results.push(mint_reader_fact(declaration, owning_package, generation));
        facts.setter_results.push(mint_setter_fact(declaration, owning_package, generation));
    }

    if let Some(parent) = activation_parent(activation) {
        facts.parents.push(mint_parent_fact(activation, file_id, package, parent, generation));
    }
    facts
}

/// Parent package established by an exact activation.
///
/// `use Mojo::Base -base;` makes the caller inherit from `Mojo::Base` itself;
/// `use Mojo::Base 'Parent';` inherits from the literal spelling. Every other
/// outcome — including a dynamic or unmodeled parent — establishes no
/// relationship.
fn activation_parent(activation: &MojoBaseActivationFacts) -> Option<&str> {
    match &activation.outcome {
        MojoBaseActivationOutcome::ExactBaseActivation => Some(MOJO_BASE_FRAMEWORK_NAME),
        MojoBaseActivationOutcome::ExactLiteralParentActivation { parent } => Some(parent.as_str()),
        _ => None,
    }
}

/// Invalidation dependencies shared by every fact of one activation: the
/// owning source file and the activating `Mojo::Base` module.
fn dependencies(file_id: FileId, generation: &SourceGeneration) -> Vec<InvalidationDependency> {
    vec![
        InvalidationDependency::new(format!("file:{}", file_id.0), generation.clone()),
        InvalidationDependency::new(
            format!("module:{MOJO_BASE_FRAMEWORK_NAME}"),
            generation.clone(),
        ),
    ]
}

/// Build the canonical envelope for one minted source-backed-generated fact.
///
/// Producer `FrameworkAdapter` with `FrameworkSynthesis` / `Medium` and the
/// `GeneratedFromSource` reason: a generated accessor is source-backed by its
/// `has` generator, but it is never explicit source. This deliberately does
/// not reuse the route family's exact-source envelope.
fn generated_envelope(
    kind: SemanticFactKind,
    fact_id: FactId,
    entity_id: EntityId,
    package: &str,
    anchor: SourceAnchor,
    generation: &SourceGeneration,
    file_id: FileId,
    boundary: Option<BoundaryLink>,
) -> SemanticFactEnvelope {
    SemanticFactEnvelope::new(
        fact_id,
        Some(entity_id),
        kind,
        anchor,
        generation.clone(),
        None,
        Some(package.to_string()),
        LifecyclePhase::Runtime,
        SemanticProducer::FrameworkAdapter,
        SemanticProvenance::Known(Provenance::FrameworkSynthesis),
        SemanticConfidence::Known(Confidence::Medium),
        SemanticFreshness::Fresh,
        boundary,
        dependencies(file_id, generation),
        SemanticReasonCode::GeneratedFromSource,
    )
}

fn mint_member_fact(
    declaration: &MojoBaseAttributeDeclaration,
    name: &str,
    package: &str,
    generation: &SourceGeneration,
) -> MojoBaseGeneratedMemberFact {
    let (fact_id, entity_id) = mojo_base_member_identity(
        declaration.file_id,
        declaration.declaration_index,
        declaration.name_index,
        generation,
    );
    // The generator anchor is the real `has` statement: a generated member has
    // no body of its own, so it never receives a fabricated source interval.
    let source_anchor_id = declaration
        .declaration_anchor
        .anchor_id
        .unwrap_or(AnchorId(u64::from(declaration.declaration_anchor.start_byte)));
    // Which of a colliding pair is live after initialization depends on the
    // declaration's phase, so the two cases cannot share one boundary.
    //
    // A run-phase `has` executes after the whole file is compiled, so every
    // explicit `sub` in the file already exists and `monkey_patch` overwrites
    // it unconditionally: the accessor is determinately live. The boundary
    // records only that it shadows a declaration which keeps the stronger
    // source evidence — a conflict a consumer should see rather than one
    // silently resolved here.
    //
    // A compile-phase `has` interleaves with subroutine compilation instead,
    // so whichever comes later in source wins, and a phaser can be overwritten
    // by a `sub` below it. Resolving that would need the explicit method's own
    // position and phase, which this carrier does not hold, so the winner is
    // reported as undetermined rather than guessed. Claiming the accessor is
    // live here would be exactly the overclaim this producer refuses
    // elsewhere.
    let boundary = match (declaration.explicit_method, declaration.execution_phase) {
        (MojoBaseExplicitMethodState::None, _) => None,
        (MojoBaseExplicitMethodState::Collides, MojoBaseExecutionPhase::Run) => {
            Some(BoundaryLink::new(
                None,
                BoundaryKind::Compatibility,
                BoundaryDisposition::Degrade,
                SemanticReasonCode::CompatibilityBoundary,
            ))
        }
        (MojoBaseExplicitMethodState::Collides, MojoBaseExecutionPhase::Compile) => {
            Some(BoundaryLink::new(
                None,
                BoundaryKind::CompileTimeExecution,
                BoundaryDisposition::Degrade,
                SemanticReasonCode::UnsupportedEffect,
            ))
        }
    };
    MojoBaseGeneratedMemberFact {
        envelope: generated_envelope(
            SemanticFactKind::Declaration,
            fact_id,
            entity_id,
            package,
            declaration.declaration_anchor,
            generation,
            declaration.file_id,
            boundary,
        ),
        member: GeneratedMember::new(
            entity_id,
            name.to_string(),
            // Every `Mojo::Base` accessor is read-write: one method both
            // reads and writes the attribute.
            GeneratedMemberKind::Accessor,
            source_anchor_id,
            package.to_string(),
            Provenance::FrameworkSynthesis,
            Confidence::Medium,
        ),
        explicit_method: declaration.explicit_method,
    }
}

/// Reader-result relation implied by the declaration's default evidence.
///
/// Only a source-literal constant default proves a value shape, and it proves
/// it for the *default* alone: a writable accessor can store any value, so the
/// relation is never a complete exit denominator. Every other default form
/// leaves the read result unknown.
fn reader_relation(
    default: &MojoBaseAttributeDefault,
) -> (CallableResultRelation, Vec<CallableResultLimitation>, BoundaryKind) {
    match default {
        MojoBaseAttributeDefault::Constant => (
            CallableResultRelation::Concrete(ValueShape::Scalar),
            vec![
                CallableResultLimitation::GeneratedNoSource,
                CallableResultLimitation::DynamicValue,
            ],
            // The default's shape is proven; what stays open is the stored
            // value a later write may put there — a dynamic value, not an
            // unsupported form.
            BoundaryKind::DynamicValue,
        ),
        // A lazy builder's value is whatever its body returns; this leaf does
        // not evaluate default expressions.
        MojoBaseAttributeDefault::LazyBuilder | MojoBaseAttributeDefault::Absent => (
            CallableResultRelation::Unknown,
            vec![
                CallableResultLimitation::GeneratedNoSource,
                CallableResultLimitation::DynamicValue,
            ],
            BoundaryKind::DynamicValue,
        ),
        MojoBaseAttributeDefault::Dynamic { .. } => (
            CallableResultRelation::Unknown,
            vec![
                CallableResultLimitation::GeneratedNoSource,
                CallableResultLimitation::DynamicValue,
            ],
            BoundaryKind::DynamicValue,
        ),
        // The only genuinely unsupported case: `Mojo::Base` rejects this
        // default at runtime, so the reviewed profile models no result at all.
        MojoBaseAttributeDefault::Unsupported { .. } => (
            CallableResultRelation::Unknown,
            vec![
                CallableResultLimitation::GeneratedNoSource,
                CallableResultLimitation::Unsupported,
            ],
            BoundaryKind::Unsupported,
        ),
    }
}

fn mint_reader_fact(
    declaration: &MojoBaseAttributeDeclaration,
    package: &str,
    generation: &SourceGeneration,
) -> CallableResultFact {
    let (fact_id, entity_id) = mojo_base_reader_identity(
        declaration.file_id,
        declaration.declaration_index,
        declaration.name_index,
        generation,
    );
    let (mut relation, mut limitations, mut boundary_kind) = reader_relation(&declaration.default);
    if !declaration.unmodeled_options.is_empty() {
        // `Mojo::Base` accepts the options (`weak => 1` and friends), so the
        // accessor and its write contract are unaffected — but an option this
        // profile does not model can change what a read yields, so the reader
        // cannot keep a default-derived shape.
        relation = CallableResultRelation::Unknown;
        limitations.push(CallableResultLimitation::Unsupported);
        boundary_kind = BoundaryKind::Unsupported;
    }
    CallableResultFact::new(
        generated_envelope(
            SemanticFactKind::CallableResult,
            fact_id,
            entity_id,
            package,
            declaration.declaration_anchor,
            generation,
            declaration.file_id,
            // The reader's certainty really is limited: what the accessor
            // returns depends on values this leaf cannot enumerate. The kind
            // names *which* limit applies, so a consumer that filters on
            // `Unsupported` sees only genuinely unsupported declarations.
            Some(BoundaryLink::new(
                None,
                boundary_kind,
                BoundaryDisposition::Degrade,
                SemanticReasonCode::GeneratedFromSource,
            )),
        ),
        relation,
        // No exit contributor is fabricated: a generated accessor has a
        // generator declaration anchor, not a real return statement.
        Vec::new(),
        CallableResultCompleteness::Partial,
        limitations,
    )
}

fn mint_setter_fact(
    declaration: &MojoBaseAttributeDeclaration,
    package: &str,
    generation: &SourceGeneration,
) -> CallableResultFact {
    let (fact_id, entity_id) = mojo_base_setter_identity(
        declaration.file_id,
        declaration.declaration_index,
        declaration.name_index,
        generation,
    );
    CallableResultFact::new(
        generated_envelope(
            SemanticFactKind::CallableResult,
            fact_id,
            entity_id,
            package,
            declaration.declaration_anchor,
            generation,
            declaration.file_id,
            // No boundary: unlike the reader, nothing about this relation is
            // dynamic or unsupported — `Mojo::Base` always returns the
            // invocant from a write. The fact is still not exact source (the
            // `GeneratedFromSource` reason code degrades it, and the
            // `GeneratedNoSource` limitation records that there is no method
            // body), but claiming a *boundary* here would mislabel a
            // determinate framework contract as unmodeled.
            None,
        ),
        // The framework contract is determinate regardless of default-value
        // uncertainty: a write returns the invocant. The relation stays
        // symbolic so no concrete package is hard-coded here.
        CallableResultRelation::ReceiverSelf,
        // Same rule as the reader: no fabricated exit contributor.
        Vec::new(),
        // Mojo::Base generates exactly one write exit, so the alternative set
        // is complete even though it is not source-anchored.
        CallableResultCompleteness::Complete,
        vec![CallableResultLimitation::GeneratedNoSource],
    )
}

fn mint_parent_fact(
    activation: &MojoBaseActivationFacts,
    file_id: FileId,
    package: Option<&str>,
    parent: &str,
    generation: &SourceGeneration,
) -> MojoBaseParentFact {
    let (import_start, import_end) = activation.source_interval;
    // The literal parent's own range when the site located it, otherwise the
    // activating import statement. A range is never fabricated.
    let (anchor_start, anchor_end) = activation.parent_range.unwrap_or((import_start, import_end));
    let (fact_id, entity_id) = mojo_base_parent_identity(file_id, import_start, generation);
    let anchor = SourceAnchor::new(
        Some(AnchorId(u64::from(anchor_start))),
        file_id,
        anchor_start,
        anchor_end,
    );
    let from_package = package.unwrap_or("main").to_string();
    // The two activation forms do not have the same evidence, so they must not
    // claim the same provenance.
    //
    // `use Mojo::Base 'Parent'` spells the parent in source: the edge repeats a
    // literal the reader can point at, anchored on that spelling's own range,
    // so it is exact.
    //
    // `use Mojo::Base -base` spells no parent at all. That the resulting
    // superclass is `Mojo::Base` is knowledge about the framework, not a
    // reading of this file, and the anchor falls back to the import statement
    // because there is no parent range to point at. Claiming `ExactAst` there
    // would assert a source backing that does not exist — the same overclaim
    // this producer refuses for generated accessors.
    let literal_parent = matches!(
        activation.outcome,
        MojoBaseActivationOutcome::ExactLiteralParentActivation { .. }
    );
    let (provenance, confidence, reason_code) = if literal_parent {
        (Provenance::ExactAst, Confidence::High, SemanticReasonCode::ExactSource)
    } else {
        (
            Provenance::FrameworkSynthesis,
            Confidence::Medium,
            SemanticReasonCode::GeneratedFromSource,
        )
    };
    MojoBaseParentFact {
        envelope: SemanticFactEnvelope::new(
            fact_id,
            Some(entity_id),
            SemanticFactKind::Declaration,
            anchor,
            generation.clone(),
            None,
            Some(from_package.clone()),
            LifecyclePhase::Runtime,
            SemanticProducer::FrameworkAdapter,
            SemanticProvenance::Known(provenance),
            SemanticConfidence::Known(confidence),
            SemanticFreshness::Fresh,
            None,
            dependencies(file_id, generation),
            reason_code,
        ),
        edge: PackageEdge::new(
            from_package,
            parent.to_string(),
            PackageEdgeKind::Inherits,
            Some(AnchorId(u64::from(anchor_start))),
            provenance,
            confidence,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::SemanticFactStatus;
    use perl_test_must::must_some;

    fn anchor(start: u32, end: u32) -> SourceAnchor {
        SourceAnchor::new(Some(AnchorId(u64::from(start))), FileId(1), start, end)
    }

    /// Byte offset the fixture declarations start at.
    ///
    /// Past [`exact_activation`]'s import interval end, so a fixture
    /// declaration is positioned after the activating import the way real
    /// source is. Placing one before that interval is how the ordering
    /// negative control below is built.
    const AFTER_IMPORT: u32 = 40;

    fn declaration(
        index: u32,
        name: &str,
        default: MojoBaseAttributeDefault,
    ) -> MojoBaseAttributeDeclaration {
        declaration_at(AFTER_IMPORT + 10 * index, index, name, default)
    }

    fn declaration_at(
        start: u32,
        index: u32,
        name: &str,
        default: MojoBaseAttributeDefault,
    ) -> MojoBaseAttributeDeclaration {
        MojoBaseAttributeDeclaration {
            declaration_index: index,
            name_index: 0,
            file_id: FileId(1),
            package: Some("App".to_string()),
            declaration_anchor: anchor(start, start + 9),
            name_anchor: anchor(start + 4, start + 8),
            name: MojoBaseAttributeName::Literal(name.to_string()),
            default,
            explicit_method: MojoBaseExplicitMethodState::None,
            execution_phase: MojoBaseExecutionPhase::Run,
            unmodeled_options: Vec::new(),
            source_generation: SourceGeneration::known("gen-1"),
        }
    }

    fn exact_activation(outcome: MojoBaseActivationOutcome) -> MojoBaseActivationFacts {
        MojoBaseActivationFacts {
            outcome,
            profile_version: crate::framework_adapters::mojo_base::MOJO_BASE_PROFILE_VERSION,
            package: Some("App".to_string()),
            source_interval: (13, 33),
            parent_range: None,
            scope_identity: None,
            environment_identity: None,
            resolved_module: None,
            framework_version: "9.34".to_string(),
            confidence: Confidence::High,
            source_generation: SourceGeneration::known("gen-1"),
            signatures: false,
            unmodeled_options: Vec::new(),
            limitations: Vec::new(),
        }
    }

    #[test]
    fn reader_and_setter_relations_are_semantically_distinct() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let declarations = [declaration(1, "name", MojoBaseAttributeDefault::Constant)];
        let facts = mint_with_detection(&activation, &declarations);
        assert_eq!(facts.members.len(), 1);
        assert_eq!(facts.reader_results.len(), 1);
        assert_eq!(facts.setter_results.len(), 1);
        assert_eq!(facts.setter_results[0].relation, CallableResultRelation::ReceiverSelf);
        assert_eq!(
            facts.reader_results[0].relation,
            CallableResultRelation::Concrete(ValueShape::Scalar)
        );
        assert_ne!(
            facts.reader_results[0].envelope.fact_id, facts.setter_results[0].envelope.fact_id,
            "reader and setter must be two distinct facts"
        );
        assert_eq!(
            facts.reader_results[0].envelope.entity_id, facts.setter_results[0].envelope.entity_id,
            "both relations describe one accessor entity"
        );
    }

    #[test]
    fn generated_facts_never_claim_exact_source_or_fabricate_an_exit() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let declarations = [declaration(1, "name", MojoBaseAttributeDefault::Constant)];
        let facts = mint_with_detection(&activation, &declarations);
        let member = &facts.members[0];
        assert_eq!(member.member.provenance, Provenance::FrameworkSynthesis);
        assert_eq!(member.member.confidence, Confidence::Medium);
        assert_eq!(member.envelope.reason_code, SemanticReasonCode::GeneratedFromSource);
        assert_ne!(member.envelope.status(), SemanticFactStatus::Exact);
        for fact in facts.reader_results.iter().chain(facts.setter_results.iter()) {
            assert!(fact.exit_anchors().is_empty(), "no exit contributor may be fabricated");
            assert!(
                fact.limitations().contains(&CallableResultLimitation::GeneratedNoSource),
                "a generated accessor has no source body"
            );
            assert_ne!(fact.status(), SemanticFactStatus::Exact);
        }
    }

    #[test]
    fn member_anchors_the_real_has_declaration() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let declarations = [declaration(2, "host", MojoBaseAttributeDefault::Absent)];
        let facts = mint_with_detection(&activation, &declarations);
        let expected_start = AFTER_IMPORT + 20;
        assert_eq!(facts.members[0].member.source_anchor_id, AnchorId(u64::from(expected_start)));
        assert_eq!(facts.members[0].envelope.anchor.start_byte, expected_start);
    }

    #[test]
    fn base_activation_inherits_from_mojo_base_and_literal_parent_from_its_spelling() {
        let base = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let facts = mint_with_detection(&base, &[]);
        assert_eq!(facts.parents.len(), 1);
        assert_eq!(facts.parents[0].edge.to_package, "Mojo::Base");
        assert_eq!(facts.parents[0].edge.kind, PackageEdgeKind::Inherits);

        let literal = exact_activation(MojoBaseActivationOutcome::ExactLiteralParentActivation {
            parent: "Mojo::EventEmitter".to_string(),
        });
        let facts = mint_with_detection(&literal, &[]);
        assert_eq!(facts.parents[0].edge.to_package, "Mojo::EventEmitter");
        assert_eq!(facts.parents[0].edge.from_package, "App");
        assert_eq!(facts.parents[0].edge.provenance, Provenance::ExactAst);
    }

    #[test]
    fn a_computed_name_mints_no_member() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let mut dynamic = declaration(1, "ignored", MojoBaseAttributeDefault::Absent);
        dynamic.name =
            MojoBaseAttributeName::Dynamic { reason: "computed name expression".to_string() };
        let facts = mint_with_detection(&activation, &[dynamic]);
        assert!(facts.members.is_empty(), "a guessed accessor name is never minted");
        assert!(facts.reader_results.is_empty());
        assert!(facts.setter_results.is_empty());
    }

    #[test]
    fn declarations_of_another_package_are_skipped() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let mut foreign = declaration(1, "name", MojoBaseAttributeDefault::Absent);
        foreign.package = Some("Other".to_string());
        let facts = mint_with_detection(&activation, &[foreign]);
        assert!(facts.members.is_empty());
    }

    #[test]
    fn a_collision_is_recorded_without_dropping_the_live_accessor() {
        // `monkey_patch` overwrites unconditionally and runs after compile-time
        // sub installation, so the accessor is the live method and must still
        // be minted; the boundary records the shadowing for consumers.
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let mut colliding = declaration(1, "name", MojoBaseAttributeDefault::Absent);
        colliding.explicit_method = MojoBaseExplicitMethodState::Collides;
        let facts = mint_with_detection(&activation, &[colliding]);
        assert_eq!(facts.members.len(), 1, "the live accessor is still a fact");
        assert_eq!(facts.members[0].member.name, "name");
        assert_eq!(facts.setter_results.len(), 1, "its write contract is unaffected");
    }

    #[test]
    fn an_explicit_same_named_method_degrades_the_generated_member() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let mut colliding = declaration(1, "name", MojoBaseAttributeDefault::Absent);
        colliding.explicit_method = MojoBaseExplicitMethodState::Collides;
        let facts = mint_with_detection(&activation, &[colliding]);
        assert_eq!(facts.members[0].explicit_method, MojoBaseExplicitMethodState::Collides);
        let boundary = facts.members[0].envelope.boundary.as_ref();
        assert!(boundary.is_some(), "the collision must remain visible as conflict evidence");
    }

    #[test]
    fn a_declaration_must_match_the_activation_on_file_generation_and_order() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);

        // A compile-phase declaration above the activating import: it runs
        // during compilation, before the import has installed `has`.
        let mut before = declaration_at(0, 0, "early", MojoBaseAttributeDefault::Absent);
        before.execution_phase = MojoBaseExecutionPhase::Compile;
        assert!(mint_with_detection(&activation, &[before]).members.is_empty());

        // The same position at run phase does mint: `use` completes during
        // compilation, so an ordinary statement reaches the imported `has`
        // whether it is written above or below the import. This pairs with the
        // refusal above so the guard is pinned against widening as well as
        // narrowing.
        let before_at_run = declaration_at(0, 0, "early", MojoBaseAttributeDefault::Absent);
        assert_eq!(
            before_at_run.execution_phase,
            MojoBaseExecutionPhase::Run,
            "fixture default is run phase"
        );
        assert_eq!(mint_with_detection(&activation, &[before_at_run]).members.len(), 1);

        // Another file, same package name.
        let mut foreign_file = declaration(1, "elsewhere", MojoBaseAttributeDefault::Absent);
        foreign_file.file_id = FileId(2);
        assert!(mint_with_detection(&activation, &[foreign_file]).members.is_empty());

        // An older parse's carrier.
        let mut stale = declaration(1, "outdated", MojoBaseAttributeDefault::Absent);
        stale.source_generation = SourceGeneration::known("gen-0");
        assert!(mint_with_detection(&activation, &[stale]).members.is_empty());

        // Control: a declaration matching on all counts still mints, so the
        // refusals above isolate each guard rather than a broken fixture.
        let ok = declaration(1, "kept", MojoBaseAttributeDefault::Absent);
        assert_eq!(mint_with_detection(&activation, &[ok]).members.len(), 1);
    }

    #[test]
    fn parent_identity_never_collides_with_a_member_family_identity() {
        // The parent fact passes the import's start byte through the slot the
        // member families use for a declaration index, so only the parent
        // domain separator keeps the two apart. Sweep the byte offsets and
        // declaration indices of a realistic file against each other: without
        // the separator the diagonal (import_start_byte == declaration_index)
        // would collide on every row.
        let generation = SourceGeneration::known("gen-1");
        for import_start_byte in 0..64_u32 {
            let (parent_fact, parent_entity) =
                mojo_base_parent_identity(FileId(1), import_start_byte, &generation);
            for declaration_index in 0..64_u32 {
                for name_index in 0..4_u32 {
                    for (fact, entity) in [
                        mojo_base_member_identity(
                            FileId(1),
                            declaration_index,
                            name_index,
                            &generation,
                        ),
                        mojo_base_reader_identity(
                            FileId(1),
                            declaration_index,
                            name_index,
                            &generation,
                        ),
                        mojo_base_setter_identity(
                            FileId(1),
                            declaration_index,
                            name_index,
                            &generation,
                        ),
                    ] {
                        assert_ne!(
                            parent_fact, fact,
                            "parent fact id collided with a member-family fact id at \
                             import_start_byte={import_start_byte} \
                             declaration_index={declaration_index} name_index={name_index}"
                        );
                        assert_ne!(
                            parent_entity, entity,
                            "parent entity collided with a member-family entity at \
                             import_start_byte={import_start_byte} \
                             declaration_index={declaration_index} name_index={name_index}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn member_reader_and_setter_identities_are_mutually_disjoint() {
        let generation = SourceGeneration::known("gen-1");
        let member = mojo_base_member_identity(FileId(1), 3, 1, &generation);
        let reader = mojo_base_reader_identity(FileId(1), 3, 1, &generation);
        let setter = mojo_base_setter_identity(FileId(1), 3, 1, &generation);
        assert_ne!(member.0, reader.0);
        assert_ne!(member.0, setter.0);
        assert_ne!(reader.0, setter.0);
        assert_eq!(
            (member.1, reader.1),
            (setter.1, setter.1),
            "all three facts describe one accessor entity"
        );
    }

    #[test]
    fn only_a_genuinely_unsupported_default_carries_an_unsupported_boundary() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let determinate = [
            MojoBaseAttributeDefault::Constant,
            MojoBaseAttributeDefault::LazyBuilder,
            MojoBaseAttributeDefault::Absent,
        ];
        for default in determinate {
            let facts = mint_with_detection(&activation, &[declaration(1, "name", default)]);
            let boundary = must_some(facts.reader_results[0].envelope.boundary.as_ref());
            assert_eq!(
                boundary.kind,
                BoundaryKind::DynamicValue,
                "an admitted default limits the reader dynamically, not as unsupported"
            );
            assert!(
                facts.setter_results[0].envelope.boundary.is_none(),
                "the write contract is determinate and carries no boundary"
            );
        }

        let unsupported = MojoBaseAttributeDefault::Unsupported { reason: "ref".to_string() };
        let facts = mint_with_detection(&activation, &[declaration(1, "name", unsupported)]);
        let boundary = must_some(facts.reader_results[0].envelope.boundary.as_ref());
        assert_eq!(boundary.kind, BoundaryKind::Unsupported);
    }

    #[test]
    fn identities_are_deterministic_and_generation_scoped() {
        let first = mojo_base_member_identity(FileId(1), 3, 0, &SourceGeneration::known("gen-1"));
        let repeat = mojo_base_member_identity(FileId(1), 3, 0, &SourceGeneration::known("gen-1"));
        let newer = mojo_base_member_identity(FileId(1), 3, 0, &SourceGeneration::known("gen-2"));
        let sibling = mojo_base_member_identity(FileId(1), 3, 1, &SourceGeneration::known("gen-1"));
        assert_eq!(first, repeat, "identity must be deterministic");
        assert_ne!(first, newer, "a new generation must mint a new identity");
        assert_ne!(first, sibling, "names of one statement must not collide");
    }

    /// Run the real Mojo::Base detection path, then mint over its result, so
    /// the gating in these tests is the production gate rather than a stub.
    fn mint_with_detection(
        activation: &MojoBaseActivationFacts,
        declarations: &[MojoBaseAttributeDeclaration],
    ) -> MojoBaseObjectFacts {
        mojo_base_object_facts(
            &detected_result("9.34", "gen-1"),
            activation,
            FileId(1),
            activation.package.as_deref(),
            declarations,
        )
    }

    fn detected_result(version: &str, generation: &str) -> AdapterDetectionResult {
        use crate::framework::{
            AdapterCancellation, AdapterDetectionInput, DetectionEvidenceClass,
            ModuleActivationIdentity, ModuleObservationReceipt, ModuleSelectorEvaluation,
            ModuleSelectorOutcome, ModuleVersionEvidence,
        };
        use crate::framework_adapters::mojo_base::{detect_mojo_base, mojo_base_descriptor};

        let activation = ModuleActivationIdentity::new(
            MOJO_BASE_FRAMEWORK_NAME,
            Some(FileId(7)),
            SourceGeneration::known(generation),
        )
        .with_observed_version(ModuleVersionEvidence::new(
            version,
            SourceGeneration::known(generation),
        ));
        let evaluation = ModuleSelectorEvaluation::new(
            MOJO_BASE_FRAMEWORK_NAME,
            ModuleSelectorOutcome::Matched {
                activation,
                evidence_class: DetectionEvidenceClass::ResolvedModule,
            },
        );
        detect_mojo_base(&AdapterDetectionInput::new(
            mojo_base_descriptor(),
            ModuleObservationReceipt::new(
                "module-resolver.v1",
                "root:fixture",
                "project-environment.v1",
                SourceGeneration::known(generation),
                "sha256:fixture-input",
                vec![evaluation],
            ),
            None,
            AdapterCancellation::active(),
        ))
    }

    #[test]
    fn an_undetected_framework_mints_nothing() {
        use crate::framework::{
            AdapterCancellation, AdapterDetectionInput, ModuleObservationReceipt,
        };
        use crate::framework_adapters::mojo_base::{detect_mojo_base, mojo_base_descriptor};

        // No module evaluation at all: the framework was never established, so
        // an otherwise exact activation profile still mints no object fact.
        let undetected = detect_mojo_base(&AdapterDetectionInput::new(
            mojo_base_descriptor(),
            ModuleObservationReceipt::new(
                "module-resolver.v1",
                "root:fixture",
                "project-environment.v1",
                SourceGeneration::known("gen-1"),
                "sha256:fixture-input",
                Vec::new(),
            ),
            None,
            AdapterCancellation::active(),
        ));
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let declarations = [declaration(1, "name", MojoBaseAttributeDefault::Constant)];
        let facts =
            mojo_base_object_facts(&undetected, &activation, FileId(1), Some("App"), &declarations);
        assert!(facts.is_empty(), "detection must gate every object fact");
    }

    #[test]
    fn a_non_exact_activation_mints_nothing() {
        // The strongest negative control of #9682: `has` in a package whose
        // activation is not exact is never accessor evidence.
        let inexact = exact_activation(MojoBaseActivationOutcome::DynamicOrUnmodeledParent {
            reason: "computed parent expression".to_string(),
        });
        let declarations = [declaration(1, "name", MojoBaseAttributeDefault::Constant)];
        let facts = mint_with_detection(&inexact, &declarations);
        assert!(facts.is_empty(), "a same-named `has` without exact activation emits no facts");
    }
}
